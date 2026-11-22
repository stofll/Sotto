//! Opt-in removal of recognizer punctuation without damaging numeric notation.

use super::{is_sentence_abbreviation, pauses::PauseNormalizer, FormatStep};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::{ops::Range, sync::LazyLock};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PunctuationMode {
    NoSentenceBreaks,
    None,
    #[default]
    #[serde(other)]
    Automatic,
}

static NUMERIC_NOTATION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:[+−-][ \t]*)?[0-9]+(?:[.,:][0-9]+)*(?:[ \t]*[–—−-][ \t]*[+−-]?[0-9]+(?:[.,:][0-9]+)*)?")
        .expect("valid numeric punctuation pattern")
});

pub(super) struct PunctuationFilter {
    pub enabled: bool,
    pub mode: PunctuationMode,
}

impl FormatStep for PunctuationFilter {
    fn name(&self) -> &str {
        "Punctuation policy"
    }

    fn description(&self) -> &str {
        "Remove automatic prose punctuation, preserving numeric and technical notation"
    }

    fn permits_empty_output(&self) -> bool {
        true
    }

    fn protected_spans(&self, text: &str) -> Vec<Range<usize>> {
        // This also separates pause ellipses from dotted technical names.
        let mut spans = PauseNormalizer { enabled: true }.protected_spans(text);
        // Decide from original context: masking a number must not turn
        // «рис. 2» into a sentence boundary by hiding its numeric argument.
        spans.extend(text.char_indices().filter_map(|(offset, ch)| {
            (ch == '.' && is_sentence_abbreviation(&text[..offset], &text[offset + 1..]))
                .then_some(offset..offset + 1)
        }));
        spans.extend(NUMERIC_NOTATION.find_iter(text).map(|m| m.range()));
        spans
    }

    fn apply_unprotected(&self, text: &str) -> String {
        if !self.enabled || self.mode == PunctuationMode::Automatic {
            return text.to_owned();
        }
        let mut result = String::with_capacity(text.len());
        for (offset, ch) in text.char_indices() {
            let before = &text[..offset];
            let after = &text[offset + ch.len_utf8()..];
            let remove = match ch {
                '.' => !is_sentence_abbreviation(before, after),
                '!' | '?' | '…' => true,
                // Brackets can carry arithmetic grouping even outside code.
                ',' | ':' | ';' | '«' | '»' | '“' | '”' | '"' => {
                    self.mode == PunctuationMode::None
                }
                '\'' | '‘' | '’' => {
                    self.mode == PunctuationMode::None
                        && !(before.ends_with(char::is_alphanumeric)
                            && after.starts_with(char::is_alphanumeric))
                }
                '-' | '–' | '—' => {
                    self.mode == PunctuationMode::None
                        && !(before.ends_with(char::is_alphanumeric)
                            && after.starts_with(char::is_alphanumeric))
                }
                _ => false,
            };
            if remove {
                result.push(' ');
            } else {
                result.push(ch);
            }
        }
        compact_spacing(&result)
    }

    fn enabled(&self) -> bool {
        self.enabled && self.mode != PunctuationMode::Automatic
    }

    fn set_enabled(&mut self, value: bool) {
        self.enabled = value;
    }
}

/// Only horizontal whitespace introduced around punctuation is collapsed;
/// existing paragraph breaks and protected-token contents remain intact.
fn compact_spacing(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut pending_space = false;
    for ch in text.chars() {
        match ch {
            '\n' | '\r' => {
                pending_space = false;
                result.push(ch);
            }
            ch if ch.is_whitespace() => pending_space = true,
            _ => {
                if pending_space
                    && !result.is_empty()
                    && !result.ends_with(['\n', '\r', '(', '[', '{', '«', '“'])
                    && !super::SENTENCE_CLOSERS.contains(&ch)
                    && !matches!(ch, ',' | ':' | ';')
                {
                    result.push(' ');
                }
                result.push(ch);
                pending_space = false;
            }
        }
    }
    result.trim().to_owned()
}

/// The selected punctuation policy also applies to accepted LLM responses.
/// This small snapshot follows delayed responses without copying dictionaries.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct OutputPunctuation {
    mode: PunctuationMode,
    lowercase_start: bool,
}

impl OutputPunctuation {
    pub(crate) fn from_config(config: &super::TextFormattingConfig) -> Self {
        Self {
            mode: if config.enabled {
                config.punctuation_mode
            } else {
                PunctuationMode::Automatic
            },
            lowercase_start: !config.capitalize_sentences,
        }
    }

    pub(crate) fn apply(self, text: &str) -> String {
        if self.mode == PunctuationMode::Automatic {
            return text.to_owned();
        }
        let filtered = PunctuationFilter {
            enabled: true,
            mode: self.mode,
        }
        .apply(text);
        if self.lowercase_start {
            super::ChatStartLowercaser { enabled: true }.apply(&filtered)
        } else {
            filtered
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filter(text: &str, mode: PunctuationMode) -> String {
        PunctuationFilter {
            enabled: true,
            mode,
        }
        .apply(text)
    }

    #[test]
    fn sentence_mode_keeps_clause_punctuation_and_existing_line_breaks() {
        assert_eq!(
            filter(
                "Размеры: 8,8 мм. Затем, возможно... Проверим?\nГотово!",
                PunctuationMode::NoSentenceBreaks
            ),
            "Размеры: 8,8 мм Затем, возможно Проверим\nГотово"
        );
        assert_eq!(
            filter("можно...добавить", PunctuationMode::NoSentenceBreaks),
            "можно добавить"
        );
        assert_eq!(filter("...?!", PunctuationMode::NoSentenceBreaks), "");
        assert_eq!(
            filter(
                "«Готово.» (Проверено!) Затем, да.",
                PunctuationMode::NoSentenceBreaks
            ),
            "«Готово» (Проверено) Затем, да"
        );
    }

    #[test]
    fn full_removal_preserves_numbers_ranges_hyphens_and_apostrophes() {
        for (input, expected) in [
            (
                "Размеры: 8,8 х 3.5 мм; время 10:30!",
                "Размеры 8,8 х 3.5 мм время 10:30",
            ),
            (
                "От -0,5 до +2.0, диапазон 5 – 8 и 1...3.",
                "От -0,5 до +2.0 диапазон 5 – 8 и 1...3",
            ),
            (
                "«Кое-что» — это (проверка), don't, O’Neill.",
                "Кое-что это (проверка) don't O’Neill",
            ),
            (
                "Значение - 0,5, выражение (1 + 2) * 3 и x - 5.",
                "Значение - 0,5 выражение (1 + 2) * 3 и x - 5",
            ),
            ("Первый; второй:\n\nтретий!", "Первый второй\n\nтретий"),
        ] {
            let actual = filter(input, PunctuationMode::None);
            assert_eq!(actual, expected, "{input}");
            assert_eq!(filter(&actual, PunctuationMode::None), actual);
        }
    }

    #[test]
    fn technical_content_and_abbreviations_are_preserved() {
        for mode in [PunctuationMode::NoSentenceBreaks, PunctuationMode::None] {
            let input = "См. рис. 2: https://example.test/a?q=8,8, a.b@example.test; `x = 8.8;` и Cargo.toml.";
            let result = filter(input, mode);
            assert!(result.contains("См. рис. 2"), "{result}");
            assert!(result.contains("https://example.test/a?q=8,8"), "{result}");
            assert!(result.contains("a.b@example.test"), "{result}");
            assert!(result.contains("`x = 8.8;`"), "{result}");
            assert!(result.ends_with("Cargo.toml"), "{result}");
            assert_eq!(
                filter("рис. 2, рис. 2.5.", mode),
                if mode == PunctuationMode::None {
                    "рис. 2 рис. 2.5"
                } else {
                    "рис. 2, рис. 2.5"
                }
            );
            assert_eq!(
                filter("А. С. Пушкин, т. е. автор.", mode),
                if mode == PunctuationMode::None {
                    "А. С. Пушкин т. е. автор"
                } else {
                    "А. С. Пушкин, т. е. автор"
                }
            );
        }
    }

    #[test]
    fn disabled_and_unknown_modes_keep_existing_behavior() {
        let input = "Готово. Да, проверено!";
        assert_eq!(filter(input, PunctuationMode::Automatic), input);
        assert_eq!(
            PunctuationFilter {
                enabled: false,
                mode: PunctuationMode::None
            }
            .apply(input),
            input
        );
        assert_eq!(
            serde_json::from_str::<PunctuationMode>("\"future_mode\"").unwrap(),
            PunctuationMode::Automatic
        );
    }

    #[test]
    fn accepted_ai_output_uses_the_same_policy_without_rerunning_replacements() {
        let config = super::super::TextFormattingConfig {
            punctuation_mode: PunctuationMode::None,
            capitalize_sentences: false,
            ..Default::default()
        };
        assert_eq!(
            OutputPunctuation::from_config(&config).apply("Размеры: 8,8 мм. Готово!"),
            "размеры 8,8 мм Готово"
        );
        let disabled = super::super::TextFormattingConfig {
            enabled: false,
            ..config
        };
        assert_eq!(
            OutputPunctuation::from_config(&disabled).apply("Готово."),
            "Готово."
        );
    }
}
