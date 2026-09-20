//! Localization of the strings that are born in Rust.
//!
//! There are few: a tray menu item and half a dozen error messages that travel
//! to the frontend as they are. A full i18n crate for their sake is an
//! unnecessary dependency, so the solution here is the same as on the frontend:
//! the key is the Russian original, the translation is looked up in a table, and
//! a missing translation falls back to the key.
//!
//! The language is kept in an atomic rather than in Tauri state, because it has
//! to be read from places without an `AppHandle` — the engine thread, say.
//!
//! Log strings and the filler words from `formatter.rs` do NOT belong here: the
//! former are read by a developer, the latter belong to the language of speech.

use std::sync::atomic::{AtomicU8, Ordering};

use serde_json::Value;

const RU: u8 = 0;
const EN: u8 = 1;

static LOCALE: AtomicU8 = AtomicU8::new(RU);

/// The config key. Separate from `language`: that one is about speech.
pub const CONFIG_KEY: &str = "ui_language";

/// Apply the language from the config. If an old config does not yet carry the
/// field we use the same system fallback as the frontend: only an explicitly
/// Russian or English locale switches the language, everything else falls back
/// to Russian.
pub fn set_from_config(config: &Value) {
    let system = system_locale();
    let locale = resolve_locale(config, system.as_deref());
    LOCALE.store(locale, Ordering::Relaxed);
}

fn resolve_locale(config: &Value, system_locale: Option<&str>) -> u8 {
    match config.get(CONFIG_KEY).and_then(Value::as_str) {
        Some("en") => EN,
        Some("ru") => RU,
        _ => match system_locale
            .and_then(|tag| tag.split(['-', '_']).next())
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("en") => EN,
            _ => RU,
        },
    }
}

#[cfg(windows)]
fn system_locale() -> Option<String> {
    use windows_sys::Win32::Globalization::GetUserDefaultLocaleName;

    // LOCALE_NAME_MAX_LENGTH is 85 including the terminating NUL.
    let mut buffer = [0u16; 85];
    // SAFETY: the buffer is live and its real length travels with the
    // pointer, so the callee cannot write past `LOCALE_NAME_MAX_LENGTH`.
    let written = unsafe { GetUserDefaultLocaleName(buffer.as_mut_ptr(), buffer.len() as i32) };
    if written <= 1 {
        None
    } else {
        Some(String::from_utf16_lossy(&buffer[..written as usize - 1]))
    }
}

#[cfg(not(windows))]
fn system_locale() -> Option<String> {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .into_iter()
        .find_map(|key| {
            std::env::var(key)
                .ok()
                .filter(|value| !value.trim().is_empty())
        })
}

/// Translate a string. The key is the Russian original.
pub fn t(key: &str) -> String {
    translate(key, LOCALE.load(Ordering::Relaxed)).to_string()
}

fn translate(key: &str, locale: u8) -> &str {
    if locale == EN {
        en(key).unwrap_or(key)
    } else {
        key
    }
}

fn en(key: &str) -> Option<&'static str> {
    Some(match key {
        "Выход" => "Quit",
        "Не выбран провайдер." => "No provider selected.",
        "LLM не вернула результат." => "The LLM returned nothing.",
        "Вставьте текст для обработки." => "Paste some text to process.",
        "Модель не загружена. Откройте «Настройки → Модели» и выберите модель." => {
            "No model loaded. Open Settings → Models and pick one."
        }
        "Эта модель распознаёт только русскую речь." => {
            "This model transcribes Russian audio only."
        }
        "Эта модель распознаёт только английскую речь." => {
            "This model transcribes English audio only."
        }
        "Эта модель не поддерживает выбранный язык." => {
            "This model does not support the selected language."
        }
        "Эта модель уже скачивается." => "This model is already downloading.",
        "Не удалось вставить текст в активное окно." => {
            "Could not paste into the active window."
        }
        "Текст скопирован. Нажмите ⌘V для вставки. Для автоматической вставки разрешите Sotto доступ в настройках универсального доступа macOS." => {
            "Text copied. Press ⌘V to paste. For automatic pasting, allow Sotto in macOS Accessibility settings."
        }
        // Transcription of an attached file: the decoder, the gates and the
        // refusals.
        "Аудио" => "Audio",
        "Не удалось открыть диалог выбора файла." => "Could not open the file picker.",
        "Не удалось открыть файл: {p0}" => "Could not open the file: {p0}",
        "В файле нет звуковой дорожки." => "The file has no audio track.",
        "В файле нет звука." => "The file contains no audio.",
        "В файле не распознана речь." => "No speech was recognised in the file.",
        "Файл длиннее {p0} часов." => "The file is longer than {p0} hours.",
        "Не удалось прочитать звук из файла — возможно, он повреждён." => {
            "Could not read audio from the file — it may be damaged."
        }
        "Не удалось преобразовать частоту дискретизации файла." => {
            "Could not convert the file's sample rate."
        }
        "Этот формат не поддерживается. Сконвертируйте файл в wav, mp3 или m4a." => {
            "This format is not supported. Convert the file to wav, mp3 or m4a."
        }
        "Идёт транскрипция файла — дождитесь её окончания." => {
            "A file is being transcribed — wait for it to finish."
        }
        "Модель распознавания не скачана — записывать нечем. Скачайте модель в настройках или включите облачную обработку." => {
            "No speech model has been downloaded — there is nothing to record into. Download a model in the settings, or turn on cloud processing."
        }
        "Завершите текущую запись." => "Finish the current recording first.",
        "Эта модель не умеет расшифровывать файлы — выберите модель Whisper в «Настройки → Модели»." => {
            "This model cannot transcribe files — pick a Whisper model in Settings → Models."
        }
        "Движок не ответил. Попробуйте ещё раз." => "The engine did not respond. Try again.",
        "Транскрипция отменена." => "Transcription cancelled.",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn russian_is_the_identity() {
        // Other modules test localized errors in parallel; never change the
        // process-wide locale just to exercise translation lookup.
        assert_eq!(translate("Выход", RU), "Выход");
    }

    #[test]
    fn english_translates_known_keys() {
        assert_eq!(translate("Выход", EN), "Quit");
        assert_eq!(
            translate("Не выбран провайдер.", EN),
            "No provider selected."
        );
    }

    #[test]
    fn unknown_key_falls_back_to_the_original() {
        // The worst case is mixed language, not an empty string in the UI.
        assert_eq!(translate("Такого ключа нет", EN), "Такого ключа нет");
    }

    #[test]
    fn missing_or_unknown_value_uses_supported_system_locale() {
        assert_eq!(resolve_locale(&json!({}), Some("ru-RU")), RU);
        assert_eq!(resolve_locale(&json!({}), Some("en-US")), EN);
        assert_eq!(resolve_locale(&json!({}), Some("de-DE")), RU);
        assert_eq!(
            resolve_locale(&json!({ "ui_language": "de" }), Some("en-US")),
            EN
        );
        assert_eq!(
            resolve_locale(&json!({ "ui_language": 42 }), Some("ru-RU")),
            RU
        );
    }

    #[test]
    fn explicit_config_wins_over_the_system_locale() {
        assert_eq!(
            resolve_locale(&json!({ "ui_language": "ru" }), Some("en-US")),
            RU
        );
        assert_eq!(
            resolve_locale(&json!({ "ui_language": "en" }), Some("ru-RU")),
            EN
        );
    }

    /// Every key in the table must occur in the code — otherwise a translation
    /// hangs there as dead weight and drifts from the original unnoticed.
    ///
    /// Both halves are read off disk: the keys from `en()` below, the code from
    /// every `.rs` under `src/`. Hand-written lists of either were the weakness
    /// this replaced — a list of seven keys checked against five named files
    /// missed both a dead translation and every key that moved to a module the
    /// list had never heard of.
    #[test]
    fn every_translated_key_is_used_somewhere() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let table = root.join("ui_text.rs");
        let mut sources = Vec::new();
        collect_rust_sources(&root, &table, &mut sources);
        // Anti-vacuity: a walk that found nothing would pass every assertion
        // below without reading a line of the application.
        assert!(sources.len() > 20, "the source walk found almost no files");

        let keys = translation_keys(&std::fs::read_to_string(&table).unwrap());
        assert!(keys.len() > 20, "the table parser found almost no keys");

        for key in keys {
            assert!(
                sources.iter().any(|source| source.contains(&key)),
                "the translation exists but the string is not in the code: {key}"
            );
        }
    }

    /// A key that exists only as a match arm is dead weight. Comparing the
    /// table path as a string used to miss this on Windows: `PathBuf::push`
    /// spells a separator the concatenation did not, the table stayed in
    /// `sources`, and every key then "occurred" by definition.
    #[test]
    fn a_key_that_only_lives_in_the_table_counts_as_unused() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let table = root.join("ui_text.rs");
        std::fs::write(
            &table,
            concat!(
                "fn en(key: &str) -> Option<&'static str> {\n",
                "    Some(match key {\n",
                "        \"DEAD_KEY_ONLY_IN_TABLE\" => \"unused\",\n",
                "        \"USED_KEY\" => \"used\",\n",
                "        _ => return None,\n",
                "    })\n",
                "}\n",
            ),
        )
        .unwrap();
        std::fs::create_dir(root.join("nested")).unwrap();
        std::fs::write(
            root.join("nested").join("lib.rs"),
            "fn f() { let _ = \"USED_KEY\"; }\n",
        )
        .unwrap();

        let mut sources = Vec::new();
        collect_rust_sources(root, &table, &mut sources);
        assert_eq!(sources.len(), 1, "the table itself must not be a source");

        let keys = translation_keys(&std::fs::read_to_string(&table).unwrap());
        assert_eq!(
            keys,
            vec!["DEAD_KEY_ONLY_IN_TABLE".to_string(), "USED_KEY".to_string()]
        );
        assert!(
            !sources
                .iter()
                .any(|source| source.contains("DEAD_KEY_ONLY_IN_TABLE")),
            "a key that lives only in the table must count as unused"
        );
        assert!(sources.iter().any(|source| source.contains("USED_KEY")));
    }

    /// Concatenating `'/'` produces a string `DirEntry::path()` will not
    /// equal on Windows, where `join` inserts `'\\'`. Comparing as `Path`
    /// still excludes the table; comparing as strings would not.
    #[cfg(windows)]
    #[test]
    fn the_table_is_excluded_when_separators_are_spelled_differently() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let table = std::path::PathBuf::from(format!("{}/ui_text.rs", root.display()));
        assert!(
            table.to_string_lossy().contains('/'),
            "self-check: the excluded path must keep the slash DirEntry will not use"
        );
        let mut sources = Vec::new();
        collect_rust_sources(&root, &table, &mut sources);
        let table_text = std::fs::read_to_string(root.join("ui_text.rs")).unwrap();
        assert!(
            !sources.iter().any(|source| source == &table_text),
            "ui_text.rs must not be a source even when excluded via a slash-spelled path"
        );
    }

    /// Every `.rs` file under `src/`, except the one holding the table itself —
    /// there every key occurs by definition.
    fn collect_rust_sources(dir: &std::path::Path, table: &std::path::Path, out: &mut Vec<String>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_rust_sources(&path, table, out);
            } else if path.extension().is_some_and(|ext| ext == "rs") && path != table {
                if let Ok(text) = std::fs::read_to_string(&path) {
                    out.push(text);
                }
            }
        }
    }

    /// The left-hand sides of the `match` arms in `en()`. The arms are one key
    /// each and carry no escapes, so the line is its own delimiter.
    fn translation_keys(source: &str) -> Vec<String> {
        let start = source
            .find("fn en(")
            .expect("fn en(…) not found in ui_text.rs");
        source[start..]
            .lines()
            .skip(1)
            .take_while(|line| !line.starts_with('}'))
            .filter_map(|line| {
                let rest = line.trim_start().strip_prefix('"')?;
                let end = rest.find("\" =>")?;
                Some(rest[..end].to_string())
            })
            .collect()
    }
}
