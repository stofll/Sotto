//! Installed-version release notes, acknowledged only after the user closes them.

use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tauri::{AppHandle, Manager, State};

use crate::state::AppState;

const MAX_NOTES_BYTES: usize = 128 * 1024;
/// The launch request has the dialog waiting on it and nothing else to show.
const LAUNCH_TIMEOUT: Duration = Duration::from_secs(10);
/// The pre-install request sits in front of the download, so a slow or blocked
/// GitHub is paid for by a progress bar that has not started moving. The
/// manifest body already covers this case; a short budget is enough to prefer
/// the published description when it is actually reachable.
const PRE_INSTALL_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug, Serialize)]
pub struct ReleaseNotes {
    version: String,
    notes: String,
    url: String,
}

fn usable_notes(notes: &str) -> bool {
    !notes.trim().is_empty() && notes.len() <= MAX_NOTES_BYTES
}

fn is_upgrade(current: &str, seen: &str) -> bool {
    match (
        semver::Version::parse(current),
        semver::Version::parse(seen),
    ) {
        (Ok(current), Ok(seen)) => current.cmp_precedence(&seen).is_gt(),
        _ => false,
    }
}

fn pending(conn: &Connection, current: &str) -> rusqlite::Result<bool> {
    // First launch establishes a baseline, including installations predating this feature.
    conn.execute(
        "INSERT OR IGNORE INTO release_notes_state(id, seen_version) VALUES (1, ?1)",
        [current],
    )?;
    let seen: String = conn.query_row(
        "SELECT seen_version FROM release_notes_state WHERE id = 1",
        [],
        |r| r.get(0),
    )?;
    Ok(is_upgrade(current, &seen))
}

fn cached(conn: &Connection, current: &str) -> rusqlite::Result<Option<String>> {
    conn.query_row(
        "SELECT cached_notes FROM release_notes_state WHERE id = 1 AND cached_version = ?1",
        [current],
        |r| r.get(0),
    )
    .optional()
}

fn store_notes(conn: &Connection, version: &str, notes: &str) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE release_notes_state SET cached_version = ?1, cached_notes = ?2 WHERE id = 1",
        (version, notes),
    )?;
    Ok(())
}

fn acknowledge(conn: &Connection, version: &str) -> rusqlite::Result<()> {
    if pending(conn, version)? {
        conn.execute(
            "UPDATE release_notes_state SET seen_version = ?1 WHERE id = 1",
            [version],
        )?;
    }
    Ok(())
}

pub async fn cache_update(app: &AppHandle, version: &str, notes: Option<&str>) {
    // The draft description may be edited after CI generated latest.json, so the
    // published one wins — but only for as long as PRE_INSTALL_TIMEOUT allows,
    // because the download is queued behind this request.
    let notes = fetch_notes(version, PRE_INSTALL_TIMEOUT)
        .await
        .unwrap_or(None)
        .or_else(|| notes.filter(|s| usable_notes(s)).map(str::to_owned));
    let Some(notes) = notes else {
        return;
    };
    let current = app.package_info().version.to_string();
    let version = version.to_owned();
    let db = app.state::<AppState>().db.clone();
    if let Err(error) = crate::run_db_op(db, move |conn| {
        pending(conn, &current)?;
        store_notes(conn, &version, &notes)
    })
    .await
    {
        log::error!("Could not cache release notes before updating: {error}");
    }
}

#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
    body: Option<String>,
    draft: bool,
}

fn release_body(release: GithubRelease, version: &str) -> Option<String> {
    if release.draft || release.tag_name != format!("v{version}") {
        return None;
    }
    release.body.filter(|s| usable_notes(s))
}

async fn fetch_notes(version: &str, timeout: Duration) -> Result<Option<String>, String> {
    fetch_notes_from(
        &format!("https://api.github.com/repos/stofll/Sotto/releases/tags/v{version}"),
        version,
        timeout,
    )
    .await
    .inspect_err(|error| log::error!("Could not read release notes: {error}"))
}

async fn fetch_notes_from(
    url: &str,
    version: &str,
    timeout: Duration,
) -> Result<Option<String>, String> {
    let client = reqwest::Client::builder()
        .timeout(timeout)
        .user_agent(concat!("Sotto/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| e.to_string())?;
    let response = client
        .get(url)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status);
    let mut response = match response {
        Ok(response) => response,
        Err(error) => {
            log::warn!("Release notes unavailable: {}", error.without_url());
            return Ok(None);
        }
    };
    let mut bytes = Vec::new();
    loop {
        let chunk = match response.chunk().await {
            Ok(Some(chunk)) => chunk,
            Ok(None) => break,
            Err(error) => {
                log::warn!(
                    "Release notes download interrupted: {}",
                    error.without_url()
                );
                return Ok(None);
            }
        };
        if bytes.len() + chunk.len() > 1024 * 1024 {
            return Err("Release response too large".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    let release = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    Ok(release_body(release, version))
}

#[tauri::command]
pub async fn get_whats_new(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<ReleaseNotes>, String> {
    if cfg!(debug_assertions) {
        return Ok(None);
    }
    let version = app.package_info().version.to_string();
    let current = version.clone();
    let (show, notes) = crate::run_db_op(state.db.clone(), move |conn| {
        Ok((pending(conn, &current)?, cached(conn, &current)?))
    })
    .await
    .inspect_err(|error| log::error!("Could not read release notes state: {error}"))?;
    if !show {
        return Ok(None);
    }
    let notes = match notes {
        Some(notes) => notes,
        None => {
            // Offline or unpublished notes are retried on the next launch, never acknowledged.
            let Some(notes) = fetch_notes(&version, LAUNCH_TIMEOUT).await? else {
                return Ok(None);
            };
            let current = version.clone();
            let saved = notes.clone();
            crate::run_db_op(state.db.clone(), move |conn| {
                store_notes(conn, &current, &saved)
            })
            .await
            .inspect_err(|error| log::error!("Could not store release notes: {error}"))?;
            notes
        }
    };
    Ok(Some(ReleaseNotes {
        url: format!("https://github.com/stofll/Sotto/releases/tag/v{version}"),
        version,
        notes,
    }))
}

#[tauri::command]
pub async fn dismiss_whats_new(
    app: AppHandle,
    state: State<'_, AppState>,
    version: String,
) -> Result<(), String> {
    if version != app.package_info().version.to_string() {
        return Err("Release version does not match installed version".into());
    }
    crate::run_db_op(state.db.clone(), move |conn| acknowledge(conn, &version))
        .await
        .inspect_err(|error| log::error!("Could not acknowledge release notes: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn serve_notes(status: u16, body: &'static str) -> String {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let _ = stream.read(&mut [0; 4096]);
            write!(
                stream,
                "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        });
        format!("http://{address}")
    }

    #[tokio::test]
    async fn unavailable_notes_are_optional_but_invalid_data_is_an_error() {
        for status in [403, 404, 429, 503] {
            let url = serve_notes(status, "Unavailable");
            assert_eq!(
                fetch_notes_from(&url, "0.2.0", LAUNCH_TIMEOUT).await,
                Ok(None)
            );
        }
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        drop(listener);
        assert_eq!(
            fetch_notes_from(&url, "0.2.0", Duration::from_secs(1)).await,
            Ok(None)
        );
        let url = serve_notes(200, "not JSON");
        assert!(fetch_notes_from(&url, "0.2.0", LAUNCH_TIMEOUT)
            .await
            .is_err());
        let url = serve_notes(
            200,
            r###"{"tag_name":"v0.2.0","draft":false,"body":"## Fixed"}"###,
        );
        assert_eq!(
            fetch_notes_from(&url, "0.2.0", LAUNCH_TIMEOUT)
                .await
                .unwrap(),
            Some("## Fixed".into())
        );
    }

    #[test]
    fn database_failures_are_not_missing_notes() {
        let conn = Connection::open_in_memory().unwrap();
        assert!(pending(&conn, "0.2.0").is_err());
        assert!(cached(&conn, "0.2.0").is_err());
        assert!(store_notes(&conn, "0.2.0", "Notes").is_err());
    }

    #[test]
    fn fresh_install_upgrade_dismiss_restart_and_downgrade() {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::run_migrations(&conn).unwrap();
        assert!(!pending(&conn, "0.1.0").unwrap());
        assert!(pending(&conn, "0.3.0").unwrap());
        assert!(pending(&conn, "0.3.0").unwrap());
        acknowledge(&conn, "0.3.0").unwrap();
        assert!(!pending(&conn, "0.3.0").unwrap());
        assert!(!pending(&conn, "0.2.0").unwrap());
        acknowledge(&conn, "0.2.0").unwrap();
        assert!(!pending(&conn, "0.3.0").unwrap());
        assert!(pending(&conn, "0.4.0").unwrap());
    }

    #[test]
    fn failed_update_does_not_show_future_notes() {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::run_migrations(&conn).unwrap();
        pending(&conn, "0.1.0").unwrap();
        store_notes(&conn, "0.2.0", "## Fixed\n- Example").unwrap();
        assert!(!pending(&conn, "0.1.0").unwrap());
        assert_eq!(cached(&conn, "0.1.0").unwrap(), None);
        assert!(pending(&conn, "0.2.0").unwrap());
        assert_eq!(
            cached(&conn, "0.2.0").unwrap().unwrap(),
            "## Fixed\n- Example"
        );
        assert_eq!(cached(&conn, "0.3.0").unwrap(), None);
    }

    #[test]
    fn semantic_versions_and_metadata() {
        assert!(is_upgrade("0.10.0", "0.9.0"));
        assert!(is_upgrade("1.0.0", "1.0.0-rc.1"));
        assert!(!is_upgrade("1.0.0+build2", "1.0.0+build1"));
        assert!(!is_upgrade("bad", "1.0.0"));
    }

    #[test]
    fn release_must_match_installed_version_and_have_public_notes() {
        for (tag, body, draft, valid) in [
            ("v0.2.0", "## Fixed\n- Example", false, true),
            ("v0.3.0", "Future", false, false),
            ("v0.2.0", "Private", true, false),
            ("v0.2.0", "  ", false, false),
        ] {
            let result = release_body(
                GithubRelease {
                    tag_name: tag.into(),
                    body: Some(body.into()),
                    draft,
                },
                "0.2.0",
            );
            assert_eq!(result.is_some(), valid);
        }
    }
}
