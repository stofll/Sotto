//! AI provider implementations (Phase 4 / Batch 3 / PR 3.2).
//!
//! 1:1 Rust port of `ai_processor/_providers.py`. The provider contract
//! is the same — every provider implements `Provider::complete` and
//! returns `(text, info)` where `info` carries usage + error metadata.
//!
//! The HTTP client is `reqwest` (already a dep) with rustls-tls. The
//! secret store (`crate::secret_store`) holds API keys, so the API
//! here is a plain `&str` — the dispatcher (PR 3.2) looks up the key
//! before calling.
//!
//! Error classification mirrors the Python `(_classify_provider_exception,
//! _provider_error_info)` pair: we map transport / HTTP / parse errors
//! to a stable `error_type` enum (`auth_error`, `rate_limit`,
//! `timeout`, `connection_error`, `bad_response`, `provider_failed`)
//! and a `retryable` boolean. The orchestrator (`ai/mod.rs`) uses
//! that to decide whether to retry the call or surface a typed
//! failure to the dispatcher.

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

pub const DEFAULT_TEMPERATURE: f32 = 0.2;
pub const DEFAULT_TIMEOUT_SECS: u64 = 12;
pub const OPENCODE_GO_TIMEOUT_SECS: u64 = 90;
/// Floor for the completion budget: even a one-line dictation must leave a
/// reasoning model room to think before it answers.
pub const MIN_COMPLETION_TOKENS: u32 = 2048;
/// Ceiling, so a pathological input cannot ask a provider for an unbounded
/// completion. Well above what `MAX_INPUT_CHARS` can ever need.
pub const MAX_COMPLETION_TOKENS: u32 = 16_384;
pub const MAX_INPUT_CHARS: usize = 4000;

/// How many completion tokens to allow for tidying up `text`.
///
/// This used to be a flat 2048 for every request, which is a cap on reasoning
/// **plus** answer. The answer to a clean-up task is about as long as the
/// dictation, so a 3.7k-character transcript needs ~1.8k tokens just to come
/// back: a reasoning model spent the budget thinking and stopped mid-sentence,
/// sometimes before emitting any answer at all.
///
/// Cyrillic costs roughly two characters per token in o200k-class vocabularies
/// and Latin rather less, so this over-estimates for English — the safe
/// direction, since `max_tokens` caps a completion rather than reserving it and
/// nobody is billed for headroom they do not use. The ×3 is one part answer and
/// two parts thinking room.
pub fn completion_budget(text: &str) -> u32 {
    let input_tokens = u32::try_from(text.chars().count().div_ceil(2)).unwrap_or(u32::MAX);
    input_tokens
        .saturating_mul(3)
        .clamp(MIN_COMPLETION_TOKENS, MAX_COMPLETION_TOKENS)
}
pub const OPENCODE_GO_BASE_URL: &str = "https://opencode.ai/zen/go/v1";
pub const ANTHROPIC_BASE_URL: &str = "https://api.anthropic.com/v1";
pub const OPENAI_BASE_URL: &str = "https://api.openai.com/v1";
pub const DEFAULT_USER_AGENT: &str = concat!(
    "Sotto/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/stofll/Sotto)"
);

pub const OPENCODE_GO_MESSAGES_MODELS: &[&str] = &["minimax-m2.7", "minimax-m2.5"];
pub const OPENCODE_GO_CHAT_MODELS: &[&str] = &[
    "glm-5.1",
    "glm-5",
    "kimi-k2.6",
    "kimi-k2.5",
    "deepseek-v4-pro",
    "deepseek-v4-flash",
    "mimo-v2-pro",
    "mimo-v2-omni",
    "mimo-v2.5-pro",
    "mimo-v2.5",
    "qwen3.6-plus",
    "qwen3.5-plus",
    "hy3-preview",
];

/// Stable error categories surfaced to the dispatcher. Matches the
/// Python `_classify_provider_exception` taxonomy so the existing
/// History entry `error_type` field round-trips through both
/// runtimes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderErrorType {
    AuthError,
    RateLimit,
    Timeout,
    ConnectionError,
    BadResponse,
    ProviderFailed,
    UnknownProvider,
    MissingApiKey,
}

impl ProviderErrorType {
    pub fn is_retryable(self) -> bool {
        matches!(
            self,
            ProviderErrorType::Timeout
                | ProviderErrorType::ConnectionError
                | ProviderErrorType::BadResponse
        )
    }

    pub fn as_str(self) -> &'static str {
        match self {
            ProviderErrorType::AuthError => "auth_error",
            ProviderErrorType::RateLimit => "rate_limit",
            ProviderErrorType::Timeout => "timeout",
            ProviderErrorType::ConnectionError => "connection_error",
            ProviderErrorType::BadResponse => "bad_response",
            ProviderErrorType::ProviderFailed => "provider_failed",
            ProviderErrorType::UnknownProvider => "unknown_provider",
            ProviderErrorType::MissingApiKey => "missing_api_key",
        }
    }
}

/// Information about a failed or successful provider call. Mirrors
/// the Python `info` dict shape: `message`, `error_type`,
/// `retryable`, `elapsed_seconds`, optional `http_status`,
/// `response_snippet`, `usage`.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ProviderInfo {
    pub message: String,
    pub error_type: Option<ProviderErrorType>,
    pub retryable: bool,
    pub elapsed_seconds: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_status: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_snippet: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<UsageInfo>,
}

impl ProviderInfo {
    pub fn success(
        text: impl Into<String>,
        usage: Option<UsageInfo>,
        elapsed_seconds: f64,
    ) -> Self {
        Self {
            message: text.into(),
            elapsed_seconds,
            usage,
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct UsageInfo {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub total_tokens: u32,
}

pub trait Provider: Send + Sync {
    fn name(&self) -> &'static str;
    fn complete<'a>(&'a self, system_prompt: &'a str, text: &'a str) -> CompletionFuture<'a>;
}

/// Boxed future returned by `Provider::complete`. Aliased so
/// `clippy::type_complexity` stays quiet and the trait signature
/// stays scannable.
pub type CompletionFuture<'a> = std::pin::Pin<
    Box<
        dyn std::future::Future<Output = Result<(String, ProviderInfo), ProviderError>> + Send + 'a,
    >,
>;

#[derive(Debug, Clone)]
pub struct ProviderError {
    pub kind: ProviderErrorType,
    pub message: String,
    pub http_status: Option<u16>,
    pub response_snippet: Option<String>,
    pub retryable: bool,
}

impl std::fmt::Display for ProviderError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.message)
    }
}

impl std::error::Error for ProviderError {}

impl ProviderError {
    pub fn new(kind: ProviderErrorType, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            http_status: None,
            response_snippet: None,
            retryable: kind.is_retryable(),
        }
    }
}

// ---------------------------------------------------------------------------
// Concrete providers
// ---------------------------------------------------------------------------

pub struct AnthropicProvider {
    api_key: String,
    model: String,
    base_url: String,
    timeout: Duration,
    max_tokens: Option<u32>,
}

impl AnthropicProvider {
    pub fn new(
        api_key: impl Into<String>,
        model: impl Into<String>,
        base_url: Option<String>,
        timeout: Option<Duration>,
        max_tokens: Option<u32>,
    ) -> Self {
        Self {
            api_key: api_key.into(),
            model: model.into(),
            base_url: base_url
                .map(|value| value.trim_end_matches('/').to_string())
                .unwrap_or_else(|| ANTHROPIC_BASE_URL.to_string()),
            timeout: timeout.unwrap_or(Duration::from_secs(DEFAULT_TIMEOUT_SECS)),
            // Same contract as OpenAI's: `None` sizes the budget to the
            // request. Anthropic requires the field, so it is always sent.
            max_tokens,
        }
    }
}

impl Provider for AnthropicProvider {
    fn name(&self) -> &'static str {
        "anthropic"
    }

    fn complete<'a>(
        &'a self,
        system_prompt: &'a str,
        text: &'a str,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<(String, ProviderInfo), ProviderError>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async move {
            let body = serde_json::json!({
                "model": self.model,
                "max_tokens": self.max_tokens.unwrap_or_else(|| completion_budget(text)),
                "temperature": DEFAULT_TEMPERATURE,
                "system": system_prompt,
                "messages": [{"role": "user", "content": text}],
            });
            let request = build_request(
                reqwest::Method::POST,
                &format!("{}/messages", self.base_url),
                Some(&body),
                &[
                    ("x-api-key", self.api_key.as_str()),
                    ("anthropic-version", "2023-06-01"),
                ],
            );
            send_request(
                request,
                self.timeout,
                "anthropic",
                &self.model,
                "content[0].text",
            )
            .await
        })
    }
}

pub struct OpenAIProvider {
    api_key: String,
    model: String,
    base_url: String,
    timeout: Duration,
    max_tokens: Option<u32>,
}

impl OpenAIProvider {
    pub fn new(
        api_key: impl Into<String>,
        model: impl Into<String>,
        base_url: Option<String>,
        timeout: Option<Duration>,
        max_tokens: Option<u32>,
    ) -> Self {
        Self {
            api_key: api_key.into(),
            model: model.into(),
            base_url: base_url
                .map(|value| value.trim_end_matches('/').to_string())
                .unwrap_or_else(|| OPENAI_BASE_URL.to_string()),
            timeout: timeout.unwrap_or(Duration::from_secs(DEFAULT_TIMEOUT_SECS)),
            // `None` means "size it to the request" — see `completion_budget`.
            // An explicit value stays an explicit hard cap.
            max_tokens,
        }
    }
}

impl Provider for OpenAIProvider {
    fn name(&self) -> &'static str {
        "openai"
    }

    fn complete<'a>(
        &'a self,
        system_prompt: &'a str,
        text: &'a str,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<(String, ProviderInfo), ProviderError>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async move {
            let mut payload = serde_json::json!({
                "model": self.model,
                "messages": [
                    {"role": "system", "content": system_prompt},
                    {"role": "user", "content": text},
                ],
                "temperature": DEFAULT_TEMPERATURE,
            });
            payload["max_tokens"] =
                serde_json::json!(self.max_tokens.unwrap_or_else(|| completion_budget(text)));
            let request = build_request(
                reqwest::Method::POST,
                &format!("{}/chat/completions", self.base_url),
                Some(&payload),
                &[("Authorization", &format!("Bearer {}", self.api_key))],
            );
            send_request(
                request,
                self.timeout,
                "openai",
                &self.model,
                "choices[0].message.content",
            )
            .await
        })
    }
}

pub struct GeminiProvider {
    api_key: String,
    model: String,
    timeout: Duration,
}

impl GeminiProvider {
    pub fn new(
        api_key: impl Into<String>,
        model: impl Into<String>,
        timeout: Option<Duration>,
    ) -> Self {
        Self {
            api_key: api_key.into(),
            model: model.into(),
            timeout: timeout.unwrap_or(Duration::from_secs(DEFAULT_TIMEOUT_SECS)),
        }
    }
}

impl Provider for GeminiProvider {
    fn name(&self) -> &'static str {
        "gemini"
    }

    fn complete<'a>(
        &'a self,
        system_prompt: &'a str,
        text: &'a str,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<(String, ProviderInfo), ProviderError>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async move {
            let body = serde_json::json!({
                "system_instruction": {"parts": [{"text": system_prompt}]},
                "contents": [{"role": "user", "parts": [{"text": text}]}],
                "generationConfig": {"temperature": DEFAULT_TEMPERATURE},
            });
            let url = format!(
                "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent",
                urlencoding(&self.model)
            );
            let request = build_request(
                reqwest::Method::POST,
                &url,
                Some(&body),
                &[("x-goog-api-key", self.api_key.as_str())],
            );
            send_request(
                request,
                self.timeout,
                "gemini",
                &self.model,
                "candidates[0].content.parts[0].text",
            )
            .await
        })
    }
}

pub struct OpenCodeGoProvider {
    api_key: String,
    model: String,
    base_url: String,
    timeout: Duration,
}

impl OpenCodeGoProvider {
    pub fn new(
        api_key: impl Into<String>,
        model: impl Into<String>,
        base_url: Option<String>,
        timeout: Option<Duration>,
    ) -> Self {
        Self {
            api_key: api_key.into(),
            model: normalise_model(model.into()),
            base_url: base_url
                .map(|value| value.trim_end_matches('/').to_string())
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| OPENCODE_GO_BASE_URL.to_string()),
            timeout: timeout.unwrap_or(Duration::from_secs(OPENCODE_GO_TIMEOUT_SECS)),
        }
    }
}

impl Provider for OpenCodeGoProvider {
    fn name(&self) -> &'static str {
        "opencode-go"
    }

    fn complete<'a>(
        &'a self,
        system_prompt: &'a str,
        text: &'a str,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<(String, ProviderInfo), ProviderError>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async move {
            // OpenCode Go routes to either the Anthropic Messages API
            // (for M2.7/M2.5) or the OpenAI chat/completions API. The
            // Python `OpenCodeGoProvider` does the same dispatch.
            if OPENCODE_GO_MESSAGES_MODELS.contains(&self.model.as_str()) {
                let inner = AnthropicProvider::new(
                    self.api_key.clone(),
                    self.model.clone(),
                    Some(self.base_url.clone()),
                    Some(self.timeout),
                    // Was a flat 2048 here too; the inner provider now sizes
                    // the budget to the dictation.
                    None,
                );
                return inner.complete(system_prompt, text).await;
            }
            if !OPENCODE_GO_CHAT_MODELS.contains(&self.model.as_str()) {
                log::info!(
                    "OpenCode Go: unknown model id '{}'; falling back to chat/completions",
                    self.model
                );
            }
            let inner = OpenAIProvider::new(
                self.api_key.clone(),
                self.model.clone(),
                Some(self.base_url.clone()),
                Some(self.timeout),
                None,
            );
            inner.complete(system_prompt, text).await
        })
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a `reqwest::RequestBuilder` for the given method / URL / body /
/// auth headers. Centralised so every provider gets the same User-Agent
/// and Accept headers.
///
/// The builder comes from `shared_client` rather than a fresh
/// `Client::new()`. A `Client` is a connection pool and a TLS config, not a
/// handle — building one per request meant constructing a connector that
/// was then thrown away. `send_request` already executed through the shared
/// client, so provider calls were only paying for it; `models.rs` sends
/// through the builder's own client, so its model-list requests really did
/// open unpooled connections.
pub(super) fn build_request(
    method: reqwest::Method,
    url: &str,
    body: Option<&serde_json::Value>,
    auth: &[(&str, &str)],
) -> reqwest::RequestBuilder {
    let mut builder = shared_client()
        .request(method, url)
        .header(reqwest::header::USER_AGENT, DEFAULT_USER_AGENT)
        .header(reqwest::header::ACCEPT, "application/json")
        .header(reqwest::header::CONTENT_TYPE, "application/json");
    for (key, value) in auth {
        builder = builder.header(*key, *value);
    }
    if let Some(value) = body {
        builder = builder.json(value);
    }
    builder
}

/// Send a provider request, parse the response, and surface a typed
/// error. The `text_field` is the JSON path to the answer text
/// (provider-specific).
async fn send_request(
    builder: reqwest::RequestBuilder,
    timeout: Duration,
    provider_name: &str,
    model: &str,
    text_field: &str,
) -> Result<(String, ProviderInfo), ProviderError> {
    let started = Instant::now();
    let request = builder
        .timeout(timeout)
        .build()
        .map_err(|error| classify_request_error(error, started.elapsed()))?;
    let response = match shared_client().execute(request).await {
        Ok(value) => value,
        Err(error) => {
            return Err(classify_execution_error(error, started.elapsed()));
        }
    };
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    let elapsed = started.elapsed().as_secs_f64();
    if !status.is_success() {
        // `message` stays snippet-free: it is what reaches the overlay, the
        // logs and the AI status. The body goes only into `response_snippet`,
        // which nothing persists and only the Settings test surfaces.
        let message = format!("HTTP {status} {}", status.canonical_reason().unwrap_or(""));
        let kind = classify_http_status(status.as_u16());
        return Err(ProviderError {
            kind,
            message,
            http_status: Some(status.as_u16()),
            response_snippet: snippet_for_diagnostics(&body),
            retryable: kind.is_retryable(),
        });
    }
    let json: serde_json::Value = match serde_json::from_str(&body) {
        Ok(value) => value,
        Err(error) => {
            log::warn!("Provider {provider_name}/{model} returned invalid JSON: {error}");
            return Err(ProviderError {
                kind: ProviderErrorType::BadResponse,
                message: format!("Provider returned invalid JSON: {error}"),
                http_status: Some(status.as_u16()),
                response_snippet: snippet_for_diagnostics(&body),
                retryable: true,
            });
        }
    };
    let text = match extract_text(&json, text_field) {
        Some(value) => value,
        None => {
            log::warn!("Provider {provider_name}/{model} response missing {text_field}");
            return Err(ProviderError {
                kind: ProviderErrorType::BadResponse,
                message: format!("{provider_name} response missing {text_field}"),
                http_status: Some(status.as_u16()),
                response_snippet: snippet_for_diagnostics(&body),
                retryable: true,
            });
        }
    };
    let usage = extract_usage(&json, provider_name);
    let info = ProviderInfo {
        message: text.clone(),
        error_type: None,
        retryable: false,
        elapsed_seconds: elapsed,
        http_status: Some(status.as_u16()),
        response_snippet: None,
        usage,
    };
    log::info!("Provider {provider_name}/{model} responded in {elapsed:.2}s");
    Ok((text, info))
}

/// Process-wide shared `reqwest::Client`. We re-use one client
/// across all provider calls so connection pooling kicks in.
fn shared_client() -> &'static reqwest::Client {
    use once_cell::sync::Lazy;
    static CLIENT: Lazy<reqwest::Client> = Lazy::new(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .expect("build reqwest client")
    });
    &CLIENT
}

fn classify_http_status(status: u16) -> ProviderErrorType {
    match status {
        401 | 403 => ProviderErrorType::AuthError,
        429 => ProviderErrorType::RateLimit,
        500..=599 => ProviderErrorType::ConnectionError,
        _ => ProviderErrorType::ProviderFailed,
    }
}

fn classify_request_error(error: reqwest::Error, elapsed: Duration) -> ProviderError {
    if error.is_timeout() {
        return ProviderError::new(
            ProviderErrorType::Timeout,
            format!("request timeout: {error}"),
        );
    }
    if error.is_connect() {
        return ProviderError::new(
            ProviderErrorType::ConnectionError,
            format!("connection error: {error}"),
        );
    }
    ProviderError::new(
        ProviderErrorType::ProviderFailed,
        format!("request build failed: {error}"),
    )
    .with_elapsed(elapsed)
}

fn classify_execution_error(error: reqwest::Error, elapsed: Duration) -> ProviderError {
    if error.is_timeout() {
        return ProviderError::new(
            ProviderErrorType::Timeout,
            format!("request timeout: {error}"),
        );
    }
    if error.is_connect() {
        return ProviderError::new(
            ProviderErrorType::ConnectionError,
            format!("connection error: {error}"),
        );
    }
    if error.is_decode() {
        return ProviderError::new(
            ProviderErrorType::BadResponse,
            format!("decode error: {error}"),
        );
    }
    ProviderError::new(
        ProviderErrorType::ProviderFailed,
        format!("request failed: {error}"),
    )
    .with_elapsed(elapsed)
}

impl ProviderError {
    fn with_elapsed(mut self, elapsed: Duration) -> Self {
        self.message = format!("{} (elapsed {:.2}s)", self.message, elapsed.as_secs_f64());
        self
    }
}

fn extract_text(value: &serde_json::Value, path: &str) -> Option<String> {
    let mut current = value;
    // Support both dotted (`a.b.c`) and bracketed (`a[0].b`)
    // navigation in the same path. We tokenise the path into a
    // sequence of segments where each segment is either a field
    // name or an array index.
    for segment in tokenise_path(path) {
        match segment {
            PathSegment::Field(name) => current = current.get(name)?,
            PathSegment::Index(index) => current = current.get(index)?,
        }
    }
    if let Some(text) = current.as_str() {
        return Some(text.to_string());
    }
    // Tolerate non-string leaves by stringifying numbers/booleans
    // — defensive against provider responses that ship a typed
    // value where we expect a string. Returns `None` for objects
    // and arrays (the caller should not see those at the leaf).
    Some(match current {
        serde_json::Value::Number(value) => value.to_string(),
        serde_json::Value::Bool(value) => value.to_string(),
        serde_json::Value::Null => "".to_string(),
        _ => return None,
    })
}

enum PathSegment {
    Field(String),
    Index(usize),
}

fn tokenise_path(path: &str) -> Vec<PathSegment> {
    let mut segments = Vec::new();
    for raw in path.split('.') {
        let mut rest = raw;
        // Pull off the field-name prefix (everything before the
        // first `[`).
        if let Some(open) = rest.find('[') {
            let field = &rest[..open];
            if !field.is_empty() {
                segments.push(PathSegment::Field(field.to_string()));
            }
            rest = &rest[open..];
            // Repeated `[i]` segments.
            while let Some(close) = rest.find(']') {
                let body = &rest[1..close];
                if let Ok(index) = body.parse::<usize>() {
                    segments.push(PathSegment::Index(index));
                } else {
                    // Malformed index — stop parsing the path
                    // segment; the next `get` will fail safely.
                    break;
                }
                rest = &rest[close + 1..];
                if let Some(next_open) = rest.find('[') {
                    rest = &rest[next_open..];
                } else {
                    break;
                }
            }
        } else if rest.is_empty() {
            // Skip empty segments produced by trailing dots or
            // double-dots in the path.
        } else if let Ok(index) = rest.parse::<usize>() {
            // A bare numeric segment is an array index — `0` is
            // equivalent to `[0]`. Without this, `candidates.0`
            // would try to look up a field literally named "0".
            segments.push(PathSegment::Index(index));
        } else {
            segments.push(PathSegment::Field(rest.to_string()));
        }
    }
    segments
}

fn extract_usage(value: &serde_json::Value, provider: &str) -> Option<UsageInfo> {
    let usage = value.get("usage")?;
    let obj = usage.as_object()?;
    let pick = |openai_key: &str, anthropic_key: &str, _gemini_key: &str| {
        let get = |key: &str| {
            obj.get(key)
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0) as u32
        };
        match provider {
            "anthropic" => (get(anthropic_key), get("output_tokens")),
            "gemini" => (get("promptTokenCount"), get("candidatesTokenCount")),
            _ => (get(openai_key), get("completion_tokens")),
        }
    };
    let (input, output) = pick("prompt_tokens", "input_tokens", "promptTokenCount");
    let total = obj
        .get("total_tokens")
        .or_else(|| obj.get("totalTokenCount"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or((input as u64) + (output as u64)) as u32;
    if input == 0 && output == 0 && total == 0 {
        return None;
    }
    Some(UsageInfo {
        input_tokens: input,
        output_tokens: output,
        total_tokens: total,
    })
}

/// A bounded, whitespace-collapsed excerpt of a provider response body.
///
/// Diagnostics only. A provider response can echo the prompt, and with it the
/// transcription — so this must never reach a log line, the stats database or
/// telemetry. Its one destination is the `response_snippet` field that the
/// Settings provider test renders, on a screen the user opened themselves to
/// find out what the provider actually replied.
fn snippet_for_diagnostics(body: &str) -> Option<String> {
    let compact: String = body.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.is_empty() {
        return None;
    }
    Some(compact.chars().take(500).collect())
}

/// Normalise a user-supplied OpenCode Go model name. Strips a
/// leading `opencode-go/` or `opencode/` prefix.
fn normalise_model(value: String) -> String {
    let lowered = value.to_lowercase();
    for prefix in ["opencode-go/", "opencode/"] {
        if let Some(stripped) = lowered.strip_prefix(prefix) {
            return stripped.trim().to_string();
        }
    }
    lowered
}

/// Minimal URL-encoding for the Gemini model name (slashes, dots).
/// We avoid pulling in a `urlencoding` crate by inlining a tiny
/// encoder for the only characters that need escaping in a model
/// name (`/`, `.`).
fn urlencoding(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            _ => {
                out.push('%');
                out.push_str(&format!("{:02X}", byte));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_type_retryable_matches_python() {
        assert!(ProviderErrorType::Timeout.is_retryable());
        assert!(ProviderErrorType::ConnectionError.is_retryable());
        assert!(ProviderErrorType::BadResponse.is_retryable());
        assert!(!ProviderErrorType::AuthError.is_retryable());
        assert!(!ProviderErrorType::RateLimit.is_retryable());
        assert!(!ProviderErrorType::ProviderFailed.is_retryable());
    }

    #[test]
    fn http_status_classification_matches_python() {
        assert_eq!(classify_http_status(401), ProviderErrorType::AuthError);
        assert_eq!(classify_http_status(403), ProviderErrorType::AuthError);
        assert_eq!(classify_http_status(429), ProviderErrorType::RateLimit);
        assert_eq!(
            classify_http_status(500),
            ProviderErrorType::ConnectionError
        );
        assert_eq!(
            classify_http_status(502),
            ProviderErrorType::ConnectionError
        );
        assert_eq!(
            classify_http_status(599),
            ProviderErrorType::ConnectionError
        );
        assert_eq!(classify_http_status(400), ProviderErrorType::ProviderFailed);
        assert_eq!(classify_http_status(404), ProviderErrorType::ProviderFailed);
    }

    #[test]
    fn extract_text_navigates_nested_paths() {
        let value = serde_json::json!({
            "candidates": [{
                "content": {"parts": [{"text": "Hello"}]}
            }]
        });
        assert_eq!(
            extract_text(&value, "candidates.0.content.parts.0.text"),
            Some("Hello".to_string())
        );
        // Also support the bracketed form, which the provider
        // contracts use (`content[0].text`, `choices[0].message.content`).
        assert_eq!(
            extract_text(&value, "candidates[0].content.parts[0].text"),
            Some("Hello".to_string())
        );
        // Single-index path on a plain array.
        let value = serde_json::json!([{"x": 1}, {"x": 2}]);
        assert_eq!(extract_text(&value, "[1].x"), Some("2".to_string()));
    }

    #[test]
    fn completion_budget_covers_a_long_dictation() {
        // The transcript that exposed the bug: 3758 characters in, and the
        // answer alone needs roughly 1.8k tokens to come back.
        let long = "я".repeat(3758);
        let budget = completion_budget(&long);
        assert!(
            budget > 5000,
            "3758 chars must buy room for the answer plus thinking, got {budget}"
        );
        assert!(budget <= MAX_COMPLETION_TOKENS);
    }

    #[test]
    fn completion_budget_never_drops_below_the_floor() {
        assert_eq!(completion_budget(""), MIN_COMPLETION_TOKENS);
        assert_eq!(completion_budget("коротко"), MIN_COMPLETION_TOKENS);
    }

    #[test]
    fn completion_budget_is_capped() {
        assert_eq!(
            completion_budget(&"я".repeat(MAX_INPUT_CHARS * 100)),
            MAX_COMPLETION_TOKENS
        );
    }

    #[test]
    fn diagnostics_snippet_collapses_whitespace_and_caps_length() {
        assert_eq!(
            snippet_for_diagnostics("  hello   world  ").as_deref(),
            Some("hello world")
        );
        let long = "a".repeat(1000);
        assert_eq!(
            snippet_for_diagnostics(&long).as_deref(),
            Some("a".repeat(500).as_str())
        );
    }

    #[test]
    fn an_empty_body_produces_no_snippet() {
        // `None`, not `Some("")`: the UI decides whether to render the block
        // by the presence of the field.
        assert_eq!(snippet_for_diagnostics(""), None);
        assert_eq!(snippet_for_diagnostics("   \n\t "), None);
    }

    #[test]
    fn urlencoding_escapes_unsafe_chars() {
        assert_eq!(urlencoding("gemini-2.5-flash"), "gemini-2.5-flash");
        assert_eq!(urlencoding("models/x"), "models%2Fx");
    }

    #[test]
    fn normalise_opencode_go_model_strips_prefixes() {
        assert_eq!(
            normalise_model("opencode-go/qwen3.5-plus".to_string()),
            "qwen3.5-plus"
        );
        assert_eq!(normalise_model("OPENCODE-GO/GLM-5".to_string()), "glm-5");
        assert_eq!(normalise_model("qwen3.5-plus".to_string()), "qwen3.5-plus");
    }

    #[test]
    fn opencode_go_model_routing_classifies_correctly() {
        // M2.7 / M2.5 use the Anthropic Messages contract.
        assert!(OPENCODE_GO_MESSAGES_MODELS.contains(&"minimax-m2.7"));
        assert!(OPENCODE_GO_MESSAGES_MODELS.contains(&"minimax-m2.5"));
        // Everything else routes through chat/completions.
        assert!(OPENCODE_GO_CHAT_MODELS.contains(&"qwen3.5-plus"));
        assert!(OPENCODE_GO_CHAT_MODELS.contains(&"glm-5.1"));
    }

    #[test]
    fn mask_key_pattern_works_on_long_inputs() {
        // 12-char key: head 6 + … + tail 4.
        let chars: Vec<char> = "abcdefghijkl".chars().collect();
        let head: String = chars[..6].iter().collect();
        let tail: String = chars[8..].iter().collect();
        assert_eq!(format!("{head}…{tail}"), "abcdef…ijkl");
    }

    #[test]
    fn error_type_as_str_matches_python() {
        assert_eq!(ProviderErrorType::AuthError.as_str(), "auth_error");
        assert_eq!(ProviderErrorType::RateLimit.as_str(), "rate_limit");
        assert_eq!(ProviderErrorType::Timeout.as_str(), "timeout");
        assert_eq!(
            ProviderErrorType::ConnectionError.as_str(),
            "connection_error"
        );
        assert_eq!(ProviderErrorType::BadResponse.as_str(), "bad_response");
        assert_eq!(
            ProviderErrorType::ProviderFailed.as_str(),
            "provider_failed"
        );
        assert_eq!(
            ProviderErrorType::UnknownProvider.as_str(),
            "unknown_provider"
        );
        assert_eq!(ProviderErrorType::MissingApiKey.as_str(), "missing_api_key");
    }

    #[test]
    fn provider_info_success_sets_every_field() {
        let usage = UsageInfo {
            input_tokens: 1,
            output_tokens: 2,
            total_tokens: 3,
        };
        let info = ProviderInfo::success("answer".to_string(), Some(usage), 1.5);
        assert_eq!(info.message, "answer");
        assert_eq!(info.error_type, None);
        assert!(!info.retryable, "a success is not retryable");
        assert_eq!(info.elapsed_seconds, 1.5);
        let usage = info.usage.expect("usage must be carried through");
        assert_eq!(usage.total_tokens, 3);
    }

    #[test]
    fn extract_text_stringifies_non_string_leaves() {
        assert_eq!(
            extract_text(&serde_json::json!({ "x": true }), "x"),
            Some("true".to_string())
        );
        assert_eq!(
            extract_text(&serde_json::json!({ "x": null }), "x"),
            Some("".to_string())
        );
    }

    #[test]
    fn extract_usage_reads_openai_shape() {
        let json = serde_json::json!({ "usage": { "prompt_tokens": 4, "completion_tokens": 2, "total_tokens": 6 } });
        let usage = extract_usage(&json, "openai").expect("usage");
        assert_eq!(usage.input_tokens, 4);
        assert_eq!(usage.output_tokens, 2);
        assert_eq!(usage.total_tokens, 6);
    }

    #[test]
    fn extract_usage_reads_anthropic_shape_and_sums_total() {
        let json = serde_json::json!({ "usage": { "input_tokens": 12, "output_tokens": 7 } });
        let usage = extract_usage(&json, "anthropic").expect("usage");
        assert_eq!(usage.input_tokens, 12);
        assert_eq!(usage.output_tokens, 7);
        assert_eq!(
            usage.total_tokens, 19,
            "missing total must be input + output"
        );
    }

    #[test]
    fn extract_usage_reads_gemini_shape() {
        let json = serde_json::json!({ "usage": { "promptTokenCount": 10, "candidatesTokenCount": 3, "totalTokenCount": 13 } });
        let usage = extract_usage(&json, "gemini").expect("usage");
        assert_eq!(usage.input_tokens, 10);
        assert_eq!(usage.output_tokens, 3);
        assert_eq!(usage.total_tokens, 13);
    }

    #[test]
    fn extract_usage_all_zero_is_none() {
        let json = serde_json::json!({ "usage": { "prompt_tokens": 0, "completion_tokens": 0, "total_tokens": 0 } });
        assert!(extract_usage(&json, "openai").is_none());
    }

    #[test]
    fn extract_usage_partial_counts_are_kept() {
        // Only an ALL-zero usage is treated as absent; a partial count is
        // real data and must come back as Some.
        let json = serde_json::json!({ "usage": { "prompt_tokens": 5, "completion_tokens": 0, "total_tokens": 0 } });
        let usage = extract_usage(&json, "openai").expect("partial usage is not none");
        assert_eq!(usage.input_tokens, 5);
        assert_eq!(usage.output_tokens, 0);
    }
}
