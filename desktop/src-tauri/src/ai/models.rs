//! Списки моделей, которые провайдер отдаёт сам.
//!
//! До этого модуля model id вводился руками, а подсказка в UI отсылала
//! читать документацию. Держать актуальный список в коде нельзя: провайдеры
//! переименовывают и снимают модели чаще, чем выходят релизы приложения, и
//! устаревший id молча 404-ит в момент диктовки — то есть тогда, когда
//! разбираться меньше всего хочется.
//!
//! Запрос уходит только к выбранному провайдеру и только по явному действию
//! пользователя: подтягивание списка — это сетевой запрос с его ключом.
//!
//! Все пять провайдеров такой эндпоинт имеют — проверено запросом без ключа
//! по каждому: 200 у публичных, 401/403 у остальных, 404 нет ни у кого.
//! Различий два, и оба здесь учтены: Anthropic хочет `x-api-key` вместо
//! `Bearer`, а Gemini — другой путь и другую форму ответа.

use std::time::Duration;

use serde_json::Value;

use super::providers::{build_request, ANTHROPIC_BASE_URL, OPENAI_BASE_URL, OPENCODE_GO_BASE_URL};

/// Отдельный таймаут, короче, чем у обычного запроса к LLM: список моделей
/// тянут из настроек, глядя на спиннер, и полминуты ожидания там читаются
/// как «зависло», а не «идёт».
const FETCH_TIMEOUT_SECS: u64 = 10;

/// Куда стучаться за списком и чем авторизоваться.
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
            // Не OpenAI-совместимый: свой путь, своя форма ответа
            // (см. `parse_models`) и ключ отдельным заголовком.
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
        // OpenAI-совместимый провайдер задаётся своим base_url, и без него
        // идти некуда — подставлять сюда чей-то чужой адрес нельзя.
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

/// Разобрать ответ в список model id.
///
/// Три формы, все встречаются среди подключённых провайдеров:
/// OpenAI-совместимая `{"data": [{"id": …}]}`, голый массив строк и
/// Gemini `{"models": [{"name": "models/…"}]}`.
fn parse_models(parsed: &Value) -> Vec<String> {
    let from_entry = |entry: &Value| -> Option<String> {
        if let Some(id) = entry.get("id").and_then(Value::as_str) {
            return Some(id.to_string());
        }
        if let Some(name) = entry.get("name").and_then(Value::as_str) {
            // Gemini возвращает полный путь ресурса; в запросе нужен хвост.
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

/// Спросить у провайдера, какие модели он готов обслуживать.
///
/// Ошибку возвращаем текстом для показа рядом с полем: «список не
/// подтянулся» не должно выглядеть как «ключ неверный», поэтому вызывающая
/// сторона не превращает её в модальный тост.
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
        // Тело ответа не пересказываем целиком: у части провайдеров оно
        // содержит эхо запроса вместе с заголовками.
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

    /// Gemini отдаёт полный путь ресурса, а в запрос нужен хвост.
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

    /// У части OpenAI-совместимых провайдеров запись описана `name`, а не `id`.
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
        // Bearer здесь молча вернул бы 401 — провайдер его не понимает.
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

    /// Локальные пресеты (Ollama, LM Studio, vLLM) приходят как
    /// `compatible` с собственным адресом — для них это самая полезная
    /// функция, потому что список моделей там меняется чаще всего.
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
    // `fetch_models` против живого (loopback) HTTP — чтобы ветка статуса
    // и проверка пустого списка не были заперты за сетью.
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
