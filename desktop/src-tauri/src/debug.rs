//! Debug mode: what to ask a user for when you cannot look at their machine.
//!
//! Three things, each cheap on its own:
//!
//! - **Log level from config.** `info` is right for normal use and useless
//!   for a bug that only reproduces on someone else's setup.
//! - **A diagnostics summary** they can copy into an issue: versions, paths,
//!   the settings that actually change behaviour.
//! - **Saved recordings.** The pipeline is audio in, text out; without the
//!   audio, a "it transcribed this wrong" report cannot be reproduced. Off
//!   by default — it writes microphone recordings to disk, which is not
//!   something to switch on behind someone's back.

use std::path::{Path, PathBuf};

use tauri::AppHandle;

/// Config key: `error` | `warn` | `info` | `debug` | `trace`.
const CONFIG_LOG_LEVEL: &str = "log_level";
/// Config key: keep a copy of every recording on disk.
const CONFIG_SAVE_RECORDINGS: &str = "debug_save_recordings";
/// Config key: how many recordings to keep before the oldest is dropped.
const CONFIG_MAX_RECORDINGS: &str = "debug_max_recordings";

const DEFAULT_MAX_RECORDINGS: usize = 50;
/// Sample rate of the capture pipeline. Recordings are dumped as-is.
const RECORDING_SAMPLE_RATE: u32 = 16_000;

/// Parse the configured log level. Unknown values fall back to `Info`
/// rather than to silence — a typo in the config must not turn logging off.
pub fn log_level_from_config(config: &serde_json::Value) -> log::LevelFilter {
    match config
        .get(CONFIG_LOG_LEVEL)
        .and_then(serde_json::Value::as_str)
        .unwrap_or("info")
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "off" => log::LevelFilter::Off,
        "error" => log::LevelFilter::Error,
        "warn" => log::LevelFilter::Warn,
        "debug" => log::LevelFilter::Debug,
        "trace" => log::LevelFilter::Trace,
        _ => log::LevelFilter::Info,
    }
}

/// Whether captured audio should be kept on disk.
pub fn save_recordings_enabled(config: &serde_json::Value) -> bool {
    config
        .get(CONFIG_SAVE_RECORDINGS)
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

fn max_recordings(config: &serde_json::Value) -> usize {
    config
        .get(CONFIG_MAX_RECORDINGS)
        .and_then(serde_json::Value::as_u64)
        .map(|value| value as usize)
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_MAX_RECORDINGS)
}

/// Where diagnostics artefacts live: `<config dir>/logs`, alongside
/// `app.log`, so "send me your logs folder" covers everything.
pub fn diagnostics_dir() -> PathBuf {
    if let Ok(value) = std::env::var("SPEECH_TO_TEXT_LOG_DIR") {
        if !value.trim().is_empty() {
            return PathBuf::from(value);
        }
    }
    crate::db::db_path().join("logs")
}

fn recordings_dir() -> PathBuf {
    diagnostics_dir().join("recordings")
}

/// Write one captured recording to `recordings/<session>-<stamp>.wav`.
///
/// Best-effort and non-fatal: a diagnostics feature must never be able to
/// break a dictation. Returns the path when something was written.
pub fn save_recording(
    config: &serde_json::Value,
    session_id: u64,
    samples: &[f32],
) -> Option<PathBuf> {
    if !save_recordings_enabled(config) || samples.is_empty() {
        return None;
    }
    let dir = recordings_dir();
    if let Err(error) = std::fs::create_dir_all(&dir) {
        log::warn!("recordings dir: {error}");
        return None;
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let path = dir.join(format!("{stamp}-{session_id}.wav"));
    let pcm = crate::wav::f32_to_pcm16(samples);
    let wav = crate::wav::encode_pcm16_mono(&pcm, RECORDING_SAMPLE_RATE);
    if let Err(error) = std::fs::write(&path, wav) {
        log::warn!("write recording {}: {error}", path.display());
        return None;
    }
    prune_recordings(&dir, max_recordings(config));
    Some(path)
}

/// Keep only the newest `keep` recordings.
///
/// Sorted by filename, which starts with a Unix timestamp — no metadata
/// calls, and stable when several recordings land in the same second
/// because the session id breaks the tie.
fn prune_recordings(dir: &Path, keep: usize) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "wav"))
        .collect();
    if files.len() <= keep {
        return;
    }
    files.sort();
    for path in files.iter().take(files.len() - keep) {
        let _ = std::fs::remove_file(path);
    }
}

/// A plain-text summary of the setup, for pasting into a bug report.
///
/// Deliberately not JSON: this is read by a person deciding what to ask
/// next. Nothing here is a secret — API keys live in the OS secret store
/// and are not part of the config values reported.
pub fn diagnostics_report(app: &AppHandle, config: &serde_json::Value) -> String {
    let package = app.package_info();
    let mut out = String::new();
    let mut line = |key: &str, value: String| {
        out.push_str(&format!("{key}: {value}\n"));
    };

    line("app", format!("{} {}", package.name, package.version));
    line("os", std::env::consts::OS.to_string());
    line("arch", std::env::consts::ARCH.to_string());
    line(
        "cpu_threads",
        std::thread::available_parallelism()
            .map(|n| n.get().to_string())
            .unwrap_or_else(|_| "unknown".into()),
    );
    line("gpu_backend", gpu_backend().to_string());

    let str_setting = |key: &str, default: &str| {
        config
            .get(key)
            .and_then(serde_json::Value::as_str)
            .unwrap_or(default)
            .to_string()
    };
    line("model", str_setting("model", "—"));
    line("device", crate::config::resolve_device(config).to_string());
    line("language", str_setting("language", "auto"));
    line("hotkey", str_setting("hotkey", "—"));
    line("recording_mode", str_setting("recording_mode", "—"));
    line(
        "pipeline_mode",
        config
            .get("ai_processing")
            .and_then(|ai| ai.get("pipeline_mode"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("local")
            .to_string(),
    );
    line("log_level", format!("{}", log_level_from_config(config)));
    line(
        "save_recordings",
        save_recordings_enabled(config).to_string(),
    );
    line("logs_dir", diagnostics_dir().display().to_string());
    line(
        "models_dir",
        crate::model::models_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|e| e),
    );
    out
}

/// Which GPU backend this binary was compiled with. Build-time, not
/// runtime — the feature is chosen in `Cargo.toml`, so a report saying
/// "vulkan" and a machine with no Vulkan driver is a meaningful pair.
/// Which whisper.cpp backend this binary was built with.
///
/// Reports `cpu` for a build without the `gpu` feature rather than naming a
/// backend that was never compiled in — this line goes into the summary
/// people paste into bug reports, and "vulkan" on a CPU-only build would
/// send whoever reads it looking for a driver problem that does not exist.
fn gpu_backend() -> &'static str {
    if cfg!(feature = "gpu-metal") {
        "metal"
    } else if cfg!(feature = "gpu-vulkan") {
        "vulkan"
    } else {
        "cpu"
    }
}

/// Reveal a path in the system file manager.
pub fn open_in_file_manager(path: &Path) -> Result<(), String> {
    std::fs::create_dir_all(path).map_err(|error| format!("create {}: {error}", path.display()))?;
    let path = path.to_string_lossy().to_string();
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut c = std::process::Command::new("open");
        c.arg(&path);
        c
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut command = {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(&path);
        c
    };
    #[cfg(windows)]
    let mut command = {
        let mut c = std::process::Command::new("explorer");
        c.arg(&path);
        c
    };
    // Windows `explorer` returns a non-zero exit code even when it opened
    // the folder, so the spawn succeeding is as much confirmation as there
    // is to have.
    command
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("open {path}: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn log_level_defaults_to_info() {
        assert_eq!(log_level_from_config(&json!({})), log::LevelFilter::Info);
    }

    #[test]
    fn log_level_is_read_case_insensitively() {
        assert_eq!(
            log_level_from_config(&json!({ "log_level": "DEBUG" })),
            log::LevelFilter::Debug
        );
        assert_eq!(
            log_level_from_config(&json!({ "log_level": " trace " })),
            log::LevelFilter::Trace
        );
    }

    #[test]
    fn unknown_log_level_falls_back_to_info_not_silence() {
        // A typo must not switch logging off — that is the one outcome
        // nobody would notice until they needed a log.
        assert_eq!(
            log_level_from_config(&json!({ "log_level": "verbose" })),
            log::LevelFilter::Info
        );
        assert_eq!(
            log_level_from_config(&json!({ "log_level": 5 })),
            log::LevelFilter::Info
        );
    }

    #[test]
    fn off_is_still_reachable_deliberately() {
        assert_eq!(
            log_level_from_config(&json!({ "log_level": "off" })),
            log::LevelFilter::Off
        );
    }

    #[test]
    fn recordings_are_off_by_default() {
        assert!(!save_recordings_enabled(&json!({})));
        assert!(save_recordings_enabled(
            &json!({ "debug_save_recordings": true })
        ));
    }

    #[test]
    fn saving_is_skipped_when_disabled_or_empty() {
        assert!(save_recording(&json!({}), 1, &[0.1, 0.2]).is_none());
        assert!(save_recording(&json!({ "debug_save_recordings": true }), 1, &[]).is_none());
    }

    #[test]
    fn prune_keeps_the_newest_files() {
        let dir = tempfile::tempdir().unwrap();
        for stamp in ["1000-1", "1001-1", "1002-1", "1003-1"] {
            std::fs::write(dir.path().join(format!("{stamp}.wav")), b"x").unwrap();
        }
        // A non-wav file must survive: the directory is the diagnostics
        // folder, not ours alone.
        std::fs::write(dir.path().join("notes.txt"), b"x").unwrap();

        prune_recordings(dir.path(), 2);

        let mut left: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(left, ["1002-1.wav", "1003-1.wav", "notes.txt"]);
    }

    #[test]
    fn prune_is_a_no_op_below_the_limit() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("1000-1.wav"), b"x").unwrap();
        prune_recordings(dir.path(), 50);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn max_recordings_rejects_zero_and_garbage() {
        // `0` would mean "delete everything you just wrote".
        assert_eq!(max_recordings(&json!({ "debug_max_recordings": 0 })), 50);
        assert_eq!(
            max_recordings(&json!({ "debug_max_recordings": "ten" })),
            50
        );
        assert_eq!(max_recordings(&json!({ "debug_max_recordings": 5 })), 5);
    }
}
