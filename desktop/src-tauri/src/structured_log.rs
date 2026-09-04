//! Structured file logging (Phase 4 / Batch 6 / P1).
//!
//! Writes JSONL log records to `~/.speech-to-text/logs/app.log` (same
//! directory the legacy Python sidecar used; the path is overridable
//! via `SPEECH_TO_TEXT_LOG_DIR`). The redaction pass scrubs API keys
//! and bearer tokens out of every record so a leaked log file
//! doesn't expose credentials.
//!
//! Records are written in the background by a dedicated thread that
//! consumes an `mpsc::Sender<WriterMsg>`. The `log` crate's emit
//! hook bridges into this channel, so any `log::info!` / `log::warn!`
//! call anywhere in the codebase automatically lands in the file
//! without per-call site changes.
//!
//! That single consumer is also what makes rotation safe. The file grew
//! without a ceiling until [`RotatingWriter`] was added: nearly 17 MB of
//! `app.log` on a machine in ordinary daily use. Because only the writer
//! thread holds the handle, it can close, rename and reopen the file
//! without racing anyone — which Windows requires, since it refuses to
//! rename or delete a file that still has an open handle.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;

use serde::Serialize;

/// Rotate once the active log passes this size. At `Info` the app writes
/// on the order of 5 MB per fortnight of daily use, so one archive covers
/// roughly that span.
const MAX_LOG_BYTES: u64 = 5 * 1024 * 1024;

/// How many rotated archives to keep besides the active file. Together
/// with the size limit this caps the logs directory at ~20 MB.
///
/// Deliberately count-based and not age-based. The size limit already
/// bounds the disk, so deleting by age would buy no space and only take
/// history away — and take the most from someone who dictates rarely,
/// which is exactly when a months-old log is still the only record of
/// what happened.
const KEEP_ROTATED: usize = 3;

/// Log files the removed Python sidecar left behind in the config
/// directory. Nothing has read or trimmed them since Phase 4; one was
/// 6 MB. Swept once per launch, by exact name.
const LEGACY_LOG_NAMES: [&str; 2] = ["sidecar.log", "app.log"];

#[derive(Debug, Clone, Serialize)]
struct LogRecord {
    ts: String,
    level: String,
    target: String,
    message: String,
    file: Option<String>,
    line: Option<u32>,
}

/// What the writer thread accepts. Clearing goes through the same channel
/// as records rather than touching the file directly, because the writer
/// owns both the handle and the byte counter — truncating from a command
/// thread would race the counter, and on Windows could not open the file
/// at all.
enum WriterMsg {
    Record(LogRecord),
    /// Truncate the active log and drop the archives. Carries a channel the
    /// writer signals when it is done, so the caller can report the new
    /// size without racing the thread that produces it.
    Clear(mpsc::Sender<()>),
}

struct State {
    sender: mpsc::Sender<WriterMsg>,
}

static STATE: OnceLock<Arc<Mutex<Option<State>>>> = OnceLock::new();

fn state() -> &'static Arc<Mutex<Option<State>>> {
    STATE.get_or_init(|| Arc::new(Mutex::new(None)))
}

/// Install the file logger. Idempotent: a second call is a no-op.
/// Returns `Ok(())` on success, `Err(message)` if the log file
/// could not be opened.
pub fn install() -> Result<(), String> {
    let mut guard = state().lock().unwrap();
    if guard.is_some() {
        return Ok(());
    }
    let dir = log_dir();
    fs::create_dir_all(&dir)
        .map_err(|error| format!("create log dir {}: {error}", dir.display()))?;
    let path = dir.join("app.log");
    let writer = RotatingWriter::open(path.clone(), MAX_LOG_BYTES, KEEP_ROTATED)
        .map_err(|error| format!("open log file {}: {error}", path.display()))?;
    let (sender, receiver) = mpsc::channel::<WriterMsg>();
    thread::Builder::new()
        .name("structured-log-writer".to_string())
        .spawn(move || run_writer(receiver, writer))
        .map_err(|error| format!("spawn log writer: {error}"))?;
    *guard = Some(State { sender });
    // Bridge `log` → our channel.
    let _ = log::set_logger(&BridgeLogger).map_err(|error| format!("set logger: {error}"));
    log::set_max_level(log::LevelFilter::Info);
    // Nothing above this line may log: `BridgeLogger::log` takes the same
    // lock `guard` is holding, and it is not a reentrant one. Release it
    // first, then sweep — the sweep reports what it deleted.
    drop(guard);
    sweep_legacy_logs(&crate::db::db_path(), &path);
    Ok(())
}

/// Truncate the active log and delete its archives.
///
/// Blocks until the writer thread confirms, so a caller that reports the
/// resulting size cannot read it before the work is done.
pub fn clear() {
    let (done, wait) = mpsc::channel::<()>();
    {
        // Scoped: holding this across `recv` would block every other
        // thread's `log::` call on the writer finishing.
        let guard = state().lock().unwrap();
        let Some(state) = guard.as_ref() else {
            return;
        };
        if state.sender.send(WriterMsg::Clear(done)).is_err() {
            return;
        }
    }
    let _ = wait.recv();
}

/// Bytes on disk for the active log plus its archives — what the Help
/// screen shows next to "open logs folder".
pub fn logs_total_bytes() -> u64 {
    total_bytes(&log_dir().join("app.log"), KEEP_ROTATED)
}

/// Resolved by [`crate::debug::diagnostics_dir`] so the "open logs folder"
/// button cannot point somewhere other than where the log is written. The
/// two used to resolve the home directory independently and would diverge
/// as soon as `SOTTO_CONFIG_DIR` was set.
fn log_dir() -> PathBuf {
    crate::debug::diagnostics_dir()
}

fn run_writer(receiver: mpsc::Receiver<WriterMsg>, mut writer: RotatingWriter) {
    while let Ok(message) = receiver.recv() {
        match message {
            WriterMsg::Record(record) => {
                // Redact the message before writing. The redaction pass
                // catches API keys (sk-…, ghp_…), bearer tokens, and
                // `api_key=value` / `key: "value"` style assignments.
                let mut sanitised = record;
                sanitised.message = redact(&sanitised.message);
                if let Ok(serialised) = serde_json::to_string(&sanitised) {
                    writer.write_line(&serialised);
                }
            }
            WriterMsg::Clear(done) => {
                writer.clear();
                let _ = done.send(());
            }
        }
    }
    // Channel closed; final flush. The file is appended-mode, so
    // we don't need to re-open on the next process launch.
    writer.flush();
    log::debug!("log writer exiting ({})", writer.path.display());
}

/// Owns the log file and the byte counter that decides when to rotate.
///
/// Only the writer thread ever holds one, which is what makes the
/// close/rename/reopen cycle in [`Self::reopen_after`] safe.
struct RotatingWriter {
    /// `None` only after a reopen failed. Logging then stops quietly
    /// rather than taking the app down over a diagnostics feature.
    file: Option<fs::File>,
    path: PathBuf,
    /// Tracked in memory rather than by stat-ing the file per line.
    written: u64,
    limit: u64,
    keep: usize,
}

impl RotatingWriter {
    fn open(path: PathBuf, limit: u64, keep: usize) -> std::io::Result<Self> {
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        // Start from the real size. The file outlives the process, so
        // counting from zero would let every launch add another `limit`
        // before the first rotation.
        let written = file.metadata().map(|meta| meta.len()).unwrap_or(0);
        Ok(Self {
            file: Some(file),
            path,
            written,
            limit,
            keep,
        })
    }

    fn write_line(&mut self, line: &str) {
        let Some(file) = self.file.as_mut() else {
            return;
        };
        if writeln!(file, "{line}").is_err() {
            return;
        }
        // +1 for the newline `writeln!` added.
        self.written += line.len() as u64 + 1;
        if self.written >= self.limit {
            self.reopen_after(rotate_files);
        }
    }

    /// Truncate the active log and delete the archives.
    fn clear(&mut self) {
        self.reopen_after(|path, keep| {
            let _ = fs::remove_file(path);
            remove_archives(path, keep);
        });
    }

    fn flush(&mut self) {
        if let Some(file) = self.file.as_mut() {
            let _ = file.flush();
        }
    }

    /// Close the file, run `action` on the now-free path, reopen empty.
    ///
    /// The close is not optional on Windows: `fs::rename` and
    /// `fs::remove_file` both fail with a sharing violation while a handle
    /// to the file is open.
    fn reopen_after(&mut self, action: impl FnOnce(&Path, usize)) {
        if let Some(mut file) = self.file.take() {
            let _ = file.flush();
            // Dropped here, before `action` touches the path.
        }
        action(&self.path, self.keep);
        self.file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .ok();
        self.written = 0;
    }
}

/// Shift `app.log` into `app.log.1`, pushing existing archives one step
/// down and dropping whatever falls past `keep`.
///
/// Only valid with the log file closed — see [`RotatingWriter::reopen_after`].
fn rotate_files(path: &Path, keep: usize) {
    if keep == 0 {
        let _ = fs::remove_file(path);
        return;
    }
    // No need to delete the archive falling off the end first: `fs::rename`
    // replaces an existing destination on every platform we ship, Windows
    // included (it maps to `MoveFileEx` with `MOVEFILE_REPLACE_EXISTING`).
    for index in (1..keep).rev() {
        let _ = fs::rename(archive_path(path, index), archive_path(path, index + 1));
    }
    let _ = fs::rename(path, archive_path(path, 1));
}

fn remove_archives(path: &Path, keep: usize) {
    for index in 1..=keep {
        let _ = fs::remove_file(archive_path(path, index));
    }
}

/// `app.log` + 2 → `app.log.2`. A suffix rather than a replaced extension,
/// so the archives sort next to the active file and stay obviously related
/// to it.
fn archive_path(path: &Path, index: usize) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".{index}"));
    path.with_file_name(name)
}

fn total_bytes(path: &Path, keep: usize) -> u64 {
    std::iter::once(path.to_path_buf())
        .chain((1..=keep).map(|index| archive_path(path, index)))
        .filter_map(|candidate| fs::metadata(candidate).ok())
        .map(|meta| meta.len())
        .sum()
}

/// Delete the log files the removed Python sidecar left in the config
/// directory. Best-effort, once per launch.
///
/// Matched by exact name, never by glob, and guarded against `active`: the
/// legacy `app.log` sits in `<config dir>` while the live one sits in
/// `<config dir>/logs`, so the two differ only by directory. If
/// `SPEECH_TO_TEXT_LOG_DIR` ever points the live log at the config
/// directory itself, that guard is the only thing standing between this
/// sweep and the file we are writing to.
fn sweep_legacy_logs(legacy_dir: &Path, active: &Path) {
    for name in LEGACY_LOG_NAMES {
        let candidate = legacy_dir.join(name);
        if same_file(&candidate, active) {
            continue;
        }
        match fs::remove_file(&candidate) {
            Ok(()) => log::info!("removed legacy log {}", candidate.display()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => log::warn!("remove legacy log {}: {error}", candidate.display()),
        }
    }
}

/// Compares paths after canonicalising, so a `logs/../app.log` spelling
/// cannot slip past the guard. Falls back to a literal comparison when a
/// path does not resolve — a file that does not exist is nothing to
/// protect.
fn same_file(left: &Path, right: &Path) -> bool {
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

/// Best-effort redaction of common credential shapes. The set is
/// deliberately conservative: false positives (a literal
/// `sk-test-...` showing up in a non-secret log line) are
/// acceptable; false negatives (a real key slipping through) are
/// not.
pub fn redact(input: &str) -> String {
    let mut output = input.to_string();
    for pattern in REDACT_PATTERNS.iter() {
        // Replace with a stable placeholder so the rest of the
        // log line stays useful.
        output = pattern.replace_all(&output, "[REDACTED]").into_owned();
    }
    output
}

use once_cell::sync::Lazy;
use regex::Regex;

static REDACT_PATTERNS: Lazy<Vec<Regex>> = Lazy::new(|| {
    [
        // OpenAI / Anthropic API keys.
        r"sk-[A-Za-z0-9_-]{16,}",
        r"sk-ant-[A-Za-z0-9_-]{16,}",
        r"sk-or-[A-Za-z0-9_-]{16,}",
        r"sk-proj-[A-Za-z0-9_-]{16,}",
        r"sk-live-[A-Za-z0-9_-]{16,}",
        r"AIzaSy[A-Za-z0-9_-]{20,}",
        r"ghp_[A-Za-z0-9]{20,}",
        // Bearer / Basic auth headers.
        r"(?i)Bearer\s+[A-Za-z0-9._~+/=-]{8,}",
        r"(?i)Basic\s+[A-Za-z0-9+/=]{8,}",
        // `key = "..."` / `key: "..."` / `key = ...` JSON-ish assignments.
        r#"(?i)("?api[_-]?key"?\s*[:=]\s*"?)[^"\s,}]+"#,
        r#"(?i)("?secret"?\s*[:=]\s*"?)[^"\s,}]+"#,
        r#"(?i)("?password"?\s*[:=]\s*"?)[^"\s,}]+"#,
    ]
    .iter()
    .map(|source| Regex::new(source).expect("valid redact regex"))
    .collect()
});

struct BridgeLogger;

impl log::Log for BridgeLogger {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= current_level()
    }

    fn log(&self, record: &log::Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let guard = state().lock().unwrap();
        let Some(state) = guard.as_ref() else { return };
        let payload = LogRecord {
            ts: chrono_like_now(),
            level: record.level().to_string(),
            target: record.target().to_string(),
            message: format!("{}", record.args()),
            file: record.file().map(str::to_string),
            line: record.line(),
        };
        // Best-effort: the writer thread is the single consumer
        // and never returns Err, so a full channel just means
        // we're shutting down.
        let _ = state.sender.send(WriterMsg::Record(payload));
    }

    fn flush(&self) {}
}

/// UTC timestamp, RFC 3339.
///
/// The date part used to be computed with approximate month and year
/// lengths, which put every log line on the wrong day — fine while the
/// logs were only ever read by whoever produced them, useless the moment
/// someone sends you a log and says "it broke on Tuesday".
/// `stats::days_to_ymd` is the real civil-date conversion, already in the
/// codebase for the daily-stats keys.
fn chrono_like_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let total_seconds = duration.as_secs();
    let (year, month, day) = crate::stats::days_to_ymd((total_seconds / 86_400) as i64);
    let seconds_today = total_seconds % 86_400;
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        year,
        month,
        day,
        seconds_today / 3600,
        (seconds_today / 60) % 60,
        seconds_today % 60,
        duration.subsec_millis()
    )
}

/// Raise or lower the level at which records are kept. Called at startup
/// once config is readable, and again whenever the setting changes.
pub fn set_level(level: log::LevelFilter) {
    MAX_LEVEL.store(level as usize, std::sync::atomic::Ordering::Relaxed);
    log::set_max_level(level);
}

fn current_level() -> log::LevelFilter {
    match MAX_LEVEL.load(std::sync::atomic::Ordering::Relaxed) {
        0 => log::LevelFilter::Off,
        1 => log::LevelFilter::Error,
        2 => log::LevelFilter::Warn,
        4 => log::LevelFilter::Debug,
        5 => log::LevelFilter::Trace,
        _ => log::LevelFilter::Info,
    }
}

/// Mirrors what `log::set_max_level` was last told. `log`'s own filter is
/// checked before `BridgeLogger::enabled`, but the logger has to agree with
/// it — otherwise raising the level would let records through the crate's
/// filter only to be dropped here.
static MAX_LEVEL: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(log::LevelFilter::Info as usize);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redact_masks_openai_keys() {
        let input = "request failed: sk-abcdefghijklmnop12345";
        let output = redact(input);
        assert!(output.contains("[REDACTED]"), "got: {output}");
        assert!(!output.contains("abcdefghijklmnop"));
    }

    #[test]
    fn redact_masks_anthropic_keys() {
        let input = "x-api-key: sk-ant-foobarbazqux12345";
        let output = redact(input);
        assert!(output.contains("[REDACTED]"));
        assert!(!output.contains("foobarbazqux"));
    }

    #[test]
    fn redact_masks_bearer_tokens() {
        let input = "Authorization: Bearer abcdef1234567890";
        let output = redact(input);
        assert!(output.contains("[REDACTED]"));
        assert!(!output.contains("abcdef1234567890"));
    }

    #[test]
    fn redact_masks_json_style_keys() {
        let input = r#"{"api_key": "sk-secret1234567890", "model": "haiku"}"#;
        let output = redact(input);
        assert!(output.contains("[REDACTED]"), "got: {output}");
        assert!(!output.contains("sk-secret1234567890"));
    }

    #[test]
    fn redact_leaves_unrelated_strings_intact() {
        let input = "level=info, message=\"model loaded: haiku\"";
        let output = redact(input);
        assert_eq!(output, input);
    }

    /// `app.log` with the given text, in `dir`.
    fn write_log(dir: &Path, name: &str, contents: &str) -> PathBuf {
        let path = dir.join(name);
        fs::write(&path, contents).unwrap();
        path
    }

    fn read_log(path: &Path) -> String {
        fs::read_to_string(path).unwrap_or_default()
    }

    #[test]
    fn rotation_moves_the_active_log_into_the_first_archive() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_log(dir.path(), "app.log", "oldest line\n");

        rotate_files(&path, 3);

        assert!(!path.exists(), "active log should have been renamed away");
        assert_eq!(read_log(&archive_path(&path, 1)), "oldest line\n");
    }

    #[test]
    fn rotation_shifts_existing_archives_one_step_down() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_log(dir.path(), "app.log", "current\n");
        write_log(dir.path(), "app.log.1", "previous\n");
        write_log(dir.path(), "app.log.2", "older\n");

        rotate_files(&path, 3);

        assert_eq!(read_log(&archive_path(&path, 1)), "current\n");
        assert_eq!(read_log(&archive_path(&path, 2)), "previous\n");
        assert_eq!(read_log(&archive_path(&path, 3)), "older\n");
    }

    #[test]
    fn rotation_drops_the_archive_pushed_past_keep() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_log(dir.path(), "app.log", "current\n");
        write_log(dir.path(), "app.log.1", "previous\n");
        write_log(dir.path(), "app.log.2", "older\n");
        write_log(dir.path(), "app.log.3", "oldest\n");

        rotate_files(&path, 3);

        // "oldest" fell off the end; nothing was kept beyond `keep`.
        assert!(!archive_path(&path, 4).exists());
        assert_eq!(read_log(&archive_path(&path, 3)), "older\n");
        assert_ne!(read_log(&archive_path(&path, 3)), "oldest\n");
    }

    #[test]
    fn rotation_leaves_unrelated_files_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_log(dir.path(), "app.log", "current\n");
        write_log(dir.path(), "sotto.db", "database\n");
        write_log(dir.path(), "other.log", "someone else\n");

        rotate_files(&path, 3);

        assert_eq!(read_log(&dir.path().join("sotto.db")), "database\n");
        assert_eq!(read_log(&dir.path().join("other.log")), "someone else\n");
    }

    #[test]
    fn writer_rotates_once_the_limit_is_passed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app.log");
        let limit = 64u64;
        let mut writer = RotatingWriter::open(path.clone(), limit, 3).unwrap();

        // One line at the limit rotates; the short line after it does not,
        // so the archive holds exactly the first write.
        let long = "a".repeat(limit as usize);
        writer.write_line(&long);
        writer.write_line("after rotation");
        writer.flush();

        assert_eq!(read_log(&archive_path(&path, 1)), format!("{long}\n"));
        assert_eq!(read_log(&path), "after rotation\n");
    }

    #[test]
    fn writer_appends_without_rotating_below_the_limit() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app.log");
        let mut writer = RotatingWriter::open(path.clone(), 1024, 3).unwrap();

        writer.write_line("one");
        writer.write_line("two");
        writer.flush();

        assert!(!archive_path(&path, 1).exists(), "should not have rotated");
        assert_eq!(read_log(&path), "one\ntwo\n");
    }

    #[test]
    fn writer_counts_bytes_already_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        // A file left by a previous process run, already at the limit.
        let path = write_log(dir.path(), "app.log", &"x".repeat(64));
        let mut writer = RotatingWriter::open(path.clone(), 64, 3).unwrap();

        writer.write_line("one more line");
        writer.flush();

        // Counting from zero instead would have let this launch add
        // another full limit before rotating.
        assert!(
            archive_path(&path, 1).exists(),
            "should have rotated at once"
        );
    }

    #[test]
    fn clearing_empties_the_active_log_and_drops_archives() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app.log");
        write_log(dir.path(), "app.log.1", "previous\n");
        write_log(dir.path(), "app.log.2", "older\n");
        let mut writer = RotatingWriter::open(path.clone(), 1024, 3).unwrap();
        writer.write_line("before clearing");

        writer.clear();
        writer.write_line("after clearing");
        writer.flush();

        assert_eq!(read_log(&path), "after clearing\n");
        assert!(!archive_path(&path, 1).exists());
        assert!(!archive_path(&path, 2).exists());
    }

    #[test]
    fn total_bytes_sums_the_active_log_and_its_archives() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_log(dir.path(), "app.log", "12345");
        write_log(dir.path(), "app.log.1", "123");
        write_log(dir.path(), "app.log.2", "12");

        assert_eq!(total_bytes(&path, 3), 10);
    }

    #[test]
    fn total_bytes_ignores_files_that_are_not_ours() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_log(dir.path(), "app.log", "12345");
        write_log(dir.path(), "recordings.wav", &"x".repeat(4096));

        assert_eq!(total_bytes(&path, 3), 5);
    }

    #[test]
    fn legacy_sweep_removes_the_python_sidecar_leftovers() {
        let config_dir = tempfile::tempdir().unwrap();
        let logs_dir = config_dir.path().join("logs");
        fs::create_dir_all(&logs_dir).unwrap();
        let active = write_log(&logs_dir, "app.log", "live\n");
        write_log(config_dir.path(), "sidecar.log", "python\n");
        write_log(config_dir.path(), "app.log", "pre-Phase-4\n");

        sweep_legacy_logs(config_dir.path(), &active);

        assert!(!config_dir.path().join("sidecar.log").exists());
        assert!(!config_dir.path().join("app.log").exists());
        assert_eq!(read_log(&active), "live\n", "the live log must survive");
    }

    #[test]
    fn legacy_sweep_spares_the_active_log_when_the_paths_collide() {
        // SPEECH_TO_TEXT_LOG_DIR can point the live log at the config
        // directory itself, where it shares a name with the legacy file.
        let config_dir = tempfile::tempdir().unwrap();
        let active = write_log(config_dir.path(), "app.log", "live\n");
        write_log(config_dir.path(), "sidecar.log", "python\n");

        sweep_legacy_logs(config_dir.path(), &active);

        assert_eq!(read_log(&active), "live\n", "deleted the log in use");
        assert!(!config_dir.path().join("sidecar.log").exists());
    }

    #[test]
    fn legacy_sweep_leaves_unrelated_files_alone() {
        let config_dir = tempfile::tempdir().unwrap();
        let logs_dir = config_dir.path().join("logs");
        fs::create_dir_all(&logs_dir).unwrap();
        let active = write_log(&logs_dir, "app.log", "live\n");
        write_log(config_dir.path(), "sotto.db", "database\n");
        write_log(config_dir.path(), "config.json", "{}\n");

        sweep_legacy_logs(config_dir.path(), &active);

        assert_eq!(read_log(&config_dir.path().join("sotto.db")), "database\n");
        assert_eq!(read_log(&config_dir.path().join("config.json")), "{}\n");
    }
}
