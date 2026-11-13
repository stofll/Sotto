//! Readable paragraphs without inventing punctuation or changing words.

pub(crate) const SENTENCE_CLOSERS: [char; 8] = ['»', '”', '’', '"', '\'', ')', ']', '}'];

fn is_initial(word: &str) -> bool {
    let mut chars = word.chars();
    chars.next().is_some_and(char::is_uppercase) && chars.next().is_none()
}

/// Some abbreviations need context because their spelling is also a word.
pub(crate) fn is_sentence_abbreviation(before_period: &str, after_period: &str) -> bool {
    let word = before_period
        .rsplit(|ch: char| !ch.is_alphabetic())
        .next()
        .unwrap_or_default();
    if word.is_empty() {
        return false;
    }
    let previous_word = || {
        before_period[..before_period.len() - word.len()]
            .split_whitespace()
            .next_back()
            .unwrap_or_default()
            .trim_start_matches(|ch: char| !ch.is_alphabetic())
    };
    let next = after_period.trim_start();
    if is_initial(word) {
        let previous = previous_word();
        let after_name = previous.starts_with(char::is_uppercase)
            && previous
                .strip_suffix('.')
                .map_or_else(|| previous.ends_with(char::is_alphabetic), is_initial);
        if after_name || (!matches!(word, "Я" | "I") && next.starts_with(char::is_uppercase)) {
            return true;
        }
    }

    match word.to_lowercase().as_str() {
        "рис" => {
            next.starts_with(|ch: char| ch.is_ascii_digit())
                || previous_word().to_lowercase() == "см."
        }
        "им" if next.starts_with(char::is_uppercase) => {
            let previous = previous_word();
            let acronym = !previous.is_empty() && previous.chars().all(char::is_uppercase);
            let previous = previous.to_lowercase();
            acronym
                || [
                    "театр",
                    "музе",
                    "институт",
                    "университет",
                    "академи",
                    "школ",
                    "библиотек",
                    "парк",
                ]
                .iter()
                .any(|&prefix| previous.starts_with(prefix))
        }
        word => matches!(
            word,
            "г" | "ул"
                | "д"
                | "кв"
                | "стр"
                | "тов"
                | "проф"
                | "др"
                | "пр"
                | "руб"
                | "коп"
                | "тыс"
                | "млн"
                | "млрд"
                | "см"
                | "т"
                | "е"
                | "п"
                | "dr"
                | "mr"
                | "mrs"
                | "ms"
                | "prof"
                | "st"
                | "vs"
                | "etc"
                | "e"
                | "g"
        ),
    }
}

pub(super) fn split(text: &str) -> String {
    // Existing layout may be a list, code or a deliberate short paragraph.
    if text.contains(['\n', '\r']) || text.chars().take(250).count() < 250 {
        return text.to_owned();
    }
    let protected = crate::text_protection::cleanup_spans(text);
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut boundaries = Vec::new();
    let mut nesting = 0usize;
    let mut quoted = false;
    for (i, &(byte, ch)) in chars.iter().enumerate() {
        if protected.iter().any(|span| span.contains(&byte)) {
            continue;
        }
        match ch {
            '(' | '[' | '{' | '«' | '“' => nesting += 1,
            ')' | ']' | '}' | '»' | '”' => nesting = nesting.saturating_sub(1),
            '"' => quoted = !quoted,
            _ => {}
        }
        if nesting > 0 || quoted || !matches!(ch, '.' | '!' | '?') {
            continue;
        }
        let Some(&(next_byte, next)) = chars.get(i + 1) else {
            continue;
        };
        if !next.is_whitespace() {
            continue;
        }
        let tail = text[next_byte..].trim_start();
        if !tail.chars().next().is_some_and(char::is_uppercase) {
            continue;
        }
        if ch == '.' && is_sentence_abbreviation(&text[..byte], tail) {
            continue;
        }
        boundaries.push((next_byte, text.len() - tail.len()));
    }
    let mut out = String::with_capacity(text.len());
    let mut start = 0;
    let mut sentences = 0;
    for (end, next) in boundaries {
        sentences += 1;
        let length = text[start..end].chars().count();
        if sentences >= 2
            && (length >= 250 || (sentences >= 4 && length >= 180))
            && text[next..].chars().take(60).count() == 60
        {
            out.push_str(&text[start..end]);
            out.push_str("\n\n");
            start = next;
            sentences = 0;
        }
    }
    out.push_str(&text[start..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn medium_text_requires_two_sentences_and_a_readable_tail() {
        let first = format!("Первое {}.", "слово ".repeat(22).trim_end());
        let second = format!("Второе {}.", "слово ".repeat(22).trim_end());
        for tail_length in [59, 60] {
            let tail = format!("Т{}.", "а".repeat(tail_length - 2));
            let text = format!("{first} {second} {tail}");
            let expected = if tail_length == 60 {
                format!("{first} {second}\n\n{tail}")
            } else {
                text.clone()
            };
            assert_eq!(split(&text), expected);
            assert_eq!(split(&expected), expected);
        }
        let text = format!(
            "Первое {}. Второе {}.",
            "слово ".repeat(50).trim_end(),
            "слово ".repeat(20).trim_end()
        );
        assert_eq!(split(&text), text);
    }

    #[test]
    fn groups_sentences_without_changing_words_or_punctuation() {
        let sentence = "Это достаточно подробное предложение о настройке приложения и его поведении во время записи голоса.";
        let text = std::iter::repeat_n(sentence, 9)
            .collect::<Vec<_>>()
            .join(" ");
        let output = split(&text);
        assert!(output.contains("\n\n"));
        assert_eq!(
            output.split_whitespace().collect::<Vec<_>>(),
            text.split_whitespace().collect::<Vec<_>>()
        );
        assert_eq!(split(&output), output);
        assert!(output.split("\n\n").all(|p| p.matches('.').count() >= 2));
    }

    #[test]
    fn preserves_existing_layout_and_unpunctuated_speech() {
        for text in [
            "Короткая фраза.".to_owned(),
            "слово ".repeat(150),
            "Один абзац.\n\nДругой абзац. ".repeat(20),
        ] {
            assert_eq!(split(&text), text);
        }
    }

    #[test]
    fn does_not_invent_boundaries_inside_technical_text_or_abbreviations() {
        for fragment in [
            "г. Москва ",
            "А. Пушкин ",
            "Dr. Smith ",
            "`first. Second` ",
            "https://example.org/Test ",
            "«Первое. Второе» ",
            "(Первое. Второе) ",
            "3.14 ",
        ] {
            let text = fragment.repeat(70);
            assert_eq!(split(&text), text, "{fragment}");
        }
    }
}
