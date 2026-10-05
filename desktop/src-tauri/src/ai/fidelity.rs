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
/// and inverts what the user asked for. The checks tolerate stutters, changes
/// of form and number formatting, but they compare words, not meaning: an
/// abandoned false start or a spoken self-correction that removes a negation
/// or a number also trips them. A false alarm costs only the punctuation the
/// local transcript lacks.
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
    if !digits_survive(input, output) {
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

/// Words compared at the start of an answer to spot an echoed context.
const CONTEXT_PROBE_WORDS: usize = 6;

/// Opening words of a part that must follow a short echo for it to count.
const PART_ANCHOR_WORDS: usize = 3;

/// True when the answer for a part of a long text opens with words of the
/// context sent before it rather than with the part: the model tidied text it
/// was told only to read, and joining it would repeat that text. Either the
/// answer starts inside the context, or it restates the end of a sentence the
/// cut broke — «мы решили» before a part that starts «перенести релиз».
pub fn repeats_context(context: &str, input: &str, output: &str) -> bool {
    let output = word_tokens(output);
    let input = word_tokens(input);
    let context = word_tokens(context);
    let starts_with =
        |words: &[String], prefix: &[String]| words.get(..prefix.len()) == Some(prefix);
    if let Some(probe) = output.get(..CONTEXT_PROBE_WORDS) {
        if !starts_with(&input, probe)
            && context
                .windows(CONTEXT_PROBE_WORDS)
                .any(|window| window == probe)
        {
            return true;
        }
    }
    let anchor = &input[..input.len().min(PART_ANCHOR_WORDS)];
    !anchor.is_empty()
        && (1..=CONTEXT_PROBE_WORDS.min(context.len())).any(|echo| {
            let restated = &context[context.len() - echo..];
            starts_with(&output, restated)
                && !starts_with(&input, restated)
                && starts_with(&output[echo..], anchor)
        })
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

/// Negations, counting `n't` contractions. A negation that repeats one of
/// the two words before it is a stutter the prompt lets the model remove —
/// «не не надо», «не ну не надо» — and counts once; «не хочу и не буду»
/// keeps both.
fn negation_count(text: &str) -> usize {
    let tokens = word_tokens(text);
    let is_negation = |token: &str| {
        RUSSIAN_NEGATIONS.contains(&token)
            || ENGLISH_NEGATIONS.contains(&token)
            || token.ends_with("n't")
    };
    tokens
        .iter()
        .enumerate()
        .filter(|(index, token)| {
            is_negation(token) && !tokens[index.saturating_sub(2)..*index].contains(token)
        })
        .count()
}

/// Maximal runs of ASCII digits, in order.
fn digit_runs(text: &str) -> Vec<&str> {
    text.split(|c: char| !c.is_ascii_digit())
        .filter(|run| !run.is_empty())
        .collect()
}

/// Every digit of `input`, in order, is spelled by whole numbers of `output`.
///
/// Grouping, phone formatting, ranges and decimal commas move separators
/// between digits, so the digits of the source are compared as one stream:
/// `8 900 123 45 67` matches `8 (900) 123-45-67`, `1500` matches `1 500`.
/// That stream must be the concatenation of some of the answer's numbers,
/// taken whole and in order, so `1500` → `15000` and a list renumbered from
/// `5, 6` to `1, 2` fail. Numbers the answer adds — «пять» written as `5` —
/// are skipped.
fn digits_survive(input: &str, output: &str) -> bool {
    let source: String = digit_runs(input).concat();
    if source.is_empty() {
        return true;
    }
    // Offsets into `source` that a prefix of the answer's numbers can reach.
    let mut reached = vec![false; source.len() + 1];
    reached[0] = true;
    for run in digit_runs(output) {
        for start in (0..source.len()).rev() {
            if reached[start] && source[start..].starts_with(run) {
                reached[start + run.len()] = true;
            }
        }
    }
    reached[source.len()]
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
            ("я не хочу и не буду", "Я хочу и не буду."),
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
            ("не ну не надо так делать", "Ну не надо так делать."),
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
    fn number_formatting_is_allowed() {
        for (input, output) in [
            (
                "позвони на 8 900 123 45 67",
                "Позвони на 8 (900) 123-45-67.",
            ),
            ("с 2019 2020 года", "С 2019–2020 года."),
            ("купи 100 200 300 штук", "Купи 100, 200, 300 штук."),
            ("встреча в 10.30", "Встреча в 10:30."),
            ("версия 0 3 2", "Версия 0.3.2."),
        ] {
            assert_eq!(altered_meaning(input, output), None, "{input} → {output}");
        }
    }

    #[test]
    fn a_reordered_or_merged_number_is_caught() {
        assert_eq!(
            altered_meaning("сначала 5 потом 6", "Сначала 6, потом 5."),
            Some(Alteration::Numbers)
        );
        assert_eq!(
            altered_meaning("ровно 1500", "Ровно 15000."),
            Some(Alteration::Numbers)
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

    const CONTEXT: &str = "Вчера мы обсудили релиз и решили перенести его на пятницу.";
    const PART: &str = "потом надо обновить документацию и проверить установщик на маке";

    #[test]
    fn an_answer_that_opens_with_the_context_is_caught() {
        let echoed = format!(
            "Мы обсудили релиз и решили перенести его на пятницу. {}",
            "Потом надо обновить документацию и проверить установщик на маке."
        );
        assert!(repeats_context(CONTEXT, PART, &echoed));
    }

    /// A cut inside a sentence: the model restates its start before the part.
    #[test]
    fn a_restated_sentence_start_is_caught() {
        let context = "и потом мы решили";
        let part = "перенести релиз на пятницу и обновить документацию";
        let restated = "Мы решили перенести релиз на пятницу и обновить документацию.";
        assert!(repeats_context(context, part, restated));
        let tidied = "Перенести релиз на пятницу и обновить документацию.";
        assert!(!repeats_context(context, part, tidied));
    }

    #[test]
    fn a_tidied_part_is_not_an_echo() {
        let tidied = "Потом надо обновить документацию и проверить установщик на маке.";
        assert!(!repeats_context(CONTEXT, PART, tidied));
    }

    /// A part that itself starts by repeating the words before it, as speech
    /// does, keeps its tidy-up.
    #[test]
    fn a_part_that_repeats_its_context_itself_is_kept() {
        let part = "решили перенести его на пятницу да и потом надо обновить документацию";
        let tidied = "Решили перенести его на пятницу, да. И потом надо обновить документацию.";
        assert!(!repeats_context(CONTEXT, part, tidied));
    }
}
