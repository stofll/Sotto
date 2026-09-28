//! Readable paragraphs without inventing punctuation or changing words.

pub(super) fn split(text: &str) -> String {
    // Existing layout may be a list, code or a deliberate short paragraph.
    if text.contains(['\n', '\r']) || text.chars().take(350).count() < 350 {
        return text.to_owned();
    }
    let protected = crate::text_protection::technical_spans(text);
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
        if ch == '.' {
            let token = text[..byte]
                .rsplit(|c: char| !c.is_alphabetic())
                .next()
                .unwrap_or_default();
            if token.chars().count() <= 1
                || matches!(
                    token.to_lowercase().as_str(),
                    "г" | "ул"
                        | "д"
                        | "кв"
                        | "стр"
                        | "рис"
                        | "им"
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
                )
            {
                continue;
            }
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
            && (length >= 350 || (sentences >= 4 && length >= 200))
            && text[next..].chars().take(100).count() == 100
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
