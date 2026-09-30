//! Cloud STT over one provider shape: OpenAI-compatible transcription at
//! `{base_url}/audio/transcriptions` (Groq, Mistral, OpenAI itself, and any
//! local server that mimics the shape):
//!
//!   1. Encode the 16 kHz mono f32 audio as 16-bit PCM WAV in-memory.
//!   2. POST `{base_url}/audio/transcriptions` with multipart/form-data:
//!      - field `model`    — text
//!      - field `language` — text (optional, when the user pinned one)
//!      - field `file`     — WAV bytes
//!
//!      and `Authorization: Bearer {api_key}`.
//!   3. Parse `{"text": "..."}` from the JSON response.
//!
//! All providers reuse the same shape — there is no provider-specific
//! branching in this module. Adding a new OpenAI-compatible provider
//! (Deepgram, Together, etc.) is a config change, not a code change.
//!
//! Errors are classified:
//! - `transport: ...`     — reqwest connect/read failures
//! - `timeout after Ns`   — server did not respond within the budget
//! - `http 4xx/5xx`       — server rejected the request
//! - `malformed: ...`     — non-JSON body, missing `text` field, etc.
//!
//! These map cleanly to frontend toasts in the existing error banner.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::{stream, StreamExt};
use serde::Deserialize;

const MAX_TRANSCRIPT_RESPONSE_BYTES: usize = 4 * 1024 * 1024;
const UPLOAD_CHUNK_SAMPLES: usize = 32 * 1024;

/// Identifies the cloud STT provider. Currently only OpenAI-compatible
/// is implemented; the field exists in config for forward-compat (a
/// future `assemblyai` or `deepgram` variant would slot in here without
/// a config migration).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum CloudSttProvider {
    #[default]
    Compatible,
}

/// Inputs for a cloud STT call. `audio` is mono 16 kHz f32 — the same
/// shape the local whisper engine consumes.
#[derive(Debug, Clone)]
pub struct CloudSttRequest {
    pub provider: CloudSttProvider,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub language: Option<String>,
    pub audio: Arc<Vec<f32>>,
    pub timeout_seconds: u64,
}

/// Output of a successful cloud STT call.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CloudSttResult {
    #[serde(skip)]
    pub service: crate::telemetry::ProviderService,
    pub text: String,
    pub model: String,
    pub elapsed_ms: u64,
}

/// Run one request while observing the engine's cancellation flag.
/// The pinned future retains its connection and timeout across cancellation polls.
pub async fn transcribe_cancellable(
    request: CloudSttRequest,
    cancelled: &AtomicBool,
) -> Result<CloudSttResult, String> {
    let pending = transcribe(request);
    tokio::pin!(pending);
    loop {
        if cancelled.load(Ordering::Relaxed) {
            return Err("cloud transcribe cancelled".into());
        }
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_millis(100)) => {}
            result = &mut pending => {
                return if cancelled.load(Ordering::Relaxed) {
                    Err("cloud transcribe cancelled".into())
                } else {
                    result
                };
            }
        }
    }
}

/// [`transcribe_cancellable`] for a thread outside any async runtime, such
/// as the engine thread.
///
/// The request runs on the application runtime rather than on one built for
/// the call: pooled connections live on the runtime that opened them, and a
/// per-call runtime would close the connection the next dictation is meant
/// to reuse.
pub fn transcribe_blocking(
    request: CloudSttRequest,
    cancelled: Arc<AtomicBool>,
) -> Result<CloudSttResult, String> {
    tauri::async_runtime::block_on(tauri::async_runtime::spawn(async move {
        transcribe_cancellable(request, &cancelled).await
    }))
    .map_err(|_| "cloud STT task panicked".to_string())?
}

/// Encode mono 16 kHz f32 samples as 16-bit PCM WAV bytes.
/// Matches the legacy Python `_audio_to_wav_bytes` so existing test
/// WAV byte fixtures (header inspection, byte-rate sanity) still pass.
pub fn audio_to_wav_bytes(samples: &[f32]) -> Vec<u8> {
    let pcm: Vec<i16> = samples.iter().copied().map(pcm16_sample).collect();
    crate::wav::encode_pcm16_mono(&pcm, 16_000)
}

/// Clamp + scale to i16. NaN/Inf collapse to 0 to avoid producing undefined
/// PCM samples (a real risk if a buggy upstream stage emits non-finite floats).
fn pcm16_sample(sample: f32) -> i16 {
    let clamped = if sample.is_finite() {
        sample.clamp(-1.0, 1.0)
    } else {
        0.0
    };
    (clamped * 32_768.0) as i16
}

fn wav_header(sample_count: usize) -> Result<Vec<u8>, String> {
    let data_bytes = sample_count
        .checked_mul(2)
        .and_then(|bytes| u32::try_from(bytes).ok())
        .ok_or_else(|| "cloud STT audio exceeds WAV size limit".to_string())?;
    let riff_bytes = data_bytes
        .checked_add(36)
        .ok_or_else(|| "cloud STT audio exceeds WAV size limit".to_string())?;
    let mut header = crate::wav::encode_pcm16_mono(&[], 16_000);
    header[4..8].copy_from_slice(&riff_bytes.to_le_bytes());
    header[40..44].copy_from_slice(&data_bytes.to_le_bytes());
    Ok(header)
}

fn wav_chunk(samples: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(samples.len() * 2);
    for &sample in samples {
        bytes.extend_from_slice(&pcm16_sample(sample).to_le_bytes());
    }
    bytes
}

/// Build the multipart/form-data body for an OpenAI-compatible
/// `/audio/transcriptions` request. Returns the assembled byte buffer
/// plus the `Content-Type` header value (boundary included).
///
/// We do this by hand (no reqwest `multipart` feature) because:
///   1. It keeps the dependency surface minimal — reqwest is already
///      pulled in for the AI providers, no extra features.
///   2. The body shape is small and stable (3 fields), so hand-rolling
///      is cheaper than the alternative.
pub fn build_multipart_body(
    model: &str,
    language: Option<&str>,
    file_bytes: &[u8],
) -> (Vec<u8>, String) {
    let (mut body, suffix, content_type) = multipart_parts(model, language);
    body.extend_from_slice(file_bytes);
    body.extend_from_slice(&suffix);
    (body, content_type)
}

fn multipart_parts(model: &str, language: Option<&str>) -> (Vec<u8>, Vec<u8>, String) {
    const BOUNDARY: &str = "----WhisperDesktopBoundary7MA4YWxkTrZu0gW";
    let mut body = Vec::with_capacity(1024);
    let crlf = b"\r\n";

    let push_field = |body: &mut Vec<u8>, name: &str, value: &str| {
        body.extend_from_slice(format!("--{BOUNDARY}\r\n").as_bytes());
        body.extend_from_slice(
            format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n").as_bytes(),
        );
        body.extend_from_slice(value.as_bytes());
        body.extend_from_slice(crlf);
    };

    push_field(&mut body, "model", model);
    if let Some(lang) = language {
        push_field(&mut body, "language", lang);
    }

    // File part with filename + content-type. The legacy Python code
    // used `audio/wav`; we do the same to avoid servers that sniff the
    // declared type and reject `application/octet-stream`.
    body.extend_from_slice(format!("--{BOUNDARY}\r\n").as_bytes());
    body.extend_from_slice(
        b"Content-Disposition: form-data; name=\"file\"; filename=\"recording.wav\"\r\n",
    );
    body.extend_from_slice(b"Content-Type: audio/wav\r\n\r\n");
    let suffix = format!("\r\n--{BOUNDARY}--\r\n").into_bytes();

    let content_type = format!("multipart/form-data; boundary={BOUNDARY}");
    (body, suffix, content_type)
}

/// Send the audio to the configured provider and return the
/// transcribed text. Errors are returned as plain `String`s (not a
/// typed enum) so the dispatcher can surface them in the existing
/// error toast without a new error type on the Tauri boundary.
pub async fn transcribe(req: CloudSttRequest) -> Result<CloudSttResult, String> {
    if req.provider != CloudSttProvider::Compatible {
        return Err(format!(
            "unsupported cloud STT provider: {:?}",
            req.provider
        ));
    }
    if req.base_url.trim().is_empty() {
        return Err("cloud_stt base_url is empty".to_string());
    }
    if req.api_key.trim().is_empty() {
        return Err("cloud_stt api_key is empty".to_string());
    }
    if req.model.trim().is_empty() {
        return Err("cloud_stt model is empty".to_string());
    }

    let started = Instant::now();
    let wav_header = wav_header(req.audio.len())?;
    let (prefix, suffix, content_type) = multipart_parts(&req.model, req.language.as_deref());
    let content_length = prefix
        .len()
        .checked_add(wav_header.len())
        .and_then(|size| size.checked_add(req.audio.len() * 2))
        .and_then(|size| size.checked_add(suffix.len()))
        .ok_or_else(|| "cloud STT upload exceeds size limit".to_string())?;
    let chunks = stream::unfold(
        (Arc::clone(&req.audio), 0usize),
        |(audio, start)| async move {
            if start == audio.len() {
                None
            } else {
                let end = start.saturating_add(UPLOAD_CHUNK_SAMPLES).min(audio.len());
                let bytes = wav_chunk(&audio[start..end]);
                Some((Ok::<Vec<u8>, std::io::Error>(bytes), (audio, end)))
            }
        },
    );
    let upload = stream::iter([Ok::<Vec<u8>, std::io::Error>(prefix), Ok(wav_header)])
        .chain(chunks)
        .chain(stream::once(async move {
            Ok::<Vec<u8>, std::io::Error>(suffix)
        }));
    let body = reqwest::Body::wrap_stream(upload);

    let url = format!(
        "{}/audio/transcriptions",
        req.base_url.trim_end_matches('/')
    );
    // The shared client keeps the connection to the provider open between
    // dictations, so only the first one pays for DNS, TCP and TLS.
    let response = crate::ai::providers::shared_client()
        .post(&url)
        .timeout(Duration::from_secs(req.timeout_seconds.max(1)))
        .header("Authorization", format!("Bearer {}", req.api_key))
        .header("Content-Type", content_type)
        .header("Content-Length", content_length.to_string())
        .body(body)
        .send()
        .await
        .map_err(|error| {
            if error.is_timeout() {
                format!("timeout after {}s", req.timeout_seconds)
            } else if error.is_connect() {
                format!("transport: connect failed: {error}")
            } else {
                format!("transport: {error}")
            }
        })?;

    let status = response.status();
    if !status.is_success() {
        // A provider can echo audio, transcript or credentials in its error
        // body, so neither collect nor log it.
        log::debug!("cloud STT http {}", status.as_u16());
        return Err(format!(
            "http {} {}",
            status.as_u16(),
            status.canonical_reason().unwrap_or("")
        )
        .trim_end()
        .to_string());
    }

    let body = crate::http_client::read_bounded_body(response, MAX_TRANSCRIPT_RESPONSE_BYTES)
        .await
        .map_err(|error| match error {
            crate::http_client::ResponseBodyError::TooLarge => {
                "malformed: transcript response too large".to_string()
            }
            crate::http_client::ResponseBodyError::Read(error) if error.is_timeout() => {
                format!("timeout after {}s", req.timeout_seconds.max(1))
            }
            crate::http_client::ResponseBodyError::Read(error) => {
                format!("transport: {error}")
            }
        })?;
    let parsed: serde_json::Value = serde_json::from_slice(&body)
        .map_err(|error| format!("malformed: invalid JSON: {error}"))?;

    let text = parsed
        .get("text")
        .and_then(|value| value.as_str())
        // Not the response itself: whatever it holds instead of `text` may be
        // the transcript.
        .ok_or_else(|| "malformed: missing 'text' field in response".to_string())?
        .to_string();

    Ok(CloudSttResult {
        service: crate::telemetry::provider_service(&req.base_url),
        text,
        model: req.model,
        elapsed_ms: started.elapsed().as_millis() as u64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_wire_bytes_preserve_scaling_clipping_and_non_finite_silence() {
        let samples = [
            0.0,
            -1.0,
            1.0,
            0.5,
            -0.5,
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
            2.0,
            -2.0,
        ];
        let expected = b"\x52\x49\x46\x46\x38\x00\x00\x00\x57\x41\x56\x45\x66\x6d\x74\x20\x10\x00\x00\x00\x01\x00\x01\x00\x80\x3e\x00\x00\x00\x7d\x00\x00\x02\x00\x10\x00\x64\x61\x74\x61\x14\x00\x00\x00\x00\x00\x00\x80\xff\x7f\x00\x40\x00\xc0\x00\x00\x00\x00\x00\x00\xff\x7f\x00\x80";
        assert_eq!(audio_to_wav_bytes(&samples), expected);
    }

    #[test]
    fn chunked_wav_encoder_matches_in_memory_bytes_across_chunk_boundary() {
        let mut samples = vec![0.25_f32; UPLOAD_CHUNK_SAMPLES + 3];
        samples[UPLOAD_CHUNK_SAMPLES - 1] = f32::NAN;
        samples[UPLOAD_CHUNK_SAMPLES] = -2.0;
        let mut streamed = wav_header(samples.len()).unwrap();
        for chunk in samples.chunks(UPLOAD_CHUNK_SAMPLES) {
            streamed.extend_from_slice(&wav_chunk(chunk));
        }
        assert_eq!(streamed, audio_to_wav_bytes(&samples));
    }

    #[test]
    fn wav_bytes_have_canonical_header() {
        // Mirrors the legacy Python test: the bytes start with RIFF,
        // contain WAVE in the first 16 bytes, and the byte rate
        // matches 16000 Hz * 1 ch * 2 bytes/sample = 32000.
        let wav = audio_to_wav_bytes(&[0.0_f32, 0.5, -0.5]);
        assert!(wav.starts_with(b"RIFF"), "missing RIFF marker");
        assert!(
            wav[..16].windows(4).any(|w| w == b"WAVE"),
            "missing WAVE marker"
        );
        // 12..16 should be "fmt "
        assert_eq!(&wav[12..16], b"fmt ");
        // riff size is at offset 4..8: 36-byte header + data_size.
        let riff_size = u32::from_le_bytes([wav[4], wav[5], wav[6], wav[7]]);
        assert_eq!(riff_size, 42, "riff size must be 36 + 6 bytes of PCM");
        // Byte rate is at offset 28 (after RIFF chunk: 4 + 4 + "WAVE" + "fmt " + 16 + 1 + 1 + 1 + 4)
        let byte_rate = u32::from_le_bytes([wav[28], wav[29], wav[30], wav[31]]);
        assert_eq!(byte_rate, 32_000);
        // block align at offset 32..34 = channels * bytes/sample = 2.
        let block_align = u16::from_le_bytes([wav[32], wav[33]]);
        assert_eq!(block_align, 2, "block align must be 1 channel × 2 bytes");
        // bits per sample at offset 34
        let bits_per_sample = u16::from_le_bytes([wav[34], wav[35]]);
        assert_eq!(bits_per_sample, 16);
        // data chunk: bytes 36..40 = "data"
        assert_eq!(&wav[36..40], b"data");
        // data size = samples * 2 = 3 * 2 = 6
        let data_size = u32::from_le_bytes([wav[40], wav[41], wav[42], wav[43]]);
        assert_eq!(data_size, 6);
    }

    #[test]
    fn wav_clamps_and_silences_non_finite() {
        // NaN / Inf must NOT produce undefined i16; clamp/zero is
        // the safe path. A real bug here would surface as static /
        // pops on replay.
        let samples = vec![f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 2.0, -2.0];
        let wav = audio_to_wav_bytes(&samples);
        // 5 samples * 2 bytes = 10 bytes of PCM after the 44-byte header
        let pcm_bytes = &wav[44..];
        assert_eq!(pcm_bytes.len(), 10);
        // First three samples should all be 0 (NaN → 0, +Inf → 0 via clamp+truncate, -Inf → 0).
        // i16::to_le_bytes of 0 is [0, 0].
        for chunk in pcm_bytes[..6].chunks(2) {
            assert_eq!(chunk, &[0, 0]);
        }
        // Out-of-range values clamp to i16::MAX and i16::MIN.
        let last_two = i16::from_le_bytes([pcm_bytes[8], pcm_bytes[9]]);
        assert_eq!(last_two, i16::MIN);
    }

    #[test]
    fn multipart_body_has_required_fields_and_boundary() {
        let wav = audio_to_wav_bytes(&[0.0_f32; 100]);
        let (body, content_type) = build_multipart_body("whisper-large-v3-turbo", Some("ru"), &wav);
        let body_str = String::from_utf8_lossy(&body);
        assert!(
            body_str.contains("name=\"model\""),
            "missing model field, body: {body_str}"
        );
        assert!(
            body_str.contains("whisper-large-v3-turbo"),
            "missing model value, body: {body_str}"
        );
        assert!(
            body_str.contains("name=\"language\""),
            "missing language field"
        );
        assert!(
            body_str.contains("name=\"file\"; filename=\"recording.wav\""),
            "missing file field with filename"
        );
        assert!(content_type.starts_with("multipart/form-data; boundary="));
        // Boundary used in body must match the one declared in the
        // header — servers split on it and a mismatch yields a 400.
        let boundary = content_type
            .split("boundary=")
            .nth(1)
            .expect("boundary= in content type");
        assert!(
            body_str.contains(boundary),
            "boundary in header does not appear in body"
        );
    }

    #[test]
    fn multipart_body_omits_language_when_none() {
        // The legacy Python code sends `language` only when it was
        // provided — sending an empty `language=` causes some servers
        // (Groq) to reject the request. This test pins that behavior.
        let wav = audio_to_wav_bytes(&[0.0_f32; 100]);
        let (body, _) = build_multipart_body("m", None, &wav);
        let body_str = String::from_utf8_lossy(&body);
        assert!(
            !body_str.contains("name=\"language\""),
            "language field must be omitted when None"
        );
    }

    #[test]
    fn transcribe_rejects_empty_inputs() {
        // Validation must happen BEFORE the network call so the
        // dispatcher surfaces a clear error rather than an opaque
        // 4xx from the server.
        let req = CloudSttRequest {
            provider: CloudSttProvider::Compatible,
            base_url: "".into(),
            api_key: "k".into(),
            model: "m".into(),
            language: None,
            audio: Arc::new(vec![0.0; 16]),
            timeout_seconds: 5,
        };
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let err = runtime.block_on(transcribe(req)).unwrap_err();
        assert!(err.contains("base_url"));
    }

    #[test]
    fn provider_default_is_compatible() {
        assert_eq!(CloudSttProvider::default(), CloudSttProvider::Compatible);
    }
}
