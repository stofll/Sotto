//! Conservative offline Russian spelling. Ambiguous words are left as dictated.

use std::collections::{HashMap, HashSet};

use once_cell::sync::Lazy;
use spellbook::Dictionary;

use crate::formatter::FormatStep;

static RUSSIAN: Lazy<Dictionary> = Lazy::new(|| {
    Dictionary::new(
        include_str!("../resources/spelling/ru_RU.aff"),
        include_str!("../resources/spelling/ru_RU.dic"),
    )
    .expect("bundled Russian dictionary must parse")
});

const ALPHABET: &str = "абвгдеёжзийклмнопрстуфхцчшщъыьэюя";
const MAX_WORD_CHARS: usize = 24;
const MAX_UNKNOWN_WORDS: usize = 128;

pub(crate) fn prepare() {
    Lazy::force(&RUSSIAN);
}

fn russian_letter(c: char) -> bool {
    matches!(c, 'а'..='я' | 'А'..='Я' | 'ё' | 'Ё')
}

pub(crate) fn known_russian_word(word: &str) -> bool {
    !word.is_empty() && word.chars().all(russian_letter) && RUSSIAN.check(&word.to_lowercase())
}

fn unique_correction(word: &str) -> Option<String> {
    let chars: Vec<char> = word.chars().collect();
    let mut found: Option<String> = None;
    // Do not trust suggestion ranking: enumerate the complete one-edit
    // neighbourhood and require agreement.
    let mut accept = |candidate: String| -> bool {
        if candidate != word && RUSSIAN.check(&candidate) && found.as_ref() != Some(&candidate) {
            if found.is_some() {
                return false;
            }
            found = Some(candidate);
        }
        true
    };
    for i in 0..chars.len() {
        let candidate = chars[..i].iter().chain(&chars[i + 1..]).collect();
        if !accept(candidate) {
            return None;
        }
        for letter in ALPHABET.chars() {
            let mut candidate = chars.clone();
            candidate[i] = letter;
            if !accept(candidate.into_iter().collect()) {
                return None;
            }
        }
        if i + 1 < chars.len() {
            let mut candidate = chars.clone();
            candidate.swap(i, i + 1);
            if !accept(candidate.into_iter().collect()) {
                return None;
            }
        }
    }
    for i in 0..=chars.len() {
        for letter in ALPHABET.chars().chain(std::iter::once('-')) {
            if letter == '-' {
                let left: String = chars[..i].iter().collect();
                let right: String = chars[i..].iter().collect();
                // Hunspell accepts arbitrary hyphenated compounds, including
                // invented «по-закрывать». Only productive Russian particles
                // justify inserting a hyphen without sentence context.
                if !(matches!(right.as_str(), "то" | "либо" | "нибудь") && indefinite_base(&left)
                    || left == "кое" && indefinite_base(&right))
                {
                    continue;
                }
            }
            let candidate = chars[..i]
                .iter()
                .copied()
                .chain(std::iter::once(letter))
                .chain(chars[i..].iter().copied())
                .collect();
            if !accept(candidate) {
                return None;
            }
        }
    }
    // Even a unique dictionary neighbour can replace unknown jargon with an
    // unrelated word («мержить» → «мерить»). Only mechanical edits are automatic.
    found.filter(|candidate| mechanical_edit(&chars, candidate))
}

fn indefinite_base(word: &str) -> bool {
    // A noun plus «-то» can be accepted as a compound by Hunspell. Limit
    // automatic joins to indefinite pronouns/adverbs, not names like «сотто».
    matches!(
        word,
        "кто"
            | "кого"
            | "кому"
            | "кем"
            | "ком"
            | "что"
            | "чего"
            | "чему"
            | "чем"
            | "чём"
            | "какой"
            | "какая"
            | "какое"
            | "какие"
            | "какого"
            | "какую"
            | "какому"
            | "каким"
            | "какими"
            | "каких"
            | "каком"
            | "чей"
            | "чья"
            | "чьё"
            | "чье"
            | "чьи"
            | "чьего"
            | "чьей"
            | "чьему"
            | "чью"
            | "чьим"
            | "чьими"
            | "чьих"
            | "чьём"
            | "чьем"
            | "который"
            | "которая"
            | "которое"
            | "которые"
            | "которого"
            | "которой"
            | "которую"
            | "которому"
            | "которым"
            | "которыми"
            | "которых"
            | "котором"
            | "сколько"
            | "скольких"
            | "скольким"
            | "сколькими"
            | "как"
            | "где"
            | "куда"
            | "откуда"
            | "когда"
            | "почему"
            | "зачем"
            | "отчего"
    )
}

fn mechanical_edit(chars: &[char], candidate: &str) -> bool {
    if candidate.contains('-') {
        return true;
    }
    for i in 0..chars.len() {
        if (i > 0 && chars[i - 1] == chars[i]) || (i + 1 == chars.len() && chars[i] == 'э') {
            let without: String = chars[..i].iter().chain(&chars[i + 1..]).collect();
            if without == candidate {
                return true;
            }
        }
        if i + 1 < chars.len() {
            let mut swapped = chars.to_vec();
            swapped.swap(i, i + 1);
            if swapped.into_iter().collect::<String>() == candidate {
                return true;
            }
        }
    }
    false
}

pub(crate) struct RussianSpellingCorrector {
    enabled: bool,
    protected: HashSet<String>,
}

impl RussianSpellingCorrector {
    pub(crate) fn new(enabled: bool, language: Option<&str>, terms: &[String]) -> Self {
        Self {
            enabled: enabled && language == Some("ru"),
            protected: terms
                .iter()
                .flat_map(|term| term.split_whitespace())
                .map(str::to_lowercase)
                .collect(),
        }
    }
}

impl FormatStep for RussianSpellingCorrector {
    fn name(&self) -> &str {
        "Russian spelling"
    }

    fn description(&self) -> &str {
        "однозначные опечатки в русских словах без LLM"
    }

    fn enabled(&self) -> bool {
        self.enabled
    }

    fn set_enabled(&mut self, value: bool) {
        self.enabled = value;
    }

    fn apply_unprotected(&self, text: &str) -> String {
        if !self.enabled {
            return text.to_string();
        }
        let mut cache: HashMap<&str, Option<String>> = HashMap::new();
        let mut out = String::with_capacity(text.len());
        for part in text.split_inclusive(char::is_whitespace) {
            let token = part.trim_end_matches(char::is_whitespace);
            let word = token.trim_matches(|c| ".,!?;:()[]{}«»\"'…".contains(c));
            // Capitalisation can mark a name. Mixed scripts, identifiers, paths,
            // hyphenated compounds and quoted code must not be guessed at.
            let eligible = (5..=MAX_WORD_CHARS).contains(&word.chars().count())
                && word.chars().all(|c| russian_letter(c) && c.is_lowercase())
                && !self.protected.contains(word);
            if eligible && !known_russian_word(word) {
                if !cache.contains_key(word) && cache.len() < MAX_UNKNOWN_WORDS {
                    cache.insert(word, unique_correction(word));
                }
                if let Some(Some(replacement)) = cache.get(word) {
                    let start = token.find(word).expect("trimmed word is a substring");
                    out.push_str(&part[..start]);
                    out.push_str(replacement);
                    out.push_str(&part[start + word.len()..]);
                    continue;
                }
            }
            out.push_str(part);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dictionary_accepts_inflected_words() {
        for word in ["стал", "стать", "редактора", "словарями", "проверками"]
        {
            assert!(known_russian_word(word), "{word}");
        }
        assert!(!known_russian_word("итогуу"));
    }

    #[test]
    fn fixes_unambiguous_errors_and_preserves_layout() {
        let corrector = RussianSpellingCorrector::new(true, Some("ru"), &[]);
        assert_eq!(
            corrector.apply("по итогуу  проверки\nкакомуто пользователю нужен компьюетр"),
            "по итогу  проверки\nкакому-то пользователю нужен компьютер"
        );
    }

    #[test]
    fn keeps_ambiguous_words_names_and_technical_tokens() {
        let corrector = RussianSpellingCorrector::new(true, Some("ru"), &[]);
        let text = "имееть неронки илиная мержить чекбокс прессетов отцентруем позакрывать сотто Итогуу ИТОГУУ паруMCP `итогуу` итогуу.txt /итогуу итогуу_тест";
        assert_eq!(corrector.apply(text), text);
        let code = "`команда итогуу проверкка`\n``ещё итогуу``\n```\nпроверкка\n```";
        assert_eq!(corrector.apply(code), code);
    }

    #[test]
    fn respects_language_switch_and_custom_vocabulary() {
        for language in [None, Some("auto"), Some("en"), Some("uk")] {
            assert_eq!(
                RussianSpellingCorrector::new(true, language, &[]).apply("по итогуу"),
                "по итогуу"
            );
        }
        assert_eq!(
            RussianSpellingCorrector::new(false, Some("ru"), &[]).apply("по итогуу"),
            "по итогуу"
        );
        assert_eq!(
            RussianSpellingCorrector::new(true, Some("ru"), &["итогуу".into()]).apply("по итогуу"),
            "по итогуу"
        );
    }
}
