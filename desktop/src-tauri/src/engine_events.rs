//! Engine event dispatcher: `EngineEvent` → Tauri events, and the delivery of
//! a finished dictation — post-processing, stats and history, paste.
//!
//! CRITICAL: cancellation is checked before every step with side effects. A
//! cancelled session's text must not be pasted or recorded, and a session
//! that has claimed final delivery can no longer turn into a cancel.

use std::sync::atomic::Ordering;

use tauri::{AppHandle, Emitter, Manager};

use crate::state::AppState;
use crate::telemetry::{self, Telemetry};
use crate::whisper::{EngineEvent, EngineEventRx, InferenceResult};
use crate::{
    classify_completion, dictation, is_deliverable, post_process_transcription, telemetry_compute,
    telemetry_formatting, telemetry_pipeline_mode, telemetry_pipeline_mode_of,
    telemetry_recording_mode, Completion, ProcessedTranscription,
};

/// Start the dispatcher on the async runtime. `events` is single-consumer,
/// so it is consumed here. Needs `AppState` and `Telemetry` already managed.
pub(crate) fn spawn(app: AppHandle, events: EngineEventRx) {
    let dispatcher = Dispatcher {
        state: app.state::<AppState>().inner().clone(),
        telemetry: app.state::<Telemetry>().inner().clone(),
        app,
    };
    tauri::async_runtime::spawn(dispatcher.run(events));
}

struct Dispatcher {
    app: AppHandle,
    state: AppState,
    telemetry: Telemetry,
}

impl Dispatcher {
    async fn run(self, mut events: EngineEventRx) {
        while let Some(event) = events.recv().await {
            self.handle(event).await;
        }
        log::info!("whisper event dispatcher exiting");
    }

    async fn handle(&self, event: EngineEvent) {
        match event {
            EngineEvent::ModelLoading { name } => {
                let _ = self.app.emit("whisper-loading", name);
            }
            EngineEvent::ModelReady { name } => {
                let _ = self.app.emit("whisper-ready", name);
            }
            EngineEvent::ModelUnloaded { name } => {
                // The same event as an unload before deleting a model: the
                // lists and the status refresh the same way, and they have no
                // need to know exactly how the memory was freed.
                let _ = self.app.emit("model-unloaded", name);
            }
            EngineEvent::ModelRestored { name } => {
                let _ = self.app.emit("model-restored", name);
            }
            EngineEvent::ModelLoadFailed { name, error } => {
                // Same contract as `whisper-failed`: the frontend reads
                // `message`, not `error`.
                let _ = self.app.emit(
                    "whisper-load-failed",
                    serde_json::json!({ "name": name, "message": error }),
                );
            }
            EngineEvent::PreviewText { session_id, text } => {
                // The previous dictation's hypothesis must not be appended to
                // the current one's overlay: while the event travelled the
                // channel, the recording could have changed.
                if self.state.current_session_id.load(Ordering::Acquire) != session_id {
                    return;
                }
                let _ = self.app.emit(
                    "transcription-delta",
                    serde_json::json!({ "session_id": session_id, "text": text }),
                );
            }
            EngineEvent::InferenceStarted { session_id } => {
                // File jobs and retired sessions must not raise the dictation
                // overlay, even if their events arrive late.
                if self.state.owns_dictation(session_id) {
                    let _ = self.app.emit("whisper-started", session_id);
                }
            }
            EngineEvent::InferenceCompleted { session_id, result } => {
                self.complete(session_id, result).await;
            }
        }
    }

    async fn complete(&self, session_id: u64, result: Result<InferenceResult, String>) {
        // The session belongs to a caller that awaits the engine's `oneshot`
        // reply itself (file transcription). None of the dictation delivery
        // applies to it: no paste, no history, no stats, no FSM transition.
        // The file guard owns this marker through post-processing, which
        // remains cancellable.
        if crate::mutex_recover::lock(&self.state.dispatch_skipped).contains(&session_id) {
            log::info!("session {session_id} dispatch skipped (file job)");
            return;
        }
        // A file reply can retire its guard before this dispatcher consumes
        // completion. Ownership, not that guard's lifetime, decides whether
        // this result may touch dictation UI.
        if !self.state.owns_dictation(session_id) {
            return;
        }
        let cancelled = self.state.is_cancelled(session_id);
        match classify_completion(cancelled, result) {
            Completion::Cancelled => {
                log::info!("session {session_id} cancelled, skipping paste");
                self.report_cancelled(session_id);
            }
            Completion::Empty => {
                log::info!("session {session_id} empty transcription");
                let _ = self.app.emit("whisper-empty", session_id);
                dictation::finish(&self.state, session_id);
                crate::sounds::play(&self.app, crate::sounds::Cue::Error);
                self.record_failed(
                    telemetry::FailureStage::Stt,
                    telemetry::FailureReason::EmptyTranscript,
                );
            }
            Completion::Failed(message) => {
                crate::sounds::play(&self.app, crate::sounds::Cue::Error);
                // The overlay's ErrorPayload is `{ message?: string }`; the
                // real cause goes under that key instead of its fallback.
                let _ = self.app.emit(
                    "whisper-failed",
                    serde_json::json!({ "session_id": session_id, "message": message }),
                );
                dictation::finish(&self.state, session_id);
                self.record_failed(
                    telemetry::FailureStage::Stt,
                    telemetry::FailureReason::EngineError,
                );
            }
            Completion::Transcribed(inference) => self.deliver(session_id, inference).await,
        }
    }

    /// Every branch that ends a dictation as cancelled does so in this order:
    /// tell the UI, release the session, then record it.
    fn report_cancelled(&self, session_id: u64) {
        let _ = self.app.emit("whisper-cancelled", session_id);
        dictation::finish(&self.state, session_id);
        self.telemetry.record_cancelled(
            telemetry::Source::Microphone,
            &telemetry_pipeline_mode_of(&self.app),
        );
    }

    fn record_failed(&self, stage: telemetry::FailureStage, reason: telemetry::FailureReason) {
        self.telemetry.record_failed(
            telemetry::Source::Microphone,
            &telemetry_pipeline_mode_of(&self.app),
            stage,
            reason,
        );
    }

    async fn deliver(&self, session_id: u64, inference: InferenceResult) {
        // The engine completion and the overlay cancel are independent async
        // events. Do not even enter formatting/LLM when the cancel won the race.
        if self.state.is_cancelled(session_id) {
            self.report_cancelled(session_id);
            return;
        }
        let _ = self.app.emit("whisper-done", &inference);

        // Local formatting + optional LLM cleanup: the final text to paste and
        // the raw/formatted stages + AI status the history diff needs. In
        // hybrid mode the paste is intentionally delayed by the LLM round-trip.
        let processed = tokio::select! {
            biased;
            _ = self.state.wait_cancelled(session_id) => None,
            result = post_process_transcription(&self.app, &inference) => Some(result),
        };
        if self.state.is_cancelled(session_id) {
            self.report_cancelled(session_id);
            return;
        }
        let Some(processed) = processed else {
            return;
        };

        // The post-processor emptied the text — the whole transcription was a
        // Whisper silence hallucination. Treat it exactly like an empty
        // transcription: no paste, no history entry, no stats. `whisper-done`
        // already fired above, so `whisper-empty` is what corrects the overlay
        // from "Текст готов" back to idle.
        if !is_deliverable(&processed.final_text) {
            log::info!("session {session_id} produced only hallucinations, skipping paste");
            let _ = self.app.emit("whisper-empty", session_id);
            dictation::finish(&self.state, session_id);
            crate::sounds::play(&self.app, crate::sounds::Cue::Error);
            self.record_failed(
                telemetry::FailureStage::PostProcess,
                telemetry::FailureReason::EmptyAfterProcessing,
            );
            return;
        }

        // Atomically claim final delivery before any stats/history write. A
        // cancel that arrives first keeps this session out of all successful
        // side effects; a cancel after this point is too late to turn a
        // committed session into a false success.
        if !self.state.begin_commit(session_id) {
            if self.state.is_cancelled(session_id) {
                let _ = self.app.emit("whisper-cancelled", session_id);
                self.telemetry.record_cancelled(
                    telemetry::Source::Microphone,
                    &telemetry_pipeline_mode_of(&self.app),
                );
            }
            dictation::finish(&self.state, session_id);
            return;
        }

        let config = crate::config::Config::load(&self.app).ok();
        let paste = PendingPaste {
            session_id,
            text: processed.final_text.clone(),
            // Whether the LLM fell back to the local text is only known now,
            // so the overlay's warning rides along with the paste rather than
            // with `whisper-done`.
            ai: processed.ai_status.as_ref().map(|status| {
                serde_json::json!({
                    "fallback": status.fallback,
                    "skipped_reason": status.skipped_reason,
                })
            }),
            telemetry: DictationTelemetry::capture(
                config.as_ref(),
                &telemetry_pipeline_mode(config.as_ref()),
                inference.model_id.clone(),
                inference.audio_seconds,
                inference.inference_time_ms,
                processed.ai_status.clone(),
                inference.stt_service,
            ),
        };
        self.record(session_id, &inference, processed, config.as_ref())
            .await;
        // HistoryPage re-fetches on this.
        let _ = self.app.emit(
            "history-updated",
            serde_json::json!({ "session_id": session_id }),
        );
        self.paste(paste, config.as_ref());
    }

    /// Stats and history on a blocking worker: the connection guard must not
    /// be held across an `.await`. Failures are logged, never fatal — a broken
    /// database must not prevent the paste.
    async fn record(
        &self,
        session_id: u64,
        inference: &InferenceResult,
        processed: ProcessedTranscription,
        config: Option<&crate::config::Config>,
    ) {
        let started = std::time::Instant::now();
        let db = self.state.db.clone();
        let retention = config
            .map(|cfg| crate::history::RetentionPolicy::from_config(cfg.as_value()))
            .unwrap_or_default();
        let language = inference.language.clone();
        let transcription_model = inference.model_id.clone();
        let inference_ms = inference.inference_time_ms;
        let audio_seconds = inference.audio_seconds;
        let speech_seconds = inference.speech_seconds;
        let inference_session = inference.session_id;
        let written = tokio::task::spawn_blocking(move || {
            let ProcessedTranscription {
                raw_text,
                formatted_text,
                final_text,
                ai_json,
                ai_status,
                stats_json,
                system_prompt,
            } = processed;
            // LLM outcome first: it is the one aggregate that survives history
            // pruning, so it must not be skipped when a later write fails.
            if let Some(status) = &ai_status {
                if let Err(e) = crate::stats::record_ai_outcome(&db, status) {
                    log::warn!("llm stats write failed (non-fatal): {e}");
                }
            }
            crate::stats::record_transcription(
                &db,
                &final_text,
                language.as_deref(),
                inference_ms,
                audio_seconds,
                crate::stats::TIME_SAVED_CPM_FALLBACK,
                speech_seconds,
            )
            .and_then(|_| {
                crate::history::append_entry(
                    &db,
                    &crate::history::NewEntry {
                        text: &final_text,
                        raw_text: &raw_text,
                        formatted_text: &formatted_text,
                        session_id: Some(inference_session),
                        language: language.as_deref(),
                        inference_time_ms: inference_ms,
                        ai_processing_json: ai_json.as_deref(),
                        processing_stats_json: Some(&stats_json),
                        system_prompt: system_prompt.as_deref(),
                        transcription_model: transcription_model.as_deref(),
                    },
                )
            })
            .and_then(|_| crate::history::prune(&crate::mutex_recover::lock(&db), retention))
            .map_err(|e| e.to_string())
        })
        .await;
        match written {
            Ok(Ok(())) => {}
            Ok(Err(e)) => log::warn!("stats/history write failed (non-fatal): {e}"),
            Err(_) => log::warn!("stats/history worker channel closed (non-fatal)"),
        }
        log::info!(
            "delivery timing: session={session_id} database_ms={}",
            started.elapsed().as_millis()
        );
    }

    /// Paste (or copy) the final text on the delivery thread. The cue and
    /// `paste-done` follow the paste result: the overlay waits for it before
    /// claiming anything was inserted, which keeps it quiet while a slow LLM
    /// is still working.
    fn paste(&self, paste: PendingPaste, config: Option<&crate::config::Config>) {
        // Read here rather than inside the delivery call, which on macOS runs
        // on the main thread.
        let delivery = config
            .map(|cfg| crate::clipboard::DeliveryOptions::from_config(cfg.as_value()))
            .unwrap_or_default();
        let success = if delivery.auto_paste {
            telemetry::PasteResult::Success
        } else {
            telemetry::PasteResult::ClipboardOnly
        };
        let session_id = paste.session_id;
        let (app, state, recorder) = (self.app.clone(), self.state.clone(), self.telemetry.clone());
        let queued = std::time::Instant::now();
        let scheduled = crate::clipboard::run_delivery(&self.app, move || {
            let started = std::time::Instant::now();
            let queue_ms = queued.elapsed().as_millis();
            // Measured on the text that is actually going in: `whisper-done`
            // counted the pre-cleanup draft.
            let length = paste.text.chars().count();
            match crate::clipboard::deliver(app.clone(), paste.text, delivery) {
                Ok(()) => {
                    crate::sounds::play(&app, crate::sounds::Cue::Done);
                    let _ = app.emit(
                        "paste-done",
                        serde_json::json!({
                            "session_id": session_id,
                            "length": length,
                            "ai_processing": paste.ai,
                        }),
                    );
                    paste.telemetry.record(&recorder, length, success);
                }
                Err(e) => {
                    log::error!("paste failed: {e}");
                    crate::sounds::play(&app, crate::sounds::Cue::Error);
                    let message = format!(
                        "{} {e}",
                        crate::ui_text::t("Не удалось вставить текст в активное окно.")
                    );
                    let _ = app.emit(
                        "app-error",
                        serde_json::json!({ "kind": "paste", "message": message }),
                    );
                    // The overlay is waiting in "распознано" for a paste that is
                    // never coming. Without this it sits there until the
                    // stuck-overlay timeout — three minutes of pretending to work.
                    let _ = app.emit(
                        "paste-failed",
                        serde_json::json!({ "session_id": session_id, "message": message }),
                    );
                    paste
                        .telemetry
                        .record(&recorder, length, telemetry::PasteResult::Failed);
                }
            }
            log::info!(
                "delivery timing: session={session_id} queue_ms={queue_ms} paste_ms={}",
                started.elapsed().as_millis()
            );
            dictation::finish(&state, session_id);
        });
        if scheduled.is_err() {
            let _ = self.app.emit(
                "paste-failed",
                serde_json::json!({
                    "session_id": session_id,
                    "message": crate::ui_text::t("Не удалось вставить текст в активное окно."),
                }),
            );
            dictation::finish(&self.state, session_id);
        }
    }
}

/// What the delivery thread needs once the text is final.
struct PendingPaste {
    session_id: u64,
    text: String,
    ai: Option<serde_json::Value>,
    telemetry: DictationTelemetry,
}

/// The completed-dictation telemetry, captured before the paste so the
/// delivery thread does not read the config.
struct DictationTelemetry {
    stt_service: Option<telemetry::ProviderService>,
    pipeline_mode: String,
    stt_model: Option<String>,
    audio_seconds: f64,
    stt_millis: u64,
    ai_status: Option<crate::ai::step::AiStatus>,
    recording_mode: telemetry::RecordingMode,
    compute: Option<telemetry::Compute>,
    formatting_enabled: bool,
    replacement_rules: usize,
}

impl DictationTelemetry {
    fn capture(
        config: Option<&crate::config::Config>,
        pipeline_mode: &str,
        stt_model: Option<String>,
        audio_seconds: f64,
        stt_millis: u64,
        ai_status: Option<crate::ai::step::AiStatus>,
        stt_service: Option<telemetry::ProviderService>,
    ) -> Self {
        let (formatting_enabled, replacement_rules) =
            config.map(telemetry_formatting).unwrap_or((false, 0));
        Self {
            stt_service,
            pipeline_mode: pipeline_mode.to_string(),
            recording_mode: config
                .map(telemetry_recording_mode)
                .unwrap_or(telemetry::RecordingMode::NotApplicable),
            compute: config.map(|config| telemetry_compute(config, stt_model.as_deref())),
            stt_model,
            audio_seconds,
            stt_millis,
            ai_status,
            formatting_enabled,
            replacement_rules,
        }
    }

    fn record(&self, telemetry: &Telemetry, chars: usize, paste_result: telemetry::PasteResult) {
        telemetry.record_completed(telemetry::Outcome {
            source: telemetry::Source::Microphone,
            pipeline_mode: &self.pipeline_mode,
            recording_mode: self.recording_mode,
            stt_model: self.stt_model.as_deref(),
            stt_service: self.stt_service,
            audio_seconds: self.audio_seconds,
            stt_millis: self.stt_millis,
            chars,
            ai_status: self.ai_status.as_ref(),
            compute: self.compute,
            formatting_enabled: self.formatting_enabled,
            replacement_rules: self.replacement_rules,
            paste_result,
        });
    }
}
