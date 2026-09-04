//! Local text post-processing pipeline (Phase 4 / Batch 2 / PR 2.2).
//!
//! A 1:1 Rust port of `transcription/text_formatter.py`. Each
//! `FormatStep` has a stable Python counterpart (see the doc comment on
//! each step); the parity tests in `tests/formatter_parity_test.rs`
//! replay the Python test suite's fixtures against this module.
//!
//! Pipeline
//! ---------
//!
//! `Formatter::process` applies the steps in a fixed order (the same
//! order the Python `TextFormatter` uses). Toggling a step off in the
//! config flips the corresponding `enabled` flag, but the order is
//! preserved. The order matters — for example, hallucination cleanup
//! must run BEFORE filler removal, otherwise a sign-off like
//! "DimaTorzok" could be partially consumed by the filler pattern
//! (lowercase letter "a" + "a" inside "DimaTorzok") and lose the
//! signature.
//!
//! Replacement rules
//! -----------------
//!
//! `apply_replacement_rules` mirrors Python's behaviour:
//!   * `match=word` → word-boundary regex (Rust: `(?<!\w){find}(?!\w)`)
//!   * `match=phrase` → literal, no boundary
//!   * `match=contains` → literal, no boundary (legacy alias for phrase)
//!   * `match=regex` → user-supplied regex
//!   * `case_sensitive=false` → IGNORECASE
//!   * `preserve_case=true` → keep all-uppercase / first-letter-uppercase
//!
//! `preview_format` is the Tauri command for the live Settings preview;
//! `preview_replacements` is the Tauri command for the rule-list live
//! preview. Both are sync (no I/O) so they can run inline.

use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

// ---------------------------------------------------------------------------
// Defaults
// ---------------------------------------------------------------------------

/// Default Russian parasite / filler words. Mirrors Python's
/// `DEFAULT_PARASITE_WORDS` exactly — the parity tests rely on this.
const DEFAULT_PARASITE_WORDS: &[&str] = &[
    "ну",
    "типа",
    "как бы",
    "в общем",
    "короче",
    "это самое",
    "значит",
    "собственно",
    "так сказать",
    "понимаешь",
    "понимаете",
    "блин",
    "чё",
    "че",
    "короч",
    "вот",
    "да",
    "нет",
];

/// Default filler-sound regex patterns (э-э, ммм, а-а, …). Each is
/// compiled lazily by `default_filler_patterns()` to keep the module
/// loading order side-effect-free (Rust forbids `Lazy<Regex>` with a
/// non-const initialiser at the top level).
fn default_filler_patterns() -> Vec<Regex> {
    [
        r"\b(э+[-\s]*)+\b",
        r"\b(м+[-\s]*)+\b",
        r"\b(а+[-\s]*)+\b",
        r"\b(о+[-\s]*)+\b",
        r"\bну-+у*\b",
        r"\bмм-+\b",
    ]
    .iter()
    .map(|pattern| Regex::new(pattern).expect("valid filler pattern"))
    .collect()
}

/// Tier 1 — strong hallucination signatures. Drop a segment that merely
/// CONTAINS one of these (these phrases do not occur in genuine
/// dictation, so a partial match is already conclusive). The `(?i)`
/// prefix is required because Whisper's hallucinations come back in
/// mixed case ("DimaTorzok", "Субтитры", …) and the user-facing text is
/// usually capitalised.
fn hallucination_strong() -> Vec<Regex> {
    [
        // Russian subtitle-credit family. "Субтитры сделал DimaTorzok"
        // is by far the most common Russian silence artifact; the
        // "Редактор субтитров А.Семкин / Корректор А.Егорова" pair is
        // the second.
        r"(?i)\bdima\s*torzok\b",
        r"(?i)\bдима\s*торжок\b",
        r"(?i)субтитр\w*\b.{0,40}?(?:сделал|создавал|делал|подготов|редактир|правил|коррект|перевод)",
        r"(?i)(?:сделал|создавал|подготов|редактир)\w*\s+субтитр",
        r"(?i)\bредактор\s+субтитр",
        r"(?i)\bкорректор\s+[a-zа-яё]\.\s*[a-zа-яё]+",
        r"(?i)субтитр\w*\s+(?:и\s+)?перевод",
        // Subtitle-community credits (English / service names).
        r"(?i)\bamara\.org\b",
        r"(?i)\bsubtitles?\s+(?:by|provided\s+by)\b",
        // Channel-promo outros.
        r"(?i)подпис\w*\s+на\s+(?:наш\s+)?канал",
        r"(?i)ставь\w*\s+лайк",
        r"(?i)\bподпишись\b.{0,20}\bканал",
        r"(?i)жми\w*\s+(?:на\s+)?колокольчик",
        r"(?i)\blike\s+and\s+subscribe\b",
        r"(?i)\bdon'?t\s+forget\s+to\s+subscribe\b",
        // Language leakage: on silence Whisper sometimes mis-detects the
        // language and emits that language's stock outro. Japanese
        // "thank you for watching" is the one that shows up in practice.
        r"ご視聴ありがとうござい",
    ]
    .iter()
    .map(|pattern| Regex::new(pattern).expect("valid hallucination pattern"))
    .collect()
}

/// Tier 2 — generic sign-offs. Only drop a segment whose ENTIRE content
/// is the phrase, so "Спасибо за просмотр документов, я всё проверил"
/// survives. The `(?i)` flag MUST appear at the very start of the
/// pattern; the Rust regex crate rejects inline flag groups inside an
/// alternation wrapper.
fn hallucination_generic() -> Vec<Regex> {
    [
        r"(?i)^\W*спасибо\s+за\s+просмотр\W*$",
        r"(?i)^\W*спасибо\s+за\s+внимание\W*$",
        r"(?i)^\W*спасибо,?\s+что\s+смотрите\W*$",
        r"(?i)^\W*продолжение\s+следует\W*$",
        r"(?i)^\W*до\s+новых\s+встреч\W*$",
        r"(?i)^\W*всем\s+пока\W*$",
        r"(?i)^\W*thanks?\s+for\s+watching\W*$",
        r"(?i)^\W*thank\s+you\s+for\s+watching\W*$",
        r"(?i)^\W*please\s+subscribe\W*$",
        r"(?i)^\W*subscribe\s+to\s+(?:my|our)\s+channel\W*$",
        r"(?i)^\W*see\s+you\s+(?:in\s+the\s+)?next\s+(?:time|video|one)\W*$",
    ]
    .iter()
    .map(|pattern| Regex::new(pattern).expect("valid sign-off pattern"))
    .collect()
}

/// Tier 3 — phrases that are hallucinations ONLY when they are all the
/// transcription contains. Whisper's canonical silence output for an
/// English-ish decode is a bare "you" or "Thank you."; both are also
/// perfectly ordinary things to say mid-dictation ("I told you.
/// Thank you."), so a per-segment drop would eat real speech.
///
/// `is_all_hallucination` therefore requires EVERY segment of the text
/// to match tier 1, 2, or 3 before any of these are removed — a
/// transcription that is nothing but sign-offs and "you" is silence, a
/// transcription that merely ends with one is not.
fn hallucination_whole_text() -> Vec<Regex> {
    [
        r"(?i)^\W*you\W*$",
        r"(?i)^\W*thank\s+you(?:\s+very\s+much)?\W*$",
        r"(?i)^\W*thanks(?:\s+a\s+lot)?\W*$",
        r"(?i)^\W*bye(?:[-\s]*bye)?\W*$",
        r"(?i)^\W*goodbye\W*$",
        r"(?i)^\W*субтитры\W*$",
        r"(?i)^\W*музыка\W*$",
        r"(?i)^\W*аплодисменты\W*$",
    ]
    .iter()
    .map(|pattern| Regex::new(pattern).expect("valid whole-text pattern"))
    .collect()
}

/// Non-speech annotations Whisper emits for music / silence / room tone:
/// `[Music]`, `[BLANK_AUDIO]`, `(upbeat music)`, `[аплодисменты]`, …
///
/// The keyword list is deliberately required — stripping every
/// bracketed run would eat legitimately dictated parentheses.
static SOUND_TAG: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?i)[\[(][^\[\]()\n]{0,40}?(?:music|silence|blank[_\s]*audio|applause|laughter|inaudible|no\s+audio|музык\w*|аплодисмент\w*|смех|тишина|неразборчиво)[^\[\]()\n]{0,40}?[\])]",
    )
    .expect("valid sound-tag pattern")
});

/// A run of musical notes, paired (`♪ la la ♪`) or bare (`♪♪♪`).
static MUSIC_NOTES: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"[♪♫]+(?:[^♪♫\n]*[♪♫]+)?").expect("valid music-note pattern"));

static COMMA_BEFORE_CONJ: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\s*,\s+(и|а|но|да|или)\s+").expect("valid comma-conj pattern"));

static DOUBLE_COMMA: Lazy<Regex> = Lazy::new(|| Regex::new(r",\s*,").expect("valid double comma"));

static LEADING_COMMA: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^\s*,\s*").expect("valid leading comma"));

static MULTI_SPACE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r" {2,}").expect("valid multi-space pattern"));

static SPACE_BEFORE_PUNCT: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\s+([,.;:!?\)])").expect("valid space-before-punct pattern"));

static SPLIT_KEYWORDS: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"\s+(и\s+(я|мы|он|она|оно|они|это|мне|нам|ему|ей|им|их|меня|нас|его|её|нас|вас)\s+|потом\s+|далее\s+|во-первых\s*[,;:]?\s*|во-вторых\s*[,;:]?\s*|в-третьих\s*[,;:]?\s*)",
    )
    .expect("valid split keyword pattern")
});

/// `\b(\w+)\s+\1\b` from Python — the Rust `regex` crate does not
/// support backreferences, so `DuplicateWordsRemover` uses the
/// `dedupe_adjacent_words` helper (whitespace-token scan) instead.
static TOKEN_BOUNDARY: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\s+").expect("valid token boundary"));

// ---------------------------------------------------------------------------
// Replacement rule model
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReplacementMatchMode {
    Word,
    Phrase,
    Contains,
    Regex,
}

/// Replacement rule. The on-disk format uses `"match"` (Python
/// reserved word); we accept both `"match"` and `"match_"` (the
/// Rust-conventional snake_case name).
#[derive(Debug, Clone, Serialize)]
pub struct ReplacementRule {
    pub id: String,
    pub find: String,
    pub replace: String,
    pub enabled: bool,
    pub match_: ReplacementMatchMode,
    pub case_sensitive: bool,
    pub preserve_case: bool,
    pub usage_count: u64,
}

impl<'de> serde::de::Deserialize<'de> for ReplacementRule {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(serde::Deserialize)]
        struct Raw {
            #[serde(default)]
            id: String,
            #[serde(default)]
            find: String,
            #[serde(default)]
            replace: String,
            #[serde(default = "default_true")]
            enabled: bool,
            #[serde(rename = "match", default)]
            match_: Option<ReplacementMatchMode>,
            #[serde(default)]
            case_sensitive: bool,
            #[serde(default)]
            preserve_case: bool,
            #[serde(default)]
            usage_count: u64,
        }
        let raw = Raw::deserialize(deserializer).map_err(serde::de::Error::custom)?;
        let id = if raw.id.is_empty() {
            format!("rule-{}", raw.find)
        } else {
            raw.id
        };
        Ok(ReplacementRule {
            id,
            find: raw.find,
            replace: raw.replace,
            enabled: raw.enabled,
            match_: raw.match_.unwrap_or(ReplacementMatchMode::Word),
            case_sensitive: raw.case_sensitive,
            preserve_case: raw.preserve_case,
            usage_count: raw.usage_count,
        })
    }
}

fn default_true() -> bool {
    true
}

impl ReplacementRule {
    /// Build a normalised rule from a raw `serde_json::Value`. Returns
    /// `None` for empty `find` (Python's `_normalize_replacement_rule`
    /// skips these).
    pub fn from_value(value: &Value, index: usize) -> Option<Self> {
        let obj = value.as_object()?;
        let find = obj.get("find")?.as_str()?.trim();
        if find.is_empty() {
            return None;
        }
        let match_str = obj.get("match").and_then(Value::as_str).unwrap_or("word");
        let match_mode = match match_str {
            "word" => ReplacementMatchMode::Word,
            "phrase" => ReplacementMatchMode::Phrase,
            "contains" => ReplacementMatchMode::Contains,
            "regex" => ReplacementMatchMode::Regex,
            _ => ReplacementMatchMode::Word,
        };
        let id = obj
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| format!("rule-{}", index + 1));
        Some(Self {
            id,
            find: find.to_string(),
            replace: obj
                .get("replace")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            enabled: obj.get("enabled").and_then(Value::as_bool).unwrap_or(true),
            match_: match_mode,
            case_sensitive: obj
                .get("case_sensitive")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            preserve_case: obj
                .get("preserve_case")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            usage_count: obj.get("usage_count").and_then(Value::as_u64).unwrap_or(0),
        })
    }
}

pub fn normalize_replacement_rules(source: Option<&Value>) -> Vec<ReplacementRule> {
    let Some(value) = source else {
        return Vec::new();
    };
    let mut rules: Vec<ReplacementRule> = Vec::new();

    if let Some(arr) = value.as_array() {
        for (i, item) in arr.iter().enumerate() {
            if let Some(rule) = ReplacementRule::from_value(item, i) {
                rules.push(rule);
            }
        }
        if !rules.is_empty() {
            return rules;
        }
    }
    if let Some(obj) = value.as_object() {
        if let Some(arr) = obj.get("replacement_rules").and_then(Value::as_array) {
            for (i, item) in arr.iter().enumerate() {
                if let Some(rule) = ReplacementRule::from_value(item, i) {
                    rules.push(rule);
                }
            }
            if !rules.is_empty() {
                return rules;
            }
        }
        // Legacy `replacements` dict fallback — only if no rules were
        // produced from the structured form.
        if rules.is_empty() {
            if let Some(legacy) = obj.get("replacements").and_then(Value::as_object) {
                for (i, (find, replace)) in legacy.iter().enumerate() {
                    let find_str = find.trim();
                    if find_str.is_empty() {
                        continue;
                    }
                    rules.push(ReplacementRule {
                        id: format!("legacy-{}", i + 1),
                        find: find_str.to_string(),
                        replace: replace.as_str().unwrap_or("").to_string(),
                        enabled: true,
                        match_: ReplacementMatchMode::Word,
                        case_sensitive: false,
                        preserve_case: false,
                        usage_count: 0,
                    });
                }
            }
        }
    }
    rules
}

// ---------------------------------------------------------------------------
// Replacement application
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize)]
pub struct ReplacementStats {
    pub total: u64,
    pub rules: Vec<ReplacementRuleMatch>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReplacementRuleMatch {
    pub id: String,
    pub find: String,
    pub replace: String,
    pub count: u64,
}

fn replacement_pattern(rule: &ReplacementRule) -> String {
    if matches!(rule.match_, ReplacementMatchMode::Regex) {
        return rule.find.clone();
    }
    let escaped = regex::escape(&rule.find);
    // Rust's `regex` crate does not support look-behind, so the
    // `match=word` case uses `\b` (unicode-aware word boundary)
    // instead of the Python `(?<!\w){find}(?!\w)`. The two are
    // equivalent for the use cases here (Cyrillic + Latin words,
    // no hyphenated compound matches).
    if matches!(rule.match_, ReplacementMatchMode::Word) {
        format!(r"\b{escaped}\b")
    } else {
        escaped
    }
}

fn preserve_replacement_case(matched: &str, replacement: &str) -> String {
    if replacement.is_empty() {
        return replacement.to_string();
    }
    if matched
        .chars()
        .all(|c| !c.is_lowercase() && c.is_alphabetic())
        && !matched.is_empty()
    {
        return replacement.to_uppercase();
    }
    // Uppercase the first CHARACTER (not byte) of the replacement
    // and concatenate the rest. Char-based slicing (not
    // byte-based) is required because Cyrillic and Latin letters
    // have different UTF-8 widths.
    let mut replacement_chars = replacement.chars();
    let first = replacement_chars.next().unwrap();
    let mut out: String = first.to_uppercase().collect();
    out.push_str(replacement_chars.as_str());
    out
}

pub fn apply_replacement_rules(
    text: &str,
    source: Option<&Value>,
    paused: bool,
) -> (String, ReplacementStats) {
    let rules = normalize_replacement_rules(source);
    let mut stats = ReplacementStats {
        total: 0,
        rules: Vec::new(),
    };
    if text.is_empty() || paused {
        return (text.to_string(), stats);
    }

    let mut current = text.to_string();
    for rule in rules.iter().filter(|r| r.enabled) {
        let pattern_src = replacement_pattern(rule);
        // Build the regex with the case-sensitive flag wired in.
        // Rust's `regex` crate does not support inline `(?i)`, so we
        // use the builder pattern. `(?-i)` resets case sensitivity
        // in case the user's `find` regex embeds its own flag.
        let pattern = match if rule.case_sensitive {
            Regex::new(&pattern_src)
        } else {
            Regex::new(&format!("(?i){pattern_src}"))
        } {
            Ok(pattern) => pattern,
            Err(error) => {
                log::warn!(
                    "formatter: invalid replacement regex skipped: {}: {error}",
                    rule.find
                );
                continue;
            }
        };

        let preserve_case = rule.preserve_case;
        let match_mode = rule.match_;
        let replace_template = rule.replace.clone();
        let id = rule.id.clone();
        let find = rule.find.clone();
        let replace_owned = rule.replace.clone();

        let mut total_count: u64 = 0;
        let new_string = pattern
            .replace_all(&current, |caps: &regex::Captures<'_>| {
                total_count += 1;
                let matched = caps.get(0).map(|m| m.as_str()).unwrap_or("");
                let mut replacement = replace_template.clone();
                if matches!(match_mode, ReplacementMatchMode::Regex) {
                    // Expand $1, $2, ... back-references from the captures.
                    // The `regex` crate does this via `replacen` with a
                    // closure; we manually substitute.
                    replacement = expand_capture_references(&replacement, caps);
                }
                if preserve_case {
                    replacement = preserve_replacement_case(matched, &replacement);
                }
                replacement
            })
            .into_owned();
        let count: u64 = total_count;
        current = new_string;
        if count > 0 {
            stats.total += count;
            stats.rules.push(ReplacementRuleMatch {
                id,
                find,
                replace: replace_owned,
                count,
            });
        }
    }
    (current, stats)
}

/// Expand `$1`, `$2`, … back-references in `template` from `caps`. The
/// whole-match `$0` and named groups `${name}` are not yet supported
/// (matches the Python `re.sub` behaviour we are paralleling — Python
/// also accepts `$0` only when a capturing group is involved; the
/// common case is positional groups).
fn expand_capture_references(template: &str, caps: &regex::Captures<'_>) -> String {
    let mut out = String::with_capacity(template.len());
    let mut chars = template.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '$' {
            let mut digits = String::new();
            while let Some(&next) = chars.peek() {
                if next.is_ascii_digit() {
                    digits.push(next);
                    chars.next();
                } else {
                    break;
                }
            }
            if let Ok(index) = digits.parse::<usize>() {
                if let Some(group) = caps.get(index) {
                    out.push_str(group.as_str());
                }
                // Out-of-range index → drop the reference (Python
                // `re.sub` leaves the placeholder in place; we drop
                // it to keep the formatter predictable).
            } else {
                out.push('$');
                out.push_str(&digits);
            }
        } else {
            out.push(ch);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Format steps
// ---------------------------------------------------------------------------

pub trait FormatStep {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn apply(&self, text: &str) -> String;
    fn enabled(&self) -> bool;
    fn set_enabled(&mut self, value: bool);
}

pub struct HallucinationCleaner {
    enabled: bool,
}

impl HallucinationCleaner {
    pub fn new(enabled: bool) -> Self {
        Self { enabled }
    }
}

fn is_hallucinated_segment(segment: &str) -> bool {
    let trimmed = segment.trim();
    if trimmed.is_empty() {
        return false;
    }
    for pat in HALLUCINATION_STRONG.iter() {
        if pat.is_match(trimmed) {
            return true;
        }
    }
    for pat in HALLUCINATION_GENERIC.iter() {
        if pat.is_match(trimmed) {
            return true;
        }
    }
    false
}

/// True when EVERY sentence of `text` is a hallucination — tier 1/2
/// signatures plus the tier-3 phrases that are only suspicious in
/// isolation. This is the "recording was pure silence" test: Whisper
/// filled the void with sign-offs and nothing else, so there is no
/// dictation to keep.
fn is_all_hallucination(text: &str) -> bool {
    let mut saw_segment = false;
    for segment in split_sentences(text) {
        let trimmed = segment.trim();
        if trimmed.is_empty() {
            continue;
        }
        saw_segment = true;
        if is_hallucinated_segment(trimmed) {
            continue;
        }
        if HALLUCINATION_WHOLE_TEXT
            .iter()
            .any(|pat| pat.is_match(trimmed))
        {
            continue;
        }
        return false;
    }
    saw_segment
}

/// Split on sentence terminators, using the same rule as the main
/// cleanup walk in `HallucinationCleaner::apply`: a terminator only ends
/// a sentence when whitespace or end-of-text follows it. Splitting on a
/// bare `.` would cut "Корректор А.Егорова" into "Корректор А" and
/// "Егорова", and neither half matches the credit pattern any more.
///
/// `apply` still does its own pass rather than reusing this, because it
/// has to preserve the separators verbatim.
fn split_sentences(text: &str) -> Vec<&str> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut segments = Vec::new();
    let mut start = 0;
    for (i, (byte, ch)) in chars.iter().enumerate() {
        let ends_sentence = if matches!(ch, '.' | '!' | '?' | '…') {
            chars
                .get(i + 1)
                .is_none_or(|(_, next)| next.is_whitespace())
        } else {
            *ch == '\n'
        };
        if ends_sentence {
            segments.push(&text[start..*byte]);
            start = byte + ch.len_utf8();
        }
    }
    if start < text.len() {
        segments.push(&text[start..]);
    }
    segments
}

// Use OnceCell / Lazy so we don't recompile the patterns on every step instantiation.
static HALLUCINATION_STRONG: Lazy<Vec<Regex>> = Lazy::new(hallucination_strong);
static HALLUCINATION_GENERIC: Lazy<Vec<Regex>> = Lazy::new(hallucination_generic);
static HALLUCINATION_WHOLE_TEXT: Lazy<Vec<Regex>> = Lazy::new(hallucination_whole_text);

/// Strip non-speech annotations (`[Music]`, `♪♪♪`, `(applause)`) from
/// anywhere in the text. Runs before segmentation so a segment that is
/// nothing but a sound tag collapses to empty and disappears.
fn strip_sound_tags(text: &str) -> String {
    let out = SOUND_TAG.replace_all(text, " ");
    let out = MUSIC_NOTES.replace_all(&out, " ");
    MULTI_SPACE.replace_all(&out, " ").trim().to_string()
}

/// True when the whole transcription is a Whisper silence artifact and
/// there is nothing worth pasting. Callers use this to skip the paste
/// entirely rather than inserting a sign-off into the user's document.
///
/// Empty input is NOT a hallucination — the caller's own empty-text
/// guard owns that case.
pub fn is_pure_hallucination(text: &str) -> bool {
    let stripped = strip_sound_tags(text);
    if text.trim().is_empty() {
        return false;
    }
    // Sound tags alone (`[BLANK_AUDIO]`) leave nothing behind.
    if stripped.is_empty() {
        return true;
    }
    is_all_hallucination(&stripped)
}

impl FormatStep for HallucinationCleaner {
    fn name(&self) -> &str {
        "Remove hallucinations"
    }
    fn description(&self) -> &str {
        "субтитры, «спасибо за просмотр», [Music] и др. артефакты Whisper"
    }
    fn apply(&self, text: &str) -> String {
        if !self.enabled || text.is_empty() {
            return text.to_string();
        }
        // Non-speech annotations first: they are not sentences, so they
        // must go before the segment walk or `[Music]` would be treated
        // as the opening of a real segment.
        let stripped = strip_sound_tags(text);
        if stripped.is_empty() {
            return stripped;
        }
        // Tier 3: an utterance made up ENTIRELY of sign-offs and bare
        // "you"/"thank you" is a silence artifact end to end. Checked
        // on the whole text (not per segment) so "I told you." keeps
        // its "you".
        if is_all_hallucination(&stripped) {
            return String::new();
        }
        let text = stripped.as_str();
        // Walk the text and find each (segment, separator) pair.
        // A "separator" is the whitespace run that follows a sentence
        // terminator (`.!?…`) or a multi-newline run. The Rust `regex`
        // crate does not support look-behind, so this is implemented
        // by hand to match Python's `_SEGMENT_SPLIT` semantics.
        let mut out = String::with_capacity(text.len());
        let mut seg_start: Option<usize> = Some(0);
        let chars: Vec<(usize, char)> = text.char_indices().collect();
        let mut i = 0;
        while i < chars.len() {
            let (byte, ch) = chars[i];
            if matches!(ch, '.' | '!' | '?' | '…') {
                // Find the run of whitespace after this terminator.
                let mut j = i + 1;
                if j < chars.len() && chars[j].1.is_whitespace() {
                    // Close out the current segment first.
                    if let Some(start) = seg_start.take() {
                        let segment = &text[start..byte + ch.len_utf8()];
                        if !is_hallucinated_segment(segment) {
                            out.push_str(segment);
                        }
                    }
                    // Skip the whitespace run.
                    while j < chars.len() && chars[j].1.is_whitespace() {
                        j += 1;
                    }
                    // Emit the separator and start a new segment.
                    let next_byte = if j < chars.len() {
                        chars[j].0
                    } else {
                        text.len()
                    };
                    out.push_str(&text[byte + ch.len_utf8()..next_byte]);
                    seg_start = Some(next_byte);
                    i = j;
                    continue;
                }
            } else if ch == '\n' {
                let mut j = i;
                while j < chars.len() && chars[j].1 == '\n' {
                    j += 1;
                }
                if j > i {
                    if let Some(start) = seg_start.take() {
                        let segment = &text[start..byte];
                        if !is_hallucinated_segment(segment) {
                            out.push_str(segment);
                        }
                    }
                    let mut end_byte = if j < chars.len() {
                        chars[j].0
                    } else {
                        text.len()
                    };
                    while j < chars.len() && chars[j].1.is_whitespace() {
                        end_byte = chars[j].0 + chars[j].1.len_utf8();
                        j += 1;
                    }
                    out.push_str(&text[byte..end_byte]);
                    seg_start = Some(end_byte);
                    i = j;
                    continue;
                }
            }
            i += 1;
        }
        // Final segment (no trailing terminator / newline).
        if let Some(start) = seg_start {
            let segment = &text[start..];
            if !is_hallucinated_segment(segment) {
                out.push_str(segment);
            }
        }
        let out = MULTI_SPACE.replace_all(&out, " ").into_owned();
        out.trim().to_string()
    }
    fn enabled(&self) -> bool {
        self.enabled
    }
    fn set_enabled(&mut self, value: bool) {
        self.enabled = value;
    }
}

pub struct FillerWordsRemover {
    enabled: bool,
    patterns: Vec<Regex>,
}

impl FillerWordsRemover {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            patterns: default_filler_patterns(),
        }
    }
}

impl FormatStep for FillerWordsRemover {
    fn name(&self) -> &str {
        "Remove fillers"
    }
    fn description(&self) -> &str {
        "э-э, ммм, а-а и подобные"
    }
    fn apply(&self, text: &str) -> String {
        if !self.enabled {
            return text.to_string();
        }
        let mut out = text.to_string();
        for pattern in &self.patterns {
            out = pattern.replace_all(&out, "").into_owned();
        }
        out = MULTI_SPACE.replace_all(&out, " ").into_owned();
        out.trim().to_string()
    }
    fn enabled(&self) -> bool {
        self.enabled
    }
    fn set_enabled(&mut self, value: bool) {
        self.enabled = value;
    }
}

/// Готовый набор терминов для словаря пользователя.
///
/// Смысл не в самих словах — их человек и сам бы вписал. Смысл в том, что
/// про словарь вспоминают слишком поздно: функция есть, покрыта тестами,
/// выведена в настройки, и всё равно остаётся пустой ровно до того дня,
/// когда термины начинают мазать. Набор в один клик снимает этот барьер.
///
/// На Whisper это к тому же работает **до** распознавания, а не только
/// после: содержимое словаря уходит в `initial_prompt` (см.
/// `custom_words_prompt`), то есть модель заранее знает, каких слов ждать,
/// и с большей вероятностью не ломает их вовсе.
pub struct DictionaryPreset {
    /// Стабильный идентификатор. Название показывает фронтенд — оно
    /// переводимое, а список слов нет.
    pub id: &'static str,
    pub words: &'static [&'static str],
}

/// Разработка: то, что произносят вслух каждый день и что движок пишет
/// кириллицей.
///
/// Отбор консервативный, и вот по каким правилам — их стоит соблюдать,
/// если набор когда-нибудь будут пополнять:
///
/// 1. Термин обязан переживать свёртку и оставаться не короче
///    [`CUSTOM_WORD_MIN_CHARS`]. `Vite` сворачивается в `vit` (немая `e`
///    отбрасывается), `Node` — в `nod`: такие слова словарь молча
///    игнорирует, и обещать их пользователю нечестно.
/// 2. Термин не должен садиться на обычное русское слово в пределах
///    бюджета правок. `buffer` сворачивается в `bufer` и отличается от
///    «буфет» ровно на одну правку — за такую подмену расплачивается
///    человек, диктующий про обед.
///
/// Оба правила проверяются тестами, а не глазами.
const PRESET_DEVELOPMENT: &[&str] = &[
    // Git и процесс
    "pull request",
    "merge request",
    "commit",
    "rebase",
    "branch",
    "checkout",
    "cherry-pick",
    "squash",
    "stash",
    "changelog",
    // Сборка и выкладка
    "pipeline",
    "deploy",
    "release",
    "rollback",
    "staging",
    "production",
    "Docker",
    "Dockerfile",
    "Kubernetes",
    "nginx",
    // Языки и инструменты
    "Rust",
    "Cargo",
    "Cargo.toml",
    "clippy",
    "rustfmt",
    "TypeScript",
    "JavaScript",
    "Python",
    "React",
    "ESLint",
    "Prettier",
    "Webpack",
    "Vitest",
    "GitHub",
    "GitLab",
    "Postgres",
    "SQLite",
    "Redis",
    // Файлы и форматы
    "package.json",
    "tsconfig.json",
    "README",
    "Markdown",
    // Понятия
    "backend",
    "frontend",
    "middleware",
    "endpoint",
    "refactor",
    "linter",
    "debug",
    "callback",
    "thread",
];

/// Наборы, которые фронтенд предлагает добавить в словарь.
pub const DICTIONARY_PRESETS: &[DictionaryPreset] = &[DictionaryPreset {
    id: "development",
    words: PRESET_DEVELOPMENT,
}];

/// Сколько правок прощается термину такой длины, чтобы он всё ещё считался
/// искажением словарной формы.
///
/// Раньше здесь стоял один относительный порог 0.8 на термины любой длины,
/// и он был неверен на обоих концах шкалы. У аббревиатуры из четырёх букв
/// («NSIS») одна неверная буква — это сразу 0.75, ниже порога: такой термин
/// не проходил никогда, сколько бы раз его в словарь ни вписали. А у
/// длинного составного («structured_log» против услышанного «структуре
/// тлок») две-три правки на тринадцати символах — очевидно то же слово, но
/// относительная мера давала 0.769 и тоже отказывала.
///
/// Абсолютный бюджет решает оба случая разом: чем длиннее термин, тем
/// дешевле обходится одна ошибка распознавания.
///
/// Верхнюю границу задаёт то, что ловить **не** нужно. «брайтер» и
/// «райдере» — это движки услышали другие слова, а не записали «writer»
/// другим алфавитом; от свёрнутого `vriter` их отделяют две и три правки на
/// шести символах. Поэтому шестибуквенному термину прощается ровно одна:
/// ошибочная замена выглядит как то, что человек якобы сам продиктовал, и
/// портит текст сильнее пропущенной.
fn edit_budget(key_len: usize) -> usize {
    match key_len {
        0..=6 => 1,
        7..=11 => 2,
        _ => 3,
    }
}

/// Считается ли свёрнутое окно искажением свёрнутого термина.
fn within_budget(folded: &str, key: &str) -> bool {
    let folded: Vec<char> = folded.chars().collect();
    let key: Vec<char> = key.chars().collect();
    // Разница длин сама по себе не меньше расстояния: отсекает очевидно
    // чужие окна до подсчёта матрицы.
    if folded.len().abs_diff(key.len()) > edit_budget(key.len()) {
        return false;
    }
    edit_distance(&folded, &key) <= edit_budget(key.len())
}

/// Короче этого нечёткое сравнение не работает: у слов на три буквы почти
/// любая опечатка даёт похожесть выше порога, и словарь начинает
/// переписывать здоровый текст.
const CUSTOM_WORD_MIN_CHARS: usize = 4;

/// Транслитерация кириллицы в латиницу.
///
/// Без неё вся затея не работает: типичный случай — человек диктует
/// английский бренд, а движок пишет его кириллицей. «Таури» и «Tauri» не
/// имеют ни одного общего символа, и посимвольное расстояние между ними
/// максимально возможное, хотя это одно и то же слово.
fn translit_char(c: char) -> &'static str {
    match c {
        'а' => "a",
        'б' => "b",
        'в' => "v",
        'г' => "g",
        'д' => "d",
        'е' | 'ё' | 'э' => "e",
        'ж' => "zh",
        'з' => "z",
        'и' | 'й' => "i",
        'к' => "k",
        'л' => "l",
        'м' => "m",
        'н' => "n",
        'о' => "o",
        'п' => "p",
        'р' => "r",
        'с' => "s",
        'т' => "t",
        'у' => "u",
        'ф' => "f",
        'х' => "h",
        'ц' => "c",
        'ч' => "ch",
        'ш' | 'щ' => "sh",
        'ы' => "y",
        'ю' => "yu",
        'я' => "ya",
        // Твёрдый и мягкий знаки звука не дают.
        'ъ' | 'ь' => "",
        _ => "",
    }
}

/// Свести слово к форме, по которой сравниваем.
///
/// Регистр не важен, пунктуация к слову не относится, кириллица уезжает в
/// латиницу, а дальше снимаются написания, различающие одно и то же
/// звучание: `ph`/`f`, `ck`/`k`, `c`/`k`, удвоенные буквы, немая `e` на
/// конце. Диграфы `sh`/`ch`/`zh` сначала прячутся в служебные символы,
/// иначе последующее `c → k` разобрало бы `ch` на части.
fn fold_for_match(word: &str) -> String {
    let mut latin = String::with_capacity(word.len());
    for c in word.chars().filter(|c| c.is_alphanumeric()) {
        for lower in c.to_lowercase() {
            if lower.is_ascii_alphanumeric() {
                latin.push(lower);
            } else {
                latin.push_str(translit_char(lower));
            }
        }
    }

    // Служебные символы для диграфов: заглавные, а строка уже в нижнем
    // регистре, так что столкнуться им не с чем.
    let stage: String = latin
        .replace("sh", "S")
        .replace("ch", "C")
        .replace("zh", "Z")
        .replace("ph", "f")
        .replace("ck", "k")
        .replace("th", "t")
        .replace("kh", "h")
        .replace("qu", "kv");

    let mut out = String::with_capacity(stage.len());
    for c in stage.chars() {
        match c {
            'c' | 'q' => out.push('k'),
            'w' => out.push('v'),
            'x' => out.push_str("ks"),
            other => out.push(other),
        }
    }

    // Удвоения на слух не слышны: «Ollama» и «олама» — одно слово.
    let mut deduped = String::with_capacity(out.len());
    for c in out.chars() {
        if !deduped.ends_with(c) {
            deduped.push(c);
        }
    }

    // Немая «e» на конце: «code» и «код» должны сойтись.
    if deduped.chars().count() > 3 && deduped.ends_with('e') {
        deduped.pop();
    }
    deduped
}

/// Расстояние Левенштейна по символам (не байтам — текст смешанный,
/// кириллица и латиница разной ширины в UTF-8).
///
/// Своя реализация вместо зависимости: тридцать строк против лишнего
/// крейта в дереве сборки.
fn edit_distance(a: &[char], b: &[char]) -> usize {
    if a.is_empty() {
        return b.len();
    }
    if b.is_empty() {
        return a.len();
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut current = vec![0usize; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        current[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            current[j + 1] = (prev[j] + cost).min(prev[j + 1] + 1).min(current[j] + 1);
        }
        std::mem::swap(&mut prev, &mut current);
    }
    prev[b.len()]
}

fn ratio(a: &str, b: &str) -> f64 {
    let ac: Vec<char> = a.chars().collect();
    let bc: Vec<char> = b.chars().collect();
    let longest = ac.len().max(bc.len());
    if longest == 0 {
        return 0.0;
    }
    1.0 - (edit_distance(&ac, &bc) as f64 / longest as f64)
}

/// Похожесть двух свёрнутых ключей.
///
/// Только полная форма. Пробовался второй сигнал — согласный скелет, где
/// у слова остаются первый символ и все согласные: он ловит заимствования,
/// разошедшиеся гласными («клод» и «claude» после него совпадают
/// буквально). Но он же склеивает несвязанные слова: «город» и «град» без
/// гласных неразличимы, и словарь с термином «Град» начинал переписывать
/// здоровый текст. Отсечь одно, не потеряв другое, порогом не вышло.
///
/// Выбор в пользу пропущенной замены: её человек увидит и поправит, а
/// подменённое слово выглядит как то, что он якобы сам продиктовал.
/// Кросс-алфавитный случай — тот, ради которого всё затевалось, — свёртка
/// закрывает и без скелета: «таури» и «Tauri» после транслитерации
/// совпадают точно.
fn similarity(a: &str, b: &str) -> f64 {
    ratio(a, b)
}

/// Перенести регистр найденного фрагмента на словарную форму.
///
/// Написание термина задаёт пользователь, поэтому оно и побеждает: «таури»
/// становится «Tauri», а не «tauri». Но если фрагмент стоял в начале
/// предложения или набран капсом, это свойство текста, а не термина, и его
/// нужно сохранить.
fn apply_case_of(matched: &str, canonical: &str) -> String {
    let letters: Vec<char> = matched.chars().filter(|c| c.is_alphabetic()).collect();
    if letters.len() > 1 && letters.iter().all(|c| c.is_uppercase()) {
        return canonical.to_uppercase();
    }
    let starts_upper = letters.first().is_some_and(|c| c.is_uppercase());
    if !starts_upper {
        return canonical.to_string();
    }
    let mut chars = canonical.chars();
    match chars.next() {
        Some(first) if first.is_lowercase() => {
            let mut out: String = first.to_uppercase().collect();
            out.push_str(chars.as_str());
            out
        }
        _ => canonical.to_string(),
    }
}

/// Приводит распознанные слова к написанию из пользовательского словаря.
///
/// Нужен потому, что движок не может знать имена, бренды, термины и жаргон
/// конкретного человека. Whisper мы дополнительно подсказываем словарём
/// через `initial_prompt` — там он влияет на само декодирование; GigaAM
/// такого входа не имеет (offline NemoCtc, hotwords в sherpa-onnx есть
/// только у transducer), и для него этот шаг — единственный способ.
///
/// Сравнение нечёткое: смысл словаря в том, чтобы ловить именно те случаи,
/// где движок услышал похоже, но записал не так.
pub struct CustomWordsCorrector {
    enabled: bool,
    /// (словарная форма, свёрнутый склеенный ключ)
    terms: Vec<(String, String)>,
}

impl CustomWordsCorrector {
    pub fn new(enabled: bool, words: Vec<String>) -> Self {
        let terms: Vec<(String, String)> = words
            .into_iter()
            .filter_map(|word| {
                let canonical = word.trim().to_string();
                if canonical.is_empty() {
                    return None;
                }
                // Ключ склеенный, без пробелов: движок сегментирует речь
                // по-своему, и «Giga AM» приходит двумя токенами там, где в
                // словаре записано одно слово «GigaAM». Сравнение по
                // склейке снимает этот вопрос целиком, а не гадает, сколько
                // слов окажется в тексте.
                let key: String = canonical.split_whitespace().map(fold_for_match).collect();
                if key.chars().filter(|c| c.is_alphanumeric()).count() < CUSTOM_WORD_MIN_CHARS {
                    return None;
                }
                Some((canonical, key))
            })
            .collect();
        Self { enabled, terms }
    }

    /// Сколько токенов текста имеет смысл пробовать за раз.
    ///
    /// Число слов в самом длинном термине плюс один: движок может разбить
    /// на один пробел больше, чем в словаре («Клод Код» против «Клодкод»).
    fn max_window(&self) -> usize {
        self.terms
            .iter()
            .map(|(canonical, _)| canonical.split_whitespace().count())
            .max()
            .unwrap_or(0)
            + 1
    }

    /// Похожесть окна `words[start .. start + n]` на конкретный термин.
    /// `None`, если окно слишком короткое, чтобы его вообще сравнивать.
    fn window_score(&self, words: &[&str], start: usize, n: usize, key: &str) -> Option<f64> {
        if n == 0 || start + n > words.len() {
            return None;
        }
        let folded: String = words[start..start + n]
            .iter()
            .map(|w| fold_for_match(w))
            .collect();
        if folded.chars().filter(|c| c.is_alphanumeric()).count() < CUSTOM_WORD_MIN_CHARS {
            return None;
        }
        Some(similarity(&folded, key))
    }

    /// Лучшее совпадение, начинающееся с позиции `start`.
    ///
    /// Возвращает длину окна и словарную форму.
    ///
    /// Составное окно принимается только если для того же термина оно
    /// похоже **строго сильнее**, чем то же окно без первого и без
    /// последнего токена. Сравнение именно с тем же термином, а не с любым:
    /// иначе «Claude» из словаря отменял бы совпадение «Claude Code», ведь
    /// он идеально ложится на первую половину окна. Без этого
    /// правила длинный термин утаскивает соседнее служебное слово: порог
    /// относительный, «на опен роутер» отличается от «openrouter» на два
    /// символа из двенадцати — то есть проходит, — и предлог исчезал из
    /// текста. Проверка симметрична, потому что прилипнуть слово может с
    /// любой стороны: «Tauri и» ломается ровно так же.
    ///
    /// При равном счёте побеждает длинное окно: иначе «Claude» съедал бы
    /// начало «Claude Code» и оставлял «код» болтаться.
    fn best_match(&self, words: &[&str], start: usize) -> Option<(usize, &str)> {
        let limit = self.max_window().min(words.len() - start);
        let mut best: Option<(f64, usize, &str)> = None;
        for n in 1..=limit {
            let folded: String = words[start..start + n]
                .iter()
                .map(|w| fold_for_match(w))
                .collect();
            if folded.chars().filter(|c| c.is_alphanumeric()).count() < CUSTOM_WORD_MIN_CHARS {
                continue;
            }
            for (canonical, key) in &self.terms {
                if !within_budget(&folded, key) {
                    continue;
                }
                // Счёт нужен и дальше — им выбирается лучший из подошедших
                // кандидатов и сравниваются вложенные окна.
                let score = similarity(&folded, key);
                if n > 1 {
                    let trimmed = self
                        .window_score(words, start + 1, n - 1, key)
                        .into_iter()
                        .chain(self.window_score(words, start, n - 1, key))
                        .fold(0.0_f64, f64::max);
                    if trimmed >= score {
                        continue;
                    }
                }
                // `<` а не `<=`: окна перебираются от коротких к длинным,
                // и равный счёт должен доставаться длинному.
                match best {
                    Some((best_score, _, _)) if score < best_score => {}
                    _ => best = Some((score, n, canonical.as_str())),
                }
            }
        }
        best.map(|(_, n, canonical)| (n, canonical))
    }
}

/// Слово вместе с разделителем, который стоял перед ним.
///
/// Разделитель хранится дословно, а не пересобирается пробелом: шаг
/// словаря не имеет права трогать разметку текста. Первая версия резала
/// вход через `split_whitespace` и склеивала через `join(" ")` — и тем
/// самым схлопывала двойные пробелы даже при выключенной нормализации, а
/// переводы строк превращала в пробелы, то есть уничтожала абзацы.
struct Token<'a> {
    gap: &'a str,
    raw: &'a str,
}

fn tokenize(text: &str) -> Vec<Token<'_>> {
    let mut tokens = Vec::new();
    let mut gap_start = 0;
    let mut word_start = None;
    for (i, c) in text.char_indices() {
        if c.is_whitespace() {
            if let Some(start) = word_start.take() {
                tokens.push(Token {
                    gap: &text[gap_start..start],
                    raw: &text[start..i],
                });
                gap_start = i;
            }
        } else if word_start.is_none() {
            word_start = Some(i);
        }
    }
    if let Some(start) = word_start {
        tokens.push(Token {
            gap: &text[gap_start..start],
            raw: &text[start..],
        });
    } else if gap_start < text.len() {
        // Хвостовой пробел без слова после него — тоже часть текста.
        tokens.push(Token {
            gap: &text[gap_start..],
            raw: "",
        });
    }
    tokens
}

/// Пунктуация до первой буквы или цифры.
fn leading_punctuation(word: &str) -> &str {
    match word.char_indices().find(|(_, c)| c.is_alphanumeric()) {
        Some((i, _)) => &word[..i],
        // Целиком пунктуация: считаем её ведущей, чтобы не продублировать
        // тот же кусок ещё и как хвост.
        None => word,
    }
}

/// Пунктуация после последней буквы или цифры.
fn trailing_punctuation(word: &str) -> &str {
    match word.char_indices().rfind(|(_, c)| c.is_alphanumeric()) {
        Some((i, c)) => &word[i + c.len_utf8()..],
        None => "",
    }
}

impl FormatStep for CustomWordsCorrector {
    fn name(&self) -> &str {
        "Custom words"
    }
    fn description(&self) -> &str {
        "имена, термины и бренды из словаря пользователя"
    }
    fn apply(&self, text: &str) -> String {
        if !self.enabled || self.terms.is_empty() {
            return text.to_string();
        }
        let tokens = tokenize(text);
        let raws: Vec<&str> = tokens.iter().map(|t| t.raw).collect();
        let mut out = String::with_capacity(text.len());
        let mut i = 0;
        while i < tokens.len() {
            match self.best_match(&raws, i) {
                Some((n, canonical)) => {
                    let first = &tokens[i];
                    let last = &tokens[i + n - 1];
                    // Кавычки и скобки вокруг термина принадлежат
                    // предложению, а не термину, — с обеих сторон. Хвост
                    // сохранялся с самого начала, а вот открывающая
                    // пунктуация терялась: «таури» превращалось в Tauri».
                    out.push_str(first.gap);
                    out.push_str(leading_punctuation(first.raw));
                    let matched = raws[i..i + n].join(" ");
                    out.push_str(&apply_case_of(&matched, canonical));
                    out.push_str(trailing_punctuation(last.raw));
                    i += n;
                }
                None => {
                    out.push_str(tokens[i].gap);
                    out.push_str(tokens[i].raw);
                    i += 1;
                }
            }
        }
        out
    }
    fn enabled(&self) -> bool {
        self.enabled
    }
    fn set_enabled(&mut self, value: bool) {
        self.enabled = value;
    }
}

pub struct ParasiteWordsRemover {
    enabled: bool,
    custom_words: Vec<String>,
}

impl ParasiteWordsRemover {
    pub fn new(enabled: bool, custom_words: Vec<String>) -> Self {
        Self {
            enabled,
            custom_words,
        }
    }
}

impl FormatStep for ParasiteWordsRemover {
    fn name(&self) -> &str {
        "Remove parasites"
    }
    fn description(&self) -> &str {
        "ну, типа, как бы, в общем..."
    }
    fn apply(&self, text: &str) -> String {
        if !self.enabled {
            return text.to_string();
        }
        let mut all: Vec<&str> = DEFAULT_PARASITE_WORDS.to_vec();
        let custom: Vec<&str> = self.custom_words.iter().map(String::as_str).collect();
        all.extend(custom);

        // Sort longest first so multi-word parasites ("как бы", "в общем")
        // match before their substrings ("как", "бы", "общем").
        let mut sorted: Vec<&str> = all;
        sorted.sort_by_key(|word| std::cmp::Reverse(word.len()));

        let mut out = text.to_string();
        for word in sorted {
            // Rust's `regex` crate does not support look-behind
            // (`(?<!\w)`), so we use `\b` word boundaries instead.
            // The `\b` form is unicode-aware and works for Cyrillic
            // words, which is what the parasite list contains.
            let pattern = format!(r"\b{}\b", regex::escape(word));
            if let Ok(re) = Regex::new(&pattern) {
                out = re.replace_all(&out, "").into_owned();
            }
        }
        out = MULTI_SPACE.replace_all(&out, " ").into_owned();
        let out = SPACE_BEFORE_PUNCT.replace_all(&out, "$1").into_owned();
        // Comma-punctuation dedup: `, ,` → `,`.
        let out = Regex::new(r"([,.;:!?])\s*,")
            .unwrap()
            .replace_all(&out, "$1,")
            .into_owned();
        out.trim().to_string()
    }
    fn enabled(&self) -> bool {
        self.enabled
    }
    fn set_enabled(&mut self, value: bool) {
        self.enabled = value;
    }
}

pub struct DuplicateWordsRemover {
    enabled: bool,
}

impl DuplicateWordsRemover {
    pub fn new(enabled: bool) -> Self {
        Self { enabled }
    }
}

impl FormatStep for DuplicateWordsRemover {
    fn name(&self) -> &str {
        "Remove duplicates"
    }
    fn description(&self) -> &str {
        "я я хочу → я хочу"
    }
    fn apply(&self, text: &str) -> String {
        if !self.enabled {
            return text.to_string();
        }
        // Iterative application: each pass removes the next
        // back-to-back duplicate. Mirrors Python's
        // `while prev != text: prev = text; text = DUP_WORD.sub(...)`.
        let mut out = text.to_string();
        let mut previous = String::new();
        while previous != out {
            previous = out.clone();
            out = dedupe_adjacent_words(&out);
        }
        out
    }
    fn enabled(&self) -> bool {
        self.enabled
    }
    fn set_enabled(&mut self, value: bool) {
        self.enabled = value;
    }
}

/// Drop a token that is identical (case-insensitive) to the previous
/// token. Mirrors Python's `\b(\w+)\s+\1\b` semantics. We split on
/// whitespace, walk the tokens, and skip the second of any adjacent
/// duplicate. The original whitespace between the duplicate and the
/// next token is collapsed.
fn dedupe_adjacent_words(text: &str) -> String {
    // Tokenize while preserving whitespace.
    let mut tokens: Vec<(String, bool)> = Vec::new(); // (text, is_word)
    let mut last = 0;
    for mat in TOKEN_BOUNDARY.find_iter(text) {
        if mat.start() > last {
            tokens.push((text[last..mat.start()].to_string(), true));
        }
        tokens.push((mat.as_str().to_string(), false));
        last = mat.end();
    }
    if last < text.len() {
        tokens.push((text[last..].to_string(), true));
    }
    // Walk and skip duplicates.
    let mut last_word: Option<String> = None;
    let mut out = String::with_capacity(text.len());
    for (tok, is_word) in tokens {
        if is_word {
            let lowered = tok.to_lowercase();
            if last_word.as_deref() == Some(lowered.as_str()) {
                continue;
            }
            last_word = Some(lowered);
        } else {
            // Whitespace separates two distinct words; clear the dedupe
            // memory only if the whitespace is a NEWLINE (paragraph
            // break). Plain spaces are still within the same sentence
            // and a duplicate across them is a real duplicate.
            if tok.contains('\n') {
                last_word = None;
            }
        }
        out.push_str(&tok);
    }
    out
}

/// Longest repeated phrase, in words, that the collapser will look for.
/// Longer runs are *safer* to collapse (nobody dictates twelve identical
/// words in a row on purpose) — the cap is only there to bound the scan.
const MAX_LOOP_PHRASE_WORDS: usize = 12;

pub struct PhraseLoopCollapser {
    enabled: bool,
}

impl PhraseLoopCollapser {
    pub fn new(enabled: bool) -> Self {
        Self { enabled }
    }
}

impl FormatStep for PhraseLoopCollapser {
    fn name(&self) -> &str {
        "Collapse phrase loops"
    }
    fn description(&self) -> &str {
        "я думаю что. я думаю что. → я думаю что."
    }
    fn apply(&self, text: &str) -> String {
        if !self.enabled {
            return text.to_string();
        }
        // Each pass collapses the loops it can see; collapsing one can
        // expose another (`a b a b c a b a b c` needs two rounds). Bounded
        // because every pass that changes anything removes tokens.
        let mut out = text.to_string();
        let mut previous = String::new();
        while previous != out {
            previous = out.clone();
            out = collapse_phrase_loops(&out);
        }
        out
    }
    fn enabled(&self) -> bool {
        self.enabled
    }
    fn set_enabled(&mut self, value: bool) {
        self.enabled = value;
    }
}

/// One word plus the whitespace that follows it, so collapsing a run can
/// put the text back together without inventing separators.
struct LoopToken {
    text: String,
    separator: String,
    /// Lowercased and stripped of surrounding punctuation. Whisper loops
    /// come back with the punctuation varying between repeats
    /// ("я думаю что. я думаю что"), so comparing raw tokens misses them.
    normalized: String,
}

/// Collapse a phrase repeated back-to-back down to a single occurrence.
///
/// This is the decoder-loop artefact that [`dedupe_adjacent_words`] cannot
/// see: that one compares single tokens, so it catches "я я хочу" but walks
/// straight past "я думаю что. я думаю что. я думаю что." It shows up on
/// long recordings with pauses for thought, where whisper has silence to
/// hallucinate into.
///
/// The first occurrence is kept verbatim — punctuation and casing included
/// — and the repeats are dropped.
fn collapse_phrase_loops(text: &str) -> String {
    let tokens = tokenize_loop_words(text);
    if tokens.len() < 4 {
        return text.to_string();
    }

    // Leading whitespace is not attached to any token.
    let prefix_len = text.len() - text.trim_start().len();
    let mut out = String::with_capacity(text.len());
    out.push_str(&text[..prefix_len]);

    let mut i = 0;
    while i < tokens.len() {
        let mut collapsed = false;
        // Shortest phrase first: a loop with period 2 must not be mistaken
        // for a period-4 one and left half-collapsed.
        for n in 2..=MAX_LOOP_PHRASE_WORDS.min((tokens.len() - i) / 2) {
            let repeats = count_phrase_repeats(&tokens, i, n);
            if repeats < min_repeats_for_phrase(n) {
                continue;
            }
            // Keep occurrence 1, drop the rest.
            let kept_end = i + n;
            let run_end = i + n * repeats;
            for index in i..kept_end {
                out.push_str(&tokens[index].text);
                // The separator after the kept phrase is the one that
                // followed the *last* repeat, so the text flows into
                // whatever came after the loop.
                if index == kept_end - 1 {
                    out.push_str(&tokens[run_end - 1].separator);
                } else {
                    out.push_str(&tokens[index].separator);
                }
            }
            i = run_end;
            collapsed = true;
            break;
        }
        if !collapsed {
            out.push_str(&tokens[i].text);
            out.push_str(&tokens[i].separator);
            i += 1;
        }
    }
    out
}

/// How many times a two-word phrase has to repeat before it counts as a
/// loop rather than as speech.
///
/// Two words landing next to themselves once is ordinary ("так так",
/// "ну ладно, ну ладно") — three in a row is not. From three words up, a
/// verbatim back-to-back repeat is already conclusive.
fn min_repeats_for_phrase(phrase_words: usize) -> usize {
    if phrase_words == 2 {
        3
    } else {
        2
    }
}

/// Consecutive occurrences of `tokens[start..start + n]` starting at
/// `start`, counting the phrase itself. Returns 1 when it does not repeat.
fn count_phrase_repeats(tokens: &[LoopToken], start: usize, n: usize) -> usize {
    // An all-punctuation "phrase" would match anything similar and is not
    // evidence of a loop.
    if tokens[start..start + n]
        .iter()
        .all(|token| token.normalized.is_empty())
    {
        return 1;
    }
    let mut repeats = 1;
    loop {
        let next = start + n * repeats;
        if next + n > tokens.len() {
            return repeats;
        }
        let matches = (0..n)
            .all(|offset| tokens[start + offset].normalized == tokens[next + offset].normalized);
        if !matches {
            return repeats;
        }
        repeats += 1;
    }
}

fn tokenize_loop_words(text: &str) -> Vec<LoopToken> {
    let mut tokens = Vec::new();
    let mut rest = text;
    rest = rest.trim_start();
    while !rest.is_empty() {
        let word_end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        let (word, tail) = rest.split_at(word_end);
        let separator_end = tail
            .find(|c: char| !c.is_whitespace())
            .unwrap_or(tail.len());
        let (separator, next) = tail.split_at(separator_end);
        tokens.push(LoopToken {
            text: word.to_string(),
            separator: separator.to_string(),
            normalized: normalize_loop_word(word),
        });
        rest = next;
    }
    tokens
}

/// Lowercase and strip surrounding punctuation, so the same word compares
/// equal however the decoder happened to punctuate that repeat.
fn normalize_loop_word(word: &str) -> String {
    word.trim_matches(|c: char| !c.is_alphanumeric())
        .to_lowercase()
}

pub struct CommaCleaner {
    enabled: bool,
}

impl CommaCleaner {
    pub fn new(enabled: bool) -> Self {
        Self { enabled }
    }
}

impl FormatStep for CommaCleaner {
    fn name(&self) -> &str {
        "Clean commas"
    }
    fn description(&self) -> &str {
        "убрать лишние запятые перед и/а/но"
    }
    fn apply(&self, text: &str) -> String {
        if !self.enabled {
            return text.to_string();
        }
        let mut out = text.to_string();
        out = COMMA_BEFORE_CONJ.replace_all(&out, " $1 ").into_owned();
        out = DOUBLE_COMMA.replace_all(&out, ",").into_owned();
        out = LEADING_COMMA.replace_all(&out, "").into_owned();
        out
    }
    fn enabled(&self) -> bool {
        self.enabled
    }
    fn set_enabled(&mut self, value: bool) {
        self.enabled = value;
    }
}

pub struct SpaceNormalizer {
    enabled: bool,
}

impl SpaceNormalizer {
    pub fn new(enabled: bool) -> Self {
        Self { enabled }
    }
}

impl FormatStep for SpaceNormalizer {
    fn name(&self) -> &str {
        "Normalize spaces"
    }
    fn description(&self) -> &str {
        "двойные пробелы, пробелы перед знаками"
    }
    fn apply(&self, text: &str) -> String {
        if !self.enabled {
            return text.to_string();
        }
        let mut out = MULTI_SPACE.replace_all(text, " ").into_owned();
        out = SPACE_BEFORE_PUNCT.replace_all(&out, "$1").into_owned();
        out.trim().to_string()
    }
    fn enabled(&self) -> bool {
        self.enabled
    }
    fn set_enabled(&mut self, value: bool) {
        self.enabled = value;
    }
}

pub struct SentenceSplitter {
    enabled: bool,
}

impl SentenceSplitter {
    pub fn new(enabled: bool) -> Self {
        Self { enabled }
    }
}

impl FormatStep for SentenceSplitter {
    fn name(&self) -> &str {
        "Split sentences"
    }
    fn description(&self) -> &str {
        "разбивать длинные предложения"
    }
    fn apply(&self, text: &str) -> String {
        if !self.enabled {
            return text.to_string();
        }
        if text.split_whitespace().count() < 20 {
            return text.to_string();
        }
        let mut out = SPLIT_KEYWORDS
            .replace_all(text, |caps: &regex::Captures| {
                let conj = caps.get(1).map(|m| m.as_str().trim()).unwrap_or("");
                if conj.is_empty() {
                    return caps[0].to_string();
                }
                // Capitalize the first letter of the conjunction.
                let mut chars = conj.chars();
                let first = chars.next().unwrap_or(' ');
                let mut upper: String = first.to_uppercase().collect();
                upper.push_str(chars.as_str());
                format!(". {} ", upper)
            })
            .into_owned();
        out = MULTI_SPACE.replace_all(&out, " ").into_owned();
        out
    }
    fn enabled(&self) -> bool {
        self.enabled
    }
    fn set_enabled(&mut self, value: bool) {
        self.enabled = value;
    }
}

pub struct ContextReplacements {
    enabled: bool,
    rules: Vec<ReplacementRule>,
}

impl ContextReplacements {
    pub fn new(enabled: bool, rules: Vec<ReplacementRule>) -> Self {
        Self { enabled, rules }
    }
}

impl FormatStep for ContextReplacements {
    fn name(&self) -> &str {
        "Text replacements"
    }
    fn description(&self) -> &str {
        "замена слов из списка"
    }
    fn apply(&self, text: &str) -> String {
        if !self.enabled || self.rules.is_empty() {
            return text.to_string();
        }
        let source = serde_json::json!({ "replacement_rules": self.rules.iter().map(|r| serde_json::json!({
            "id": r.id,
            "find": r.find,
            "replace": r.replace,
            "enabled": r.enabled,
            "match": match r.match_ {
                ReplacementMatchMode::Word => "word",
                ReplacementMatchMode::Phrase => "phrase",
                ReplacementMatchMode::Contains => "contains",
                ReplacementMatchMode::Regex => "regex",
            },
            "case_sensitive": r.case_sensitive,
            "preserve_case": r.preserve_case,
            "usage_count": r.usage_count,
        })).collect::<Vec<_>>() });
        let (out, _) = apply_replacement_rules(text, Some(&source), false);
        out
    }
    fn enabled(&self) -> bool {
        self.enabled
    }
    fn set_enabled(&mut self, value: bool) {
        self.enabled = value;
    }
}

pub struct Capitalizer {
    enabled: bool,
}

impl Capitalizer {
    pub fn new(enabled: bool) -> Self {
        Self { enabled }
    }
}

impl FormatStep for Capitalizer {
    fn name(&self) -> &str {
        "Capitalize"
    }
    fn description(&self) -> &str {
        "заглавные в начале предложений"
    }
    fn apply(&self, text: &str) -> String {
        if !self.enabled {
            return text.to_string();
        }
        let mut out = String::with_capacity(text.len());
        let mut capitalize_next = true;
        for ch in text.chars() {
            if capitalize_next && ch.is_alphabetic() {
                out.extend(ch.to_uppercase());
                capitalize_next = false;
            } else {
                out.push(ch);
            }
            if matches!(ch, '.' | '!' | '?') {
                capitalize_next = true;
            }
        }
        out
    }
    fn enabled(&self) -> bool {
        self.enabled
    }
    fn set_enabled(&mut self, value: bool) {
        self.enabled = value;
    }
}

pub struct PunctuationFinalizer {
    enabled: bool,
}

impl PunctuationFinalizer {
    pub fn new(enabled: bool) -> Self {
        Self { enabled }
    }
}

impl FormatStep for PunctuationFinalizer {
    fn name(&self) -> &str {
        "Final punctuation"
    }
    fn description(&self) -> &str {
        "точка в конце если нет знаков"
    }
    fn apply(&self, text: &str) -> String {
        if !self.enabled || text.is_empty() {
            return text.to_string();
        }
        let last = text.chars().last().unwrap();
        if matches!(last, '.' | '!' | '?' | '…') {
            return text.to_string();
        }
        let mut out = text.to_string();
        out.push('.');
        out
    }
    fn enabled(&self) -> bool {
        self.enabled
    }
    fn set_enabled(&mut self, value: bool) {
        self.enabled = value;
    }
}

// ---------------------------------------------------------------------------
// Formatter pipeline
// ---------------------------------------------------------------------------

/// Subset of the on-disk config that the formatter actually reads. We
/// don't depend on the full `Config` (it pulls in `tauri::AppHandle`)
/// so the formatter can be unit-tested without Tauri.
///
/// `Default` is hand-written (not derived) because the derived version
/// would zero every `bool` and silently disable the whole pipeline for
/// any config that predates the `text_formatting` key — which is the
/// common case, since the block is only written once the user visits
/// the Formatting page. It MUST stay in sync with `FORMAT_DEFAULTS` in
/// `desktop/src/pages/OtherPages.tsx`; the two are what the backend and
/// the settings UI each believe is on.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextFormattingConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_true")]
    pub remove_hallucinations: bool,
    #[serde(default = "default_true")]
    pub remove_fillers: bool,
    #[serde(default = "default_true")]
    pub remove_parasites: bool,
    #[serde(default = "default_true")]
    pub remove_duplicates: bool,
    #[serde(default = "default_true")]
    pub collapse_phrase_loops: bool,
    #[serde(default = "default_true")]
    pub clean_commas: bool,
    #[serde(default = "default_true")]
    pub normalize_spaces: bool,
    #[serde(default)]
    pub split_sentences: bool,
    /// Идентификаторы включённых готовых наборов ([`DICTIONARY_PRESETS`]).
    ///
    /// Хранится списком id, а не копией слов, ровно ради выключателя:
    /// набор можно снять одним движением и без потерь, а свой словарь
    /// пользователя при этом не задет — он лежит отдельно в
    /// `custom_words` и остаётся тем, что человек написал руками.
    #[serde(default)]
    pub enabled_presets: Vec<String>,
    #[serde(default = "default_true")]
    pub capitalize_sentences: bool,
    #[serde(default = "default_true")]
    pub final_punctuation: bool,
    #[serde(default)]
    pub custom_parasite_words: Vec<String>,
    /// Имена, бренды, термины и жаргон, которых движок знать не может.
    /// Пустой список — шаг не выполняется вовсе.
    #[serde(default)]
    pub custom_words: Vec<String>,
}

impl TextFormattingConfig {
    /// Словарь, который на самом деле применяется: слова пользователя плюс
    /// слова каждого включённого набора.
    ///
    /// Свои слова идут первыми и побеждают при совпадении: если человек
    /// вписал термин в своём написании, набор не имеет права подменить его
    /// своим. Сравнение без учёта регистра, иначе `Cargo` из набора и
    /// `cargo` из своего списка стали бы двумя разными терминами и оба
    /// боролись бы за одно и то же окно текста.
    pub fn effective_custom_words(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut seen: Vec<String> = Vec::new();
        let push = |word: &str, out: &mut Vec<String>, seen: &mut Vec<String>| {
            let trimmed = word.trim();
            if trimmed.is_empty() {
                return;
            }
            let key = trimmed.to_lowercase();
            if seen.contains(&key) {
                return;
            }
            seen.push(key);
            out.push(trimmed.to_string());
        };
        for word in &self.custom_words {
            push(word, &mut out, &mut seen);
        }
        for id in &self.enabled_presets {
            let Some(set) = DICTIONARY_PRESETS.iter().find(|set| set.id == id) else {
                // Набор из конфига, которого больше нет в сборке: молча
                // пропускаем. Откат приложения на версию без этого набора
                // не должен ломать форматирование.
                continue;
            };
            for word in set.words {
                push(word, &mut out, &mut seen);
            }
        }
        out
    }
}

impl Default for TextFormattingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            remove_hallucinations: true,
            remove_fillers: true,
            remove_parasites: true,
            remove_duplicates: true,
            collapse_phrase_loops: true,
            clean_commas: true,
            normalize_spaces: true,
            // Off by default: sentence splitting rewrites the user's
            // phrasing, which is a bigger intervention than the cleanup
            // steps above.
            split_sentences: false,
            capitalize_sentences: true,
            final_punctuation: true,
            custom_parasite_words: Vec::new(),
            custom_words: Vec::new(),
            enabled_presets: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FormatterConfig {
    #[serde(default)]
    pub text_formatting: TextFormattingConfig,
    #[serde(default)]
    pub replacement_rules: Vec<ReplacementRule>,
    #[serde(default)]
    pub replacements_paused: bool,
}

pub struct Formatter {
    enabled: bool,
    steps: Vec<Box<dyn FormatStep>>,
}

impl Formatter {
    /// Build a pipeline from a config. The `replacement_rules` source
    /// is taken from the explicit field, falling back to the legacy
    /// `replacements` dict inside `text_formatting` (Python parity).
    pub fn from_config(config: &FormatterConfig) -> Self {
        let fmt = &config.text_formatting;
        let rules: Vec<ReplacementRule> = if !config.replacement_rules.is_empty() {
            config.replacement_rules.clone()
        } else {
            normalize_replacement_rules(Some(&serde_json::json!({
                "replacements": config
                    .replacement_rules
                    .iter()
                    .map(|r| (r.id.clone(), r.replace.clone()))
                    .collect::<std::collections::HashMap<_, _>>(),
            })))
        };
        let replacements_enabled = !rules.is_empty() && !config.replacements_paused;

        let steps: Vec<Box<dyn FormatStep>> = vec![
            Box::new(HallucinationCleaner::new(fmt.remove_hallucinations)),
            Box::new(FillerWordsRemover::new(fmt.remove_fillers)),
            Box::new(ParasiteWordsRemover::new(
                fmt.remove_parasites,
                fmt.custom_parasite_words.clone(),
            )),
            Box::new(DuplicateWordsRemover::new(fmt.remove_duplicates)),
            // After the single-word dedupe: with "да да да да" already
            // reduced to "да", the phrase collapser cannot mistake a run of
            // one repeated word for a two-word loop.
            Box::new(PhraseLoopCollapser::new(fmt.collapse_phrase_loops)),
            Box::new(CommaCleaner::new(fmt.clean_commas)),
            Box::new(SpaceNormalizer::new(fmt.normalize_spaces)),
            // После нормализации пробелов — окна из слов режутся по
            // единичным пробелам, а не по случайным пачкам. И до правил
            // замен: пользовательское правило должно видеть уже
            // исправленный термин, а не то, что послышалось движку.
            Box::new({
                let words = fmt.effective_custom_words();
                CustomWordsCorrector::new(!words.is_empty(), words)
            }),
            Box::new(SentenceSplitter::new(fmt.split_sentences)),
            Box::new(ContextReplacements::new(replacements_enabled, rules)),
            Box::new(Capitalizer::new(fmt.capitalize_sentences)),
            Box::new(PunctuationFinalizer::new(fmt.final_punctuation)),
        ];
        Self {
            enabled: fmt.enabled,
            steps,
        }
    }

    /// Apply the pipeline. `paused` short-circuits to the trimmed
    /// input (Python parity for `TextFormatter.process` when
    /// `text_formatting.enabled = False`).
    pub fn process(&self, text: &str) -> String {
        if !self.enabled {
            return text.trim().to_string();
        }
        let mut current = text.trim().to_string();
        if current.is_empty() {
            return current;
        }
        for step in &self.steps {
            if step.enabled() {
                current = step.apply(&current);
            }
        }
        current
    }
}

// ---------------------------------------------------------------------------
// Tauri command result types
// ---------------------------------------------------------------------------

/// Returned by `preview_format`. Mirrors Python's
/// `handle_preview_format` shape: `{ original, formatted }`.
#[derive(Debug, Clone, Serialize)]
pub struct PreviewFormatResult {
    pub original: String,
    pub formatted: String,
}

/// Returned by `preview_replacements`. Mirrors Python's
/// `handle_preview_replacements` shape:
/// `{ original, result, applied_count, matched_rules }`.
#[derive(Debug, Clone, Serialize)]
pub struct PreviewReplacementsResult {
    pub original: String,
    pub result: String,
    pub applied_count: u64,
    pub matched_rules: Vec<ReplacementRuleMatch>,
}

/// Normalise a JSON value into the array form `normalize_replacement_rules`
/// expects. Accepts both a list and a `{replacement_rules: [...]}` object
/// so the Tauri command layer can forward either shape.
pub fn normalize_replacement_rules_value(value: Value) -> Value {
    if value.is_array() {
        return value;
    }
    if let Some(obj) = value.as_object() {
        if let Some(arr) = obj.get("replacement_rules") {
            if arr.is_array() {
                return arr.clone();
            }
        }
    }
    Value::Array(Vec::new())
}

/// Run the full text-formatting pipeline on `text`, building the
/// `Formatter` from a whole-config JSON `Value`. `unwrap_or_default` so a
/// malformed/partial config degrades to a disabled formatter (which just
/// trims) rather than erroring. Shared by the live dictation path
/// (`post_process_transcription` in lib.rs) and the Settings `preview_format`
/// command so the two can never diverge on how the formatter is built.
pub fn format_with_config_value(config: &Value, text: &str) -> String {
    let fmt_cfg: FormatterConfig = serde_json::from_value(config.clone()).unwrap_or_default();
    Formatter::from_config(&fmt_cfg).process(text)
}

/// Run the full text-formatting pipeline on `text` using the given
/// config. The `config` JSON must contain the `text_formatting` and
/// `replacement_rules` fields used by the formatting pipeline.
pub fn preview_format(text: &str, config: &Value) -> Result<PreviewFormatResult, String> {
    // Run the FULL formatting pipeline (hallucination/filler/parasite/
    // duplicate cleaners, comma/space normalizers, sentence splitter,
    // pre-LLM replacements, capitalizer, punctuation finalizer) — the same
    // `Formatter` the live dictation path uses.
    let formatted = format_with_config_value(config, text);
    Ok(PreviewFormatResult {
        original: text.to_string(),
        formatted,
    })
}

/// Run just the replacement-rule pass on `text` with the given rules.
/// `rules` can be an array of rule objects or an object with a
/// `replacement_rules` field. Returns the result with metadata about
/// which rules matched and how many times.
///
/// The pause flag is deliberately NOT honoured here. Pausing stops
/// replacements in live dictation; the preview exists precisely to show
/// what the rules would do, so it always applies them.
pub fn preview_replacements(
    text: &str,
    rules: &Value,
) -> Result<PreviewReplacementsResult, String> {
    let normalized = normalize_replacement_rules_value(rules.clone());
    let (result, stats) = apply_replacement_rules(text, Some(&normalized), false);
    Ok(PreviewReplacementsResult {
        original: text.to_string(),
        result,
        applied_count: stats.total,
        matched_rules: stats.rules,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// A word-mode, case-insensitive replacement rule: the shape every
/// replacement test needs, where only `find` and `replace` differ. Lives in
/// the parent module because both test modules below build one.
#[cfg(test)]
fn word_rule(find: &str, replace: &str) -> ReplacementRule {
    ReplacementRule {
        id: "r1".to_string(),
        find: find.to_string(),
        replace: replace.to_string(),
        enabled: true,
        match_: ReplacementMatchMode::Word,
        case_sensitive: false,
        preserve_case: false,
        usage_count: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_fmt() -> FormatterConfig {
        FormatterConfig {
            text_formatting: TextFormattingConfig {
                enabled: true,
                remove_hallucinations: true,
                remove_fillers: true,
                remove_parasites: true,
                remove_duplicates: true,
                collapse_phrase_loops: true,
                clean_commas: true,
                normalize_spaces: true,
                split_sentences: false,
                capitalize_sentences: true,
                final_punctuation: true,
                custom_parasite_words: Vec::new(),
                custom_words: Vec::new(),
                enabled_presets: Vec::new(),
            },
            replacement_rules: Vec::new(),
            replacements_paused: false,
        }
    }

    // ----- Phrase-loop collapsing -----

    fn collapse(text: &str) -> String {
        PhraseLoopCollapser::new(true).apply(text)
    }

    #[test]
    fn collapses_a_looped_phrase_mid_text() {
        // The decoder-loop shape this step exists for: a phrase repeating
        // back-to-back inside otherwise valid text.
        assert_eq!(
            collapse("я думаю что. я думаю что. я думаю что. на самом деле нет"),
            "я думаю что. на самом деле нет"
        );
    }

    #[test]
    fn collapses_despite_differing_punctuation_and_case() {
        // Repeats rarely come back punctuated identically.
        assert_eq!(
            collapse("Мы поедем в магазин, мы поедем в магазин мы поедем в магазин завтра"),
            "Мы поедем в магазин, завтра"
        );
    }

    #[test]
    fn collapses_a_loop_that_runs_to_the_end() {
        assert_eq!(
            collapse("текст готов спасибо за внимание спасибо за внимание"),
            "текст готов спасибо за внимание"
        );
    }

    #[test]
    fn two_word_phrase_needs_three_repeats() {
        // "ну ладно, ну ладно" is speech. Three in a row is a loop.
        assert_eq!(
            collapse("ну ладно ну ладно и поехали"),
            "ну ладно ну ладно и поехали"
        );
        assert_eq!(
            collapse("ну ладно ну ладно ну ладно и поехали"),
            "ну ладно и поехали"
        );
    }

    #[test]
    fn leaves_non_adjacent_repetition_alone() {
        // The same phrase twice in one thought is normal speech; only
        // back-to-back repetition is evidence of a decoder loop.
        let text = "я думаю что это важно и ещё я думаю что это срочно";
        assert_eq!(collapse(text), text);
    }

    #[test]
    fn leaves_ordinary_text_alone() {
        let text = "Нужно закончить этот документ сегодня и отправить его на проверку.";
        assert_eq!(collapse(text), text);
    }

    #[test]
    fn preserves_surrounding_whitespace() {
        assert_eq!(
            collapse("  привет как дела привет как дела  "),
            "  привет как дела  "
        );
    }

    #[test]
    fn collapses_nested_periods_across_passes() {
        // Period 2 inside period 5: one pass leaves work for the next, which
        // is why the step iterates to a fixed point.
        assert_eq!(collapse("а б а б а б в а б а б а б в"), "а б в");
    }

    #[test]
    fn ignores_punctuation_only_runs() {
        // "— —" normalises to empty tokens, which would otherwise match
        // anything and eat a dash.
        let text = "— — — вот так";
        assert_eq!(collapse(text), text);
    }

    #[test]
    fn disabled_collapser_is_a_no_op() {
        let text = "я думаю что. я думаю что. я думаю что.";
        assert_eq!(PhraseLoopCollapser::new(false).apply(text), text);
    }

    /// Run the collapser over a corpus of real transcriptions (one per
    /// line) and print every line it changes:
    ///
    /// ```text
    /// SOTTO_CORPUS=path/to/raw.txt cargo test --lib \
    ///     formatter::tests::collapse_over_corpus -- --ignored --nocapture
    /// ```
    ///
    /// The risk in this step is false positives on ordinary speech, and no
    /// hand-written case tells you about those — only text the user
    /// actually dictated does. Ignored by default because the corpus is not
    /// in the repo.
    #[test]
    #[ignore = "needs SOTTO_CORPUS pointing at a transcription dump"]
    fn collapse_over_corpus() {
        let Ok(path) = std::env::var("SOTTO_CORPUS") else {
            panic!("set SOTTO_CORPUS to a file with one transcription per line");
        };
        let corpus = std::fs::read_to_string(&path).expect("read corpus");
        let (mut total, mut changed) = (0usize, 0usize);
        for line in corpus.lines().filter(|line| !line.trim().is_empty()) {
            total += 1;
            let out = collapse(line);
            if out != line {
                changed += 1;
                println!("--- before: {line}\n--- after:  {out}\n");
            }
        }
        println!("{changed} of {total} transcriptions changed");
    }

    #[test]
    fn pipeline_collapses_loops_end_to_end() {
        let formatter = Formatter::from_config(&default_fmt());
        let out = formatter.process("нужно купить хлеб. нужно купить хлеб. и молоко");
        assert_eq!(out, "Нужно купить хлеб. И молоко.");
    }

    // ----- Pipeline parity with Python -----

    #[test]
    fn default_pipeline_removes_fillers_duplicates_and_finalizes() {
        let formatter = Formatter::from_config(&default_fmt());
        let out = formatter.process("эээ ну я я хочу проверить текст");
        assert_eq!(out, "Я хочу проверить текст.");
    }

    #[test]
    fn preview_format_runs_full_pipeline_not_just_replacements() {
        // Regression: `preview_format` used to call only
        // `apply_replacement_rules`, so with no custom rules the live
        // Settings/Overview preview returned the text unchanged and the
        // diff showed no differences. It must now run the whole formatter.
        let config = serde_json::json!({
            "text_formatting": {
                "enabled": true,
                "remove_fillers": true,
                "remove_parasites": true,
                "remove_duplicates": true,
                "capitalize_sentences": true,
                "final_punctuation": true,
            },
            "replacement_rules": [],
        });
        let result = preview_format("эээ ну я я хочу проверить текст", &config).unwrap();
        assert_eq!(result.formatted, "Я хочу проверить текст.");
        assert_ne!(result.formatted, result.original);
    }

    #[test]
    fn preview_format_disabled_formatting_only_trims() {
        let config = serde_json::json!({ "text_formatting": { "enabled": false } });
        let result = preview_format("  эээ ну текст  ", &config).unwrap();
        assert_eq!(result.formatted, "эээ ну текст");
    }

    #[test]
    fn removes_trailing_subtitle_hallucination() {
        let formatter = Formatter::from_config(&default_fmt());
        let out =
            formatter.process("Нужно закончить этот документ сегодня. Субтитры сделал DimaTorzok.");
        assert_eq!(out, "Нужно закончить этот документ сегодня.");
    }

    #[test]
    fn removes_cyrillic_subtitle_credit() {
        let formatter = Formatter::from_config(&default_fmt());
        let out = formatter.process("Это основной текст. Субтитры сделал Дима Торжок");
        assert_eq!(out, "Это основной текст.");
    }

    #[test]
    fn removes_generic_video_signoff() {
        let formatter = Formatter::from_config(&default_fmt());
        let out = formatter.process("Закончил на сегодня. Спасибо за просмотр!");
        assert_eq!(out, "Закончил на сегодня.");
    }

    #[test]
    fn keeps_legitimate_text_resembling_signoff() {
        let formatter = Formatter::from_config(&default_fmt());
        let out = formatter.process("Спасибо за просмотр документов, я всё проверил.");
        assert_eq!(out, "Спасибо за просмотр документов, я всё проверил.");
    }

    // ----- Whisper silence hallucinations -----

    /// Every phrase the user reported plus the rest of the canonical
    /// silence-artifact set. Each must vanish when it is the entire
    /// transcription, which is how they actually show up.
    #[test]
    fn pure_hallucinations_are_dropped_entirely() {
        let cases = [
            "Субтитры сделал DimaTorzok",
            "Субтитры создавал DimaTorzok",
            "Продолжение следует...",
            "Спасибо за просмотр!",
            "Спасибо за просмотр",
            "Спасибо за внимание!",
            "Редактор субтитров А.Семкин Корректор А.Егорова",
            "Thank you.",
            "Thanks for watching!",
            "Thank you for watching!",
            "you",
            "You.",
            "Bye-bye.",
            "Подписывайтесь на канал!",
            "Ставьте лайки и подписывайтесь на канал",
            "Subtitles by the Amara.org community",
            "ご視聴ありがとうございました",
            "[BLANK_AUDIO]",
            "[Music]",
            "♪♪♪",
            "Продолжение следует... Продолжение следует...",
            "Thank you. Thank you. Thank you.",
        ];
        let formatter = Formatter::from_config(&default_fmt());
        for case in cases {
            assert!(
                is_pure_hallucination(case),
                "should be detected as a pure hallucination: {case:?}"
            );
            assert_eq!(
                formatter.process(case),
                "",
                "formatter should empty the text: {case:?}"
            );
        }
    }

    /// The tier-3 phrases ("you", "thank you") are ordinary speech in
    /// context. They must survive whenever anything real accompanies them.
    #[test]
    fn short_generic_phrases_survive_inside_real_dictation() {
        let formatter = Formatter::from_config(&default_fmt());
        for case in [
            "I told you. Thank you.",
            "Thank you for the report, I will read it.",
            "Спасибо за просмотр документов, я всё проверил.",
        ] {
            assert!(
                !is_pure_hallucination(case),
                "must not be treated as a hallucination: {case:?}"
            );
            let out = formatter.process(case);
            assert!(
                out.to_lowercase().contains("you") || out.to_lowercase().contains("спасибо"),
                "real speech was eaten: {case:?} -> {out:?}"
            );
        }
    }

    #[test]
    fn trailing_hallucination_is_stripped_but_dictation_survives() {
        let formatter = Formatter::from_config(&default_fmt());
        let out = formatter.process("Надо купить молока. Спасибо за просмотр!");
        assert_eq!(out, "Надо купить молока.");
        assert!(!is_pure_hallucination(
            "Надо купить молока. Спасибо за просмотр!"
        ));
    }

    #[test]
    fn sound_tags_are_stripped_from_the_middle_of_speech() {
        let formatter = Formatter::from_config(&default_fmt());
        let out = formatter.process("Первая часть [Music] вторая часть");
        assert_eq!(out, "Первая часть вторая часть.");
    }

    #[test]
    fn empty_input_is_not_a_hallucination() {
        // The dispatcher's own empty-text guard owns that case; reporting
        // `true` here would make the two guards fight over the same event.
        assert!(!is_pure_hallucination(""));
        assert!(!is_pure_hallucination("   "));
    }

    #[test]
    fn hallucination_detection_respects_the_disable_toggle() {
        let mut cfg = default_fmt();
        cfg.text_formatting.remove_hallucinations = false;
        let formatter = Formatter::from_config(&cfg);
        assert_eq!(formatter.process("you"), "You.");
    }

    // ----- Config defaults -----

    #[test]
    fn absent_text_formatting_block_enables_the_pipeline() {
        // Regression: `TextFormattingConfig` derived `Default`, so a config
        // written before the `text_formatting` key existed deserialized to
        // all-false and silently disabled every cleanup step — including
        // hallucination removal — while the settings UI showed them on.
        let config = serde_json::json!({ "model": "turbo" });
        let out = format_with_config_value(&config, "Спасибо за просмотр!");
        assert_eq!(out, "");

        let defaults = TextFormattingConfig::default();
        assert!(defaults.enabled);
        assert!(defaults.remove_hallucinations);
        assert!(!defaults.split_sentences);
    }

    #[test]
    fn partial_text_formatting_block_keeps_unlisted_steps_on() {
        // Only `split_sentences` is written; the rest must fall back to the
        // same defaults the settings UI displays, not to `false`.
        let config = serde_json::json!({
            "text_formatting": { "split_sentences": true },
        });
        let out = format_with_config_value(&config, "Надо купить молока. Спасибо за просмотр!");
        assert_eq!(out, "Надо купить молока.");
    }

    #[test]
    fn explicitly_disabled_formatting_is_still_respected() {
        let config = serde_json::json!({ "text_formatting": { "enabled": false } });
        let out = format_with_config_value(&config, "  Спасибо за просмотр!  ");
        assert_eq!(out, "Спасибо за просмотр!");
    }

    #[test]
    fn can_disable_hallucination_cleanup() {
        let mut cfg = default_fmt();
        cfg.text_formatting.remove_hallucinations = false;
        let formatter = Formatter::from_config(&cfg);
        let out = formatter.process("Текст здесь. Субтитры сделал DimaTorzok.");
        assert!(
            out.contains("DimaTorzok"),
            "hallucination must survive: {out}"
        );
    }

    // ----- Replacement rule parity -----

    #[test]
    fn replacement_rules_respect_pause_flag() {
        let mut cfg = default_fmt();
        cfg.replacement_rules = vec![word_rule("шепот", "Шёпот")];
        cfg.replacements_paused = true;
        let formatter = Formatter::from_config(&cfg);
        let out = formatter.process("шепот работает");
        assert_eq!(out, "Шепот работает.");
    }

    /// Зеркало теста выше. Тот проверяет, что на паузе правило молчит, но
    /// сам по себе он остаётся зелёным и в мире, где правило не срабатывает
    /// никогда. Этот закрывает вторую половину и тем самым — выбор ветки
    /// построения правил в `from_config` и вычисление `replacements_enabled`.
    #[test]
    fn replacement_rules_apply_when_not_paused() {
        let mut cfg = default_fmt();
        cfg.replacement_rules = vec![word_rule("тайпскрипт", "TypeScript")];
        cfg.replacements_paused = false;
        let formatter = Formatter::from_config(&cfg);
        let out = formatter.process("тайпскрипт рядом");
        assert!(
            out.contains("TypeScript"),
            "правило замены обязано сработать, когда паузы нет; получено: {out}"
        );
    }

    /// Правило из старого конфига со `stage: post_llm` раньше не выполнялось
    /// нигде: конвейер применял замены только на этапе `pre_llm`, а `post_llm`
    /// передавала одна лишь команда предпросмотра. Этапы удалены, поле
    /// осталось в чужих конфигах — и оно обязано игнорироваться, то есть
    /// правило теперь срабатывает.
    #[test]
    fn legacy_stage_field_is_ignored_and_preserve_case_applies() {
        let source = serde_json::json!({
            "replacement_rules": [{
                "id": "r1",
                "find": "тайпскрипт",
                "replace": "typescript",
                "enabled": true,
                "match": "word",
                "stage": "post_llm",
                "preserve_case": true,
            }],
        });
        let (out, stats) = apply_replacement_rules("Тайпскрипт рядом", Some(&source), false);
        assert_eq!(out, "Typescript рядом");
        assert_eq!(stats.total, 1);
    }

    #[test]
    fn preview_replacements_applies_rules_even_when_paused() {
        // Pause stops replacements in live dictation. The preview exists to
        // show what the rules WOULD do, so it must ignore the flag — the UI
        // used to claim otherwise while the backend already applied them.
        let rules = serde_json::json!([{
            "id": "r1", "find": "шепот", "replace": "Шёпот",
            "enabled": true, "match": "word", "stage": "pre_llm",
        }]);
        let result = preview_replacements("шепот работает", &rules).unwrap();
        assert_eq!(result.result, "Шёпот работает");
        assert_eq!(result.applied_count, 1);
    }

    // ----- Replacement normalisation -----

    #[test]
    fn normalize_replacement_rules_preserves_structured_form() {
        let source = serde_json::json!({
            "replacement_rules": [
                {"id": "r1", "find": "foo", "replace": "bar", "enabled": true, "match": "word", "stage": "pre_llm"},
            ],
        });
        let rules = normalize_replacement_rules(Some(&source));
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].find, "foo");
    }

    #[test]
    fn normalize_replacement_rules_falls_back_to_legacy_dict() {
        let source = serde_json::json!({
            "replacements": {"a": "b"},
        });
        let rules = normalize_replacement_rules(Some(&source));
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].find, "a");
        assert_eq!(rules[0].replace, "b");
        assert_eq!(rules[0].id, "legacy-1");
    }
}

#[cfg(test)]
mod custom_words_tests {
    use super::*;

    fn correct(words: &[&str], text: &str) -> String {
        let corrector =
            CustomWordsCorrector::new(true, words.iter().map(|w| w.to_string()).collect());
        corrector.apply(text)
    }

    /// Ради этого случая всё и затевалось: движок услышал похоже, но
    /// записал кириллицей то, что пишется латиницей.
    #[test]
    fn misheard_term_gets_the_spelling_from_the_dictionary() {
        assert_eq!(
            correct(&["Tauri"], "переписал на таури вчера"),
            "переписал на Tauri вчера"
        );
    }

    /// Написание задаёт пользователь, но начало предложения принадлежит
    /// тексту, а не термину.
    #[test]
    fn a_sentence_start_keeps_its_capital() {
        assert_eq!(
            correct(&["tauri"], "Таури собирает быстро"),
            "Tauri собирает быстро"
        );
    }

    #[test]
    fn shouting_stays_shouting() {
        assert_eq!(correct(&["Tauri"], "ТАУРИ"), "TAURI");
    }

    /// Пунктуация приклеена к слову, но принадлежит предложению.
    #[test]
    fn trailing_punctuation_survives() {
        assert_eq!(correct(&["Tauri"], "собрано в таури."), "собрано в Tauri.");
        assert_eq!(correct(&["Tauri"], "таури, дальше"), "Tauri, дальше");
    }

    #[test]
    fn multi_word_terms_match_as_one() {
        assert_eq!(
            correct(&["Claude Code"], "запустил клауд код вчера"),
            "запустил Claude Code вчера"
        );
    }

    /// Длинный термин должен победить короткий, иначе «Клод Код» распадётся
    /// на «Claude» плюс мусор.
    #[test]
    fn the_longer_term_wins() {
        assert_eq!(
            correct(&["Claude", "Claude Code"], "открыл клауд код"),
            "открыл Claude Code"
        );
    }

    /// Главный риск словаря — испортить здоровый текст. Непохожие слова
    /// трогать нельзя, даже если они той же длины.
    #[test]
    fn unrelated_words_are_left_alone() {
        assert_eq!(
            correct(&["Tauri"], "тайна осталась тайной"),
            "тайна осталась тайной"
        );
        assert_eq!(
            correct(&["Ollama"], "оладьи на завтрак"),
            "оладьи на завтрак"
        );
    }

    #[test]
    fn already_correct_text_is_untouched() {
        assert_eq!(correct(&["Tauri"], "собрано в Tauri"), "собрано в Tauri");
    }

    /// «ё» и «е» на слух не различаются, и диктовка их путает.
    #[test]
    fn yo_and_ye_are_the_same_sound() {
        assert_eq!(correct(&["Шёпот"], "запустил шепот"), "запустил Шёпот");
    }

    /// Короткие термины дают слишком много ложных срабатываний, чтобы их
    /// можно было сопоставлять нечётко.
    #[test]
    fn very_short_terms_are_ignored_entirely() {
        assert_eq!(correct(&["Go"], "го дальше"), "го дальше");
    }

    #[test]
    fn an_empty_dictionary_is_a_no_op() {
        assert_eq!(correct(&[], "любой текст"), "любой текст");
        assert_eq!(correct(&["   "], "любой текст"), "любой текст");
    }

    /// Шаг выключается пустым списком, но и явный флаг должен работать.
    #[test]
    fn a_disabled_step_changes_nothing() {
        let corrector = CustomWordsCorrector::new(false, vec!["Tauri".to_string()]);
        assert_eq!(corrector.apply("таури"), "таури");
    }

    /// Собственно то, ради чего понадобилась фонетическая свёртка: слово
    /// записано другим алфавитом, общих символов ноль, а слово то же самое.
    #[test]
    fn a_latin_term_is_found_behind_cyrillic_spelling() {
        assert_eq!(fold_for_match("Tauri"), fold_for_match("таури"));
        assert_eq!(fold_for_match("Ollama"), fold_for_match("олама"));
    }

    /// Пара к сходящимся случаям выше — и она обязательна.
    ///
    /// Все остальные проверки свёртки сравнивают её выход с её же выходом:
    /// «эти два слова дают одно и то же». Такое утверждение переживает
    /// свёртку, которая всегда возвращает пустую строку, — равенство-то
    /// сохраняется. Здесь закреплено обратное: разные слова обязаны
    /// расходиться, а значит свёртка обязана что-то делать.
    #[test]
    fn folding_keeps_different_words_apart() {
        assert_ne!(fold_for_match("Tauri"), fold_for_match("тайна"));
        assert_ne!(fold_for_match("Ollama"), fold_for_match("оладьи"));
        assert_ne!(fold_for_match("город"), fold_for_match("град"));
        assert_ne!(fold_for_match("chat"), fold_for_match("шелл"));
        // И не схлопывается в пустоту: слово после свёртки остаётся словом.
        assert!(!fold_for_match("Tauri").is_empty());
        assert_eq!(fold_for_match("Tauri").chars().count(), 5);
    }

    /// Слова, похожие согласными, но не звучанием, трогать нельзя. Ровно
    /// на этой паре развалился отброшенный вариант со скелетом из
    /// согласных — см. комментарий у `similarity`.
    #[test]
    fn words_that_share_consonants_are_still_different_words() {
        assert_eq!(correct(&["Град"], "город проснулся"), "город проснулся");
        assert!(!within_budget(
            &fold_for_match("город"),
            &fold_for_match("град")
        ));
    }

    // ── Реальная диктовка ───────────────────────────────────────────────
    //
    // Четыре фразы, продиктованные на GigaAM v3 и на Whisper large-v3-turbo
    // без LLM-постобработки, с известным исходным текстом (см. #20 и #31).
    // Придуманные случаи про ложные срабатывания не рассказывают: они
    // возникают на словах, которые никому не пришло бы в голову
    // перечислить.

    /// Словарь стека, как его завёл бы человек, диктующий про свой проект.
    const REAL_DICT: [&str; 8] = [
        "Cargo.toml",
        "structured_log",
        "writer",
        "NSIS",
        "clippy",
        "pull request",
        "build-installer",
        "handle",
    ];

    fn fix(text: &str) -> String {
        correct(&REAL_DICT, text)
    }

    /// Четырёхбуквенная аббревиатура с одной неверной буквой. При старом
    /// относительном пороге это 0.75 — то есть `NSIS` не восстанавливался
    /// никогда, каким бы очевидным промах ни выглядел.
    #[test]
    fn a_short_acronym_survives_one_wrong_letter() {
        assert_eq!(fix("проверь NSYS Installer"), "проверь NSIS Installer");
        assert_eq!(fix("проверь энсис инсталлятор"), "проверь NSIS инсталлятор");
    }

    /// Длинный идентификатор, разобранный движком на два токена с потерей
    /// слога. При старом пороге — 0.769, чуть ниже отсечки.
    #[test]
    fn a_long_identifier_survives_several_errors() {
        assert_eq!(fix("В структуре тлок лежит"), "В structured_log лежит");
        assert_eq!(fix("В структуре лог лежит"), "В structured_log лежит");
    }

    /// Имя файла с точкой: движки слышат его как два слова или как одно, и
    /// оба варианта должны сойтись на словарной форме.
    #[test]
    fn a_dotted_file_name_is_restored_from_either_segmentation() {
        assert_eq!(fix("Поправь карга томол и"), "Поправь Cargo.toml и");
        assert_eq!(fix("Поправь карготомал и"), "Поправь Cargo.toml и");
    }

    /// Верхняя граница бюджета. Здесь движки услышали **другое слово**, а
    /// не записали `writer` другим алфавитом: лишняя «б» в одном случае,
    /// выдуманный предлог и другая согласная в другом. Растянуть порог до
    /// них — значит начать подменять слова, а подменённое выглядит как то,
    /// что человек якобы сам продиктовал.
    #[test]
    fn a_misheard_word_is_not_stretched_into_a_dictionary_term() {
        assert_eq!(fix("лежит брайтер он"), "лежит брайтер он");
        assert_eq!(fix("лежит в райдере, он"), "лежит в райдере, он");
    }

    /// Кириллица, которая читается, но пишется не так: основная масса
    /// технической диктовки. Ради этого словарь и нужен.
    #[test]
    fn readable_cyrillic_is_pulled_to_the_dictionary_spelling() {
        assert_eq!(fix("открой пул реквест"), "открой pull request");
        assert_eq!(fix("прогони клипи с флагом"), "прогони clippy с флагом");
        assert_eq!(fix("через билд инстеллер"), "через build-installer");
        assert_eq!(fix("держит хэндл файла"), "держит handle файла");
    }

    /// Цена узкого бюджета на коротких терминах, зафиксированная честно.
    ///
    /// Второй движок услышал то же слово как «хендел»: лишний слог уводит
    /// его от свёрнутого `handl` на три правки при пяти символах ключа.
    /// Расширять бюджет ради этого нельзя — тем же движением притянулся бы
    /// «брайтер» к `writer`. Промах человек увидит и поправит; подмена
    /// выглядит как то, что он якобы сам продиктовал.
    #[test]
    fn a_short_term_can_be_missed_when_a_whole_syllable_is_wrong() {
        assert_eq!(fix("держит хендел файла"), "держит хендел файла");
    }

    /// Слова вне словаря шаг не трогает, даже стоя вплотную к найденному
    /// термину: «денай ворнингс» человек в словарь не вносил.
    #[test]
    fn words_outside_the_dictionary_are_left_as_dictated() {
        assert_eq!(
            fix("прогони клипи с флагом денай ворнингс"),
            "прогони clippy с флагом денай ворнингс"
        );
    }

    /// Бюджет растёт от длины, но не безгранично: у шести символов он
    /// остаётся равен одному, иначе `writer` притянул бы «брайтер».
    #[test]
    fn the_budget_grows_with_length_but_stays_tight_on_short_terms() {
        assert_eq!(edit_budget(4), 1);
        assert_eq!(edit_budget(6), 1);
        assert_eq!(edit_budget(7), 2);
        assert_eq!(edit_budget(13), 3);
    }

    /// Разные написания одного звука не должны считаться разными словами.
    #[test]
    fn spelling_variants_of_one_sound_collapse() {
        assert_eq!(fold_for_match("Phil"), fold_for_match("Фил"));
        assert_eq!(fold_for_match("code"), fold_for_match("код"));
        assert_eq!(fold_for_match("Nick"), fold_for_match("Ник"));
    }

    /// Диграфы должны пережить замену `c → k`, иначе «ч» и «ш» разбираются
    /// на части и слово перестаёт совпадать само с собой.
    #[test]
    fn digraphs_survive_the_single_letter_pass() {
        assert_eq!(fold_for_match("Чат"), fold_for_match("chat"));
        assert_eq!(fold_for_match("Шелл"), fold_for_match("shell"));
    }

    /// Мягкий и твёрдый знаки звука не дают и сравнению мешать не должны.
    #[test]
    fn soft_signs_are_silent() {
        assert_eq!(fold_for_match("Гугль"), fold_for_match("гугл"));
    }

    // ── Готовые наборы ──────────────────────────────────────────────────

    /// Обычная русская речь без единого технического термина. Служит
    /// синтетической проверкой на ложные срабатывания: набор из полусотни
    /// слов не имеет права тронуть здесь ни одного слова.
    ///
    /// Слова подобраны так, чтобы бить по известным опасным местам:
    /// «буфет» рядом с `buffer`, «комитет» рядом с `commit`, «морж» рядом
    /// с `merge`, «треть» рядом с `thread`, «дебют» рядом с `debug`.
    const PLAIN_RUSSIAN: [&str; 12] = [
        "буфет закрылся на обед, пришлось идти в столовую",
        "комитет собрался в среду и ничего не решил",
        "морж вылез на берег и долго лежал на солнце",
        "треть выручки ушла на аренду помещения",
        "дебют оказался удачнее, чем все ожидали",
        "город проснулся поздно, потому что была суббота",
        "оладьи на завтрак, а к ужину обещали пирог",
        "ветка сирени перевесилась через забор",
        "прачечная работает до восьми, успеешь",
        "продукция завода расходится по всей области",
        "кран течёт вторую неделю, надо вызвать мастера",
        "передай телефон, там звонили из поликлиники",
    ];

    fn preset(id: &str) -> Vec<String> {
        DICTIONARY_PRESETS
            .iter()
            .find(|p| p.id == id)
            .unwrap_or_else(|| panic!("нет набора {id}"))
            .words
            .iter()
            .map(|w| w.to_string())
            .collect()
    }

    /// Термин, который после свёртки короче порога, словарь молча
    /// игнорирует. Обещать такой термин пользователю нечестно: он его
    /// видит в наборе и считает, что тот работает.
    ///
    /// Так отсеиваются `Vite` (→ `vit`, немая `e`) и `Node` (→ `nod`).
    #[test]
    fn every_preset_term_survives_folding() {
        for set in DICTIONARY_PRESETS {
            for word in set.words {
                let key: String = word.split_whitespace().map(fold_for_match).collect();
                let len = key.chars().filter(|c| c.is_alphanumeric()).count();
                assert!(
                    len >= CUSTOM_WORD_MIN_CHARS,
                    "«{word}» из набора {} сворачивается в «{key}» ({len} симв.) и никогда не совпадёт",
                    set.id
                );
            }
        }
    }

    #[test]
    fn preset_terms_are_unique() {
        for set in DICTIONARY_PRESETS {
            let mut seen: Vec<String> = Vec::new();
            for word in set.words {
                let lower = word.to_lowercase();
                assert!(
                    !seen.contains(&lower),
                    "«{word}» встречается в наборе {} дважды",
                    set.id
                );
                seen.push(lower);
            }
        }
    }

    /// Главный риск готового набора: человек нажал одну кнопку и получил
    /// полсотни терминов, каждый из которых может сесть на обычное слово.
    /// Свой словарь человек пишет сам и отвечает за него; набор даём мы.
    #[test]
    fn a_preset_leaves_ordinary_russian_alone() {
        for set in DICTIONARY_PRESETS {
            let corrector = CustomWordsCorrector::new(true, preset(set.id));
            for line in PLAIN_RUSSIAN {
                assert_eq!(
                    corrector.apply(line),
                    line,
                    "набор {} переписал обычную речь",
                    set.id
                );
            }
        }
    }

    /// И ради чего всё: набор должен чинить реальную диктовку без единой
    /// строчки, вписанной руками.
    #[test]
    fn the_development_preset_fixes_real_dictation() {
        let corrector = CustomWordsCorrector::new(true, preset("development"));
        assert_eq!(corrector.apply("открой пул реквест"), "открой pull request");
        assert_eq!(corrector.apply("поправь карга томол"), "поправь Cargo.toml");
        assert_eq!(corrector.apply("прогони клипи"), "прогони clippy");
    }

    // ── Включение и выключение наборов ──────────────────────────────────

    fn fmt_with(words: &[&str], presets: &[&str]) -> TextFormattingConfig {
        TextFormattingConfig {
            custom_words: words.iter().map(|w| w.to_string()).collect(),
            enabled_presets: presets.iter().map(|p| p.to_string()).collect(),
            ..TextFormattingConfig::default()
        }
    }

    /// Ради этого набор и хранится идентификатором, а не копией слов:
    /// выключение должно быть одним движением и без потерь.
    #[test]
    fn a_disabled_preset_contributes_nothing() {
        let off = fmt_with(&["Шёпот"], &[]);
        assert_eq!(off.effective_custom_words(), vec!["Шёпот".to_string()]);
    }

    #[test]
    fn an_enabled_preset_is_added_to_the_users_own_words() {
        let on = fmt_with(&["Шёпот"], &["development"]);
        let words = on.effective_custom_words();
        assert!(words.contains(&"Шёпот".to_string()), "своё слово пропало");
        assert!(
            words.contains(&"clippy".to_string()),
            "слово набора не приехало"
        );
        assert_eq!(words.len(), 1 + PRESET_DEVELOPMENT.len());
    }

    /// Свои слова идут первыми и побеждают: человек вписал термин в своём
    /// написании, и набор не имеет права подменить его своим.
    #[test]
    fn the_users_own_spelling_wins_over_the_preset() {
        let both = fmt_with(&["cargo"], &["development"]);
        let words = both.effective_custom_words();
        assert!(words.contains(&"cargo".to_string()));
        assert!(
            !words.contains(&"Cargo".to_string()),
            "набор подменил написание, выбранное пользователем"
        );
    }

    /// Откат приложения на версию без такого набора не должен ломать
    /// форматирование: неизвестный id молча пропускается.
    #[test]
    fn an_unknown_preset_id_is_ignored() {
        let stale = fmt_with(&["Шёпот"], &["набора-больше-нет"]);
        assert_eq!(stale.effective_custom_words(), vec!["Шёпот".to_string()]);
    }

    #[test]
    fn blank_lines_in_the_users_list_are_dropped() {
        let messy = fmt_with(&["  Шёпот  ", "   ", ""], &[]);
        assert_eq!(messy.effective_custom_words(), vec!["Шёпот".to_string()]);
    }

    /// Выключенный набор не должен чинить текст — иначе выключатель
    /// декоративный.
    #[test]
    fn the_correction_follows_the_toggle() {
        let off = fmt_with(&[], &[]).effective_custom_words();
        let on = fmt_with(&[], &["development"]).effective_custom_words();
        assert_eq!(
            CustomWordsCorrector::new(!off.is_empty(), off).apply("прогони клипи"),
            "прогони клипи"
        );
        assert_eq!(
            CustomWordsCorrector::new(!on.is_empty(), on).apply("прогони клипи"),
            "прогони clippy"
        );
    }

    /// Прогон словаря по реальным расшифровкам.
    ///
    /// Единственная опасность этого шага — испортить здоровый текст, и ни
    /// один придуманный случай про это не расскажет: ложное срабатывание
    /// возникает на словах, которые никому не пришло бы в голову
    /// перечислить. Отвечает на вопрос только то, что человек реально
    /// надиктовал.
    ///
    /// Словарь задаётся через `SOTTO_DICT` (по одному термину в строке),
    /// корпус — через `SOTTO_CORPUS`, как у `collapse_over_corpus`.
    ///
    /// ```bash
    /// SOTTO_CORPUS=corpus.txt SOTTO_DICT=dict.txt cargo test --lib \
    ///     formatter::custom_words_tests::dictionary_over_corpus -- --ignored --nocapture
    /// ```
    #[test]
    #[ignore = "needs SOTTO_CORPUS and SOTTO_DICT"]
    fn dictionary_over_corpus() {
        let Ok(corpus_path) = std::env::var("SOTTO_CORPUS") else {
            panic!("set SOTTO_CORPUS to a file with one transcription per line");
        };
        let Ok(dict_path) = std::env::var("SOTTO_DICT") else {
            panic!("set SOTTO_DICT to a file with one dictionary term per line");
        };
        let dict: Vec<String> = std::fs::read_to_string(&dict_path)
            .expect("read dictionary")
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_string)
            .collect();
        let corrector = CustomWordsCorrector::new(true, dict.clone());
        let corpus = std::fs::read_to_string(&corpus_path).expect("read corpus");

        let (mut total, mut changed) = (0usize, 0usize);
        for line in corpus.lines().filter(|line| !line.trim().is_empty()) {
            total += 1;
            let out = corrector.apply(line);
            if out != line {
                changed += 1;
                // Строку целиком, а не пары слов: замена может склеить два
                // токена в один, после чего попарное сравнение объявляет
                // изменившимся весь остаток строки. Первая версия этого
                // теста так и делала — и утопила настоящую находку в двух
                // сотнях ложных строк.
                println!("--- before: {line}");
                println!("--- after:  {out}");
            }
        }
        println!(
            "{changed} of {total} transcriptions changed by a {}-term dictionary",
            dict.len()
        );
    }

    /// Пунктуация обрамляет термин с обеих сторон. Хвост сохранялся с
    /// самого начала, а открывающая кавычка терялась — «таури» приезжало
    /// как Tauri».
    #[test]
    fn punctuation_survives_on_both_sides() {
        assert_eq!(
            correct(&["Tauri"], "сказал «таури» вчера"),
            "сказал «Tauri» вчера"
        );
        assert_eq!(correct(&["Tauri"], "(таури)"), "(Tauri)");
        assert_eq!(correct(&["Tauri"], "\"таури\""), "\"Tauri\"");
        assert_eq!(correct(&["Tauri"], "—таури..."), "—Tauri...");
    }

    /// Шаг словаря не имеет права переписывать разметку текста. Первая
    /// версия резала вход через split_whitespace и склеивала через
    /// join(" ") — двойные пробелы схлопывались даже при выключенной
    /// нормализации, а переводы строк превращались в пробелы, то есть
    /// абзацы пропадали.
    #[test]
    fn whitespace_is_left_exactly_as_it_was() {
        assert_eq!(
            correct(&["Tauri"], "до  двойной  пробел"),
            "до  двойной  пробел"
        );
        assert_eq!(
            correct(&["Tauri"], "первая строка\nвторая строка"),
            "первая строка\nвторая строка"
        );
        assert_eq!(correct(&["Tauri"], "абзац\n\nвторой"), "абзац\n\nвторой");
        // И то же самое, когда замена всё-таки произошла.
        assert_eq!(
            correct(&["Tauri"], "первая\n\nтаури  здесь"),
            "первая\n\nTauri  здесь"
        );
    }

    /// Ведущие и хвостовые пробелы принадлежат тексту: их снимает трим на
    /// входе в пайплайн, а не этот шаг.
    #[test]
    fn leading_and_trailing_gaps_are_preserved() {
        assert_eq!(correct(&["Tauri"], "  таури  "), "  Tauri  ");
        assert_eq!(correct(&["Tauri"], "\n таури"), "\n Tauri");
    }

    /// Токен из одной пунктуации не должен ни совпадать, ни дублироваться.
    #[test]
    fn punctuation_only_tokens_pass_through_once() {
        assert_eq!(correct(&["Tauri"], "таури — это"), "Tauri — это");
        assert_eq!(correct(&["Tauri"], "... ..."), "... ...");
    }

    #[test]
    fn distance_is_measured_in_characters_not_bytes() {
        // Кириллица — два байта на символ; побайтовое расстояние дало бы
        // здесь вдвое больший ответ и увело похожесть ниже порога.
        let a: Vec<char> = "кот".chars().collect();
        let b: Vec<char> = "кит".chars().collect();
        assert_eq!(edit_distance(&a, &b), 1);
    }

    #[test]
    fn similarity_is_symmetric_and_bounded() {
        assert!((similarity("tauri", "tauri") - 1.0).abs() < f64::EPSILON);
        assert_eq!(similarity("tauri", "таури"), similarity("таури", "tauri"));
        // Не «>= 0.0»: этот порог проходила бы и функция, всегда
        // возвращающая единицу, а вместе с ней и два утверждения выше.
        // Непохожие строки обязаны получать низкий счёт, а не любой.
        assert_eq!(similarity("abc", "xyz"), 0.0);
        assert!(similarity("tauri", "тайна") < 0.7);
    }

    // ------------------------------------------------------------------
    // Мутационное покрытие (очередь 3): правила замены, транслитерация,
    // границы сегментации.
    // ------------------------------------------------------------------

    #[test]
    fn from_value_parses_each_match_mode() {
        for (wire, expected) in [
            ("word", ReplacementMatchMode::Word),
            ("phrase", ReplacementMatchMode::Phrase),
            ("contains", ReplacementMatchMode::Contains),
            ("regex", ReplacementMatchMode::Regex),
        ] {
            let rule =
                ReplacementRule::from_value(&serde_json::json!({ "find": "x", "match": wire }), 0)
                    .expect("valid rule");
            assert_eq!(rule.match_, expected, "match mode {wire}");
        }
    }

    #[test]
    fn normalize_replacement_rules_empty_array_falls_through_to_legacy() {
        // Пустой structured-массив не должен прерывать путь до legacy-
        // словаря `replacements`: подмена `!rules.is_empty()` на `is_empty()`
        // вернула бы пусто.
        let src = serde_json::json!({ "replacement_rules": [], "replacements": { "а": "б" } });
        let rules = normalize_replacement_rules(Some(&src));
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].find, "а");
    }

    #[test]
    fn preserve_case_uppercases_all_caps_matches() {
        assert_eq!(preserve_replacement_case("HELLO", "world"), "WORLD");
    }

    #[test]
    fn preserve_case_capitalises_first_char_otherwise() {
        assert_eq!(preserve_replacement_case("Hello", "world"), "World");
    }

    #[test]
    fn paused_skips_replacement() {
        let source = serde_json::json!([{ "find": "hello", "replace": "hi" }]);
        let (out, _) = apply_replacement_rules("hello world", Some(&source), true);
        assert_eq!(out, "hello world", "paused must skip replacement");
    }

    #[test]
    fn non_matching_rule_records_no_stats() {
        let source = serde_json::json!([{ "find": "xyzzy", "replace": "x" }]);
        let (_, stats) = apply_replacement_rules("hello", Some(&source), false);
        assert!(
            stats.rules.is_empty(),
            "a rule that never matched must not record stats"
        );
    }

    #[test]
    fn expand_capture_references_substitutes_and_drops() {
        let re = Regex::new(r"(\w+)-(\w+)").unwrap();
        let caps = re.captures("foo-bar").unwrap();
        assert_eq!(expand_capture_references("$1/$2", &caps), "foo/bar");
        assert_eq!(expand_capture_references("$0", &caps), "foo-bar");
        assert_eq!(expand_capture_references("no dollars", &caps), "no dollars");
        assert_eq!(expand_capture_references("$9", &caps), "");
    }

    #[test]
    fn split_sentences_does_not_emit_a_trailing_empty() {
        assert_eq!(split_sentences("Hello. World."), vec!["Hello", " World"]);
    }

    #[test]
    fn translit_covers_the_remaining_cyrillic_chars() {
        assert_eq!(translit_char('б'), "b");
        assert_eq!(translit_char('в'), "v");
        assert_eq!(translit_char('ж'), "zh");
        assert_eq!(translit_char('з'), "z");
        assert_eq!(translit_char('ц'), "c");
        assert_eq!(translit_char('ы'), "y");
        assert_eq!(translit_char('ю'), "yu");
        assert_eq!(translit_char('я'), "ya");
    }

    #[test]
    fn disabled_cleaner_leaves_sound_tags() {
        let cleaner = HallucinationCleaner::new(false);
        assert_eq!(
            cleaner.apply("hello [Music] world"),
            "hello [Music] world",
            "a disabled cleaner must not strip sound tags"
        );
    }

    #[test]
    fn hallucination_walk_drops_first_segment_keeps_second() {
        let cleaner = HallucinationCleaner::new(true);
        let out = cleaner.apply("Спасибо за просмотр. Реальный текст.");
        assert_eq!(out, "Реальный текст.");
    }

    /// Новая строка — тоже разделитель: галлюцинированный первый сегмент
    /// обязан уйти и в этой ветке (`delete !` в `is_hallucinated_segment`).
    #[test]
    fn hallucination_walk_drops_newline_separated_segment() {
        let cleaner = HallucinationCleaner::new(true);
        let out = cleaner.apply("Спасибо за просмотр\nРеальный текст");
        assert_eq!(out, "Реальный текст");
    }

    /// Немая «e» отбрасывается только при длине СТРОГО больше трёх: трёхбуквенное
    /// слово свою «e» сохраняет.
    #[test]
    fn fold_keeps_the_silent_e_on_three_char_words() {
        assert_eq!(fold_for_match("кое"), "koe");
    }

    #[test]
    fn apply_case_of_handles_a_single_capital_letter() {
        // Одна заглавная буква — это регистр текста, а не «всё капсом».
        assert_eq!(apply_case_of("А", "tauri"), "Tauri");
    }

    #[test]
    fn max_window_is_longest_term_plus_one() {
        let c = CustomWordsCorrector::new(true, vec!["Claude Code".to_string()]);
        assert_eq!(c.max_window(), 3);
    }

    #[test]
    fn window_score_rejects_out_of_bounds_and_short_windows() {
        let c = CustomWordsCorrector::new(true, vec!["Claude Code".to_string()]);
        // Окно, вылезающее за границу входа, — None, а не паника.
        assert!(c.window_score(&["a", "b"], 1, 2, "claudecode").is_none());
        // Ровно CUSTOM_WORD_MIN_CHARS буквенно-цифровых символов — граница.
        assert!(c.window_score(&["abcd"], 0, 1, "abcd").is_some());
    }

    #[test]
    fn edit_distance_substitutions_are_counted() {
        let a: Vec<char> = "ab".chars().collect();
        let b: Vec<char> = "xy".chars().collect();
        assert_eq!(edit_distance(&a, &b), 2);
        // Удаление: «ab» → «b» стоит одну правку. Ловит сдвиг граничного
        // `current[0] = i + 1` (подмену `+` на `*`).
        let b1: Vec<char> = "b".chars().collect();
        assert_eq!(edit_distance(&a, &b1), 1);
    }

    #[test]
    fn tokenize_empty_text_yields_no_tokens() {
        assert!(tokenize("").is_empty());
    }

    // ----- Флаги enabled, пороги и применение правил замены -----
    //
    // Закрывает промахи контрольного прогона по formatter.rs. Общий узор у
    // них один: проверялось только поведение включённого шага на «удобном»
    // входе, поэтому снятие `!` в `if !self.enabled` меняло шаг на строго
    // противоположный незаметно для тестов.

    /// Ровно двадцать слов — граница, на которой `SentenceSplitter`
    /// перестаёт считать текст коротким, — и ключевое слово «потом» внутри,
    /// по которому шаг делит фразу.
    fn twenty_words_with_split_keyword() -> &'static str {
        "один два три четыре пять шесть семь восемь девять десять потом \
         одиннадцать двенадцать тринадцать четырнадцать пятнадцать шестнадцать \
         семнадцать восемнадцать девятнадцать"
    }

    /// Проверяются обе стороны флага на одном входе. Одного случая мало:
    /// тест на выключенный шаг переживает снятие `!`, если не показать, что
    /// включённый шаг тот же текст меняет.
    #[test]
    fn comma_cleaner_runs_only_when_enabled() {
        let input = "раз, и два";
        assert_eq!(
            CommaCleaner::new(true).apply(input),
            "раз и два",
            "включённый шаг убирает запятую перед союзом"
        );
        assert_eq!(
            CommaCleaner::new(false).apply(input),
            input,
            "выключенный шаг обязан вернуть текст нетронутым"
        );
    }

    #[test]
    fn sentence_splitter_runs_only_when_enabled() {
        let text = twenty_words_with_split_keyword();
        assert!(
            SentenceSplitter::new(true).apply(text).contains(". Потом "),
            "включённый шаг делит фразу по ключевому слову"
        );
        assert_eq!(
            SentenceSplitter::new(false).apply(text),
            text,
            "выключенный шаг обязан вернуть текст нетронутым"
        );
    }

    /// Порог короткого текста — строго меньше двадцати слов. Нужны все три
    /// точки: короткий текст, ровно граница и текст длиннее её. По одной
    /// точке подмена `<` на `==`, `<=` или `>` неотличима от оригинала.
    #[test]
    fn sentence_splitter_threshold_is_twenty_words() {
        let short = "раз два потом три";
        assert_eq!(
            short.split_whitespace().count(),
            4,
            "проверка самого теста: вход должен быть заведомо короче порога"
        );
        assert_eq!(
            SentenceSplitter::new(true).apply(short),
            short,
            "текст короче двадцати слов шаг не делит, хотя ключевое слово в нём есть"
        );

        let at_threshold = twenty_words_with_split_keyword();
        assert_eq!(
            at_threshold.split_whitespace().count(),
            20,
            "проверка самого теста: вход должен быть ровно на границе"
        );
        assert!(
            SentenceSplitter::new(true)
                .apply(at_threshold)
                .contains(". Потом "),
            "ровно двадцать слов — уже не короткий текст, шаг обязан поделить"
        );

        let over_threshold = format!("{at_threshold} двадцать");
        assert!(
            SentenceSplitter::new(true)
                .apply(&over_threshold)
                .contains(". Потом "),
            "текст длиннее порога шаг тоже делит"
        );
    }

    /// Без этого теста тело шага можно заменить на пустую строку или на
    /// произвольную константу, и ни один тест не покраснеет: замены
    /// проверялись только через `apply_replacement_rules` напрямую, минуя
    /// сам шаг.
    #[test]
    fn context_replacements_apply_the_rule() {
        let step = ContextReplacements::new(true, vec![word_rule("тайпскрипт", "TypeScript")]);
        assert_eq!(
            step.apply("тайпскрипт рядом"),
            "TypeScript рядом",
            "включённый шаг обязан применить правило"
        );
    }

    /// Выключенный шаг с непустым списком правил — единственный вход, на
    /// котором `!enabled || rules.is_empty()` отличается от той же связки
    /// через `&&`: при `&&` шаг не отступает и применяет правила.
    #[test]
    fn context_replacements_stay_out_when_disabled() {
        let step = ContextReplacements::new(false, vec![word_rule("тайпскрипт", "TypeScript")]);
        assert_eq!(
            step.apply("тайпскрипт рядом"),
            "тайпскрипт рядом",
            "выключенный шаг не применяет правила, даже когда они есть"
        );
    }
}
