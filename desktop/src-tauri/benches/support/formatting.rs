//! Deterministic inputs shared by the formatter measurements.

use serde_json::{json, Value};

pub const TEXT: &str = "ну мы проверили таури и тайп скрипт, обновили гит хаб и пост грэс. затем отправили готовый результат коллегам и сохранили настройки проекта";

pub fn config(term_count: usize, enabled: bool, rule_count: usize) -> Value {
    let known = [
        "Tauri",
        "TypeScript",
        "GitHub",
        "PostgreSQL",
        "WebSocket",
        "OpenAI",
    ];
    let words: Vec<String> = (0..term_count)
        .map(|index| {
            if let Some(word) = known.get(index) {
                return (*word).to_string();
            }
            let suffix: String = (0..4)
                .rev()
                .map(|place| char::from(b'a' + ((index / 26usize.pow(place)) % 26) as u8))
                .collect();
            format!("Orbitbrand{suffix}")
        })
        .collect();
    let rules: Vec<Value> = (0..rule_count)
        .map(|index| {
            json!({
                "id": format!("benchmark-{index}"),
                "find": if index == 0 { "готовый результат".to_string() } else { format!("benchmark_marker_{index}") },
                "replace": "проверенный результат",
                "match": "phrase", "enabled": true,
                "preserve_case": false, "case_sensitive": false,
            })
        })
        .collect();
    json!({
        "language": "ru",
        "text_formatting": { "enabled": enabled, "custom_words": words },
        "replacement_rules": rules,
    })
}
