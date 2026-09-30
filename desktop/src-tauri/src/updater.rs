//! Self-update via `tauri-plugin-updater`.
//!
//! The plugin fetches `latest.json` from GitHub Releases, verifies the minisign
//! signature against the public key in `tauri.conf.json`, and installs the
//! artifact. This module selects the stable or opted-in beta release and exposes
//! commands and progress events without changing signature verification.
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

use semver::Version;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use tauri_plugin_updater::{Update, UpdaterExt};

const RELEASES_URL: &str = "https://api.github.com/repos/stofll/Sotto/releases?per_page=100";
const UPDATE_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_RELEASE_RESPONSE_BYTES: usize = 8 * 1024 * 1024;

fn beta_enabled(config: &serde_json::Value) -> bool {
    config
        .get("receive_beta_updates")
        .and_then(serde_json::Value::as_bool)
        == Some(true)
}

fn allowed_upgrade(current: &Version, next: &Version, beta: bool) -> bool {
    (beta || next.pre.is_empty()) && next.cmp_precedence(current).is_gt()
}

#[derive(Debug, Deserialize)]
struct ReleaseAsset {
    name: String,
}

#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<ReleaseAsset>,
}

fn newest_release(releases: &[GithubRelease], current: &Version) -> Option<Version> {
    releases
        .iter()
        .filter_map(|release| {
            if release.draft
                || !release
                    .assets
                    .iter()
                    .any(|asset| asset.name == "latest.json")
            {
                return None;
            }
            let version = Version::parse(release.tag_name.strip_prefix('v')?).ok()?;
            // Both the tag and GitHub flag must agree on the channel.
            if release.prerelease != !version.pre.is_empty()
                || !allowed_upgrade(current, &version, true)
            {
                return None;
            }
            Some(version)
        })
        .max_by(Version::cmp_precedence)
}

async fn fetch_releases(url: &str) -> Result<Vec<GithubRelease>, String> {
    let client = crate::http_client::builder()
        .timeout(UPDATE_TIMEOUT)
        .user_agent(concat!("Sotto/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| e.to_string())?;
    let mut response = client
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|e| e.without_url().to_string())?;
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| e.without_url().to_string())?
    {
        if bytes.len() + chunk.len() > MAX_RELEASE_RESPONSE_BYTES {
            return Err(crate::ui_text::t(
                "Ответ сервера обновлений слишком большой.",
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|e| e.to_string())
}

async fn available_update(app: &AppHandle) -> Result<Option<Update>, String> {
    // Read the persisted choice for both checking and installation. A previously
    // displayed beta must not bypass a subsequently disabled preference.
    let beta = beta_enabled(crate::config::Config::load(app)?.as_value());
    let selected = if beta {
        let releases = fetch_releases(RELEASES_URL).await?;
        let Some(version) = newest_release(&releases, &app.package_info().version) else {
            return Ok(None);
        };
        Some(version)
    } else {
        None
    };
    let mut builder = app.updater_builder().timeout(UPDATE_TIMEOUT);
    if let Some(version) = &selected {
        let url =
            format!("https://github.com/stofll/Sotto/releases/download/v{version}/latest.json");
        builder = builder
            .endpoints(vec![url
                .parse::<reqwest::Url>()
                .map_err(|e| e.to_string())?])
            .map_err(|e| e.to_string())?;
    }
    builder
        .version_comparator(move |current, release| {
            allowed_upgrade(&current, &release.version, beta)
                && selected
                    .as_ref()
                    .is_none_or(|expected| expected == &release.version)
        })
        .build()
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| e.to_string())
}

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
pub fn unsupported_reason() -> Option<String> {
    if crate::portable::data_dir().is_some() {
        return Some(crate::ui_text::t(
            "Портативная версия: скачайте новый ZIP и замените файлы приложения, сохранив папку data.",
        ));
    }
    cfg!(debug_assertions)
        .then(|| crate::ui_text::t("обновления работают только в собранном приложении"))
}

/// Ask the server about an update.
pub async fn check(app: &AppHandle) -> Result<UpdateInfo, String> {
    let current = app.package_info().version.to_string();
    if let Some(reason) = unsupported_reason() {
        log::debug!("update check skipped: {reason}");
        return Ok(UpdateInfo::none(current));
    }
    match available_update(app).await? {
        Some(update) => Ok(UpdateInfo {
            available: true,
            current_version: current,
            version: Some(update.version.clone()),
            date: update.date.map(|d| d.to_string()),
            notes: update.body.filter(|s| !s.trim().is_empty()),
        }),
        None => Ok(UpdateInfo::none(current)),
    }
}

/// Download and install the update, then restart the application.
///
/// The check is performed again rather than taken from [`check`]'s result: the
/// plugin's `Update` does not survive the IPC boundary, and repeating the
/// request is still cheaper than holding it in state between two commands.
pub async fn install(app: &AppHandle) -> Result<(), String> {
    if let Some(reason) = unsupported_reason() {
        return Err(reason);
    }
    let update = available_update(app)
        .await?
        .ok_or_else(|| crate::ui_text::t("обновление больше недоступно"))?;

    crate::release_notes::cache_update(app, &update.version, update.body.as_deref()).await;

    let app_for_progress = app.clone();
    let mut downloaded: u64 = 0;
    update
        .download_and_install(
            move |chunk, total| {
                downloaded += chunk as u64;
                let _ =
                    app_for_progress.emit(PROGRESS_EVENT, DownloadProgress { downloaded, total });
            },
            || log::info!("update downloaded, installing"),
        )
        .await
        .map_err(|e| e.to_string())?;

    // On Windows installMode = passive: the installer is already running and
    // will ask to close the application. We exit ourselves so it does not wait.
    app.restart();
}

/// Ask the update server. An `available: false` answer is not an error.
#[tauri::command]
pub(crate) async fn check_update(app: AppHandle) -> Result<UpdateInfo, String> {
    check(&app).await
}

/// Download and install the update. Progress arrives as
/// `update-download-progress` events; on success the application restarts and
/// the command never returns control to the frontend.
#[tauri::command]
pub(crate) async fn install_update(app: AppHandle) -> Result<(), String> {
    install(&app).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn beta_updates_require_an_explicit_boolean_opt_in() {
        for config in [
            serde_json::json!({}),
            serde_json::json!({"receive_beta_updates": false}),
            serde_json::json!({"receive_beta_updates": "true"}),
        ] {
            assert!(!beta_enabled(&config));
        }
        assert!(beta_enabled(
            &serde_json::json!({"receive_beta_updates": true})
        ));
    }

    #[test]
    fn stable_channel_rejects_betas_and_both_channels_reject_downgrades() {
        let stable = Version::parse("0.2.0").unwrap();
        let beta = Version::parse("0.2.1-beta.2").unwrap();
        let promoted = Version::parse("0.2.1").unwrap();
        assert!(!allowed_upgrade(&stable, &beta, false));
        assert!(allowed_upgrade(&stable, &beta, true));
        assert!(allowed_upgrade(&beta, &promoted, false));
        for channel in [false, true] {
            assert!(!allowed_upgrade(&beta, &stable, channel));
            assert!(!allowed_upgrade(&beta, &beta, channel));
            assert!(!allowed_upgrade(
                &stable,
                &Version::parse("0.2.0+rebuild").unwrap(),
                channel
            ));
        }
    }

    fn release(version: &str, draft: bool, prerelease: bool, manifest: bool) -> GithubRelease {
        GithubRelease {
            tag_name: format!("v{version}"),
            draft,
            prerelease,
            assets: if manifest {
                vec![ReleaseAsset {
                    name: "latest.json".into(),
                }]
            } else {
                vec![]
            },
        }
    }

    #[test]
    fn beta_channel_selects_by_semver_and_accepts_a_newer_stable_release() {
        let current = Version::parse("0.2.0").unwrap();
        let mut releases = vec![
            release("0.2.1-beta.9", false, true, true),
            release("0.2.1-beta.10", false, true, true),
        ];
        assert_eq!(
            newest_release(&releases, &current).unwrap().to_string(),
            "0.2.1-beta.10"
        );
        releases.push(release("0.2.1", false, false, true));
        assert_eq!(
            newest_release(&releases, &current).unwrap().to_string(),
            "0.2.1"
        );
    }

    #[test]
    fn release_selection_excludes_drafts_malformed_tags_missing_assets_and_wrong_channel_flags() {
        let current = Version::parse("0.2.0").unwrap();
        let releases = vec![
            release("0.3.0-beta.1", true, true, true),
            release("0.3.0-beta.2", false, true, false),
            release("0.3.0-beta.3", false, false, true),
            release("0.3.0", false, true, true),
            release("invalid", false, true, true),
            release("0.1.0", false, false, true),
        ];
        assert!(newest_release(&releases, &current).is_none());
    }

    #[tokio::test]
    async fn beta_release_api_errors_and_oversized_responses_are_reported() {
        use std::io::{Read, Write};
        for (status, body) in [
            (500, "[]".into()),
            (200, " ".repeat(MAX_RELEASE_RESPONSE_BYTES + 1)),
            (200, "[]".into()),
        ] {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let url = format!("http://{}/releases", listener.local_addr().unwrap());
            let oversized = body.len() > MAX_RELEASE_RESPONSE_BYTES;
            let server = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut request = [0; 4096];
                assert!(stream.read(&mut request).unwrap() > 0);
                let response = format!("HTTP/1.1 {status} Result\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                // The bounded client intentionally closes an oversized response.
                let _ = stream.write_all(response.as_bytes());
            });
            let result = fetch_releases(&url).await;
            server.join().unwrap();
            if status == 500 {
                assert!(result.is_err());
            } else if oversized {
                assert_eq!(
                    result.unwrap_err(),
                    crate::ui_text::t("Ответ сервера обновлений слишком большой.")
                );
            } else {
                assert!(result.unwrap().is_empty());
            }
        }
    }

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
    #[cfg(debug_assertions)]
    fn dev_builds_never_check() {
        // A debug build must never contact the update server.
        assert_eq!(
            unsupported_reason().as_deref(),
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
