//! Phase 4 / Batch 1 / PR 1.1 — GGML model downloader and verifier.
//!
//! The downloader streams an HTTP response into a `*.part` sibling, then
//! re-hashes the file in one pass and renames it onto the final path only
//! when the size and SHA-256 match the manifest entry. The pipeline is
//! self-contained: it owns the HTTP client, the disk I/O, and the
//! cancellation signal — there is no Python or sidecar round-trip.
//!
//! PR 1.1 deliberately stops at the Tauri-command boundary. A separate PR
//! (1.2) wraps `download_spec_to_dir` in a Tauri command + events; PR 1.3
//! wires Settings to the new commands. The work in this file is only the
//! trusted, unit-tested, no-IPC core that PR 1.2 will build on.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use futures_util::stream::StreamExt;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use crate::model::{manifest_entry, ModelManifestEntry};

/// Errors surfaced by the downloader. The variants are deliberately
/// `Clone + PartialEq` so the Tauri command layer (PR 1.2) can pattern
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
/// command layer (PR 1.2) looks entries up via
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
    /// `DownloadSpec`. Centralizes the Tauri-side lookup so PR 1.2
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
/// (PR 1.2) translates each into a `model-download-progress` event with
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

    /// Serialize the outcome for the Tauri IPC boundary. The Rust
    /// worker thread returns this to the command layer in PR 1.2
    /// so the frontend can show "downloaded 1.6 GB → /path/to/..."
    /// and trigger a Settings refresh.
    pub fn to_info(&self) -> DownloadOutcomeInfo {
        DownloadOutcomeInfo {
            model_id: String::new(),
            path: self.path.to_string_lossy().into_owned(),
            bytes: self.bytes,
        }
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
/// Called after a cancel: we have no resume here, every attempt starts from
/// scratch, and an unfinished gigabyte on disk is simply space the user never
/// asked to give up. Deletion errors are swallowed: the file may already be
/// gone, and there is no point failing while cleaning up after a cancel.
pub fn discard_partial(dir: &Path, spec: &DownloadSpec) {
    let _ = std::fs::remove_file(part_path_for(&dir.join(&spec.file_name)));
}

/// The same for a bundle: its unfinished download is a whole folder.
pub fn discard_bundle_partial(dir: &Path, spec: &BundleDownloadSpec) {
    let _ = std::fs::remove_dir_all(stage_dir_for(dir, &spec.directory_name));
}

/// Synchronously verify that `path` matches `spec`. The size and the
/// SHA-256 hash are both checked before returning `Ok(())`. The check
/// is intentionally `async` so PR 1.2 can call it from inside a Tokio
/// worker without blocking; the body streams the file in 64 KiB chunks
/// so a 1.6 GB model never has to live in memory all at once.
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

/// Best-effort pre-flight check for free space. Returns the number of
/// bytes available on the filesystem that contains `dir`. `0` means
/// "the OS does not report free space" — the caller should treat that
/// as "skip the check" rather than fail the download.
pub fn available_bytes(dir: &Path) -> u64 {
    fs2::available_space(dir).unwrap_or(0)
}

/// Check free space for the upcoming download. We require
/// `expected_bytes + 1 MiB` of slack (final write + atomic rename
/// bookkeeping); if the platform does not expose `available_space`
/// (returns 0) we treat it as unknown and let the download proceed.
pub fn ensure_free_space(dir: &Path, expected_bytes: u64) -> Result<(), ModelDownloadError> {
    free_space_verdict(available_bytes(dir), expected_bytes)
}

/// Pure verdict for the pre-flight free-space check, split out so the
/// boundary arithmetic is testable without a real filesystem.
///
/// `available == 0` means "the OS does not report free space" and is
/// treated as unknown, not as an empty disk.
fn free_space_verdict(available: u64, expected_bytes: u64) -> Result<(), ModelDownloadError> {
    let slack: u64 = 1024 * 1024;
    let required = expected_bytes.saturating_add(slack);
    if available == 0 || available >= required {
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

/// Download `spec` into `dir`, verifying on completion.
///
/// Steps:
/// 1. Run `ensure_free_space`. If the check returns
///    `InsufficientFreeSpace`, abort before opening any socket.
/// 2. Download the remainder if a usable `*.part` is left from the previous
///    attempt and the server agrees to `Range` (see `resume_verdict`).
/// 3. Stream the response body to `*.part`, periodically checking
///    `cancel_flag` and forwarding byte counts to `progress`.
/// 4. After the stream completes, fire `on_verifying` (PR 1.2) so
///    the UI can flip to a "verifying" spinner before the SHA-256
///    check starts.
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
    ensure_free_space(dir, spec.expected_bytes)?;

    let final_path = dir.join(&spec.file_name);
    let part_path = part_path_for(&final_path);
    if let Some(parent) = part_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| ModelDownloadError::Transport(format!("mkdir: {error}")))?;
    }
    // What is finished and verified is not downloaded again. Inside a bundle
    // these are artifacts the previous attempt managed to pull down in full.
    if final_path.exists() && verify_file(&final_path, spec).await.is_ok() {
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
    // The loop exists for exactly one retry: on 416 we erase the leftover, zero
    // the offset and ask for the whole file. The `continue` branch requires
    // `offset > 0`, and we come back into it already at zero, so it cannot be
    // entered a second time.
    let response = loop {
        let mut request = client.get(&spec.url);
        if offset > 0 {
            request = request.header(reqwest::header::RANGE, format!("bytes={offset}-"));
        }
        let response = request
            .send()
            .await
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
    while let Some(chunk_result) = stream.next().await {
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

    // Stream finished. Notify observers (PR 1.2 emits the
    // `model-download-verifying` event here) so the UI can switch
    // to a verifying spinner before the SHA-256 / size check runs.
    if let Some(verifying_cb) = on_verifying {
        verifying_cb();
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
        assert_eq!(resume_verdict(None, 100), 0, "качать нечего");
        assert_eq!(
            resume_verdict(Some(0), 100),
            0,
            "пустой остаток — не остаток"
        );
        assert_eq!(resume_verdict(Some(40), 100), 40, "обычная докачка");
        assert_eq!(
            resume_verdict(Some(100), 100),
            0,
            "файл целиком: прошлая попытка не сошлась хешем, продолжать нечего"
        );
        assert_eq!(
            resume_verdict(Some(140), 100),
            0,
            "длиннее ожидаемого — остаток от другого файла"
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
        assert!(!final_path.exists(), "битое не публикуется");
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
        // `available == 0` means the OS didn't report free space, not
        // that the disk is empty.
        assert_eq!(free_space_verdict(0, 1000), Ok(()));
    }

    #[test]
    fn free_space_verdict_requires_a_mib_of_slack() {
        // Exactly `expected` bytes is NOT enough: we reserve 1 MiB on top.
        assert!(matches!(
            free_space_verdict(1000, 1000),
            Err(ModelDownloadError::InsufficientFreeSpace { .. })
        ));
    }

    #[test]
    fn free_space_verdict_accepts_exactly_one_mib_of_slack() {
        // `available == expected + 1 MiB` is the acceptance boundary.
        assert_eq!(free_space_verdict(1000 + 1024 * 1024, 1000), Ok(()));
    }

    #[test]
    fn free_space_verdict_pins_the_slack_arithmetic() {
        // One byte under the slack must still be rejected. This is what
        // catches `1024 * 1024` being mutated to `+` or `/`: either would
        // shrink the slack and let this case through.
        assert!(matches!(
            free_space_verdict(1000 + 1024 * 1024 - 1, 1000),
            Err(ModelDownloadError::InsufficientFreeSpace { .. })
        ));
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

        assert!(!partial.exists(), "недокачанное стёрто");
        assert!(installed.exists(), "установленная модель не тронута");
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

        assert!(!stage.exists(), "черновая папка бандла стёрта целиком");
        assert!(installed.exists(), "установленный бандл не тронут");
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
