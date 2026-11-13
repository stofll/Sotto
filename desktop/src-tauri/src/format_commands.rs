//! Tauri command wrappers around the formatting pipeline.
//!
//! The frontend calls these via `invoke("preview_format", ...)` /
//! `invoke("preview_replacements", ...)` from the text settings page
//! (`TextPage.tsx`).
//!
//! Both commands accept a `patch` payload — a JSON Merge Patch fragment
//! that is merged into the saved config before running the formatter.
//! This lets the Settings UI show a live preview as the user tweaks
//! formatting options without saving first.

use std::io::{self, Write};
use std::path::Path;

use serde_json::{Map, Value};
use tauri::{AppHandle, WebviewWindow};
use tauri_plugin_dialog::DialogExt;

use crate::config;

/// Resolve `Option<Value>` from a Tauri command to a concrete JSON Merge
/// Patch fragment. `None` (frontend omitted the field) → empty object
/// `{}`, which is a no-op merge against the saved config. Explicit
/// `Some(v)` is passed through unchanged.
fn resolve_patch(patch: Option<Value>) -> Value {
    match patch {
        Some(v) => v,
        None => Value::Object(Map::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// `preview_format` and `preview_replacements` receive `patch:
    /// Option<Value>` from Tauri. When the frontend omits the field,
    /// Tauri sends `None`. The command must resolve it to an empty
    /// object `{}` so that `merge_json_patch(base, {})` returns `base`
    /// unchanged (instead of `merge_json_patch(base, null)` which
    /// returns `null`).
    #[test]
    fn none_patch_resolves_to_empty_object_for_merge() {
        let base = json!({ "theme": "dark", "hotkey": "ctrl+space" });
        let resolved = resolve_patch(None);
        assert_eq!(resolved, json!({}));
        // An empty-object patch must be a no-op merge:
        let merged = config::merge_json_patch(base.clone(), resolved);
        assert_eq!(merged, base);
    }

    #[test]
    fn some_patch_is_preserved_unchanged() {
        let resolved = resolve_patch(Some(json!({ "theme": "light" })));
        assert_eq!(resolved, json!({ "theme": "light" }));
    }

    #[test]
    fn export_writes_and_replaces_the_selected_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rules.json");
        let rules = r#"{"replacement_rules":[{"find":"flare","replace":"FLAIR"}]}"#;
        write_replacement_export(&path, rules).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), rules);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();
        }

        let updated = "{\"replacement_rules\":[]}";
        write_replacement_export(&path, updated).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), updated);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o640
            );
        }
    }

    #[test]
    fn failed_export_keeps_the_destination_and_removes_the_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rules.json");
        std::fs::create_dir(&path).unwrap();
        assert!(write_replacement_export(&path, "{}").is_err());
        assert!(path.is_dir());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}

/// Load the current config, merge `patch` into it, and return the merged
/// root value. Used by both `preview_*` commands.
fn load_config_apply_patch(app: &AppHandle, patch: &Value) -> Result<Value, String> {
    let cfg = config::Config::load(app)?;
    let base = cfg.as_value().clone();
    Ok(config::merge_json_patch(base, patch.clone()))
}

/// Preview the full text-formatting pipeline with a temporary config
/// patch.
///
/// The frontend sends the current typing-buffer as `text` and a
/// partial config fragment as `patch`. This command merges the patch
/// into the saved config (without persisting), runs the formatter, and
/// returns `{ original, formatted }`.
#[tauri::command]
pub async fn preview_format(
    app: AppHandle,
    text: String,
    patch: Option<Value>,
) -> Result<crate::formatter::PreviewFormatResult, String> {
    let patch = resolve_patch(patch);
    let merged = load_config_apply_patch(&app, &patch)?;
    tauri::async_runtime::spawn_blocking(move || crate::formatter::preview_format(&text, &merged))
        .await
        .map_err(|_| "Formatting worker failed".to_string())?
}

/// Preview just the replacement-rule pass with a temporary patch.
///
/// Same merge semantics as `preview_format`, but only runs
/// `apply_replacement_rules` instead of the full pipeline. The
/// result includes `applied_count` and `matched_rules` metadata
/// so the UI can highlight which rules fired.
#[tauri::command]
pub async fn preview_replacements(
    app: AppHandle,
    text: String,
    patch: Option<Value>,
) -> Result<crate::formatter::PreviewReplacementsResult, String> {
    let patch = resolve_patch(patch);
    let merged = load_config_apply_patch(&app, &patch)?;
    tauri::async_runtime::spawn_blocking(move || {
        crate::formatter::preview_replacements(&text, &merged)
    })
    .await
    .map_err(|_| "Formatting worker failed".to_string())?
}

#[tauri::command]
pub async fn export_replacement_rules(
    window: WebviewWindow,
    content: String,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let Some(path) = window
            .dialog()
            .file()
            .set_parent(&window)
            .set_file_name("replacement-rules.json")
            .add_filter("JSON", &["json"])
            .blocking_save_file()
        else {
            return Ok(());
        };
        let path = path.into_path().map_err(|_| {
            log::warn!("Replacement export requires a local path");
            "Non-local export destination".to_string()
        })?;
        write_replacement_export(&path, &content).map_err(|error| {
            log::warn!("Replacement export write failed: {}", error.kind());
            error.kind().to_string()
        })
    })
    .await
    .map_err(|error| {
        log::warn!("Replacement export worker failed: {error}");
        "Export worker failed".to_string()
    })?
}

fn write_replacement_export(path: &Path, content: &str) -> io::Result<()> {
    let parent = path.parent().ok_or(io::ErrorKind::InvalidInput)?;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    #[cfg(unix)]
    let mut file = tempfile::Builder::new()
        .permissions(std::fs::Permissions::from_mode(0o666))
        .tempfile_in(parent)?;
    #[cfg(not(unix))]
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    #[cfg(unix)]
    match std::fs::metadata(path) {
        Ok(metadata) => file
            .as_file()
            .set_permissions(std::fs::Permissions::from_mode(
                metadata.permissions().mode() & 0o777,
            ))?,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    file.write_all(content.as_bytes())?;
    file.as_file().sync_all()?;
    file.persist(path).map_err(|error| error.error)?;
    Ok(())
}
