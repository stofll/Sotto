//! Splitting a long text into parts the LLM tidies one at a time.
//!
//! One request for an hour-long transcript cannot work: the answer is as long
//! as the text and outgrows any model's output limit, the request outlives any
//! sensible timeout, and one failure throws away the whole result. Parts of a
//! few thousand characters each fit a small budget, run side by side, and a
//! part that fails keeps only its own local text.

/// Above this length a text is tidied in parts. Below it one request is
/// faster and keeps the most context for paragraphing.
pub const SPLIT_ABOVE_CHARS: usize = 6_000;

/// The length a part aims for. Long enough for paragraphing to have context,
/// short enough that the answer fits any model's output limit.
pub const PART_TARGET_CHARS: usize = 3_000;

/// One part and the whitespace that followed it in the source, so the tidied
/// parts can be joined back with the same layout.
#[derive(Debug, PartialEq, Eq)]
pub struct Part<'a> {
    pub text: &'a str,
    pub separator: &'a str,
}

/// Split `text` into parts of about `target` characters, cutting at a
/// paragraph break if one is near, else at a sentence end, else at a space.
/// A cut never falls inside a word; separators are kept, not lost.
pub fn split(text: &str, target: usize) -> Vec<Part<'_>> {
    let mut parts = Vec::new();
    let mut rest = text.trim_start();
    while !rest.is_empty() {
        let chars = rest.chars().count();
        if chars <= target + target / 2 {
            parts.push(Part {
                text: rest.trim_end(),
                separator: "",
            });
            break;
        }
        let cut = cut_point(rest, target);
        let (part, tail) = rest.split_at(cut);
        let next = tail.trim_start();
        let separator = &tail[..tail.len() - next.len()];
        // Empty only after a hard cut inside one enormous word.
        parts.push(Part {
            text: part.trim_end(),
            separator,
        });
        rest = next;
    }
    parts
}

/// Byte offset to cut `text` at: the best boundary between half and one and
/// a half times `target` characters.
fn cut_point(text: &str, target: usize) -> usize {
    let byte_at = |chars: usize| {
        text.char_indices()
            .nth(chars)
            .map_or(text.len(), |(index, _)| index)
    };
    let (low, high) = (byte_at(target / 2), byte_at(target + target / 2));
    let window = &text[low..high];
    let after = |offset: usize| low + offset;
    if let Some(offset) = window.rfind("\n\n") {
        return after(offset);
    }
    let sentence_end = window
        .char_indices()
        .rev()
        .filter(|(_, c)| matches!(c, '.' | '!' | '?' | '…'))
        .find_map(|(index, c)| {
            let end = index + c.len_utf8();
            window[end..]
                .starts_with(char::is_whitespace)
                .then_some(end)
        });
    if let Some(end) = sentence_end {
        return after(end);
    }
    if let Some(offset) = window.rfind(char::is_whitespace) {
        return after(offset);
    }
    // One word longer than the whole window: cut at the target rather than
    // send it whole.
    byte_at(target)
}

/// Join tidied parts with the separators that followed them in the source.
pub fn join(parts: &[Part<'_>], tidied: &[String]) -> String {
    let mut joined = String::new();
    for (part, text) in parts.iter().zip(tidied) {
        joined.push_str(text);
        joined.push_str(part.separator);
    }
    joined.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sentences(count: usize) -> String {
        (0..count)
            .map(|index| format!("Это предложение номер {index} про работу."))
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn a_short_text_stays_whole() {
        let text = sentences(10);
        assert_eq!(
            split(&text, 3_000),
            [Part {
                text: &text,
                separator: ""
            }]
        );
    }

    #[test]
    fn parts_end_at_sentences_and_stay_near_the_target() {
        let text = sentences(400);
        let parts = split(&text, 3_000);
        assert!(parts.len() > 3);
        for part in &parts {
            let chars = part.text.chars().count();
            assert!((1_500..=4_500).contains(&chars), "part of {chars} chars");
            assert!(
                part.text.ends_with('.'),
                "{:?}",
                &part.text[part.text.len() - 20..]
            );
        }
    }

    #[test]
    fn a_paragraph_break_is_preferred_to_a_sentence_end() {
        let text = format!("{}\n\n{}", sentences(80), sentences(80));
        let parts = split(&text, 3_000);
        assert_eq!(parts[0].separator, "\n\n");
        assert_eq!(parts[0].text, sentences(80));
    }

    #[test]
    fn joining_the_parts_unchanged_restores_the_text() {
        let text = format!(
            "{}\n\n{}  {}",
            sentences(150),
            sentences(90),
            sentences(200)
        );
        let parts = split(&text, 3_000);
        let unchanged: Vec<String> = parts.iter().map(|part| part.text.to_string()).collect();
        assert_eq!(join(&parts, &unchanged), text);
    }

    #[test]
    fn text_without_punctuation_is_cut_between_words() {
        let text = "слово ".repeat(2_000);
        let parts = split(&text, 3_000);
        assert!(parts.len() > 1);
        assert!(parts
            .iter()
            .all(|part| part.text.split(' ').all(|word| word == "слово")));
    }

    #[test]
    fn one_enormous_word_is_cut_at_the_target() {
        let text = "я".repeat(10_000);
        let parts = split(&text, 3_000);
        assert_eq!(parts[0].text.chars().count(), 3_000);
        let total: usize = parts.iter().map(|part| part.text.chars().count()).sum();
        assert_eq!(total, 10_000);
    }
}
