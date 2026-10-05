//! AI processing orchestrator.
//!
//! Mirrors `ai_processor/_step.py::ai_process_text_with_status`.
//! The dispatch flow is:
//!
//! 1. Decide whether AI is enabled (pipeline mode + duration + key).
//! 2. Look up the key from `secret_store`.
//! 3. Render the editable system prompt and send the source text separately
//!    in a `<dictation>` envelope.
//! 4. Call the provider with at most 2 attempts, applying a fixed
//!    back-off between them for transient errors.
//! 5. Strip reasoning blocks, detect meta-noop responses, and
//!    surface a typed `AiStatus` to the caller.
//!
//! The orchestrator is intentionally a free function (not a trait)
//! because the only legitimate caller is the dispatcher / Tauri
//! command layer; tests call it directly.

use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;

use super::fidelity::{altered_meaning, dropped_too_much, kept_word_ratio, repeats_context};
use super::long_text;
use super::model_params::{OutputLimit, ReasoningMode};
use super::providers::{
    answer_tokens, AnthropicProvider, GeminiProvider, OpenAIProvider, OpenCodeGoProvider, Provider,
    ProviderError, ProviderErrorType, CHARS_PER_TOKEN,
};
#[cfg(test)]
use super::providers::{CompletionFuture, ProviderInfo};
use super::reasoning::{is_meta_noop_response, strip_reasoning};

const MAX_PROVIDER_ATTEMPTS: u32 = 2;
const RETRY_BACKOFF: Duration = Duration::from_millis(300);
const TRANSIENT_ERRORS: &[ProviderErrorType] = &[
    ProviderErrorType::Timeout,
    ProviderErrorType::ConnectionError,
    ProviderErrorType::BadResponse,
];

const SKIPPED_REASON_BY_ERROR_TYPE: &[(&str, &str)] = &[
    ("auth_error", "provider_auth_error"),
    ("rate_limit", "provider_quota_or_rate_limit"),
    ("timeout", "provider_timeout"),
    ("connection_error", "provider_connection_error"),
    ("bad_response", "provider_bad_response"),
];

#[derive(Debug, Clone, Default, Serialize)]
pub struct AiStatus {
    #[serde(skip)]
    pub telemetry_service: Option<crate::telemetry::ProviderService>,
    pub mode: String,
    pub provider: String,
    pub model: String,
    pub profile_id: String,
    pub profile_name: String,
    pub api_key_ref: String,
    pub audio_duration_seconds: Option<f64>,
    pub min_duration_seconds: f64,
    pub enabled: bool,
    pub attempted: bool,
    pub used: bool,
    pub fallback: bool,
    pub skipped_reason: String,
    pub timeout_seconds: u64,
    pub attempt_timeout_seconds: u64,
    pub attempts: u32,
    pub elapsed_seconds: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<super::providers::UsageInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_status: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_snippet: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_length: Option<usize>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub provider_attempts: Vec<ProviderAttemptInfo>,
    /// The answer arrived after the text had already been pasted, and was
    /// written into the history entry afterwards.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub late: bool,
    /// A long text tidied in parts: how many there were and how many came
    /// back from the model. `used` is true when at least one did.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parts: Option<PartsSummary>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct PartsSummary {
    pub total: usize,
    pub used: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProviderAttemptInfo {
    pub attempt: u32,
    pub elapsed_seconds: f64,
    pub error_type: String,
    pub provider_error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_status: Option<u16>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AiConfig {
    pub pipeline_mode: String,
    pub provider: String,
    pub model: String,
    pub profile_id: String,
    pub profile_name: String,
    pub api_key_ref: String,
    pub system_prompt: String,
    pub language: String,
    pub base_url: Option<String>,
    pub audio_duration_seconds: Option<f64>,
    pub llm_min_duration_seconds: f64,
    pub llm_timeout_seconds: u64,
    pub late_answer: LateAnswerMode,
    pub reasoning: ReasoningMode,
    pub output_limit: OutputLimit,
    /// A file transcription, which can be cancelled from its panel and has
    /// no overlay waiting on it: a long text in parts may take up to
    /// [`FILE_PARTS_MAX_SECS`]. Everything else keeps the dictation's budget.
    pub may_run_long: bool,
}

/// What happens to an answer that arrives after the timeout, when the local
/// text has already been pasted (`llm_late_answer` in `ai_processing`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LateAnswerMode {
    /// Write it into the history entry and say so in the overlay.
    Notify,
    /// Write it into the history entry without a word.
    Silent,
    /// Cancel the request at the timeout, as before late answers existed.
    Off,
}

impl LateAnswerMode {
    fn parse(value: Option<&str>) -> Self {
        match value {
            Some("silent") => Self::Silent,
            Some("off") => Self::Off,
            _ => Self::Notify,
        }
    }
}

impl AiConfig {
    /// Build an `AiConfig` from an `ai_processing` config object with no
    /// live-recording context: `profile_*` are empty and
    /// `audio_duration_seconds` is `None`, so the min-duration gate is
    /// skipped. This is exactly the shape the history "retry AI" path
    /// needs; a live path that has a recording can set
    /// `audio_duration_seconds` on the returned value.
    ///
    /// Defaults: `pipeline_mode` falls back to `"local"`, the app-wide
    /// default for a config without it, `llm_timeout_seconds` to `12`, string
    /// fields to empty, and `system_prompt` accepts the legacy
    /// `format_prompt` alias.
    pub fn from_ai_processing(v: &serde_json::Value) -> Self {
        let s = |key: &str| {
            v.get(key)
                .and_then(serde_json::Value::as_str)
                .unwrap_or("")
                .to_string()
        };
        AiConfig {
            pipeline_mode: v
                .get("pipeline_mode")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("local")
                .to_string(),
            provider: s("provider"),
            model: s("model"),
            profile_id: String::new(),
            profile_name: String::new(),
            api_key_ref: s("api_key_ref"),
            system_prompt: v
                .get("system_prompt")
                .or_else(|| v.get("format_prompt"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or("")
                .to_string(),
            language: s("language"),
            // Blank is absent: the settings UI persists `base_url: ""` for
            // every provider without a URL field of its own.
            base_url: v
                .get("base_url")
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string),
            audio_duration_seconds: None,
            llm_min_duration_seconds: v
                .get("llm_min_duration_seconds")
                .and_then(serde_json::Value::as_f64)
                .unwrap_or(0.0),
            llm_timeout_seconds: v
                .get("llm_timeout_seconds")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(12),
            late_answer: LateAnswerMode::parse(
                v.get("llm_late_answer").and_then(serde_json::Value::as_str),
            ),
            reasoning: ReasoningMode::parse(
                v.get("llm_reasoning").and_then(serde_json::Value::as_str),
            ),
            output_limit: OutputLimit::parse(v.get("llm_output_limit")),
            may_run_long: false,
        }
    }
}

pub struct CallOutcome {
    pub text: String,
    pub status: AiStatus,
    /// The request that ran out of time but was not cancelled. Only a live
    /// dictation waits for it; anyone else drops it, which cancels it.
    pub late: Option<LateAnswer>,
}

/// How long a request may keep running after its timeout, while the local
/// text is already in the user's window.
pub const LATE_ANSWER_GRACE: Duration = Duration::from_secs(5 * 60);

/// A provider request still running when its time ran out.
///
/// The timeout used to cancel it, so a model that needed a second longer was
/// paid for and its answer thrown away. Now the dictation is pasted without
/// it as before, and the answer, if it comes within [`LATE_ANSWER_GRACE`],
/// is judged exactly like one on time and stored in the history entry.
pub struct LateAnswer {
    request: InFlight,
    source_text: String,
    config: AiConfig,
    status: AiStatus,
    started: tokio::time::Instant,
    give_up_at: tokio::time::Instant,
}

impl LateAnswer {
    pub fn mode(&self) -> LateAnswerMode {
        self.config.late_answer
    }

    /// The answer, or `None` when the request failed or outlived its grace.
    pub async fn wait(mut self) -> Option<CallOutcome> {
        let joined = tokio::time::timeout_at(self.give_up_at, &mut self.request.0)
            .await
            .ok()?;
        let (raw, info) = joined.ok()?.ok()?;
        let mut status = std::mem::take(&mut self.status);
        status.late = true;
        status.fallback = false;
        status.skipped_reason.clear();
        status.error_type = None;
        status.provider_error = None;
        status.http_status = info.http_status;
        status.usage = info.usage;
        status.elapsed_seconds = self.started.elapsed().as_secs_f64();
        Some(judge_answer(
            &self.source_text,
            &self.config,
            raw,
            info.response_snippet,
            status,
        ))
    }
}

type ProviderReply = Result<(String, super::providers::ProviderInfo), ProviderError>;

/// A provider request on a task of its own, cancelled when dropped — so no
/// way out of the retry loop leaves one running unseen.
struct InFlight(tokio::task::JoinHandle<ProviderReply>);

impl InFlight {
    fn spawn(provider: &Arc<dyn Provider>, system_prompt: &str, text: &str) -> Self {
        let provider = Arc::clone(provider);
        let (system_prompt, text) = (system_prompt.to_string(), text.to_string());
        Self(tokio::spawn(async move {
            provider.complete(&system_prompt, &text).await
        }))
    }
}

impl Drop for InFlight {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Build a "skipped" `CallOutcome`: the LLM step did not run, the
/// original `text` passes through unchanged, and `status` records why.
/// Extracted from the five early-return sites in
/// `ai_process_text_with_status` that all built the same shape.
fn skipped(text: &str, mut status: AiStatus, reason: &str) -> CallOutcome {
    status.skipped_reason = reason.to_string();
    CallOutcome {
        text: text.to_string(),
        status,
        late: None,
    }
}

pub async fn ai_process_text_with_status(
    text: &str,
    config: &AiConfig,
    api_key: Option<&str>,
) -> CallOutcome {
    let status = AiStatus {
        telemetry_service: Some(match config.provider.as_str() {
            // The Gemini adapter ignores `base_url` and always calls the one
            // Google endpoint.
            "gemini" => crate::telemetry::ProviderService::Gemini,
            provider => crate::telemetry::provider_service(&super::providers::effective_base_url(
                provider,
                config.base_url.as_deref(),
            )),
        }),
        mode: config.pipeline_mode.clone(),
        provider: config.provider.clone(),
        model: config.model.clone(),
        profile_id: config.profile_id.clone(),
        profile_name: config.profile_name.clone(),
        api_key_ref: config.api_key_ref.clone(),
        audio_duration_seconds: config.audio_duration_seconds,
        min_duration_seconds: config.llm_min_duration_seconds,
        enabled: matches!(config.pipeline_mode.as_str(), "hybrid" | "cloud"),
        attempted: false,
        used: false,
        fallback: false,
        skipped_reason: String::new(),
        timeout_seconds: config.llm_timeout_seconds,
        attempt_timeout_seconds: attempt_budget(config, text).as_secs(),
        attempts: 0,
        elapsed_seconds: 0.0,
        usage: None,
        error_type: None,
        provider_error: None,
        http_status: None,
        response_snippet: None,
        output_length: None,
        provider_attempts: Vec::new(),
        late: false,
        parts: None,
    };

    if config.pipeline_mode == "local" {
        return skipped(text, status, "local_mode");
    }
    if config.provider.is_empty() {
        return skipped(text, status, "missing_provider");
    }
    if duration_below_threshold(
        config.llm_min_duration_seconds,
        config.audio_duration_seconds,
    ) {
        return skipped(text, status, "duration_below_threshold");
    }
    if api_key.map(str::is_empty).unwrap_or(true) {
        return skipped(text, status, "missing_api_key");
    }
    let Some(api_key) = api_key else {
        return skipped(text, status, "missing_api_key");
    };

    let rendered_system = render_system_prompt(&config.system_prompt, &config.language);
    // The app appends no rules of its own, so an empty prompt would send the
    // dictation to the model with no instructions at all.
    if rendered_system.trim().is_empty() {
        return skipped(text, status, "missing_system_prompt");
    }
    let parts = long_text::split(text, max_request_chars(config));
    if parts.len() > 1 {
        let longest = parts
            .iter()
            .map(|part| part.text)
            .max_by_key(|part| part.len())
            .unwrap_or_default();
        let provider: Arc<dyn Provider> = Arc::from(build_provider(
            config,
            api_key,
            attempt_budget(config, longest),
        ));
        return process_in_parts(&parts, config, provider, &rendered_system, status).await;
    }
    let user_message = wrap_dictation(text);

    // The HTTP client gives up on its own clock: kept to the attempt budget, it
    // would cut off the very request a late answer waits for.
    let budget = attempt_budget(config, text);
    let http_timeout = match config.late_answer {
        LateAnswerMode::Off => budget,
        _ => budget + LATE_ANSWER_GRACE,
    };
    let provider: Arc<dyn Provider> = Arc::from(build_provider(config, api_key, http_timeout));
    finish_with_provider(
        text,
        config,
        provider,
        &rendered_system,
        &user_message,
        budget,
        status,
    )
    .await
}

/// Parts tidied at once. Enough to finish a long file in a fraction of the
/// time one at a time would take, few enough not to trip a provider's rate
/// limit on an ordinary plan.
const PARTS_IN_FLIGHT: usize = 3;

/// How long a file transcription may wait for its parts in total. Nobody is
/// waiting to paste and the panel can cancel it, so an hour-long recording
/// gets the time it needs.
const FILE_PARTS_MAX_SECS: u64 = 30 * 60;

/// Tidy a long text in parts and join them back. Each part is judged on its
/// own, so one that fails or changes the meaning keeps its local text while
/// the others keep their tidy-up. A dictation keeps its usual total budget —
/// the overlay waits on it — and a part not done by then stays local.
async fn process_in_parts(
    parts: &[long_text::Part<'_>],
    config: &AiConfig,
    provider: Arc<dyn Provider>,
    rendered_system: &str,
    mut status: AiStatus,
) -> CallOutcome {
    use futures_util::StreamExt;

    let text = long_text::join(
        parts,
        &parts
            .iter()
            .map(|part| part.text.to_string())
            .collect::<Vec<_>>(),
    );
    let started = tokio::time::Instant::now();
    let total = if config.may_run_long {
        Duration::from_secs(FILE_PARTS_MAX_SECS)
    } else {
        attempt_budget(config, &text)
    };
    let deadline = started + total;
    // A part's answer is never late: past the deadline its local text stands.
    let part_config = AiConfig {
        late_answer: LateAnswerMode::Off,
        ..config.clone()
    };
    // Boxed one by one: futures built in an iterator closure lose the proof
    // that they are `Send`, which the dictation's spawned task needs.
    let mut jobs: Vec<
        std::pin::Pin<Box<dyn std::future::Future<Output = CallOutcome> + Send + '_>>,
    > = Vec::with_capacity(parts.len());
    for (index, part) in parts.iter().enumerate() {
        // The local text before the cut, not its tidied version: the parts
        // run side by side, and the model only reads it.
        let before = index
            .checked_sub(1)
            .map(|previous| long_text::tail(parts[previous].text, long_text::CONTEXT_CHARS));
        jobs.push(Box::pin(tidy_part(
            part.text,
            before,
            &part_config,
            Arc::clone(&provider),
            rendered_system,
            deadline,
            &status,
        )));
    }
    let outcomes: Vec<CallOutcome> = futures_util::stream::iter(jobs)
        .buffered(PARTS_IN_FLIGHT)
        .collect()
        .await;

    for (index, (part, outcome)) in parts.iter().zip(&outcomes).enumerate() {
        log::debug!(
            "Part {}/{}: {} chars, {}",
            index + 1,
            parts.len(),
            part.text.chars().count(),
            if outcome.status.used {
                "tidied"
            } else {
                outcome.status.skipped_reason.as_str()
            }
        );
    }
    let used = outcomes
        .iter()
        .filter(|outcome| outcome.status.used)
        .count();
    let tidied: Vec<String> = outcomes
        .iter()
        .map(|outcome| outcome.text.clone())
        .collect();
    let joined = long_text::join(parts, &tidied);

    status.attempted = true;
    status.used = used > 0;
    status.fallback = used == 0;
    status.parts = Some(PartsSummary {
        total: parts.len(),
        used,
    });
    status.elapsed_seconds = started.elapsed().as_secs_f64();
    status.attempt_timeout_seconds = total.as_secs();
    status.attempts = outcomes.iter().map(|outcome| outcome.status.attempts).sum();
    status.provider_attempts = outcomes
        .iter()
        .flat_map(|outcome| outcome.status.provider_attempts.iter().cloned())
        .collect();
    status.usage = outcomes
        .iter()
        .filter_map(|outcome| outcome.status.usage.as_ref())
        .fold(None, |sum: Option<super::providers::UsageInfo>, usage| {
            let mut sum = sum.unwrap_or_default();
            sum.input_tokens += usage.input_tokens;
            sum.output_tokens += usage.output_tokens;
            sum.total_tokens += usage.total_tokens;
            Some(sum)
        });
    // With no part tidied, the history names the first part's failure.
    if let Some(failed) = outcomes.iter().find(|outcome| !outcome.status.used) {
        if used == 0 {
            status.skipped_reason = failed.status.skipped_reason.clone();
            status.error_type = failed.status.error_type.clone();
            status.provider_error = failed.status.provider_error.clone();
            status.http_status = failed.status.http_status;
            status.response_snippet = failed.status.response_snippet.clone();
        }
    }
    status.output_length = Some(joined.chars().count());
    CallOutcome {
        text: joined,
        status,
        late: None,
    }
}

/// One part of a long text, within what is left of the shared `deadline`.
/// `before` is the end of the preceding part, sent for context only.
async fn tidy_part(
    text: &str,
    before: Option<&str>,
    config: &AiConfig,
    provider: Arc<dyn Provider>,
    rendered_system: &str,
    deadline: tokio::time::Instant,
    base: &AiStatus,
) -> CallOutcome {
    let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
    let budget = attempt_budget(config, text).min(remaining);
    let status = AiStatus {
        provider_attempts: Vec::new(),
        ..base.clone()
    };
    if budget.is_zero() {
        let mut outcome = skipped(text, status, "provider_timeout");
        outcome.status.attempted = true;
        outcome.status.fallback = true;
        outcome.status.error_type = Some("timeout".to_string());
        return outcome;
    }
    let user_message = match before {
        Some(before) => format!("{}\n{}", wrap_context(before), wrap_dictation(text)),
        None => wrap_dictation(text),
    };
    let mut outcome = finish_with_provider(
        text,
        config,
        provider,
        rendered_system,
        &user_message,
        budget,
        status,
    )
    .await;
    if outcome.status.used
        && before.is_some_and(|before| repeats_context(before, text, &outcome.text))
    {
        log::warn!(
            "Provider {}/{} repeated the context before a part; keeping the part's local text",
            config.provider,
            config.model
        );
        outcome.text = text.to_string();
        outcome.status.used = false;
        outcome.status.fallback = true;
        outcome.status.error_type = Some("altered_response".to_string());
        outcome.status.skipped_reason = "model_repeated_context".to_string();
    }
    outcome
}

/// Keep a tidy edit, and return the original dictation when the answer is
/// empty, a remark about the text, or a retelling that dropped the author's
/// words.
async fn finish_with_provider(
    text: &str,
    config: &AiConfig,
    provider: Arc<dyn Provider>,
    rendered_system: &str,
    user_message: &str,
    budget: Duration,
    mut status: AiStatus,
) -> CallOutcome {
    status.attempted = true;
    let (result, info) = call_provider_with_retry(
        provider,
        rendered_system,
        user_message,
        budget,
        budget,
        &mut status,
    )
    .await;

    status.attempts = info.attempts;
    status.attempt_timeout_seconds = info.attempt_timeout_seconds;
    status.elapsed_seconds = info.elapsed_seconds;
    status.usage = info.usage;

    let Some(raw) = result else {
        status.fallback = true;
        status.provider_error = Some(info.message.clone());
        status.error_type = Some(
            info.error_type
                .unwrap_or(ProviderErrorType::ProviderFailed)
                .as_str()
                .to_string(),
        );
        if let Some(http) = info.http_status {
            status.http_status = Some(http);
        }
        if let Some(snippet) = info.response_snippet {
            status.response_snippet = Some(snippet);
        }
        status.skipped_reason =
            skipped_reason_for(&status.error_type.clone().unwrap_or_default()).to_string();
        let late = info
            .pending
            .filter(|_| config.late_answer != LateAnswerMode::Off)
            .map(|request| LateAnswer {
                request,
                source_text: text.to_string(),
                config: config.clone(),
                status: status.clone(),
                started: info.started,
                give_up_at: info.started + budget + LATE_ANSWER_GRACE,
            });
        return CallOutcome {
            text: text.to_string(),
            status,
            late,
        };
    };
    judge_answer(text, config, raw, info.response_snippet, status)
}

/// Keep the answer if it is a tidy edit of `text`; otherwise the dictation
/// stays as it was and `status` says why. The same judgement for an answer on
/// time and for a late one.
fn judge_answer(
    text: &str,
    config: &AiConfig,
    raw: String,
    response_snippet: Option<String>,
    mut status: AiStatus,
) -> CallOutcome {
    let cleaned = strip_reasoning(&raw);
    if cleaned.is_empty() {
        status.fallback = true;
        status.error_type = Some("empty_response".to_string());
        status.skipped_reason = "empty_response".to_string();
        status.provider_error = response_snippet;
        return CallOutcome {
            text: text.to_string(),
            status,
            late: None,
        };
    }
    if is_meta_noop_response(&cleaned) {
        status.fallback = true;
        status.error_type = Some("meta_response".to_string());
        status.skipped_reason = "model_returned_meta_response".to_string();
        return CallOutcome {
            text: text.to_string(),
            status,
            late: None,
        };
    }
    // A model that gave back half the dictation retold it instead of tidying
    // it up, whatever the prompt asked for. Falling back to the untouched
    // local transcript loses the punctuation; keeping the answer loses the
    // user's words, and they have no way to know which happened.
    if dropped_too_much(text, &cleaned) {
        let ratio = kept_word_ratio(text, &cleaned).unwrap_or(0.0);
        log::warn!(
            "Provider {}/{} returned {:.0}% of the dictation's words; falling back to the local transcript",
            config.provider,
            config.model,
            ratio * 100.0
        );
        status.fallback = true;
        status.error_type = Some("summarised_response".to_string());
        status.skipped_reason = "model_dropped_text".to_string();
        status.output_length = Some(cleaned.chars().count());
        return CallOutcome {
            text: text.to_string(),
            status,
            late: None,
        };
    }
    // Same length, different meaning: a lost «never», a renumbered list, a
    // product name «corrected» into another one.
    if let Some(alteration) = altered_meaning(text, &cleaned) {
        log::warn!(
            "Provider {}/{} changed the dictation's meaning ({}); falling back to the local transcript",
            config.provider,
            config.model,
            alteration.reason()
        );
        status.fallback = true;
        status.error_type = Some("altered_response".to_string());
        status.skipped_reason = alteration.reason().to_string();
        status.output_length = Some(cleaned.chars().count());
        return CallOutcome {
            text: text.to_string(),
            status,
            late: None,
        };
    }

    status.used = true;
    status.output_length = Some(cleaned.chars().count());
    CallOutcome {
        text: cleaned,
        status,
        late: None,
    }
}

pub async fn ai_process_text(text: &str, config: &AiConfig, api_key: Option<&str>) -> String {
    ai_process_text_with_status(text, config, api_key)
        .await
        .text
}

fn wrap_dictation(text: &str) -> String {
    // Neutralize any literal `</dictation>` the user dictated so it
    // can't break out of the envelope. The Python implementation
    // does the same; we keep the byte-for-byte shape.
    let safe = text.replace("</dictation>", "</ dictation>");
    format!("<dictation>\n{safe}\n</dictation>")
}

/// The end of the preceding part, sent before a part of a long text. The
/// system prompt is the user's and knows nothing of parts, so the block says
/// itself what it is for.
fn wrap_context(text: &str) -> String {
    let safe = text.replace("</preceding_text>", "</ preceding_text>");
    format!(
        "<preceding_text note=\"The text right before this dictation, already handled separately. The dictation continues it, possibly in the middle of a sentence. Use it only to understand the context; do not edit it or include it in your answer.\">\n{safe}\n</preceding_text>"
    )
}

/// Turn the speech-language setting into the phrase that replaces
/// `{{language}}` in the system prompt.
///
/// The prompt line reads "Output language: {{language}}.", so the value has
/// to be a name, not a code: `ru` would ship as "Output language: ru." And
/// `auto` — the default — has no name at all, so it becomes an instruction
/// to follow the dictation instead of a language.
fn language_directive(language: &str) -> &'static str {
    match language.trim() {
        "ru" => "Russian",
        "en" => "English",
        // "auto", empty, or a code we do not have a name for.
        _ => "the same language the dictation is in",
    }
}

fn render_system_prompt(template: &str, language: &str) -> String {
    let now = chrono_like_now();
    template
        .replace("{{text}}", "")
        .replace("{{transcript}}", "")
        .replace("{{language}}", language_directive(language))
        .replace("{{app}}", "Sotto")
        .replace("{{datetime}}", &now)
}

fn chrono_like_now() -> String {
    // Lightweight ISO 8601 timestamp without pulling in `chrono`.
    // Python's `datetime.now().strftime("%Y-%m-%d %H:%M")` matches
    // this format.
    use std::time::{SystemTime, UNIX_EPOCH};
    let total_seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    format_timestamp(total_seconds)
}

/// Format a Unix timestamp (seconds since 1970-01-01 UTC) as
/// `YYYY-MM-DD HH:MM`. Split from [`chrono_like_now`] so the date
/// arithmetic is testable against fixed instants instead of the wall
/// clock.
fn format_timestamp(total_seconds: i64) -> String {
    // Days since 1970-01-01.
    let mut year = 1970;
    let mut day_of_year = total_seconds / 86_400;
    // Bounded rather than `loop`: the format is `{year:04}`, so a year past
    // 9999 is unrepresentable anyway. The bound also keeps a corrupted
    // timestamp (or a mutated exit condition) from spinning forever.
    while year <= 9999 {
        let leap = is_leap(year);
        let year_days = if leap { 366 } else { 365 };
        if day_of_year < year_days {
            break;
        }
        day_of_year -= year_days;
        year += 1;
    }
    let leap = is_leap(year);
    let month_days = if leap {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    let mut month = 1;
    let mut day_of_month = day_of_year + 1;
    for &days_in_month in &month_days {
        if day_of_month <= days_in_month {
            break;
        }
        day_of_month -= days_in_month;
        month += 1;
    }
    let total_today_seconds = total_seconds - (day_of_year * 86_400);
    let hours = (total_today_seconds / 3600) % 24;
    let minutes = (total_today_seconds / 60) % 60;
    format!(
        "{year:04}-{:02}-{:02} {:02}:{:02}",
        month, day_of_month, hours, minutes
    )
}

/// True when a recording is present but shorter than the configured
/// minimum for the LLM pass. `min_duration == 0` disables the gate.
/// Extracted from the orchestrator so the strict boundary is testable
/// without a network round-trip.
fn duration_below_threshold(min_duration: f64, audio_duration: Option<f64>) -> bool {
    min_duration > 0.0 && audio_duration.is_some_and(|seconds| seconds < min_duration)
}

fn is_leap(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// A conservative output speed for a hosted model, so a long text is given
/// time to come back rather than timing out on every attempt.
const ANSWER_TOKENS_PER_SECOND: u32 = 50;

/// The longest one attempt may take, however long the text. The overlay's
/// stuck-guard is derived from it, so it never hides a request still running.
pub(crate) const MAX_ATTEMPT_SECS: u64 = 300;

/// One attempt may use the whole budget: the configured timeout, or longer
/// when `text` cannot be rewritten in it at [`ANSWER_TOKENS_PER_SECOND`]. A
/// fixed cap cut off every long dictation, and retrying the same request after
/// the cap cannot finish sooner. Fast transient failures still retry within
/// the budget that remains.
fn attempt_timeout(configured: u64, text: &str) -> Duration {
    let needed = u64::from(answer_tokens(text).div_ceil(ANSWER_TOKENS_PER_SECOND));
    Duration::from_secs(configured.max(needed).clamp(1, MAX_ATTEMPT_SECS))
}

/// [`attempt_timeout`] for `config`. Thinking left to the model takes time
/// before the first word of the answer — on a one-line dictation as much as
/// on a long one — so such a profile gets three times the whole budget,
/// within the cap the overlay's stuck guard is derived from.
fn attempt_budget(config: &AiConfig, text: &str) -> Duration {
    let budget = attempt_timeout(config.llm_timeout_seconds, text);
    match config.reasoning {
        ReasoningMode::Minimal => budget,
        ReasoningMode::Model => (budget * 3).min(Duration::from_secs(MAX_ATTEMPT_SECS)),
    }
}

/// The output cap an automatic limit counts on. The app asks for more, but
/// many hosted models stop at this, and an answer cut short loses its part.
const AUTO_OUTPUT_TOKENS: u32 = 8_192;

/// The output cap an unlimited profile counts on; past it the attempt's time
/// cap binds first anyway.
const UNLIMITED_OUTPUT_TOKENS: u32 = 32_000;

/// The longest text one request may carry for `config`: its answer, about as
/// long as the text, must fit the profile's output limit with room left for
/// thinking, and come back within one attempt's time cap. A longer text is
/// tidied in parts.
fn max_request_chars(config: &AiConfig) -> usize {
    let cap = match config.output_limit {
        OutputLimit::Auto => AUTO_OUTPUT_TOKENS,
        OutputLimit::Unlimited => UNLIMITED_OUTPUT_TOKENS,
        OutputLimit::Tokens(tokens) => tokens,
    };
    // Thinking left to the model may take two thirds of the cap and of the
    // time, as `completion_budget` and `attempt_budget` allow it; minimal
    // thinking still needs some room.
    let in_time = MAX_ATTEMPT_SECS * u64::from(ANSWER_TOKENS_PER_SECOND);
    let (answer, in_time) = match config.reasoning {
        ReasoningMode::Minimal => (cap / 3 * 2, in_time),
        ReasoningMode::Model => (cap / 3, in_time / 3),
    };
    // A fifth of the time spare: the output speed is an estimate, and an
    // answer that misses the cap loses the whole text, not one part.
    let tokens = u64::from(answer).min(in_time / 5 * 4);
    usize::try_from(tokens).unwrap_or(usize::MAX) * CHARS_PER_TOKEN
}

fn build_provider(config: &AiConfig, api_key: &str, timeout: Duration) -> Box<dyn Provider> {
    let base_url = config.base_url.as_deref().map(str::to_string);
    let (key, model) = (api_key.to_string(), config.model.clone());
    let (reasoning, limit) = (config.reasoning, config.output_limit);
    match config.provider.as_str() {
        "openai" => Box::new(
            OpenAIProvider::new(key, model, base_url, Some(timeout), None)
                .for_openai_api()
                .with_options(reasoning, limit),
        ),
        "compatible" => Box::new(
            OpenAIProvider::new(key, model, base_url, Some(timeout), None)
                .with_options(reasoning, limit),
        ),
        "opencode-go" => Box::new(
            OpenCodeGoProvider::new(key, model, base_url, Some(timeout))
                .with_options(reasoning, limit),
        ),
        "gemini" => {
            Box::new(GeminiProvider::new(key, model, Some(timeout)).with_options(reasoning, limit))
        }
        _ => Box::new(
            AnthropicProvider::new(key, model, base_url, Some(timeout), None)
                .with_options(reasoning, limit),
        ),
    }
}

struct CallOutcomeInfo {
    started: tokio::time::Instant,
    /// The last attempt, when it was still running at the deadline.
    pending: Option<InFlight>,
    attempts: u32,
    attempt_timeout_seconds: u64,
    elapsed_seconds: f64,
    usage: Option<super::providers::UsageInfo>,
    message: String,
    error_type: Option<ProviderErrorType>,
    http_status: Option<u16>,
    response_snippet: Option<String>,
}

async fn call_provider_with_retry(
    provider: Arc<dyn Provider>,
    system_prompt: &str,
    text: &str,
    attempt_timeout: Duration,
    total_timeout: Duration,
    status: &mut AiStatus,
) -> (Option<String>, CallOutcomeInfo) {
    let started = tokio::time::Instant::now();
    let deadline = started + total_timeout;
    let mut last_error =
        ProviderError::new(ProviderErrorType::Timeout, "LLM time budget exhausted");
    let mut pending = None;
    for attempt in 1..=MAX_PROVIDER_ATTEMPTS {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        // A retry replaces the attempt that timed out before it.
        pending = None;
        let attempt_started = tokio::time::Instant::now();
        let mut request = InFlight::spawn(&provider, system_prompt, text);
        let joined = tokio::time::timeout(remaining.min(attempt_timeout), &mut request.0).await;
        let result = match joined {
            Ok(Ok(reply)) => reply,
            Ok(Err(failure)) => Err(ProviderError::new(
                ProviderErrorType::ProviderFailed,
                format!("provider task failed: {failure}"),
            )),
            Err(_) => {
                pending = Some(request);
                Err(ProviderError::new(
                    ProviderErrorType::Timeout,
                    "LLM attempt timed out",
                ))
            }
        };
        let elapsed = attempt_started.elapsed().as_secs_f64();
        let error = result.as_ref().err();
        status.provider_attempts.push(ProviderAttemptInfo {
            attempt,
            elapsed_seconds: elapsed,
            error_type: error.map_or_else(String::new, |e| e.kind.as_str().to_string()),
            provider_error: error.map_or_else(String::new, |e| e.message.clone()),
            http_status: match &result {
                Ok((_, info)) => info.http_status,
                Err(e) => e.http_status,
            },
        });
        match result {
            Ok((text, info)) => {
                return (
                    Some(text),
                    CallOutcomeInfo {
                        started,
                        pending: None,
                        attempts: attempt,
                        attempt_timeout_seconds: attempt_timeout.as_secs(),
                        elapsed_seconds: started.elapsed().as_secs_f64(),
                        usage: info.usage,
                        message: String::new(),
                        error_type: None,
                        http_status: info.http_status,
                        response_snippet: info.response_snippet,
                    },
                )
            }
            Err(error) => {
                let retry = should_retry(&error, attempt);
                last_error = error;
                if !retry
                    || deadline.saturating_duration_since(tokio::time::Instant::now())
                        <= RETRY_BACKOFF
                {
                    break;
                }
                tokio::time::sleep(RETRY_BACKOFF).await;
            }
        }
    }
    (
        None,
        CallOutcomeInfo {
            started,
            pending,
            attempts: status.provider_attempts.len() as u32,
            attempt_timeout_seconds: attempt_timeout.as_secs(),
            elapsed_seconds: started.elapsed().as_secs_f64(),
            usage: None,
            message: last_error.message,
            error_type: Some(last_error.kind),
            http_status: last_error.http_status,
            response_snippet: last_error.response_snippet,
        },
    )
}

fn should_retry(error: &ProviderError, attempt: u32) -> bool {
    if attempt >= MAX_PROVIDER_ATTEMPTS {
        return false;
    }
    if !error.kind.is_retryable() {
        return false;
    }
    TRANSIENT_ERRORS.contains(&error.kind)
}

fn skipped_reason_for(error_type: &str) -> &'static str {
    for (key, value) in SKIPPED_REASON_BY_ERROR_TYPE {
        if *key == error_type {
            return value;
        }
    }
    "provider_failed"
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_config() -> AiConfig {
        AiConfig {
            pipeline_mode: "hybrid".to_string(),
            provider: "anthropic".to_string(),
            model: "claude-haiku-4-5".to_string(),
            profile_id: "default".to_string(),
            profile_name: "Default".to_string(),
            api_key_ref: "anthropic".to_string(),
            system_prompt: "You are a dictation editor.".to_string(),
            language: "ru".to_string(),
            base_url: None,
            audio_duration_seconds: Some(45.0),
            llm_min_duration_seconds: 30.0,
            llm_timeout_seconds: 12,
            late_answer: LateAnswerMode::Notify,
            reasoning: ReasoningMode::Minimal,
            output_limit: OutputLimit::Auto,
            may_run_long: false,
        }
    }

    #[tokio::test]
    async fn telemetry_service_uses_effective_provider_endpoint() {
        use crate::telemetry::ProviderService;
        for (provider, base_url, expected) in [
            ("openai", None, ProviderService::Openai),
            ("anthropic", None, ProviderService::Anthropic),
            (
                "compatible",
                Some("https://openrouter.ai/api/v1"),
                ProviderService::Openrouter,
            ),
            (
                "openai",
                Some("https://private-proxy.test/v1"),
                ProviderService::Custom,
            ),
            (
                "gemini",
                Some("https://ignored.test"),
                ProviderService::Gemini,
            ),
            ("opencode-go", Some(""), ProviderService::Opencode),
            // Blank is what the settings UI writes for a provider that has
            // no Base URL field.
            ("openai", Some(""), ProviderService::Openai),
            ("anthropic", Some("   "), ProviderService::Anthropic),
            ("compatible", Some(""), ProviderService::Openai),
        ] {
            let mut config = base_config();
            config.provider = provider.into();
            config.base_url = base_url.map(str::to_string);
            // Missing credentials return before any network request.
            let outcome = ai_process_text_with_status("synthetic text", &config, None).await;
            assert_eq!(outcome.status.telemetry_service, Some(expected));
            assert!(!outcome.status.attempted);
            assert!(serde_json::to_value(&outcome.status)
                .unwrap()
                .get("telemetry_service")
                .is_none());
        }
    }

    /// `run_ai_prompt` drops a blank Base URL before it builds the config;
    /// the live dictation path comes through here instead, so this is where
    /// the empty string the UI persists has to disappear.
    #[test]
    fn config_from_ai_processing_defaults_to_local() {
        let config = AiConfig::from_ai_processing(&serde_json::json!({}));
        assert_eq!(config.pipeline_mode, "local");
    }

    #[test]
    fn config_from_ai_processing_treats_a_blank_base_url_as_absent() {
        for blank in ["", "   "] {
            let config = AiConfig::from_ai_processing(&serde_json::json!({
                "provider": "anthropic",
                "base_url": blank,
            }));
            assert_eq!(config.base_url, None);
        }
        let config = AiConfig::from_ai_processing(&serde_json::json!({
            "provider": "compatible",
            "base_url": " https://api.groq.com/openai/v1 ",
        }));
        assert_eq!(
            config.base_url.as_deref(),
            Some("https://api.groq.com/openai/v1")
        );
    }

    #[tokio::test]
    async fn delayed_http_falls_back_within_the_configured_total_budget() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut config = base_config();
        config.provider = "compatible".into();
        config.base_url = Some(format!("http://{}/v1", listener.local_addr().unwrap()));
        config.llm_timeout_seconds = 1;
        let (accepted, received) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0; 4096];
            assert!(socket.read(&mut request).await.unwrap() > 0);
            let _ = accepted.send(());
            tokio::time::sleep(Duration::from_secs(5)).await;
            let _ = socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}")
                .await;
        });
        let outcome = tokio::time::timeout(
            Duration::from_secs(2),
            ai_process_text_with_status("original dictation", &config, Some("test-key")),
        )
        .await;
        server.abort();
        received.await.unwrap();
        let outcome = outcome.expect("total budget must include retries and backoff");
        assert_eq!(outcome.text, "original dictation");
        assert!(outcome.status.fallback);
        assert_eq!(outcome.status.attempts, 1);
        assert_eq!(outcome.status.error_type.as_deref(), Some("timeout"));
    }

    #[test]
    fn local_mode_skips_ai() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let mut cfg = base_config();
        cfg.pipeline_mode = "local".to_string();
        let outcome = runtime.block_on(ai_process_text_with_status("hello", &cfg, Some("sk-test")));
        assert!(!outcome.status.attempted);
        assert_eq!(outcome.status.skipped_reason, "local_mode");
        assert_eq!(outcome.text, "hello");
    }

    #[test]
    fn missing_provider_skips_ai() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let mut cfg = base_config();
        cfg.provider = String::new();
        let outcome = runtime.block_on(ai_process_text_with_status("hello", &cfg, Some("sk-test")));
        assert_eq!(outcome.status.skipped_reason, "missing_provider");
    }

    #[test]
    fn empty_system_prompt_skips_ai() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        for prompt in ["", "  \n", "{{transcript}}"] {
            let mut cfg = base_config();
            cfg.system_prompt = prompt.to_string();
            let outcome =
                runtime.block_on(ai_process_text_with_status("hello", &cfg, Some("sk-test")));
            assert!(!outcome.status.attempted, "{prompt:?}");
            assert_eq!(outcome.status.skipped_reason, "missing_system_prompt");
            assert_eq!(outcome.text, "hello");
        }
    }

    /// Plays a model that tidies by capitalising, and refuses any part that
    /// contains `refuse` with a server error.
    #[derive(Default)]
    struct PartsProvider {
        refuse: &'static str,
        /// How long each answer takes, on tokio's clock.
        delay: Duration,
        /// Answer with the preceding text in front of the part.
        echo: bool,
        calls: std::sync::atomic::AtomicUsize,
        with_context: std::sync::atomic::AtomicUsize,
    }

    impl Provider for PartsProvider {
        fn name(&self) -> &'static str {
            "parts"
        }

        fn complete<'a>(&'a self, _system_prompt: &'a str, text: &'a str) -> CompletionFuture<'a> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let (context, body) = text.split_once("<dictation>\n").unwrap();
            let mut body = body.trim_end_matches("\n</dictation>").to_string();
            if let Some((_, context)) = context.split_once("\">\n") {
                self.with_context
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                if self.echo {
                    let context = context.trim_end_matches("\n</preceding_text>\n");
                    body = format!("{context} {body}");
                }
            }
            let refuse = !self.refuse.is_empty() && body.contains(self.refuse);
            let delay = self.delay;
            Box::pin(async move {
                tokio::time::sleep(delay).await;
                if refuse {
                    Err(ProviderError::new(
                        ProviderErrorType::BadResponse,
                        "refused",
                    ))
                } else {
                    Ok((body.to_uppercase(), ProviderInfo::success("ok", None, 0.1)))
                }
            })
        }
    }

    fn long_text(marker_at: usize) -> String {
        (0..900)
            .map(|index| {
                let marker = if index == marker_at {
                    " МЕТКА"
                } else {
                    ""
                };
                format!("предложение номер {index} про работу{marker}.")
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    async fn run_in_parts(
        text: &str,
        provider: PartsProvider,
    ) -> (CallOutcome, Arc<PartsProvider>) {
        let provider = Arc::new(provider);
        let parts = long_text::split(text, 3_000);
        let outcome = process_in_parts(
            &parts,
            &base_config(),
            provider.clone(),
            "system",
            AiStatus::default(),
        )
        .await;
        (outcome, provider)
    }

    /// Past the old 28k-character limit the text is tidied, not skipped:
    /// every part comes back and the parts join into one text.
    #[tokio::test]
    async fn a_long_text_is_tidied_in_parts() {
        let text = long_text(usize::MAX);
        let (outcome, provider) = run_in_parts(&text, PartsProvider::default()).await;
        let parts = outcome.status.parts.unwrap();
        assert!(parts.total > 2);
        assert_eq!(parts.used, parts.total);
        let count = |counter: &std::sync::atomic::AtomicUsize| {
            counter.load(std::sync::atomic::Ordering::SeqCst)
        };
        assert_eq!(count(&provider.calls), parts.total);
        // Every part but the first reads the end of the one before it.
        assert_eq!(count(&provider.with_context), parts.total - 1);
        assert_eq!(outcome.text, text.to_uppercase());
    }

    /// A part answered with its context in front keeps its local text, so
    /// the joined result does not say that text twice.
    #[tokio::test]
    async fn a_part_that_repeats_its_context_keeps_its_local_text() {
        let text = long_text(usize::MAX);
        let provider = PartsProvider {
            echo: true,
            ..PartsProvider::default()
        };
        let (outcome, _) = run_in_parts(&text, provider).await;
        let parts = outcome.status.parts.unwrap();
        assert_eq!(parts.used, 1);
        assert!(outcome.text.starts_with("ПРЕДЛОЖЕНИЕ НОМЕР 0"));
        assert!(outcome.text.ends_with("предложение номер 899 про работу."));
        assert_eq!(outcome.text.chars().count(), text.chars().count());
    }

    /// A part the model fails keeps its local text; the rest keep theirs.
    #[tokio::test]
    async fn a_failed_part_keeps_its_local_text() {
        let text = long_text(150);
        let provider = PartsProvider {
            refuse: "МЕТКА",
            ..PartsProvider::default()
        };
        let (outcome, _) = run_in_parts(&text, provider).await;
        let parts = outcome.status.parts.unwrap();
        assert_eq!(parts.used, parts.total - 1);
        assert!(outcome.status.used && !outcome.status.fallback);
        assert!(outcome
            .text
            .contains("предложение номер 150 про работу МЕТКА."));
        assert!(outcome.text.starts_with("ПРЕДЛОЖЕНИЕ НОМЕР 0"));
    }

    /// A dictation keeps its overlay-bounded budget: parts still waiting when
    /// it runs out keep their local text instead of holding the paste.
    #[tokio::test(start_paused = true)]
    async fn a_dictation_in_parts_stops_at_its_budget() {
        let provider = Arc::new(PartsProvider {
            // Past each part's own budget, so every round takes a full one.
            delay: Duration::from_secs(400),
            ..PartsProvider::default()
        });
        let text = (0..6_000)
            .map(|index| format!("предложение {index}."))
            .collect::<Vec<_>>()
            .join(" ");
        let parts = long_text::split(&text, 3_000);
        let started = tokio::time::Instant::now();
        let outcome = process_in_parts(
            &parts,
            &base_config(),
            provider.clone(),
            "system",
            AiStatus::default(),
        )
        .await;
        let summary = outcome.status.parts.unwrap();
        assert_eq!(summary.used, 0);
        assert!(started.elapsed() <= Duration::from_secs(MAX_ATTEMPT_SECS + 1));
        assert!(
            provider.calls.load(std::sync::atomic::Ordering::SeqCst) < summary.total,
            "parts past the budget must not be sent"
        );
        assert_eq!(outcome.text, text);
    }

    /// The real entry point goes through the parts path for a long text and
    /// reports it, however the provider answers.
    #[tokio::test]
    async fn the_entry_point_splits_a_long_text() {
        let mut config = base_config();
        config.provider = "compatible".to_string();
        // Nothing listens here: every part fails fast and stays local.
        config.base_url = Some("http://127.0.0.1:9".to_string());
        let text = long_text(usize::MAX);
        let outcome = ai_process_text_with_status(&text, &config, Some("sk-test")).await;
        let parts = outcome.status.parts.expect("long text goes through parts");
        assert!(parts.total > 2);
        assert_eq!(parts.used, 0);
        assert!(outcome.status.fallback);
        assert_eq!(outcome.text, text);
    }

    #[test]
    fn short_audio_skips_ai() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let mut cfg = base_config();
        cfg.audio_duration_seconds = Some(5.0);
        cfg.llm_min_duration_seconds = 30.0;
        let outcome = runtime.block_on(ai_process_text_with_status("hello", &cfg, Some("sk-test")));
        assert_eq!(outcome.status.skipped_reason, "duration_below_threshold");
    }

    #[test]
    fn missing_api_key_skips_ai() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let outcome = runtime.block_on(ai_process_text_with_status("hello", &base_config(), None));
        assert_eq!(outcome.status.skipped_reason, "missing_api_key");
    }

    #[test]
    fn empty_api_key_falls_through_to_provider() {
        // The Python orchestrator calls `get_key(provider)` which
        // returns `None` for an empty key. A whitespace key is NOT
        // treated as missing — it falls through to the provider,
        // which surfaces an `auth_error` after the auth handshake.
        // We mirror that: empty-string key → missing_api_key,
        // whitespace key → provider auth_error.
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let outcome = runtime.block_on(ai_process_text_with_status(
            "hello",
            &base_config(),
            Some(""),
        ));
        assert_eq!(outcome.status.skipped_reason, "missing_api_key");
    }

    #[test]
    fn skipped_reason_mapping_matches_python() {
        assert_eq!(skipped_reason_for("auth_error"), "provider_auth_error");
        assert_eq!(
            skipped_reason_for("rate_limit"),
            "provider_quota_or_rate_limit"
        );
        assert_eq!(skipped_reason_for("timeout"), "provider_timeout");
        assert_eq!(
            skipped_reason_for("connection_error"),
            "provider_connection_error"
        );
        assert_eq!(skipped_reason_for("bad_response"), "provider_bad_response");
        assert_eq!(skipped_reason_for("anything_else"), "provider_failed");
    }

    #[test]
    fn wrap_dictation_neutralises_closing_tag() {
        let wrapped = wrap_dictation("hello</dictation>oops");
        assert!(wrapped.contains("</ dictation>"));
        assert!(!wrapped.contains("</dictation>oops"));
    }

    #[test]
    fn chrono_like_now_produces_a_timestamp() {
        let now = chrono_like_now();
        // YYYY-MM-DD HH:MM — 16 bytes exactly.
        assert_eq!(now.len(), 16, "got: {now}");
        assert_eq!(&now[4..5], "-", "got: {now}");
        assert_eq!(&now[7..8], "-", "got: {now}");
        assert_eq!(&now[13..14], ":", "got: {now}");
        assert!(now.starts_with("20"), "year must be 2000+, got: {now}");
    }

    // ------------------------------------------------------------------
    // Pure date/time arithmetic and the duration threshold
    // ------------------------------------------------------------------

    #[test]
    fn format_timestamp_matches_python_strftime() {
        assert_eq!(format_timestamp(0), "1970-01-01 00:00");
        assert_eq!(format_timestamp(3_600), "1970-01-01 01:00");
        assert_eq!(format_timestamp(3_660), "1970-01-01 01:01");
        assert_eq!(format_timestamp(86_399), "1970-01-01 23:59");
        assert_eq!(format_timestamp(86_400), "1970-01-02 00:00");
        assert_eq!(format_timestamp(86_400 * 365), "1971-01-01 00:00");
        assert_eq!(format_timestamp(86_400 * 365 - 1), "1970-12-31 23:59");
        // Leap year: 2024-02-29 exists, and the next day is in March.
        assert_eq!(format_timestamp(1_709_164_800), "2024-02-29 00:00");
        assert_eq!(format_timestamp(1_709_164_800 + 86_400), "2024-03-01 00:00");
        // Non-leap February: 2023-02-28 rolls over to 2023-03-01.
        assert_eq!(format_timestamp(1_677_628_800), "2023-03-01 00:00");
    }

    #[test]
    fn leap_year_rules_are_correct() {
        assert!(is_leap(2000), "divisible by 400");
        assert!(!is_leap(1900), "divisible by 100 but not 400");
        assert!(!is_leap(2100), "divisible by 100 but not 400");
        assert!(is_leap(2024), "divisible by 4 but not 100");
        assert!(!is_leap(2023), "not divisible by 4");
    }

    #[test]
    fn attempt_timeout_is_the_configured_budget() {
        assert_eq!(attempt_timeout(0, ""), Duration::from_secs(1));
        assert_eq!(
            attempt_timeout(25, "короткая фраза"),
            Duration::from_secs(25)
        );
        assert_eq!(attempt_timeout(1000, ""), Duration::from_secs(300));
    }

    #[test]
    fn the_request_size_follows_the_profile_output_limit() {
        let chars = |output_limit, reasoning| {
            max_request_chars(&AiConfig {
                output_limit,
                reasoning,
                ..base_config()
            })
        };
        let auto = chars(OutputLimit::Auto, ReasoningMode::Minimal);
        // A ten-minute dictation still goes as one request by default.
        assert!(auto > 10_000, "{auto}");
        assert!(chars(OutputLimit::Auto, ReasoningMode::Model) < auto);
        assert!(chars(OutputLimit::Tokens(4_000), ReasoningMode::Minimal) < auto);
        let large = chars(OutputLimit::Tokens(64_000), ReasoningMode::Minimal);
        assert!(large > auto * 2, "{large}");
        // However large the cap, an answer must come back in one attempt with
        // time to spare, and with time to think when thinking is left on.
        assert_eq!(chars(OutputLimit::Unlimited, ReasoningMode::Minimal), large);
        let answer_secs = |chars: usize| {
            u64::try_from(chars / CHARS_PER_TOKEN).unwrap() / u64::from(ANSWER_TOKENS_PER_SECOND)
        };
        assert!(answer_secs(large) < MAX_ATTEMPT_SECS);
        let thinking = chars(OutputLimit::Tokens(64_000), ReasoningMode::Model);
        assert!(answer_secs(thinking) < MAX_ATTEMPT_SECS / 3);
    }

    #[test]
    fn attempt_timeout_grows_with_a_long_text() {
        // 15 minutes of speech: ~7.5k answer tokens, far past a 12 s default.
        let long = "я".repeat(15_000);
        assert_eq!(attempt_timeout(12, &long), Duration::from_secs(150));
        assert_eq!(attempt_timeout(200, &long), Duration::from_secs(200));
        let longest = "я".repeat(28_000);
        assert_eq!(attempt_timeout(12, &longest), Duration::from_secs(280));
    }

    #[test]
    fn thinking_left_to_the_model_gets_three_times_the_time() {
        let mut config = base_config();
        let short = "короткая фраза";
        let long = "я".repeat(6_000);
        assert_eq!(attempt_budget(&config, short), Duration::from_secs(12));
        assert_eq!(attempt_budget(&config, &long), Duration::from_secs(60));
        config.reasoning = ReasoningMode::Model;
        // A one-line dictation needs the thinking time as much as a long one.
        assert_eq!(attempt_budget(&config, short), Duration::from_secs(36));
        assert_eq!(attempt_budget(&config, &long), Duration::from_secs(180));
        // Still within the cap the overlay's stuck guard is derived from.
        assert_eq!(
            attempt_budget(&config, &"я".repeat(28_000)),
            Duration::from_secs(MAX_ATTEMPT_SECS)
        );
    }

    #[test]
    fn duration_gate_is_strict_at_the_boundary() {
        assert!(duration_below_threshold(30.0, Some(29.9)));
        assert!(
            !duration_below_threshold(30.0, Some(30.0)),
            "exactly at the minimum is not below it"
        );
        assert!(!duration_below_threshold(0.0, Some(5.0)), "min=0 disables");
        assert!(
            !duration_below_threshold(30.0, None),
            "no recording, no gate"
        );
    }

    // ------------------------------------------------------------------
    // build_provider / retry / render
    // ------------------------------------------------------------------

    #[test]
    fn build_provider_selects_each_arm_by_name() {
        let mut cfg = base_config();
        for (provider, expected_name) in [
            ("anthropic", "anthropic"),
            ("openai", "openai"),
            ("compatible", "openai"),
            ("opencode-go", "opencode-go"),
            ("gemini", "gemini"),
            ("something-unknown", "anthropic"), // fallback arm
        ] {
            cfg.provider = provider.to_string();
            let built = build_provider(&cfg, "sk-test", Duration::from_secs(12));
            assert_eq!(built.name(), expected_name, "provider {provider}");
        }
    }

    #[test]
    fn should_retry_respects_attempt_cap_and_retryability() {
        let timeout = ProviderError::new(ProviderErrorType::Timeout, "t");
        let auth = ProviderError::new(ProviderErrorType::AuthError, "a");
        assert!(
            should_retry(&timeout, 1),
            "transient error retries on attempt 1"
        );
        assert!(!should_retry(&timeout, 2), "attempt cap stops retrying");
        assert!(!should_retry(&auth, 1), "non-transient error never retries");
    }

    #[test]
    fn render_system_prompt_only_substitutes_explicit_placeholders() {
        assert_eq!(
            render_system_prompt("Language: {{language}}; app: {{app}}", "ru"),
            "Language: Russian; app: Sotto"
        );
        for prompt in [
            "Summarise the text.",
            "Переведи текст.",
            "  Custom rules.\n",
            "",
        ] {
            assert_eq!(render_system_prompt(prompt, "ru"), prompt);
        }
    }

    /// `{{language}}` used to be fed from `ai_processing.language`, a field
    /// nothing ever wrote, so every prompt shipped "Output language: ." —
    /// an instruction with an empty value. The speech setting also defaults
    /// to `auto`, which has no language name at all and must turn into an
    /// instruction rather than a code.
    #[test]
    fn language_placeholder_never_renders_a_code_or_a_blank() {
        for (setting, expected) in [
            ("ru", "Russian"),
            ("en", "English"),
            ("auto", "the same language the dictation is in"),
            ("", "the same language the dictation is in"),
        ] {
            let rendered = render_system_prompt("Output language: {{language}}.", setting);
            assert!(
                rendered.contains(&format!("Output language: {expected}.")),
                "setting {setting:?} rendered: {rendered}"
            );
            assert!(
                !rendered.contains("Output language: ."),
                "setting {setting:?} left the placeholder empty"
            );
        }
    }

    /// Capture actual provider payloads without credentials or external requests.
    #[tokio::test]
    async fn requests_send_only_custom_instructions_and_keep_the_source_text() {
        use std::io::{Read, Write};
        for provider in ["openai", "compatible", "anthropic", "opencode-go"] {
            for placeholder in ["", "{{text}}", "{{transcript}}"] {
                let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
                let address = listener.local_addr().unwrap();
                let server = std::thread::spawn(move || {
                    let (mut stream, _) = listener.accept().unwrap();
                    stream
                        .set_read_timeout(Some(Duration::from_secs(5)))
                        .unwrap();
                    let mut request = Vec::new();
                    let (header_end, length) = loop {
                        let mut chunk = [0; 4096];
                        let count = stream.read(&mut chunk).unwrap();
                        assert!(count > 0);
                        request.extend_from_slice(&chunk[..count]);
                        if let Some(end) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                            let headers = String::from_utf8_lossy(&request[..end]);
                            let length = headers
                                .lines()
                                .find_map(|line| {
                                    let (name, value) = line.split_once(':')?;
                                    name.eq_ignore_ascii_case("content-length")
                                        .then(|| value.trim().parse::<usize>().unwrap())
                                })
                                .unwrap();
                            break (end + 4, length);
                        }
                    };
                    while request.len() < header_end + length {
                        let mut chunk = [0; 4096];
                        let count = stream.read(&mut chunk).unwrap();
                        assert!(count > 0);
                        request.extend_from_slice(&chunk[..count]);
                    }
                    let payload: serde_json::Value =
                        serde_json::from_slice(&request[header_end..header_end + length]).unwrap();
                    let body = r#"{"choices":[{"message":{"content":"Synthetic input."}}],"content":[{"type":"text","text":"Synthetic input."}]}"#;
                    write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
                    payload
                });
                let mut cfg = base_config();
                cfg.provider = provider.to_string();
                cfg.model = "synthetic-model".to_string();
                cfg.base_url = Some(format!("http://{address}"));
                cfg.system_prompt = format!("Use my rules only.{placeholder}");
                let outcome =
                    ai_process_text_with_status("Synthetic input.", &cfg, Some("synthetic-key"))
                        .await;
                assert!(outcome.status.used, "{provider}: {:?}", outcome.status);
                let payload = server.join().unwrap();
                let (system, source) = if provider == "anthropic" {
                    (&payload["system"], &payload["messages"][0]["content"])
                } else {
                    (
                        &payload["messages"][0]["content"],
                        &payload["messages"][1]["content"],
                    )
                };
                assert_eq!(system, "Use my rules only.", "{provider}");
                assert_eq!(
                    source, "<dictation>\nSynthetic input.\n</dictation>",
                    "{provider}"
                );
            }
        }
    }

    #[tokio::test]
    async fn ai_process_text_passes_through_in_local_mode() {
        let mut cfg = base_config();
        cfg.pipeline_mode = "local".to_string();
        let text = ai_process_text("hello", &cfg, Some("sk-test")).await;
        assert_eq!(text, "hello");
    }

    // ------------------------------------------------------------------
    // Retry loop against a scripted provider
    // ------------------------------------------------------------------

    struct MockProvider {
        outcomes: std::sync::Mutex<Vec<Result<(String, ProviderInfo), ProviderError>>>,
        calls: std::sync::atomic::AtomicUsize,
    }

    impl Provider for MockProvider {
        fn name(&self) -> &'static str {
            "mock"
        }

        fn complete<'a>(&'a self, _system_prompt: &'a str, _text: &'a str) -> CompletionFuture<'a> {
            let index = self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let outcome = self
                .outcomes
                .lock()
                .unwrap()
                .get(index)
                .cloned()
                .unwrap_or_else(|| {
                    Err(ProviderError::new(
                        ProviderErrorType::ProviderFailed,
                        "mock exhausted",
                    ))
                });
            Box::pin(async move { outcome })
        }
    }

    #[tokio::test]
    async fn call_provider_with_retry_retries_a_transient_error() {
        let timeout = ProviderError::new(ProviderErrorType::Timeout, "timeout");
        let success = ProviderInfo::success("answer", None, 0.1);
        let provider = Arc::new(MockProvider {
            outcomes: std::sync::Mutex::new(vec![
                Err(timeout),
                Ok(("answer".to_string(), success)),
            ]),
            calls: std::sync::atomic::AtomicUsize::new(0),
        });
        let mut status = AiStatus::default();

        let (text, info) = call_provider_with_retry(
            provider.clone(),
            "system",
            "text",
            Duration::from_secs(4),
            Duration::from_secs(12),
            &mut status,
        )
        .await;

        assert_eq!(text.as_deref(), Some("answer"));
        assert_eq!(info.attempts, 2, "one failure + one success = two attempts");
        assert_eq!(
            provider.calls.load(std::sync::atomic::Ordering::SeqCst),
            2,
            "the provider must have been called twice"
        );
    }

    /// 69 of 100 words is under the fidelity line. The paste must stay the
    /// dictation, and the status must name this fallback.
    #[tokio::test]
    async fn a_shortened_answer_falls_back_to_the_dictation() {
        let input = "слово ".repeat(100);
        let provider = Arc::new(MockProvider {
            outcomes: std::sync::Mutex::new(vec![Ok((
                "слово ".repeat(69),
                ProviderInfo::success("ignored", None, 0.1),
            ))]),
            calls: std::sync::atomic::AtomicUsize::new(0),
        });
        let outcome = finish_with_provider(
            &input,
            &base_config(),
            provider.clone(),
            "system",
            "user",
            Duration::from_secs(12),
            AiStatus::default(),
        )
        .await;
        assert_eq!(outcome.text, input);
        assert!(outcome.status.fallback);
        assert!(!outcome.status.used);
        assert_eq!(
            outcome.status.error_type.as_deref(),
            Some("summarised_response")
        );
        assert_eq!(outcome.status.skipped_reason, "model_dropped_text");
    }

    /// Punctuation at the same word count is the edit the prompt asks for.
    #[tokio::test]
    async fn punctuation_at_the_same_length_is_kept() {
        let input = "слово ".repeat(40);
        let tidied = format!("{},", input.trim().replace(' ', ", "));
        let provider = Arc::new(MockProvider {
            outcomes: std::sync::Mutex::new(vec![Ok((
                tidied.clone(),
                ProviderInfo::success("ignored", None, 0.1),
            ))]),
            calls: std::sync::atomic::AtomicUsize::new(0),
        });
        let outcome = finish_with_provider(
            &input,
            &base_config(),
            provider.clone(),
            "system",
            "user",
            Duration::from_secs(12),
            AiStatus::default(),
        )
        .await;
        assert_eq!(outcome.text, tidied);
        assert!(!outcome.status.fallback);
        assert!(outcome.status.used);
        assert!(outcome.status.error_type.is_none());
        assert!(outcome.status.skipped_reason.is_empty());
    }

    /// A short answer without its «never» is too short for the word ratio to
    /// judge; the paste must still stay the dictation.
    #[tokio::test]
    async fn an_answer_without_the_negation_falls_back_to_the_dictation() {
        let input = "I'd prefer to never merge this";
        let provider = Arc::new(MockProvider {
            outcomes: std::sync::Mutex::new(vec![Ok((
                "I'd prefer to merge this.".to_string(),
                ProviderInfo::success("ignored", None, 0.1),
            ))]),
            calls: std::sync::atomic::AtomicUsize::new(0),
        });
        let outcome = finish_with_provider(
            input,
            &base_config(),
            provider.clone(),
            "system",
            "user",
            Duration::from_secs(12),
            AiStatus::default(),
        )
        .await;
        assert_eq!(outcome.text, input);
        assert!(outcome.status.fallback);
        assert!(!outcome.status.used);
        assert_eq!(
            outcome.status.error_type.as_deref(),
            Some("altered_response")
        );
        assert_eq!(outcome.status.skipped_reason, "model_dropped_negation");
    }
    struct SlowProvider;
    impl Provider for SlowProvider {
        fn name(&self) -> &'static str {
            "slow"
        }
        fn complete<'a>(&'a self, _: &'a str, _: &'a str) -> CompletionFuture<'a> {
            Box::pin(async {
                tokio::time::sleep(Duration::from_secs(50)).await;
                Ok(("late".into(), ProviderInfo::default()))
            })
        }
    }

    /// Rewriting a long dictation takes the model longer than a phrase; the
    /// configured timeout, not a fixed per-attempt cap, decides when to give up.
    #[tokio::test(start_paused = true)]
    async fn a_slow_answer_within_the_configured_timeout_is_kept() {
        struct SixSecondProvider;
        impl Provider for SixSecondProvider {
            fn name(&self) -> &'static str {
                "six-seconds"
            }
            fn complete<'a>(&'a self, _: &'a str, text: &'a str) -> CompletionFuture<'a> {
                Box::pin(async move {
                    tokio::time::sleep(Duration::from_secs(6)).await;
                    Ok((text.to_string(), ProviderInfo::success(text, None, 6.0)))
                })
            }
        }
        let input = "слово ".repeat(40);
        let outcome = finish_with_provider(
            &input,
            &base_config(),
            Arc::new(SixSecondProvider),
            "system",
            &input,
            Duration::from_secs(12),
            AiStatus::default(),
        )
        .await;
        assert!(
            outcome.status.used,
            "{:?}",
            outcome.status.provider_attempts
        );
        assert_eq!(outcome.status.attempts, 1);
    }

    #[tokio::test(start_paused = true)]
    async fn actual_attempts_and_backoff_respect_the_overall_deadline() {
        let mut status = AiStatus::default();
        let (text, info) = call_provider_with_retry(
            Arc::new(SlowProvider),
            "system",
            "text",
            Duration::from_secs(4),
            Duration::from_secs(5),
            &mut status,
        )
        .await;
        assert!(text.is_none());
        assert_eq!(info.attempts, 2);
        assert_eq!(info.error_type, Some(ProviderErrorType::Timeout));
        assert!((info.elapsed_seconds - 5.0).abs() < 0.01);
        assert!((status.provider_attempts[0].elapsed_seconds - 4.0).abs() < 0.01);
        assert!((status.provider_attempts[1].elapsed_seconds - 0.7).abs() < 0.01);
    }

    #[tokio::test(start_paused = true)]
    async fn an_attempt_timeout_does_not_wait_for_a_late_success() {
        let mut status = AiStatus::default();
        let (text, info) = call_provider_with_retry(
            Arc::new(SlowProvider),
            "system",
            "text",
            Duration::from_secs(4),
            Duration::from_secs(12),
            &mut status,
        )
        .await;
        assert!(text.is_none());
        assert!((info.elapsed_seconds - 8.3).abs() < 0.01);
        assert_eq!(info.attempts, 2);
    }

    /// Echoes the text it was given, `.0` seconds later.
    struct DelayedEcho(u64);
    impl Provider for DelayedEcho {
        fn name(&self) -> &'static str {
            "delayed-echo"
        }
        fn complete<'a>(&'a self, _: &'a str, text: &'a str) -> CompletionFuture<'a> {
            let delay = self.0;
            Box::pin(async move {
                tokio::time::sleep(Duration::from_secs(delay)).await;
                Ok((text.to_string(), ProviderInfo::success(text, None, 0.0)))
            })
        }
    }

    #[tokio::test(start_paused = true)]
    async fn an_answer_after_the_timeout_is_kept_as_a_late_answer() {
        let input = "слово ".repeat(40);
        let outcome = finish_with_provider(
            &input,
            &base_config(),
            Arc::new(DelayedEcho(20)),
            "system",
            &input,
            Duration::from_secs(12),
            AiStatus::default(),
        )
        .await;
        assert!(
            outcome.status.fallback,
            "the paste does not wait past the timeout"
        );
        assert_eq!(outcome.text, input);
        let late = outcome.late.expect("the request keeps running");
        let answer = late
            .wait()
            .await
            .expect("the answer arrives within the grace");
        assert!(answer.status.used);
        assert!(answer.status.late);
        assert!(!answer.status.fallback);
        assert!(answer.status.skipped_reason.is_empty());
        assert!((answer.status.elapsed_seconds - 20.0).abs() < 0.01);
    }

    #[tokio::test(start_paused = true)]
    async fn a_late_answer_past_the_grace_is_given_up() {
        let input = "слово ".repeat(40);
        let outcome = finish_with_provider(
            &input,
            &base_config(),
            Arc::new(DelayedEcho(12 + LATE_ANSWER_GRACE.as_secs() + 10)),
            "system",
            &input,
            Duration::from_secs(12),
            AiStatus::default(),
        )
        .await;
        assert!(outcome
            .late
            .expect("still running at the timeout")
            .wait()
            .await
            .is_none());
    }

    #[tokio::test(start_paused = true)]
    async fn late_answers_turned_off_cancel_the_request_at_the_timeout() {
        let input = "слово ".repeat(40);
        let mut config = base_config();
        config.late_answer = LateAnswerMode::Off;
        let outcome = finish_with_provider(
            &input,
            &config,
            Arc::new(DelayedEcho(20)),
            "system",
            &input,
            Duration::from_secs(12),
            AiStatus::default(),
        )
        .await;
        assert!(outcome.status.fallback);
        assert!(outcome.late.is_none());
    }
}
