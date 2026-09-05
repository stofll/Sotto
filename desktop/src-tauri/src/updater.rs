//! Self-update via `tauri-plugin-updater`.
//!
//! The plugin fetches `latest.json` from GitHub Releases, verifies the minisign
//! signature against the public key in `tauri.conf.json`, and installs the
//! artifact. This module is only a wrapper over it: three commands and progress
//! events.
//!
//! What the signature does and does not give: minisign confirms that the
//! artifact was not swapped in transit and that it was built by the holder of
//! the private key. It is **not** Authenticode: without a code-signing
//! certificate Windows will show SmartScreen's "unknown publisher" both on
//! install and on every update.
//!
//! An update is never installed on its own. The check at startup is silent and
//! its errors do not surface: the network may be unreachable, there may be no
//! releases at all, and neither is a reason to bother the user. Downloading
//! starts only on an explicit click.

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tauri_plugin_updater::UpdaterExt;

/// Download progress event. `total` comes from `Content-Length` and may be
/// absent — in that case the frontend shows an indeterminate indicator.
pub const PROGRESS_EVENT: &str = "update-download-progress";

/// What is known about an available update. `available: false` means the
/// version is current; that is a normal answer, not an error.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UpdateInfo {
    pub available: bool,
    /// The version currently installed.
    pub current_version: String,
    /// The version from the manifest. `None` when there is no update.
    pub version: Option<String>,
    /// Publication date exactly as it was put into the manifest (RFC 3339).
    pub date: Option<String>,
    /// The "what's new" text. In the manifest this is a single `notes` field;
    /// formatting (markdown, lists) is the release notes' own responsibility.
    pub notes: Option<String>,
}

impl UpdateInfo {
    pub fn none(current_version: impl Into<String>) -> Self {
        Self {
            available: false,
            current_version: current_version.into(),
            version: None,
            date: None,
            notes: None,
        }
    }
}

/// Download progress in bytes. The fraction is computed by the frontend: it
/// needs the raw byte counts anyway to show "12 of 34 MB".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct DownloadProgress {
    pub downloaded: u64,
    pub total: Option<u64>,
}

/// The reason why starting the check makes no sense at all.
///
/// Only an installed build can be updated: `cargo tauri dev` has no installer
/// for the updater to replace — the plugin answers such a request with an error,
/// and showing it to the user is pointless.
pub fn unsupported_reason() -> Option<&'static str> {
    if crate::portable::data_dir().is_some() {
        return Some("Портативная версия: скачайте новый ZIP и замените файлы приложения, сохранив папку data.");
    }
    cfg!(debug_assertions).then_some("обновления работают только в собранном приложении")
}

/// Ask the server about an update.
pub async fn check(app: &AppHandle) -> Result<UpdateInfo, String> {
    let current = app.package_info().version.to_string();
    if let Some(reason) = unsupported_reason() {
        log::debug!("проверка обновлений пропущена: {reason}");
        return Ok(UpdateInfo::none(current));
    }
    let updater = app.updater().map_err(|e| e.to_string())?;
    match updater.check().await {
        Ok(Some(update)) => Ok(UpdateInfo {
            available: true,
            current_version: current,
            version: Some(update.version.clone()),
            date: update.date.map(|d| d.to_string()),
            notes: update.body.clone().filter(|s| !s.trim().is_empty()),
        }),
        Ok(None) => Ok(UpdateInfo::none(current)),
        Err(e) => Err(e.to_string()),
    }
}

/// Download and install the update, then restart the application.
///
/// The check is performed again rather than taken from [`check`]'s result: the
/// plugin's `Update` does not survive the IPC boundary, and repeating the
/// request is still cheaper than holding it in state between two commands.
pub async fn install(app: &AppHandle) -> Result<(), String> {
    if let Some(reason) = unsupported_reason() {
        return Err(reason.to_string());
    }
    let updater = app.updater().map_err(|e| e.to_string())?;
    let update = updater
        .check()
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "обновление больше недоступно".to_string())?;

    let app_for_progress = app.clone();
    let mut downloaded: u64 = 0;
    update
        .download_and_install(
            move |chunk, total| {
                downloaded += chunk as u64;
                let _ =
                    app_for_progress.emit(PROGRESS_EVENT, DownloadProgress { downloaded, total });
            },
            || log::info!("обновление скачано, ставим"),
        )
        .await
        .map_err(|e| e.to_string())?;

    // On Windows installMode = passive: the installer is already running and
    // will ask to close the application. We exit ourselves so it does not wait.
    app.restart();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_update_reports_current_version() {
        let info = UpdateInfo::none("0.1.1");
        assert!(!info.available);
        assert_eq!(info.current_version, "0.1.1");
        assert_eq!(info.version, None);
        assert_eq!(info.notes, None);
    }

    #[test]
    fn progress_keeps_an_unknown_total_unknown() {
        // A server without Content-Length is a legitimate case: the frontend
        // shows an indeterminate indicator rather than 0 %.
        let json = serde_json::to_value(DownloadProgress {
            downloaded: 5,
            total: None,
        })
        .unwrap();
        assert_eq!(json["downloaded"], 5);
        assert!(json["total"].is_null());
    }

    #[test]
    fn dev_builds_never_check() {
        // Tests run in debug, so a reason is guaranteed to be present.
        assert_eq!(
            unsupported_reason(),
            Some("обновления работают только в собранном приложении")
        );
    }

    #[test]
    fn info_serializes_with_the_shape_the_frontend_expects() {
        let json = serde_json::to_value(UpdateInfo {
            available: true,
            current_version: "0.1.1".into(),
            version: Some("0.2.0".into()),
            date: Some("2026-08-15T10:00:00Z".into()),
            notes: Some("Светлая тема".into()),
        })
        .unwrap();
        assert_eq!(json["available"], true);
        assert_eq!(json["current_version"], "0.1.1");
        assert_eq!(json["version"], "0.2.0");
        assert_eq!(json["notes"], "Светлая тема");
    }
}
