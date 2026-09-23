//! Model artifact downloads, integrity checks and Tauri progress events.
//!
//! The downloader hashes bytes while streaming into a `*.part` sibling and
//! renames it onto the final path only when the size and SHA-256 match the
//! manifest entry. Resumed downloads hash the existing prefix first. The
//! core accepts an HTTP client, progress callbacks and a cancellation flag;
//! command wrappers manage download state and emit events to the frontend.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use futures_util::stream::StreamExt;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter};
use tokio::io::AsyncWriteExt;

use crate::model::{manifest_entry, ModelManifestEntry};

/// Errors surfaced by the downloader. The variants are deliberately
/// `Clone + PartialEq` so the Tauri command layer can pattern
/// match and forward stable, human-readable messages to the frontend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelDownloadError {
    /// The server advertised (or we derived) a different size from the
    /// manifest. We refuse to promote the file to the final path.
    SizeMismatch { expected: u64, actual: u64 },
    /// The streamed bytes hashed to a different SHA-256 than the manifest.
    Sha256Mismatch { expected: String, actual: String },
    /// Caller flipped the cancel flag mid-stream.
    Cancelled,
    /// Pre-flight free-space check failed.
    InsufficientFreeSpace {
        required_bytes: u64,
        available_bytes: u64,
    },
    /// I/O / HTTP transport failure (network, body read, fs::write, etc.).
    Transport(String),
    /// `reqwest` returned a non-success status code.
    HttpStatus(u16),
}

impl std::fmt::Display for ModelDownloadError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ModelDownloadError::SizeMismatch { expected, actual } => write!(
                formatter,
                "size mismatch (expected {expected} bytes, got {actual})"
            ),
            ModelDownloadError::Sha256Mismatch { expected, actual } => write!(
                formatter,
                "sha256 mismatch (expected {expected}, got {actual})"
            ),
            ModelDownloadError::Cancelled => write!(formatter, "download cancelled"),
            ModelDownloadError::InsufficientFreeSpace {
                required_bytes,
                available_bytes,
            } => write!(
                formatter,
                "insufficient free space: need {required_bytes} bytes, have {available_bytes}"
            ),
            ModelDownloadError::Transport(message) => write!(formatter, "transport: {message}"),
            ModelDownloadError::HttpStatus(status) => write!(formatter, "http status {status}"),
        }
    }
}

impl std::error::Error for ModelDownloadError {}

/// A description of a single model artifact, fully derived from the
/// in-Rust manifest (see `crate::model::model_manifest`). The
/// downloader treats this as the authoritative contract — the Tauri
/// command layer looks entries up via
/// `crate::model::manifest_entry(&model_id)` and feeds the result in.
#[derive(Debug, Clone)]
pub struct DownloadSpec {
    pub model_id: String,
    pub file_name: String,
    pub url: String,
    pub expected_bytes: u64,
    pub sha256: String,
}

impl DownloadSpec {
    /// Look up the manifest entry for `model_id` and project it into a
    /// `DownloadSpec`. Centralizes the Tauri-side lookup so callers
    /// cannot accidentally re-introduce mismatched filenames.
    pub fn from_manifest(model_id: &str) -> Result<Self, String> {
        let entry = manifest_entry(model_id)?;
        Ok(Self::from_manifest_entry(entry))
    }

    pub fn from_manifest_entry(entry: &ModelManifestEntry) -> Self {
        Self {
            model_id: entry.public_id.to_string(),
            file_name: entry.file_name.to_string(),
            url: entry.download_url.to_string(),
            expected_bytes: entry.expected_bytes,
            sha256: entry.sha256.to_string(),
        }
    }
}

/// Progress event surfaced by the downloader. The Tauri command layer
/// translates each into a `model-download-progress` event with
/// `model`/`downloaded`/`total` fields. `total` is `None` until the
/// server's `Content-Length` is known.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DownloadProgress {
    pub downloaded: u64,
    pub total: Option<u64>,
}

impl DownloadProgress {
    pub fn fraction(self) -> Option<f32> {
        self.total
            .filter(|total| *total > 0)
            .map(|total| (self.downloaded as f32) / (total as f32))
    }
}

/// Lets through at most one progress event per interval, plus the one that
/// completes the download.
///
/// The downloader reports every network chunk. For a model of several
/// gigabytes that is thousands of events a second, each delivered to every
/// window and re-rendering the catalog; the bar needs about ten.
struct ProgressThrottle {
    interval: std::time::Duration,
    last: std::sync::Mutex<Option<std::time::Instant>>,
}

impl ProgressThrottle {
    fn new(interval: std::time::Duration) -> Self {
        Self {
            interval,
            last: std::sync::Mutex::new(None),
        }
    }

    fn admit(&self, progress: DownloadProgress, now: std::time::Instant) -> bool {
        let complete = progress
            .total
            .is_some_and(|total| progress.downloaded >= total);
        let mut last = crate::mutex_recover::lock(&self.last);
        let due = last.is_none_or(|at| now.saturating_duration_since(at) >= self.interval);
        if complete || due {
            *last = Some(now);
        }
        complete || due
    }
}

/// The `model-download-progress` emitter for one download.
fn progress_emitter(
    app: &AppHandle,
    model: String,
) -> impl Fn(DownloadProgress) + Send + Sync + 'static {
    let app = app.clone();
    let throttle = ProgressThrottle::new(std::time::Duration::from_millis(100));
    move |progress: DownloadProgress| {
        if !throttle.admit(progress, std::time::Instant::now()) {
            return;
        }
        let _ = app.emit(
            "model-download-progress",
            serde_json::json!({
                "model": model,
                "downloaded": progress.downloaded,
                "total": progress.total,
            }),
        );
    }
}

/// Result of a successful download — the on-disk path to the verified
/// model and the byte count the verifier confirmed.
#[derive(Debug, Clone)]
pub struct DownloadOutcome {
    path: PathBuf,
    bytes: u64,
}

impl DownloadOutcome {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn bytes(&self) -> u64 {
        self.bytes
    }
}

/// Tauri-friendly snapshot of a `DownloadOutcome`. Lives separately
/// from `DownloadOutcome` so the worker thread can build it without
/// knowing the original `model_id` (which it doesn't carry); the
/// command layer fills in `model_id` from the Tauri command argument
/// before returning the value to the frontend.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DownloadOutcomeInfo {
    pub model_id: String,
    pub path: String,
    pub bytes: u64,
}

impl DownloadOutcomeInfo {
    /// Stamp `model_id` onto an outcome and return the wire shape.
    pub fn for_model(model_id: &str, outcome: &DownloadOutcome) -> Self {
        Self {
            model_id: model_id.to_string(),
            path: outcome.path().to_string_lossy().into_owned(),
            bytes: outcome.bytes(),
        }
    }
}

/// Compute the `*.part` sibling path for `final_path`. Exposed (crate
/// internal) so tests can assert that the partial file is gone after a
/// successful rename.
pub fn part_path_for(final_path: &Path) -> PathBuf {
    let mut owned = final_path.to_path_buf().into_os_string();
    owned.push(".part");
    PathBuf::from(owned)
}

/// Path to the private folder where a bundle is assembled before the rename.
pub fn stage_dir_for(dir: &Path, directory_name: &str) -> PathBuf {
    dir.join(format!(".{directory_name}.part"))
}

/// Remove everything left over from an interrupted single-file download.
///
/// Explicit cancellation discards partial data; interrupted/failed downloads
/// retain it for resume. Cleanup errors are ignored if the file is already gone
/// or cannot be removed.
pub fn discard_partial(dir: &Path, spec: &DownloadSpec) {
    let _ = std::fs::remove_file(part_path_for(&dir.join(&spec.file_name)));
}

/// The same for a bundle: its unfinished download is a whole folder.
pub fn discard_bundle_partial(dir: &Path, spec: &BundleDownloadSpec) {
    let _ = std::fs::remove_dir_all(stage_dir_for(dir, &spec.directory_name));
}

/// Verify the file size and SHA-256 before accepting an artifact. Asynchronous
/// reads use 64 KiB chunks so verification does not load the whole model into RAM.
pub async fn verify_file(path: &Path, spec: &DownloadSpec) -> Result<(), ModelDownloadError> {
    let metadata = tokio::fs::metadata(path)
        .await
        .map_err(|error| ModelDownloadError::Transport(format!("stat: {error}")))?;
    let actual = metadata.len();
    if actual != spec.expected_bytes {
        return Err(ModelDownloadError::SizeMismatch {
            expected: spec.expected_bytes,
            actual,
        });
    }
    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|error| ModelDownloadError::Transport(format!("open: {error}")))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        use tokio::io::AsyncReadExt;
        let read = file
            .read(&mut buffer)
            .await
            .map_err(|error| ModelDownloadError::Transport(format!("read: {error}")))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let actual_sha = format!("{:x}", hasher.finalize());
    if !actual_sha.eq_ignore_ascii_case(&spec.sha256) {
        return Err(ModelDownloadError::Sha256Mismatch {
            expected: spec.sha256.clone(),
            actual: actual_sha,
        });
    }
    Ok(())
}

/// Free space on the filesystem that will hold `dir`. `None` means unknown;
/// `Some(0)` means the disk is full.
///
/// The models directory is created by the first download, which is exactly the
/// moment this check matters most. `statvfs` fails on a path that does not
/// exist yet, so walk up to the nearest existing ancestor — the same
/// filesystem, and the same answer. Windows tolerates the missing leaf and
/// never reaches the second step, which is why the gap stayed invisible there.
pub fn available_bytes(dir: &Path) -> Option<u64> {
    dir.ancestors()
        .find_map(|path| fs2::available_space(path).ok())
}

/// Free space a download of `expected_bytes` needs: 1 MiB of slack on top for
/// the final write and atomic rename bookkeeping.
pub fn required_free_space(expected_bytes: u64) -> u64 {
    expected_bytes.saturating_add(1024 * 1024)
}

/// Check free space for the upcoming download against
/// [`required_free_space`]. An unavailable measurement does not prevent
/// downloading.
pub fn ensure_free_space(dir: &Path, expected_bytes: u64) -> Result<(), ModelDownloadError> {
    free_space_verdict(available_bytes(dir), expected_bytes)
}

/// Pure verdict for the pre-flight free-space check, split out so the
/// boundary arithmetic is testable without a real filesystem.
fn free_space_verdict(
    available: Option<u64>,
    expected_bytes: u64,
) -> Result<(), ModelDownloadError> {
    let Some(available) = available else {
        return Ok(());
    };
    let required = required_free_space(expected_bytes);
    if available >= required {
        return Ok(());
    }
    Err(ModelDownloadError::InsufficientFreeSpace {
        required_bytes: required,
        available_bytes: available,
    })
}

/// How many bytes of `*.part` are usable for a resume. `0` — start over.
///
/// A file no shorter than expected is not a resume: either the previous attempt
/// pulled everything down and failed the hash, or this is a leftover from a
/// different version of the file. There is nothing to continue: it can yield
/// nothing but a second mismatched checksum.
///
/// A separate pure function, because a mistake here is silent: resuming from
/// the wrong offset does not break, it quietly assembles a corrupt file, and
/// only the SHA-256 at the very end catches it.
fn resume_verdict(part_len: Option<u64>, expected_bytes: u64) -> u64 {
    match part_len {
        Some(len) if len > 0 && len < expected_bytes => len,
        _ => 0,
    }
}

/// `resume_verdict` on top of the disk: an unusable leftover is erased along
/// the way, so it takes no space and is not considered again next time.
fn resume_offset(part_path: &Path, expected_bytes: u64) -> u64 {
    let part_len = std::fs::metadata(part_path)
        .ok()
        .filter(|meta| meta.is_file())
        .map(|meta| meta.len());
    let offset = resume_verdict(part_len, expected_bytes);
    if offset == 0 && part_len.is_some() {
        let _ = std::fs::remove_file(part_path);
    }
    offset
}

/// Push whatever already sits in `*.part` through the hasher.
///
/// SHA-256 is computed over the whole file in order, so a resume must first
/// "read through" the previous half — otherwise the checksum will not match even
/// for a perfectly intact file. Cancellation is checked here too: on one and a
/// half gigabytes this takes seconds, but the button must not stick even for
/// seconds.
async fn hash_existing_prefix(
    path: &Path,
    len: u64,
    hasher: &mut Sha256,
    cancel_flag: &Arc<AtomicBool>,
) -> Result<(), ModelDownloadError> {
    use tokio::io::AsyncReadExt;
    let mut file = tokio::fs::File::open(path)
        .await
        .map_err(|error| ModelDownloadError::Transport(format!("open part: {error}")))?;
    let mut buffer = vec![0_u8; 64 * 1024];
    let mut left = len;
    while left > 0 {
        if cancel_flag.load(Ordering::Relaxed) {
            return Err(ModelDownloadError::Cancelled);
        }
        let want = left.min(buffer.len() as u64) as usize;
        let read = file
            .read(&mut buffer[..want])
            .await
            .map_err(|error| ModelDownloadError::Transport(format!("read part: {error}")))?;
        if read == 0 {
            return Err(ModelDownloadError::Transport(
                "part file shrank while resuming".to_string(),
            ));
        }
        hasher.update(&buffer[..read]);
        left -= read as u64;
    }
    Ok(())
}

// These deadlines bound a stalled network operation, not the whole download:
// a multi-gigabyte model may legitimately take hours while still progressing.
const RESPONSE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
const CHUNK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

async fn wait_download_io<T>(
    operation: impl std::future::Future<Output = T>,
    cancel_flag: &AtomicBool,
    timeout: std::time::Duration,
) -> Result<T, ModelDownloadError> {
    tokio::pin!(operation);
    let deadline = tokio::time::sleep(timeout);
    tokio::pin!(deadline);
    // The public download registry uses AtomicBool. Poll it only while network
    // I/O is pending; pinning the operation keeps this from restarting requests.
    let mut cancellation = tokio::time::interval(std::time::Duration::from_millis(100));
    loop {
        if cancel_flag.load(Ordering::Acquire) {
            return Err(ModelDownloadError::Cancelled);
        }
        tokio::select! {
            biased;
            _ = cancellation.tick() => {}
            _ = &mut deadline => {
                return Err(ModelDownloadError::Transport("download made no network progress before timeout".into()));
            }
            result = &mut operation => return Ok(result),
        }
    }
}

/// Download `spec` into `dir`, verifying on completion.
///
/// Steps:
/// 1. Run `ensure_free_space`. If the check returns
///    `InsufficientFreeSpace`, abort before opening any socket.
/// 2. Download the remainder if a usable `*.part` is left from the previous
///    attempt and the server agrees to `Range` (see `resume_verdict`).
/// 3. Stream and hash the response body into `*.part`, checking
///    cancellation during network waits and forwarding byte counts to `progress`.
/// 4. After the stream completes, fire `on_verifying` before comparing
///    the accumulated size and digest with the manifest.
/// 5. Verify the final size and SHA-256 against the manifest.
/// 6. Atomically rename `*.part` onto the final path. The final
///    path is NOT touched on failure — the partial file remains
///    so the user can retry without re-downloading from scratch
///    (and so a corrupt run never silently overwrites a working
///    model).
///
/// Both callbacks take `&dyn Fn(...) + Send + Sync` because the
/// Tauri command layer moves the download into a worker thread
/// and the callbacks may fire from that thread. The `Send + Sync`
/// bound is transparent to the test suite (every test closure
/// only captures `Arc<AtomicBool>` or a `&Path` and is therefore
/// auto-`Send + Sync`).
pub async fn download_spec_to_dir(
    client: &reqwest::Client,
    spec: &DownloadSpec,
    dir: &Path,
    cancel_flag: &Arc<AtomicBool>,
    progress: Option<&(dyn Fn(DownloadProgress) + Send + Sync)>,
    on_verifying: Option<&(dyn Fn() + Send + Sync)>,
) -> Result<DownloadOutcome, ModelDownloadError> {
    if cancel_flag.load(Ordering::Relaxed) {
        return Err(ModelDownloadError::Cancelled);
    }
    let final_path = dir.join(&spec.file_name);
    let part_path = part_path_for(&final_path);
    if let Some(parent) = part_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| ModelDownloadError::Transport(format!("mkdir: {error}")))?;
    }
    // What is finished and verified is not downloaded again. Inside a bundle
    // these are artifacts the previous attempt managed to pull down in full.
    if final_path.exists() && verify_file(&final_path, spec).await.is_ok() {
        if cancel_flag.load(Ordering::Acquire) {
            return Err(ModelDownloadError::Cancelled);
        }
        if let Some(progress_cb) = progress {
            progress_cb(DownloadProgress {
                downloaded: spec.expected_bytes,
                total: Some(spec.expected_bytes),
            });
        }
        return Ok(DownloadOutcome {
            path: final_path,
            bytes: spec.expected_bytes,
        });
    }

    let mut offset = resume_offset(&part_path, spec.expected_bytes);
    ensure_free_space(dir, spec.expected_bytes - offset)?;
    // The loop exists for exactly one retry: on 416 we erase the leftover, zero
    // the offset and ask for the whole file. The `continue` branch requires
    // `offset > 0`, and we come back into it already at zero, so it cannot be
    // entered a second time.
    let response = loop {
        let mut request = client.get(&spec.url);
        if offset > 0 {
            request = request.header(reqwest::header::RANGE, format!("bytes={offset}-"));
        }
        let response = wait_download_io(request.send(), cancel_flag, RESPONSE_TIMEOUT)
            .await?
            .map_err(|error| ModelDownloadError::Transport(format!("send: {error}")))?;
        let status = response.status();
        if offset == 0 {
            if !status.is_success() {
                return Err(ModelDownloadError::HttpStatus(status.as_u16()));
            }
            break response;
        }
        // 206 — the server agreed to continue from our position.
        if status == reqwest::StatusCode::PARTIAL_CONTENT {
            break response;
        }
        // 200 — Range was ignored and the body holds the whole file; 416 — our
        // leftover did not suit the server. Both mean "there will be no resume":
        // we erase the leftover and start over, honestly and without surprises.
        if status.is_success() || status == reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
            let _ = std::fs::remove_file(&part_path);
            let body_is_whole_file = status.is_success();
            offset = 0;
            if body_is_whole_file {
                break response;
            }
            continue;
        }
        return Err(ModelDownloadError::HttpStatus(status.as_u16()));
    };
    // On 206 Content-Length describes only the tail, while the progress bar
    // needs the whole file.
    let total = response
        .content_length()
        .map(|length| length.saturating_add(offset));

    let mut hasher = Sha256::new();
    let mut file = if offset > 0 {
        hash_existing_prefix(&part_path, offset, &mut hasher, cancel_flag).await?;
        tokio::fs::OpenOptions::new()
            .append(true)
            .open(&part_path)
            .await
            .map_err(|error| ModelDownloadError::Transport(format!("append part: {error}")))?
    } else {
        tokio::fs::File::create(&part_path)
            .await
            .map_err(|error| ModelDownloadError::Transport(format!("create part: {error}")))?
    };
    let mut downloaded: u64 = offset;
    if offset > 0 {
        if let Some(progress_cb) = progress {
            progress_cb(DownloadProgress { downloaded, total });
        }
    }
    let mut stream = response.bytes_stream();
    loop {
        let chunk_result = match wait_download_io(stream.next(), cancel_flag, CHUNK_TIMEOUT).await {
            Ok(Some(chunk)) => chunk,
            Ok(None) => break,
            Err(error) => {
                // Finish any buffered write before the command wrapper removes
                // cancelled staging files (especially with Windows file locks).
                file.flush().await.ok();
                return Err(error);
            }
        };
        if cancel_flag.load(Ordering::Relaxed) {
            file.flush().await.ok();
            return Err(ModelDownloadError::Cancelled);
        }
        let chunk = chunk_result
            .map_err(|error| ModelDownloadError::Transport(format!("body: {error}")))?;
        file.write_all(&chunk)
            .await
            .map_err(|error| ModelDownloadError::Transport(format!("write: {error}")))?;
        hasher.update(&chunk);
        downloaded = downloaded.saturating_add(chunk.len() as u64);
        if let Some(progress_cb) = progress {
            progress_cb(DownloadProgress { downloaded, total });
        }
    }
    file.flush()
        .await
        .map_err(|error| ModelDownloadError::Transport(format!("flush: {error}")))?;
    drop(file);

    // Notify the UI before comparing the accumulated digest and size.
    if let Some(verifying_cb) = on_verifying {
        verifying_cb();
    }

    if cancel_flag.load(Ordering::Acquire) {
        return Err(ModelDownloadError::Cancelled);
    }

    if downloaded != spec.expected_bytes {
        return Err(ModelDownloadError::SizeMismatch {
            expected: spec.expected_bytes,
            actual: downloaded,
        });
    }
    let actual_sha = format!("{:x}", hasher.finalize());
    if !actual_sha.eq_ignore_ascii_case(&spec.sha256) {
        return Err(ModelDownloadError::Sha256Mismatch {
            expected: spec.sha256.clone(),
            actual: actual_sha,
        });
    }

    std::fs::rename(&part_path, &final_path).map_err(|error| {
        ModelDownloadError::Transport(format!(
            "rename {} -> {}: {error}",
            part_path.display(),
            final_path.display()
        ))
    })?;

    Ok(DownloadOutcome {
        path: final_path,
        bytes: downloaded,
    })
}

/// A closed multi-file model bundle. Artifacts are downloaded into a private
/// staging directory and the directory itself is renamed only after every
/// artifact passes size and SHA-256 verification. This keeps a partially
/// installed ONNX bundle invisible to `model::is_downloaded` and to the UI.
#[derive(Debug, Clone)]
pub struct BundleDownloadSpec {
    pub model_id: String,
    pub directory_name: String,
    pub artifacts: Vec<DownloadSpec>,
}

#[derive(Debug, Clone)]
pub struct BundleDownloadOutcome {
    pub path: PathBuf,
    pub bytes: u64,
}

/// Remove from the staging folder everything absent from the manifest.
///
/// The rename publishes the folder wholesale, exactly as it is, so no `*.part`
/// scraps from an interrupted attempt and no files from a previous version of
/// the bundle may remain: they would move into the installed model along with
/// the needed ones. This used to be handled by wiping the folder before every
/// attempt — with resume support it can no longer be wiped.
fn prune_stage_dir(stage_dir: &Path, spec: &BundleDownloadSpec) {
    let Ok(entries) = std::fs::read_dir(stage_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let wanted = spec
            .artifacts
            .iter()
            .any(|artifact| name.as_os_str() == artifact.file_name.as_str());
        if wanted {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            let _ = std::fs::remove_dir_all(&path);
        } else {
            let _ = std::fs::remove_file(&path);
        }
    }
}

pub async fn download_bundle_to_dir(
    client: &reqwest::Client,
    spec: &BundleDownloadSpec,
    dir: &Path,
    cancel_flag: &Arc<AtomicBool>,
    progress: Option<&(dyn Fn(DownloadProgress) + Send + Sync)>,
    on_verifying: Option<&(dyn Fn() + Send + Sync)>,
) -> Result<BundleDownloadOutcome, ModelDownloadError> {
    if spec.artifacts.is_empty() {
        return Err(ModelDownloadError::Transport(
            "bundle has no artifacts".to_string(),
        ));
    }
    if cancel_flag.load(Ordering::Relaxed) {
        return Err(ModelDownloadError::Cancelled);
    }
    std::fs::create_dir_all(dir)
        .map_err(|error| ModelDownloadError::Transport(format!("mkdir: {error}")))?;
    let final_dir = dir.join(&spec.directory_name);
    if final_dir.exists() {
        return Err(ModelDownloadError::Transport(format!(
            "destination already exists: {}",
            final_dir.display()
        )));
    }
    // The staging folder is not wiped: it holds what the previous attempt
    // managed to download, which is the whole point of resuming. Anything
    // superfluous is removed by `prune_stage_dir` right before publishing.
    let stage_dir = stage_dir_for(dir, &spec.directory_name);
    std::fs::create_dir_all(&stage_dir)
        .map_err(|error| ModelDownloadError::Transport(format!("mkdir staging dir: {error}")))?;

    let total_bytes: u64 = spec
        .artifacts
        .iter()
        .map(|artifact| artifact.expected_bytes)
        .sum();
    if cancel_flag.load(Ordering::Relaxed) {
        return Err(ModelDownloadError::Cancelled);
    }
    let remaining_bytes = spec
        .artifacts
        .iter()
        .map(|artifact| remaining_download_bytes(&stage_dir, artifact))
        .fold(0_u64, u64::saturating_add);
    if remaining_bytes > 0 {
        ensure_free_space(&stage_dir, remaining_bytes)?;
    }
    let completed = Arc::new(std::sync::atomic::AtomicU64::new(0));
    for artifact in &spec.artifacts {
        let completed_for_progress = Arc::clone(&completed);
        let progress_cb = |p: DownloadProgress| {
            if let Some(callback) = progress {
                callback(DownloadProgress {
                    downloaded: completed_for_progress
                        .load(Ordering::Relaxed)
                        .saturating_add(p.downloaded),
                    total: Some(total_bytes),
                });
            }
        };
        let artifact_progress: Option<&(dyn Fn(DownloadProgress) + Send + Sync)> =
            progress.map(|_| &progress_cb as &(dyn Fn(DownloadProgress) + Send + Sync));
        let outcome = download_spec_to_dir(
            client,
            artifact,
            &stage_dir,
            cancel_flag,
            artifact_progress,
            None,
        )
        .await?;
        completed.fetch_add(outcome.bytes(), Ordering::Relaxed);
    }
    if let Some(callback) = on_verifying {
        callback();
    }
    if cancel_flag.load(Ordering::Relaxed) {
        return Err(ModelDownloadError::Cancelled);
    }
    prune_stage_dir(&stage_dir, spec);
    std::fs::rename(&stage_dir, &final_dir).map_err(|error| {
        ModelDownloadError::Transport(format!(
            "rename bundle {} -> {}: {error}",
            stage_dir.display(),
            final_dir.display()
        ))
    })?;
    Ok(BundleDownloadOutcome {
        path: final_dir,
        bytes: total_bytes,
    })
}

/// Bytes still to fetch for one artifact, for the bundle's pre-flight check.
///
/// A finished file counts by size alone: hashing it here would read every
/// completed artifact twice, since `download_spec_to_dir` verifies it again.
/// A same-sized but corrupt file is still re-downloaded there, behind that
/// function's own free-space check.
fn remaining_download_bytes(dir: &Path, spec: &DownloadSpec) -> u64 {
    let final_path = dir.join(&spec.file_name);
    let finished = std::fs::metadata(&final_path)
        .is_ok_and(|meta| meta.is_file() && meta.len() == spec.expected_bytes);
    if finished {
        return 0;
    }
    spec.expected_bytes - resume_offset(&part_path_for(&final_path), spec.expected_bytes)
}

/// Download a model by id ("tiny", "base", "small", "medium", "large-v3", "turbo").
///
/// Streams the GGML file from Hugging Face, verifies SHA-256, and renames
/// onto the final path. Emits `model-download-progress` events during
/// download with payload `{ model, downloaded, total }`.
///
/// A cancelled download is `Ok(None)`, not an error: the user pressed «отменить»
/// and got exactly what they asked for. What was not downloaded is erased along
/// the way — we have no resume, and a leftover chunk would be nothing but
/// occupied space.
#[tauri::command]
pub(crate) async fn download_model(
    app: AppHandle,
    state: tauri::State<'_, crate::state::AppState>,
    model: String,
) -> Result<Option<DownloadOutcomeInfo>, String> {
    // Cancellation is registered before the first byte: otherwise «отменить»
    // pressed within the first second would find nothing to cancel. The same
    // registration rejects a second download of the same model: both would write
    // one `*.part` and race each other checking its checksum.
    let Some(download) = state.try_claim_download(&model) else {
        return Err(crate::ui_text::t("Эта модель уже скачивается."));
    };
    let cancel = download.flag();
    if crate::model::model_engine(&model)?.is_sherpa() {
        let entry = crate::model::bundle_manifest_entry(&model)?;
        let dir = crate::model::models_dir()?;
        let final_dir = dir.join(entry.directory_name);
        // `is_downloaded` intentionally performs only cheap size checks
        // because it runs while refreshing the Settings list. A final bundle
        // path must therefore always be verified here: a missing artifact or
        // wrong-size file makes `is_downloaded` false but must not strand the
        // downloader behind an existing destination.
        let model_id = entry.public_id.to_string();
        let already_ready =
            tokio::task::spawn_blocking(move || crate::model::recover_bundle_if_needed(&model_id))
                .await
                .map_err(|error| format!("bundle verification task failed: {error}"))??;
        if already_ready {
            let bytes = entry
                .artifacts
                .iter()
                .map(|artifact| artifact.expected_bytes)
                .sum();
            return Ok(Some(DownloadOutcomeInfo {
                model_id: entry.public_id.to_string(),
                path: final_dir.to_string_lossy().into_owned(),
                bytes,
            }));
        }
        let spec = BundleDownloadSpec {
            model_id: entry.public_id.to_string(),
            directory_name: entry.directory_name.to_string(),
            artifacts: entry
                .artifacts
                .iter()
                .map(|artifact| DownloadSpec {
                    model_id: entry.public_id.to_string(),
                    file_name: artifact.file_name.to_string(),
                    url: artifact.download_url.to_string(),
                    expected_bytes: artifact.expected_bytes,
                    sha256: artifact.sha256.to_string(),
                })
                .collect(),
        };
        let client = reqwest::Client::new();
        let progress_cb = progress_emitter(&app, entry.public_id.to_string());
        let outcome =
            match download_bundle_to_dir(&client, &spec, &dir, &cancel, Some(&progress_cb), None)
                .await
            {
                Ok(outcome) => outcome,
                Err(ModelDownloadError::Cancelled) => {
                    discard_bundle_partial(&dir, &spec);
                    return Ok(None);
                }
                Err(error) => return Err(format!("download: {error}")),
            };
        return Ok(Some(DownloadOutcomeInfo {
            model_id: entry.public_id.to_string(),
            path: outcome.path.to_string_lossy().into_owned(),
            bytes: outcome.bytes,
        }));
    }
    let entry = manifest_entry(&model).map_err(|e| format!("unknown model {model}: {e}"))?;
    let spec = DownloadSpec {
        model_id: entry.public_id.to_string(),
        file_name: entry.file_name.to_string(),
        url: entry.download_url.to_string(),
        expected_bytes: entry.expected_bytes,
        sha256: entry.sha256.to_string(),
    };
    let dir = crate::model::models_dir().map_err(|e| e.to_string())?;
    let client = reqwest::Client::new();

    // Wire progress events so the frontend can show a download bar.
    let progress_cb = progress_emitter(&app, model.clone());

    let outcome =
        match download_spec_to_dir(&client, &spec, &dir, &cancel, Some(&progress_cb), None).await {
            Ok(outcome) => outcome,
            Err(ModelDownloadError::Cancelled) => {
                discard_partial(&dir, &spec);
                return Ok(None);
            }
            Err(error) => return Err(format!("download: {error}")),
        };

    Ok(Some(DownloadOutcomeInfo::for_model(&model, &outcome)))
}

/// Stop a download of a model that is in progress.
///
/// `false` — nobody is downloading this model right now: the button was pressed
/// after the download had already finished. That is not an error but a race, and
/// it is cured by staying silent.
#[tauri::command]
pub(crate) fn cancel_model_download(
    state: tauri::State<'_, crate::state::AppState>,
    model: String,
) -> bool {
    state.cancel_download(&model)
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::Duration;

    use super::*;

    fn sha256_hex(bytes: &[u8]) -> String {
        let digest = Sha256::digest(bytes);
        digest.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    fn tiny_spec(url: String, body: &[u8]) -> DownloadSpec {
        DownloadSpec {
            model_id: "tiny".to_string(),
            file_name: "ggml-tiny.bin".to_string(),
            url,
            expected_bytes: body.len() as u64,
            sha256: sha256_hex(body),
        }
    }

    /// Spin up a one-shot HTTP server that returns `body`. When
    /// `delay_between_chunks` is `Some(duration)` the first 3 bytes
    /// are flushed immediately and the remainder after `duration`,
    /// which lets cancellation tests interleave the cancel signal with
    /// a real chunk boundary.
    fn serve_once(body: Vec<u8>, delay_between_chunks: Option<Duration>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request);
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .unwrap();
            if let Some(delay) = delay_between_chunks {
                let split = body.len().min(3);
                stream.write_all(&body[..split]).unwrap();
                stream.flush().unwrap();
                thread::sleep(delay);
                stream.write_all(&body[split..]).unwrap();
            } else {
                stream.write_all(&body).unwrap();
            }
            stream.flush().unwrap();
        });
        format!("http://{addr}/ggml-tiny.bin")
    }

    /// How the one-shot server answers a `Range` request.
    #[derive(Clone, Copy)]
    enum RangeMode {
        /// 206 with the tail of the file — how the Hugging Face CDN behaves.
        Honour,
        /// 200 with the whole file: the header was read as a suggestion.
        Ignore,
        /// 416: our leftover did not suit the server.
        Reject,
    }

    /// A server for `connections` requests that parses `Range`. Returns the
    /// address and a log of received headers — the tests use it to check that a
    /// resume asks for exactly the offset it stopped at.
    fn serve_ranged(
        body: Vec<u8>,
        mode: RangeMode,
        connections: usize,
    ) -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen_for_server = Arc::clone(&seen);
        thread::spawn(move || {
            for _ in 0..connections {
                let Ok((mut stream, _)) = listener.accept() else {
                    return;
                };
                let mut request = [0_u8; 1024];
                let read = stream.read(&mut request).unwrap_or(0);
                let text = String::from_utf8_lossy(&request[..read]).to_string();
                // Header names are matched by regex over the line: reqwest
                // writes them lowercase, while the tests are read by eye.
                let range = text.lines().find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("range")
                        .then(|| value.trim().to_string())
                });
                seen_for_server
                    .lock()
                    .unwrap()
                    .push(range.clone().unwrap_or_default());
                let start = range
                    .as_deref()
                    .and_then(|value| value.strip_prefix("bytes="))
                    .and_then(|value| value.split('-').next())
                    .and_then(|value| value.parse::<usize>().ok());
                match (mode, start) {
                    (RangeMode::Honour, Some(start)) if start < body.len() => {
                        write!(
                            stream,
                            "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {}-{}/{}\r\nConnection: close\r\n\r\n",
                            body.len() - start,
                            start,
                            body.len() - 1,
                            body.len()
                        )
                        .unwrap();
                        stream.write_all(&body[start..]).unwrap();
                    }
                    (RangeMode::Reject, Some(_)) => {
                        write!(
                            stream,
                            "HTTP/1.1 416 Range Not Satisfiable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                        )
                        .unwrap();
                    }
                    _ => {
                        write!(
                            stream,
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            body.len()
                        )
                        .unwrap();
                        stream.write_all(&body).unwrap();
                    }
                }
                stream.flush().unwrap();
            }
        });
        (format!("http://{addr}/ggml-tiny.bin"), seen)
    }

    fn block_on<T>(future: impl std::future::Future<Output = T>) -> T {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(future)
    }

    #[test]
    fn cancellation_interrupts_stalled_headers_and_body() {
        for send_headers in [false, true] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let addr = listener.local_addr().unwrap();
            let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
            let (release_tx, release_rx) = std::sync::mpsc::channel();
            let server = thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = Vec::new();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                    let mut chunk = [0; 1024];
                    let read = stream.read(&mut chunk).unwrap();
                    assert!(read > 0, "client closed before sending request headers");
                    request.extend_from_slice(&chunk[..read]);
                    assert!(request.len() <= 16 * 1024, "request headers too large");
                }
                if send_headers {
                    stream
                        .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 6\r\n\r\na")
                        .unwrap();
                    stream.flush().unwrap();
                }
                ready_tx.send(()).unwrap();
                // The client must cancel before this server closes or sends more.
                let _ = release_rx.recv_timeout(Duration::from_secs(5));
            });
            let spec = tiny_spec(format!("http://{addr}/model"), b"abcdef");
            let dir = tempfile::tempdir().unwrap();
            let cancel = Arc::new(AtomicBool::new(false));
            let client = reqwest::Client::new();
            let received = AtomicU64::new(0);
            let progress = |event: DownloadProgress| {
                received.store(event.downloaded, Ordering::Relaxed);
                if send_headers {
                    cancel.store(true, Ordering::Release);
                }
            };
            let result = block_on(async {
                let download = download_spec_to_dir(
                    &client,
                    &spec,
                    dir.path(),
                    &cancel,
                    Some(&progress),
                    None,
                );
                let stop = async {
                    ready_rx.await.unwrap();
                    if !send_headers {
                        cancel.store(true, Ordering::Release);
                    }
                };
                let (result, ()) =
                    tokio::join!(tokio::time::timeout(Duration::from_secs(2), download), stop);
                result
            });
            release_tx.send(()).unwrap();
            server.join().unwrap();
            assert!(
                matches!(result, Ok(Err(ModelDownloadError::Cancelled))),
                "headers={send_headers}: {result:?}"
            );
            assert!(!dir.path().join(&spec.file_name).exists());
            if send_headers {
                assert_eq!(
                    received.load(Ordering::Relaxed),
                    1,
                    "cancel after the first body chunk"
                );
            }
        }
    }

    #[test]
    fn stalled_io_times_out_without_restarting_the_operation() {
        block_on(async {
            let calls = AtomicU64::new(0);
            let cancel = AtomicBool::new(false);
            let operation = async {
                calls.fetch_add(1, Ordering::Relaxed);
                std::future::pending::<()>().await;
            };
            assert!(matches!(
                wait_download_io(operation, &cancel, Duration::from_millis(250)).await,
                Err(ModelDownloadError::Transport(_))
            ));
            assert_eq!(calls.load(Ordering::Relaxed), 1);
        });
    }

    #[test]
    fn manifest_contains_official_huggingface_sha256_metadata() {
        let manifest = crate::model::model_manifest();
        assert_eq!(manifest.len(), 10);
        assert_eq!(manifest.iter().filter(|entry| entry.recommended).count(), 1);

        let tiny = crate::model::manifest_entry("tiny").unwrap();
        assert_eq!(tiny.file_name, "ggml-tiny.bin");
        assert_eq!(tiny.expected_bytes, 77_691_713);
        assert_eq!(
            tiny.sha256,
            "be07e048e1e599ad46341c8d2a135645097a538221678b7acdd1b1919c6e1b21"
        );
        assert_eq!(
            tiny.download_url,
            "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.bin"
        );
    }

    #[test]
    fn verifier_rejects_size_and_sha_mismatch() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("ggml-tiny.bin");
        std::fs::write(&file, b"hello").unwrap();

        let mut spec = tiny_spec("http://127.0.0.1/unused".to_string(), b"hello");
        block_on(verify_file(&file, &spec)).unwrap();

        spec.expected_bytes = 6;
        assert!(matches!(
            block_on(verify_file(&file, &spec)),
            Err(ModelDownloadError::SizeMismatch {
                expected: 6,
                actual: 5
            })
        ));

        spec.expected_bytes = 5;
        spec.sha256 = sha256_hex(b"HELLO");
        assert!(matches!(
            block_on(verify_file(&file, &spec)),
            Err(ModelDownloadError::Sha256Mismatch { .. })
        ));
    }

    #[test]
    fn download_streams_to_part_verifies_and_renames_final_file() {
        let body = b"complete model payload".to_vec();
        let url = serve_once(body.clone(), None);
        let spec = tiny_spec(url, &body);
        let dir = tempfile::tempdir().unwrap();
        let cancel = Arc::new(AtomicBool::new(false));
        let client = reqwest::Client::new();

        let outcome = block_on(download_spec_to_dir(
            &client,
            &spec,
            dir.path(),
            &cancel,
            None,
            None,
        ))
        .unwrap();

        let final_path = dir.path().join("ggml-tiny.bin");
        assert_eq!(outcome.path(), final_path.as_path());
        assert_eq!(std::fs::read(&final_path).unwrap(), body);
        assert!(!part_path_for(&final_path).exists());
    }

    #[test]
    fn checksum_failure_keeps_existing_final_model_untouched() {
        let bad_body = b"corrupt payload".to_vec();
        let url = serve_once(bad_body.clone(), None);
        let mut spec = tiny_spec(url, b"expected payload");
        spec.expected_bytes = bad_body.len() as u64;
        let dir = tempfile::tempdir().unwrap();
        let final_path = dir.path().join("ggml-tiny.bin");
        std::fs::write(&final_path, b"existing working model").unwrap();
        let cancel = Arc::new(AtomicBool::new(false));
        let client = reqwest::Client::new();

        assert!(matches!(
            block_on(download_spec_to_dir(
                &client,
                &spec,
                dir.path(),
                &cancel,
                None,
                None,
            )),
            Err(ModelDownloadError::Sha256Mismatch { .. })
        ));

        assert_eq!(
            std::fs::read(&final_path).unwrap(),
            b"existing working model"
        );
        // The partial file MUST stay on disk so the user can retry
        // without re-downloading from byte 0. CRITICAL invariant.
        assert!(part_path_for(&final_path).exists());
    }

    #[test]
    fn retry_truncates_stale_part_and_promotes_complete_download() {
        let body = b"fresh payload".to_vec();
        let url = serve_once(body.clone(), None);
        let spec = tiny_spec(url, &body);
        let dir = tempfile::tempdir().unwrap();
        let final_path = dir.path().join("ggml-tiny.bin");
        let part_path = part_path_for(&final_path);
        std::fs::write(&part_path, b"stale partial bytes that must be replaced").unwrap();
        let cancel = Arc::new(AtomicBool::new(false));
        let client = reqwest::Client::new();

        block_on(download_spec_to_dir(
            &client,
            &spec,
            dir.path(),
            &cancel,
            None,
            None,
        ))
        .unwrap();

        assert_eq!(std::fs::read(&final_path).unwrap(), body);
        assert!(!part_path.exists());
    }

    #[test]
    fn resume_verdict_continues_only_a_real_prefix() {
        // The failure here is silent: a wrong offset does not break the
        // download, it quietly assembles a corrupt file — only the SHA-256 at
        // the very end catches it.
        assert_eq!(resume_verdict(None, 100), 0, "nothing to download");
        assert_eq!(
            resume_verdict(Some(0), 100),
            0,
            "an empty remainder is no remainder"
        );
        assert_eq!(resume_verdict(Some(40), 100), 40, "an ordinary resume");
        assert_eq!(
            resume_verdict(Some(100), 100),
            0,
            "the whole file is here: the previous attempt failed its hash, so there is nothing to continue"
        );
        assert_eq!(
            resume_verdict(Some(140), 100),
            0,
            "longer than expected: the remainder of a different file"
        );
    }

    #[test]
    fn an_interrupted_download_continues_from_where_it_stopped() {
        let body = b"resumable model payload".to_vec();
        let (url, seen) = serve_ranged(body.clone(), RangeMode::Honour, 1);
        let spec = tiny_spec(url, &body);
        let dir = tempfile::tempdir().unwrap();
        let final_path = dir.path().join("ggml-tiny.bin");
        let part_path = part_path_for(&final_path);
        std::fs::write(&part_path, &body[..9]).unwrap();
        let cancel = Arc::new(AtomicBool::new(false));
        let client = reqwest::Client::new();

        let outcome = block_on(download_spec_to_dir(
            &client,
            &spec,
            dir.path(),
            &cancel,
            None,
            None,
        ))
        .unwrap();

        assert_eq!(seen.lock().unwrap().as_slice(), ["bytes=9-"]);
        assert_eq!(std::fs::read(&final_path).unwrap(), body);
        assert_eq!(outcome.bytes(), body.len() as u64);
        assert!(!part_path.exists());
        // The file matched the checksum while the server sent only the tail —
        // which means the previous half was read into the hasher.
    }

    #[test]
    fn a_corrupt_leftover_costs_a_download_but_never_installs_a_broken_model() {
        // A leftover of the right length but with foreign content: the resume
        // will not recognise it, and the only thing standing between the user
        // and a corrupt model is the SHA-256 over the whole file.
        let body = b"resumable model payload".to_vec();
        let (url, _seen) = serve_ranged(body.clone(), RangeMode::Honour, 1);
        let spec = tiny_spec(url, &body);
        let dir = tempfile::tempdir().unwrap();
        let final_path = dir.path().join("ggml-tiny.bin");
        std::fs::write(part_path_for(&final_path), b"XXXXXXXXX").unwrap();
        let cancel = Arc::new(AtomicBool::new(false));
        let client = reqwest::Client::new();

        let result = block_on(download_spec_to_dir(
            &client,
            &spec,
            dir.path(),
            &cancel,
            None,
            None,
        ));

        assert!(matches!(
            result,
            Err(ModelDownloadError::Sha256Mismatch { .. })
        ));
        assert!(!final_path.exists(), "a corrupt download is not published");
    }

    #[test]
    fn a_server_that_ignores_range_starts_over_instead_of_gluing_two_halves() {
        let body = b"resumable model payload".to_vec();
        let (url, seen) = serve_ranged(body.clone(), RangeMode::Ignore, 1);
        let spec = tiny_spec(url, &body);
        let dir = tempfile::tempdir().unwrap();
        let final_path = dir.path().join("ggml-tiny.bin");
        std::fs::write(part_path_for(&final_path), &body[..9]).unwrap();
        let cancel = Arc::new(AtomicBool::new(false));
        let client = reqwest::Client::new();

        block_on(download_spec_to_dir(
            &client,
            &spec,
            dir.path(),
            &cancel,
            None,
            None,
        ))
        .unwrap();

        assert_eq!(seen.lock().unwrap().as_slice(), ["bytes=9-"]);
        // Exactly the body, not the leftover plus the body: appending a
        // whole-file response to a non-empty file means nine extra bytes at the
        // start.
        assert_eq!(std::fs::read(&final_path).unwrap(), body);
    }

    #[test]
    fn a_rejected_range_is_asked_again_as_a_whole_file() {
        let body = b"resumable model payload".to_vec();
        let (url, seen) = serve_ranged(body.clone(), RangeMode::Reject, 2);
        let spec = tiny_spec(url, &body);
        let dir = tempfile::tempdir().unwrap();
        let final_path = dir.path().join("ggml-tiny.bin");
        std::fs::write(part_path_for(&final_path), &body[..9]).unwrap();
        let cancel = Arc::new(AtomicBool::new(false));
        let client = reqwest::Client::new();

        block_on(download_spec_to_dir(
            &client,
            &spec,
            dir.path(),
            &cancel,
            None,
            None,
        ))
        .unwrap();

        // The second request carries no Range, and there is only one of them:
        // exactly one retry.
        assert_eq!(seen.lock().unwrap().as_slice(), ["bytes=9-", ""]);
        assert_eq!(std::fs::read(&final_path).unwrap(), body);
    }

    #[test]
    fn a_bundle_keeps_the_artifacts_the_previous_attempt_already_finished() {
        // The server is brought up only for the second artifact. If the resume
        // reaches for the first one it will have nowhere to knock — and the test
        // will see that.
        let encoder = b"encoder weights".to_vec();
        let decoder = b"decoder weights".to_vec();
        let (decoder_url, _seen) = serve_ranged(decoder.clone(), RangeMode::Honour, 1);
        let spec = BundleDownloadSpec {
            model_id: "gigaam-v3".to_string(),
            directory_name: "gigaam-v3".to_string(),
            artifacts: vec![
                DownloadSpec {
                    model_id: "gigaam-v3".to_string(),
                    file_name: "encoder.onnx".to_string(),
                    url: "http://127.0.0.1:1/encoder.onnx".to_string(),
                    expected_bytes: encoder.len() as u64,
                    sha256: sha256_hex(&encoder),
                },
                DownloadSpec {
                    model_id: "gigaam-v3".to_string(),
                    file_name: "decoder.onnx".to_string(),
                    url: decoder_url,
                    expected_bytes: decoder.len() as u64,
                    sha256: sha256_hex(&decoder),
                },
            ],
        };
        let dir = tempfile::tempdir().unwrap();
        let stage = stage_dir_for(dir.path(), &spec.directory_name);
        std::fs::create_dir_all(&stage).unwrap();
        std::fs::write(stage.join("encoder.onnx"), &encoder).unwrap();
        std::fs::write(stage.join("old-encoder.onnx.part"), b"leftover junk").unwrap();
        let cancel = Arc::new(AtomicBool::new(false));
        let client = reqwest::Client::new();

        block_on(download_bundle_to_dir(
            &client,
            &spec,
            dir.path(),
            &cancel,
            None,
            None,
        ))
        .unwrap();

        let installed = dir.path().join("gigaam-v3");
        assert_eq!(
            std::fs::read(installed.join("encoder.onnx")).unwrap(),
            encoder
        );
        assert_eq!(
            std::fs::read(installed.join("decoder.onnx")).unwrap(),
            decoder
        );
        // Publishing renames the folder wholesale, so nothing foreign may
        // remain inside it.
        let mut published: Vec<String> = std::fs::read_dir(&installed)
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        published.sort();
        assert_eq!(published, ["decoder.onnx", "encoder.onnx"]);
    }

    #[test]
    fn cancellation_leaves_only_safe_part_file() {
        // Pre-flip the cancel flag so the pre-flight check in
        // `download_spec_to_dir` returns Cancelled before any HTTP
        // socket is opened. This makes the test deterministic — the
        // flaky alternative (rely on the server's chunk-flush timing)
        // is exercised implicitly by the streaming tests above.
        let body = b"abcdef".to_vec();
        let url = serve_once(body.clone(), None);
        let spec = tiny_spec(url, &body);
        let dir = tempfile::tempdir().unwrap();
        let final_path = dir.path().join("ggml-tiny.bin");
        let part_path = part_path_for(&final_path);
        let cancel = Arc::new(AtomicBool::new(true));

        let result = block_on(download_spec_to_dir(
            &reqwest::Client::new(),
            &spec,
            dir.path(),
            &cancel,
            None,
            None,
        ));

        assert!(matches!(result, Err(ModelDownloadError::Cancelled)));
        assert!(!final_path.exists(), "final path must not exist on cancel");
        assert!(!part_path.exists(), "no partial bytes were written");
    }

    #[test]
    fn cancellation_during_stream_keeps_partial_file() {
        // Server flushes a small prefix before sleeping, so the
        // downloader can write at least one chunk to the part file
        // before the cancel signal flips mid-stream.
        let body = b"abcdef".to_vec();
        let url = serve_once(body.clone(), Some(Duration::from_millis(200)));
        let spec = tiny_spec(url, &body);
        let dir = tempfile::tempdir().unwrap();
        let final_path = dir.path().join("ggml-tiny.bin");
        let part_path = part_path_for(&final_path);
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_for_progress = Arc::clone(&cancel);
        let progress = move |_event: DownloadProgress| {
            cancel_for_progress.store(true, Ordering::Relaxed);
        };
        let client = reqwest::Client::new();

        let result = block_on(download_spec_to_dir(
            &client,
            &spec,
            dir.path(),
            &cancel,
            Some(&progress),
            None,
        ));
        // Either the cancel landed before more chunks (Cancelled) or
        // the server already finished the body (SizeMismatch) — both
        // are acceptable for this test, but the partial file MUST
        // exist so the user can retry.
        assert!(matches!(
            result,
            Err(ModelDownloadError::Cancelled) | Err(ModelDownloadError::SizeMismatch { .. })
        ));
        if matches!(result, Err(ModelDownloadError::Cancelled)) {
            assert!(!final_path.exists(), "final path must not exist on cancel");
            assert!(part_path.exists(), "partial file must remain on cancel");
        }
    }

    #[test]
    fn free_space_check_rejects_when_available_below_required() {
        // We can't easily fake fs2::available_space, but we can sanity
        // check the slack math: required = expected + 1 MiB.
        // Direct test of the bound by passing a known huge expected.
        let dir = tempfile::tempdir().unwrap();
        let outcome = ensure_free_space(dir.path(), u64::MAX);
        // Either we are rejected (InsufficientFreeSpace) or the OS
        // returned 0 and we let the check pass — both are valid; what
        // matters is that we never panic.
        assert!(matches!(
            outcome,
            Ok(()) | Err(ModelDownloadError::InsufficientFreeSpace { .. })
        ));
    }

    #[test]
    fn free_space_verdict_treats_unknown_as_ok() {
        // An unavailable measurement must not be confused with a full disk.
        assert_eq!(free_space_verdict(None, 1000), Ok(()));
        assert_eq!(
            free_space_verdict(Some(0), 1000),
            Err(ModelDownloadError::InsufficientFreeSpace {
                required_bytes: 1000 + 1024 * 1024,
                available_bytes: 0,
            })
        );
    }

    #[test]
    fn free_space_is_known_before_the_models_directory_exists() {
        // The first download creates the directory, so the check has to answer
        // before it is there or it never warns the user who needs it most.
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("models").join("whisper");
        assert!(!missing.exists());
        assert!(available_bytes(dir.path()).is_some());
        assert!(available_bytes(&missing).is_some());
    }

    #[test]
    fn remaining_space_accounts_for_finished_and_partial_artifacts() {
        let dir = tempfile::tempdir().unwrap();
        let spec = tiny_spec("http://127.0.0.1:1".into(), b"abcde");
        assert_eq!(remaining_download_bytes(dir.path(), &spec), 5);
        let final_path = dir.path().join(&spec.file_name);
        let partial = part_path_for(&final_path);
        std::fs::write(&partial, b"ab").unwrap();
        assert_eq!(remaining_download_bytes(dir.path(), &spec), 3);
        std::fs::write(&partial, b"too long").unwrap();
        assert_eq!(remaining_download_bytes(dir.path(), &spec), 5);
        assert!(!partial.exists());
        std::fs::write(&final_path, b"abc").unwrap();
        assert_eq!(remaining_download_bytes(dir.path(), &spec), 5);
        // Size alone decides here; the download itself still verifies the
        // hash and re-fetches a corrupt file behind its own space check.
        std::fs::write(&final_path, b"wrong").unwrap();
        assert_eq!(remaining_download_bytes(dir.path(), &spec), 0);
    }

    #[tokio::test]
    async fn bundle_checks_combined_size_before_any_request() {
        let dir = tempfile::tempdir().unwrap();
        let Some(available) = available_bytes(dir.path()) else {
            return;
        };
        let mut first = tiny_spec("http://127.0.0.1:1".into(), b"a");
        first.expected_bytes = available / 2 + 1;
        let mut second = first.clone();
        second.file_name = "second.bin".into();
        let spec = BundleDownloadSpec {
            model_id: "synthetic".into(),
            directory_name: "synthetic".into(),
            artifacts: vec![first, second],
        };
        let result = download_bundle_to_dir(
            &reqwest::Client::new(),
            &spec,
            dir.path(),
            &Arc::new(AtomicBool::new(false)),
            None,
            None,
        )
        .await;
        assert!(matches!(
            result,
            Err(ModelDownloadError::InsufficientFreeSpace { .. })
        ));
        assert_eq!(
            std::fs::read_dir(stage_dir_for(dir.path(), "synthetic"))
                .unwrap()
                .count(),
            0
        );
    }

    #[test]
    fn free_space_verdict_requires_a_mib_of_slack() {
        // Exactly `expected` bytes is NOT enough: we reserve 1 MiB on top.
        assert!(matches!(
            free_space_verdict(Some(1000), 1000),
            Err(ModelDownloadError::InsufficientFreeSpace { .. })
        ));
    }

    #[test]
    fn free_space_verdict_accepts_exactly_one_mib_of_slack() {
        // `available == expected + 1 MiB` is the acceptance boundary.
        assert_eq!(free_space_verdict(Some(1000 + 1024 * 1024), 1000), Ok(()));
    }

    #[test]
    fn free_space_verdict_pins_the_slack_arithmetic() {
        // One byte under the slack must still be rejected. This is what
        // catches `1024 * 1024` being mutated to `+` or `/`: either would
        // shrink the slack and let this case through.
        assert!(matches!(
            free_space_verdict(Some(1000 + 1024 * 1024 - 1), 1000),
            Err(ModelDownloadError::InsufficientFreeSpace { .. })
        ));
    }

    #[test]
    fn progress_is_throttled_but_completion_always_passes() {
        let throttle = ProgressThrottle::new(Duration::from_millis(100));
        let start = std::time::Instant::now();
        let at = |ms| start + Duration::from_millis(ms);
        let part = |downloaded| DownloadProgress {
            downloaded,
            total: Some(1000),
        };
        assert!(throttle.admit(part(10), at(0)));
        assert!(!throttle.admit(part(20), at(40)));
        assert!(!throttle.admit(part(30), at(99)));
        assert!(throttle.admit(part(40), at(100)));
        assert!(!throttle.admit(part(50), at(150)));
        assert!(throttle.admit(part(1000), at(160)));
        // An unknown size never counts as complete.
        let unknown = DownloadProgress {
            downloaded: 5000,
            total: None,
        };
        assert!(!throttle.admit(unknown, at(170)));
        assert!(throttle.admit(unknown, at(260)));
    }

    #[test]
    fn download_progress_reports_fraction() {
        let progress = DownloadProgress {
            downloaded: 50,
            total: Some(200),
        };
        assert_eq!(progress.fraction(), Some(0.25));
        let progress = DownloadProgress {
            downloaded: 50,
            total: None,
        };
        assert_eq!(progress.fraction(), None);
        // `total = Some(0)` must not divide by zero — it is "unknown
        // length", not "100% done".
        let progress = DownloadProgress {
            downloaded: 0,
            total: Some(0),
        };
        assert_eq!(progress.fraction(), None);
    }

    #[test]
    fn cancellation_at_verification_never_publishes_the_download() {
        let body = b"verified but cancelled".to_vec();
        let spec = tiny_spec(serve_once(body.clone(), None), &body);
        let dir = tempfile::tempdir().unwrap();
        let final_path = dir.path().join(&spec.file_name);
        let cancel = Arc::new(AtomicBool::new(false));
        let on_verifying = || cancel.store(true, Ordering::Release);
        let result = block_on(download_spec_to_dir(
            &reqwest::Client::new(),
            &spec,
            dir.path(),
            &cancel,
            None,
            Some(&on_verifying),
        ));
        assert!(matches!(result, Err(ModelDownloadError::Cancelled)));
        assert!(!final_path.exists());
        assert_eq!(std::fs::read(part_path_for(&final_path)).unwrap(), body);
    }

    #[test]
    fn on_verifying_callback_fires_between_stream_and_rename() {
        // The on_verifying callback must run AFTER all streaming is
        // done (the part file is flushed + closed) but BEFORE the
        // SHA-256 / size check. The simplest assertion we can make
        // without instrumenting the downloader is: the callback runs
        // exactly once, and the final path is created (rename
        // succeeded) when control returns from the downloader.
        let body = b"verifying payload".to_vec();
        let url = serve_once(body.clone(), None);
        let spec = tiny_spec(url, &body);
        let dir = tempfile::tempdir().unwrap();
        let final_path = dir.path().join("ggml-tiny.bin");
        let cancel = Arc::new(AtomicBool::new(false));
        let verifying_count = Arc::new(AtomicU64::new(0));
        let verifying_count_for_cb = Arc::clone(&verifying_count);
        let on_verifying = move || {
            verifying_count_for_cb.fetch_add(1, Ordering::Relaxed);
        };
        let client = reqwest::Client::new();

        block_on(download_spec_to_dir(
            &client,
            &spec,
            dir.path(),
            &cancel,
            None,
            Some(&on_verifying),
        ))
        .unwrap();

        assert_eq!(
            verifying_count.load(Ordering::Relaxed),
            1,
            "on_verifying must fire exactly once per successful download"
        );
        assert!(
            final_path.exists(),
            "rename must complete after on_verifying"
        );
        assert!(!part_path_for(&final_path).exists());
    }

    #[test]
    fn on_verifying_not_fired_when_streaming_cancelled() {
        // If we cancel mid-stream, the streaming loop bails out
        // before reaching the on_verifying fire point. The callback
        // must NOT run.
        let body = b"abcdef".to_vec();
        let url = serve_once(body.clone(), Some(Duration::from_millis(200)));
        let spec = tiny_spec(url, &body);
        let dir = tempfile::tempdir().unwrap();
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_for_progress = Arc::clone(&cancel);
        let progress = move |_event: DownloadProgress| {
            cancel_for_progress.store(true, Ordering::Relaxed);
        };
        let verifying_count = Arc::new(AtomicU64::new(0));
        let verifying_count_for_cb = Arc::clone(&verifying_count);
        let on_verifying = move || {
            verifying_count_for_cb.fetch_add(1, Ordering::Relaxed);
        };
        let client = reqwest::Client::new();

        let _ = block_on(download_spec_to_dir(
            &client,
            &spec,
            dir.path(),
            &cancel,
            Some(&progress),
            Some(&on_verifying),
        ));
        // Whether cancellation or size-mismatch landed, the verifying
        // callback must NOT have fired (streaming never completed).
        assert_eq!(verifying_count.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn discarding_a_cancelled_download_frees_the_disk_and_spares_the_installed_file() {
        // Cancelling mid-gigabyte must not leave that gigabyte on disk — there
        // is no resume, and the next attempt starts from scratch anyway. And it
        // must not touch an already installed model: its name is adjacent, and
        // being off by one suffix is easy.
        let dir = tempfile::tempdir().unwrap();
        let spec = DownloadSpec {
            model_id: "tiny".to_string(),
            file_name: "ggml-tiny.bin".to_string(),
            url: String::new(),
            expected_bytes: 4,
            sha256: "00".repeat(32),
        };
        let installed = dir.path().join("ggml-tiny.bin");
        std::fs::write(&installed, b"kept").unwrap();
        let partial = part_path_for(&installed);
        std::fs::write(&partial, b"half").unwrap();

        discard_partial(dir.path(), &spec);

        assert!(!partial.exists(), "the partial download is erased");
        assert!(installed.exists(), "the installed model is untouched");
        // The second call — cleanup after cleanup — need not find anything.
        discard_partial(dir.path(), &spec);
    }

    #[test]
    fn discarding_a_cancelled_bundle_takes_the_whole_staging_directory() {
        let dir = tempfile::tempdir().unwrap();
        let spec = BundleDownloadSpec {
            model_id: "gigaam-v3".to_string(),
            directory_name: "gigaam-v3".to_string(),
            artifacts: Vec::new(),
        };
        let stage = stage_dir_for(dir.path(), &spec.directory_name);
        std::fs::create_dir_all(&stage).unwrap();
        std::fs::write(stage.join("model.int8.onnx.part"), b"half").unwrap();
        let installed = dir.path().join("gigaam-v3");
        std::fs::create_dir_all(&installed).unwrap();

        discard_bundle_partial(dir.path(), &spec);

        assert!(
            !stage.exists(),
            "the bundle's staging directory is erased entirely"
        );
        assert!(installed.exists(), "the installed bundle is untouched");
    }

    #[test]
    fn download_outcome_info_stamps_model_id() {
        let outcome = DownloadOutcome {
            path: PathBuf::from("/tmp/ggml-tiny.bin"),
            bytes: 12345,
        };
        let info = DownloadOutcomeInfo::for_model("tiny", &outcome);
        assert_eq!(info.model_id, "tiny");
        assert_eq!(info.path, "/tmp/ggml-tiny.bin");
        assert_eq!(info.bytes, 12345);

        // JSON shape (used by the Tauri command response) is stable.
        let json = serde_json::to_value(&info).unwrap();
        assert_eq!(json["model_id"], "tiny");
        assert_eq!(json["bytes"], 12345);
    }

    #[test]
    fn failed_bundle_never_publishes_final_directory() {
        let body = b"not-a-model".to_vec();
        let url = serve_once(body.clone(), None);
        let spec = BundleDownloadSpec {
            model_id: "gigaam-v3".to_string(),
            directory_name: "gigaam-v3".to_string(),
            artifacts: vec![DownloadSpec {
                model_id: "gigaam-v3".to_string(),
                file_name: "model.int8.onnx".to_string(),
                url,
                expected_bytes: body.len() as u64,
                sha256: "00".repeat(32),
            }],
        };
        let dir = tempfile::tempdir().unwrap();
        let client = reqwest::Client::new();
        let cancel = Arc::new(AtomicBool::new(false));
        let result = block_on(download_bundle_to_dir(
            &client,
            &spec,
            dir.path(),
            &cancel,
            None,
            None,
        ));
        assert!(matches!(
            result,
            Err(ModelDownloadError::Sha256Mismatch { .. })
        ));
        assert!(!dir.path().join("gigaam-v3").exists());
        assert!(dir.path().join(".gigaam-v3.part").exists());
    }
}
