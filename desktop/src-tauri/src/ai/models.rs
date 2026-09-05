//! Model lists that the provider serves itself.
//!
//! Before this module the model id was typed by hand and the UI hint sent you
//! off to read the documentation. Keeping a current list in the code is not an
//! option: providers rename and retire models more often than the app ships
//! releases, and a stale id silently 404s in the middle of a dictation — that
//! is, exactly when you least want to investigate.
//!
//! The request goes only to the selected provider and only on an explicit user
//! action: pulling the list is a network request made with their key.
//!
//! All five providers have such an endpoint — verified with a keyless request
//! against each: 200 for the public ones, 401/403 for the rest, 404 from none.
//! There are two differences, and both are handled here: Anthropic wants
//! `x-api-key` instead of `Bearer`, and Gemini uses a different path and a
//! different response shape.

use std::time::Duration;

use serde_json::Value;

use super::providers::{build_request, ANTHROPIC_BASE_URL, OPENAI_BASE_URL, OPENCODE_GO_BASE_URL};

/// A separate timeout, shorter than for an ordinary LLM request: the model list
/// is pulled from the settings screen while watching a spinner, and half a
/// minute of waiting there reads as "hung", not as "in progress".
const FETCH_TIMEOUT_SECS: u64 = 10;

/// Where to knock for the list and what to authorise with.
struct Endpoint {
    url: String,
    headers: Vec<(String, String)>,
}

fn endpoint(provider: &str, base_url: Option<&str>, api_key: &str) -> Result<Endpoint, String> {
    let trimmed = base_url
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.trim_end_matches('/').to_string());
    let bearer = || vec![("Authorization".to_string(), format!("Bearer {api_key}"))];

    match provider {
        "gemini" => Ok(Endpoint {
            // Not OpenAI-compatible: its own path, its own response shape
            // (see `parse_models`) and the key in a separate header.
            url: "https://generativelanguage.googleapis.com/v1beta/models".to_string(),
            headers: vec![("x-goog-api-key".to_string(), api_key.to_string())],
        }),
        "anthropic" => Ok(Endpoint {
            url: format!(
                "{}/models",
                trimmed.unwrap_or_else(|| ANTHROPIC_BASE_URL.to_string())
            ),
            headers: vec![
                ("x-api-key".to_string(), api_key.to_string()),
                ("anthropic-version".to_string(), "2023-06-01".to_string()),
            ],
        }),
        "openai" => Ok(Endpoint {
            url: format!(
                "{}/models",
                trimmed.unwrap_or_else(|| OPENAI_BASE_URL.to_string())
            ),
            headers: bearer(),
        }),
        "opencode-go" => Ok(Endpoint {
            url: format!(
                "{}/models",
                trimmed.unwrap_or_else(|| OPENCODE_GO_BASE_URL.to_string())
            ),
            headers: bearer(),
        }),
        // An OpenAI-compatible provider is defined by its own base_url, and
        // without it there is nowhere to go — substituting somebody else's
        // address here is not allowed.
        "compatible" => match trimmed {
            Some(base) => Ok(Endpoint {
                url: format!("{base}/models"),
                headers: bearer(),
            }),
            None => Err("не задан base_url".to_string()),
        },
        other => Err(format!("неизвестный провайдер: {other}")),
    }
}

/// Parse the response into a list of model ids.
///
/// Three shapes, all of them seen among the connected providers: the
/// OpenAI-compatible `{"data": [{"id": …}]}`, a bare array of strings, and
/// Gemini `{"models": [{"name": "models/…"}]}`.
fn parse_models(parsed: &Value) -> Vec<String> {
    let from_entry = |entry: &Value| -> Option<String> {
        if let Some(id) = entry.get("id").and_then(Value::as_str) {
            return Some(id.to_string());
        }
        if let Some(name) = entry.get("name").and_then(Value::as_str) {
            // Gemini returns the full resource path; the request needs the tail.
            return Some(name.trim_start_matches("models/").to_string());
        }
        entry.as_str().map(str::to_string)
    };

    let list = parsed
        .get("data")
        .or_else(|| parsed.get("models"))
        .and_then(Value::as_array)
        .or_else(|| parsed.as_array());

    let mut models: Vec<String> = match list {
        Some(items) => items
            .iter()
            .filter_map(from_entry)
            .filter(|id| !id.trim().is_empty())
            .collect(),
        None => Vec::new(),
    };
    models.sort_unstable();
    models.dedup();
    models
}

/// Ask the provider which models it is willing to serve.
///
/// The error is returned as text to be shown next to the field: "the list did
/// not load" must not look like "the key is wrong", which is why the caller does
/// not turn it into a modal toast.
pub async fn fetch_models(
    provider: &str,
    base_url: Option<&str>,
    api_key: &str,
) -> Result<Vec<String>, String> {
    let endpoint = endpoint(provider, base_url, api_key)?;
    let headers: Vec<(&str, &str)> = endpoint
        .headers
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    let request = build_request(reqwest::Method::GET, &endpoint.url, None, &headers);

    let response = request
        .timeout(Duration::from_secs(FETCH_TIMEOUT_SECS))
        .send()
        .await
        .map_err(|e| format!("запрос не прошёл: {e}"))?;

    let status = response.status();
    if !status.is_success() {
        // The response body is not repeated in full: for some providers it
        // echoes the request along with its headers.
        return Err(match status.as_u16() {
            401 | 403 => "ключ не подошёл".to_string(),
            404 => "провайдер не отдаёт список моделей".to_string(),
            code => format!("провайдер ответил {code}"),
        });
    }

    let parsed: Value = response
        .json()
        .await
        .map_err(|e| format!("ответ не разобрался: {e}"))?;
    let models = parse_models(&parsed);
    if models.is_empty() {
        return Err("провайдер вернул пустой список".to_string());
    }
    Ok(models)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openai_shape() {
        let body = serde_json::json!({
            "object": "list",
            "data": [{"id": "gpt-4o-mini"}, {"id": "gpt-4o"}],
        });
        assert_eq!(parse_models(&body), vec!["gpt-4o", "gpt-4o-mini"]);
    }

    #[test]
    fn bare_array_of_strings() {
        let body = serde_json::json!(["b-model", "a-model"]);
        assert_eq!(parse_models(&body), vec!["a-model", "b-model"]);
    }

    /// Gemini serves the full resource path, while the request needs the tail.
    #[test]
    fn gemini_shape_loses_the_resource_prefix() {
        let body = serde_json::json!({
            "models": [{"name": "models/gemini-2.5-flash"}, {"name": "models/gemini-2.5-pro"}],
        });
        assert_eq!(
            parse_models(&body),
            vec!["gemini-2.5-flash", "gemini-2.5-pro"]
        );
    }

    /// Some OpenAI-compatible providers describe an entry with `name`, not `id`.
    #[test]
    fn falls_back_from_id_to_name() {
        let body = serde_json::json!({ "data": [{"name": "some-model"}] });
        assert_eq!(parse_models(&body), vec!["some-model"]);
    }

    #[test]
    fn unknown_shape_yields_nothing_rather_than_garbage() {
        assert!(parse_models(&serde_json::json!({"error": "nope"})).is_empty());
        assert!(parse_models(&serde_json::json!("just a string")).is_empty());
    }

    #[test]
    fn duplicates_and_blanks_are_dropped() {
        let body = serde_json::json!({ "data": [{"id": "a"}, {"id": "a"}, {"id": "  "}] });
        assert_eq!(parse_models(&body), vec!["a"]);
    }

    #[test]
    fn anthropic_authenticates_with_its_own_header() {
        let endpoint = endpoint("anthropic", None, "secret").unwrap();
        assert_eq!(endpoint.url, "https://api.anthropic.com/v1/models");
        // Bearer would silently return 401 here — the provider does not
        // understand it.
        assert!(endpoint
            .headers
            .iter()
            .any(|(k, v)| k == "x-api-key" && v == "secret"));
        assert!(endpoint
            .headers
            .iter()
            .any(|(k, _)| k == "anthropic-version"));
    }

    #[test]
    fn gemini_ignores_base_url_and_uses_its_own_path() {
        let endpoint = endpoint("gemini", Some("https://example.test/v1"), "secret").unwrap();
        assert!(endpoint.url.ends_with("/v1beta/models"));
        assert!(endpoint
            .headers
            .iter()
            .any(|(k, v)| k == "x-goog-api-key" && v == "secret"));
    }

    #[test]
    fn a_custom_base_url_wins_over_the_default() {
        let endpoint = endpoint("openai", Some("https://proxy.test/v1/"), "secret").unwrap();
        assert_eq!(endpoint.url, "https://proxy.test/v1/models");
    }

    /// Local presets (Ollama, LM Studio, vLLM) arrive as `compatible` with their
    /// own address — for them this is the most useful function of all, because
    /// their model list changes most often.
    #[test]
    fn compatible_needs_a_base_url() {
        assert!(endpoint("compatible", None, "").is_err());
        assert!(endpoint("compatible", Some("   "), "").is_err());
        let endpoint = endpoint("compatible", Some("http://localhost:11434/v1"), "").unwrap();
        assert_eq!(endpoint.url, "http://localhost:11434/v1/models");
    }

    #[test]
    fn an_unknown_provider_is_an_error_not_a_guess() {
        assert!(endpoint("something-else", Some("https://x.test"), "k").is_err());
    }

    // ------------------------------------------------------------------
    // `fetch_models` against live (loopback) HTTP — so that the status branch
    // and the empty-list check are not locked away behind the network.
    // ------------------------------------------------------------------

    fn mock_models_server(status_line: &str, body: &str) -> String {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let status = status_line.to_string();
        let body = body.to_string();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0_u8; 4096];
            let _ = stream.read(&mut buf);
            let response = format!(
                "{status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
        });
        format!("http://{addr}")
    }

    #[tokio::test]
    async fn fetch_models_returns_parsed_list_on_success() {
        let url = mock_models_server(
            "HTTP/1.1 200 OK",
            r#"{"data":[{"id":"gpt-4o-mini"},{"id":"gpt-4o"}]}"#,
        );
        let models = fetch_models("openai", Some(&url), "key").await.unwrap();
        assert_eq!(models, vec!["gpt-4o", "gpt-4o-mini"]);
    }

    #[tokio::test]
    async fn fetch_models_maps_401_to_auth_error() {
        let url = mock_models_server("HTTP/1.1 401 Unauthorized", r#"{"error":"bad key"}"#);
        let err = fetch_models("openai", Some(&url), "bad").await.unwrap_err();
        assert_eq!(err, "ключ не подошёл");
    }

    #[tokio::test]
    async fn fetch_models_rejects_an_empty_list() {
        let url = mock_models_server("HTTP/1.1 200 OK", r#"{"data":[]}"#);
        let err = fetch_models("openai", Some(&url), "key").await.unwrap_err();
        assert_eq!(err, "провайдер вернул пустой список");
    }
}
