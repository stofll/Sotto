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
        "В файле меняется частота дискретизации. Перекодируйте его в один поток." => {
            "The file changes sample rate partway through. Re-encode it as a single stream."
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
        // Updates.
        "Портативная версия: скачайте новый ZIP и замените файлы приложения, сохранив папку data." => {
            "Portable version: download the new ZIP and replace the application files, keeping the data folder."
        }
        "обновления работают только в собранном приложении" => {
            "updates work only in an installed build"
        }
        "обновление больше недоступно" => "the update is no longer available",
        // Feedback report and diagnostics.
        "Не удалось прочитать логи" => "Could not read the logs",
        "Отчёт слишком большой" => "The report is too large",
        "Не удалось сохранить отчёт" => "Could not save the report",
        "Звук не обнаружен. Скажите что-нибудь, проверьте подключение, выбранный микрофон и его громкость. Тишина сама по себе не означает запрет доступа." => {
            "No sound detected. Say something, and check the connection, the selected microphone and its volume. Silence on its own does not mean access was denied."
        }
        "Тест вставки Sotto — " => "Sotto paste test — ",
        "Вставка сработала, текст в буфере обмена: {p0}" => {
            "Paste worked; the text is on the clipboard: {p0}"
        }
        "Вставка не удалась: {p0}" => "Paste failed: {p0}",
        // macOS Accessibility.
        "Sotto нужен доступ к Специальным возможностям (Accessibility), чтобы автоматически вставлять распознанный текст в активное окно. Откройте «Системные настройки → Конфиденциальность → Специальные возможности» и разрешите доступ для приложения." => {
            "Sotto needs Accessibility access to paste the recognised text into the active window automatically. Open System Settings → Privacy & Security → Accessibility and allow the app."
        }
        "Sotto нужен доступ к Специальным возможностям (Accessibility), чтобы автоматически вставлять распознанный текст в активное окно. Откройте «Системные настройки → Конфиденциальность → Специальные возможности» и разрешите доступ для «{p0}». Это сборка вне «Программ»: система выдаёт доступ именно этому файлу, а если он запущен из терминала — то терминалу. Включённый переключатель у другой копии Sotto здесь не поможет." => {
            "Sotto needs Accessibility access to paste the recognised text into the active window automatically. Open System Settings → Privacy & Security → Accessibility and allow «{p0}». This build is outside Applications: macOS grants access to this exact file, or to the terminal if it was started from one. A switch turned on for another copy of Sotto does not help here."
        }
        // Provider model lists; shown after «Список моделей:».
        "не задан base_url" => "base_url is not set",
        "неизвестный провайдер: {p0}" => "unknown provider: {p0}",
        "запрос не прошёл: {p0}" => "the request failed: {p0}",
        "ключ не подошёл" => "the key was rejected",
        "провайдер не отдаёт список моделей" => "the provider does not list its models",
        "провайдер ответил {p0}" => "the provider answered {p0}",
        "ответ не разобрался: {p0}" => "the response could not be parsed: {p0}",
        "провайдер вернул пустой список" => "the provider returned an empty list",
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

    /// Russian text reaches the user only through `t()`; the frontend has the
    /// same check in `check-i18n.mjs`. Speech-language content — the
    /// formatter's and spell checker's vocabulary, prompts, samples — is not
    /// UI text: whole modules are listed below, a single literal is marked with
    /// a `Speech language` comment in the three lines above it. Log strings
    /// are read by developers and stay in English.
    #[test]
    fn russian_text_reaches_the_user_only_through_t() {
        const SPEECH_MODULES: [&str; 3] = ["formatter.rs", "spelling.rs", "ui_text.rs"];
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = Vec::new();
        collect_rust_paths(&src, &mut files);
        let mut offenders = Vec::new();
        for path in files {
            if SPEECH_MODULES.iter().any(|name| path.ends_with(name)) {
                continue;
            }
            let source = std::fs::read_to_string(&path).unwrap();
            // Test modules assert on the translated strings themselves.
            let code = match source
                .find("#[cfg(test)]\nmod ")
                .or_else(|| source.find("#[cfg(test)]\r\nmod "))
            {
                Some(end) => &source[..end],
                None => &source,
            };
            for (offset, literal) in string_literals(code) {
                if !literal.chars().any(|c| matches!(c, 'А'..='я' | 'Ё' | 'ё')) {
                    continue;
                }
                let before = code[..offset].trim_end();
                if before.ends_with("t(") {
                    continue;
                }
                let context: Vec<&str> = before.lines().rev().take(3).collect();
                if context.iter().any(|line| line.contains("Speech language")) {
                    continue;
                }
                let line = before.lines().count().max(1);
                offenders.push(format!("{}:{line}: {literal}", path.display()));
            }
        }
        assert!(
            offenders.is_empty(),
            "Russian text outside ui_text::t():\n{}",
            offenders.join("\n")
        );
    }

    #[test]
    fn the_literal_scanner_sees_raw_strings_and_skips_comments_and_chars() {
        let source = "// «комментарий»\nlet a = t(\"ключ\");\nlet b = r#\"сырой \"x\"\"#;\nlet c = 'ы';\nlet d = \"a \\\" б\";";
        let literals: Vec<String> = string_literals(source)
            .into_iter()
            .map(|(_, text)| text)
            .collect();
        assert_eq!(literals, ["ключ", "сырой \"x\"", "a \\\" б"]);
    }

    fn collect_rust_paths(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_rust_paths(&path, out);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                out.push(path);
            }
        }
    }

    /// String literals with the byte offset where each starts. Comments and
    /// char literals are skipped so their quotes do not open a string.
    fn string_literals(source: &str) -> Vec<(usize, String)> {
        let bytes = source.as_bytes();
        let mut found = Vec::new();
        let mut i = 0;
        while i < bytes.len() {
            match bytes[i] {
                b'/' if bytes.get(i + 1) == Some(&b'/') => {
                    i = source[i..].find('\n').map_or(bytes.len(), |n| i + n);
                }
                b'/' if bytes.get(i + 1) == Some(&b'*') => {
                    i = source[i + 2..]
                        .find("*/")
                        .map_or(bytes.len(), |n| i + n + 4);
                }
                b'\'' => {
                    // A char literal closes within a few bytes; a lifetime
                    // does not, and is stepped over as a plain character.
                    let rest = &source[i + 1..];
                    let close = if rest.starts_with('\\') {
                        rest[2..].find('\'').map(|n| n + 2)
                    } else {
                        rest.char_indices()
                            .nth(1)
                            .filter(|(_, c)| *c == '\'')
                            .map(|(n, _)| n)
                    };
                    i += close.map_or(1, |n| n + 2);
                }
                b'r' if matches!(bytes.get(i + 1), Some(b'"' | b'#'))
                    && (i == 0
                        || !(bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_')) =>
                {
                    let hashes = source[i + 1..].bytes().take_while(|b| *b == b'#').count();
                    let open = i + 1 + hashes;
                    if bytes.get(open) != Some(&b'"') {
                        i += 1;
                        continue;
                    }
                    let terminator = format!("\"{}", "#".repeat(hashes));
                    let body = open + 1;
                    let end = source[body..]
                        .find(&terminator)
                        .map_or(bytes.len(), |n| body + n);
                    found.push((i, source[body..end].to_string()));
                    i = end + terminator.len();
                }
                b'"' => {
                    let body = i + 1;
                    let mut j = body;
                    while j < bytes.len() && bytes[j] != b'"' {
                        j += if bytes[j] == b'\\' { 2 } else { 1 };
                    }
                    found.push((i, source[body..j.min(bytes.len())].to_string()));
                    i = j + 1;
                }
                _ => i += 1,
            }
        }
        found
    }
}
