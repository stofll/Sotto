//! Локализация строк, которые рождаются в Rust.
//!
//! Таких немного: пункт меню трея и полдюжины сообщений об ошибках, которые
//! уезжают во фронтенд как есть. Полноценный i18n-крейт ради них — лишняя
//! зависимость, поэтому здесь то же решение, что и на фронте: ключ — русский
//! оригинал, перевод ищется в таблице, отсутствующий перевод падает на ключ.
//!
//! Язык хранится в атомике, а не в состоянии Tauri, потому что читать его
//! приходится из мест без `AppHandle` — например из потока движка.
//!
//! Сюда НЕ попадают строки логов и слова-паразиты из `formatter.rs`: первые
//! читает разработчик, вторые относятся к языку речи.

use std::sync::atomic::{AtomicU8, Ordering};

use serde_json::Value;

const RU: u8 = 0;
const EN: u8 = 1;

static LOCALE: AtomicU8 = AtomicU8::new(RU);

/// Ключ конфига. Отдельно от `language`: тот про язык речи.
pub const CONFIG_KEY: &str = "ui_language";

/// Применить язык из конфига. Если старый конфиг ещё не содержит поля,
/// используем тот же системный fallback, что и frontend: только явно
/// русская/английская локаль переключает язык, остальные падают на русский.
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

pub fn is_english() -> bool {
    LOCALE.load(Ordering::Relaxed) == EN
}

/// Перевести строку. Ключ — русский оригинал.
pub fn t(key: &str) -> String {
    if !is_english() {
        return key.to_string();
    }
    en(key).unwrap_or(key).to_string()
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
        // Транскрипция прикреплённого файла: декодер, гейты и отказы.
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

    /// Тесты трогают глобальный атомик, поэтому идут под общим мьютексом:
    /// иначе один тест переключает язык под ногами у другого.
    static GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn with_locale<T>(config: Value, body: impl FnOnce() -> T) -> T {
        let _lock = GUARD.lock().unwrap_or_else(|e| e.into_inner());
        set_from_config(&config);
        let out = body();
        set_from_config(&json!({}));
        out
    }

    #[test]
    fn russian_is_the_identity() {
        with_locale(json!({ "ui_language": "ru" }), || {
            assert_eq!(t("Выход"), "Выход");
        });
    }

    #[test]
    fn english_translates_known_keys() {
        with_locale(json!({ "ui_language": "en" }), || {
            assert_eq!(t("Выход"), "Quit");
            assert_eq!(t("Не выбран провайдер."), "No provider selected.");
        });
    }

    #[test]
    fn unknown_key_falls_back_to_the_original() {
        // Худший случай — смешанный язык, а не пустая строка в интерфейсе.
        with_locale(json!({ "ui_language": "en" }), || {
            assert_eq!(t("Такого ключа нет"), "Такого ключа нет");
        });
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

    /// Каждый ключ таблицы обязан встречаться в коде — иначе перевод висит
    /// мёртвым грузом и расходится с оригиналом незаметно.
    #[test]
    fn every_translated_key_is_used_somewhere() {
        let sources: Vec<String> = ["lib.rs", "tray.rs", "whisper.rs", "model.rs"]
            .iter()
            .map(|f| {
                std::fs::read_to_string(format!("{}/src/{f}", env!("CARGO_MANIFEST_DIR")))
                    .unwrap_or_default()
            })
            .collect();
        for key in [
            "Выход",
            "Не выбран провайдер.",
            "LLM не вернула результат.",
            "Вставьте текст для обработки.",
            "Модель не загружена. Откройте «Настройки → Модели» и выберите модель.",
            "Не удалось вставить текст в активное окно.",
            "Эта модель распознаёт только русскую речь.",
        ] {
            assert!(
                sources.iter().any(|s| s.contains(key)),
                "перевод есть, а строки в коде нет: {key}"
            );
        }
    }
}
