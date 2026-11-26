use serde_json::{json, Value};
use sotto_lib::formatter::{
    format_transcription_with_config_value, format_with_config_value, preview_format,
};

fn assert_pipeline(config: &Value, input: &str, expected: &str) {
    assert_eq!(format_with_config_value(config, input), expected);
    assert_eq!(
        format_transcription_with_config_value(config, input),
        expected
    );
    assert_eq!(preview_format(input, config).unwrap().formatted, expected);
    assert_eq!(format_with_config_value(config, expected), expected);
}

#[test]
fn removal_modes_override_final_period_and_keep_numeric_notation() {
    for (mode, expected) in [
        (
            "no_sentence_breaks",
            "Размеры: 8,8 мм Затем, возможно Проверим",
        ),
        ("none", "Размеры 8,8 мм Затем возможно Проверим"),
    ] {
        let config = json!({"language": "ru", "text_formatting": {
            "punctuation_mode": mode,
            "split_sentences": true,
            "split_paragraphs": true,
            "final_punctuation": true
        }});
        assert_pipeline(
            &config,
            "Размеры: 8,8 мм. Затем, возможно... Проверим?",
            expected,
        );
    }
}

#[test]
fn removal_modes_have_an_independent_casing_choice_and_allow_empty_output() {
    let mut config = json!({"language": "ru", "text_formatting": {
        "punctuation_mode": "none",
        "capitalize_sentences": false,
        "final_punctuation": true
    }});
    assert_pipeline(&config, "Размеры: 8,8 мм. Готово!", "размеры 8,8 мм Готово");
    assert_pipeline(&config, ".,?!…", "");
    config["text_formatting"]["enabled"] = json!(false);
    assert_pipeline(
        &config,
        "Размеры: 8,8 мм. Готово!",
        "Размеры: 8,8 мм. Готово!",
    );
}

#[test]
fn replacement_matches_keep_priority_over_local_punctuation() {
    let config = json!({"language": "ru", "text_formatting": {
        "punctuation_mode": "none",
        "capitalize_sentences": false
    }, "replacement_rules": [{
        "find": "имя:пример", "replace": "Name:Example", "match": "phrase"
    }, {
        "find": "знак вопроса", "replace": "?", "match": "phrase"
    }]});
    // Explicit outputs are not recursively processed inside one local pass.
    let input = "имя:пример. знак вопроса";
    assert_eq!(format_with_config_value(&config, input), "Name:Example ?");
    assert_eq!(
        format_transcription_with_config_value(&config, input),
        "Name:Example ?"
    );
    assert_eq!(
        preview_format(input, &config).unwrap().formatted,
        "Name:Example ?"
    );
}

#[test]
fn numeric_sentence_start_does_not_capitalize_dimensions_or_units() {
    let config = json!({"language": "ru", "text_formatting": {
        "normalize_russian_numbers": true,
        "normalize_dimensions": true
    }});
    assert_pipeline(&config, "тридцать пять на тридцать пять мм", "35 х 35 мм.");
    assert_pipeline(&config, "пять миллиграммов", "5 мг.");
    assert_pipeline(&config, "один. следующий пункт", "1. Следующий пункт.");
}
