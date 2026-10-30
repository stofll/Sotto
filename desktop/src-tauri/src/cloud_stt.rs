//! Cloud STT: the whole recording goes to one provider in one request and the
//! transcript comes back. The adapter follows the profile's provider:
//!
//! - `Compatible` — OpenAI-compatible `{base_url}/audio/transcriptions`
//!   (Groq, Mistral, OpenAI itself, and any local server that mimics the
//!   shape): multipart `model`, optional `language` and a WAV `file`, with
//!   `Authorization: Bearer`. Reads `text`.
//! - `ElevenLabs` — `{base_url}/speech-to-text`: multipart `model_id`,
//!   optional `language_code` and the raw 16 kHz PCM as `file`, with
//!   `xi-api-key`. Reads `text`.
//! - `AzureSpeech` — the Azure Speech fast-transcription API that serves
//!   MAI-Transcribe: multipart WAV `audio` and a `definition` JSON naming the
//!   model, with `Ocp-Apim-Subscription-Key`. Reads `combinedPhrases[].text`.
//! - `Gemini` — the Gemini Interactions API with the WAV inline as base64,
//!   with `x-goog-api-key`. Reads the text of the model output.
//!
//! Errors are classified:
//! - `transport: ...`     — reqwest connect/read failures
//! - `timeout after Ns`   — server did not respond within the budget
//! - `http 4xx/5xx`       — server rejected the request
//! - `malformed: ...`     — non-JSON body, missing transcript, etc.
//!
//! These map cleanly to frontend toasts in the existing error banner.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use base64::Engine as _;
use futures_util::stream;
use serde_json::{json, Value};

use crate::telemetry::ProviderService;

const MAX_TRANSCRIPT_RESPONSE_BYTES: usize = 4 * 1024 * 1024;
const UPLOAD_CHUNK_SAMPLES: usize = 32 * 1024;
/// An error body is read only for its reason code.
const MAX_ERROR_RESPONSE_BYTES: usize = 64 * 1024;
const MAX_ERROR_CODE_CHARS: usize = 48;

const ELEVENLABS_BASE_URL: &str = "https://api.elevenlabs.io/v1";
const GEMINI_BASE_URL: &str = "https://generativelanguage.googleapis.com/v1beta";
const AZURE_SPEECH_API_VERSION: &str = "2025-10-15";
/// Gemini accepts inline audio only in a request under 20 MB, about seven
/// minutes of 16 kHz WAV once base64-encoded. Longer audio would have to go
/// through the Files API, which keeps the upload on Google's side for two days.
const GEMINI_INLINE_LIMIT_BYTES: usize = 20_000_000;

/// Which API the request speaks. Chosen from the profile's provider id by
/// [`CloudSttProvider::for_profile`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CloudSttProvider {
    #[default]
    Compatible,
    ElevenLabs,
    AzureSpeech,
    Gemini,
}

impl CloudSttProvider {
    /// A provider without an adapter of its own speaks the OpenAI shape.
    pub fn for_profile(provider: &str) -> Self {
        match provider {
            "elevenlabs" => Self::ElevenLabs,
            "azure-speech" => Self::AzureSpeech,
            "gemini" => Self::Gemini,
            _ => Self::Compatible,
        }
    }

    /// The fixed label telemetry reports for the adapter.
    pub fn wire(self) -> &'static str {
        match self {
            Self::Compatible => "compatible",
            Self::ElevenLabs => "elevenlabs",
            Self::AzureSpeech => "azure_speech",
            Self::Gemini => "gemini",
        }
    }
}

/// Providers that only transcribe and have no text API for the LLM step.
pub fn transcription_only(provider: &str) -> bool {
    matches!(provider, "elevenlabs" | "azure-speech")
}

/// The models a transcription-only provider offers. Neither serves a list
/// of its speech models: ElevenLabs' `/v1/models` lists voices for synthesis,
/// and an Azure Speech resource has no listing at all.
pub fn transcription_models(provider: &str) -> Option<&'static [&'static str]> {
    match provider {
        "elevenlabs" => Some(&["scribe_v2", "scribe_v2_medical", "scribe_v1"]),
        "azure-speech" => Some(&["MAI-Transcribe-2", "MAI-Transcribe-1.5"]),
        _ => None,
    }
}

/// The endpoint a profile transcribes with. Gemini has one public endpoint,
/// as in its LLM adapter; ElevenLabs and OpenAI fall back to theirs; an Azure
/// resource and an OpenAI-compatible server exist only at the address given.
pub fn effective_base_url(provider: &str, configured: Option<&str>) -> String {
    let configured = configured.map(str::trim).filter(|value| !value.is_empty());
    match provider {
        "gemini" => GEMINI_BASE_URL.to_string(),
        "elevenlabs" => configured.unwrap_or(ELEVENLABS_BASE_URL).to_string(),
        "openai" => configured
            .unwrap_or(crate::ai::providers::OPENAI_BASE_URL)
            .to_string(),
        _ => configured.unwrap_or_default().to_string(),
    }
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

/// What telemetry may say about a completed request: fixed labels only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CloudSttRoute {
    pub provider: CloudSttProvider,
    pub service: ProviderService,
}

/// Output of a successful cloud STT call.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CloudSttResult {
    #[serde(skip)]
    pub route: CloudSttRoute,
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
///   2. The body shape is small and stable, so hand-rolling is cheaper
///      than the alternative.
pub fn build_multipart_body(
    model: &str,
    language: Option<&str>,
    file_bytes: &[u8],
) -> (Vec<u8>, String) {
    let (mut body, suffix, content_type) =
        multipart_parts(&compatible_fields(model, language), WAV_FILE);
    body.extend_from_slice(file_bytes);
    body.extend_from_slice(&suffix);
    (body, content_type)
}

/// The file part of a multipart upload.
struct FormFile {
    field: &'static str,
    name: &'static str,
    content_type: &'static str,
}

// The legacy Python code used `audio/wav`; we do the same to avoid servers
// that sniff the declared type and reject `application/octet-stream`.
const WAV_FILE: FormFile = FormFile {
    field: "file",
    name: "recording.wav",
    content_type: "audio/wav",
};

fn compatible_fields<'a>(
    model: &'a str,
    language: Option<&'a str>,
) -> Vec<(&'static str, &'a str)> {
    let mut fields = vec![("model", model)];
    if let Some(language) = language {
        fields.push(("language", language));
    }
    fields
}

/// Everything before the file bytes, everything after them, and the
/// `Content-Type` value: the file itself is streamed in between.
fn multipart_parts(fields: &[(&str, &str)], file: FormFile) -> (Vec<u8>, Vec<u8>, String) {
    const BOUNDARY: &str = "----WhisperDesktopBoundary7MA4YWxkTrZu0gW";
    let mut body = Vec::with_capacity(1024);
    for (name, value) in fields {
        body.extend_from_slice(format!("--{BOUNDARY}\r\n").as_bytes());
        body.extend_from_slice(
            format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n").as_bytes(),
        );
        body.extend_from_slice(value.as_bytes());
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{BOUNDARY}\r\n").as_bytes());
    body.extend_from_slice(
        format!(
            "Content-Disposition: form-data; name=\"{}\"; filename=\"{}\"\r\n",
            file.field, file.name
        )
        .as_bytes(),
    );
    body.extend_from_slice(format!("Content-Type: {}\r\n\r\n", file.content_type).as_bytes());
    let suffix = format!("\r\n--{BOUNDARY}--\r\n").into_bytes();

    let content_type = format!("multipart/form-data; boundary={BOUNDARY}");
    (body, suffix, content_type)
}

/// The production upload encoder. Iteration encodes one bounded PCM chunk at
/// a time, so consuming it in a benchmark exercises the same allocation and
/// conversion path as HTTP backpressure does without contacting a provider.
pub struct MultipartUpload {
    pub content_length: usize,
    pub content_type: String,
    prefix: Option<Vec<u8>>,
    header: Option<Vec<u8>>,
    suffix: Option<Vec<u8>>,
    audio: Arc<Vec<f32>>,
    offset: usize,
}

impl Iterator for MultipartUpload {
    type Item = Vec<u8>;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(prefix) = self.prefix.take() {
            return Some(prefix);
        }
        if let Some(header) = self.header.take() {
            return Some(header);
        }
        if self.offset < self.audio.len() {
            let end = self
                .offset
                .saturating_add(UPLOAD_CHUNK_SAMPLES)
                .min(self.audio.len());
            let bytes = wav_chunk(&self.audio[self.offset..end]);
            self.offset = end;
            return Some(bytes);
        }
        self.suffix.take()
    }
}

/// The OpenAI-compatible upload.
pub fn multipart_upload(
    audio: Arc<Vec<f32>>,
    model: &str,
    language: Option<&str>,
) -> Result<MultipartUpload, String> {
    encode_upload(audio, &compatible_fields(model, language), WAV_FILE, true)
}

/// A multipart upload whose file part is the audio as 16-bit PCM, preceded by
/// a WAV header when `wav` is set.
fn encode_upload(
    audio: Arc<Vec<f32>>,
    fields: &[(&str, &str)],
    file: FormFile,
    wav: bool,
) -> Result<MultipartUpload, String> {
    let header = wav.then(|| wav_header(audio.len())).transpose()?;
    let (prefix, suffix, content_type) = multipart_parts(fields, file);
    let content_length = audio
        .len()
        .checked_mul(2)
        .and_then(|size| size.checked_add(prefix.len()))
        .and_then(|size| size.checked_add(header.as_ref().map_or(0, Vec::len)))
        .and_then(|size| size.checked_add(suffix.len()))
        .ok_or_else(|| "cloud STT upload exceeds size limit".to_string())?;
    Ok(MultipartUpload {
        content_length,
        content_type,
        prefix: Some(prefix),
        header,
        suffix: Some(suffix),
        audio,
        offset: 0,
    })
}

/// A request ready to send. Multipart uploads are streamed so a long file is
/// never held as a second, encoded copy in memory.
struct Upload {
    url: String,
    auth: (&'static str, String),
    content_type: String,
    content_length: usize,
    body: reqwest::Body,
}

fn pcm_upload(
    url: String,
    auth: (&'static str, String),
    fields: &[(&str, &str)],
    file: FormFile,
    audio: &Arc<Vec<f32>>,
    wav: bool,
) -> Result<Upload, String> {
    let mut upload = encode_upload(Arc::clone(audio), fields, file, wav)?;
    Ok(Upload {
        url,
        auth,
        content_type: std::mem::take(&mut upload.content_type),
        content_length: upload.content_length,
        body: reqwest::Body::wrap_stream(stream::iter(upload.map(Ok::<_, std::io::Error>))),
    })
}

fn build_upload(req: &CloudSttRequest, language: Option<&str>) -> Result<Upload, String> {
    let base = req.base_url.trim().trim_end_matches('/');
    match req.provider {
        CloudSttProvider::Compatible => pcm_upload(
            format!("{base}/audio/transcriptions"),
            ("Authorization", format!("Bearer {}", req.api_key)),
            &compatible_fields(&req.model, language),
            WAV_FILE,
            &req.audio,
            true,
        ),
        CloudSttProvider::ElevenLabs => {
            // Raw PCM rather than WAV: the API documents lower latency for
            // `pcm_s16le_16`. Audio-event tags such as «(laughter)» and word
            // timestamps are of no use to dictation.
            let mut fields = vec![
                ("model_id", req.model.as_str()),
                ("file_format", "pcm_s16le_16"),
                ("tag_audio_events", "false"),
                ("timestamps_granularity", "none"),
            ];
            if let Some(language) = language {
                fields.push(("language_code", language));
            }
            pcm_upload(
                format!("{base}/speech-to-text"),
                ("xi-api-key", req.api_key.clone()),
                &fields,
                FormFile {
                    field: "file",
                    name: "recording.pcm",
                    content_type: "application/octet-stream",
                },
                &req.audio,
                false,
            )
        }
        CloudSttProvider::AzureSpeech => {
            let mut definition = json!({
                "enhancedMode": { "enabled": true, "model": req.model },
            });
            if let Some(language) = language {
                definition["locales"] = json!([language]);
            }
            let definition = definition.to_string();
            pcm_upload(
                format!(
                    "{base}/speechtotext/transcriptions:transcribe?api-version={AZURE_SPEECH_API_VERSION}"
                ),
                ("Ocp-Apim-Subscription-Key", req.api_key.clone()),
                &[("definition", definition.as_str())],
                FormFile {
                    field: "audio",
                    ..WAV_FILE
                },
                &req.audio,
                true,
            )
        }
        CloudSttProvider::Gemini => {
            let wav = audio_to_wav_bytes(&req.audio);
            if wav.len().div_ceil(3) * 4 > GEMINI_INLINE_LIMIT_BYTES {
                return Err(
                    "cloud STT audio exceeds the Gemini inline limit (about 7 minutes)".to_string(),
                );
            }
            let mut body = json!({
                "model": req.model,
                // Interactions are kept on Google's side by default; a
                // dictation needs no server-side history.
                "store": false,
                "input": [{
                    "type": "audio",
                    "mime_type": "audio/wav",
                    "data": base64::engine::general_purpose::STANDARD.encode(&wav),
                }],
            });
            if let Some(language) = language {
                body["generation_config"] =
                    json!({ "transcription_config": { "language_codes": [language] } });
            }
            let body = body.to_string().into_bytes();
            Ok(Upload {
                url: format!("{base}/interactions"),
                auth: ("x-goog-api-key", req.api_key.clone()),
                content_type: "application/json".to_string(),
                content_length: body.len(),
                body: body.into(),
            })
        }
    }
}

/// The transcript in a successful response, by the adapter's response shape.
fn transcript_text(provider: CloudSttProvider, parsed: &Value) -> Option<String> {
    let texts = |items: &Value, kind: Option<&str>| -> Option<Vec<String>> {
        let items = items.as_array()?;
        Some(
            items
                .iter()
                .filter(|item| {
                    kind.is_none_or(|kind| item.get("type").and_then(Value::as_str) == Some(kind))
                })
                .filter_map(|item| item.get("text").and_then(Value::as_str))
                .map(str::to_string)
                .collect(),
        )
    };
    match provider {
        CloudSttProvider::Compatible | CloudSttProvider::ElevenLabs => parsed
            .get("text")
            .and_then(Value::as_str)
            .map(str::to_string),
        // One entry per channel, and the recording has one.
        CloudSttProvider::AzureSpeech => {
            texts(parsed.get("combinedPhrases")?, None).map(|texts| texts.join(" "))
        }
        // Since the May 2026 API revision the answer is a `model_output`
        // step; earlier revisions return it in `outputs`.
        CloudSttProvider::Gemini => match parsed.get("steps").and_then(Value::as_array) {
            Some(steps) => {
                let outputs: Vec<&Value> = steps
                    .iter()
                    .filter(|step| step.get("type").and_then(Value::as_str) == Some("model_output"))
                    .collect();
                if outputs.is_empty() {
                    return None;
                }
                let parts = outputs
                    .iter()
                    .filter_map(|step| texts(step.get("content")?, Some("text")))
                    .flatten()
                    .collect::<Vec<_>>();
                Some(parts.concat())
            }
            None => texts(parsed.get("outputs")?, Some("text")).map(|texts| texts.concat()),
        },
    }
}

/// The field named in the error when the transcript is missing.
fn transcript_field(provider: CloudSttProvider) -> &'static str {
    match provider {
        CloudSttProvider::Compatible | CloudSttProvider::ElevenLabs => "text",
        CloudSttProvider::AzureSpeech => "combinedPhrases",
        CloudSttProvider::Gemini => "model_output",
    }
}

/// The provider's machine-readable reason from an error body, and nothing else
/// from it: the body can echo audio, transcript or credentials. Only a short
/// identifier passes, so a message or a key in a `code` field is dropped.
///
/// Where each API puts it: ElevenLabs `detail.status`, Gemini `error.status`,
/// Azure and OpenAI `error.code`, with OpenAI's `error.type` when the code is
/// null.
fn provider_error_code(body: &[u8]) -> Option<String> {
    let parsed: Value = serde_json::from_slice(body).ok()?;
    let error = parsed.get("error");
    let code = [
        parsed.get("detail").and_then(|detail| detail.get("status")),
        error.and_then(|error| error.get("status")),
        error.and_then(|error| error.get("code")),
        error.and_then(|error| error.get("type")),
    ]
    .into_iter()
    .flatten()
    .filter_map(Value::as_str)
    .find(|code| {
        (1..=MAX_ERROR_CODE_CHARS).contains(&code.len())
            && code.bytes().any(|byte| byte.is_ascii_alphabetic())
            && code
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    })
    .map(str::to_string);
    code
}

/// Telemetry's service label. An Azure Speech resource lives on a host of its
/// own, so the adapter itself names the service rather than the hostname.
fn route(provider: CloudSttProvider, base_url: &str) -> CloudSttRoute {
    let service = match provider {
        CloudSttProvider::AzureSpeech => ProviderService::Azure,
        _ => crate::telemetry::provider_service(base_url),
    };
    CloudSttRoute { provider, service }
}

/// Send the audio to the configured provider and return the
/// transcribed text. Errors are returned as plain `String`s (not a
/// typed enum) so the dispatcher can surface them in the existing
/// error toast without a new error type on the Tauri boundary.
pub async fn transcribe(req: CloudSttRequest) -> Result<CloudSttResult, String> {
    if req.base_url.trim().is_empty() {
        return Err("cloud_stt base_url is empty".to_string());
    }
    if req.api_key.trim().is_empty() {
        return Err("cloud_stt api_key is empty".to_string());
    }
    if req.model.trim().is_empty() {
        return Err("cloud_stt model is empty".to_string());
    }
    // `auto` is the app's spelling of "detect it"; every API here says that by
    // leaving the language out.
    let language = req
        .language
        .as_deref()
        .map(str::trim)
        .filter(|language| !language.is_empty() && *language != "auto");

    let started = Instant::now();
    let upload = build_upload(&req, language)?;
    // The shared client keeps the connection to the provider open between
    // dictations, so only the first one pays for DNS, TCP and TLS.
    let response = crate::ai::providers::shared_client()
        .post(&upload.url)
        .timeout(Duration::from_secs(req.timeout_seconds.max(1)))
        .header(upload.auth.0, upload.auth.1)
        .header("Content-Type", upload.content_type)
        .header("Content-Length", upload.content_length.to_string())
        .body(upload.body)
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
        log::debug!("cloud STT http {}", status.as_u16());
        let mut message = format!(
            "http {} {}",
            status.as_u16(),
            status.canonical_reason().unwrap_or("")
        )
        .trim_end()
        .to_string();
        // A 401 means anything from a mistyped key to a key without access to
        // speech recognition; the provider's own code tells which.
        let body = crate::http_client::read_bounded_body(response, MAX_ERROR_RESPONSE_BYTES).await;
        if let Some(code) = body.ok().and_then(|body| provider_error_code(&body)) {
            message.push_str(&format!(" ({code})"));
        }
        return Err(message);
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
    let parsed: Value = serde_json::from_slice(&body)
        .map_err(|error| format!("malformed: invalid JSON: {error}"))?;

    // Not the response itself: whatever it holds instead of the transcript
    // may be the transcript.
    let text = transcript_text(req.provider, &parsed).ok_or_else(|| {
        format!(
            "malformed: missing '{}' field in response",
            transcript_field(req.provider)
        )
    })?;

    Ok(CloudSttResult {
        route: route(req.provider, &req.base_url),
        text,
        model: req.model,
        elapsed_ms: started.elapsed().as_millis() as u64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upload_preserves_wire_bytes_and_bounds_pcm_chunks() {
        for count in [
            0,
            1,
            UPLOAD_CHUNK_SAMPLES - 1,
            UPLOAD_CHUNK_SAMPLES,
            UPLOAD_CHUNK_SAMPLES * 2 + 7,
        ] {
            let audio: Arc<Vec<f32>> = Arc::new((0..count).map(|i| (i as f32).sin()).collect());
            for language in [None, Some("ru")] {
                let (expected, content_type) =
                    build_multipart_body("model", language, &audio_to_wav_bytes(&audio));
                let mut upload = multipart_upload(Arc::clone(&audio), "model", language).unwrap();
                assert_eq!(upload.content_length, expected.len());
                assert_eq!(upload.content_type, content_type);
                let mut actual = Vec::new();
                for chunk in upload.by_ref() {
                    assert!(chunk.len() <= UPLOAD_CHUNK_SAMPLES * 2);
                    actual.extend(chunk);
                }
                assert_eq!(actual, expected);
                assert!(upload.next().is_none());
            }
        }
    }

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
    fn profile_provider_selects_the_adapter() {
        assert_eq!(
            CloudSttProvider::for_profile("elevenlabs"),
            CloudSttProvider::ElevenLabs
        );
        assert_eq!(
            CloudSttProvider::for_profile("azure-speech"),
            CloudSttProvider::AzureSpeech
        );
        assert_eq!(
            CloudSttProvider::for_profile("gemini"),
            CloudSttProvider::Gemini
        );
        // Presets (Groq, Mistral, local servers) are `compatible` profiles.
        for provider in ["compatible", "openai", "opencode-go", "anthropic"] {
            assert_eq!(
                CloudSttProvider::for_profile(provider),
                CloudSttProvider::Compatible
            );
        }
    }

    #[test]
    fn base_url_falls_back_only_where_the_provider_has_one_endpoint() {
        assert_eq!(
            effective_base_url("elevenlabs", Some(" ")),
            ELEVENLABS_BASE_URL
        );
        assert_eq!(
            effective_base_url(
                "elevenlabs",
                Some("https://api.eu.residency.elevenlabs.io/v1")
            ),
            "https://api.eu.residency.elevenlabs.io/v1"
        );
        assert_eq!(
            effective_base_url("openai", None),
            crate::ai::providers::OPENAI_BASE_URL
        );
        // Gemini's adapter ignores base_url for the LLM as well.
        assert_eq!(
            effective_base_url("gemini", Some("https://proxy.test/v1")),
            GEMINI_BASE_URL
        );
        // An Azure resource or a compatible server has no address to guess.
        assert_eq!(effective_base_url("azure-speech", None), "");
        assert_eq!(effective_base_url("compatible", Some("")), "");
    }

    #[test]
    fn gemini_text_is_read_from_both_response_revisions() {
        let steps = json!({ "steps": [
            { "type": "thought", "summary": [{ "type": "text", "text": "ignored" }] },
            { "type": "model_output", "content": [{ "type": "text", "text": "привет мир" }] },
        ]});
        assert_eq!(
            transcript_text(CloudSttProvider::Gemini, &steps).as_deref(),
            Some("привет мир")
        );
        let outputs = json!({ "outputs": [{ "type": "text", "text": "hello" }] });
        assert_eq!(
            transcript_text(CloudSttProvider::Gemini, &outputs).as_deref(),
            Some("hello")
        );
        // A turn with no model output is not an empty transcript.
        let no_output = json!({ "steps": [{ "type": "thought" }] });
        assert_eq!(transcript_text(CloudSttProvider::Gemini, &no_output), None);
    }

    #[test]
    fn error_code_is_read_where_each_provider_puts_it() {
        let code = |body: Value| provider_error_code(body.to_string().as_bytes());
        let elevenlabs = json!({ "detail": { "status": "missing_permissions", "message": "The API key you used is missing the permission speech_to_text" } });
        assert_eq!(code(elevenlabs).as_deref(), Some("missing_permissions"));
        let gemini =
            json!({ "error": { "code": 400, "status": "INVALID_ARGUMENT", "message": "x" } });
        assert_eq!(code(gemini).as_deref(), Some("INVALID_ARGUMENT"));
        let azure = json!({ "error": { "code": "InvalidPayload", "message": "x" } });
        assert_eq!(code(azure).as_deref(), Some("InvalidPayload"));
        let openai = json!({ "error": { "message": "Incorrect API key", "type": "invalid_request_error", "code": null } });
        assert_eq!(code(openai).as_deref(), Some("invalid_request_error"));
    }

    #[test]
    fn error_code_never_carries_free_text() {
        let code = |body: Value| provider_error_code(body.to_string().as_bytes());
        assert_eq!(code(json!({ "error": "missing api key" })), None);
        assert_eq!(
            code(json!({ "detail": { "status": "the key sk-abc is wrong" } })),
            None
        );
        assert_eq!(
            code(json!({ "error": { "code": "x".repeat(MAX_ERROR_CODE_CHARS + 1) } })),
            None
        );
        assert_eq!(code(json!({ "error": { "code": "401" } })), None);
        assert_eq!(provider_error_code(b"<html>Unauthorized</html>"), None);
    }

    #[test]
    fn azure_text_joins_the_combined_phrases() {
        let parsed = json!({ "combinedPhrases": [{ "text": "Weather" }], "phrases": [] });
        assert_eq!(
            transcript_text(CloudSttProvider::AzureSpeech, &parsed).as_deref(),
            Some("Weather")
        );
        // Silence comes back as no phrases, which is an empty transcript.
        let silence = json!({ "combinedPhrases": [] });
        assert_eq!(
            transcript_text(CloudSttProvider::AzureSpeech, &silence).as_deref(),
            Some("")
        );
        assert_eq!(
            transcript_text(CloudSttProvider::AzureSpeech, &json!({ "text": "x" })),
            None
        );
    }

    #[test]
    fn azure_is_reported_as_azure_whatever_the_resource_host() {
        let route = route(
            CloudSttProvider::AzureSpeech,
            "https://my-resource.cognitiveservices.azure.com",
        );
        assert_eq!(route.service, ProviderService::Azure);
        assert_eq!(
            super::route(CloudSttProvider::ElevenLabs, ELEVENLABS_BASE_URL).service,
            ProviderService::Elevenlabs
        );
    }

    #[test]
    fn gemini_refuses_audio_past_the_inline_limit_before_sending() {
        let req = CloudSttRequest {
            provider: CloudSttProvider::Gemini,
            base_url: GEMINI_BASE_URL.into(),
            api_key: "k".into(),
            model: "gemini-3.5-transcribe".into(),
            language: None,
            // Eight minutes of 16 kHz audio.
            audio: Arc::new(vec![0.0; 16_000 * 60 * 8]),
            timeout_seconds: 5,
        };
        let error = build_upload(&req, None).err().unwrap();
        assert!(error.contains("inline limit"), "{error}");
    }
}
