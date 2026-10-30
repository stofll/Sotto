//! Public issue diagnostics are an allow-listed projection, never a log archive.
use std::io::{Read, Seek, SeekFrom};

use serde_json::Value;
use tauri::AppHandle;

const LIMIT: u64 = 256 * 1024;

pub fn summary(version: &str, config: &Value) -> String {
    // A fresh install writes no keys until the user changes a setting, so an
    // absent key reports the default the app actually runs with; only a value
    // outside the allow-list is withheld.
    let model = match config.get("model").and_then(Value::as_str) {
        None => "not_selected",
        Some(value) => crate::model::catalog_model(value).map_or("custom", |m| m.id),
    };
    let mode = match config
        .pointer("/ai_processing/pipeline_mode")
        .and_then(Value::as_str)
    {
        None | Some("local") => "local",
        Some("hybrid") => "hybrid",
        Some("cloud") => "cloud",
        _ => "unknown",
    };
    // Dictation treats everything except push-to-talk as toggle.
    let recording = match config.get("recording_mode").and_then(Value::as_str) {
        None | Some("toggle") => "toggle",
        Some("push_to_talk") => "push_to_talk",
        _ => "unknown",
    };
    format!("Sotto: {version}\nOS: {}\nArchitecture: {}\nModel: {model}\nPipeline: {mode}\nRecording: {recording}\n", std::env::consts::OS, std::env::consts::ARCH)
}

#[tauri::command]
pub async fn get_public_diagnostics(app: AppHandle) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let config = crate::config::Config::load(&app)?;
        Ok(summary(
            &app.package_info().version.to_string(),
            config.as_value(),
        ))
    })
    .await
    .map_err(|e| e.to_string())?
}

fn project_logs(input: &str) -> String {
    static TIMING: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(concat!(
            r"^(capture timing: (first_callback_ms=[0-9]+|session=[0-9]+ queue_ms=[0-9]+ stream_ready_ms=[0-9]+)",
            r"|stop timing: session=[0-9]+ queue_ms=[0-9]+ native_stop_ms=[0-9]+ processing_notice_ms=[0-9]+ prepare_ms=[0-9]+",
            r"|delivery timing: session=[0-9]+ (database_ms=[0-9]+|(main_)?queue_ms=[0-9]+ paste_ms=[0-9]+)",
            r"|model timing: stage=(verify|context|state|unload|request) reason=(startup|device|requested|restore|replace|idle|cloud|delete|first_inference) elapsed_ms=[0-9]+(\.[0-9]{3})? ok=(true|false)",
            r"|preview timing: session=[0-9]+ gap_count=[0-9]+ fallback_count=[0-9]+ reused=(true|false) pending_samples=[0-9]+)$",
        )).unwrap()
    });
    let mut out = String::from("Public diagnostic log: recent active-file tail (up to 256 KiB).\nFree-form messages, paths, source locations and recordings are excluded.\n");
    for line in input.lines() {
        let Ok(row) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let ts = row["ts"].as_str().unwrap_or("");
        let level = row["level"].as_str().unwrap_or("");
        let target = row["target"].as_str().unwrap_or("");
        if ts.len() > 32
            || !ts
                .bytes()
                .all(|b| b.is_ascii_digit() || b"-:TZ. +".contains(&b))
            || !["ERROR", "WARN", "INFO", "DEBUG", "TRACE"].contains(&level)
        {
            continue;
        }
        // Only compile-time module targets from this crate; no arbitrary target text.
        let module = target
            .strip_prefix("sotto_lib::")
            .unwrap_or("")
            .split("::")
            .next()
            .unwrap_or("");
        let module = match module {
            "audio" | "whisper" | "sherpa" | "ai" | "paste" | "hotkey" | "model" | "overlay" => {
                module
            }
            _ => "app",
        };
        let message = row["message"].as_str().unwrap_or("");
        let detail = if message.len() < 200 && TIMING.is_match(message) {
            message
        } else {
            "[message omitted]"
        };
        out.push_str(&format!("{ts} {level} {module}: {detail}\n"));
    }
    out
}

#[tauri::command]
pub async fn get_public_logs() -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let mut file = std::fs::File::open(crate::debug::diagnostics_dir().join("app.log"))
            .map_err(|_| crate::ui_text::t("Не удалось прочитать логи"))?;
        let len = file
            .metadata()
            .map_err(|_| crate::ui_text::t("Не удалось прочитать логи"))?
            .len();
        let start = len.saturating_sub(LIMIT);
        file.seek(SeekFrom::Start(start))
            .map_err(|_| crate::ui_text::t("Не удалось прочитать логи"))?;
        let mut bytes = Vec::new();
        file.take(LIMIT)
            .read_to_end(&mut bytes)
            .map_err(|_| crate::ui_text::t("Не удалось прочитать логи"))?;
        let input = String::from_utf8_lossy(&bytes);
        let input = if start > 0 {
            input.split_once('\n').map(|(_, rest)| rest).unwrap_or("")
        } else {
            &input
        };
        Ok(project_logs(input))
    })
    .await
    .map_err(|_| crate::ui_text::t("Не удалось прочитать логи"))?
}

#[tauri::command]
pub async fn save_public_logs(app: AppHandle, content: String) -> Result<bool, String> {
    use tauri_plugin_dialog::DialogExt;
    if content.len() > 1024 * 1024 {
        return Err(crate::ui_text::t("Отчёт слишком большой"));
    }
    tauri::async_runtime::spawn_blocking(move || {
        let Some(path) = app
            .dialog()
            .file()
            .set_file_name("sotto-diagnostics.txt")
            .add_filter("Text", &["txt"])
            .blocking_save_file()
        else {
            return Ok(false);
        };
        let path = path
            .into_path()
            .map_err(|_| crate::ui_text::t("Не удалось сохранить отчёт"))?;
        std::fs::write(path, content)
            .map_err(|_| crate::ui_text::t("Не удалось сохранить отчёт"))?;
        Ok(true)
    })
    .await
    .map_err(|_| crate::ui_text::t("Не удалось сохранить отчёт"))?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn public_summary_excludes_custom_config_values() {
        let report = summary(
            "1.0",
            &serde_json::json!({"model":"/Users/private/model", "recording_mode":"secret", "ai_processing":{"pipeline_mode":"token"}}),
        );
        assert!(!report.contains("private"));
        assert!(!report.contains("secret"));
        assert!(!report.contains("token"));
        assert!(report.lines().any(|line| line == "Model: custom"));
    }
    #[test]
    fn public_summary_reports_defaults_for_a_fresh_config() {
        let report = summary("1.0", &serde_json::json!({}));
        assert!(report.lines().any(|line| line == "Model: not_selected"));
        assert!(report.lines().any(|line| line == "Pipeline: local"));
        assert!(report.lines().any(|line| line == "Recording: toggle"));
    }
    #[test]
    fn export_drops_private_messages_and_malformed_records() {
        let row = |message: &str| {
            serde_json::json!({"ts":"2026-09-14T12:00:00Z","level":"ERROR","target":"sotto_lib::ai","message":message,"file":"/Users/private/file"}).to_string()
        };
        let report = project_logs(&format!(
            "broken\n{}\n{}",
            row("token transcript /Users/private"),
            row("capture timing: first_callback_ms=42")
        ));
        assert!(!report.contains("private"));
        assert!(!report.contains("token"));
        assert!(report.contains("first_callback_ms=42"));
        assert!(report.contains("ERROR ai"));
    }

    #[test]
    fn export_preserves_numeric_capture_delivery_and_model_timings() {
        let messages = [
            "capture timing: first_callback_ms=42",
            "capture timing: session=7 queue_ms=2 stream_ready_ms=31",
            "stop timing: session=7 queue_ms=2 native_stop_ms=5 processing_notice_ms=8 prepare_ms=13",
            "delivery timing: session=7 database_ms=3",
            "delivery timing: session=7 queue_ms=4 paste_ms=25",
            "delivery timing: session=7 main_queue_ms=4 paste_ms=25",
            "model timing: stage=verify reason=startup elapsed_ms=120 ok=true",
            "model timing: stage=state reason=first_inference elapsed_ms=8 ok=true",
            "model timing: stage=context reason=device elapsed_ms=1.234 ok=true",
            "model timing: stage=unload reason=cloud elapsed_ms=30 ok=true",
            "model timing: stage=request reason=requested elapsed_ms=200 ok=false",
            "preview timing: session=7 gap_count=1 fallback_count=1 reused=false pending_samples=16000",
        ];
        let input = messages.map(|message| {
            serde_json::json!({"ts":"2026-09-14T12:00:00Z","level":"INFO","target":"sotto_lib::whisper","message":message}).to_string()
        }).join("\n");
        let report = project_logs(&input);
        for message in messages {
            assert!(report.contains(message), "missing timing: {message}");
        }
        assert!(!report.contains("[message omitted]"));
    }

    #[test]
    fn timing_prefix_does_not_allow_free_form_diagnostics() {
        let messages = [
            "capture timing: session=7 queue_ms=2 stream_ready_ms=/Users/private",
            "stop timing: session=7 queue_ms=2 native_stop_ms=5 processing_notice_ms=8 prepare_ms=13 text=private",
            "delivery timing: session=7 queue_ms=4 paste_ms=25 transcript=private",
            "model timing: stage=context reason=/Users/private elapsed_ms=1 ok=true",
            "model timing: stage=context reason=restore elapsed_ms=1 ok=true path=/Users/private",
            "preview timing: session=7 gap_count=0 fallback_count=0 reused=true pending_samples=1 token=private",
            "capture timing: first_callback_ms=42\nprivate",
        ];
        let input = messages.map(|message| {
            serde_json::json!({"ts":"2026-09-14T12:00:00Z","level":"INFO","target":"sotto_lib::audio","message":message}).to_string()
        }).join("\n");
        let report = project_logs(&input);
        assert_eq!(report.matches("[message omitted]").count(), messages.len());
        assert!(!report.contains("private"));
        assert!(!report.contains("timing:"));
    }
}
