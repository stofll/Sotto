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

use whisper_desktop_lib::ai::providers::{
    AnthropicProvider, OpenAIProvider, Provider, ProviderErrorType,
};
use whisper_desktop_lib::ai::step::{ai_process_text_with_status, AiConfig};

/// Per-test type alias — the captured-request log is an
/// `Arc<Mutex<Vec<HashMap<...>>>>`, too noisy to repeat in every
/// function signature.
type CapturedRequests = Arc<Mutex<Vec<HashMap<String, String>>>>;

/// Start a one-shot HTTP server on a random port. Returns the URL and
/// a `requests` Arc that records every captured request body.
fn mock_server(status_line: &str, response_body: &str) -> (String, CapturedRequests) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let requests: CapturedRequests = Arc::new(Mutex::new(Vec::new()));
    let requests_for_thread = Arc::clone(&requests);
    let status = status_line.to_string();
    let body = response_body.to_string();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
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
            "{status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(response.as_bytes()).unwrap();
        stream.flush().ok();
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
    };
    let outcome = block_on(ai_process_text_with_status("hello", &config, None));
    assert_eq!(outcome.status.skipped_reason, "missing_api_key");
    assert!(!outcome.status.attempted);
    assert_eq!(captured.lock().unwrap().len(), 0);
}
