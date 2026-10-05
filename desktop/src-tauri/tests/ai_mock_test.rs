//! Mock HTTP tests for the AI provider subsystem.
//!
//! These tests stand up a local `TcpListener`, replay canned HTTP
//! responses, and verify the provider request shape, the typed
//! error classification, and the retry/usage extraction logic.
//! They are NOT marked `#[ignore]` because they don't hit the
//! public network — only the loopback interface.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::Arc;
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use sotto_lib::ai::model_params::{OutputLimit, ReasoningMode};
use sotto_lib::ai::providers::{AnthropicProvider, OpenAIProvider, Provider, ProviderErrorType};
use sotto_lib::ai::step::{ai_process_text_with_status, AiConfig, LateAnswerMode};

/// Per-test type alias — the captured-request log is an
/// `Arc<Mutex<Vec<HashMap<...>>>>`, too noisy to repeat in every
/// function signature.
type CapturedRequests = Arc<Mutex<Vec<HashMap<String, String>>>>;

/// Start a one-shot HTTP server on a random port. Returns the URL and
/// a `requests` Arc that records every captured request body.
fn mock_server(status_line: &str, response_body: &str) -> (String, CapturedRequests) {
    mock_server_sequence(&[(status_line, response_body)])
}

/// Serve `responses` in order, one per connection, recording each request.
fn mock_server_sequence(responses: &[(&str, &str)]) -> (String, CapturedRequests) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let requests: CapturedRequests = Arc::new(Mutex::new(Vec::new()));
    let requests_for_thread = Arc::clone(&requests);
    let responses: Vec<(String, String)> = responses
        .iter()
        .map(|(status, body)| (status.to_string(), body.to_string()))
        .collect();
    thread::spawn(move || {
        for (status, body) in responses {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let mut captured = HashMap::new();
            let mut buffer = Vec::new();
            let mut temp = [0_u8; 4096];
            // Read headers + body. The body length is `Content-Length`.
            loop {
                let read = stream.read(&mut temp).unwrap_or(0);
                if read == 0 {
                    break;
                }
                buffer.extend_from_slice(&temp[..read]);
                if let Some(headers_end) = find_header_end(&buffer) {
                    let body_so_far = buffer[headers_end..].len();
                    let expected = parse_content_length(&buffer[..headers_end]).unwrap_or(0);
                    if body_so_far >= expected {
                        break;
                    }
                }
            }
            // Capture headers + body for the test to inspect.
            if let Some(headers_end) = find_header_end(&buffer) {
                let headers = String::from_utf8_lossy(&buffer[..headers_end]).into_owned();
                let request_body = String::from_utf8_lossy(&buffer[headers_end..]).into_owned();
                captured.insert("headers".to_string(), headers);
                captured.insert("body".to_string(), request_body);
            }
            requests_for_thread.lock().unwrap().push(captured);
            // Send canned response.
            let response = format!(
                "{status}
Content-Type: application/json
Content-Length: {}
Connection: close

{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).unwrap();
            stream.flush().ok();
        }
    });
    (format!("http://{addr}"), requests)
}

fn find_header_end(buffer: &[u8]) -> Option<usize> {
    buffer
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|i| i + 4)
}

fn parse_content_length(headers: &[u8]) -> Option<usize> {
    let text = std::str::from_utf8(headers).ok()?;
    for line in text.lines() {
        if let Some(value) = line.strip_prefix("Content-Length:") {
            return value.trim().parse().ok();
        }
        if let Some(value) = line.strip_prefix("content-length:") {
            return value.trim().parse().ok();
        }
    }
    None
}

fn block_on<T>(future: impl std::future::Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(future)
}

#[test]
fn anthropic_provider_sends_correct_request_shape() {
    let (url, captured) = mock_server(
        "HTTP/1.1 200 OK",
        r#"{"content":[{"text":"Hello, world."}],"usage":{"input_tokens":12,"output_tokens":7}}"#,
    );
    let provider = AnthropicProvider::new(
        "sk-test-key",
        "claude-haiku-4-5",
        Some(url),
        Some(Duration::from_secs(5)),
        None,
    );
    let (text, info) = block_on(provider.complete("system prompt", "user message"))
        .expect("mock server returned 200");
    assert_eq!(text, "Hello, world.");
    let usage = info.usage.expect("usage");
    assert_eq!(usage.input_tokens, 12);
    assert_eq!(usage.output_tokens, 7);
    let requests = captured.lock().unwrap();
    assert_eq!(requests.len(), 1);
    let body = &requests[0]["body"];
    assert!(body.contains("\"model\":\"claude-haiku-4-5\""));
    assert!(body.contains("\"system\":\"system prompt\""));
    assert!(body.contains("\"content\":\"user message\""));
    let headers = &requests[0]["headers"];
    assert!(headers.contains("x-api-key: sk-test-key"));
    assert!(headers.contains("anthropic-version: 2023-06-01"));
}

#[test]
fn openai_provider_extracts_choices_content() {
    let (url, _) = mock_server(
        "HTTP/1.1 200 OK",
        r#"{"choices":[{"message":{"content":"Polished reply"}}],"usage":{"prompt_tokens":4,"completion_tokens":2,"total_tokens":6}}"#,
    );
    let provider = OpenAIProvider::new(
        "sk-test",
        "gpt-4o-mini",
        Some(url),
        Some(Duration::from_secs(5)),
        None,
    );
    let (text, info) =
        block_on(provider.complete("system", "user")).expect("mock server returned 200");
    assert_eq!(text, "Polished reply");
    let usage = info.usage.expect("usage");
    assert_eq!(usage.input_tokens, 4);
    assert_eq!(usage.output_tokens, 2);
    assert_eq!(usage.total_tokens, 6);
}

#[test]
fn http_401_is_classified_as_auth_error() {
    let (url, _) = mock_server(
        "HTTP/1.1 401 Unauthorized",
        r#"{"error":{"message":"invalid api key"}}"#,
    );
    let provider = OpenAIProvider::new(
        "sk-bad",
        "gpt-4o-mini",
        Some(url),
        Some(Duration::from_secs(5)),
        None,
    );
    let result = block_on(provider.complete("system", "user"));
    let error = result.expect_err("expected an auth error");
    assert_eq!(error.kind, ProviderErrorType::AuthError);
    assert!(!error.retryable);
    assert_eq!(error.http_status, Some(401));
}

#[test]
fn http_429_is_classified_as_rate_limit() {
    let (url, _) = mock_server(
        "HTTP/1.1 429 Too Many Requests",
        r#"{"error":{"message":"rate limited"}}"#,
    );
    let provider = OpenAIProvider::new(
        "sk-test",
        "gpt-4o-mini",
        Some(url),
        Some(Duration::from_secs(5)),
        None,
    );
    let result = block_on(provider.complete("system", "user"));
    let error = result.expect_err("expected a rate-limit error");
    assert_eq!(error.kind, ProviderErrorType::RateLimit);
    assert!(!error.retryable);
}

#[test]
fn malformed_json_response_is_bad_response_error() {
    let (url, _) = mock_server("HTTP/1.1 200 OK", "not json");
    let provider = OpenAIProvider::new(
        "sk-test",
        "gpt-4o-mini",
        Some(url),
        Some(Duration::from_secs(5)),
        None,
    );
    let result = block_on(provider.complete("system", "user"));
    let error = result.expect_err("expected a bad-response error");
    assert_eq!(error.kind, ProviderErrorType::BadResponse);
    assert!(error.retryable);
}

#[test]
fn missing_choices_field_is_bad_response_error() {
    let (url, _) = mock_server("HTTP/1.1 200 OK", r#"{"unrelated":true}"#);
    let provider = OpenAIProvider::new(
        "sk-test",
        "gpt-4o-mini",
        Some(url),
        Some(Duration::from_secs(5)),
        None,
    );
    let result = block_on(provider.complete("system", "user"));
    let error = result.expect_err("expected a bad-response error");
    assert_eq!(error.kind, ProviderErrorType::BadResponse);
}

#[test]
fn local_mode_does_not_call_provider() {
    // The mock server would panic on a request; if the orchestrator
    // touches it, the test fails.
    let (url, captured) = mock_server("HTTP/1.1 200 OK", "{}");
    let config = AiConfig {
        pipeline_mode: "local".to_string(),
        provider: "openai".to_string(),
        model: "gpt-4o-mini".to_string(),
        profile_id: "default".to_string(),
        profile_name: "Default".to_string(),
        api_key_ref: "openai".to_string(),
        system_prompt: "system".to_string(),
        language: "ru".to_string(),
        base_url: Some(url),
        audio_duration_seconds: Some(45.0),
        llm_min_duration_seconds: 0.0,
        llm_timeout_seconds: 12,
        late_answer: LateAnswerMode::Off,
        reasoning: ReasoningMode::Minimal,
        output_limit: OutputLimit::Auto,
        waits_to_paste: true,
    };
    let outcome = block_on(ai_process_text_with_status(
        "hello",
        &config,
        Some("sk-test"),
    ));
    assert_eq!(outcome.status.skipped_reason, "local_mode");
    // No HTTP request should have been made.
    assert_eq!(captured.lock().unwrap().len(), 0);
}

#[test]
fn missing_api_key_short_circuits_before_http() {
    let (url, captured) = mock_server("HTTP/1.1 200 OK", "{}");
    let config = AiConfig {
        pipeline_mode: "hybrid".to_string(),
        provider: "openai".to_string(),
        model: "gpt-4o-mini".to_string(),
        profile_id: "default".to_string(),
        profile_name: "Default".to_string(),
        api_key_ref: "openai".to_string(),
        system_prompt: "system".to_string(),
        language: "ru".to_string(),
        base_url: Some(url),
        audio_duration_seconds: Some(45.0),
        llm_min_duration_seconds: 0.0,
        llm_timeout_seconds: 12,
        late_answer: LateAnswerMode::Off,
        reasoning: ReasoningMode::Minimal,
        output_limit: OutputLimit::Auto,
        waits_to_paste: true,
    };
    let outcome = block_on(ai_process_text_with_status("hello", &config, None));
    assert_eq!(outcome.status.skipped_reason, "missing_api_key");
    assert!(!outcome.status.attempted);
    assert_eq!(captured.lock().unwrap().len(), 0);
}

/// Anthropic's key travels in `x-api-key`, which reqwest forwards on a
/// redirect to another host or port; the client must not follow one.
#[test]
fn provider_keys_are_not_forwarded_to_another_server() {
    // `localhost` is another host for the policy; the mock servers listen on
    // 127.0.0.1, so the second case is the same host on another port.
    for host in ["localhost", "127.0.0.1"] {
        let (other_server, other_requests) = mock_server(
            "HTTP/1.1 200 OK",
            r#"{"content":[{"text":"stolen"}],"usage":{"input_tokens":1,"output_tokens":1}}"#,
        );
        let other_port = other_server.rsplit(':').next().unwrap();
        let (url, _) = mock_server(
            &format!(
                "HTTP/1.1 307 Temporary Redirect\r\nLocation: http://{host}:{other_port}/messages"
            ),
            "",
        );
        let provider = AnthropicProvider::new(
            "sk-test-key",
            "claude-haiku-4-5",
            Some(url),
            Some(Duration::from_secs(5)),
            None,
        );

        let error =
            block_on(provider.complete("system", "user")).expect_err("redirect must not succeed");

        assert_eq!(error.http_status, Some(307), "{host}");
        assert_eq!(
            other_requests.lock().unwrap().len(),
            0,
            "the key reached {host}:{other_port}"
        );
    }
}

fn request_json(requests: &CapturedRequests, index: usize) -> serde_json::Value {
    serde_json::from_str(&requests.lock().unwrap()[index]["body"]).unwrap()
}

const OPENAI_OK: &str = r#"{"choices":[{"message":{"content":"Tidied"}}]}"#;

/// OpenAI's own API takes the limit as `max_completion_tokens`; its reasoning
/// models reject the deprecated `max_tokens`.
#[test]
fn openai_api_sends_max_completion_tokens() {
    let (url, requests) = mock_server("HTTP/1.1 200 OK", OPENAI_OK);
    let provider = OpenAIProvider::new(
        "sk-test",
        "gpt-4o-mini",
        Some(url),
        Some(Duration::from_secs(5)),
        None,
    )
    .for_openai_api();
    block_on(provider.complete("system", "user")).expect("mock server returned 200");
    let body = request_json(&requests, 0);
    assert!(body["max_completion_tokens"].as_u64().is_some());
    assert!(body.get("max_tokens").is_none());
    assert_eq!(body["temperature"].as_f64(), Some(0.2_f32 as f64));
}

#[test]
fn openai_reasoning_model_gets_no_temperature() {
    let (url, requests) = mock_server("HTTP/1.1 200 OK", OPENAI_OK);
    let provider = OpenAIProvider::new(
        "sk-test",
        "o4-mini",
        Some(url),
        Some(Duration::from_secs(5)),
        None,
    )
    .for_openai_api();
    block_on(provider.complete("system", "user")).expect("mock server returned 200");
    let body = request_json(&requests, 0);
    assert!(body.get("temperature").is_none());
    assert!(body["max_completion_tokens"].as_u64().is_some());
}

/// A compatible server keeps the field every such server understands.
#[test]
fn compatible_server_keeps_max_tokens_and_temperature() {
    let (url, requests) = mock_server("HTTP/1.1 200 OK", OPENAI_OK);
    let provider = OpenAIProvider::new(
        "sk-test",
        "o4-mini",
        Some(url),
        Some(Duration::from_secs(5)),
        None,
    );
    block_on(provider.complete("system", "user")).expect("mock server returned 200");
    let body = request_json(&requests, 0);
    assert!(body["max_tokens"].as_u64().is_some());
    assert!(body.get("max_completion_tokens").is_none());
    assert!(body["temperature"].as_f64().is_some());
}

/// A model the name check misses still works: the 400 that names
/// `temperature` is answered by one request without it.
#[test]
fn a_temperature_refusal_is_retried_without_temperature() {
    let (url, requests) = mock_server_sequence(&[
        (
            "HTTP/1.1 400 Bad Request",
            r#"{"error":{"message":"Unsupported value: 'temperature' does not support 0.2 with this model. Only the default (1) value is supported.","type":"invalid_request_error","param":"temperature","code":"unsupported_value"}}"#,
        ),
        ("HTTP/1.1 200 OK", OPENAI_OK),
    ]);
    let provider = OpenAIProvider::new(
        "sk-test",
        "future-model",
        Some(url),
        Some(Duration::from_secs(5)),
        None,
    );
    let (text, _) = block_on(provider.complete("system", "user")).expect("retry succeeds");
    assert_eq!(text, "Tidied");
    assert_eq!(requests.lock().unwrap().len(), 2);
    assert!(request_json(&requests, 0)["temperature"].as_f64().is_some());
    assert!(request_json(&requests, 1).get("temperature").is_none());
}

#[test]
fn another_400_is_not_retried() {
    let (url, requests) = mock_server_sequence(&[
        (
            "HTTP/1.1 400 Bad Request",
            r#"{"error":{"message":"Invalid model"}}"#,
        ),
        ("HTTP/1.1 200 OK", OPENAI_OK),
    ]);
    let provider = OpenAIProvider::new(
        "sk-test",
        "gpt-4o-mini",
        Some(url),
        Some(Duration::from_secs(5)),
        None,
    );
    let error = block_on(provider.complete("system", "user")).expect_err("400 stays an error");
    assert_eq!(error.http_status, Some(400));
    assert_eq!(requests.lock().unwrap().len(), 1);
}

/// The OpenAI profile reaches the provider in OpenAI's own dialect, and a
/// compatible profile in the shared one.
#[test]
fn profile_kind_chooses_the_request_dialect() {
    for (provider, field) in [
        ("openai", "max_completion_tokens"),
        ("compatible", "max_tokens"),
    ] {
        let (url, requests) = mock_server("HTTP/1.1 200 OK", OPENAI_OK);
        let config = AiConfig {
            pipeline_mode: "hybrid".to_string(),
            provider: provider.to_string(),
            model: "gpt-5-mini".to_string(),
            profile_id: "default".to_string(),
            profile_name: "Default".to_string(),
            api_key_ref: provider.to_string(),
            system_prompt: "system".to_string(),
            language: "ru".to_string(),
            base_url: Some(url),
            audio_duration_seconds: Some(45.0),
            llm_min_duration_seconds: 0.0,
            llm_timeout_seconds: 12,
            late_answer: LateAnswerMode::Off,
            reasoning: ReasoningMode::Minimal,
            output_limit: OutputLimit::Auto,
            waits_to_paste: true,
        };
        let outcome = block_on(ai_process_text_with_status(
            "hello",
            &config,
            Some("sk-test"),
        ));
        assert!(
            outcome.status.used,
            "{provider}: {:?}",
            outcome.status.skipped_reason
        );
        let body = request_json(&requests, 0);
        assert!(
            body[field].as_u64().is_some(),
            "{provider} must send {field}"
        );
    }
}

/// The lowest effort differs between OpenAI models: a refused value steps
/// down to the next one rather than failing the dictation.
#[test]
fn openai_reasoning_effort_steps_down_when_refused() {
    let (url, requests) = mock_server_sequence(&[
        (
            "HTTP/1.1 400 Bad Request",
            r#"{"error":{"message":"Unsupported value: 'reasoning_effort' does not support 'none' with this model.","param":"reasoning_effort"}}"#,
        ),
        ("HTTP/1.1 200 OK", OPENAI_OK),
    ]);
    let provider = OpenAIProvider::new(
        "sk-test",
        "o9-mock-stepping",
        Some(url),
        Some(Duration::from_secs(5)),
        None,
    )
    .for_openai_api();
    block_on(provider.complete("system", "user")).expect("second value accepted");
    assert_eq!(request_json(&requests, 0)["reasoning_effort"], "none");
    assert_eq!(request_json(&requests, 1)["reasoning_effort"], "minimal");
}

#[test]
fn reasoning_left_to_the_model_sends_no_reasoning_field() {
    let (url, requests) = mock_server("HTTP/1.1 200 OK", OPENAI_OK);
    let provider = OpenAIProvider::new(
        "sk-test",
        "o9-mock-model",
        Some(url),
        Some(Duration::from_secs(5)),
        None,
    )
    .for_openai_api()
    .with_options(ReasoningMode::Model, OutputLimit::Unlimited);
    block_on(provider.complete("system", "user")).expect("mock server returned 200");
    let body = request_json(&requests, 0);
    assert!(body.get("reasoning_effort").is_none());
    // Unlimited leaves the cap out where the API allows it.
    assert!(body.get("max_completion_tokens").is_none());
}

#[test]
fn anthropic_is_told_not_to_think_unless_left_to_the_model() {
    const ANTHROPIC_OK: &str =
        r#"{"content":[{"text":"Tidied"}],"usage":{"input_tokens":1,"output_tokens":1}}"#;
    for (mode, expect_disabled) in [
        (ReasoningMode::Minimal, true),
        (ReasoningMode::Model, false),
    ] {
        let (url, requests) = mock_server("HTTP/1.1 200 OK", ANTHROPIC_OK);
        let provider = AnthropicProvider::new(
            "sk-test",
            "claude-mock",
            Some(url),
            Some(Duration::from_secs(5)),
            None,
        )
        .with_options(mode, OutputLimit::Tokens(3000));
        block_on(provider.complete("system", "user")).expect("mock server returned 200");
        let body = request_json(&requests, 0);
        assert_eq!(body["max_tokens"], 3000);
        assert_eq!(
            body.get("thinking") == Some(&serde_json::json!({"type": "disabled"})),
            expect_disabled,
            "{mode:?}"
        );
    }
}
