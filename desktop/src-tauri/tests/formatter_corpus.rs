use serde::Deserialize;
use serde_json::{json, Value};
use sotto_lib::formatter::{
    format_transcription_with_config_value, format_with_config_value, preview_format,
};

#[derive(Deserialize)]
struct Case {
    id: String,
    category: String,
    input: String,
    expected: String,
    transcription_expected: Option<String>,
    config: Option<Value>,
    #[serde(default = "default_true")]
    idempotent: bool,
}

fn default_true() -> bool {
    true
}

#[test]
fn reviewed_formatter_corpus() {
    let cases: Vec<Case> =
        serde_json::from_str(include_str!("fixtures/formatter/cases.json")).unwrap();
    let mut failures = Vec::new();
    let mut ids = std::collections::HashSet::new();
    for case in cases {
        assert!(ids.insert(case.id.clone()), "duplicate case: {}", case.id);
        assert!(!case.category.is_empty(), "missing category: {}", case.id);
        let config = case.config.unwrap_or_else(|| json!({"language": "ru"}));
        let actual = format_with_config_value(&config, &case.input);
        let preview = preview_format(&case.input, &config).unwrap().formatted;
        let delivered = format_transcription_with_config_value(&config, &case.input);
        let delivery_expected = case
            .transcription_expected
            .as_ref()
            .unwrap_or(&case.expected);
        if &delivered != delivery_expected {
            failures.push(format!(
                "{}: transcription expected {:?}, got {:?}",
                case.id, delivery_expected, delivered
            ));
        }
        if actual != case.expected {
            failures.push(format!(
                "{} [{}]:\nexpected: {:?}\n  actual: {:?}",
                case.id, case.category, case.expected, actual
            ));
        }
        if preview != actual {
            failures.push(format!("{}: preview differs from formatter", case.id));
        }
        if case.idempotent && format_with_config_value(&config, &actual) != actual {
            failures.push(format!(
                "{}: repeated processing changed the result",
                case.id
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}
