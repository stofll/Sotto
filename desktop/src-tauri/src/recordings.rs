//! Microphone recordings kept on disk when the user asks for it.
//!
//! Off by default: it writes the user's voice to disk, which is not
//! something to switch on behind their back. Each dictation is written while
//! it is transcribed, so a recording exists even when recognition fails; the
//! history entry of that dictation refers to it by file name, which is how
//! History plays it back. A cancelled dictation loses its recording.
//!
//! The folder is separate from the logs: the logs are what people are asked
//! to share when something breaks, and their voice must not travel with them.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use tauri::async_runtime::JoinHandle;

/// Config key: keep a copy of every dictation on disk. The `debug_` prefix
/// is historical — the switch used to live in Help → Diagnostics.
const CONFIG_SAVE: &str = "debug_save_recordings";
/// Config key: how many recordings to keep before the oldest is dropped.
/// `0` means no limit.
const CONFIG_MAX: &str = "debug_max_recordings";

const DEFAULT_MAX_RECORDINGS: usize = 50;
/// Sample rate of the capture pipeline. Recordings are written as captured.
const SAMPLE_RATE: u32 = 16_000;
/// Recordings whose dictation has not ended yet. Every ending releases its
/// entry; the bound is a backstop against a path that forgets to.
const MAX_PENDING: usize = 16;

/// Whether dictations should be kept on disk.
pub fn enabled(config: &serde_json::Value) -> bool {
    config
        .get(CONFIG_SAVE)
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

/// How many recordings to keep; `None` keeps them all.
fn limit(config: &serde_json::Value) -> Option<usize> {
    match config.get(CONFIG_MAX).and_then(serde_json::Value::as_u64) {
        Some(0) => None,
        Some(value) => Some(value as usize),
        None => Some(DEFAULT_MAX_RECORDINGS),
    }
}

/// `<data dir>/recordings`, next to the database rather than inside `logs/`.
pub fn dir() -> PathBuf {
    crate::user_data::data_dir().join("recordings")
}

/// Where builds before the move to Settings kept the recordings.
fn diagnostics_recordings_dir() -> PathBuf {
    crate::debug::diagnostics_dir().join("recordings")
}

/// Start writing one dictation to `recordings/<unix seconds>-<session>.wav`;
/// the task yields the file name.
///
/// Only the conversion to 16-bit samples runs on the caller's thread: the
/// caller is on the way from the stopped recording to transcription, and the
/// write and the limit's directory scan must not delay it. Best-effort and
/// non-fatal: keeping a copy must never break a dictation.
pub fn save(
    config: &serde_json::Value,
    session_id: u64,
    samples: &[f32],
) -> Option<JoinHandle<Option<String>>> {
    if !enabled(config) || samples.is_empty() {
        return None;
    }
    let pcm = crate::wav::f32_to_pcm16(samples);
    let keep = limit(config);
    Some(tauri::async_runtime::spawn_blocking(move || {
        save_in(&dir(), keep, session_id, &pcm)
    }))
}

fn save_in(dir: &Path, keep: Option<usize>, session_id: u64, pcm: &[i16]) -> Option<String> {
    if let Err(error) = std::fs::create_dir_all(dir) {
        log::warn!("recordings dir: {error}");
        return None;
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let name = format!("{stamp}-{session_id}.wav");
    let wav = crate::wav::encode_pcm16_mono(pcm, SAMPLE_RATE);
    if let Err(error) = std::fs::write(dir.join(&name), wav) {
        log::warn!("write recording {name}: {error}");
        return None;
    }
    log::info!("session {session_id}: recording saved");
    if let Some(keep) = keep {
        prune(dir, keep);
    }
    Some(name)
}

fn wav_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "wav"))
        .collect()
}

/// Keep only the newest `keep` recordings.
///
/// Sorted by file name, which starts with a Unix timestamp — no metadata
/// calls, and stable when several recordings land in the same second
/// because the session id breaks the tie.
fn prune(dir: &Path, keep: usize) {
    let mut files = wav_files(dir);
    if files.len() <= keep {
        return;
    }
    files.sort();
    for path in files.iter().take(files.len() - keep) {
        let _ = std::fs::remove_file(path);
    }
}

/// The names of the recordings currently on disk, read once per history
/// listing: files pruned by the limit or deleted by hand must not show a
/// play button that leads nowhere.
pub fn existing_names() -> std::collections::HashSet<String> {
    wav_files(&dir())
        .into_iter()
        .filter_map(|path| Some(path.file_name()?.to_string_lossy().into_owned()))
        .collect()
}

/// The path of a recording named in the database. `None` for anything that
/// is not a plain `.wav` file name: the value is only ever ours, but it is
/// read back from a file on disk and joined to a directory.
fn path_of(dir: &Path, name: &str) -> Option<PathBuf> {
    let plain = Path::new(name).file_name().is_some_and(|file| file == name);
    (plain && name.ends_with(".wav")).then(|| dir.join(name))
}

pub fn read(name: &str) -> Result<Vec<u8>, String> {
    let path = path_of(&dir(), name).ok_or_else(|| "invalid recording name".to_string())?;
    std::fs::read(&path).map_err(|error| format!("read recording {name}: {error}"))
}

/// Delete the recordings of deleted history entries. Missing files are fine.
pub fn remove(names: &[String]) {
    remove_in(&dir(), names);
}

fn remove_in(dir: &Path, names: &[String]) {
    for path in names.iter().filter_map(|name| path_of(dir, name)) {
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => log::warn!("remove recording: {error}"),
        }
    }
}

/// Move recordings left in the diagnostics folder by older builds. Runs at
/// startup; does nothing once that folder is gone.
pub fn adopt_diagnostics_recordings() {
    adopt_from(&diagnostics_recordings_dir(), &dir());
}

fn adopt_from(old: &Path, new: &Path) {
    if !old.is_dir() || old == new {
        return;
    }
    let files = wav_files(old);
    if !files.is_empty() {
        if let Err(error) = std::fs::create_dir_all(new) {
            log::warn!("recordings dir: {error}");
            return;
        }
    }
    for path in files {
        let Some(name) = path.file_name() else {
            continue;
        };
        let target = new.join(name);
        // `rename` fails across volumes, which `SOTTO_LOG_DIR` allows.
        let moved = std::fs::rename(&path, &target).is_ok()
            || (std::fs::copy(&path, &target).is_ok() && std::fs::remove_file(&path).is_ok());
        if !moved {
            log::warn!("could not move a recording out of the logs folder");
        }
    }
    // Only when empty: anything else in there is not ours to delete.
    let _ = std::fs::remove_dir(old);
}

/// Recordings of dictations that have not ended yet.
///
/// The audio is written from the moment capture stops, the history entry
/// after transcription and formatting; the session id carries the write
/// across, and whoever ends the dictation settles what becomes of the file.
#[derive(Default)]
pub struct Pending(Mutex<HashMap<u64, JoinHandle<Option<String>>>>);

impl Pending {
    pub fn put(&self, session_id: u64, saving: JoinHandle<Option<String>>) {
        let mut pending = crate::mutex_recover::lock(&self.0);
        if pending.len() >= MAX_PENDING {
            if let Some(oldest) = pending.keys().min().copied() {
                pending.remove(&oldest);
            }
        }
        pending.insert(session_id, saving);
    }

    /// The file name for the dictation's history entry, once written.
    pub async fn take(&self, session_id: u64) -> Option<String> {
        let saving = crate::mutex_recover::lock(&self.0).remove(&session_id)?;
        saving.await.ok().flatten()
    }

    /// The dictation ended without a history entry. A cancelled one must not
    /// leave the user's voice behind; one that produced no text keeps its
    /// recording in the folder, where it shows what recognition was given.
    pub fn release(&self, session_id: u64, cancelled: bool) {
        let saving = crate::mutex_recover::lock(&self.0).remove(&session_id);
        if let (Some(saving), true) = (saving, cancelled) {
            discard_in(dir(), saving);
        }
    }
}

/// Delete a recording once its write has finished.
fn discard_in(dir: PathBuf, saving: JoinHandle<Option<String>>) -> JoinHandle<()> {
    tauri::async_runtime::spawn(async move {
        if let Ok(Some(name)) = saving.await {
            let _ = tauri::async_runtime::spawn_blocking(move || remove_in(&dir, &[name])).await;
        }
    })
}

#[tauri::command]
pub(crate) async fn open_recordings_folder() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(|| crate::debug::open_in_file_manager(&dir()))
        .await
        .map_err(|e| e.to_string())?
}

/// Bytes the saved recordings occupy. Async with a blocking worker: without a
/// limit the folder can hold thousands of files, and a synchronous command
/// would stat them on the main thread.
#[tauri::command]
pub(crate) async fn recordings_size() -> Result<u64, String> {
    tauri::async_runtime::spawn_blocking(|| {
        wav_files(&dir())
            .iter()
            .filter_map(|path| std::fs::metadata(path).ok())
            .map(|meta| meta.len())
            .sum()
    })
    .await
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn recordings_are_off_by_default() {
        assert!(!enabled(&json!({})));
        assert!(enabled(&json!({ "debug_save_recordings": true })));
    }

    #[test]
    fn saving_is_skipped_when_disabled_or_empty() {
        assert!(save(&json!({}), 1, &[0.1, 0.2]).is_none());
        assert!(save(&json!({ "debug_save_recordings": true }), 1, &[]).is_none());
    }

    #[test]
    fn a_saved_recording_is_a_playable_wav_named_by_session() {
        let dir = tempfile::tempdir().unwrap();
        let pcm = crate::wav::f32_to_pcm16(&[0.0, 0.5, -0.5]);
        let name = save_in(dir.path(), Some(50), 7, &pcm).unwrap();
        assert!(name.ends_with("-7.wav"));
        let bytes = std::fs::read(dir.path().join(&name)).unwrap();
        assert_eq!(&bytes[..4], b"RIFF");
        assert_eq!(bytes.len(), 44 + 3 * 2);
    }

    #[test]
    fn the_limit_keeps_the_newest_files_and_zero_keeps_all() {
        assert_eq!(limit(&json!({})), Some(50));
        assert_eq!(limit(&json!({ "debug_max_recordings": 5 })), Some(5));
        assert_eq!(limit(&json!({ "debug_max_recordings": 0 })), None);
        assert_eq!(limit(&json!({ "debug_max_recordings": "ten" })), Some(50));

        let dir = tempfile::tempdir().unwrap();
        for stamp in ["1000-1", "1001-1", "1002-1", "1003-1"] {
            std::fs::write(dir.path().join(format!("{stamp}.wav")), b"x").unwrap();
        }
        std::fs::write(dir.path().join("notes.txt"), b"x").unwrap();
        prune(dir.path(), 2);
        assert_eq!(names(dir.path()), ["1002-1.wav", "1003-1.wav", "notes.txt"]);
    }

    #[test]
    fn only_plain_wav_names_resolve() {
        let dir = Path::new("recordings");
        assert!(path_of(dir, "1000-1.wav").is_some());
        assert!(path_of(dir, "../sotto.db").is_none());
        assert!(path_of(dir, "sub/1000-1.wav").is_none());
        assert!(path_of(dir, "1000-1.txt").is_none());
    }

    #[test]
    fn old_recordings_move_out_of_the_logs_folder() {
        let root = tempfile::tempdir().unwrap();
        let old = root.path().join("logs/recordings");
        let new = root.path().join("recordings");
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("1000-1.wav"), b"RIFF").unwrap();

        adopt_from(&old, &new);

        assert_eq!(names(&new), ["1000-1.wav"]);
        assert!(!old.exists());
        // A second launch finds nothing to do.
        adopt_from(&old, &new);
        assert_eq!(names(&new), ["1000-1.wav"]);
    }

    fn written(name: &str) -> JoinHandle<Option<String>> {
        let name = name.to_owned();
        tauri::async_runtime::spawn_blocking(move || Some(name))
    }

    #[test]
    fn pending_names_are_taken_once_and_stay_bounded() {
        use tauri::async_runtime::block_on;
        let pending = Pending::default();
        pending.put(1, written("a.wav"));
        assert_eq!(block_on(pending.take(1)).as_deref(), Some("a.wav"));
        assert_eq!(block_on(pending.take(1)), None);

        for session in 0..(MAX_PENDING as u64 + 4) {
            pending.put(session, written(&format!("{session}.wav")));
        }
        assert_eq!(block_on(pending.take(0)), None);
        assert!(block_on(pending.take(MAX_PENDING as u64 + 3)).is_some());
    }

    #[test]
    fn a_dictation_that_ends_without_text_keeps_its_recording_but_not_its_entry() {
        let pending = Pending::default();
        pending.put(1, written("1000-1.wav"));
        pending.release(1, false);
        assert_eq!(tauri::async_runtime::block_on(pending.take(1)), None);
    }

    #[test]
    fn a_cancelled_dictation_loses_its_recording_once_written() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("1000-1.wav"), b"RIFF").unwrap();
        std::fs::write(dir.path().join("1001-2.wav"), b"RIFF").unwrap();
        tauri::async_runtime::block_on(discard_in(dir.path().to_path_buf(), written("1000-1.wav")))
            .unwrap();
        assert_eq!(names(dir.path()), ["1001-2.wav"]);
    }
}
