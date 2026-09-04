//! Guard against an LLM that retells the dictation instead of tidying it up.
//!
//! The prompt forbids paraphrasing, shortening and dropping the author's
//! lexis in the strongest terms it can (`PROMPT_EDIT_SCOPE` in the presets,
//! repeated in `step::OUTPUT_CONTRACT`). Nothing checked that the model
//! obeyed, so a summary reached the clipboard looking exactly like a clean-up:
//! a real 3758-character transcript came back at 1957 characters with whole
//! passages missing from the middle, and the pipeline reported success.
//!
//! Comparing characters would be wrong here. Tidying up *adds* characters —
//! commas, full stops, capitals, dashes, paragraph breaks — so the character
//! count moves for reasons that have nothing to do with content. Words are the
//! stable unit: punctuation attaches to a word instead of becoming one, and
//! the edits the prompt permits (filler removal, de-stuttering) subtract only
//! a small share of them.

/// Smallest share of the dictation's words we accept back.
///
/// The model is allowed to delete: "э-э", stutters, false starts and
/// unintentional repeats. On a filler-heavy transcript that is well under a
/// fifth of the words, so a third of slack clears any honest clean-up while
/// still catching a retelling — the observed failure kept 45%.
const MIN_KEPT_WORD_RATIO: f64 = 0.7;

/// Below this many words a ratio says nothing: on a ten-word note, dropping
/// three fillers is both correct and a 30% loss. Short dictations are also the
/// ones where a bad LLM pass costs the user least to notice and redo.
const MIN_WORDS_TO_JUDGE: usize = 40;

/// Count words, ignoring punctuation entirely.
///
/// Split on whitespace, then strip non-alphanumeric characters from both ends
/// of each token. `«сказал»,` and `сказал` are one word either way, which is
/// the whole point: added quotes and commas must not move the count. Tokens
/// that are pure punctuation (a lone dash between clauses) drop out.
pub fn word_count(text: &str) -> usize {
    text.split_whitespace()
        .filter(|token| {
            token
                .trim_matches(|c: char| !c.is_alphanumeric())
                .chars()
                .next()
                .is_some()
        })
        .count()
}

/// Share of the input's words that came back, or `None` when the input is too
/// short to judge. Values above 1.0 are normal and fine — a model that splits
/// a run-on sentence can legitimately return more words than it got.
pub fn kept_word_ratio(input: &str, output: &str) -> Option<f64> {
    let before = word_count(input);
    if before < MIN_WORDS_TO_JUDGE {
        return None;
    }
    Some(word_count(output) as f64 / before as f64)
}

/// True when the answer is too short to be the same text tidied up.
pub fn dropped_too_much(input: &str, output: &str) -> bool {
    kept_word_ratio(input, output).is_some_and(|ratio| ratio < MIN_KEPT_WORD_RATIO)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The edits the prompt actually permits must never trip the guard, and
    /// they all live in punctuation: this is the example the presets ship.
    const TIDY_INPUT: &str = "так вот вчера собрал наконец полку в коридоре шурупы оказались короткие пришлось ехать в магазин ещё раз в общем провозился до вечера отдельная история это инструкция там нарисовано одно а в коробке лежит совсем другое так что я её мало-мальски полистал и собрал по наитию а потом ещё час искал куда делась вторая полка";

    #[test]
    fn punctuation_does_not_change_the_word_count() {
        assert_eq!(
            word_count("так вот вчера собрал полку"),
            word_count("Так вот, вчера собрал полку.")
        );
        assert_eq!(
            word_count("он сказал что уходит"),
            word_count("Он сказал: «что уходит» —")
        );
    }

    #[test]
    fn lone_punctuation_is_not_a_word() {
        assert_eq!(word_count("да — нет"), 2);
        assert_eq!(word_count("  "), 0);
    }

    #[test]
    fn adding_punctuation_and_paragraphs_passes() {
        let tidied = "Так вот, вчера собрал наконец полку в коридоре. Шурупы оказались короткие, пришлось ехать в магазин ещё раз. В общем, провозился до вечера.\n\nОтдельная история — это инструкция. Там нарисовано одно, а в коробке лежит совсем другое, так что я её мало-мальски полистал и собрал по наитию. А потом ещё час искал, куда делась вторая полка.";
        assert!(!dropped_too_much(TIDY_INPUT, tidied));
    }

    #[test]
    fn removing_fillers_and_repeats_passes() {
        // Every "ну", "э-э" and doubled word stripped — the most aggressive
        // honest clean-up there is. Still well inside the allowance.
        let input = "ну э-э я думаю что нам нужно нам нужно собраться завтра ну и обсудить это дело потому что потому что времени мало и э-э короче надо решать уже сегодня а то потом будет поздно совсем и все разъедутся кто куда по отпускам да";
        let output = "Я думаю, что нам нужно собраться завтра и обсудить это дело, потому что времени мало. Короче, надо решать уже сегодня, а то потом будет поздно совсем и все разъедутся кто куда по отпускам.";
        assert!(!dropped_too_much(input, output));
    }

    #[test]
    fn a_retelling_is_caught() {
        // Ratio of the real incident: 639 words in, 289 back.
        let input = "слово ".repeat(639);
        let output = "слово ".repeat(289);
        assert!(dropped_too_much(&input, &output));
        let ratio = kept_word_ratio(&input, &output).unwrap();
        assert!((0.45..0.46).contains(&ratio), "ratio was {ratio}");
    }

    #[test]
    fn a_truncated_answer_is_caught() {
        let input = "слово ".repeat(100);
        assert!(dropped_too_much(&input, &"слово ".repeat(55)));
    }

    #[test]
    fn short_dictation_is_never_judged() {
        // Three of ten words dropped — a 30% loss that says nothing at this
        // length, so the guard must abstain rather than fall back.
        let input = "ну э-э короче надо бы завтра встретиться уже наконец";
        assert_eq!(
            kept_word_ratio(input, "Короче, надо бы завтра встретиться."),
            None
        );
        assert!(!dropped_too_much(
            input,
            "Короче, надо бы завтра встретиться."
        ));
    }

    #[test]
    fn growing_the_text_is_allowed() {
        let output = format!("{TIDY_INPUT} и ещё немного сверху для верности вот так");
        assert!(!dropped_too_much(TIDY_INPUT, &output));
        assert!(kept_word_ratio(TIDY_INPUT, &output).unwrap() > 1.0);
    }

    #[test]
    fn an_empty_answer_is_caught() {
        assert!(dropped_too_much(&"слово ".repeat(100), ""));
    }

    /// Ровно на пороге длины проверка должна судить, а не воздерживаться.
    #[test]
    fn exactly_forty_words_is_judged() {
        let input = "слово ".repeat(40);
        let output = "слово ".repeat(28);
        // 40 слов — это MIN_WORDS_TO_JUDGE: на границе отношение уже
        // осмысленно и обязано вернуться как Some, а не None.
        assert!(kept_word_ratio(&input, &output).is_some());
    }

    /// Ровно 70 % — граница допуска: такой ответ сохраняется, а не режется.
    #[test]
    fn exactly_seventy_percent_is_kept() {
        let input = "слово ".repeat(100);
        let output = "слово ".repeat(70);
        assert!(!dropped_too_much(&input, &output));
    }
}
