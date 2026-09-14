//! Public issue diagnostics are an allow-listed projection, never a log archive.
use std::io::{Read, Seek, SeekFrom};

use serde_json::Value;
use tauri::AppHandle;

const LIMIT: u64 = 256 * 1024;

pub fn summary(version: &str, config: &Value) -> String {
    let model = config
        .get("model")
        .and_then(Value::as_str)
        .and_then(crate::model::catalog_model)
        .map(|m| m.id)
        .unwrap_or("custom_or_unset");
    let mode = match config
        .pointer("/ai_processing/pipeline_mode")
        .and_then(Value::as_str)
    {
        Some("local") => "local",
        Some("hybrid") => "hybrid",
        Some("cloud") => "cloud",
        _ => "unknown",
    };
    let recording = match config.get("recording_mode").and_then(Value::as_str) {
        Some("push_to_talk") => "push_to_talk",
        Some("toggle") => "toggle",
        _ => "unknown",
    };
    format!("Sotto: {version}\nOS: {}\nArchitecture: {}\nModel: {model}\nPipeline: {mode}\nRecording: {recording}\n", std::env::consts::OS, std::env::consts::ARCH)
}

#[tauri::command]
pub fn get_public_diagnostics(app: AppHandle) -> Result<String, String> {
    let config = crate::config::Config::load(&app)?;
    Ok(summary(
        &app.package_info().version.to_string(),
        config.as_value(),
    ))
}

fn project_logs(input: &str) -> String {
    let timing = regex::Regex::new(r"^(capture timing: first_callback_ms=[0-9]+|delivery timing: session=[0-9]+ (database_ms=[0-9]+|main_queue_ms=[0-9]+ paste_ms=[0-9]+))$").unwrap();
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
        let detail = if message.len() < 200 && timing.is_match(message) {
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
            .map_err(|_| "Не удалось прочитать логи".to_string())?;
        let len = file
            .metadata()
            .map_err(|_| "Не удалось прочитать логи".to_string())?
            .len();
        let start = len.saturating_sub(LIMIT);
        file.seek(SeekFrom::Start(start))
            .map_err(|_| "Не удалось прочитать логи".to_string())?;
        let mut bytes = Vec::new();
        file.take(LIMIT)
            .read_to_end(&mut bytes)
            .map_err(|_| "Не удалось прочитать логи".to_string())?;
        let input = String::from_utf8_lossy(&bytes);
        let input = if start > 0 {
            input.split_once('\n').map(|(_, rest)| rest).unwrap_or("")
        } else {
            &input
        };
        Ok(project_logs(input))
    })
    .await
    .map_err(|_| "Не удалось прочитать логи".to_string())?
}

#[tauri::command]
pub async fn save_public_logs(app: AppHandle, content: String) -> Result<bool, String> {
    use tauri_plugin_dialog::DialogExt;
    if content.len() > 1024 * 1024 {
        return Err("Отчёт слишком большой".into());
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
            .map_err(|_| "Не удалось сохранить отчёт".to_string())?;
        std::fs::write(path, content).map_err(|_| "Не удалось сохранить отчёт".to_string())?;
        Ok(true)
    })
    .await
    .map_err(|_| "Не удалось сохранить отчёт".to_string())?
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
}
