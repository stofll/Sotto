//! Offline text cleanup shared by dictation, file transcription and preview.
//!
//! Pipeline
//! ---------
//!
//! `Formatter::process` applies the steps in a fixed order. Toggling a step off in the
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
//! Replacement matching modes:
//!   * `match=word` → Unicode word-boundary regex (`\b`)
//!   * `match=phrase` → literal, no boundary
//!   * `match=contains` → literal, no boundary (legacy alias for phrase)
//!   * `match=regex` → user-supplied regex
//!   * `case_sensitive=false` → IGNORECASE
//!   * `preserve_case=true` → keep all-uppercase / first-letter-uppercase
//!
//! The preview helpers here are synchronous; their Tauri command wrappers
//! live in `format_commands`. The async `preview_format` command runs the
//! full pipeline in `spawn_blocking`, as dictation does, to keep lexicon
//! initialization and spelling work off the UI thread. The rule-only
//! `preview_replacements` command remains synchronous and does not use
//! the spelling lexicon.

use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

// ---------------------------------------------------------------------------
// Defaults
// ---------------------------------------------------------------------------

/// Default Russian parasite words.
///
/// The rule this list must satisfy: a word belongs here only when deleting it
/// cannot change what the sentence asserts. The step deletes every entry
/// unconditionally, by word boundary, everywhere in the text, and the user is
/// never shown which words those are — so a word that is padding in one
/// position and load-bearing in another does not belong here at all.
///
/// Five entries were dropped from the Python original for failing that rule:
///
/// * «да», «нет» — the answer itself, not padding around it. «Он спросил
///   приедешь ли я сказал нет» came out as «…я сказал», and «нет не надо это
///   мержить» came out as the instruction to merge.
/// * «вот» — a demonstrative particle carrying the emphasis of the sentence it
///   opens.
/// * «значит», «собственно» — parasites only as interjections. As ordinary
///   words they are the predicate and the qualifier: «это значит, что мы
///   опоздали» collapsed into «это, что мы опоздали», and «собственно код» lost
///   the word that said which code was meant.
///
/// What is left is padding in every position it can occupy.
pub const RU_PARASITE_WORDS: &[&str] = &[
    "ну",
    "типа",
    "как бы",
    "в общем",
    "короче",
    "это самое",
    "так сказать",
    "понимаешь",
    "понимаете",
    "блин",
    "чё",
    "че",
    "короч",
];

/// English parasite words. Shipped switched OFF — see [`PARASITE_PRESETS`].
///
/// English fails the admission rule the Russian list is held to, and not at the
/// edges: its commonest fillers are ordinary vocabulary. «I like it» and «looks
/// like this», «well done» and «the well», «turn right» and «that's right» —
/// a word-boundary regex sees no difference, exactly as it saw none between the
/// answer «нет» and padding.
///
/// So the set exists but nobody gets it by default: switching it on is a
/// deliberate act, and every word in it is a chip that can be switched off on
/// its own. The order below runs from safe to dangerous, because that is the
/// order a person should read them in:
///
/// * `basically` … `apparently` — adverbial hedges. They colour a sentence
///   without carrying its claim, so dropping them is the closest English has to
///   the Russian «короче».
/// * `you know` … `kind of` — phrasal padding. Riskier: «you know the answer»
///   and «a kind of bird» are ordinary sentences.
/// * `like`, `well`, `right` — the ones people actually over-use, and the ones
///   that break real sentences. Present because leaving them out makes the set
///   pointless, last because they are why it is opt-in.
///
/// Deliberately absent: `just`, `so`, `then`, `now`. They are filler as often
/// as the three above and destroy more when wrong — `just one`, `so we ship`.
pub const EN_PARASITE_WORDS: &[&str] = &[
    "basically",
    "literally",
    "essentially",
    "honestly",
    "obviously",
    "actually",
    "apparently",
    "you know",
    "i mean",
    "sort of",
    "kind of",
    "like",
    "well",
    "right",
];

/// A built-in parasite word list for one language.
pub struct ParasitePreset {
    /// Stable id, stored in the config when the user changes the selection.
    pub id: &'static str,
    /// The dictation language the set is written for. The interface shows the
    /// matching set first; nothing in the pipeline reads it, because the words
    /// of one language cannot match the text of another anyway.
    pub language: &'static str,
    pub words: &'static [&'static str],
    /// Whether the set applies to someone who has never opened these settings.
    pub default_on: bool,
}

/// Every built-in set, in the order the interface lists them.
pub const PARASITE_PRESETS: &[ParasitePreset] = &[
    ParasitePreset {
        id: "ru",
        language: "ru",
        words: RU_PARASITE_WORDS,
        default_on: true,
    },
    ParasitePreset {
        id: "en",
        language: "en",
        words: EN_PARASITE_WORDS,
        default_on: false,
    },
];

/// Filler sounds for a dictation in Russian.
///
/// «А» and «о» are drawn out into a filler («а-а-а», «о-о»), but on their own
/// they are ordinary words: the conjunction «а» and the preposition «о». The
/// patterns for those two therefore require the sound to be held — two or more
/// vowels, run together or split by a hyphen or a space. Written as a single
/// letter it is left alone, so «речь о том, а потом мы всё переделали» keeps its
/// preposition and its conjunction instead of collapsing into «речь том что
/// потом». «Э» and «м» need no such guard: neither is a Russian word.
///
/// These are applied to every dictation, whatever the language: Cyrillic cannot
/// match a text written in any other alphabet, so there is nothing to gate.
const RUSSIAN_FILLER_PATTERNS: &[&str] = &[
    r"\b(э+[-\s]*)+\b",
    r"\b(м+[-\s]*)+\b",
    r"\bа+(?:[-\s]*а+)+\b",
    r"\bо+(?:[-\s]*о+)+\b",
    r"\bну-+у*\b",
    r"\bмм-+\b",
];

/// Filler sounds for a dictation in English. Each is a sound that is not an
/// English word:
///
/// * `uh+m*` — uh, uhh, uhm, uhmm.
/// * `um+` — um, umm.
/// * `erm*` — er, erm, ermm. Deliberately NOT `err`, which is a verb.
/// * `hm+` — hm, hmm, hmmm.
/// * `mmm+` — three or more, because «mm» is millimetres.
/// * `ah+` — ah, ahh.
///
/// «Oh» is left out for the reason «о» is: it carries the emotion of the line
/// it opens rather than padding it.
///
/// UNLIKE the Cyrillic set, these run ONLY when the dictation language is
/// English — and that gate is a correction, not caution. They were ungated at
/// first, on the reasoning that a Latin pattern cannot match Russian. True, and
/// beside the point: most of the languages Sotto transcribes are written in the
/// same alphabet as English. «Er kommt um acht» came out as «Kommt acht» —
/// `er` is a German pronoun and `um` a German preposition — and Dutch «er» and
/// French «ah» go the same way. An unknown language («auto», or a config older
/// than the setting) counts as not-English: failing to strip a filler costs a
/// word of noise, stripping a pronoun costs the sentence.
///
/// `(?i)` is here because an engine capitalises the first word of a sentence
/// and a filler is very often that word. The Cyrillic patterns have never had
/// it and still miss a capitalised «Ну» — a separate gap, not one this list
/// should fix quietly.
const ENGLISH_FILLER_PATTERNS: &[&str] = &[
    r"(?i)\buh+m*\b",
    r"(?i)\bum+\b",
    r"(?i)\berm*\b",
    r"(?i)\bhm+\b",
    r"(?i)\bmmm+\b",
    r"(?i)\bah+\b",
];

/// The filler patterns in force for a dictation in `language`, compiled
/// once per set rather than on every dictation.
fn default_filler_patterns(language: Option<&str>) -> &'static [Regex] {
    fn compile(sets: &[&[&str]]) -> Vec<Regex> {
        sets.iter()
            .flat_map(|set| set.iter())
            .map(|pattern| Regex::new(pattern).expect("valid filler pattern"))
            .collect()
    }
    static RUSSIAN: Lazy<Vec<Regex>> = Lazy::new(|| compile(&[RUSSIAN_FILLER_PATTERNS]));
    static WITH_ENGLISH: Lazy<Vec<Regex>> =
        Lazy::new(|| compile(&[RUSSIAN_FILLER_PATTERNS, ENGLISH_FILLER_PATTERNS]));
    if language == Some("en") {
        &WITH_ENGLISH
    } else {
        &RUSSIAN
    }
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

fn replacement_regex(rule: &ReplacementRule) -> Result<Regex, regex::Error> {
    let source = replacement_pattern(rule);
    if rule.case_sensitive {
        Regex::new(&source)
    } else {
        Regex::new(&format!("(?i){source}"))
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

struct CompiledReplacement {
    rule: ReplacementRule,
    pattern: Regex,
}

fn compile_replacements(rules: Vec<ReplacementRule>) -> Vec<CompiledReplacement> {
    rules
        .into_iter()
        .filter(|rule| rule.enabled)
        .filter_map(|mut rule| {
            rule.find = rule.find.trim().to_string();
            if rule.find.is_empty() {
                return None;
            }
            match replacement_regex(&rule) {
                Ok(pattern) => Some(CompiledReplacement { rule, pattern }),
                Err(error) => {
                    log::warn!(
                        "formatter: invalid replacement regex skipped: {}: {error}",
                        rule.find
                    );
                    None
                }
            }
        })
        .collect()
}

pub fn apply_replacement_rules(
    text: &str,
    source: Option<&Value>,
    paused: bool,
) -> (String, ReplacementStats) {
    if text.is_empty() || paused {
        return (text.to_string(), ReplacementStats::default());
    }
    apply_compiled_replacements(
        text,
        &compile_replacements(normalize_replacement_rules(source)),
    )
}

fn apply_compiled_replacements(
    text: &str,
    rules: &[CompiledReplacement],
) -> (String, ReplacementStats) {
    if text.is_empty() {
        return (String::new(), ReplacementStats::default());
    }
    let mut stats = ReplacementStats {
        total: 0,
        rules: Vec::new(),
    };
    let mut current = text.to_string();
    for CompiledReplacement { rule, pattern } in rules {
        let preserve_case = rule.preserve_case;
        let match_mode = rule.match_;
        let replace_template = rule.replace.clone();

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
                id: rule.id.clone(),
                find: rule.find.clone(),
                replace: rule.replace.clone(),
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
    fn apply_unprotected(&self, text: &str) -> String;
    fn edits_protected_text(&self) -> bool {
        false
    }
    fn permits_empty_output(&self) -> bool {
        false
    }
    fn protected_spans(&self, text: &str) -> Vec<std::ops::Range<usize>> {
        crate::text_protection::technical_spans(text)
    }
    fn apply(&self, text: &str) -> String {
        if self.edits_protected_text() {
            self.apply_unprotected(text)
        } else {
            crate::text_protection::apply(text, self.protected_spans(text), |s| {
                self.apply_unprotected(s)
            })
        }
    }
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
#[cfg(test)]
fn is_pure_hallucination(text: &str) -> bool {
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
    fn permits_empty_output(&self) -> bool {
        true
    }
    fn protected_spans(&self, text: &str) -> Vec<std::ops::Range<usize>> {
        // Strong signatures can contain a domain (Amara.org). Quoted code
        // remains literal, but a domain must not hide a silence hallucination.
        crate::text_protection::code_spans(text)
    }
    fn name(&self) -> &str {
        "Remove hallucinations"
    }
    fn description(&self) -> &str {
        "субтитры, «спасибо за просмотр», [Music] и др. артефакты Whisper"
    }
    fn apply_unprotected(&self, text: &str) -> String {
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
    patterns: &'static [Regex],
}

impl FillerWordsRemover {
    /// `language` is the dictation language, and decides whether the English
    /// sounds are in force — see [`ENGLISH_FILLER_PATTERNS`].
    pub fn new(enabled: bool, language: Option<&str>) -> Self {
        Self {
            enabled,
            patterns: default_filler_patterns(language),
        }
    }
}

impl FormatStep for FillerWordsRemover {
    fn name(&self) -> &str {
        "Remove fillers"
    }
    fn description(&self) -> &str {
        "э-э, ммм, а-а, uh, umm, hmm и подобные"
    }
    fn apply_unprotected(&self, text: &str) -> String {
        if !self.enabled {
            return text.to_string();
        }
        let mut out = text.to_string();
        for pattern in self.patterns {
            out = pattern
                .replace_all(&out, |caps: &regex::Captures| {
                    let matched = caps.get(0).unwrap();
                    let before = out[..matched.start()].chars().next_back();
                    let after = out[matched.end()..].chars().next();
                    if before == Some('-')
                        || (matched.as_str().ends_with('-')
                            && after.is_some_and(char::is_alphabetic))
                    {
                        matched.as_str().to_string()
                    } else {
                        String::new()
                    }
                })
                .into_owned();
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

/// A ready-made set of terms for the user's dictionary.
///
/// The point is not the words themselves — a person would enter those anyway.
/// The point is that the dictionary is remembered too late: the feature exists,
/// is covered by tests and surfaced in settings, and still stays empty right up
/// to the day the terms start going wrong. A one-click set removes that
/// barrier.
///
/// On Whisper it additionally works **before** recognition rather than only
/// after: the dictionary's contents go into `initial_prompt` (see
/// `custom_words_prompt`), so the model knows in advance which words to expect
/// and is more likely not to mangle them at all.
pub struct DictionaryPreset {
    /// A stable identifier. The name is displayed by the frontend — that is
    /// translatable, the word list is not.
    pub id: &'static str,
    pub words: &'static [&'static str],
}

#[cfg(test)]
mod localized_preview_tests {
    use super::*;

    #[test]
    fn ui_samples_demonstrate_real_cleanup_and_every_suggested_rule() {
        let fixtures: Value =
            serde_json::from_str(include_str!("../../src/pages/textExamples.fixture.json"))
                .unwrap();
        for fixture in fixtures.as_array().unwrap() {
            let sample = fixture["sample"].as_str().unwrap();
            assert_eq!(
                preview_format(sample, &serde_json::json!({}))
                    .unwrap()
                    .formatted,
                fixture["clean"].as_str().unwrap(),
                "{} cleanup",
                fixture["locale"]
            );
            let rules: Vec<Value> = fixture["rules"].as_array().unwrap().iter().enumerate().map(|(index, pair)| serde_json::json!({
                "id": index.to_string(), "find": pair[0], "replace": pair[1], "match": "word", "enabled": true, "preserve_case": false, "case_sensitive": false
            })).collect();
            let config = serde_json::json!({"replacement_rules": rules});
            assert_eq!(
                preview_format(sample, &config).unwrap().formatted,
                fixture["formatted"].as_str().unwrap(),
                "{} replacements",
                fixture["locale"]
            );
            let matched = preview_replacements(sample, &config).unwrap();
            assert_eq!(
                matched.matched_rules.len(),
                rules.len(),
                "every suggestion must match the sample"
            );
        }
    }

    #[test]
    fn english_custom_filler_placeholder_is_supported() {
        let result = preview_format(
            "basically we can so to speak begin",
            &serde_json::json!({
                "text_formatting": {"custom_parasite_words": ["basically", "so to speak"]}
            }),
        )
        .unwrap();
        assert_eq!(result.formatted, "We can begin.");
    }

    #[test]
    fn capitalization_preserves_addresses_and_still_starts_sentences() {
        let capitalizer = Capitalizer::new(true);
        assert_eq!(
            capitalizer.apply("first.last@example.com is my email. next sentence! done"),
            "first.last@example.com is my email. Next sentence! Done"
        );
        assert_eq!(
            capitalizer.apply("пишите на first.last@example.com. потом обсудим"),
            "Пишите на first.last@example.com. Потом обсудим"
        );
        assert_eq!(
            capitalizer.apply("hello. world?yes!fine"),
            "Hello. World?Yes!Fine"
        );
        for (input, expected) in [
            ("контакт:name@example.com", "Контакт:name@example.com"),
            ("hello!name@example.com", "Hello!name@example.com"),
            (
                "(first.Last+tag@example.com).next",
                "(first.Last+tag@example.com).Next",
            ),
            ("name@example.com!next", "name@example.com!Next"),
            (
                "name@example.com,other@example.org?yes",
                "name@example.com,other@example.org?Yes",
            ),
            ("hello@!next", "Hello@!Next"),
        ] {
            assert_eq!(capitalizer.apply(input), expected, "{input}");
        }
    }
}

/// Development: what people say out loud every day and what the engine writes
/// in Cyrillic.
///
/// The selection is conservative, and here are the rules — worth following if
/// the set is ever extended:
///
/// 1. A term must survive folding and stay no shorter than
///    [`CUSTOM_WORD_MIN_CHARS`]. `Vite` folds to `vit` (the silent `e` is
///    dropped), `Node` to `nod`: the dictionary silently ignores such words, and
///    promising them to the user is dishonest.
/// 2. A term must not land on an ordinary Russian word within the edit budget.
///    `buffer` folds to `bufer` and differs from «буфет» by exactly one edit —
///    the person dictating about lunch pays for that substitution.
///
/// Both rules are checked by tests, not by eye.
const PRESET_DEVELOPMENT: &[&str] = &[
    // Git and process
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
    // Build and release
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
    // Languages and tools
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
    // Files and formats
    "package.json",
    "tsconfig.json",
    "README",
    "Markdown",
    // Concepts
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

/// The sets the frontend offers to add to the dictionary.
pub const DICTIONARY_PRESETS: &[DictionaryPreset] = &[DictionaryPreset {
    id: "development",
    words: PRESET_DEVELOPMENT,
}];

/// How many edits are forgiven a term of this length for it still to count as a
/// distortion of the dictionary form.
///
/// There used to be a single relative threshold of 0.8 for terms of any length,
/// and it was wrong at both ends of the scale. For a four-letter abbreviation
/// («NSIS») one wrong letter is 0.75 straight away, below the threshold: such a
/// term never passed, however many times it was entered into the dictionary. And
/// for a long compound one («structured_log» against the heard «структуре
/// тлок») two or three edits over thirteen characters is obviously the same
/// word, yet the relative measure gave 0.769 and refused as well.
///
/// An absolute budget solves both cases at once: the longer the term, the
/// cheaper a single recognition error becomes.
///
/// The upper bound is set by what must **not** be caught. «брайтер» and
/// «райдере» are the engines hearing different words rather than writing
/// «writer» in another alphabet; two and three edits over six characters
/// separate them from the folded `vriter`. So a six-letter term is forgiven
/// exactly one: a mistaken substitution looks like something the person
/// supposedly dictated themselves, and spoils the text worse than a missed
/// one.
fn edit_budget(key_len: usize) -> usize {
    match key_len {
        0..=6 => 1,
        7..=11 => 2,
        _ => 3,
    }
}

#[cfg(test)]
fn within_budget(folded: &str, key: &str) -> bool {
    let folded: Vec<char> = folded.chars().collect();
    let key: Vec<char> = key.chars().collect();
    DistanceScratch::default()
        .within(&folded, &key, edit_budget(key.len()))
        .is_some()
}

/// Shorter than this, fuzzy comparison does not work: for three-letter words
/// almost any typo gives a similarity above the threshold, and the dictionary
/// starts rewriting healthy text.
const CUSTOM_WORD_MIN_CHARS: usize = 4;

/// Transliteration of Cyrillic into Latin.
///
/// Without it the whole idea does not work: the typical case is a person
/// dictating an English brand while the engine writes it in Cyrillic. «Таури»
/// and «Tauri» share not a single character, and the per-character distance
/// between them is the maximum possible, even though it is one and the same
/// word.
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
        // The hard and soft signs carry no sound.
        'ъ' | 'ь' => "",
        _ => "",
    }
}

/// Fold a word into the form used for comparison.
///
/// Case does not matter, punctuation does not belong to the word, Cyrillic goes
/// to Latin, and then the spellings that distinguish one and the same sound are
/// removed: `ph`/`f`, `ck`/`k`, `c`/`k`, doubled letters, a silent `e` at the
/// end. The digraphs `sh`/`ch`/`zh` are first hidden in sentinel characters,
/// otherwise the subsequent `c → k` would take `ch` apart.
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

    // Sentinel characters for the digraphs: uppercase, and the string is already
    // lowercase, so there is nothing for them to collide with.
    let stage: String = latin
        .replace("sh", "S")
        .replace("ch", "C")
        .replace("zh", "Z")
        .replace("ph", "f")
        .replace("ck", "k")
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

    // Doubling is inaudible: «Ollama» and «олама» are one word.
    let mut deduped = String::with_capacity(out.len());
    for c in out.chars() {
        if !deduped.ends_with(c) {
            deduped.push(c);
        }
    }

    // A silent «e» at the end: «code» and «код» must meet.
    if deduped.chars().count() > 3 && deduped.ends_with('e') {
        deduped.pop();
    }
    deduped
}

/// Reuse the two Levenshtein rows for every candidate in one formatting call.
/// The result supplies both the edit-budget check and the similarity score.
#[derive(Default)]
struct DistanceScratch {
    previous: Vec<usize>,
    current: Vec<usize>,
}

impl DistanceScratch {
    fn distance(&mut self, a: &[char], b: &[char]) -> usize {
        self.within(a, b, usize::MAX).expect("unbounded distance")
    }

    fn within(&mut self, a: &[char], b: &[char], budget: usize) -> Option<usize> {
        if a.len().abs_diff(b.len()) > budget {
            return None;
        }
        if a.is_empty() {
            return Some(b.len());
        }
        if b.is_empty() {
            return Some(a.len());
        }
        self.previous.clear();
        self.previous.extend(0..=b.len());
        self.current.resize(b.len() + 1, 0);
        for (i, ca) in a.iter().enumerate() {
            self.current[0] = i + 1;
            let mut row_min = self.current[0];
            for (j, cb) in b.iter().enumerate() {
                let cost = usize::from(ca != cb);
                let distance = (self.previous[j] + cost)
                    .min(self.previous[j + 1] + 1)
                    .min(self.current[j] + 1);
                self.current[j + 1] = distance;
                row_min = row_min.min(distance);
            }
            // Every path into a later row extends one of these prefixes; if
            // all already exceed the budget, no suffix can restore a match.
            if row_min > budget {
                return None;
            }
            std::mem::swap(&mut self.previous, &mut self.current);
        }
        let distance = self.previous[b.len()];
        (distance <= budget).then_some(distance)
    }
}

#[cfg(test)]
fn edit_distance(a: &[char], b: &[char]) -> usize {
    DistanceScratch::default().distance(a, b)
}

fn distance_score(distance: usize, a_len: usize, b_len: usize) -> f64 {
    let longest = a_len.max(b_len);
    if longest == 0 {
        return 0.0;
    }
    1.0 - distance as f64 / longest as f64
}

/// Similarity of two folded keys.
///
/// The full form only. A second signal was tried — a consonant skeleton keeping
/// a word's first character and all its consonants: it catches loanwords that
/// diverged in their vowels («клод» and «claude» match literally after it). But
/// it also glues unrelated words together: «город» and «град» are
/// indistinguishable without vowels, and a dictionary containing the term «Град»
/// began rewriting healthy text. No threshold managed to cut one off without
/// losing the other.
///
/// The choice favours a missed replacement: a person sees and fixes that, while
/// a substituted word looks like something they supposedly dictated themselves.
/// The cross-alphabet case — the one this was all started for — the folding
/// covers even without the skeleton: «таури» and «Tauri» match exactly after
/// transliteration.
#[cfg(test)]
fn similarity(a: &str, b: &str) -> f64 {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    distance_score(edit_distance(&a, &b), a.len(), b.len())
}

/// Carry the case of the matched fragment over to the dictionary form.
///
/// The user sets a term's spelling, which is why it wins: «таури» becomes
/// «Tauri», not «tauri». But if the fragment stood at the start of a sentence or
/// was typed in caps, that is a property of the text rather than of the term,
/// and it must be preserved.
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

/// Brings recognised words to the spelling from the user's dictionary.
///
/// Needed because the engine cannot know a particular person's names, brands,
/// terms and jargon. For Whisper we additionally hint with the dictionary via
/// `initial_prompt` — there it influences the decoding itself; GigaAM has no
/// such input (offline NemoCtc, and hotwords in sherpa-onnx exist only for a
/// transducer), and for it this step is the only way.
///
/// The comparison is fuzzy: the point of the dictionary is to catch exactly
/// those cases where the engine heard something close but wrote it otherwise.
pub struct CustomWordsCorrector {
    enabled: bool,
    terms: Vec<CustomTerm>,
    max_window: usize,
}

struct CustomTerm {
    canonical: String,
    key: Vec<char>,
    acronym: bool,
}

pub(crate) fn custom_word_supported(word: &str) -> bool {
    word.split_whitespace()
        .map(fold_for_match)
        .collect::<String>()
        .chars()
        .filter(|c| c.is_alphanumeric())
        .count()
        >= CUSTOM_WORD_MIN_CHARS
}

impl CustomWordsCorrector {
    pub fn new(enabled: bool, words: Vec<String>) -> Self {
        let terms: Vec<CustomTerm> = words
            .into_iter()
            .filter_map(|word| {
                let canonical = word.trim().to_string();
                if canonical.is_empty() {
                    return None;
                }
                // The key is joined, without spaces: the engine segments
                // speech its own way, and «Giga AM» arrives as two tokens where
                // the dictionary holds the single word «GigaAM». Comparing by
                // the joined form removes the question entirely rather than
                // guessing how many words the text will contain.
                let key: String = canonical.split_whitespace().map(fold_for_match).collect();
                if key.chars().filter(|c| c.is_alphanumeric()).count() < CUSTOM_WORD_MIN_CHARS {
                    return None;
                }
                let acronym = canonical.chars().all(|c| c.is_ascii_uppercase());
                Some(CustomTerm {
                    canonical,
                    key: key.chars().collect(),
                    acronym,
                })
            })
            .collect();
        let max_window = terms
            .iter()
            .map(|term| term.canonical.split_whitespace().count())
            .max()
            .unwrap_or(0)
            + 1;
        Self {
            enabled,
            terms,
            max_window,
        }
    }

    /// How many text tokens it makes sense to try at once.
    ///
    /// The word count of the longest term plus one: the engine may split on one
    /// more space than the dictionary does («Клод Код» against «Клодкод»).
    fn max_window(&self) -> usize {
        self.max_window
    }

    /// Similarity of the window `words[start .. start + n]` to a specific term.
    /// `None` if the window is too short to be compared at all.
    fn window_score(
        &self,
        words: &[&str],
        start: usize,
        n: usize,
        key: &[char],
        scratch: &mut DistanceScratch,
    ) -> Option<f64> {
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
        let folded: Vec<char> = folded.chars().collect();
        Some(distance_score(
            scratch.distance(&folded, key),
            folded.len(),
            key.len(),
        ))
    }

    /// The best match beginning at position `start`.
    ///
    /// Returns the window length and the dictionary form.
    ///
    /// A compound window is accepted only if, for the same term, it is
    /// **strictly** more similar than the same window without its first and
    /// without its last token. The comparison is against the same term rather
    /// than any term: otherwise «Claude» from the dictionary would cancel the
    /// «Claude Code» match, since it fits the first half of the window
    /// perfectly. Without this rule a long term drags a neighbouring function
    /// word along: the threshold is relative, «на опен роутер» differs from
    /// «openrouter» by two characters out of twelve — that is, it passes — and
    /// the preposition disappeared from the text. The check is symmetric because
    /// a word can stick on either side: «Tauri и» breaks in exactly the same
    /// way.
    ///
    /// On an equal score the long window wins: otherwise «Claude» would eat the
    /// beginning of «Claude Code» and leave «код» dangling.
    fn best_match(
        &self,
        words: &[&str],
        start: usize,
        scratch: &mut DistanceScratch,
    ) -> Option<(usize, &str)> {
        let limit = self.max_window().min(words.len() - start);
        let mut best: Option<(f64, usize, &str)> = None;
        let mut ambiguous = false;
        for n in 1..=limit {
            if n > 1
                && words[start..start + n - 1]
                    .iter()
                    .any(|word| !trailing_punctuation(word).is_empty())
            {
                break;
            }
            let folded: String = words[start..start + n]
                .iter()
                .map(|w| fold_for_match(w))
                .collect();
            if folded.chars().filter(|c| c.is_alphanumeric()).count() < CUSTOM_WORD_MIN_CHARS {
                continue;
            }
            let ordinary_russian = words[start..start + n].iter().all(|word| {
                crate::spelling::known_russian_word(
                    word.trim_matches(|c: char| !c.is_alphanumeric()),
                )
            });
            let folded: Vec<char> = folded.chars().collect();
            for term in &self.terms {
                let CustomTerm {
                    canonical,
                    key,
                    acronym,
                } = term;
                let budget = edit_budget(key.len());
                if folded.len().abs_diff(key.len()) > budget {
                    continue;
                }
                // A dictionary term is not permission to rewrite valid prose.
                // Exact phonetic matches still restore transliterated terms.
                if ordinary_russian && &folded != key {
                    continue;
                }
                // A single edit in a short token can change the subject
                // entirely (REST → Rust, «буст» → Rust). Acronyms explicitly
                // supplied in capitals retain their existing matching budget.
                if folded != *key && folded.len().min(key.len()) <= 4 && !acronym {
                    continue;
                }
                let Some(distance) = scratch.within(&folded, key, budget) else {
                    continue;
                };
                let score = distance_score(distance, folded.len(), key.len());
                if n > 1 {
                    let trimmed = self
                        .window_score(words, start + 1, n - 1, key, scratch)
                        .into_iter()
                        .chain(self.window_score(words, start, n - 1, key, scratch))
                        .fold(0.0_f64, f64::max);
                    if trimmed >= score {
                        continue;
                    }
                }
                // `<` rather than `<=`: windows are iterated from short to
                // long, and an equal score must go to the long one.
                match best {
                    Some((best_score, _, _)) if score < best_score => {}
                    Some((best_score, best_n, best_term)) if score == best_score && n == best_n => {
                        ambiguous |= !canonical.eq_ignore_ascii_case(best_term);
                    }
                    _ => {
                        best = Some((score, n, canonical.as_str()));
                        ambiguous = false;
                    }
                }
            }
        }
        best.filter(|_| !ambiguous)
            .map(|(_, n, canonical)| (n, canonical))
    }
}

/// A word together with the separator that stood before it.
///
/// The separator is stored verbatim rather than rebuilt from a space: the
/// dictionary step has no right to touch the text's layout. The first version
/// cut the input with `split_whitespace` and joined it with `join(" ")` — and by
/// doing so collapsed double spaces even with normalisation off, and turned
/// newlines into spaces, that is destroyed paragraphs.
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
        // A trailing space with no word after it is part of the text too.
        tokens.push(Token {
            gap: &text[gap_start..],
            raw: "",
        });
    }
    tokens
}

/// Punctuation before the first letter or digit.
fn leading_punctuation(word: &str) -> &str {
    match word.char_indices().find(|(_, c)| c.is_alphanumeric()) {
        Some((i, _)) => &word[..i],
        // All punctuation: we treat it as leading so as not to duplicate the
        // same chunk as a trailing part as well.
        None => word,
    }
}

/// Punctuation after the last letter or digit.
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
    fn apply_unprotected(&self, text: &str) -> String {
        if !self.enabled || self.terms.is_empty() {
            return text.to_string();
        }
        let tokens = tokenize(text);
        let raws: Vec<&str> = tokens.iter().map(|t| t.raw).collect();
        let mut out = String::with_capacity(text.len());
        let mut i = 0;
        let mut scratch = DistanceScratch::default();
        while i < tokens.len() {
            match self.best_match(&raws, i, &mut scratch) {
                Some((n, canonical)) => {
                    let first = &tokens[i];
                    let last = &tokens[i + n - 1];
                    // Quotes and brackets around a term belong to the sentence
                    // rather than to the term — on both sides. The trailing part
                    // was preserved from the start, but the opening punctuation
                    // was lost: «таури» turned into Tauri».
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
    /// Built-in words the user switched off in settings.
    ///
    /// Only the words of the active sets are filtered through this: a word the
    /// user typed in themselves is switched off by deleting it.
    disabled_words: Vec<String>,
    /// The words of the sets that are switched on, already resolved.
    builtin_words: Vec<&'static str>,
}

impl ParasiteWordsRemover {
    pub fn new(
        enabled: bool,
        custom_words: Vec<String>,
        disabled_words: Vec<String>,
        builtin_words: Vec<&'static str>,
    ) -> Self {
        Self {
            enabled,
            custom_words,
            disabled_words,
            builtin_words,
        }
    }

    /// Whether a built-in word is still in force.
    ///
    /// Trimmed and lowercased on both sides rather than compared byte for byte:
    /// the list travels through the config as plain strings, and the constant
    /// is Cyrillic, where `eq_ignore_ascii_case` would not fold a single letter.
    fn is_on(&self, word: &str) -> bool {
        let word = word.to_lowercase();
        !self
            .disabled_words
            .iter()
            .any(|off| off.trim().to_lowercase() == word)
    }
}

impl FormatStep for ParasiteWordsRemover {
    fn name(&self) -> &str {
        "Remove parasites"
    }
    fn description(&self) -> &str {
        "ну, типа, как бы, в общем..."
    }
    fn apply_unprotected(&self, text: &str) -> String {
        if !self.enabled {
            return text.to_string();
        }
        let mut all: Vec<&str> = self
            .builtin_words
            .iter()
            .copied()
            .filter(|word| self.is_on(word))
            .collect();
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
            // `(?i)`: an engine capitalises the first word of a sentence and a
            // parasite is very often that word, while «i mean» is never
            // dictated in lower case at all. Without it the English set would
            // miss most of its own matches, and the Russian one still missed a
            // sentence-leading «Ну».
            let pattern = format!(r"(?i)\b{}\b(?:[ \t]*,)?", regex::escape(word));
            if let Ok(re) = Regex::new(&pattern) {
                out = re
                    .replace_all(&out, |caps: &regex::Captures| {
                        let matched = caps.get(0).unwrap();
                        let before = out[..matched.start()].chars().next_back();
                        let after = out[matched.end()..].chars().next();
                        let prefix = out[..matched.start()].trim_end();
                        let suffix = out[matched.end()..].trim_start();
                        // Preserve comparative «короче» inside a clause and
                        // the fixed expression «в общем и целом».
                        let explicit_custom = self
                            .custom_words
                            .iter()
                            .any(|custom| custom.eq_ignore_ascii_case(word));
                        let meaningful = !explicit_custom
                            && ((word == "короче"
                                && !prefix.is_empty()
                                && !prefix.ends_with(['.', '!', '?', '…'])
                                && prefix
                                    .rsplit(['.', '!', '?', '…'])
                                    .next()
                                    .is_none_or(|clause| clause.trim().to_lowercase() != "ну")
                                && !(prefix.ends_with(',') && matched.as_str().ends_with(',')))
                                || (word == "в общем"
                                    && suffix
                                        .split_whitespace()
                                        .take(2)
                                        .map(|s| {
                                            s.trim_matches(|c: char| !c.is_alphabetic())
                                                .to_lowercase()
                                        })
                                        .eq(["и", "целом"])));
                        // A word boundary also occurs inside «чё-то» and «ну-ка».
                        // Removing only one side would leave a dangling suffix.
                        if meaningful
                            || before.is_some_and(|c| matches!(c, '-' | '‑' | '–' | '\''))
                            || after.is_some_and(|c| matches!(c, '-' | '‑' | '–' | '\''))
                        {
                            matched.as_str().to_string()
                        } else {
                            String::new()
                        }
                    })
                    .into_owned();
            }
        }
        out = MULTI_SPACE.replace_all(&out, " ").into_owned();
        let out = SPACE_BEFORE_PUNCT.replace_all(&out, "$1").into_owned();
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
    fn apply_unprotected(&self, text: &str) -> String {
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
            if last_word.as_deref() == Some(lowered.as_str())
                && !matches!(lowered.as_str(), "чуть" | "еле" | "едва")
            {
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
    fn apply_unprotected(&self, text: &str) -> String {
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
        "убрать двойные запятые и запятую в начале текста"
    }
    fn apply_unprotected(&self, text: &str) -> String {
        if !self.enabled {
            return text.to_string();
        }
        let mut out = text.to_string();
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
    fn apply_unprotected(&self, text: &str) -> String {
        if !self.enabled {
            return text.to_string();
        }
        let mut out = MULTI_SPACE.replace_all(text, " ").into_owned();
        out = SPACE_BEFORE_PUNCT.replace_all(&out, "$1").into_owned();
        restore_sentence_spaces(out.trim())
    }
    fn enabled(&self) -> bool {
        self.enabled
    }
    fn set_enabled(&mut self, value: bool) {
        self.enabled = value;
    }
}

fn restore_sentence_spaces(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for part in text.split_inclusive(char::is_whitespace) {
        let chars: Vec<char> = part.chars().collect();
        for (i, &ch) in chars.iter().enumerate() {
            out.push(ch);
            let Some(&next) = chars.get(i + 1) else {
                continue;
            };
            let previous = i.checked_sub(1).and_then(|j| chars.get(j));
            let cyrillic = |c: char| matches!(c, 'а'..='я' | 'А'..='Я' | 'ё' | 'Ё');
            if previous.is_some_and(|&c| cyrillic(c))
                && cyrillic(next)
                && (matches!(ch, ',' | ';' | '!' | '?') || (ch == '.' && next.is_uppercase()))
            {
                // Initials are not sentence endings.
                let initial = ch == '.'
                    && previous.is_some_and(|c| c.is_uppercase())
                    && (i == 1 || chars.get(i - 2) == Some(&'.'));
                if !initial {
                    out.push(' ');
                }
            }
        }
    }
    out
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
    fn apply_unprotected(&self, text: &str) -> String {
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
    rules: Vec<CompiledReplacement>,
}

impl ContextReplacements {
    pub fn new(enabled: bool, rules: Vec<ReplacementRule>) -> Self {
        Self {
            enabled,
            rules: compile_replacements(rules),
        }
    }
}

impl FormatStep for ContextReplacements {
    fn permits_empty_output(&self) -> bool {
        true
    }
    fn edits_protected_text(&self) -> bool {
        true
    }
    fn name(&self) -> &str {
        "Text replacements"
    }
    fn description(&self) -> &str {
        "замена слов из списка"
    }
    fn apply_unprotected(&self, text: &str) -> String {
        if !self.enabled || self.rules.is_empty() {
            return text.to_string();
        }
        apply_compiled_replacements(text, &self.rules).0
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
    fn apply_unprotected(&self, text: &str) -> String {
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
    fn protected_spans(&self, _text: &str) -> Vec<std::ops::Range<usize>> {
        // This step only appends punctuation and needs to see the actual ending.
        Vec::new()
    }
    fn name(&self) -> &str {
        "Final punctuation"
    }
    fn description(&self) -> &str {
        "точка в конце если нет знаков"
    }
    fn apply_unprotected(&self, text: &str) -> String {
        if !self.enabled || text.is_empty() {
            return text.to_string();
        }
        if crate::text_protection::code_spans(text)
            .last()
            .is_some_and(|span| span.end == text.len())
        {
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
    #[serde(default)]
    pub dictionary_sets: Vec<crate::dictionaries::DictionarySet>,
    #[serde(default)]
    pub dictionary_spellings: Vec<String>,
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
    #[serde(default = "default_true")]
    pub correct_spelling: bool,
    #[serde(default)]
    pub split_sentences: bool,
    /// Identifiers of the enabled ready-made sets ([`DICTIONARY_PRESETS`]).
    ///
    /// Stored as a list of ids rather than a copy of the words precisely for the
    /// sake of the switch: a set can be removed in one motion and without loss,
    /// while the user's own dictionary is untouched — it lives separately in
    /// `custom_words` and stays what the person wrote by hand.
    #[serde(default)]
    pub enabled_presets: Vec<String>,
    #[serde(default = "default_true")]
    pub capitalize_sentences: bool,
    #[serde(default = "default_true")]
    pub final_punctuation: bool,
    #[serde(default)]
    pub custom_parasite_words: Vec<String>,
    /// Built-in parasite words the user switched off, stored as the words
    /// themselves.
    ///
    /// The list holds what is OFF rather than what is on, so that the default
    /// stays live: a word added to the constant later starts working for
    /// everyone, and a word removed from it simply stops being offered. Storing
    /// the enabled set instead would freeze each config at the day it was first
    /// opened — the same trap `effectiveSystemPrompt` documents for prompts.
    #[serde(default)]
    pub disabled_parasite_words: Vec<String>,
    /// Ids of the built-in sets ([`PARASITE_PRESETS`]) that are switched on.
    ///
    /// `None` means the user has never touched the selection, and each set then
    /// applies according to its own `default_on` — that is what keeps the
    /// Russian set working for everybody and the English one off until someone
    /// asks for it. `Some` is an explicit choice and is obeyed as written,
    /// including `Some([])`, which an enabled-set list alone could not tell
    /// apart from «never configured».
    #[serde(default)]
    pub parasite_sets: Option<Vec<String>>,
    /// Names, brands, terms and jargon the engine cannot know. An empty list
    /// means the step does not run at all.
    #[serde(default)]
    pub custom_words: Vec<String>,
}

impl TextFormattingConfig {
    /// The dictionary that is actually applied: the user's words plus the words
    /// of every enabled set.
    ///
    /// The user's own words come first and win on a collision: if a person
    /// entered a term in their own spelling, a set has no right to replace it
    /// with its own. The comparison ignores case, otherwise `Cargo` from a set
    /// and `cargo` from the user's list would become two different terms and
    /// both would fight over the same window of text.
    pub fn effective_custom_words(&self) -> Vec<String> {
        crate::dictionaries::effective_words(self)
    }

    /// The built-in sets that are switched on.
    pub fn active_parasite_sets(&self) -> Vec<&'static ParasitePreset> {
        PARASITE_PRESETS
            .iter()
            .filter(|preset| match &self.parasite_sets {
                Some(chosen) => chosen.iter().any(|id| id == preset.id),
                None => preset.default_on,
            })
            .collect()
    }

    /// Every built-in word in force, before the per-word switches are applied.
    pub fn active_parasite_words(&self) -> Vec<&'static str> {
        self.active_parasite_sets()
            .into_iter()
            .flat_map(|preset| preset.words.iter().copied())
            .collect()
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
            correct_spelling: true,
            // Off by default: sentence splitting rewrites the user's
            // phrasing, which is a bigger intervention than the cleanup
            // steps above.
            split_sentences: false,
            capitalize_sentences: true,
            final_punctuation: true,
            custom_parasite_words: Vec::new(),
            disabled_parasite_words: Vec::new(),
            parasite_sets: None,
            custom_words: Vec::new(),
            enabled_presets: Vec::new(),
            dictionary_sets: Vec::new(),
            dictionary_spellings: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FormatterConfig {
    /// The dictation language, read from the ROOT of the config rather than
    /// from `text_formatting` — that is where the setting lives, and both the
    /// live path and `preview_format` hand the whole config value over, so the
    /// field arrives on its own.
    ///
    /// `None` is «not told»: an old config, or a preview built from a fragment.
    /// Steps that could damage a language they were not written for treat it as
    /// a foreign language rather than as a permission.
    #[serde(default)]
    pub language: Option<String>,
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
    replacement_protection: Vec<Regex>,
}

impl Formatter {
    /// Build a pipeline from the typed settings and prepare its matchers once.
    pub fn from_config(config: &FormatterConfig) -> Self {
        let fmt = &config.text_formatting;
        let rules = if config.replacements_paused {
            Vec::new()
        } else {
            config.replacement_rules.clone()
        };
        let replacements_enabled = !rules.is_empty();
        let words = fmt.effective_custom_words();
        let spelling_terms = words.clone();
        let replacements = ContextReplacements::new(replacements_enabled, rules);
        let replacement_protection = replacements
            .rules
            .iter()
            .filter(|_| replacements_enabled)
            .map(|compiled| compiled.pattern.clone())
            .collect();

        let steps: Vec<Box<dyn FormatStep>> = vec![
            Box::new(HallucinationCleaner::new(fmt.remove_hallucinations)),
            Box::new(FillerWordsRemover::new(
                fmt.remove_fillers,
                config.language.as_deref(),
            )),
            Box::new(ParasiteWordsRemover::new(
                fmt.remove_parasites,
                fmt.custom_parasite_words.clone(),
                fmt.disabled_parasite_words.clone(),
                fmt.active_parasite_words(),
            )),
            Box::new(DuplicateWordsRemover::new(fmt.remove_duplicates)),
            // After the single-word dedupe: with "да да да да" already
            // reduced to "да", the phrase collapser cannot mistake a run of
            // one repeated word for a two-word loop.
            Box::new(PhraseLoopCollapser::new(fmt.collapse_phrase_loops)),
            Box::new(CommaCleaner::new(fmt.clean_commas)),
            Box::new(SpaceNormalizer::new(fmt.normalize_spaces)),
            // After whitespace normalisation — word windows are cut on single
            // spaces rather than on random clumps. And before the replacement
            // rules: a user rule must see the already-corrected term rather than
            // what the engine thought it heard.
            Box::new(CustomWordsCorrector::new(!words.is_empty(), words)),
            Box::new(crate::spelling::RussianSpellingCorrector::new(
                fmt.correct_spelling,
                config.language.as_deref(),
                &spelling_terms,
            )),
            Box::new(SentenceSplitter::new(fmt.split_sentences)),
            Box::new(replacements),
            Box::new(Capitalizer::new(fmt.capitalize_sentences)),
            Box::new(PunctuationFinalizer::new(fmt.final_punctuation)),
        ];
        Self {
            enabled: fmt.enabled,
            steps,
            replacement_protection,
        }
    }

    /// Apply the pipeline. `paused` short-circuits to the trimmed
    /// input (Python parity for `TextFormatter.process` when
    /// `text_formatting.enabled = False`).
    pub fn process(&self, text: &str) -> String {
        self.process_inner(text, false)
    }

    fn process_inner(&self, text: &str, restore_accidental_empty: bool) -> String {
        if !self.enabled {
            return text.trim().to_string();
        }
        let mut current = text.trim().to_string();
        if current.is_empty() {
            return current;
        }
        let mut before_replacements = true;
        for step in &self.steps {
            if step.enabled() {
                let had_text = !current.trim().is_empty();
                if step.edits_protected_text() {
                    current = step.apply(&current);
                    before_replacements = false;
                } else {
                    let mut spans = step.protected_spans(&current);
                    if before_replacements {
                        for pattern in &self.replacement_protection {
                            spans.extend(pattern.find_iter(&current).map(|m| m.range()));
                        }
                    }
                    current = crate::text_protection::apply(&current, spans, |s| {
                        step.apply_unprotected(s)
                    });
                }
                if had_text && current.trim().is_empty() && step.permits_empty_output() {
                    return String::new();
                }
            }
        }
        if restore_accidental_empty && current.trim().is_empty() {
            text.trim().to_string()
        } else {
            current
        }
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
/// malformed config falls back to defaults rather than erroring; partial
/// configs use the per-field defaults. Shared by the live dictation path
/// (`post_process_transcription` in lib.rs) and the Settings `preview_format`
/// command so the two can never diverge on how the formatter is built.
pub fn format_with_config_value(config: &Value, text: &str) -> String {
    formatter_from_value(config).process(text)
}

fn formatter_from_value(config: &Value) -> Formatter {
    let fmt_cfg: FormatterConfig = serde_json::from_value(config.clone()).unwrap_or_default();
    Formatter::from_config(&fmt_cfg)
}

/// Dictation/file delivery keeps the raw text if incidental cleanup erased it.
/// Silence hallucinations and explicit deletion rules deliberately stay empty.
pub fn format_transcription_with_config_value(config: &Value, text: &str) -> String {
    formatter_from_value(config).process_inner(text, true)
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
            // The tests that exercise the English sounds set this themselves.
            language: Some("ru".to_string()),
            text_formatting: TextFormattingConfig {
                enabled: true,
                remove_hallucinations: true,
                remove_fillers: true,
                remove_parasites: true,
                remove_duplicates: true,
                collapse_phrase_loops: true,
                clean_commas: true,
                normalize_spaces: true,
                correct_spelling: true,
                split_sentences: false,
                capitalize_sentences: true,
                final_punctuation: true,
                custom_parasite_words: Vec::new(),
                disabled_parasite_words: Vec::new(),
                parasite_sets: None,
                custom_words: Vec::new(),
                enabled_presets: Vec::new(),
                dictionary_sets: Vec::new(),
                dictionary_spellings: Vec::new(),
            },
            replacement_rules: Vec::new(),
            replacements_paused: false,
        }
    }

    #[test]
    fn local_cleanup_preserves_prose_and_repairs_only_unambiguous_errors() {
        let mut config = default_fmt();
        config.text_formatting.enabled_presets = vec!["development".into()];
        let formatter = Formatter::from_config(&config);
        for (input, expected) in [
            (
                "заголовок редактора готов, но стал меньше",
                "Заголовок редактора готов, но стал меньше.",
            ),
            (
                "можно стать лучше, а не хуже",
                "Можно стать лучше, а не хуже.",
            ),
            ("это работает? Ну, продолжим", "Это работает? Продолжим."),
            (
                "трудно описать. Ну, попробуем",
                "Трудно описать. Попробуем.",
            ),
            ("чё-то изменилось", "Чё-то изменилось."),
            (
                "нажали.Затем проверили,кажется всё готово",
                "Нажали. Затем проверили, кажется всё готово.",
            ),
            ("по итогуу нужна проверкка", "По итогу нужна проверка."),
            (
                "передай какомуто человеку компьюетр",
                "Передай какому-то человеку компьютер.",
            ),
        ] {
            assert_eq!(formatter.process(input), expected, "input: {input}");
            assert_eq!(
                formatter.process(expected),
                expected,
                "must be stable: {expected}"
            );
        }
    }

    #[test]
    fn new_spelling_option_works_in_live_and_preview_configs() {
        let mut config = serde_json::json!({"language": "ru"});
        let input = "по итогуу";
        assert_eq!(format_with_config_value(&config, input), "По итогу.");
        assert_eq!(
            preview_format(input, &config).unwrap().formatted,
            "По итогу."
        );
        config["text_formatting"] = serde_json::json!({"correct_spelling": false});
        assert_eq!(format_with_config_value(&config, input), "По итогуу.");
        config["text_formatting"] = serde_json::json!({"enabled": false});
        assert_eq!(format_with_config_value(&config, input), input);
    }

    #[test]
    fn local_cleanup_preserves_comparisons_and_fixed_expressions() {
        let formatter = Formatter::from_config(&default_fmt());
        for (input, expected) in [
            (
                "после правок текст стал короче",
                "После правок текст стал короче.",
            ),
            ("в общем и целом всё готово", "В общем и целом всё готово."),
            ("чуть чуть поправь текст", "Чуть чуть поправь текст."),
            ("еле еле успели", "Еле еле успели."),
            ("короче, надо решать", "Надо решать."),
            ("ну короче надо решать", "Надо решать."),
            ("Ну короче надо решать", "Надо решать."),
            ("проверь пример.рф", "Проверь пример.рф."),
            ("готово.затем проверили", "готово.затем проверили."),
        ] {
            assert_eq!(formatter.process(input), expected);
            assert_eq!(formatter.process(expected), expected);
        }
    }

    #[test]
    fn spelling_preserves_explicit_replacement_triggers_and_output_spacing() {
        let mut config = default_fmt();
        config.replacement_rules = vec![word_rule("итогуу", "конец  проверки")];
        assert_eq!(
            Formatter::from_config(&config).process("итогуу"),
            "Конец  проверки."
        );
    }

    #[test]
    fn cleanup_keeps_conjunctions_compounds_and_technical_punctuation() {
        let cleaner = CommaCleaner::new(true);
        for text in [
            "лучше, но медленнее",
            "не слева, а справа",
            "он ушёл, и дверь закрылась",
        ] {
            assert_eq!(cleaner.apply(text), text);
        }
        let normalizer = SpaceNormalizer::new(true);
        for text in [
            "Cargo.toml 3.14 https://example.com/a?b=1",
            "А.Б.В. пример.рф",
            "путь/файл.Название",
            "`тест,пример`",
        ] {
            assert_eq!(normalizer.apply(text), text);
        }
        assert_eq!(
            SpaceNormalizer::new(false).apply("готово.Затем"),
            "готово.Затем"
        );
        assert_eq!(
            FillerWordsRemover::new(true, Some("ru")).apply("будетээ-э подсказка"),
            "будетээ-э подсказка"
        );
    }

    #[test]
    #[ignore = "needs SOTTO_CORPUS, SOTTO_FORMAT_CONFIG and SOTTO_FORMATTED outside the repository"]
    fn format_over_corpus() {
        use std::io::Write;
        let config: Value = serde_json::from_str(
            &std::fs::read_to_string(
                std::env::var("SOTTO_FORMAT_CONFIG").expect("SOTTO_FORMAT_CONFIG"),
            )
            .unwrap(),
        )
        .unwrap();
        let corpus =
            std::fs::read_to_string(std::env::var("SOTTO_CORPUS").expect("SOTTO_CORPUS")).unwrap();
        let mut output = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(std::env::var("SOTTO_FORMATTED").expect("SOTTO_FORMATTED"))
            .unwrap();
        let mut timings = Vec::new();
        for line in corpus.lines() {
            let started = std::time::Instant::now();
            let formatted = format_with_config_value(&config, line);
            timings.push(started.elapsed().as_secs_f64() * 1000.0);
            writeln!(output, "{}", serde_json::to_string(&formatted).unwrap()).unwrap();
        }
        let first = timings.first().copied().unwrap_or_default();
        timings.sort_by(f64::total_cmp);
        eprintln!(
            "{} entries; first={first:.1} ms; median={:.1} ms; max={:.1} ms",
            timings.len(),
            timings[timings.len() / 2],
            timings.last().unwrap()
        );
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

    // ----- Words the cleanup must never take away -----

    /// «Да» and «нет» are the answer, not padding around it. Deleting them
    /// does not tidy the sentence up, it reverses or empties it — and the
    /// author has no way to see that a word was taken.
    #[test]
    fn an_answer_of_yes_or_no_survives_the_cleanup() {
        let formatter = Formatter::from_config(&default_fmt());
        assert_eq!(
            formatter.process("он спросил приедешь ли я сказал нет"),
            "Он спросил приедешь ли я сказал нет."
        );
        assert_eq!(
            formatter.process("нет не надо это мержить"),
            "Нет не надо это мержить."
        );
        assert_eq!(
            formatter.process("да я согласен давай так и сделаем"),
            "Да я согласен давай так и сделаем."
        );
    }

    /// «Вот» opens the sentence it emphasises; removing it changes what is
    /// being pointed at.
    #[test]
    fn a_demonstrative_particle_survives_the_cleanup() {
        let formatter = Formatter::from_config(&default_fmt());
        assert_eq!(
            formatter.process("вот это и есть главная проблема"),
            "Вот это и есть главная проблема."
        );
    }

    /// «Значит» and «собственно» are parasites only as interjections. Deleting
    /// them everywhere takes out a predicate and a qualifier.
    #[test]
    fn znachit_and_sobstvenno_survive_as_ordinary_words() {
        let formatter = Formatter::from_config(&default_fmt());
        assert_eq!(
            formatter.process("это значит что мы опоздали"),
            "Это значит что мы опоздали."
        );
        assert_eq!(
            formatter.process("посмотри на собственно код а не на тесты"),
            "Посмотри на собственно код а не на тесты."
        );
    }

    // ----- English filler sounds -----

    /// A dictation declared as English.
    fn english_fmt() -> FormatterConfig {
        let mut config = default_fmt();
        config.language = Some("en".to_string());
        config
    }

    /// Before these patterns existed the whole cleanup was a no-op on English
    /// dictation: every built-in word and every filler pattern was Cyrillic, so
    /// both switches did nothing while the settings said otherwise.
    #[test]
    fn english_filler_sounds_are_removed() {
        let formatter = Formatter::from_config(&english_fmt());
        assert_eq!(
            formatter.process("uh i think umm we should ship it"),
            "I think we should ship it."
        );
        assert_eq!(
            formatter.process("er we could ahh try the other one"),
            "We could try the other one."
        );
        assert_eq!(
            formatter.process("hmm let me check that"),
            "Let me check that."
        );
    }

    /// The engine capitalises the first word of a sentence, and a filler is
    /// very often that word.
    #[test]
    fn a_capitalised_filler_is_removed_too() {
        let formatter = Formatter::from_config(&english_fmt());
        assert_eq!(formatter.process("Um, I think so"), "I think so.");
    }

    /// The English sounds are ordinary words in the other languages written in
    /// the same alphabet, so they must not run on a dictation that is not
    /// English. «Er» is a German pronoun and «um» a German preposition: ungated,
    /// the patterns turned «Er kommt um acht» into «Kommt acht».
    #[test]
    fn english_sounds_do_not_touch_another_latin_language() {
        let mut config = default_fmt();
        config.language = Some("de".to_string());
        let formatter = Formatter::from_config(&config);
        assert_eq!(formatter.process("Er kommt um acht"), "Er kommt um acht.");
        assert_eq!(
            formatter.process("Ich gehe um sieben, er auch"),
            "Ich gehe um sieben, er auch."
        );

        config.language = Some("nl".to_string());
        let formatter = Formatter::from_config(&config);
        assert_eq!(
            formatter.process("Er is niets om te zien"),
            "Er is niets om te zien."
        );
    }

    /// «auto» and a config too old to carry the setting are «not told», which
    /// is not permission: a missed filler costs a word of noise, a swallowed
    /// pronoun costs the sentence.
    #[test]
    fn an_unknown_language_does_not_get_the_english_sounds() {
        let mut config = default_fmt();
        config.language = Some("auto".to_string());
        let formatter = Formatter::from_config(&config);
        assert_eq!(formatter.process("Er kommt um acht"), "Er kommt um acht.");

        config.language = None;
        let formatter = Formatter::from_config(&config);
        assert_eq!(formatter.process("Er kommt um acht"), "Er kommt um acht.");
    }

    /// The Russian sounds are not gated, and need not be: Cyrillic cannot match
    /// a text written in another alphabet.
    #[test]
    fn russian_sounds_run_whatever_the_language_says() {
        let mut config = default_fmt();
        config.language = Some("en".to_string());
        let formatter = Formatter::from_config(&config);
        assert_eq!(formatter.process("э-э я забыл"), "Я забыл.");
    }

    /// The gate lives on the dictation language, not on the word sets: a filler
    /// sound is a separate step, and switching the English word set on must not
    /// be what turns the sounds on (nor off).
    #[test]
    fn the_sound_gate_is_the_language_not_the_word_set() {
        let mut config = english_fmt();
        config.text_formatting.parasite_sets = Some(Vec::new());
        let formatter = Formatter::from_config(&config);
        assert_eq!(formatter.process("um i think so"), "I think so.");
    }

    /// The point of the whole exercise: a sound may go, a word may not.
    #[test]
    fn english_words_that_look_like_fillers_survive() {
        let formatter = Formatter::from_config(&default_fmt());
        // "err" is a verb, "mm" is millimetres, and "oh" carries the line it
        // opens — none of them are on the list.
        assert_eq!(formatter.process("to err is human"), "To err is human.");
        assert_eq!(
            formatter.process("cut it to 5 mm exactly"),
            "Cut it to 5 mm exactly."
        );
        assert_eq!(
            formatter.process("oh that explains it"),
            "Oh that explains it."
        );
        // Ordinary words that merely start with the same letters.
        assert_eq!(
            formatter.process("uhuru ahead of umbrella hmx"),
            "Uhuru ahead of umbrella hmx."
        );
    }

    /// The Latin patterns run on every dictation and must be inert on Russian,
    /// exactly as the Cyrillic ones are inert on English.
    #[test]
    fn latin_patterns_do_not_touch_russian() {
        let formatter = Formatter::from_config(&default_fmt());
        assert_eq!(
            formatter.process("речь о том что а потом мы всё переделали"),
            "Речь о том что а потом мы всё переделали."
        );
    }

    /// A built-in word the user switched off must stop being removed, while the
    /// rest of the list keeps working.
    #[test]
    fn a_switched_off_builtin_word_is_kept() {
        let mut config = default_fmt();
        config.text_formatting.disabled_parasite_words = vec!["короче".to_string()];
        let formatter = Formatter::from_config(&config);
        assert_eq!(
            formatter.process("короче ну надо решать"),
            "Короче надо решать."
        );
    }

    /// The stored word comes back from a text field, so it arrives with
    /// whatever spacing and case the person typed.
    #[test]
    fn switching_off_ignores_case_and_padding() {
        let mut config = default_fmt();
        config.text_formatting.disabled_parasite_words = vec!["  Короче  ".to_string()];
        let formatter = Formatter::from_config(&config);
        assert_eq!(
            formatter.process("короче надо решать"),
            "Короче надо решать."
        );
    }

    /// Switching a built-in word off must not disarm the user's own additions.
    #[test]
    fn switching_off_a_builtin_leaves_custom_words_working() {
        let mut config = default_fmt();
        config.text_formatting.disabled_parasite_words = vec!["короче".to_string()];
        config.text_formatting.custom_parasite_words = vec!["скажем так".to_string()];
        let formatter = Formatter::from_config(&config);
        assert_eq!(
            formatter.process("короче скажем так надо решать"),
            "Короче надо решать."
        );
    }

    // ----- Built-in sets -----

    /// English words are shipped but not applied: the set is opt-in because
    /// «like», «well» and «right» are ordinary vocabulary.
    #[test]
    fn the_english_set_is_off_until_it_is_asked_for() {
        let formatter = Formatter::from_config(&default_fmt());
        assert_eq!(
            formatter.process("basically i mean we could just like ship it right"),
            "Basically i mean we could just like ship it right."
        );
    }

    #[test]
    fn switching_the_english_set_on_applies_it() {
        let mut config = english_fmt();
        config.text_formatting.parasite_sets = Some(vec!["ru".to_string(), "en".to_string()]);
        let formatter = Formatter::from_config(&config);
        assert_eq!(
            formatter.process("basically we could ship it"),
            "We could ship it."
        );
    }

    /// The reason the matching had to stop being case-sensitive: an engine
    /// capitalises the first word of a sentence, and «I» is never dictated
    /// lower case at all, so the whole English set would have missed itself.
    #[test]
    fn the_english_set_matches_the_case_an_engine_actually_writes() {
        let mut config = english_fmt();
        config.text_formatting.parasite_sets = Some(vec!["en".to_string()]);
        let formatter = Formatter::from_config(&config);
        assert_eq!(formatter.process("Basically we can begin"), "We can begin.");
        assert_eq!(
            formatter.process("it is, I mean, the same thing"),
            "It is, the same thing."
        );
    }

    /// Case folding reaches the Russian set too, which had quietly been missing
    /// a sentence-leading «Ну» all along.
    #[test]
    fn a_capitalised_russian_parasite_is_removed() {
        let formatter = Formatter::from_config(&default_fmt());
        assert_eq!(formatter.process("Ну, поехали"), "Поехали.");
    }

    /// Choosing the sets is an explicit act, and choosing none of them is a
    /// choice too — it must not read as «never configured» and fall back to the
    /// defaults.
    #[test]
    fn an_empty_selection_is_a_choice_not_a_default() {
        let mut config = default_fmt();
        config.text_formatting.parasite_sets = Some(Vec::new());
        let formatter = Formatter::from_config(&config);
        assert_eq!(
            formatter.process("ну короче надо решать"),
            "Ну короче надо решать."
        );
        assert!(config.text_formatting.active_parasite_words().is_empty());
    }

    /// An untouched config resolves to each set's own default.
    #[test]
    fn an_untouched_config_takes_the_defaults() {
        let config = default_fmt();
        let ids: Vec<&str> = config
            .text_formatting
            .active_parasite_sets()
            .iter()
            .map(|preset| preset.id)
            .collect();
        assert_eq!(ids, vec!["ru"]);
    }

    /// Switching the Russian set off leaves the user's own words working: the
    /// two live in different fields for exactly this reason.
    #[test]
    fn switching_a_set_off_leaves_custom_words_working() {
        let mut config = default_fmt();
        config.text_formatting.parasite_sets = Some(Vec::new());
        config.text_formatting.custom_parasite_words = vec!["скажем так".to_string()];
        let formatter = Formatter::from_config(&config);
        assert_eq!(
            formatter.process("ну скажем так надо решать"),
            "Ну надо решать."
        );
    }

    /// The interface offers a chip for every word of every active set, so every
    /// one of them has to be switchable — otherwise settings show a switch that
    /// does nothing.
    #[test]
    fn every_builtin_word_can_be_switched_off() {
        let mut config = default_fmt();
        // Both sets on, so the check covers the English words too.
        config.text_formatting.parasite_sets = Some(vec!["ru".to_string(), "en".to_string()]);
        let words = config.text_formatting.active_parasite_words();
        config.text_formatting.disabled_parasite_words =
            words.iter().map(|word| word.to_string()).collect();
        let formatter = Formatter::from_config(&config);
        let text = words.join(" ");
        assert_eq!(
            formatter
                .process(&text)
                .to_lowercase()
                .trim_end_matches('.'),
            text
        );
    }

    /// The conjunction «а» and the preposition «о» are single letters, and the
    /// filler patterns used to swallow both. Losing them does not remove a
    /// sound, it breaks the grammar of the phrase around it.
    #[test]
    fn single_letter_conjunction_and_preposition_survive() {
        let formatter = Formatter::from_config(&default_fmt());
        assert_eq!(
            formatter.process("речь о том что а потом мы всё переделали"),
            "Речь о том что а потом мы всё переделали."
        );
        assert_eq!(
            formatter.process("поговорим о проекте"),
            "Поговорим о проекте."
        );
    }

    /// The held-out sound is still a filler and must still go: the fix narrows
    /// the patterns, it does not switch them off.
    #[test]
    fn a_held_vowel_is_still_removed() {
        let formatter = Formatter::from_config(&default_fmt());
        assert_eq!(
            formatter.process("а-а-а я забыл про встречу"),
            "Я забыл про встречу."
        );
        assert_eq!(
            formatter.process("ааа это была моя ошибка"),
            "Это была моя ошибка."
        );
        assert_eq!(formatter.process("о-о теперь понятно"), "Теперь понятно.");
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

    /// A mirror of the test above. That one checks the rule stays silent while
    /// paused, but on its own it stays green in a world where the rule never
    /// fires at all. This one covers the other half and with it the branch
    /// choice for building rules in `from_config` and the computation of
    /// `replacements_enabled`.
    #[test]
    fn replacement_rules_apply_when_not_paused() {
        let mut cfg = default_fmt();
        cfg.replacement_rules = vec![word_rule("тайпскрипт", "TypeScript")];
        cfg.replacements_paused = false;
        let formatter = Formatter::from_config(&cfg);
        let out = formatter.process("тайпскрипт рядом");
        assert!(
            out.contains("TypeScript"),
            "the replacement rule must fire while not paused; got: {out}"
        );
    }

    /// A rule from an old config carrying `stage: post_llm` used to run nowhere:
    /// the pipeline applied replacements only at the `pre_llm` stage, and
    /// `post_llm` was passed by the preview command alone. The stages are gone
    /// while the field survives in other people's configs — and it must be
    /// ignored, that is, the rule now fires.
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

    /// This is the case it was all started for: the engine heard something close
    /// but wrote in Cyrillic what is spelled in Latin.
    #[test]
    fn misheard_term_gets_the_spelling_from_the_dictionary() {
        assert_eq!(
            correct(&["Tauri"], "переписал на таури вчера"),
            "переписал на Tauri вчера"
        );
    }

    /// The user sets the spelling, but the start of a sentence belongs to the
    /// text rather than to the term.
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

    /// The punctuation is glued to the word but belongs to the sentence.
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

    /// A long term must beat a short one, otherwise «Клод Код» falls apart into
    /// «Claude» plus garbage.
    #[test]
    fn the_longer_term_wins() {
        assert_eq!(
            correct(&["Claude", "Claude Code"], "открыл клауд код"),
            "открыл Claude Code"
        );
    }

    /// The dictionary's main risk is spoiling healthy text. Dissimilar words
    /// must not be touched, even when they are the same length.
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

    /// «ё» and «е» are indistinguishable by ear, and dictation confuses them.
    #[test]
    fn yo_and_ye_are_the_same_sound() {
        assert_eq!(correct(&["Шёпот"], "запустил шепот"), "запустил Шёпот");
    }

    /// Short terms produce too many false positives to be matched fuzzily.
    #[test]
    fn very_short_terms_are_ignored_entirely() {
        assert_eq!(correct(&["Go"], "го дальше"), "го дальше");
    }

    #[test]
    fn an_empty_dictionary_is_a_no_op() {
        assert_eq!(correct(&[], "любой текст"), "любой текст");
        assert_eq!(correct(&["   "], "любой текст"), "любой текст");
    }

    /// The step is disabled by an empty list, but the explicit flag must work
    /// too.
    #[test]
    fn a_disabled_step_changes_nothing() {
        let corrector = CustomWordsCorrector::new(false, vec!["Tauri".to_string()]);
        assert_eq!(corrector.apply("таури"), "таури");
    }

    /// Precisely what the phonetic folding was needed for: the word is written
    /// in a different alphabet, shares zero characters, and is the same word.
    #[test]
    fn a_latin_term_is_found_behind_cyrillic_spelling() {
        assert_eq!(fold_for_match("Tauri"), fold_for_match("таури"));
        assert_eq!(fold_for_match("Ollama"), fold_for_match("олама"));
    }

    /// The counterpart to the converging cases above — and it is mandatory.
    ///
    /// Every other check of the folding compares its output against its own
    /// output: "these two words give the same thing". Such an assertion survives
    /// a folding that always returns an empty string — the equality still holds.
    /// This one pins the opposite: different words must diverge, which means the
    /// folding must actually do something.
    #[test]
    fn folding_keeps_different_words_apart() {
        assert_ne!(fold_for_match("Tauri"), fold_for_match("тайна"));
        assert_ne!(fold_for_match("Ollama"), fold_for_match("оладьи"));
        assert_ne!(fold_for_match("город"), fold_for_match("град"));
        assert_ne!(fold_for_match("chat"), fold_for_match("шелл"));
        // And it does not collapse into nothing: after folding a word is still
        // a word.
        assert!(!fold_for_match("Tauri").is_empty());
        assert_eq!(fold_for_match("Tauri").chars().count(), 5);
    }

    /// Words alike in their consonants but not in sound must not be touched. It
    /// is on exactly this pair that the discarded consonant-skeleton variant fell
    /// apart — see the comment on `similarity`.
    #[test]
    fn words_that_share_consonants_are_still_different_words() {
        assert_eq!(correct(&["Град"], "город проснулся"), "город проснулся");
        assert!(!within_budget(
            &fold_for_match("город"),
            &fold_for_match("град")
        ));
    }

    // ── Real dictation ──────────────────────────────────────────────────
    //
    // Four phrases dictated on GigaAM v3 and on Whisper large-v3-turbo without
    // LLM post-processing, with a known source text (see #20 and #31). Invented
    // cases say nothing about false positives: those arise on words nobody would
    // think to list.

    /// A stack dictionary as a person dictating about their own project would
    /// set it up.
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

    /// A four-letter abbreviation with one wrong letter. Under the old relative
    /// threshold that is 0.75 — that is, `NSIS` was never restored, however
    /// obvious the miss looked.
    #[test]
    fn a_short_acronym_survives_one_wrong_letter() {
        assert_eq!(fix("проверь NSYS Installer"), "проверь NSIS Installer");
        assert_eq!(fix("проверь энсис инсталлятор"), "проверь NSIS инсталлятор");
    }

    /// A long identifier the engine broke into two tokens while losing a
    /// syllable. Under the old threshold that is 0.769, just below the cut-off.
    #[test]
    fn a_long_identifier_survives_several_errors() {
        assert_eq!(fix("В структуре тлок лежит"), "В structured_log лежит");
        // Both words are valid Russian: a fuzzy term must not override them.
        assert_eq!(fix("В структуре лог лежит"), "В структуре лог лежит");
    }

    /// A file name with a dot: the engines hear it as two words or as one, and
    /// both variants must converge on the dictionary form.
    #[test]
    fn a_dotted_file_name_is_restored_from_either_segmentation() {
        assert_eq!(fix("Поправь карга томол и"), "Поправь Cargo.toml и");
        assert_eq!(fix("Поправь карготомал и"), "Поправь Cargo.toml и");
    }

    /// The budget's upper bound. Here the engines heard **a different word**
    /// rather than writing `writer` in another alphabet: a spare «б» in one case,
    /// an invented preposition and a different consonant in the other. Stretching
    /// the threshold to reach them means starting to substitute words, and a
    /// substituted one looks like something the person supposedly dictated
    /// themselves.
    #[test]
    fn a_misheard_word_is_not_stretched_into_a_dictionary_term() {
        assert_eq!(fix("лежит брайтер он"), "лежит брайтер он");
        assert_eq!(fix("лежит в райдере, он"), "лежит в райдере, он");
    }

    /// Cyrillic that reads correctly but is spelled otherwise: the bulk of
    /// technical dictation. This is what the dictionary is for.
    #[test]
    fn readable_cyrillic_is_pulled_to_the_dictionary_spelling() {
        assert_eq!(fix("открой пул реквест"), "открой pull request");
        assert_eq!(fix("прогони клипи с флагом"), "прогони clippy с флагом");
        assert_eq!(fix("через билд инстеллер"), "через build-installer");
        assert_eq!(fix("держит хэндл файла"), "держит handle файла");
    }

    /// The price of a narrow budget on short terms, recorded honestly.
    ///
    /// The second engine heard the same word as «хендел»: the extra syllable
    /// takes it three edits away from the folded `handl` on a five-character key.
    /// The budget must not be widened for this — the same motion would drag
    /// «брайтер» to `writer`. A person sees and fixes a miss; a substitution
    /// looks like something they supposedly dictated themselves.
    #[test]
    fn a_short_term_can_be_missed_when_a_whole_syllable_is_wrong() {
        assert_eq!(fix("держит хендел файла"), "держит хендел файла");
    }

    /// The step does not touch words outside the dictionary even when they stand
    /// right next to a matched term: the person never entered «денай ворнингс»
    /// into the dictionary.
    #[test]
    fn words_outside_the_dictionary_are_left_as_dictated() {
        assert_eq!(
            fix("прогони клипи с флагом денай ворнингс"),
            "прогони clippy с флагом денай ворнингс"
        );
    }

    /// The budget grows with length but not without limit: at six characters it
    /// stays equal to one, otherwise `writer` would drag «брайтер» in.
    #[test]
    fn the_budget_grows_with_length_but_stays_tight_on_short_terms() {
        assert_eq!(edit_budget(4), 1);
        assert_eq!(edit_budget(6), 1);
        assert_eq!(edit_budget(7), 2);
        assert_eq!(edit_budget(13), 3);
    }

    /// Different spellings of one sound must not count as different words.
    #[test]
    fn spelling_variants_of_one_sound_collapse() {
        assert_eq!(fold_for_match("Phil"), fold_for_match("Фил"));
        assert_eq!(fold_for_match("code"), fold_for_match("код"));
        assert_eq!(fold_for_match("Nick"), fold_for_match("Ник"));
    }

    /// The digraphs must survive the `c → k` replacement, otherwise «ч» and «ш»
    /// are taken apart and a word stops matching itself.
    #[test]
    fn digraphs_survive_the_single_letter_pass() {
        assert_eq!(fold_for_match("Чат"), fold_for_match("chat"));
        assert_eq!(fold_for_match("Шелл"), fold_for_match("shell"));
    }

    /// The soft and hard signs carry no sound and must not disturb the
    /// comparison.
    #[test]
    fn soft_signs_are_silent() {
        assert_eq!(fold_for_match("Гугль"), fold_for_match("гугл"));
    }

    // ── Ready-made sets ─────────────────────────────────────────────────

    /// Ordinary Russian speech without a single technical term. It serves as a
    /// synthetic false-positive check: a set of some fifty words has no right to
    /// touch a single word here.
    ///
    /// The words are chosen to strike the known danger spots: «буфет» next to
    /// `buffer`, «комитет» next to `commit`, «морж» next to `merge`, «треть»
    /// next to `thread`, «дебют» next to `debug`.
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
            .unwrap_or_else(|| panic!("no preset {id}"))
            .words
            .iter()
            .map(|w| w.to_string())
            .collect()
    }

    /// A term that is shorter than the threshold after folding is silently
    /// ignored by the dictionary. Promising such a term to the user is
    /// dishonest: they see it in the set and assume it works.
    ///
    /// This is how `Vite` (→ `vit`, silent `e`) and `Node` (→ `nod`) are
    /// filtered out.
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
                    "«{word}» appears twice in set {}",
                    set.id
                );
                seen.push(lower);
            }
        }
    }

    /// The main risk of a ready-made set: a person pressed one button and got
    /// fifty terms, each of which may land on an ordinary word. A person writes
    /// their own dictionary and answers for it; the set is supplied by us.
    #[test]
    fn a_preset_leaves_ordinary_russian_alone() {
        for set in DICTIONARY_PRESETS {
            let corrector = CustomWordsCorrector::new(true, preset(set.id));
            for line in PLAIN_RUSSIAN {
                assert_eq!(
                    corrector.apply(line),
                    line,
                    "set {} rewrote ordinary speech",
                    set.id
                );
            }
        }
    }

    /// And what it is all for: a set must fix real dictation without a single
    /// line entered by hand.
    #[test]
    fn the_development_preset_fixes_real_dictation() {
        let corrector = CustomWordsCorrector::new(true, preset("development"));
        assert_eq!(corrector.apply("открой пул реквест"), "открой pull request");
        assert_eq!(corrector.apply("поправь карга томол"), "поправь Cargo.toml");
        assert_eq!(corrector.apply("прогони клипи"), "прогони clippy");
    }

    #[test]
    fn short_unrelated_terms_and_ambiguous_brands_are_preserved() {
        let corrector = CustomWordsCorrector::new(true, preset("development"));
        for input in [
            "получили буст",
            "Rest Asuret",
            "скли запрос",
            "гитхаб",
            "Githabe",
        ] {
            assert_eq!(corrector.apply(input), input);
        }
        for terms in [["GitHub", "GitLab"], ["GitLab", "GitHub"]] {
            assert_eq!(correct(&terms, "гитхаб"), "гитхаб");
            assert_eq!(correct(&terms, "GitHub"), "GitHub");
            assert_eq!(correct(&terms, "GitLab"), "GitLab");
        }
        assert_eq!(corrector.apply("код на Rust"), "код на Rust");
        assert_eq!(corrector.apply("гитлаб"), "GitLab");
    }

    // ── Enabling and disabling sets ─────────────────────────────────────

    fn fmt_with(words: &[&str], presets: &[&str]) -> TextFormattingConfig {
        TextFormattingConfig {
            custom_words: words.iter().map(|w| w.to_string()).collect(),
            enabled_presets: presets.iter().map(|p| p.to_string()).collect(),
            ..TextFormattingConfig::default()
        }
    }

    /// This is why a set is stored as an identifier rather than a copy of the
    /// words: disabling it must be one motion and lossless.
    #[test]
    fn a_disabled_preset_contributes_nothing() {
        let off = fmt_with(&["Шёпот"], &[]);
        assert_eq!(off.effective_custom_words(), vec!["Шёпот".to_string()]);
    }

    #[test]
    fn an_enabled_preset_is_added_to_the_users_own_words() {
        let on = fmt_with(&["Шёпот"], &["development"]);
        let words = on.effective_custom_words();
        assert!(
            words.contains(&"Шёпот".to_string()),
            "the user's own word is gone"
        );
        assert!(
            words.contains(&"clippy".to_string()),
            "the preset's word did not arrive"
        );
        assert_eq!(words.len(), 1 + PRESET_DEVELOPMENT.len());
    }

    /// The user's own words come first and win: a person entered the term in
    /// their own spelling, and a set has no right to replace it with its own.
    #[test]
    fn the_users_own_spelling_wins_over_the_preset() {
        let both = fmt_with(&["cargo"], &["development"]);
        let words = both.effective_custom_words();
        assert!(words.contains(&"cargo".to_string()));
        assert!(
            !words.contains(&"Cargo".to_string()),
            "the set replaced the spelling the user chose"
        );
    }

    /// Rolling the app back to a version without such a set must not break
    /// formatting: an unknown id is skipped silently.
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

    /// A disabled set must not fix the text — otherwise the switch is
    /// decorative.
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

    /// Running the dictionary over real transcriptions.
    ///
    /// The only danger of this step is spoiling healthy text, and no invented
    /// case will tell you about it: a false positive arises on words nobody would
    /// think to list. Only what a person actually dictated answers the
    /// question.
    ///
    /// The dictionary is given through `SOTTO_DICT` (one term per line) and the
    /// corpus through `SOTTO_CORPUS`, as with `collapse_over_corpus`.
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
                // The whole line rather than word pairs: a replacement may glue
                // two tokens into one, after which a pairwise comparison
                // declares the entire rest of the line changed. The first
                // version of this test did exactly that — and drowned a genuine
                // finding in two hundred false lines.
                println!("--- before: {line}");
                println!("--- after:  {out}");
            }
        }
        println!(
            "{changed} of {total} transcriptions changed by a {}-term dictionary",
            dict.len()
        );
    }

    /// Punctuation frames a term on both sides. The trailing part was preserved
    /// from the start while the opening quote was lost — «таури» arrived as
    /// Tauri».
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

    /// The dictionary step has no right to rewrite the text's layout. The first
    /// version cut the input with split_whitespace and joined it with join(" ") —
    /// double spaces collapsed even with normalisation off, and newlines turned
    /// into spaces, that is paragraphs disappeared.
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
        // And the same when a replacement did take place.
        assert_eq!(
            correct(&["Tauri"], "первая\n\nтаури  здесь"),
            "первая\n\nTauri  здесь"
        );
    }

    /// Leading and trailing spaces belong to the text: they are removed by the
    /// trim at the pipeline's entrance, not by this step.
    #[test]
    fn leading_and_trailing_gaps_are_preserved() {
        assert_eq!(correct(&["Tauri"], "  таури  "), "  Tauri  ");
        assert_eq!(correct(&["Tauri"], "\n таури"), "\n Tauri");
    }

    /// A token of pure punctuation must neither match nor be duplicated.
    #[test]
    fn punctuation_only_tokens_pass_through_once() {
        assert_eq!(correct(&["Tauri"], "таури — это"), "Tauri — это");
        assert_eq!(correct(&["Tauri"], "... ..."), "... ...");
    }

    #[test]
    fn distance_is_measured_in_characters_not_bytes() {
        // Cyrillic is two bytes per character; a per-byte distance would give
        // twice the answer here and push the similarity below the threshold.
        let a: Vec<char> = "кот".chars().collect();
        let b: Vec<char> = "кит".chars().collect();
        assert_eq!(edit_distance(&a, &b), 1);
    }

    #[test]
    fn similarity_is_symmetric_and_bounded() {
        assert!((similarity("tauri", "tauri") - 1.0).abs() < f64::EPSILON);
        assert_eq!(similarity("tauri", "таури"), similarity("таури", "tauri"));
        // Not ">= 0.0": that threshold would also be passed by a function that
        // always returns one, and with it the two assertions above. Dissimilar
        // strings must receive a low score, not just any score.
        assert_eq!(similarity("abc", "xyz"), 0.0);
        assert!(similarity("tauri", "тайна") < 0.7);
    }

    // ------------------------------------------------------------------
    // Mutation coverage (queue 3): replacement rules, transliteration,
    // segmentation boundaries.
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
        // An empty structured array must not cut off the path to the legacy
        // `replacements` dictionary: swapping `!rules.is_empty()` for
        // `is_empty()` would return nothing.
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

    /// A newline is a separator too: a hallucinated first segment must go in
    /// this branch as well (`delete !` in `is_hallucinated_segment`).
    #[test]
    fn hallucination_walk_drops_newline_separated_segment() {
        let cleaner = HallucinationCleaner::new(true);
        let out = cleaner.apply("Спасибо за просмотр\nРеальный текст");
        assert_eq!(out, "Реальный текст");
    }

    /// A silent «e» is dropped only at a length STRICTLY greater than three: a
    /// three-letter word keeps its «e».
    #[test]
    fn fold_keeps_the_silent_e_on_three_char_words() {
        assert_eq!(fold_for_match("кое"), "koe");
    }

    #[test]
    fn apply_case_of_handles_a_single_capital_letter() {
        // A single capital letter is the text's case, not "all caps".
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
        // A window running past the input's edge is None, not a panic.
        assert!(c
            .window_score(
                &["a", "b"],
                1,
                2,
                &"claudecode".chars().collect::<Vec<_>>(),
                &mut DistanceScratch::default()
            )
            .is_none());
        // Exactly CUSTOM_WORD_MIN_CHARS alphanumeric characters is the
        // boundary.
        assert!(c
            .window_score(
                &["abcd"],
                0,
                1,
                &"abcd".chars().collect::<Vec<_>>(),
                &mut DistanceScratch::default()
            )
            .is_some());
    }

    #[test]
    fn reused_distance_rows_preserve_unicode_and_length_boundaries() {
        let mut scratch = DistanceScratch::default();
        for (a, b, expected) in [
            ("kitten", "sitting", 3),
            ("a", "b", 1),
            ("", "два", 3),
            ("привет", "привіт", 1),
            ("abcdef", "", 6),
            ("ab", "b", 1),
            ("same", "same", 0),
            ("Saturday", "Sunday", 3),
        ] {
            let a: Vec<char> = a.chars().collect();
            let b: Vec<char> = b.chars().collect();
            assert_eq!(scratch.distance(&a, &b), expected);
            assert_eq!(scratch.distance(&b, &a), expected);
            for budget in 0..=expected + 1 {
                assert_eq!(
                    scratch.within(&a, &b, budget),
                    (expected <= budget).then_some(expected)
                );
                assert_eq!(
                    scratch.within(&b, &a, budget),
                    (expected <= budget).then_some(expected)
                );
            }
        }
    }

    #[test]
    fn edit_distance_substitutions_are_counted() {
        let a: Vec<char> = "ab".chars().collect();
        let b: Vec<char> = "xy".chars().collect();
        assert_eq!(edit_distance(&a, &b), 2);
        // Deletion: «ab» → «b» costs one edit. Catches a shift in the boundary
        // `current[0] = i + 1` (swapping `+` for `*`).
        let b1: Vec<char> = "b".chars().collect();
        assert_eq!(edit_distance(&a, &b1), 1);
    }

    #[test]
    fn tokenize_empty_text_yields_no_tokens() {
        assert!(tokenize("").is_empty());
    }

    // ----- Enabled flags, thresholds and applying replacement rules -----
    //
    // Covers the misses from the control run over formatter.rs. They share one
    // pattern: only the behaviour of an enabled step on a "convenient" input was
    // checked, so dropping the `!` in `if !self.enabled` flipped the step to the
    // exact opposite without the tests noticing.

    /// Exactly twenty words — the boundary at which `SentenceSplitter` stops
    /// treating the text as short — and the keyword «потом» inside it, which the
    /// step splits the phrase on.
    fn twenty_words_with_split_keyword() -> &'static str {
        "один два три четыре пять шесть семь восемь девять десять потом \
         одиннадцать двенадцать тринадцать четырнадцать пятнадцать шестнадцать \
         семнадцать восемнадцать девятнадцать"
    }

    /// Both sides of the flag are checked on one input. A single case is not
    /// enough: a test for a disabled step survives dropping the `!` unless it is
    /// also shown that an enabled step changes the same text.
    #[test]
    fn comma_cleaner_runs_only_when_enabled() {
        let input = "раз,, и два";
        assert_eq!(
            CommaCleaner::new(true).apply(input),
            "раз, и два",
            "an enabled step collapses duplicate commas"
        );
        assert_eq!(
            CommaCleaner::new(false).apply(input),
            input,
            "a disabled step must return the text untouched"
        );
    }

    #[test]
    fn sentence_splitter_runs_only_when_enabled() {
        let text = twenty_words_with_split_keyword();
        assert!(
            SentenceSplitter::new(true).apply(text).contains(". Потом "),
            "an enabled step splits the phrase at the keyword"
        );
        assert_eq!(
            SentenceSplitter::new(false).apply(text),
            text,
            "a disabled step must return the text untouched"
        );
    }

    /// The short-text threshold is strictly fewer than twenty words. All three
    /// points are needed: a short text, exactly the boundary, and a text longer
    /// than it. From one point alone, swapping `<` for `==`, `<=` or `>` is
    /// indistinguishable from the original.
    #[test]
    fn sentence_splitter_threshold_is_twenty_words() {
        let short = "раз два потом три";
        assert_eq!(
            short.split_whitespace().count(),
            4,
            "test self-check: the input must be clearly shorter than the threshold"
        );
        assert_eq!(
            SentenceSplitter::new(true).apply(short),
            short,
            "the step does not split a text shorter than twenty words, even with the keyword in it"
        );

        let at_threshold = twenty_words_with_split_keyword();
        assert_eq!(
            at_threshold.split_whitespace().count(),
            20,
            "test self-check: the input must sit exactly on the boundary"
        );
        assert!(
            SentenceSplitter::new(true)
                .apply(at_threshold)
                .contains(". Потом "),
            "exactly twenty words is no longer short text, so the step must split"
        );

        let over_threshold = format!("{at_threshold} двадцать");
        assert!(
            SentenceSplitter::new(true)
                .apply(&over_threshold)
                .contains(". Потом "),
            "the step splits a text longer than the threshold too"
        );
    }

    /// Without this test the step's body could be replaced with an empty string
    /// or an arbitrary constant and no test would go red: the replacements were
    /// checked only through `apply_replacement_rules` directly, bypassing the
    /// step itself.
    #[test]
    fn prepared_replacements_keep_order_captures_deletion_and_pause_behavior() {
        let rules = normalize_replacement_rules(Some(&serde_json::json!([
            {"id":"first", "find":" alpha ", "replace":"beta", "match":"word"},
            {"id":"capture", "find":"(beta) ([0-9]+)", "replace":"$2:$1", "match":"regex"},
            {"id":"delete", "find":"remove", "replace":"", "match":"word"},
            {"id":"invalid", "find":"[", "replace":"broken", "match":"regex"},
            {"id":"empty", "find":" ", "replace":"broken"},
            {"id":"disabled", "find":"keep", "replace":"broken", "enabled":false}
        ])));
        let mut step = ContextReplacements::new(true, rules);
        assert_eq!(step.apply("alpha 42 remove keep"), "42:beta  keep");
        assert_eq!(step.apply("alpha 7"), "7:beta");
        step.set_enabled(false);
        assert_eq!(step.apply("alpha 7"), "alpha 7");
        step.set_enabled(true);
        assert_eq!(step.apply("alpha 7"), "7:beta");
    }

    #[test]
    fn context_replacements_apply_the_rule() {
        let step = ContextReplacements::new(true, vec![word_rule("тайпскрипт", "TypeScript")]);
        assert_eq!(
            step.apply("тайпскрипт рядом"),
            "TypeScript рядом",
            "an enabled step must apply the rule"
        );
    }

    #[test]
    fn prepared_replacements_do_not_create_text_from_empty_input() {
        let source = serde_json::json!([
            {"id":"prefix", "find":"^", "replace":"prefix", "match":"regex"}
        ]);
        let step = ContextReplacements::new(true, normalize_replacement_rules(Some(&source)));
        assert_eq!(step.apply(""), "");
        let (preview, stats) = apply_replacement_rules("", Some(&source), false);
        assert_eq!(preview, "");
        assert_eq!(stats.total, 0);
        assert!(stats.rules.is_empty());
        assert_eq!(step.apply("text"), "prefixtext");
    }

    #[test]
    fn prepared_replacements_preserve_raw_fallback_after_filler_removal() {
        let config = serde_json::json!({
            "language": "en",
            "text_formatting": {
                "remove_fillers": true,
                "correct_spelling": false,
                "capitalize_sentences": false,
                "final_punctuation": false
            },
            "replacement_rules": [
                {"id":"prefix", "find":"^", "replace":"prefix", "match":"regex"}
            ]
        });
        assert_eq!(preview_format("umm", &config).unwrap().formatted, "");
        assert_eq!(
            format_transcription_with_config_value(&config, "umm"),
            "umm"
        );
    }

    /// A disabled step with a non-empty rule list is the only input on which
    /// `!enabled || rules.is_empty()` differs from the same pair joined by `&&`:
    /// with `&&` the step does not back off and applies the rules.
    #[test]
    fn context_replacements_stay_out_when_disabled() {
        let step = ContextReplacements::new(false, vec![word_rule("тайпскрипт", "TypeScript")]);
        assert_eq!(
            step.apply("тайпскрипт рядом"),
            "тайпскрипт рядом",
            "a disabled step does not apply the rules even when it has them"
        );
    }
}
