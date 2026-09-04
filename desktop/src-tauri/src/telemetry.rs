//! Privacy-preserving product telemetry.
//!
//! Feature code can only construct the typed events in this module.  The
//! serde/JSON boundary is private to the module, after the allowlist has been
//! applied. Producers use a bounded, non-blocking channel; SQLite and HTTP
//! work run on the telemetry worker and never on audio, hotkey, whisper, or
//! paste critical paths.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use serde_json::{Map, Value};
use uuid::Uuid;

use crate::config::Config;

pub const POSTHOG_CAPTURE_URL: &str = "https://eu.i.posthog.com/capture/";

const TELEMETRY_ENABLED_KEY: &str = "telemetry_enabled";
const SESSION_TIMEOUT_KEY: &str = "telemetry_session_timeout_minutes";
const META_INSTALLATION_ID: &str = "installation_id";
const CHANNEL_CAPACITY: usize = 256;
const FLUSH_INTERVAL: Duration = Duration::from_secs(5);
const SESSION_WATCH_INTERVAL: Duration = Duration::from_secs(15);
const BATCH_SIZE: i64 = 20;
const MAX_OUTBOX_ROWS: i64 = 1_000;
const OUTBOX_TTL_SECONDS: f64 = 7.0 * 24.0 * 3600.0;
const MAX_ATTEMPTS: i64 = 8;
const HTTP_TIMEOUT: Duration = Duration::from_secs(15);
const SCHEMA_VERSION: u64 = 1;

/// The only authority on the usage-session inactivity range. Every writer
/// clamps to it: a hand-edited config must not make the app refuse unrelated
/// saves, and the UI's own min/max is a convenience, not a second contract.
pub const SESSION_TIMEOUT_MINUTES: std::ops::RangeInclusive<u64> = 5..=120;
pub const DEFAULT_SESSION_TIMEOUT_MINUTES: u64 = 30;

fn clamp_session_timeout(minutes: u64) -> u64 {
    minutes.clamp(
        *SESSION_TIMEOUT_MINUTES.start(),
        *SESSION_TIMEOUT_MINUTES.end(),
    )
}

/// Product policy: telemetry is enabled unless the user explicitly opts out.
pub fn enabled_from_config(config: Option<&Config>) -> bool {
    config
        .map(|cfg| enabled_from_value(cfg.as_value()))
        .unwrap_or(true)
}

pub fn enabled_from_value(value: &Value) -> bool {
    value
        .get(TELEMETRY_ENABLED_KEY)
        .and_then(Value::as_bool)
        .unwrap_or(true)
}

/// Return the configured session timeout, clamped to the supported range.
pub fn session_timeout_minutes_from_config(config: Option<&Config>) -> u64 {
    config
        .map(|cfg| session_timeout_minutes_from_value(cfg.as_value()))
        .unwrap_or(DEFAULT_SESSION_TIMEOUT_MINUTES)
}

pub fn session_timeout_minutes_from_value(value: &Value) -> u64 {
    clamp_session_timeout(
        value
            .get(SESSION_TIMEOUT_KEY)
            .and_then(Value::as_u64)
            .unwrap_or(DEFAULT_SESSION_TIMEOUT_MINUTES),
    )
}

fn now_seconds() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

fn random_id() -> String {
    Uuid::new_v4().to_string()
}

fn build_api_key() -> Option<&'static str> {
    // This is the public PostHog ingest token, not a user/provider secret. A
    // missing build key is a supported dev/test configuration and makes the
    // entire path a no-op.
    let key = option_env!("SOTTO_POSTHOG_API_KEY")
        .or(option_env!("POSTHOG_API_KEY"))
        .map(str::trim)?;
    (!key.is_empty()).then_some(key)
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionTrigger {
    Microphone,
    File,
    /// Any explicit LLM utility action (manual run, prompt test, history
    /// retry). They are session activity but never an in-flight transcription.
    Llm,
}

impl SessionTrigger {
    /// Only microphone/file work can straddle the inactivity timeout, so only
    /// those hold the session open against the idle watcher.
    fn is_transcription(self) -> bool {
        matches!(self, Self::Microphone | Self::File)
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureStage {
    Start,
    Capture,
    Queue,
    Decode,
    Stt,
    PostProcess,
}

#[derive(Debug, Clone, Copy)]
pub enum FailureReason {
    EngineBusy,
    NoTranscriptionRoute,
    RecorderStart,
    NoAudio,
    RecorderStop,
    CloudConfiguration,
    EngineQueue,
    EngineError,
    Decode,
    EmptyTranscript,
    EmptyAfterProcessing,
    UserCancelled,
}

impl FailureReason {
    fn wire(self) -> &'static str {
        match self {
            Self::NoAudio => "no_audio",
            Self::RecorderStart => "permission_denied",
            Self::NoTranscriptionRoute | Self::CloudConfiguration => "model_unavailable",
            Self::UserCancelled => "cancelled",
            Self::EngineBusy => "other",
            Self::RecorderStop | Self::Decode => "other",
            Self::EngineQueue | Self::EngineError => "provider_unavailable",
            Self::EmptyTranscript | Self::EmptyAfterProcessing => "empty",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum PasteResult {
    Success,
    ClipboardOnly,
    Failed,
    NotApplicable,
}

impl PasteResult {
    fn wire(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::ClipboardOnly => "clipboard_only",
            Self::Failed => "failed",
            Self::NotApplicable => "not_applicable",
        }
    }
}

/// Which entry point produced the audio.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Microphone,
    File,
}

impl Source {
    fn wire(self) -> &'static str {
        match self {
            Self::Microphone => "microphone",
            Self::File => "file",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TerminalKind {
    Completed,
    Failed,
    Cancelled,
}

/// Where the speech model actually ran. Cloud mode overrides whatever the
/// local device setting says, so the caller reports what it configured and
/// the pipeline mode has the last word.
#[derive(Debug, Clone, Copy)]
pub enum Compute {
    Cpu,
    Gpu,
}

#[derive(Debug, Clone, Copy)]
pub enum RecordingMode {
    PushToTalk,
    Toggle,
    NotApplicable,
}

impl RecordingMode {
    fn wire(self) -> &'static str {
        match self {
            Self::PushToTalk => "push_to_talk",
            Self::Toggle => "toggle",
            Self::NotApplicable => "not_applicable",
        }
    }
}

/// Everything a terminal transcription reports. Grouped because the five
/// former entry points had grown to fourteen positional arguments each, and
/// the duplicated argument lists had already drifted apart.
pub struct Outcome<'a> {
    pub source: Source,
    pub pipeline_mode: &'a str,
    pub recording_mode: RecordingMode,
    pub stt_model: Option<&'a str>,
    pub audio_seconds: f64,
    pub stt_millis: u64,
    pub chars: usize,
    pub ai_status: Option<&'a crate::ai::step::AiStatus>,
    /// `None` where the route never got far enough to know — a failure before
    /// the engine ran reports `other`, not a guess.
    pub compute: Option<Compute>,
    pub formatting_enabled: bool,
    pub replacement_rules: usize,
    pub paste_result: PasteResult,
}

impl<'a> Outcome<'a> {
    /// The shape a failure or a cancellation can honestly fill in.
    fn bare(source: Source, pipeline_mode: &'a str) -> Self {
        Self {
            source,
            pipeline_mode,
            recording_mode: RecordingMode::NotApplicable,
            stt_model: None,
            audio_seconds: 0.0,
            stt_millis: 0,
            chars: 0,
            ai_status: None,
            compute: None,
            formatting_enabled: false,
            replacement_rules: 0,
            paste_result: PasteResult::NotApplicable,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct AppStartedPayload {
    start_mode: &'static str,
    ui_language: &'static str,
}

#[derive(Debug, Clone, Serialize)]
struct UsageFinishedPayload {
    duration_seconds: u64,
    transcription_count: u64,
    success_count: u64,
    failure_count: u64,
    cancel_count: u64,
    audio_seconds: u64,
    time_saved_seconds: u64,
    dominant_pipeline_mode: &'static str,
    timeout_minutes: u64,
}

#[derive(Debug, Clone, Serialize)]
struct OutcomePayload {
    source: &'static str,
    pipeline_mode: &'static str,
    recording_mode: &'static str,
    stt_engine: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    stt_provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stt_model: Option<String>,
    compute: &'static str,
    audio_seconds: u64,
    processing_seconds: u64,
    time_saved_seconds: u64,
    output_length_bucket: &'static str,
    llm_attempted: bool,
    llm_used: bool,
    llm_fallback: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    llm_provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    llm_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    llm_fallback_reason: Option<&'static str>,
    paste_result: &'static str,
    formatting_enabled: bool,
    replacements_applied_bucket: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    stage: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<&'static str>,
}

#[derive(Debug, Clone)]
enum TelemetryEvent {
    AppStarted(AppStartedPayload),
    UsageSessionFinished(UsageFinishedPayload),
    TranscriptionCompleted(OutcomePayload),
    TranscriptionFailed(OutcomePayload),
    TranscriptionCancelled(OutcomePayload),
}

impl TelemetryEvent {
    fn name(&self) -> &'static str {
        match self {
            Self::AppStarted(_) => "app.started",
            Self::UsageSessionFinished(_) => "usage_session.finished",
            Self::TranscriptionCompleted(_) => "transcription.completed",
            Self::TranscriptionFailed(_) => "transcription.failed",
            Self::TranscriptionCancelled(_) => "transcription.cancelled",
        }
    }

    fn properties(&self) -> Result<Map<String, Value>, serde_json::Error> {
        let value = match self {
            Self::AppStarted(payload) => serde_json::to_value(payload)?,
            Self::UsageSessionFinished(payload) => serde_json::to_value(payload)?,
            Self::TranscriptionCompleted(payload) => serde_json::to_value(payload)?,
            Self::TranscriptionFailed(payload) => serde_json::to_value(payload)?,
            Self::TranscriptionCancelled(payload) => serde_json::to_value(payload)?,
        };
        Ok(value.as_object().cloned().unwrap_or_default())
    }
}

#[derive(Debug, Clone)]
struct QueuedEvent {
    event_id: String,
    event_name: &'static str,
    properties_json: String,
    created_at: f64,
}

#[derive(Debug, Clone)]
struct OutboxRow {
    event_id: String,
    event_name: String,
    payload_json: String,
    attempts: i64,
}

#[derive(Debug, Clone)]
struct UsageAccumulator {
    id: String,
    started_at: Instant,
    last_activity: Instant,
    transcription_count: u64,
    success_count: u64,
    failure_count: u64,
    cancel_count: u64,
    audio_seconds: f64,
    time_saved_seconds: f64,
    mode_counts: [u64; MODES.len()],
    /// Prevent the idle watcher from splitting one long transcription.
    /// Explicit LLM utility actions are instantaneous session activity and
    /// therefore do not increment this counter.
    active_transcriptions: u64,
}

impl UsageAccumulator {
    fn started(now: Instant) -> Self {
        Self {
            id: random_id(),
            started_at: now,
            last_activity: now,
            transcription_count: 0,
            success_count: 0,
            failure_count: 0,
            cancel_count: 0,
            audio_seconds: 0.0,
            time_saved_seconds: 0.0,
            mode_counts: [0; MODES.len()],
            active_transcriptions: 0,
        }
    }
}

/// `mode_counts` is positional; this is the one place the order is defined.
const MODES: [&str; 3] = ["local", "hybrid", "cloud"];

fn mode_index(mode: &str) -> Option<usize> {
    MODES.iter().position(|known| *known == mode)
}

/// A session ends only once it has been idle for the whole timeout *and* has
/// no transcription still running — a long file job must stay one session.
fn take_expired(
    usage: &mut UsageState,
    now: Instant,
    timeout: Duration,
) -> Option<UsageAccumulator> {
    let expired = usage.current.as_ref().is_some_and(|current| {
        current.active_transcriptions == 0 && now.duration_since(current.last_activity) >= timeout
    });
    expired.then(|| usage.current.take()).flatten()
}

#[derive(Debug, Default)]
struct UsageState {
    current: Option<UsageAccumulator>,
}

/// Cloneable handle used by Tauri commands and all pipeline entry points.
#[derive(Clone)]
pub struct Telemetry {
    enabled: Arc<AtomicBool>,
    timeout_seconds: Arc<AtomicU64>,
    installation_id: Arc<String>,
    usage: Arc<Mutex<UsageState>>,
    tx: tokio::sync::mpsc::Sender<QueuedEvent>,
}

impl Telemetry {
    pub fn new(db: Arc<Mutex<Connection>>, config: Option<&Config>) -> Self {
        let installation_id = {
            let conn = crate::mutex_recover::lock(&db);
            match ensure_installation_id(&conn) {
                Ok(value) => value,
                Err(error) => {
                    // Product telemetry must never make startup fail. This
                    // fallback is only used if SQLite itself is unavailable.
                    log::warn!("telemetry installation id unavailable: {error}");
                    random_id()
                }
            }
        };
        let (tx, rx) = tokio::sync::mpsc::channel(CHANNEL_CAPACITY);
        let telemetry = Self {
            enabled: Arc::new(AtomicBool::new(enabled_from_config(config))),
            timeout_seconds: Arc::new(AtomicU64::new(
                session_timeout_minutes_from_config(config) * 60,
            )),
            installation_id: Arc::new(installation_id),
            usage: Arc::new(Mutex::new(UsageState::default())),
            tx,
        };
        // Without a build key nothing can ever be enqueued or sent, so the
        // flush ticker and the idle watcher would be two timers waking up
        // forever to take the shared DB mutex and find nothing. Dev and test
        // builds are exactly that case.
        if build_api_key().is_some() {
            let client = reqwest::Client::builder()
                .timeout(HTTP_TIMEOUT)
                .build()
                .unwrap_or_else(|_| reqwest::Client::new());
            let worker = Worker {
                db,
                enabled: Arc::clone(&telemetry.enabled),
                installation_id: Arc::clone(&telemetry.installation_id),
                client,
            };
            tauri::async_runtime::spawn(worker.run(rx));
            // Closing a session must not depend on another user action.  The
            // watcher only touches the in-memory accumulator and uses the
            // same bounded channel as terminal events, so it cannot block a
            // pipeline.
            tauri::async_runtime::spawn(telemetry.clone().run_session_watcher());
        }
        telemetry
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Acquire)
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Release);
        if !enabled {
            // A session that was active before opt-out must never be resumed
            // or reported after opt-in.  This deliberately drops only the
            // in-memory accumulator; durable outbox rows remain untouched.
            crate::mutex_recover::lock(&self.usage).current = None;
        }
    }

    pub fn session_timeout_minutes(&self) -> u64 {
        self.timeout_seconds.load(Ordering::Acquire) / 60
    }

    pub fn set_session_timeout_minutes(&self, minutes: u64) {
        self.timeout_seconds
            .store(clamp_session_timeout(minutes) * 60, Ordering::Release);
    }

    pub fn is_configured(&self) -> bool {
        build_api_key().is_some()
    }

    /// Mark product activity. The session starts at the first real user
    /// action (microphone/file/explicit LLM), not at app launch. When a new
    /// action arrives after the timeout, the old session is closed using the
    /// last activity instant, so idle time is never counted in its duration.
    pub fn begin_usage_session(&self, trigger: SessionTrigger) {
        if !self.accepting() {
            return;
        }
        let now = Instant::now();
        let timeout = self.session_timeout();
        let finished = {
            let mut usage = crate::mutex_recover::lock(&self.usage);
            let finished = take_expired(&mut usage, now, timeout);
            let current = usage
                .current
                .get_or_insert_with(|| UsageAccumulator::started(now));
            current.last_activity = now;
            if trigger.is_transcription() {
                current.active_transcriptions = current.active_transcriptions.saturating_add(1);
            }
            finished
        };
        self.emit_finished(finished, timeout_minutes(timeout));
    }

    async fn run_session_watcher(self) {
        let mut ticker = tokio::time::interval(SESSION_WATCH_INTERVAL);
        loop {
            ticker.tick().await;
            self.finish_expired_session();
        }
    }

    fn finish_expired_session(&self) {
        if !self.accepting() {
            return;
        }
        let timeout = self.session_timeout();
        let finished = take_expired(
            &mut crate::mutex_recover::lock(&self.usage),
            Instant::now(),
            timeout,
        );
        self.emit_finished(finished, timeout_minutes(timeout));
    }

    /// Close the current session during an orderly app exit. This is
    /// best-effort and does not wait for network delivery.
    pub fn finish_usage_session(&self) {
        if !self.accepting() {
            return;
        }
        let finished = crate::mutex_recover::lock(&self.usage).current.take();
        self.emit_finished(finished, self.session_timeout_minutes());
    }

    fn session_timeout(&self) -> Duration {
        Duration::from_secs(self.timeout_seconds.load(Ordering::Acquire))
    }

    fn emit_finished(&self, session: Option<UsageAccumulator>, timeout_minutes: u64) {
        let Some(session) = session else {
            return;
        };
        self.enqueue_for(
            TelemetryEvent::UsageSessionFinished(usage_payload(&session, timeout_minutes)),
            Some(session.id),
        );
    }

    pub fn record_app_started(&self, autostart: bool, ui_language: &str) {
        self.enqueue(TelemetryEvent::AppStarted(AppStartedPayload {
            start_mode: if autostart {
                "autostart"
            } else {
                "interactive"
            },
            ui_language: language_wire(ui_language),
        }));
    }

    /// One terminal event for a transcription that produced text, whether it
    /// reached the focused window, the clipboard, or only the file panel.
    pub fn record_completed(&self, outcome: Outcome<'_>) {
        let mode = mode_wire(outcome.pipeline_mode);
        self.touch_terminal(mode, TerminalKind::Completed, &outcome);
        self.enqueue(TelemetryEvent::TranscriptionCompleted(outcome_payload(
            mode, &outcome, None, None,
        )));
    }

    /// A user-visible failure. Reliability metrics are the point, so a
    /// deliberate cancellation goes to [`Telemetry::record_cancelled`].
    pub fn record_failed(
        &self,
        source: Source,
        pipeline_mode: &str,
        stage: FailureStage,
        reason: FailureReason,
    ) {
        self.record_terminal(
            source,
            pipeline_mode,
            TerminalKind::Failed,
            Some(stage),
            Some(reason),
        );
    }

    pub fn record_cancelled(&self, source: Source, pipeline_mode: &str) {
        self.record_terminal(
            source,
            pipeline_mode,
            TerminalKind::Cancelled,
            Some(FailureStage::Stt),
            Some(FailureReason::UserCancelled),
        );
    }

    /// Failures and cancellations know nothing but where they came from, so
    /// they share one shape: no model, no durations, no delivery result.
    fn record_terminal(
        &self,
        source: Source,
        pipeline_mode: &str,
        kind: TerminalKind,
        stage: Option<FailureStage>,
        reason: Option<FailureReason>,
    ) {
        let mode = mode_wire(pipeline_mode);
        let outcome = Outcome::bare(source, pipeline_mode);
        self.touch_terminal(mode, kind, &outcome);
        let payload = outcome_payload(mode, &outcome, stage, reason);
        self.enqueue(match kind {
            TerminalKind::Cancelled => TelemetryEvent::TranscriptionCancelled(payload),
            _ => TelemetryEvent::TranscriptionFailed(payload),
        });
    }

    fn accepting(&self) -> bool {
        self.is_enabled() && self.is_configured()
    }

    fn touch_terminal(&self, mode: &'static str, kind: TerminalKind, outcome: &Outcome<'_>) {
        if !self.accepting() {
            return;
        }
        let now = Instant::now();
        let mut usage = crate::mutex_recover::lock(&self.usage);
        // Terminal failures can happen before the normal start hook (for
        // example a missing model or recorder permission). They still count
        // as a real product action. Do not call `begin_usage_session` here:
        // a single file operation may legitimately outlive the inactivity
        // timeout and must remain one session until its terminal outcome.
        let current = usage
            .current
            .get_or_insert_with(|| UsageAccumulator::started(now));
        current.last_activity = now;
        current.transcription_count += 1;
        match kind {
            TerminalKind::Completed => current.success_count += 1,
            TerminalKind::Failed => current.failure_count += 1,
            TerminalKind::Cancelled => current.cancel_count += 1,
        }
        current.audio_seconds += finite_nonnegative(outcome.audio_seconds);
        current.time_saved_seconds += estimated_time_saved(outcome.chars);
        current.active_transcriptions = current.active_transcriptions.saturating_sub(1);
        if let Some(index) = mode_index(mode) {
            current.mode_counts[index] += 1;
        }
    }

    fn current_session_id(&self) -> Option<String> {
        crate::mutex_recover::lock(&self.usage)
            .current
            .as_ref()
            .map(|session| session.id.clone())
    }

    fn enqueue(&self, event: TelemetryEvent) {
        self.enqueue_for(event, self.current_session_id());
    }

    fn enqueue_for(&self, event: TelemetryEvent, usage_session_id: Option<String>) {
        if !self.accepting() {
            return;
        }
        let mut properties = match event.properties() {
            Ok(value) => value,
            Err(error) => {
                log::warn!("telemetry event serialization failed: {error}");
                return;
            }
        };
        add_common_properties(&mut properties);
        if let Some(session_id) = usage_session_id {
            properties.insert("usage_session_id".to_string(), Value::String(session_id));
        }
        let properties_json = match serde_json::to_string(&properties) {
            Ok(value) => value,
            Err(error) => {
                log::warn!("telemetry properties serialization failed: {error}");
                return;
            }
        };
        let queued = QueuedEvent {
            event_id: random_id(),
            event_name: event.name(),
            properties_json,
            created_at: now_seconds(),
        };
        // A telemetry outage must never add backpressure to dictation.
        let _ = self.tx.try_send(queued);
    }
}

fn add_common_properties(properties: &mut Map<String, Value>) {
    properties.insert("schema_version".to_string(), Value::from(SCHEMA_VERSION));
    properties.insert(
        "app_version".to_string(),
        Value::String(env!("CARGO_PKG_VERSION").to_string()),
    );
    properties.insert(
        "os".to_string(),
        Value::String(std::env::consts::OS.to_string()),
    );
    properties.insert("os_major".to_string(), Value::String("unknown".to_string()));
    properties.insert(
        "arch".to_string(),
        Value::String(std::env::consts::ARCH.to_string()),
    );
    properties.insert("$process_person_profile".to_string(), Value::Bool(false));
}

fn language_wire(language: &str) -> &'static str {
    match language.trim().to_lowercase().as_str() {
        "ru" => "ru",
        "en" => "en",
        _ => "other",
    }
}

fn mode_wire(mode: &str) -> &'static str {
    match mode {
        "local" => "local",
        "hybrid" => "hybrid",
        "cloud" => "cloud",
        _ => "other",
    }
}

fn provider_wire(provider: &str) -> Option<&'static str> {
    match provider.trim().to_lowercase().as_str() {
        "openai" => Some("openai"),
        "anthropic" => Some("anthropic"),
        "gemini" => Some("gemini"),
        "opencode-go" => Some("opencode-go"),
        "compatible" => Some("compatible"),
        _ => None,
    }
}

/// Cloud STT currently executes through the OpenAI-compatible adapter. Keep
/// the normalization strict so a future config value cannot turn into a
/// high-cardinality provider label. The call sites use the actual request's
/// fixed `CloudSttProvider::Compatible` value rather than the user endpoint.
fn cloud_stt_provider_wire(provider: &str) -> Option<&'static str> {
    match provider.trim().to_lowercase().as_str() {
        "compatible" | "openai-compatible" | "openai_compatible" => Some("compatible"),
        _ => None,
    }
}

const MAX_EXTERNAL_MODEL_CHARS: usize = 64;

/// Normalize a model supplied by a cloud/LLM provider without ever forwarding
/// paths, URLs, keys, error text, or arbitrary Unicode. A bounded ASCII label
/// is enough for dashboards; suspicious values collapse to a stable bucket.
fn external_model_wire(value: Option<&str>, fallback: &'static str) -> Option<String> {
    let value = value?.trim();
    if value.is_empty() {
        return None;
    }
    let lower = value.to_ascii_lowercase();
    if lower.starts_with("sk-")
        || lower.starts_with("rk-")
        || lower.starts_with("pk-")
        || lower.starts_with("gsk_")
        || lower.starts_with("csk-")
        || lower.starts_with("xai-")
        || lower.starts_with("aiza")
        || lower.starts_with("api-")
        || lower.contains("api_key")
        || lower.starts_with("key_")
        || lower.starts_with("secret_")
        || lower.starts_with("bearer")
        || lower.starts_with("token_")
        || lower.starts_with("eyj")
        || lower.contains("://")
        || lower.starts_with('/')
        || lower.starts_with('\\')
        || lower.as_bytes().get(1) == Some(&b':')
        || lower.contains("../")
        || lower.contains("/..")
    {
        return Some(fallback.to_string());
    }
    let mut normalized = String::with_capacity(lower.len());
    for byte in lower.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':' | b'@') {
            normalized.push(byte as char);
        } else if byte == b'/' {
            // Namespaced model IDs such as `org/model` are common. Keep the
            // namespace while removing the path separator itself.
            normalized.push('_');
        } else {
            return Some(fallback.to_string());
        }
    }
    if normalized.len() > MAX_EXTERNAL_MODEL_CHARS {
        normalized.truncate(MAX_EXTERNAL_MODEL_CHARS);
    }
    Some(normalized)
}

/// Bundled local model IDs are a closed allowlist. Models discovered in the
/// cache (including filenames and fine-tunes) are intentionally coalesced so
/// no local path/model name leaves the device.
fn local_stt_model_wire(value: Option<&str>) -> Option<String> {
    let value = value?.trim().to_ascii_lowercase();
    let known = match value.as_str() {
        "tiny" => "tiny",
        "base" => "base",
        "small" => "small",
        "medium" => "medium",
        "large-v3" => "large-v3",
        "turbo" => "turbo",
        "gigaam-v3" => "gigaam-v3",
        _ => "custom_local",
    };
    Some(known.to_string())
}

fn stt_model_wire(mode: &'static str, value: Option<&str>) -> Option<String> {
    if mode == "cloud" {
        external_model_wire(value, "custom_cloud")
    } else {
        local_stt_model_wire(value)
    }
}

fn stt_provider_wire(mode: &'static str, completed_operation: bool) -> Option<String> {
    if mode == "cloud" {
        if completed_operation {
            cloud_stt_provider_wire("compatible").map(str::to_string)
        } else {
            None
        }
    } else {
        // Local is a fixed product route, not a user/provider label, so it is
        // safe to retain it even when a local setup/capture failure occurs.
        Some("local".to_string())
    }
}

fn llm_reason_wire(status: &crate::ai::step::AiStatus) -> Option<&'static str> {
    match status
        .error_type
        .as_deref()
        .or_else(|| (!status.skipped_reason.is_empty()).then_some(status.skipped_reason.as_str()))
    {
        Some("auth_error") | Some("missing_api_key") => Some("auth"),
        Some("rate_limit") | Some("provider_quota_or_rate_limit") => Some("rate_limit"),
        Some("timeout") | Some("provider_timeout") => Some("timeout"),
        Some("connection_error") | Some("provider_connection_error") => Some("connection"),
        Some("bad_response") | Some("provider_bad_response") => Some("bad_response"),
        Some(_) => Some("other"),
        None => None,
    }
}

/// Exact model labels are emitted only after the provider returned a usable
/// response. Transport/auth/model errors prove only that a string was sent,
/// not that the provider recognizes it as a model ID.
fn llm_model_was_accepted(status: &crate::ai::step::AiStatus) -> bool {
    status.used
        || matches!(
            status.error_type.as_deref(),
            Some("empty_response" | "meta_response" | "summarised_response")
        )
}

fn stt_engine_wire(mode: &'static str, model: Option<&str>) -> &'static str {
    if mode == "cloud" {
        return "cloud_stt";
    }
    match model.unwrap_or("") {
        "gigaam-v3" => "sherpa",
        "tiny" | "base" | "small" | "medium" | "large-v3" | "turbo" => "whisper",
        _ => "other",
    }
}

fn compute_wire(mode: &'static str, compute: Option<Compute>) -> &'static str {
    if mode == "cloud" {
        return "cloud";
    }
    match compute {
        Some(Compute::Cpu) => "cpu",
        Some(Compute::Gpu) => "gpu",
        None => "other",
    }
}

fn outcome_payload(
    mode: &'static str,
    outcome: &Outcome<'_>,
    stage: Option<FailureStage>,
    reason: Option<FailureReason>,
) -> OutcomePayload {
    let (llm_attempted, llm_used, llm_fallback, llm_provider, llm_model, llm_reason) =
        match outcome.ai_status {
            Some(status) => {
                let actual_operation = status.attempted || status.used || status.fallback;
                (
                    status.attempted,
                    status.used,
                    status.fallback,
                    actual_operation
                        .then(|| provider_wire(&status.provider).map(str::to_string))
                        .flatten(),
                    llm_model_was_accepted(status)
                        .then(|| external_model_wire(Some(&status.model), "custom_llm"))
                        .flatten(),
                    llm_reason_wire(status),
                )
            }
            None => (false, false, false, None, None, None),
        };
    OutcomePayload {
        source: outcome.source.wire(),
        pipeline_mode: mode,
        recording_mode: outcome.recording_mode.wire(),
        stt_engine: stt_engine_wire(mode, outcome.stt_model),
        stt_provider: stt_provider_wire(mode, stage.is_none()),
        stt_model: stt_model_wire(mode, outcome.stt_model),
        compute: compute_wire(mode, outcome.compute),
        audio_seconds: round_audio_seconds(outcome.audio_seconds),
        processing_seconds: round_processing_seconds(outcome.stt_millis, outcome.ai_status),
        time_saved_seconds: round_time_saved(outcome.chars),
        output_length_bucket: output_length_bucket(outcome.chars),
        llm_attempted,
        llm_used,
        llm_fallback,
        llm_provider,
        llm_model,
        llm_fallback_reason: if llm_fallback { llm_reason } else { None },
        paste_result: outcome.paste_result.wire(),
        formatting_enabled: outcome.formatting_enabled,
        replacements_applied_bucket: replacements_bucket(outcome.replacement_rules),
        stage: stage.map(stage_wire),
        reason: reason.map(FailureReason::wire),
    }
}

fn stage_wire(stage: FailureStage) -> &'static str {
    match stage {
        FailureStage::Start | FailureStage::Capture => "capture",
        FailureStage::Queue | FailureStage::Decode => "stt_setup",
        FailureStage::Stt => "stt",
        FailureStage::PostProcess => "format",
    }
}

fn replacements_bucket(count: usize) -> &'static str {
    match count {
        0 => "0",
        1..=5 => "1_5",
        6..=20 => "6_20",
        _ => "gte_21",
    }
}

fn output_length_bucket(value: usize) -> &'static str {
    match value {
        0 => "0",
        1..=80 => "1_80",
        81..=500 => "81_500",
        501..=2_000 => "501_2000",
        2_001..=10_000 => "2001_10000",
        _ => "gte_10001",
    }
}

fn finite_nonnegative(value: f64) -> f64 {
    if value.is_finite() && value > 0.0 {
        value
    } else {
        0.0
    }
}

fn round_audio_seconds(value: f64) -> u64 {
    (finite_nonnegative(value).min(86_400.0) / 10.0).round() as u64 * 10
}

fn round_processing_seconds(stt_millis: u64, ai_status: Option<&crate::ai::step::AiStatus>) -> u64 {
    let llm = ai_status
        .map(|status| finite_nonnegative(status.elapsed_seconds))
        .unwrap_or(0.0);
    ((stt_millis as f64 / 1000.0 + llm).min(86_400.0)).round() as u64
}

/// Deliberately the flat fallback rate rather than the user's configured
/// typing speed: a per-user rate would make the aggregate incomparable
/// across installations. Shares the constant with the statistics writer so
/// the two cannot drift.
fn estimated_time_saved(chars: usize) -> f64 {
    chars as f64 * 60.0 / crate::stats::TIME_SAVED_CPM_FALLBACK
}

fn round_time_saved(chars: usize) -> u64 {
    estimated_time_saved(chars).min(86_400.0).round() as u64
}

fn usage_payload(session: &UsageAccumulator, timeout_minutes: u64) -> UsageFinishedPayload {
    let elapsed = session
        .last_activity
        .saturating_duration_since(session.started_at)
        .as_secs_f64()
        .min(86_400.0);
    let dominant = session
        .mode_counts
        .iter()
        .enumerate()
        .max_by_key(|(_, count)| *count)
        .and_then(|(index, _)| MODES.get(index).copied())
        .unwrap_or("other");
    UsageFinishedPayload {
        duration_seconds: elapsed.round() as u64,
        transcription_count: session.transcription_count,
        success_count: session.success_count,
        failure_count: session.failure_count,
        cancel_count: session.cancel_count,
        audio_seconds: round_audio_seconds(session.audio_seconds),
        time_saved_seconds: session.time_saved_seconds.min(86_400.0).round() as u64,
        dominant_pipeline_mode: dominant,
        timeout_minutes,
    }
}

fn timeout_minutes(timeout: Duration) -> u64 {
    clamp_session_timeout(timeout.as_secs() / 60)
}

fn ensure_installation_id(conn: &Connection) -> Result<String, rusqlite::Error> {
    let existing: Option<String> = conn
        .query_row(
            "SELECT value FROM telemetry_meta WHERE key = ?1",
            [META_INSTALLATION_ID],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(value) = existing {
        if Uuid::parse_str(&value).is_ok() {
            return Ok(value);
        }
    }
    let value = random_id();
    conn.execute(
        "INSERT INTO telemetry_meta(key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![META_INSTALLATION_ID, value],
    )?;
    Ok(value)
}

struct Worker {
    db: Arc<Mutex<Connection>>,
    enabled: Arc<AtomicBool>,
    installation_id: Arc<String>,
    client: reqwest::Client,
}

impl Worker {
    async fn run(self, mut rx: tokio::sync::mpsc::Receiver<QueuedEvent>) {
        let mut ticker = tokio::time::interval(FLUSH_INTERVAL);
        loop {
            tokio::select! {
                event = rx.recv() => {
                    match event {
                        Some(event) if self.enabled.load(Ordering::Acquire) => self.persist(event).await,
                        Some(_) => {},
                        None => break,
                    }
                }
                _ = ticker.tick() => self.flush().await,
            }
        }
        self.flush().await;
    }

    async fn persist(&self, event: QueuedEvent) {
        let db = Arc::clone(&self.db);
        let _ = tokio::task::spawn_blocking(move || {
            let conn = crate::mutex_recover::lock(&db);
            insert_outbox(&conn, &event)
        })
        .await;
    }

    async fn flush(&self) {
        if !self.enabled.load(Ordering::Acquire) {
            return;
        }
        let db = Arc::clone(&self.db);
        let rows = match tokio::task::spawn_blocking(move || {
            let conn = crate::mutex_recover::lock(&db);
            let _ = prune_outbox(&conn);
            pending_outbox(&conn, BATCH_SIZE, now_seconds())
        })
        .await
        {
            Ok(Ok(rows)) => rows,
            _ => return,
        };
        for row in rows {
            if !self.enabled.load(Ordering::Acquire) {
                return;
            }
            match send_capture(&row, &self.installation_id, &self.client).await {
                Delivery::Success => self.delete(&row.event_id).await,
                Delivery::Drop(category) => {
                    log::debug!("telemetry event dropped ({category})");
                    self.delete(&row.event_id).await;
                }
                Delivery::Retry(category) => self.retry(&row, category).await,
            }
        }
    }

    async fn delete(&self, event_id: &str) {
        let db = Arc::clone(&self.db);
        let event_id = event_id.to_string();
        let _ = tokio::task::spawn_blocking(move || {
            let conn = crate::mutex_recover::lock(&db);
            conn.execute(
                "DELETE FROM telemetry_outbox WHERE event_id = ?1",
                [event_id],
            )
        })
        .await;
    }

    async fn retry(&self, row: &OutboxRow, category: &'static str) {
        let db = Arc::clone(&self.db);
        let id = row.event_id.clone();
        let attempts = row.attempts + 1;
        let next = now_seconds() + retry_delay_seconds(attempts, &id);
        let _ = tokio::task::spawn_blocking(move || {
            let conn = crate::mutex_recover::lock(&db);
            if attempts >= MAX_ATTEMPTS {
                conn.execute("DELETE FROM telemetry_outbox WHERE event_id = ?1", [id])
            } else {
                conn.execute(
                    "UPDATE telemetry_outbox SET attempts = ?1, next_attempt_at = ?2, last_error = ?3 WHERE event_id = ?4",
                    params![attempts, next, category, id],
                )
            }
        })
        .await;
    }
}

fn insert_outbox(conn: &Connection, event: &QueuedEvent) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT OR IGNORE INTO telemetry_outbox
         (event_id, event_name, payload_json, created_at, attempts, next_attempt_at, last_error)
         VALUES (?1, ?2, ?3, ?4, 0, 0, '')",
        params![
            event.event_id,
            event.event_name,
            event.properties_json,
            event.created_at
        ],
    )
    .map(|_| ())
}

/// Enforce the age and size bounds. Amortized onto the flush tick rather than
/// run per insert: both statements scan the table, and a bound that is a few
/// seconds late costs nothing.
fn prune_outbox(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute(
        "DELETE FROM telemetry_outbox WHERE created_at < ?1",
        [now_seconds() - OUTBOX_TTL_SECONDS],
    )?;
    conn.execute(
        "DELETE FROM telemetry_outbox WHERE event_id NOT IN
         (SELECT event_id FROM telemetry_outbox ORDER BY created_at DESC LIMIT ?1)",
        [MAX_OUTBOX_ROWS],
    )?;
    Ok(())
}

fn pending_outbox(
    conn: &Connection,
    limit: i64,
    now: f64,
) -> Result<Vec<OutboxRow>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT event_id, event_name, payload_json, attempts
         FROM telemetry_outbox
         WHERE next_attempt_at <= ?1
         ORDER BY created_at ASC LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![now, limit], |row| {
        Ok(OutboxRow {
            event_id: row.get(0)?,
            event_name: row.get(1)?,
            payload_json: row.get(2)?,
            attempts: row.get(3)?,
        })
    })?;
    rows.collect()
}

fn retry_delay_seconds(attempts: i64, event_id: &str) -> f64 {
    let exponent = attempts.clamp(0, 10) as u32;
    let base = (5.0 * 2_f64.powi(exponent as i32)).min(3_600.0);
    // Small deterministic jitter avoids synchronized retries without adding
    // another random source or making tests flaky.
    let jitter = event_id.as_bytes().first().copied().unwrap_or(0) as f64 / 255.0 * 0.2;
    (base * (0.9 + jitter)).min(3_600.0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Delivery {
    Success,
    Retry(&'static str),
    Drop(&'static str),
}

#[derive(Serialize)]
struct CaptureRequest<'a> {
    api_key: &'a str,
    event: &'a str,
    properties: Map<String, Value>,
}

async fn send_capture(
    row: &OutboxRow,
    installation_id: &str,
    client: &reqwest::Client,
) -> Delivery {
    let Some(api_key) = build_api_key() else {
        return Delivery::Drop("missing_api_key");
    };
    let mut properties: Map<String, Value> = match serde_json::from_str(&row.payload_json) {
        Ok(value) => value,
        Err(_) => return Delivery::Drop("invalid_payload"),
    };
    properties.insert(
        "$insert_id".to_string(),
        Value::String(row.event_id.clone()),
    );
    properties.insert(
        "distinct_id".to_string(),
        Value::String(installation_id.to_string()),
    );
    properties.insert("$process_person_profile".to_string(), Value::Bool(false));
    properties.insert("$geoip_disable".to_string(), Value::Bool(true));
    let body = CaptureRequest {
        api_key,
        event: &row.event_name,
        properties,
    };
    let response = match client.post(POSTHOG_CAPTURE_URL).json(&body).send().await {
        Ok(response) => response,
        Err(_) => return Delivery::Retry("transport"),
    };
    let status = response.status().as_u16();
    if (200..300).contains(&status) {
        Delivery::Success
    } else if status == 429 || status >= 500 {
        Delivery::Retry(if status == 429 {
            "http_429"
        } else {
            "http_5xx"
        })
    } else if (400..500).contains(&status) {
        Delivery::Drop("http_4xx")
    } else {
        Delivery::Retry("http_other")
    }
}

pub const fn enabled_config_key() -> &'static str {
    TELEMETRY_ENABLED_KEY
}

pub const fn session_timeout_config_key() -> &'static str {
    SESSION_TIMEOUT_KEY
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A completed outcome with the fields a test cares about filled in.
    fn completed<'a>(
        mode: &'a str,
        stt_model: Option<&'a str>,
        ai_status: Option<&'a crate::ai::step::AiStatus>,
    ) -> Outcome<'a> {
        Outcome {
            recording_mode: RecordingMode::NotApplicable,
            stt_model,
            audio_seconds: 1.0,
            stt_millis: 100,
            chars: 10,
            ai_status,
            compute: Some(Compute::Cpu),
            paste_result: PasteResult::Success,
            ..Outcome::bare(Source::Microphone, mode)
        }
    }

    #[test]
    fn config_defaults_to_enabled_and_thirty_minutes() {
        let config = json!({});
        assert!(enabled_from_value(&config));
        assert_eq!(
            session_timeout_minutes_from_value(&config),
            DEFAULT_SESSION_TIMEOUT_MINUTES
        );
    }

    #[test]
    fn timeout_is_clamped_to_product_range() {
        let low = json!({ SESSION_TIMEOUT_KEY: 1 });
        let high = json!({ SESSION_TIMEOUT_KEY: 999 });
        assert_eq!(
            session_timeout_minutes_from_value(&low),
            *SESSION_TIMEOUT_MINUTES.start()
        );
        assert_eq!(
            session_timeout_minutes_from_value(&high),
            *SESSION_TIMEOUT_MINUTES.end()
        );
    }

    #[test]
    fn provider_and_engine_values_are_strictly_allowlisted() {
        assert_eq!(provider_wire("OpenAI"), Some("openai"));
        assert_eq!(provider_wire("https://example.test"), None);
        assert_eq!(
            cloud_stt_provider_wire(" OpenAI-Compatible "),
            Some("compatible")
        );
        assert_eq!(stt_engine_wire("local", Some("tiny")), "whisper");
        assert_eq!(stt_engine_wire("local", Some("custom-model")), "other");
    }

    #[test]
    fn stt_model_ids_are_known_or_coalesced_without_local_names() {
        assert_eq!(
            stt_model_wire("local", Some("turbo")),
            Some("turbo".to_string())
        );
        assert_eq!(
            stt_model_wire("local", Some("ggml-my-finetune-q5_0.bin")),
            Some("custom_local".to_string())
        );
        assert_eq!(
            stt_model_wire("cloud", Some(" Whisper-1 ")),
            Some("whisper-1".to_string())
        );
        assert_eq!(
            stt_model_wire("cloud", Some("org/Whisper-1")),
            Some("org_whisper-1".to_string())
        );
        assert_eq!(
            stt_model_wire("cloud", Some("https://provider.test/model")),
            Some("custom_cloud".to_string())
        );
        assert!(
            external_model_wire(Some(&"m".repeat(100)), "custom_cloud")
                .unwrap()
                .len()
                <= MAX_EXTERNAL_MODEL_CHARS
        );
    }

    #[test]
    fn llm_provider_and_model_are_normalized_only_after_a_response() {
        let mut status = crate::ai::step::AiStatus {
            provider: " OpenAI ".to_string(),
            model: "Org/GPT-4O-Mini".to_string(),
            ..Default::default()
        };
        let skipped = serde_json::to_value(outcome_payload(
            "hybrid",
            &completed("hybrid", Some("tiny"), Some(&status)),
            None,
            None,
        ))
        .unwrap();
        assert!(skipped.get("llm_provider").is_none());
        assert!(skipped.get("llm_model").is_none());

        status.attempted = true;
        status.used = true;
        let attempted = serde_json::to_value(outcome_payload(
            "hybrid",
            &completed("hybrid", Some("tiny"), Some(&status)),
            None,
            None,
        ))
        .unwrap();
        assert_eq!(attempted["llm_provider"], "openai");
        assert_eq!(attempted["llm_model"], "org_gpt-4o-mini");
        assert_eq!(attempted["stt_provider"], "local");
        assert_eq!(attempted["stt_model"], "tiny");
    }

    #[test]
    fn suspicious_external_models_never_reach_event_properties() {
        let status = crate::ai::step::AiStatus {
            attempted: true,
            used: true,
            provider: "compatible".to_string(),
            model: "https://example.test/sk-live-secret/path".to_string(),
            ..Default::default()
        };
        let outcome = Outcome {
            source: Source::File,
            compute: None,
            ..completed("cloud", Some("C:\\Users\\me\\secret-key"), Some(&status))
        };
        let event =
            TelemetryEvent::TranscriptionCompleted(outcome_payload("cloud", &outcome, None, None));
        let properties = event.properties().unwrap();
        assert_eq!(properties["source"], "file");
        assert_eq!(properties["stt_provider"], "compatible");
        assert_eq!(properties["stt_model"], "custom_cloud");
        assert_eq!(properties["llm_model"], "custom_llm");
        let serialized = serde_json::to_string(&properties).unwrap();
        assert!(!serialized.contains("example.test"));
        assert!(!serialized.contains("secret-key"));
        assert!(!serialized.contains("sk-live-secret"));
    }

    #[test]
    fn rejected_llm_request_does_not_publish_unvalidated_model_name() {
        let status = crate::ai::step::AiStatus {
            attempted: true,
            provider: "openai".to_string(),
            model: "possibly-not-a-real-model".to_string(),
            error_type: Some("bad_response".to_string()),
            ..Default::default()
        };
        let properties = serde_json::to_value(outcome_payload(
            "hybrid",
            &completed("hybrid", Some("tiny"), Some(&status)),
            None,
            None,
        ))
        .unwrap();
        assert_eq!(properties["llm_provider"], "openai");
        assert!(properties.get("llm_model").is_none());
    }

    #[test]
    fn typed_event_contains_only_non_sensitive_fields() {
        let outcome = Outcome::bare(Source::Microphone, "hybrid");
        let event = TelemetryEvent::TranscriptionFailed(outcome_payload(
            "hybrid",
            &outcome,
            Some(FailureStage::Stt),
            Some(FailureReason::EngineError),
        ));
        let properties = event.properties().unwrap();
        assert!(!properties.contains_key("text"));
        assert!(!properties.contains_key("prompt"));
        assert!(!properties.contains_key("path"));
        assert!(!properties.contains_key("api_key"));
        assert_eq!(properties["stt_provider"], "local");
        assert_eq!(event.name(), "transcription.failed");
    }

    /// A route that never reached the engine cannot claim a device. Both
    /// sources must say so the same way — they used to disagree.
    #[test]
    fn a_failure_reports_unknown_compute_for_both_sources() {
        for source in [Source::Microphone, Source::File] {
            let properties = serde_json::to_value(outcome_payload(
                "local",
                &Outcome::bare(source, "local"),
                Some(FailureStage::Start),
                Some(FailureReason::EngineBusy),
            ))
            .unwrap();
            assert_eq!(properties["compute"], "other");
        }
    }

    #[test]
    fn cloud_pipeline_overrides_the_local_device_reading() {
        let outcome = completed("cloud", Some("whisper-1"), None);
        let properties =
            serde_json::to_value(outcome_payload("cloud", &outcome, None, None)).unwrap();
        assert_eq!(properties["compute"], "cloud");
    }

    #[test]
    fn replacement_counts_collapse_into_stable_buckets() {
        assert_eq!(replacements_bucket(0), "0");
        assert_eq!(replacements_bucket(5), "1_5");
        assert_eq!(replacements_bucket(6), "6_20");
        assert_eq!(replacements_bucket(21), "gte_21");
    }

    /// The estimate must track the statistics writer's rate, not a literal
    /// copy of it that can drift.
    #[test]
    fn time_saved_uses_the_statistics_fallback_rate() {
        let chars = crate::stats::TIME_SAVED_CPM_FALLBACK as usize;
        assert_eq!(round_time_saved(chars), 60);
    }

    #[test]
    fn session_duration_excludes_idle_gap_and_is_rounded() {
        let now = Instant::now();
        let session = UsageAccumulator {
            started_at: now - Duration::from_secs(125),
            last_activity: now - Duration::from_secs(65),
            transcription_count: 1,
            success_count: 1,
            audio_seconds: 12.0,
            time_saved_seconds: 71.0,
            mode_counts: [1, 0, 0],
            ..UsageAccumulator::started(now)
        };
        let payload = usage_payload(&session, 30);
        assert_eq!(payload.duration_seconds, 60);
        assert_eq!(payload.audio_seconds, 10);
        assert_eq!(payload.dominant_pipeline_mode, "local");
        assert_eq!(payload.timeout_minutes, 30);
    }

    /// A file job can legitimately run longer than the whole inactivity
    /// timeout; splitting it in two would invent a session.
    #[test]
    fn an_in_flight_transcription_holds_the_session_open() {
        let now = Instant::now();
        let mut usage = UsageState {
            current: Some(UsageAccumulator {
                last_activity: now - Duration::from_secs(3_600),
                active_transcriptions: 1,
                ..UsageAccumulator::started(now)
            }),
        };
        let timeout = Duration::from_secs(60);
        assert!(take_expired(&mut usage, now, timeout).is_none());

        usage.current.as_mut().unwrap().active_transcriptions = 0;
        assert!(take_expired(&mut usage, now, timeout).is_some());
        assert!(usage.current.is_none());
    }

    #[test]
    fn outbox_insert_is_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(include_str!("migrations/v5.sql"))
            .unwrap();
        let event = QueuedEvent {
            event_id: "same".to_string(),
            event_name: "app.started",
            properties_json: "{}".to_string(),
            created_at: now_seconds(),
        };
        insert_outbox(&conn, &event).unwrap();
        insert_outbox(&conn, &event).unwrap();
        assert_eq!(outbox_len(&conn), 1);
    }

    /// The cap is enforced on the flush tick rather than per insert, so it
    /// has to survive a burst that overshoots it.
    #[test]
    fn pruning_keeps_the_newest_rows_within_the_cap() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(include_str!("migrations/v5.sql"))
            .unwrap();
        let now = now_seconds();
        for index in 0..(MAX_OUTBOX_ROWS + 25) {
            insert_outbox(
                &conn,
                &QueuedEvent {
                    event_id: format!("event-{index}"),
                    event_name: "app.started",
                    properties_json: "{}".to_string(),
                    created_at: now + index as f64,
                },
            )
            .unwrap();
        }
        // One row is older than the TTL and must go regardless of the cap.
        insert_outbox(
            &conn,
            &QueuedEvent {
                event_id: "ancient".to_string(),
                event_name: "app.started",
                properties_json: "{}".to_string(),
                created_at: now - OUTBOX_TTL_SECONDS - 1.0,
            },
        )
        .unwrap();

        prune_outbox(&conn).unwrap();

        assert_eq!(outbox_len(&conn), MAX_OUTBOX_ROWS);
        let survives: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM telemetry_outbox WHERE event_id IN ('ancient', 'event-0')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(survives, 0);
    }

    #[test]
    fn disabling_drops_active_session_but_preserves_outbox() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(include_str!("migrations/v5.sql"))
            .unwrap();
        let db = Arc::new(Mutex::new(conn));
        let telemetry = Telemetry::new(db.clone(), None);
        crate::mutex_recover::lock(&telemetry.usage).current =
            Some(UsageAccumulator::started(Instant::now()));
        insert_outbox(
            &crate::mutex_recover::lock(&db),
            &QueuedEvent {
                event_id: "pending".to_string(),
                event_name: "app.started",
                properties_json: "{}".to_string(),
                created_at: now_seconds(),
            },
        )
        .unwrap();

        telemetry.set_enabled(false);

        assert!(crate::mutex_recover::lock(&telemetry.usage)
            .current
            .is_none());
        assert_eq!(outbox_len(&crate::mutex_recover::lock(&db)), 1);
    }

    #[test]
    fn retry_delay_is_bounded() {
        assert!(retry_delay_seconds(0, "abc") >= 4.5);
        assert!(retry_delay_seconds(20, "abc") <= 3_600.0);
    }

    fn outbox_len(conn: &Connection) -> i64 {
        conn.query_row("SELECT COUNT(*) FROM telemetry_outbox", [], |row| {
            row.get(0)
        })
        .unwrap()
    }
}
