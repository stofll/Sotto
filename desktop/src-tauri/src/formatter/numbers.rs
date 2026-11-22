//! Opt-in, deterministic formatting of Russian numerical values and dimensions.

use super::FormatStep;
use regex::Regex;
use std::{ops::Range, sync::LazyLock};

static TOKENS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"[\p{L}\p{N}_]+(?:[.,][0-9]+)?|×").expect("valid number token pattern")
});
static NUMERIC_IDENTIFIERS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b[0-9]+(?:[.,:][0-9]+){2,}\b|\b[0-9]+:[0-9]+\b|\b[0-9]+(?:[.,][0-9]+)?[ \t]*(?:[-–—]|\.{2,}|…)[ \t]*[0-9]+(?:[.,][0-9]+)?\b")
        .expect("valid numeric identifier pattern")
});

/// Keep a spoken measurement's "мм" from being mistaken for a hesitation.
/// Sentence punctuation is not stripped: "пять. мм" can start a new utterance.
pub(super) fn preceded_by_number(text: &str) -> bool {
    text.split_whitespace().next_back().is_some_and(|word| {
        let word = word.to_lowercase();
        small(&word).is_some()
            || scale(&word).is_some()
            || denominator(&word).is_some()
            || literal(&word)
    })
}

/// Repeated axes are meaningful even when a chain is outside the normalization
/// grammar. Keep them away from repetition cleanup without inferring a value.
pub(super) fn dimension_spans(text: &str) -> Vec<Range<usize>> {
    let tokens = tokenize(text);
    let mut spans = Vec::new();
    let mut cursor = 0;
    while cursor < tokens.len() {
        if !number_word(&tokens[cursor].word) {
            cursor += 1;
            continue;
        }
        let start = cursor;
        let mut axes = 0;
        loop {
            axes += 1;
            cursor = number_run(text, &tokens, cursor);
            if let Some((end, _)) = following_unit(text, &tokens, cursor) {
                cursor = end;
            }
            if tokens.get(cursor).is_some_and(|separator| {
                dimension_separator(&separator.word)
                    && next_to(text, &tokens[cursor - 1], separator)
                    && tokens.get(cursor + 1).is_some_and(|next| {
                        number_word(&next.word) && next_to(text, separator, next)
                    })
            }) {
                cursor += 1;
            } else {
                break;
            }
        }
        if axes >= 2 {
            spans.push(tokens[start].span.start..tokens[cursor - 1].span.end);
        }
    }
    spans
}

pub(super) struct RussianNumbers {
    pub enabled: bool,
    pub normalize_numbers: bool,
    pub normalize_dimensions: bool,
    pub language: Option<String>,
}

impl FormatStep for RussianNumbers {
    fn name(&self) -> &str {
        "Russian numbers and dimensions"
    }

    fn description(&self) -> &str {
        "Write explicit Russian numerical values as digits and format dimensions"
    }

    fn enabled(&self) -> bool {
        self.enabled
            && (self.normalize_numbers || self.normalize_dimensions)
            && matches!(self.language.as_deref(), Some("ru" | "auto"))
    }

    fn set_enabled(&mut self, value: bool) {
        self.enabled = value;
    }

    fn protected_spans(&self, text: &str) -> Vec<Range<usize>> {
        // The generic cleanup mask also hides "8 мм". This step must see that
        // measurement to format "8 на 8 мм", while preserving technical text.
        let mut spans = crate::text_protection::technical_spans(text);
        spans.extend(NUMERIC_IDENTIFIERS.find_iter(text).map(|m| m.range()));
        spans
    }

    fn apply_unprotected(&self, text: &str) -> String {
        if !self.enabled() {
            return text.to_owned();
        }
        let tokens = tokenize(text);
        let mut result = String::with_capacity(text.len());
        let mut copied = 0;
        let mut cursor = 0;
        while cursor < tokens.len() {
            if !number_word(&tokens[cursor].word) || attached(text, &tokens[cursor]) {
                cursor += 1;
                continue;
            }
            if self.normalize_dimensions {
                if let Some((end, value)) = dimension(text, &tokens, cursor) {
                    result.push_str(&text[copied..tokens[cursor].span.start]);
                    result.push_str(&value);
                    copied = tokens[end - 1].span.end;
                    cursor = end;
                    continue;
                }
            }
            let end = number_run(text, &tokens, cursor);
            if self.normalize_numbers && !unsupported_fraction(text, &tokens, end) {
                if let Some(value) = parse_number(&tokens[cursor..end]) {
                    let (end, unit) = following_unit(text, &tokens, end)
                        .map_or((end, None), |(end, unit)| (end, Some(unit)));
                    result.push_str(&text[copied..tokens[cursor].span.start]);
                    result.push_str(&value);
                    if let Some(unit) = unit {
                        result.push(' ');
                        result.push_str(unit.symbol);
                    }
                    copied = tokens[end - 1].span.end;
                    cursor = end;
                    continue;
                }
            }
            // Do not turn an invalid phrase such as "двадцать третьего" into
            // a valid suffix after rejecting its first token.
            cursor = end;
        }
        result.push_str(&text[copied..]);
        result
    }
}

struct Token {
    span: Range<usize>,
    word: String,
}

fn tokenize(text: &str) -> Vec<Token> {
    TOKENS
        .find_iter(text)
        .map(|m| Token {
            span: m.range(),
            word: m.as_str().to_lowercase(),
        })
        .collect()
}

fn dimension_separator(word: &str) -> bool {
    matches!(word, "на" | "х" | "x" | "×")
}

fn horizontal_space(text: &str) -> bool {
    !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_whitespace() && !matches!(c, '\n' | '\r'))
}

fn next_to(text: &str, left: &Token, right: &Token) -> bool {
    horizontal_space(&text[left.span.end..right.span.start])
}

fn attached(text: &str, token: &Token) -> bool {
    let before = text[..token.span.start].chars().next_back();
    let after = text[token.span.end..].chars().next();
    // Preserve hyphenated ranges, fractions, identifiers and compound words.
    let connected = |c: char| c.is_alphanumeric() || matches!(c, '_' | '-' | '–' | '/' | '\\');
    before.is_some_and(connected) || after.is_some_and(connected)
}

fn number_run(text: &str, tokens: &[Token], start: usize) -> usize {
    let mut end = start + 1;
    while end < tokens.len()
        && next_to(text, &tokens[end - 1], &tokens[end])
        && number_word(&tokens[end].word)
    {
        end += 1;
    }
    end
}

fn unsupported_fraction(text: &str, tokens: &[Token], end: usize) -> bool {
    tokens.get(end).is_some_and(|next| {
        next_to(text, &tokens[end - 1], next)
            && (matches!(next.word.as_str(), "половина" | "половиной" | "четверть")
                || (matches!(next.word.as_str(), "с" | "и")
                    && tokens.get(end + 1).is_some_and(|after| {
                        next_to(text, next, after)
                            && matches!(
                                after.word.as_str(),
                                "половиной" | "половина" | "четверть" | "четвертью"
                            )
                    })))
    })
}

fn small(word: &str) -> Option<u64> {
    Some(match word {
        "ноль" | "нуль" => 0,
        "один" | "одна" | "одно" => 1,
        "два" | "две" => 2,
        "три" => 3,
        "четыре" => 4,
        "пять" => 5,
        "шесть" => 6,
        "семь" => 7,
        "восемь" => 8,
        "девять" => 9,
        "десять" => 10,
        "одиннадцать" => 11,
        "двенадцать" => 12,
        "тринадцать" => 13,
        "четырнадцать" => 14,
        "пятнадцать" => 15,
        "шестнадцать" => 16,
        "семнадцать" => 17,
        "восемнадцать" => 18,
        "девятнадцать" => 19,
        "двадцать" => 20,
        "тридцать" => 30,
        "сорок" => 40,
        "пятьдесят" => 50,
        "шестьдесят" => 60,
        "семьдесят" => 70,
        "восемьдесят" => 80,
        "девяносто" => 90,
        "сто" => 100,
        "двести" => 200,
        "триста" => 300,
        "четыреста" => 400,
        "пятьсот" => 500,
        "шестьсот" => 600,
        "семьсот" => 700,
        "восемьсот" => 800,
        "девятьсот" => 900,
        _ => return None,
    })
}

fn scale(word: &str) -> Option<u64> {
    Some(match word {
        "тысяча" | "тысячи" | "тысяч" => 1_000,
        "миллион" | "миллиона" | "миллионов" => 1_000_000,
        "миллиард" | "миллиарда" | "миллиардов" => 1_000_000_000,
        _ => return None,
    })
}

fn denominator(word: &str) -> Option<usize> {
    Some(match word {
        "десятая" | "десятых" => 1,
        "сотая" | "сотых" => 2,
        "тысячная" | "тысячных" => 3,
        _ => return None,
    })
}

fn literal(word: &str) -> bool {
    let mut separator = false;
    !word.is_empty()
        && word.chars().all(|c| {
            if matches!(c, '.' | ',') && !separator {
                separator = true;
                true
            } else {
                c.is_ascii_digit()
            }
        })
        && word.starts_with(|c: char| c.is_ascii_digit())
        && word.ends_with(|c: char| c.is_ascii_digit())
}

fn number_word(word: &str) -> bool {
    small(word).is_some()
        || scale(word).is_some()
        || denominator(word).is_some()
        || literal(word)
        || matches!(word, "минус" | "плюс" | "целая" | "целых" | "запятая")
        // Unsupported inflections and ordinals make the whole adjacent run
        // ineligible, rather than partially rewriting dates or identifiers.
        || ordinal(word)
        || [
            "пят", "шест", "сем", "восьм", "восем", "девят", "десят", "одиннадцат",
            "двенадцат", "тринадцат", "четырнадцат", "пятнадцат", "шестнадцат",
            "семнадцат", "восемнадцат", "девятнадцат", "двадцат", "тридцат",
        ]
        .iter()
        .any(|stem| word.strip_prefix(stem).is_some_and(|ending| matches!(ending, "и" | "ью")))
        || matches!(
            word,
            "одного" | "одной" | "одному" | "одним" | "одном" | "одну" | "двух" | "двум"
                | "двумя" | "трех" | "трёх" | "трем" | "трём" | "тремя" | "четырех"
                | "четырёх" | "четырем" | "четырём" | "четырьмя" | "ста" | "двухсот"
                | "трехсот" | "трёхсот" | "четырехсот" | "четырёхсот" | "сотня" | "сотни"
                | "сотен" | "тысяче" | "тысячей" | "тысячу" | "тысячам" | "тысячами"
                | "тысячах" | "миллиону" | "миллионом" | "миллионе" | "миллионам"
                | "миллионами" | "миллионах" | "миллиарду" | "миллиардом" | "миллиарде"
                | "миллиардам" | "миллиардами" | "миллиардах"
        )
}

fn ordinal(word: &str) -> bool {
    [
        "перв",
        "втор",
        "трет",
        "треть",
        "четверт",
        "четвёрт",
        "пят",
        "шест",
        "седьм",
        "восьм",
        "девят",
        "десят",
        "одиннадцат",
        "двенадцат",
        "тринадцат",
        "четырнадцат",
        "пятнадцат",
        "шестнадцат",
        "семнадцат",
        "восемнадцат",
        "девятнадцат",
        "двадцат",
        "тридцат",
        "сороков",
        "пятидесят",
        "шестидесят",
        "семидесят",
        "восьмидесят",
        "девяност",
        "сот",
        "тысячн",
        "миллионн",
        "миллиардн",
    ]
    .iter()
    .any(|stem| {
        word.strip_prefix(stem).is_some_and(|ending| {
            matches!(
                ending,
                "ый" | "ой"
                    | "ий"
                    | "ая"
                    | "яя"
                    | "ое"
                    | "ее"
                    | "ые"
                    | "ие"
                    | "ого"
                    | "его"
                    | "ому"
                    | "ему"
                    | "ую"
                    | "юю"
                    | "ых"
                    | "их"
                    | "ым"
                    | "им"
                    | "ыми"
                    | "ими"
                    | "ом"
                    | "ем"
            )
        })
    })
}

fn under_thousand(tokens: &[Token]) -> Option<u64> {
    if tokens.is_empty() {
        return None;
    }
    let mut cursor = 0;
    let mut result = 0;
    if let Some(value) = small(&tokens[0].word).filter(|&v| v >= 100) {
        result = value;
        cursor += 1;
    }
    if let Some(value) = tokens.get(cursor).and_then(|t| small(&t.word)) {
        if (20..100).contains(&value) {
            result += value;
            cursor += 1;
            if let Some(value) = tokens
                .get(cursor)
                .and_then(|t| small(&t.word))
                .filter(|&v| (1..10).contains(&v))
            {
                result += value;
                cursor += 1;
            }
        } else if value < 20 && (value != 0 || tokens.len() == 1) {
            result += value;
            cursor += 1;
        }
    }
    (cursor == tokens.len()).then_some(result)
}

fn cardinal(tokens: &[Token]) -> Option<u64> {
    if tokens.len() == 1 && tokens[0].word.bytes().all(|c| c.is_ascii_digit()) {
        return tokens[0].word.parse().ok();
    }
    let mut start = 0;
    let mut last_scale = u64::MAX;
    let mut result = 0;
    for (i, token) in tokens.iter().enumerate() {
        if let Some(unit) = scale(&token.word) {
            if unit >= last_scale {
                return None;
            }
            let coefficient = if start == i {
                // A bare singular scale is one; an omitted coefficient on a
                // later scale is not a valid cardinal phrase.
                if i != 0 || !matches!(token.word.as_str(), "тысяча" | "миллион" | "миллиард")
                {
                    return None;
                }
                1
            } else {
                under_thousand(&tokens[start..i])?
            };
            if coefficient == 0 {
                return None;
            }
            result += coefficient * unit;
            start = i + 1;
            last_scale = unit;
        }
    }
    if start < tokens.len() {
        let remainder = under_thousand(&tokens[start..])?;
        if start > 0 && remainder == 0 {
            return None;
        }
        result += remainder;
    } else if tokens.is_empty() {
        return None;
    }
    Some(result)
}

fn parse_number(tokens: &[Token]) -> Option<String> {
    let (sign, tokens) = match tokens.first()?.word.as_str() {
        "минус" => ("-", &tokens[1..]),
        "плюс" => ("+", &tokens[1..]),
        _ => ("", tokens),
    };
    if tokens.len() == 1 && literal(&tokens[0].word) {
        return Some(format!("{sign}{}", tokens[0].word));
    }
    let number = if let Some(comma) = tokens.iter().position(|t| t.word == "запятая") {
        let whole = cardinal(&tokens[..comma])?;
        let fraction = &tokens[comma + 1..];
        if fraction.is_empty() {
            return None;
        }
        let digits: Option<String> = fraction
            .iter()
            .map(|t| {
                small(&t.word)
                    .filter(|&v| v < 10)
                    .map(|v| char::from(b'0' + v as u8))
            })
            .collect();
        let fraction = if fraction.len() == 1
            && fraction[0].word.bytes().all(|c| c.is_ascii_digit())
            && fraction[0].word.len() <= 9
        {
            fraction[0].word.clone()
        } else if let Some(digits) = digits {
            if digits.len() > 9 {
                return None;
            }
            digits
        } else {
            let value = cardinal(fraction)?;
            if value >= 1_000_000_000 {
                return None;
            }
            value.to_string()
        };
        format!("{whole},{fraction}")
    } else if let Some(places) = tokens.last().and_then(|t| denominator(&t.word)) {
        let end = tokens.len() - 1;
        let (whole, fraction) = if let Some(marker) = tokens[..end]
            .iter()
            .position(|t| matches!(t.word.as_str(), "целая" | "целых"))
        {
            (cardinal(&tokens[..marker])?, &tokens[marker + 1..end])
        } else {
            (0, &tokens[..end])
        };
        let fraction = cardinal(fraction)?;
        if fraction >= 10_u64.pow(places as u32) {
            return None;
        }
        format!("{whole},{fraction:0places$}")
    } else {
        cardinal(tokens)?.to_string()
    };
    Some(format!("{sign}{number}"))
}

#[derive(Clone, Copy)]
struct Unit {
    symbol: &'static str,
    length: bool,
}

fn unit(word: &str) -> Option<Unit> {
    let (symbol, length) = match word {
        "мм" | "миллиметр" | "миллиметра" | "миллиметров" => {
            ("мм", true)
        }
        "см" | "сантиметр" | "сантиметра" | "сантиметров" => {
            ("см", true)
        }
        "м" | "метр" | "метра" | "метров" => ("м", true),
        "мкм" | "микрометр" | "микрометра" | "микрометров" => {
            ("мкм", true)
        }
        "мл" | "миллилитр" | "миллилитра" | "миллилитров" => {
            ("мл", false)
        }
        "л" | "литр" | "литра" | "литров" => ("л", false),
        "мг" | "миллиграмм" | "миллиграмма" | "миллиграммов" => {
            ("мг", false)
        }
        "мкг" | "микрограмм" | "микрограмма" | "микрограммов" => {
            ("мкг", false)
        }
        "г" | "грамм" | "грамма" | "граммов" => ("г", false),
        "кг" | "килограмм" | "килограмма" | "килограммов" => {
            ("кг", false)
        }
        "процент" | "процента" | "процентов" => ("%", false),
        _ => return None,
    };
    Some(Unit { symbol, length })
}

fn following_unit(text: &str, tokens: &[Token], end: usize) -> Option<(usize, Unit)> {
    let next = tokens.get(end)?;
    if next_to(text, &tokens[end - 1], next) && !attached(text, next) {
        unit(&next.word).map(|unit| (end + 1, unit))
    } else {
        None
    }
}

fn dimension_cue(text: &str, tokens: &[Token], start: usize) -> bool {
    start.checked_sub(1).is_some_and(|previous| {
        matches!(
            tokens[previous].word.as_str(),
            "размер" | "размеры" | "размером" | "размерами"
        ) && text[tokens[previous].span.end..tokens[start].span.start]
            .chars()
            .all(|c| c == ':' || (c.is_whitespace() && !matches!(c, '\n' | '\r')))
    })
}

fn dimension(text: &str, tokens: &[Token], start: usize) -> Option<(usize, String)> {
    // A rejected longer or mixed-unit chain must not become a valid suffix.
    if start >= 2
        && dimension_separator(&tokens[start - 1].word)
        && next_to(text, &tokens[start - 1], &tokens[start])
        && next_to(text, &tokens[start - 2], &tokens[start - 1])
        && (number_word(&tokens[start - 2].word) || unit(&tokens[start - 2].word).is_some())
    {
        return None;
    }
    let mut cursor = start;
    let mut values = Vec::with_capacity(3);
    let mut common_unit = None;
    let mut last_has_unit;
    loop {
        let end = number_run(text, tokens, cursor);
        if unsupported_fraction(text, tokens, end) || attached(text, &tokens[end - 1]) {
            return None;
        }
        let value = parse_number(&tokens[cursor..end])?;
        if value.starts_with(['-', '+']) {
            return None;
        }
        values.push(value);
        cursor = end;
        last_has_unit = false;
        if let Some((end, next_unit)) = following_unit(text, tokens, cursor) {
            if !next_unit.length
                || common_unit.is_some_and(|unit: Unit| unit.symbol != next_unit.symbol)
            {
                return None;
            }
            common_unit = Some(next_unit);
            last_has_unit = true;
            cursor = end;
        }
        if tokens.get(cursor).is_some_and(|token| {
            dimension_separator(&token.word) && next_to(text, &tokens[cursor - 1], token)
        }) {
            if values.len() == 3 {
                return None;
            }
            let next = tokens.get(cursor + 1)?;
            if !number_word(&next.word) || !next_to(text, &tokens[cursor], next) {
                return None;
            }
            cursor += 1;
        } else {
            break;
        }
    }
    if values.len() < 2
        || (common_unit.is_some() && !last_has_unit)
        || (common_unit.is_none() && !dimension_cue(text, tokens, start))
    {
        return None;
    }
    let mut result = values.join(" х ");
    if let Some(unit) = common_unit {
        result.push(' ');
        result.push_str(unit.symbol);
    }
    Some((cursor, result))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(numbers: bool, dimensions: bool) -> RussianNumbers {
        RussianNumbers {
            enabled: true,
            normalize_numbers: numbers,
            normalize_dimensions: dimensions,
            language: Some("ru".into()),
        }
    }

    fn assert_cases(step: &RussianNumbers, cases: &[(&str, &str)]) {
        for (input, expected) in cases {
            let output = step.apply(input);
            assert_eq!(&output, expected, "{input}");
            assert_eq!(step.apply(&output), output, "idempotence: {input}");
        }
    }

    #[test]
    fn formats_strict_cardinals_and_signed_values() {
        assert_cases(
            &step(true, false),
            &[
                ("тридцать пять", "35"),
                ("Сто двадцать три узла", "123 узла"),
                ("одна тысяча двести тридцать четыре", "1234"),
                ("два миллиона три тысячи семь", "2003007"),
                ("тысяча один", "1001"),
                ("миллиард двести миллионов", "1200000000"),
                ("ноль и нуль", "0 и 0"),
                ("минус сорок два", "-42"),
                ("плюс пять", "+5"),
                ("два образца, затем три", "2 образца, затем 3"),
                ("пятно пять миллиметров", "пятно 5 мм"),
                ("вторник, пятница: двадцать", "вторник, пятница: 20"),
            ],
        );
    }

    #[test]
    fn explicit_decimal_grammar_preserves_place_value() {
        assert_cases(
            &step(true, false),
            &[
                ("восемь целых пять десятых", "8,5"),
                ("одна целая пять сотых", "1,05"),
                ("ноль целых семь тысячных", "0,007"),
                ("тридцать пять сотых", "0,35"),
                ("ноль запятая восемь", "0,8"),
                ("ноль запятая ноль пять", "0,05"),
                ("минус две целых двадцать пять сотых", "-2,25"),
                ("восемь запятая двадцать пять", "8,25"),
                ("ноль запятая 05", "0,05"),
                ("минус ноль запятая пять", "-0,5"),
                ("8,8 мм", "8,8 мм"),
                ("8.8 миллиметров", "8.8 мм"),
            ],
        );
    }

    #[test]
    fn abbreviates_units_without_converting_values() {
        assert_cases(
            &step(true, false),
            &[
                ("пять миллиграммов", "5 мг"),
                ("одна целая пять десятых миллилитра", "1,5 мл"),
                ("двадцать процентов", "20 %"),
                ("сорок два микрограмма", "42 мкг"),
                ("два килограмма", "2 кг"),
            ],
        );
    }

    #[test]
    fn dimensions_work_independently_of_general_numbers() {
        assert_cases(
            &step(false, true),
            &[
                (
                    "размерами тридцать пять на тридцать пять миллиметров",
                    "размерами 35 х 35 мм",
                ),
                ("узел восемь на восемь миллиметров", "узел 8 х 8 мм"),
                ("35 на 35 мм", "35 х 35 мм"),
                ("8,8 на 9,1 на 10 мм", "8,8 х 9,1 х 10 мм"),
                (
                    "одна целая пять десятых на две целых пять десятых сантиметра",
                    "1,5 х 2,5 см",
                ),
                ("размер: восемь на восемь", "размер: 8 х 8"),
                ("три миллиметра на четыре миллиметра", "3 х 4 мм"),
                ("35 x 35 см", "35 х 35 см"),
                ("35 × 35 см", "35 х 35 см"),
                (
                    "пять и шесть, размером два на три метра",
                    "пять и шесть, размером 2 х 3 м",
                ),
            ],
        );
    }

    #[test]
    fn never_invents_dimensions_from_decimals_or_other_uses_of_na() {
        for input in [
            "8,8 мм",
            "восемь целых восемь десятых миллиметра",
            "три на четыре этаже",
            "пять на пять",
            "два на три процента",
            "два на три миллилитра",
            "восемь на восемь на восемь на восемь мм",
            "восемь мм на два см",
            "минус восемь на восемь мм",
            "-8 на 8 мм",
            "два на три дня",
            "версия 1.2.3 на четыре мм",
            "10:35 на пять мм",
            "два см на три образца",
        ] {
            assert_eq!(step(false, true).apply(input), input, "{input}");
        }
    }

    #[test]
    fn ambiguous_or_unsupported_numeric_phrases_are_preserved_whole() {
        for input in [
            "один два три",
            "двадцать тридцать",
            "сто сто",
            "двадцать третьего ноября",
            "две тысячи двадцать первого года",
            "ноль пять",
            "тысячи",
            "две тысячи миллион",
            "два с половиной",
            "пять целых",
            "одна целая пятнадцать десятых",
            "двадцать пять сотен",
            "пять-шесть",
            "8...9 мм",
            "1 000 миллиметров",
            "версия 1.2.3",
            "пять/шесть",
            "код восемь восемь",
            "сто ноль",
        ] {
            assert_eq!(step(true, true).apply(input), input, "{input}");
        }
    }

    #[test]
    fn preserves_technical_text_and_statement_boundaries() {
        assert_cases(
            &step(true, true),
            &[
                (
                    "`тридцать пять на тридцать пять мм` и пять мм",
                    "`тридцать пять на тридцать пять мм` и 5 мм",
                ),
                (
                    "https://test/пять файл_пять a@пять.рф",
                    "https://test/пять файл_пять a@пять.рф",
                ),
                ("двадцать\nпять", "20\n5"),
                ("двадцать. Пять", "20. 5"),
                ("модель T2 пять", "модель T2 5"),
            ],
        );
    }

    #[test]
    fn opt_in_and_language_gate_are_obeyed() {
        let input = "восемь на восемь миллиметров";
        let mut disabled = step(true, true);
        disabled.enabled = false;
        assert_eq!(disabled.apply(input), input);
        assert_eq!(step(false, false).apply(input), input);
        for language in [None, Some("en"), Some("uk"), Some("ru-RU")] {
            let mut step = step(true, true);
            step.language = language.map(str::to_owned);
            assert_eq!(step.apply(input), input);
        }
        let mut auto = step(true, true);
        auto.language = Some("auto".into());
        assert_eq!(auto.apply(input), "8 х 8 мм");
    }

    #[test]
    fn spoken_measurements_keep_their_abbreviated_unit() {
        for text in [
            "тридцать пять ",
            "ПЯТЬ\t",
            "полторы тысячи ",
            "семь десятых ",
            "8,8 ",
        ] {
            assert!(preceded_by_number(text), "{text}");
        }
        for text in [
            "",
            " ",
            "думаю ",
            "пять. ",
            "пять, ",
            "данные_5 ",
            "пять-шесть ",
        ] {
            assert!(!preceded_by_number(text), "{text}");
        }
    }

    #[test]
    fn dimension_protection_keeps_repeated_axes_and_mixed_units() {
        for chain in [
            "тридцать пять на тридцать пять на тридцать пять миллиметров",
            "восемь на восемь на восемь на восемь мм",
            "два миллиметра на три сантиметра",
            "два на два на два",
            "8,8 x 8,8 × 8,8 мм",
        ] {
            let text = format!("размеры {chain}, проверено");
            let spans = dimension_spans(&text);
            assert_eq!(spans.len(), 1, "{text}");
            assert_eq!(&text[spans[0].clone()], chain, "{text}");
        }
        for text in [
            "я думаю что я думаю что",
            "на столе на столе на столе",
            "пять миллиметров и пять миллиметров",
            "два на столе и три на столе",
        ] {
            assert!(dimension_spans(text).is_empty(), "{text}");
        }
    }
}
