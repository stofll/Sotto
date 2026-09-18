//! Dictionary membership, spelling choices and legacy configuration migration.

use crate::formatter::{TextFormattingConfig, DICTIONARY_PRESETS};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, HashSet};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DictionarySet {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub enabled: bool,
    pub words: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct SpellingConflict {
    pub key: String,
    pub variants: Vec<String>,
    pub selected: Option<String>,
}

#[derive(Serialize)]
pub struct DictionaryAnalysis {
    pub effective_count: usize,
    pub conflicts: Vec<SpellingConflict>,
    pub unsupported_words: Vec<String>,
}

fn variants(config: &TextFormattingConfig) -> BTreeMap<String, BTreeSet<String>> {
    let mut terms: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for word in config.custom_words.iter().chain(
        config
            .dictionary_sets
            .iter()
            .filter(|set| set.enabled)
            .flat_map(|set| &set.words),
    ) {
        let word = word.trim();
        if !word.is_empty() {
            terms
                .entry(word.to_lowercase())
                .or_default()
                .insert(word.to_owned());
        }
    }
    terms
}

pub fn conflicts(config: &TextFormattingConfig) -> Vec<SpellingConflict> {
    variants(config)
        .into_iter()
        .filter(|(_, words)| words.len() > 1)
        .map(|(key, words)| {
            let selected = config
                .dictionary_spellings
                .iter()
                .find(|word| words.contains(*word))
                .cloned();
            SpellingConflict {
                key,
                variants: words.into_iter().collect(),
                selected,
            }
        })
        .collect()
}

pub fn effective_words(config: &TextFormattingConfig) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let choices: BTreeMap<_, _> = conflicts(config)
        .into_iter()
        .map(|conflict| {
            // Hand-edited invalid configurations remain deterministic. Normal saves
            // require an explicit choice before a conflicting set can be enabled.
            let selected = conflict.selected.unwrap_or_else(|| {
                config
                    .custom_words
                    .iter()
                    .map(|word| word.trim())
                    .find(|word| word.to_lowercase() == conflict.key)
                    .map(str::to_owned)
                    .unwrap_or_else(|| conflict.variants[0].clone())
            });
            (conflict.key, selected)
        })
        .collect();
    let mut push = |word: &str| {
        let word = word.trim();
        let key = word.to_lowercase();
        if !word.is_empty() && seen.insert(key.clone()) {
            out.push(
                choices
                    .get(&key)
                    .cloned()
                    .unwrap_or_else(|| word.to_owned()),
            );
        }
    };
    for word in &config.custom_words {
        push(word);
    }
    for set in config.dictionary_sets.iter().filter(|set| set.enabled) {
        for word in &set.words {
            push(word);
        }
    }
    for id in &config.enabled_presets {
        if let Some(set) = DICTIONARY_PRESETS.iter().find(|set| set.id == id) {
            for word in set.words {
                push(word);
            }
        }
    }
    out
}

#[tauri::command]
pub fn analyze_dictionary(formatting: TextFormattingConfig) -> DictionaryAnalysis {
    let unsupported_words = formatting
        .dictionary_sets
        .iter()
        .flat_map(|set| &set.words)
        .chain(formatting.custom_words.iter())
        .map(|word| word.trim())
        .filter(|word| !word.is_empty() && !crate::formatter::custom_word_supported(word))
        .map(str::to_owned)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    DictionaryAnalysis {
        effective_count: effective_words(&formatting).len(),
        conflicts: conflicts(&formatting),
        unsupported_words,
    }
}

/// Read migration only: persistence happens with the next successful save.
pub fn migrate(config: &mut Value) {
    let Some(formatting) = config
        .get_mut("text_formatting")
        .and_then(Value::as_object_mut)
    else {
        return;
    };
    if formatting.contains_key("dictionary_sets") {
        return;
    }
    let Some(words) = formatting
        .get("custom_words")
        .and_then(Value::as_array)
        .cloned()
    else {
        return;
    };
    let has_words = words
        .iter()
        .any(|word| word.as_str().is_some_and(|word| !word.trim().is_empty()));
    let sets = if has_words {
        vec![
            json!({ "id": "legacy-personal", "name": "", "description": "", "enabled": true, "words": words }),
        ]
    } else {
        Vec::new()
    };
    // Preserve the old first-spelling-wins behavior, including case variants.
    let mut seen = HashSet::new();
    let spellings: Vec<_> = words
        .iter()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|word| !word.is_empty() && seen.insert(word.to_lowercase()))
        .collect();
    formatting.insert("dictionary_sets".into(), json!(sets));
    formatting.insert("dictionary_spellings".into(), json!(spellings));
    formatting.insert("custom_words".into(), json!([]));
}

pub fn validate(config: &Value) -> Result<(), String> {
    let Some(formatting) = config.get("text_formatting") else {
        return Ok(());
    };
    if formatting.get("dictionary_sets").is_none() {
        return Ok(());
    }
    let formatting: TextFormattingConfig = serde_json::from_value(formatting.clone())
        .map_err(|_| "Invalid dictionary configuration".to_string())?;
    let mut ids = HashSet::new();
    for set in &formatting.dictionary_sets {
        if set.id.trim().is_empty()
            || !ids.insert(&set.id)
            || (set.name.trim().is_empty() && set.id != "legacy-personal")
        {
            return Err("Dictionary sets require unique IDs and non-empty names".into());
        }
    }
    if conflicts(&formatting)
        .iter()
        .any(|conflict| conflict.selected.is_none())
    {
        return Err("Choose a spelling for conflicting dictionary terms".into());
    }
    Ok(())
}

/// Ready-made term sets for the dictionary.
///
/// We hand over the id and the words; the set's name is displayed by the
/// frontend because the name is translatable while the word list is not.
#[tauri::command]
pub(crate) fn dictionary_presets() -> Vec<(String, Vec<String>)> {
    DICTIONARY_PRESETS
        .iter()
        .map(|set| {
            (
                set.id.to_string(),
                set.words.iter().map(|w| w.to_string()).collect(),
            )
        })
        .collect()
}

/// One built-in parasite word set, as the interface needs it.
#[derive(Serialize)]
pub(crate) struct ParasiteSetInfo {
    id: String,
    language: String,
    words: Vec<String>,
    default_on: bool,
}

/// The built-in parasite word sets.
///
/// Handed to the frontend so that settings can show the words and let them be
/// switched off. Before this existed the list was invisible: a person could see
/// that something had been taken out of their dictation but had no way to find
/// out what, or to stop it.
///
/// Which sets are ON is not reported here — that lives in the config, which the
/// frontend already holds. This is the catalogue, and `default_on` is what a
/// config that has never been touched resolves to.
#[tauri::command]
pub(crate) fn parasite_sets() -> Vec<ParasiteSetInfo> {
    crate::formatter::PARASITE_PRESETS
        .iter()
        .map(|preset| ParasiteSetInfo {
            id: preset.id.to_string(),
            language: preset.language.to_string(),
            words: preset.words.iter().map(|word| word.to_string()).collect(),
            default_on: preset.default_on,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formatter::{Formatter, FormatterConfig};

    fn set(id: &str, words: &[&str], enabled: bool) -> DictionarySet {
        DictionarySet {
            id: id.into(),
            name: id.into(),
            description: String::new(),
            enabled,
            words: words.iter().map(|word| (*word).into()).collect(),
        }
    }

    #[test]
    fn migration_preserves_words_order_spelling_and_preset_selection() {
        let mut config = json!({"text_formatting": {"custom_words": ["cargo", "Cargo", "  Claude Code  "], "enabled_presets": ["development"], "remove_parasites": false}});
        migrate(&mut config);
        let once = config.clone();
        migrate(&mut config);
        assert_eq!(config, once);
        assert_eq!(
            config["text_formatting"]["dictionary_sets"][0]["words"],
            json!(["cargo", "Cargo", "  Claude Code  "])
        );
        let formatting: TextFormattingConfig =
            serde_json::from_value(config["text_formatting"].clone()).unwrap();
        assert!(!formatting.remove_parasites);
        assert_eq!(formatting.enabled_presets, ["development"]);
        assert!(validate(&config).is_ok());
        let words = effective_words(&formatting);
        assert_eq!(&words[..2], ["cargo", "Claude Code"]);
        assert!(!words.contains(&"Cargo".into()));
    }

    #[test]
    fn empty_legacy_dictionary_does_not_create_a_set() {
        let mut config = json!({"text_formatting": {"custom_words": []}});
        migrate(&mut config);
        assert_eq!(config["text_formatting"]["dictionary_sets"], json!([]));
        let mut fresh = json!({});
        migrate(&mut fresh);
        assert_eq!(fresh, json!({}));
    }

    #[test]
    fn membership_and_deduplication_follow_enabled_sets() {
        let mut config = TextFormattingConfig {
            dictionary_sets: vec![
                set("a", &["Claude Code", "Rust", ""], true),
                set("b", &["Rust", "Tauri"], true),
            ],
            ..Default::default()
        };
        assert_eq!(effective_words(&config), ["Claude Code", "Rust", "Tauri"]);
        config.dictionary_sets[0].enabled = false;
        assert_eq!(effective_words(&config), ["Rust", "Tauri"]);
        config.dictionary_sets.remove(0);
        assert_eq!(effective_words(&config), ["Rust", "Tauri"]);
        config.dictionary_sets[0].enabled = false;
        assert!(effective_words(&config).is_empty());
    }

    #[test]
    fn explicit_spelling_wins_independently_of_set_order() {
        let mut config = TextFormattingConfig {
            dictionary_sets: vec![
                set("a", &["TypeScript"], true),
                set("b", &["typescript"], true),
            ],
            ..Default::default()
        };
        assert!(conflicts(&config)[0].selected.is_none());
        assert!(validate(&json!({"text_formatting": config})).is_err());
        config.dictionary_spellings = vec!["typescript".into()];
        assert_eq!(effective_words(&config), ["typescript"]);
        config.dictionary_sets.reverse();
        assert_eq!(effective_words(&config), ["typescript"]);
        assert!(validate(&json!({"text_formatting": config})).is_ok());
        config.dictionary_sets[0].enabled = false;
        assert_eq!(effective_words(&config), ["TypeScript"]);
    }

    #[test]
    fn similar_words_are_not_spelling_conflicts_and_short_words_stay_available() {
        let config = TextFormattingConfig {
            dictionary_sets: vec![set("a", &["Node", "Vite", "Claude Code", "Клод Код"], true)],
            ..Default::default()
        };
        let analysis = analyze_dictionary(config);
        assert!(analysis.conflicts.is_empty());
        assert_eq!(analysis.effective_count, 4);
        assert_eq!(analysis.unsupported_words, ["Node", "Vite"]);
    }

    #[test]
    fn set_switch_controls_observable_text_correction() {
        let mut config = FormatterConfig::default();
        config.text_formatting.dictionary_sets = vec![set("a", &["Tauri"], true)];
        let on = Formatter::from_config(&config).process("собрано в таури");
        assert!(on.contains("Tauri"), "{on}");
        config.text_formatting.dictionary_sets[0].enabled = false;
        let off = Formatter::from_config(&config).process("собрано в таури");
        assert!(off.contains("таури"), "{off}");
        config.text_formatting.dictionary_sets[0].enabled = true;
        config.text_formatting.enabled = false;
        assert_eq!(
            Formatter::from_config(&config).process("собрано в таури"),
            "собрано в таури"
        );
        assert_eq!(effective_words(&config.text_formatting), ["Tauri"]);
    }

    #[test]
    fn custom_set_spelling_overrides_builtin_without_mutating_it() {
        let config = TextFormattingConfig {
            dictionary_sets: vec![set("a", &["cargo"], true)],
            enabled_presets: vec!["development".into()],
            ..Default::default()
        };
        assert!(effective_words(&config).contains(&"cargo".into()));
        assert!(!effective_words(&config).contains(&"Cargo".into()));
        assert!(DICTIONARY_PRESETS[0].words.contains(&"Cargo"));
    }
}
