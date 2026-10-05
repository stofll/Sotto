//! Guard against an LLM that retells the dictation instead of tidying it up.
//!
//! The built-in presets ask the model to preserve the author's words. This
//! independent response check also applies to custom prompts, so an answer
//! that drops large passages cannot silently reach the clipboard as a clean-up.
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

/// A change that reads better and says something else.
///
/// The word ratio cannot see these: dropping one «never» from a ten-word
/// prompt, or renumbering a list that started at 5, leaves the length intact
/// and inverts what the user asked for. Each check is narrow enough that an
/// edit the prompt permits does not trip it, and a false alarm only costs the
/// punctuation the local transcript lacks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Alteration {
    /// Fewer negations came back than were dictated.
    Negation,
    /// A number, amount or list number from the dictation is missing.
    Numbers,
    /// A Latin-script name or term inside Russian text was changed.
    Terms,
}

impl Alteration {
    /// The `skipped_reason` code the history and its labels use.
    pub fn reason(self) -> &'static str {
        match self {
            Self::Negation => "model_dropped_negation",
            Self::Numbers => "model_changed_numbers",
            Self::Terms => "model_changed_terms",
        }
    }
}

/// The first meaning-changing edit found in `output`, if any. Unlike the word
/// ratio this applies at any length: a short prompt is where one lost «not»
/// matters most.
pub fn altered_meaning(input: &str, output: &str) -> Option<Alteration> {
    if negation_count(output) < negation_count(input) {
        return Some(Alteration::Negation);
    }
    let kept_numbers = numbers(output);
    if numbers(input)
        .iter()
        .any(|number| !kept_numbers.contains(number))
    {
        return Some(Alteration::Numbers);
    }
    if mostly_cyrillic(input) {
        let kept_terms = latin_terms(output);
        if latin_terms(input)
            .iter()
            .any(|term| !kept_terms.contains(term))
        {
            return Some(Alteration::Terms);
        }
    }
    None
}

/// Lowercased word tokens with apostrophes kept, so `don't` stays one token.
fn word_tokens(text: &str) -> Vec<String> {
    text.split(|c: char| !(c.is_alphanumeric() || c == '\'' || c == '’'))
        .map(|token| {
            token
                .trim_matches(['\'', '’'])
                .replace('’', "'")
                .to_lowercase()
        })
        .filter(|token| !token.is_empty())
        .collect()
}

// Speech language: the words compared, not UI text.
const RUSSIAN_NEGATIONS: &[&str] = &["не", "нет", "ни", "никогда", "нельзя"];
const ENGLISH_NEGATIONS: &[&str] = &["not", "no", "never", "cannot", "nothing", "nobody", "none"];

/// Negations, counting `n't` contractions and ignoring back-to-back repeats:
/// «не не надо» → «не надо» is a removed stutter the prompt allows.
fn negation_count(text: &str) -> usize {
    let mut tokens = word_tokens(text);
    tokens.dedup();
    tokens
        .iter()
        .filter(|token| {
            RUSSIAN_NEGATIONS.contains(&token.as_str())
                || ENGLISH_NEGATIONS.contains(&token.as_str())
                || token.ends_with("n't")
        })
        .count()
}

/// Digit sequences, with thousands groups and decimal separators folded away
/// so `1500`, `1 500` and `1,500` compare equal.
fn numbers(text: &str) -> std::collections::HashSet<String> {
    static NUMBER: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"\d+(?:[ \u{00A0}\u{202F}.,]\d{3})*(?:[.,]\d+)?").expect("valid regex")
    });
    NUMBER
        .find_iter(text)
        .map(|found| {
            found
                .as_str()
                .chars()
                .filter(char::is_ascii_digit)
                .collect()
        })
        .collect()
}

/// In Russian dictation a Latin word is a name, a brand or a term — the words
/// the prompt says to keep as written. In English text every word is Latin,
/// and the edits the prompt allows would trip this check, so it stays off.
fn mostly_cyrillic(text: &str) -> bool {
    let words = word_tokens(text);
    let cyrillic = words
        .iter()
        .filter(|word| word.chars().any(|c| matches!(c, 'а'..='я' | 'ё')))
        .count();
    cyrillic * 2 > words.len()
}

/// Latin words of two letters or more, compared without case: «github» →
/// `GitHub` is a fix, `Wispr` → `Whisper` is a different product.
fn latin_terms(text: &str) -> std::collections::HashSet<String> {
    word_tokens(text)
        .into_iter()
        .filter(|token| {
            token.chars().filter(char::is_ascii_alphabetic).count() >= 2
                && token
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '\'')
        })
        .collect()
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

    /// Exactly at the length threshold the check must judge, not abstain.
    #[test]
    fn exactly_forty_words_is_judged() {
        let input = "слово ".repeat(40);
        let output = "слово ".repeat(28);
        // 40 words is MIN_WORDS_TO_JUDGE: at the boundary the ratio is already
        // meaningful and must come back as Some, not None.
        assert!(kept_word_ratio(&input, &output).is_some());
    }

    /// Exactly 70 % is the tolerance boundary: such an answer is kept, not cut.
    #[test]
    fn exactly_seventy_percent_is_kept() {
        let input = "слово ".repeat(100);
        let output = "слово ".repeat(70);
        assert!(!dropped_too_much(&input, &output));
    }

    /// One word under the length threshold is still too short to judge, even
    /// when most of those words are gone.
    #[test]
    fn thirty_nine_words_are_not_judged() {
        let input = "слово ".repeat(39);
        let output = "слово ".repeat(10);
        assert_eq!(kept_word_ratio(&input, &output), None);
        assert!(!dropped_too_much(&input, &output));
    }

    /// One word under the 70 % line is a retelling, not an allowed tidy-up.
    #[test]
    fn sixty_nine_of_a_hundred_words_is_dropped() {
        let input = "слово ".repeat(100);
        assert!(dropped_too_much(&input, &"слово ".repeat(69)));
    }

    // The cases below are the published failures of cloud clean-up: a list
    // that started at 5, a product name, an amount and a negation.

    #[test]
    fn a_dropped_negation_is_caught() {
        for (input, output) in [
            (
                "I'd prefer to never merge this",
                "I'd prefer to merge this.",
            ),
            ("I don't understand", "I understand."),
            ("я не хочу это мержить", "Я хочу это мержить."),
            ("это никогда не сработает", "Это не сработает."),
        ] {
            assert_eq!(
                altered_meaning(input, output),
                Some(Alteration::Negation),
                "{input} → {output}"
            );
        }
    }

    #[test]
    fn negations_may_change_form_and_lose_stutters() {
        for (input, output) in [
            ("I don't understand", "I do not understand."),
            ("we can not ship it", "We cannot ship it."),
            ("ну не не надо так делать", "Не надо так делать."),
            ("no no no wait", "No, wait."),
        ] {
            assert_eq!(altered_meaning(input, output), None, "{input} → {output}");
        }
    }

    #[test]
    fn a_renumbered_list_is_caught() {
        assert_eq!(
            altered_meaning(
                "пункт 5 проверить логи пункт 6 перезапустить сервис",
                "1. Проверить логи.\n2. Перезапустить сервис."
            ),
            Some(Alteration::Numbers)
        );
    }

    #[test]
    fn a_changed_amount_is_caught_and_grouping_is_not() {
        let input = "переведи 1500 рублей до 3.5 процентов";
        assert_eq!(
            altered_meaning(input, "Переведи 15 000 рублей до 3,5 процентов."),
            Some(Alteration::Numbers)
        );
        assert_eq!(
            altered_meaning(input, "Переведи 1 500 рублей до 3,5 процентов."),
            None
        );
    }

    #[test]
    fn spelling_out_digits_from_words_is_allowed() {
        assert_eq!(altered_meaning("купи пять яблок", "Купи 5 яблок."), None);
    }

    #[test]
    fn a_changed_product_name_is_caught_in_russian_text() {
        assert_eq!(
            altered_meaning(
                "отправь это в Wispr Flow и в Superwhisper",
                "Отправь это в Whisper Flow и в Superwhisper."
            ),
            Some(Alteration::Terms)
        );
        assert_eq!(
            altered_meaning("залей в github и открой pr", "Залей в GitHub и открой PR."),
            None
        );
    }

    #[test]
    fn english_text_may_lose_fillers() {
        assert_eq!(
            altered_meaning(
                "um so like we ship it on friday",
                "So we ship it on Friday."
            ),
            None
        );
    }
}
