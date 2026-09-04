//! Tauri command wrappers around the formatting pipeline.
//!
//! Phase 4 / PR-B: native Tauri commands for the formatting pipeline.
//! The frontend calls these via direct `invoke("preview_format", ...)` /
//! `invoke("preview_replacements", ...)` from the Settings UI
//! (OtherPages.tsx).
//!
//! Both commands accept a `patch` payload — a JSON Merge Patch fragment
//! that is merged into the saved config before running the formatter.
//! This lets the Settings UI show a live preview as the user tweaks
//! formatting options without saving first.

use serde_json::{Map, Value};
use tauri::AppHandle;

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
pub fn preview_format(
    app: AppHandle,
    text: String,
    patch: Option<Value>,
) -> Result<crate::formatter::PreviewFormatResult, String> {
    let patch = resolve_patch(patch);
    let merged = load_config_apply_patch(&app, &patch)?;
    crate::formatter::preview_format(&text, &merged)
}

/// Preview just the replacement-rule pass with a temporary patch.
///
/// Same merge semantics as `preview_format`, but only runs
/// `apply_replacement_rules` instead of the full pipeline. The
/// result includes `applied_count` and `matched_rules` metadata
/// so the UI can highlight which rules fired.
#[tauri::command]
pub fn preview_replacements(
    app: AppHandle,
    text: String,
    patch: Option<Value>,
) -> Result<crate::formatter::PreviewReplacementsResult, String> {
    let patch = resolve_patch(patch);
    let merged = load_config_apply_patch(&app, &patch)?;
    crate::formatter::preview_replacements(&text, &merged)
}
