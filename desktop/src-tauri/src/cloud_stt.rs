//! Phase 4 / Batch 4 — Cloud STT providers (Rust port).
//!
//! The legacy Python implementation supported one provider shape:
//! OpenAI-compatible transcription at `{base_url}/audio/transcriptions`
//! (Groq, Mistral, OpenAI itself, and any local server that mimics the
//! shape). The port mirrors that contract exactly so existing
//! user-saved configs and the front-end UI keep working:
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
//! - `transport: ...`     — reqwest build/connect/read failures
//! - `timeout after Ns`   — server did not respond within the budget
//! - `http 4xx/5xx`       — server rejected the request
//! - `malformed: ...`     — non-JSON body, missing `text` field, etc.
//!
//! These map cleanly to frontend toasts in the existing error banner.

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Deserialize;

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
    pub text: String,
    pub model: String,
    pub elapsed_ms: u64,
}

/// Encode mono 16 kHz f32 samples as 16-bit PCM WAV bytes.
/// Matches the legacy Python `_audio_to_wav_bytes` so existing test
/// WAV byte fixtures (header inspection, byte-rate sanity) still pass.
pub fn audio_to_wav_bytes(samples: &[f32]) -> Vec<u8> {
    const SAMPLE_RATE: u32 = 16_000;
    const BITS_PER_SAMPLE: u16 = 16;
    const NUM_CHANNELS: u16 = 1;
    const BYTE_RATE: u32 = SAMPLE_RATE * NUM_CHANNELS as u32 * BITS_PER_SAMPLE as u32 / 8;
    const BLOCK_ALIGN: u16 = NUM_CHANNELS * BITS_PER_SAMPLE / 8;

    // Clamp + scale to i16. NaN/Inf collapse to 0 to avoid producing
    // undefined PCM samples (a real risk if a buggy upstream stage
    // emits non-finite floats).
    let pcm: Vec<i16> = samples
        .iter()
        .map(|&s| {
            let clamped = if s.is_finite() {
                s.clamp(-1.0, 1.0)
            } else {
                0.0
            };
            (clamped * 32_768.0) as i16
        })
        .collect();

    let data_size = (pcm.len() * 2) as u32;
    let riff_size = 36 + data_size;

    let mut out = Vec::with_capacity(44 + pcm.len() * 2);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&riff_size.to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16_u32.to_le_bytes()); // sub-chunk size
    out.extend_from_slice(&1_u16.to_le_bytes()); // PCM format
    out.extend_from_slice(&NUM_CHANNELS.to_le_bytes());
    out.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    out.extend_from_slice(&BYTE_RATE.to_le_bytes());
    out.extend_from_slice(&BLOCK_ALIGN.to_le_bytes());
    out.extend_from_slice(&BITS_PER_SAMPLE.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_size.to_le_bytes());
    for sample in &pcm {
        out.extend_from_slice(&sample.to_le_bytes());
    }
    out
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
    const BOUNDARY: &str = "----WhisperDesktopBoundary7MA4YWxkTrZu0gW";
    let mut body = Vec::with_capacity(file_bytes.len() + 1024);
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
    body.extend_from_slice(file_bytes);
    body.extend_from_slice(crlf);

    body.extend_from_slice(format!("--{BOUNDARY}--\r\n").as_bytes());

    let content_type = format!("multipart/form-data; boundary={BOUNDARY}");
    (body, content_type)
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
    let wav_bytes = audio_to_wav_bytes(&req.audio);
    let (body, content_type) =
        build_multipart_body(&req.model, req.language.as_deref(), &wav_bytes);

    let url = format!(
        "{}/audio/transcriptions",
        req.base_url.trim_end_matches('/')
    );
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(req.timeout_seconds.max(1)))
        .build()
        .map_err(|error| format!("transport: build client: {error}"))?;

    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", req.api_key))
        .header("Content-Type", content_type)
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
        let body = response
            .text()
            .await
            .unwrap_or_else(|error| format!("<unreadable: {error}>"));
        return Err(format!(
            "http {}: {}",
            status.as_u16(),
            truncate(&body, 240)
        ));
    }

    let parsed: serde_json::Value = response
        .json()
        .await
        .map_err(|error| format!("malformed: invalid JSON: {error}"))?;

    let text = parsed
        .get("text")
        .and_then(|value| value.as_str())
        .ok_or_else(|| {
            format!(
                "malformed: missing 'text' field in response: {}",
                truncate(&parsed.to_string(), 200)
            )
        })?
        .to_string();

    Ok(CloudSttResult {
        text,
        model: req.model,
        elapsed_ms: started.elapsed().as_millis() as u64,
    })
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let mut truncated = s[..max].to_string();
        truncated.push('…');
        truncated
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
