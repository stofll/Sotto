//! Splitting a long text into parts the LLM tidies one at a time.
//!
//! One request for an hour-long transcript cannot work: the answer is as long
//! as the text and outgrows any model's output limit, the request outlives any
//! sensible timeout, and one failure throws away the whole result. Parts sized
//! to the profile's output limit fit it, run side by side, and a part that
//! fails keeps only its own local text. Each part after the first is sent with
//! the end of the one before it, so a part does not start as a new text.

/// Fewest characters a part is cut to, so that a tiny output limit does not
/// turn a long file into hundreds of requests.
pub const MIN_PART_CHARS: usize = 1_000;

/// About how much of the preceding part is sent as context: a paragraph or
/// two, enough to carry the topic and the paragraphing across the cut.
pub const CONTEXT_CHARS: usize = 800;

/// One part and the whitespace that followed it in the source, so the tidied
/// parts can be joined back with the same layout.
#[derive(Debug, PartialEq, Eq)]
pub struct Part<'a> {
    pub text: &'a str,
    pub separator: &'a str,
}

/// Split `text` into the fewest parts of at most `max` characters, about
/// equal in length, cutting at a paragraph break if one is near, else at a
/// sentence end, else at a space. A text within `max` stays one part. A cut
/// never falls inside a word; separators are kept, not lost.
pub fn split(text: &str, max: usize) -> Vec<Part<'_>> {
    let max = max.max(MIN_PART_CHARS);
    let mut parts = Vec::new();
    let mut rest = text.trim_start();
    while !rest.is_empty() {
        let chars = rest.chars().count();
        if chars <= max {
            parts.push(Part {
                text: rest.trim_end(),
                separator: "",
            });
            break;
        }
        // Aimed at an equal share of what is left, counted a little below
        // `max`: a cut that falls short of its target then leaves no scrap
        // for a last part.
        let target = chars.div_ceil(chars.div_ceil(max * 9 / 10));
        let cut = cut_point(rest, target, (target + target / 2).min(max));
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

/// Byte offset to cut `text` at: the best boundary between half `target` and
/// `longest` characters, the one nearest the target of its kind, so the parts
/// come out about equal.
fn cut_point(text: &str, target: usize, longest: usize) -> usize {
    let byte_at = |chars: usize| {
        text.char_indices()
            .nth(chars)
            .map_or(text.len(), |(index, _)| index)
    };
    let (low, high) = (byte_at(target / 2), byte_at(longest));
    let aim = byte_at(target);
    let window = &text[low..high];
    let nearest = |offsets: &mut dyn Iterator<Item = usize>| {
        offsets
            .map(|offset| low + offset)
            .min_by_key(|cut| cut.abs_diff(aim))
    };
    if let Some(cut) = nearest(&mut window.match_indices("\n\n").map(|(offset, _)| offset)) {
        return cut;
    }
    if let Some(cut) = nearest(&mut sentence_ends(window)) {
        return cut;
    }
    let mut spaces = window
        .char_indices()
        .filter(|(_, c)| c.is_whitespace())
        .map(|(offset, _)| offset);
    if let Some(cut) = nearest(&mut spaces) {
        return cut;
    }
    // One word longer than the whole window: cut at the target rather than
    // send it whole.
    byte_at(target)
}

/// The end of `text`, at most about `max` characters, starting at a paragraph
/// or a sentence when one begins in it, else at a word. A paragraph counts only
/// in the first half, so a short closing line does not stand in for the
/// context before it.
pub fn tail(text: &str, max: usize) -> &str {
    let text = text.trim_end();
    let chars = text.chars().count();
    if chars <= max {
        return text;
    }
    let start = text
        .char_indices()
        .nth(chars - max)
        .map_or(text.len(), |(index, _)| index);
    let window = &text[start..];
    let boundary = window
        .find("\n\n")
        .filter(|offset| *offset <= window.len() / 2)
        .or_else(|| sentence_ends(window).next())
        .or_else(|| window.find(char::is_whitespace))
        .unwrap_or(0);
    window[boundary..].trim_start()
}

/// Byte offsets just past each sentence end in `text` that whitespace follows.
fn sentence_ends(text: &str) -> impl Iterator<Item = usize> + '_ {
    text.char_indices()
        .filter(|(_, c)| matches!(c, '.' | '!' | '?' | '…'))
        .filter_map(|(index, c)| {
            let end = index + c.len_utf8();
            text[end..].starts_with(char::is_whitespace).then_some(end)
        })
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
    fn parts_end_at_sentences_and_stay_within_the_limit() {
        let text = sentences(400);
        let parts = split(&text, 3_000);
        assert!(parts.len() > 3);
        for part in &parts {
            let chars = part.text.chars().count();
            assert!((500..=3_000).contains(&chars), "part of {chars} chars");
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
        let parts = split(&text, 5_000);
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
    fn one_enormous_word_is_cut_within_the_limit() {
        let text = "я".repeat(10_000);
        let parts = split(&text, 3_000);
        assert!(parts.iter().all(|part| part.text.chars().count() <= 3_000));
        let total: usize = parts.iter().map(|part| part.text.chars().count()).sum();
        assert_eq!(total, 10_000);
    }

    /// Just over the limit, a text splits in two about equal halves rather
    /// than a full part and a scrap.
    #[test]
    fn a_text_just_over_the_limit_splits_evenly() {
        let text = sentences(130);
        let chars = text.chars().count();
        let parts = split(&text, chars - 200);
        assert_eq!(parts.len(), 2);
        for part in &parts {
            let share = part.text.chars().count() * 10 / chars;
            assert!((4..=5).contains(&share), "part of {share}/10");
        }
    }

    #[test]
    fn a_tiny_limit_still_cuts_parts_of_a_useful_size() {
        let text = sentences(400);
        let parts = split(&text, 100);
        assert!(parts
            .iter()
            .all(|part| part.text.chars().count() > MIN_PART_CHARS / 3));
    }

    #[test]
    fn the_tail_starts_at_a_sentence() {
        let text = sentences(100);
        let tail = tail(&text, 200);
        assert!(tail.starts_with("Это предложение номер"), "{tail:?}");
        assert!(tail.chars().count() <= 200);
        assert!(text.ends_with(tail));
    }

    #[test]
    fn the_tail_prefers_a_paragraph_start() {
        let text = format!("{}\n\nНовый абзац. {}", sentences(20), sentences(5));
        assert!(tail(&text, 300).starts_with("Новый абзац."));
    }

    /// A short closing line after the last paragraph break does not replace
    /// the sentences before it.
    #[test]
    fn a_late_paragraph_break_keeps_the_sentences_before_it() {
        let text = format!("{}\n\nВсё.", sentences(20));
        let tail = tail(&text, 300);
        assert!(tail.starts_with("Это предложение номер"), "{tail:?}");
        assert!(tail.ends_with("\n\nВсё."));
    }

    #[test]
    fn a_short_text_is_its_own_tail() {
        assert_eq!(tail("Коротко.\n", 800), "Коротко.");
    }
}
