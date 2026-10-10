//! Whisper engine — owning-thread pattern.
//!
//! The WhisperContext and WhisperState are NOT Send (they contain FFI raw
//! pointers). Therefore they live inside a dedicated `std::thread::spawn`
//! thread, NOT in `tauri::State`. Commands come in via `mpsc::Receiver`,
//! results go out via `mpsc::Sender`.

pub use crate::vad::SpeechTiming;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tauri::{AppHandle, Manager};
use tokio::sync::{mpsc as tmpsc, oneshot};

/// Who is asking for the model to load — and therefore who should know.
///
/// A load started by the user is shown: they are waiting for it. Bringing back
/// a model unloaded on idle happens in parallel with recording, and the same
/// events would overwrite "Идёт запись" with "Загружаю модель" — about a
/// dictation that is being recorded perfectly well at that very moment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelLoadReason {
    /// The user picked a model, changed the device, or started the app.
    Requested,
    /// The model was brought back into memory after an idle unload.
    Restore,
}

#[derive(Debug)]
pub enum EngineCommand {
    Transcribe {
        session_id: u64,
        audio: Arc<Vec<f32>>,
        /// Where `audio` starts in the recording, past the silence trimmed off
        /// its front. Lines it up with the live preview's chunks.
        audio_offset: usize,
        speech_timing: SpeechTiming,
        cancel_flag: Arc<AtomicBool>,
        /// Target language (e.g. `"ru"`). `None` or `"auto"` auto-detects.
        /// whisper.cpp defaults to `"en"` when unset, which mis-decodes
        /// non-English speech, so the caller passes the configured language.
        language: Option<String>,
        /// The user's custom vocabulary, as a prompt for the decoder.
        ///
        /// Whisper conditions on this before it starts decoding, so names,
        /// brands and jargon come out right instead of being repaired
        /// afterwards by a fuzzy match that can only guess. Ignored by the
        /// sherpa engine: an offline NemoCtc recognizer has no equivalent
        /// input (hotwords in sherpa-onnx exist for transducer models only).
        initial_prompt: Option<String>,
        reply: oneshot::Sender<Result<InferenceResult, String>>,
    },
    /// Upload to an OpenAI-compatible provider, with the same result/error
    /// contract as local inference for both dictation and file callers.
    TranscribeCloud {
        session_id: u64,
        audio: Arc<Vec<f32>>,
        speech_timing: SpeechTiming,
        cancel_flag: Arc<AtomicBool>,
        request: crate::cloud_stt::CloudSttRequest,
        reply: oneshot::Sender<Result<InferenceResult, String>>,
    },
    SetModel {
        name: String,
        spec: crate::model::ModelLoadSpec,
        /// Whether to surface the load. See [`ModelLoadReason`].
        reason: ModelLoadReason,
        reply: oneshot::Sender<Result<(), String>>,
    },
    /// A chunk of audio for the live preview.
    ///
    /// A separate command rather than a side effect of `Transcribe`: the
    /// preview runs while recording, when there is nothing to transcribe yet. A
    /// non-streaming model swallows this command silently.
    PreviewChunk {
        session_id: u64,
        /// Where the chunk starts in the recording.
        offset: usize,
        samples: Vec<f32>,
    },
    /// Forget the accumulated hypothesis before a new dictation.
    PreviewReset {
        session_id: u64,
    },
    /// Drop the loaded model and free its memory. The reply fires only after
    /// the contexts are gone, so a caller about to delete the model files
    /// knows nothing is holding them open any more.
    UnloadModel {
        reply: oneshot::Sender<()>,
    },
    /// Unload the model if the engine has been idle for longer than `after`.
    ///
    /// The engine decides for itself rather than whoever sent the command:
    /// between the decision and its execution the queue has time to accept a
    /// dictation, and an unchecked "unload" would drop the model right before it
    /// is needed. Idleness is measured here too — by the last command, not by
    /// external signs of being busy.
    UnloadIdle {
        after: std::time::Duration,
    },
    Shutdown,
}

/// `EngineEvent` channel MUST be `tokio::sync::mpsc`, NOT `std::sync::mpsc`.
/// Reason: dispatcher in `lib.rs::setup()` is an async tokio task that
/// uses `.recv().await`. `std::sync::mpsc::Receiver::recv()` is sync (no
/// `.await`), and holding `std::sync::Mutex` across `.await` is a deadlock
/// risk + clippy warning. Engine thread (in `std::thread::spawn`) uses
/// `event_tx.blocking_send()`, which is the documented interop API for
/// non-tokio producers.
pub type EngineEventTx = tmpsc::Sender<EngineEvent>;
pub type EngineEventRx = tmpsc::Receiver<EngineEvent>;

#[derive(Debug, Clone)]
pub enum EngineEvent {
    ModelLoading {
        name: String,
    },
    ModelReady {
        name: String,
    },
    /// The model was unloaded from memory on idle.
    ModelUnloaded {
        name: String,
    },
    /// The model came back into memory after an idle unload.
    ///
    /// Separate from `ModelReady`: that one drives the UI state machine
    /// ("loading" → "ready"), while here only the lists need refreshing — at
    /// that moment the dictation state belongs to the dictation itself.
    ModelRestored {
        name: String,
    },
    ModelLoadFailed {
        name: String,
        error: String,
    },
    InferenceStarted {
        session_id: u64,
    },
    InferenceCompleted {
        session_id: u64,
        result: Result<InferenceResult, String>,
    },
    /// The growing hypothesis during a dictation. It may be displayed but not
    /// inserted: the next chunk of audio is free to rewrite what was shown.
    PreviewText {
        session_id: u64,
        text: String,
    },
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct InferenceResult {
    #[serde(skip)]
    pub stt_service: Option<crate::telemetry::ProviderService>,
    pub session_id: u64,
    pub text: String,
    pub language: Option<String>,
    /// Model captured by the engine thread at the start of this inference.
    /// Do not derive this later from config or the shared runtime slot: a
    /// queued model switch may complete before the dispatcher writes history.
    #[serde(default)]
    pub model_id: Option<String>,
    pub inference_time_ms: u64,
    /// Duration of the captured audio in seconds (samples / 16 kHz). Carried
    /// back so the dispatcher can record it in per-entry history + daily
    /// stats. Failed inference travels as `Err` instead of an empty result.
    #[serde(default)]
    pub audio_seconds: f64,
    /// Estimated speech time with short pauses; internal statistics only.
    #[serde(skip)]
    pub speech_seconds: Option<f64>,
}

/// What to do with a live-preview chunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewAction {
    /// A chunk from a dictation that arrived too late — drop it.
    Skip,
    /// The first chunk of a new dictation — forget the previous hypothesis and
    /// start from scratch.
    Restart,
    /// A continuation of the current one.
    Continue,
}

/// Which dictation a preview chunk belongs to.
///
/// The audio tap is detached on stop, but up to a second of already-recorded
/// audio remains in its queue, and the forwarding thread honestly drains it —
/// sometimes after the next dictation has already begun. Without this check such
/// a chunk feeds the recognizer the tail of the previous phrase, and the first
/// hypothesis of the new one starts with someone else's words.
///
/// Session numbers only grow, so a smaller number is always the past.
pub fn preview_action(current: Option<u64>, incoming: u64) -> PreviewAction {
    match current {
        Some(active) if active == incoming => PreviewAction::Continue,
        Some(active) if active > incoming => PreviewAction::Skip,
        _ => PreviewAction::Restart,
    }
}

/// ONNX Runtime scales poorly past a handful of threads on desktop CPUs and
/// competes with the rest of the app, so the pool is capped rather than
/// matched to the machine.
fn sherpa_threads() -> i32 {
    std::thread::available_parallelism()
        .map(|n| n.get().min(8) as i32)
        .unwrap_or(4)
}

/// Engine thread entry point. Owns the WhisperContext lifecycle.
/// All channels use `tokio::sync::mpsc` so engine thread (std::thread)
/// uses `blocking_recv()`/`blocking_send()` and dispatcher (async task)
/// uses `.recv().await`.
pub fn engine_thread_main(
    mut cmd_rx: tokio::sync::mpsc::Receiver<EngineCommand>,
    event_tx: tokio::sync::mpsc::Sender<EngineEvent>,
    app_handle: AppHandle,
    engine_current_model: std::sync::Arc<std::sync::Mutex<Option<String>>>,
) {
    let mut engine = Engine {
        whisper_state: None,
        whisper_ctx: None,
        sherpa: None,
        last_preview: String::new(),
        preview_session: None,
        preview_fed: None,
        loaded_spec: None,
        last_activity: std::time::Instant::now(),
        events: event_tx,
        app: app_handle,
        current_model: engine_current_model,
    };
    while let Some(cmd) = cmd_rx.blocking_recv() {
        // The mark is set when a command arrives; transcriptions that run
        // longer than the idle timeout set it again when they finish — by the
        // next check their start is already too old.
        //
        // The idle check itself does not count as work: otherwise the engine
        // would push its own timer forward on every tick and never reach it.
        if !matches!(cmd, EngineCommand::UnloadIdle { .. }) {
            engine.last_activity = std::time::Instant::now();
        }
        match cmd {
            EngineCommand::Transcribe {
                session_id,
                audio,
                audio_offset,
                speech_timing,
                cancel_flag,
                language,
                initial_prompt,
                reply,
            } => engine.transcribe(
                Job {
                    session_id,
                    speech_timing,
                    cancel_flag,
                    reply,
                },
                &audio,
                audio_offset,
                language.as_deref(),
                initial_prompt.as_deref(),
            ),
            EngineCommand::TranscribeCloud {
                session_id,
                audio,
                speech_timing,
                cancel_flag,
                request,
                reply,
            } => engine.transcribe_cloud(
                Job {
                    session_id,
                    speech_timing,
                    cancel_flag,
                    reply,
                },
                audio.len(),
                request,
            ),
            EngineCommand::SetModel {
                name,
                spec,
                reason,
                reply,
            } => engine.set_model(name, spec, reason, reply),
            EngineCommand::PreviewChunk {
                session_id,
                offset,
                samples,
            } => engine.preview_chunk(session_id, offset, &samples),
            EngineCommand::PreviewReset { session_id } => engine.preview_reset(session_id),
            EngineCommand::UnloadModel { reply } => {
                engine.unload();
                let _ = reply.send(());
            }
            EngineCommand::UnloadIdle { after } => engine.unload_idle(after),
            EngineCommand::Shutdown => break,
        }
    }
    log::info!("whisper engine thread exiting");
}

/// What both transcription commands carry besides their input.
struct Job {
    session_id: u64,
    speech_timing: SpeechTiming,
    cancel_flag: Arc<AtomicBool>,
    reply: oneshot::Sender<Result<InferenceResult, String>>,
}

impl Job {
    fn cancelled(&self) -> bool {
        self.cancel_flag.load(std::sync::atomic::Ordering::Relaxed)
    }
}

/// Text and reported language of a finished local transcription.
type Decoded = Result<(String, Option<String>), String>;

/// Everything the engine thread owns.
///
/// Field order is drop order, and it matters: the Whisper state holds raw
/// pointers into the context, so dropping the context first would leave them
/// dangling. Every explicit unload drops them in the same order.
struct Engine {
    whisper_state: Option<whisper_rs::WhisperState>,
    whisper_ctx: Option<whisper_rs::WhisperContext>,
    /// Sherpa's C recognizer is !Send/!Sync and stays on this engine thread.
    sherpa: Option<crate::sherpa::SherpaRecognizer>,
    /// The last hypothesis sent. Chunks arrive several dozen times a second
    /// while the text changes far more rarely: without this memory the overlay
    /// would receive fifty identical events per second.
    last_preview: String,
    preview_session: Option<u64>,
    /// How much of `preview_session`'s recording the streaming preview has
    /// fed, while it holds exactly that prefix. A dropped chunk, a failed feed
    /// or a model change clears it, and the final pass then decodes the whole
    /// recording again.
    preview_fed: Option<usize>,
    /// How the model in memory was loaded, to recognise a request for the same.
    loaded_spec: Option<crate::model::ModelLoadSpec>,
    /// When the engine last did work. Idleness is measured from here, and it
    /// is what takes the model out of memory (`UnloadIdle`).
    last_activity: std::time::Instant,
    events: EngineEventTx,
    app: AppHandle,
    current_model: Arc<std::sync::Mutex<Option<String>>>,
}

impl Engine {
    fn transcribe(
        &mut self,
        job: Job,
        audio: &[f32],
        audio_offset: usize,
        language: Option<&str>,
        initial_prompt: Option<&str>,
    ) {
        let session_id = job.session_id;
        // Taken in any case: whichever way the final pass goes, it ends the
        // preview's stream.
        let preview_fed = self
            .preview_fed
            .take()
            .filter(|_| self.preview_session == Some(session_id));
        let rest = preview_fed.and_then(|fed| preview_rest(fed, audio, audio_offset));
        let _ = self
            .events
            .blocking_send(EngineEvent::InferenceStarted { session_id });
        let started = std::time::Instant::now();
        let audio_seconds = audio.len() as f64 / 16000.0;
        let model_id = crate::mutex_recover::lock(&self.current_model).clone();
        log::info!(
            "session {session_id}: transcribe start — {} samples ({:.2}s @16k), lang={:?}",
            audio.len(),
            audio_seconds,
            language
        );

        // A `cancel_recording` that arrived after this command was queued has
        // already flipped the flag. Bail before the expensive decode; the
        // dispatcher still gets `InferenceCompleted` and clears the registry.
        let decoded = if job.cancelled() {
            Err("transcribe cancelled before .full()".to_string())
        } else if let Some(recognizer) = self.sherpa.as_mut() {
            decode_sherpa(
                recognizer,
                &job,
                audio,
                rest,
                language,
                model_id.as_deref(),
                initial_prompt,
            )
        } else {
            self.decode_whisper(&job, audio, language, initial_prompt, started)
        };
        let Job {
            speech_timing,
            reply,
            ..
        } = job;
        let result = decoded.map(|(text, language)| InferenceResult {
            session_id,
            text,
            language,
            model_id,
            stt_service: None,
            inference_time_ms: started.elapsed().as_millis() as u64,
            audio_seconds,
            speech_seconds: speech_timing.resolve(),
        });
        // Both completion channels retain errors: file callers consume the
        // reply while dictation uses the event dispatcher.
        complete_inference(session_id, result, &self.events, reply);
        self.last_activity = std::time::Instant::now();
    }

    fn decode_whisper(
        &mut self,
        job: &Job,
        audio: &[f32],
        language: Option<&str>,
        initial_prompt: Option<&str>,
        started: std::time::Instant,
    ) -> Decoded {
        use std::panic::AssertUnwindSafe;
        let session_id = job.session_id;
        // Lazy create_state on first Transcribe after SetModel. WhisperState
        // is a thin handle into WhisperContext; creating it on demand avoids
        // paying the cost upfront in SetModel.
        if self.whisper_state.is_none() {
            let Some(ctx) = self.whisper_ctx.as_ref() else {
                // Translators: keep this message self-contained — it surfaces
                // in the recording overlay verbatim when the user presses the
                // hotkey before downloading a model. Tell them WHAT is
                // missing, WHERE to get it, and HOW.
                return Err(crate::ui_text::t(
                    "Модель не загружена. Откройте раздел «Модели» и выберите модель.",
                ));
            };
            self.whisper_state = Some(
                ctx.create_state()
                    .map_err(|e| format!("create_state: {e}"))?,
            );
        }

        let mut params =
            whisper_rs::FullParams::new(whisper_rs::SamplingStrategy::Greedy { best_of: 1 });
        // Set the decode language explicitly. whisper.cpp's default is "en",
        // which produces garbage (or empty) output for other languages; an
        // empty/absent/"auto" value means auto-detect.
        // English-only (`.en`) models would still run the detection pass for
        // "auto", roughly doubling their time for a fixed answer.
        let multilingual = self
            .whisper_ctx
            .as_ref()
            .is_none_or(|ctx| ctx.is_multilingual());
        match language {
            _ if !multilingual => params.set_language(Some("en")),
            Some(lang) if !lang.is_empty() && lang != "auto" => params.set_language(Some(lang)),
            _ => params.set_language(Some("auto")),
        }
        // Thread count for the CPU parts of the graph — which is all of it
        // when the context was built with `use_gpu = false`. whisper.cpp's
        // default is min(4, hw) — bump to available parallelism so turbo
        // isn't needlessly slow on many-core CPUs.
        let n_threads = std::thread::available_parallelism()
            .map(|n| n.get().min(8) as i32)
            .unwrap_or(4);
        params.set_n_threads(n_threads);
        // Custom vocabulary, if any. `set_initial_prompt` panics on an
        // interior null byte (it builds a CString), and config JSON can carry
        // one, so the string is sanitised first.
        if let Some(prompt) = initial_prompt {
            let sanitized: String = prompt.chars().filter(|c| *c != '\0').collect();
            if !sanitized.trim().is_empty() {
                params.set_initial_prompt(&sanitized);
            }
        }
        // SAFETY: `job` holds the flag's Arc for the whole `full()` call below.
        unsafe { abort_when(&mut params, &job.cancel_flag) };
        // Silence whisper.cpp's own stdout/stderr chatter — in a windowed app
        // there is no console and it only adds noise.
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);

        // catch_unwind protects against Rust panics. C++ ggml SIGSEGV from
        // malformed input would still abort the process — that is mitigated
        // by the SHA-256 model check + PCM finite-32 guard elsewhere.
        log::info!("session {session_id}: calling whisper .full() on {n_threads} threads");
        let state = self.whisper_state.as_mut().expect("state created above");
        let panic_result = std::panic::catch_unwind(AssertUnwindSafe(|| state.full(params, audio)));
        log::info!(
            "session {session_id}: whisper .full() returned in {}ms (ok={})",
            started.elapsed().as_millis(),
            panic_result.is_ok()
        );
        match panic_result {
            Ok(Ok(_rc)) => {
                // NOTE: whisper-rs 0.14 returns `Result<c_int, WhisperError>`
                // from full_n_segments (not a bare usize).
                let n_segments = state
                    .full_n_segments()
                    .map_err(|e| format!("n_segments: {e}"))?;
                let mut text = String::new();
                for i in 0..n_segments {
                    // Per-interval cancellation, best-effort: a long .full()
                    // call cannot be interrupted mid-C++ execution without
                    // engine redesign, but the tail can be skipped.
                    if job.cancelled() {
                        break;
                    }
                    match state.full_get_segment_text(i) {
                        Ok(seg) => push_segment(&mut text, &seg),
                        Err(e) => log::warn!("segment {i} read failed: {e}"),
                    }
                }
                Ok((text, None))
            }
            Ok(Err(_)) if job.cancelled() => Err("whisper transcribe cancelled".to_string()),
            Ok(Err(e)) => Err(format!("whisper error: {e}")),
            Err(_) => {
                // After a panic the FFI state is half-broken — drop it so the
                // next Transcribe recreates it via ctx.create_state().
                self.whisper_state = None;
                Err("whisper panicked".to_string())
            }
        }
    }

    fn transcribe_cloud(
        &mut self,
        job: Job,
        samples: usize,
        request: crate::cloud_stt::CloudSttRequest,
    ) {
        // Bypasses the local engine entirely, but the engine thread still owns
        // the lifecycle (InferenceStarted / InferenceCompleted), so the
        // dispatcher needs no separate branch.
        let session_id = job.session_id;
        let _ = self
            .events
            .blocking_send(EngineEvent::InferenceStarted { session_id });
        let started = std::time::Instant::now();
        let audio_seconds = samples as f64 / 16000.0;
        let model_id = Some(request.model.clone());
        let outcome = if job.cancelled() {
            Err("cloud transcribe cancelled before request".to_string())
        } else {
            match crate::cloud_stt::transcribe_blocking(request, Arc::clone(&job.cancel_flag)) {
                Ok(_) if job.cancelled() => {
                    Err("cloud transcribe cancelled after response".to_string())
                }
                outcome => outcome,
            }
        };
        let Job {
            speech_timing,
            reply,
            ..
        } = job;
        let result = outcome.map(|cloud| InferenceResult {
            session_id,
            text: cloud.text,
            language: None,
            model_id,
            stt_service: Some(cloud.service),
            inference_time_ms: started.elapsed().as_millis() as u64,
            audio_seconds,
            speech_seconds: speech_timing.resolve(),
        });
        complete_inference(session_id, result, &self.events, reply);
        self.last_activity = std::time::Instant::now();
    }

    fn set_model(
        &mut self,
        name: String,
        spec: crate::model::ModelLoadSpec,
        reason: ModelLoadReason,
        reply: oneshot::Sender<Result<(), String>>,
    ) {
        // A restore of what is already in memory is a restore that arrived
        // too late. Capture queues one whenever nothing is loaded, and
        // "loaded" only becomes true when the load finishes — so a second
        // dictation started while the first restore was still reading
        // gigabytes off disk queues its own. Honouring it would drop a
        // working model and rebuild it from scratch, re-hashing the bundle on
        // the way, while the transcription it was meant to serve waits behind
        // that in the queue. The queue is ordered, so by the time a duplicate
        // is read the original has either succeeded — and this is it — or
        // failed, leaving the slot empty for a genuine retry.
        //
        // A request for the model as it is already loaded is the same race the
        // other way round: startup's auto-load hashes the bundle before it
        // queues, and a first dictation in the meantime restores the model.
        let loaded =
            crate::mutex_recover::lock(&self.current_model).as_deref() == Some(name.as_str());
        if already_loaded(loaded, reason, self.loaded_spec.as_ref(), &spec) {
            log::debug!("model {name} is already in memory, skipping {reason:?} load");
            let _ = reply.send(Ok(()));
            if reason == ModelLoadReason::Requested {
                let _ = self.events.blocking_send(EngineEvent::ModelReady { name });
            }
            return;
        }
        if reason == ModelLoadReason::Requested {
            let _ = self
                .events
                .blocking_send(EngineEvent::ModelLoading { name: name.clone() });
        }
        log::info!("loading model {name} ({spec:?}, {reason:?})");
        let result = self.load(&name, spec, reason);
        *crate::mutex_recover::lock(&self.current_model) =
            result.as_ref().ok().map(|()| name.clone());
        match result {
            Ok(()) => {
                let _ = reply.send(Ok(()));
                let _ = self.events.blocking_send(match reason {
                    ModelLoadReason::Requested => EngineEvent::ModelReady { name },
                    ModelLoadReason::Restore => EngineEvent::ModelRestored { name },
                });
            }
            Err(err_msg) => {
                // A failed restore stays silent here not because it does not
                // matter, but because the transcription that follows will
                // report it: it will hit an empty engine and show exactly the
                // same trouble in words about the model. A "failed to load"
                // pill in the middle of a recording would explain it too early
                // and to the wrong person.
                if reason == ModelLoadReason::Requested {
                    let _ = self.events.blocking_send(EngineEvent::ModelLoadFailed {
                        name,
                        error: err_msg.clone(),
                    });
                } else {
                    log::warn!("restoring model {name} into memory failed: {err_msg}");
                }
                let _ = reply.send(Err(err_msg));
            }
        }
    }

    /// Replace whatever is loaded. A failed load leaves nothing loaded: the
    /// shared model slot is cleared on error, so keeping the previous Whisper
    /// context would transcribe with a model the UI no longer considers
    /// loaded.
    fn load(
        &mut self,
        name: &str,
        spec: crate::model::ModelLoadSpec,
        reason: ModelLoadReason,
    ) -> Result<(), String> {
        self.drop_models();
        let loaded_spec = spec.clone();
        match spec {
            crate::model::ModelLoadSpec::Whisper { path, use_gpu } => {
                let path_str = path
                    .to_str()
                    .ok_or_else(|| "invalid model path encoding".to_string())?;
                let ctx_params = whisper_rs::WhisperContextParameters {
                    use_gpu,
                    ..Default::default()
                };
                let ctx = whisper_rs::WhisperContext::new_with_params(path_str, ctx_params)
                    .map_err(|e| format!("model load: {e}"))?;
                self.whisper_ctx = Some(ctx);
            }
            crate::model::ModelLoadSpec::Sherpa { engine, files } => {
                // Restores are queued synchronously at capture start. Hashing
                // here keeps slow disk I/O ahead of all audio commands without
                // blocking capture or the UI.
                if reason == ModelLoadReason::Restore {
                    crate::model::verify_bundle_files(name)?;
                }
                let recognizer =
                    crate::sherpa::SherpaRecognizer::open(engine, &files, sherpa_threads())?;
                self.sherpa = Some(recognizer);
            }
        }
        self.loaded_spec = Some(loaded_spec);
        Ok(())
    }

    /// CRITICAL drop order: the Whisper state holds raw pointers into the
    /// context and must go first; dropping the context first leaves a
    /// use-after-free for the next `state.full()`.
    fn drop_models(&mut self) {
        self.preview_fed = None;
        self.loaded_spec = None;
        self.whisper_state = None;
        self.sherpa = None;
        self.whisper_ctx = None;
    }

    fn unload(&mut self) {
        *crate::mutex_recover::lock(&self.current_model) = None;
        self.drop_models();
    }

    fn unload_idle(&mut self, after: std::time::Duration) {
        let Some(name) = crate::mutex_recover::lock(&self.current_model).clone() else {
            return;
        };
        let idle = self.last_activity.elapsed();
        if idle < after || app_is_busy(&self.app) {
            return;
        }
        log::info!(
            "unloading model {name}: idle {} s, threshold {} s",
            idle.as_secs(),
            after.as_secs()
        );
        self.unload();
        let _ = self
            .events
            .blocking_send(EngineEvent::ModelUnloaded { name });
    }

    fn preview_chunk(&mut self, session_id: u64, offset: usize, samples: &[f32]) {
        let Some(recognizer) = self.sherpa.as_mut() else {
            self.preview_fed = None;
            return;
        };
        match preview_action(self.preview_session, session_id) {
            PreviewAction::Skip => return,
            PreviewAction::Restart => {
                self.preview_session = Some(session_id);
                self.last_preview.clear();
                self.preview_fed = Some(0);
                recognizer.reset_preview();
            }
            PreviewAction::Continue => {}
        }
        let fed = self.preview_fed.take();
        let preview = recognizer.feed_preview(16_000, samples);
        if fed == Some(offset) && preview.is_ok() {
            self.preview_fed = Some(offset + samples.len());
        }
        match preview {
            // A non-streaming model returns no hypothesis, and a streaming one
            // none while a chunk decodes nothing new — we stay silent rather
            // than send empty text: an empty string would wipe what the overlay
            // already shows.
            Ok(None) => {}
            Ok(Some(text)) => {
                if text != self.last_preview {
                    self.last_preview.clone_from(&text);
                    let _ = self
                        .events
                        .blocking_send(EngineEvent::PreviewText { session_id, text });
                }
            }
            Err(error) => log::warn!("session {session_id}: live preview failed: {error}"),
        }
    }

    fn preview_reset(&mut self, session_id: u64) {
        // The session is remembered even without a recognizer: it decides the
        // fate of late chunks, not just state cleanup.
        self.preview_session = Some(session_id);
        self.last_preview.clear();
        self.preview_fed = Some(0);
        if let Some(recognizer) = self.sherpa.as_mut() {
            recognizer.reset_preview();
        }
    }
}

/// Whether a `SetModel` asks for the model in memory as it already is. A
/// restore only needs the name to match; a request also the way it is
/// loaded, since a device change reloads the same model.
fn already_loaded(
    loaded: bool,
    reason: ModelLoadReason,
    loaded_spec: Option<&crate::model::ModelLoadSpec>,
    spec: &crate::model::ModelLoadSpec,
) -> bool {
    loaded && (reason == ModelLoadReason::Restore || loaded_spec == Some(spec))
}

/// The recording after the `fed` samples the live preview has decoded, as a
/// slice of `audio`, which starts `audio_offset` samples into the recording.
/// `None` while the preview is behind that start: what it misses was trimmed.
fn preview_rest(fed: usize, audio: &[f32], audio_offset: usize) -> Option<&[f32]> {
    let skip = fed.checked_sub(audio_offset)?;
    Some(&audio[skip.min(audio.len())..])
}

/// Make `full()` stop once `flag` turns true. whisper.cpp asks between encoder
/// windows and decoder steps, so a cancelled long file frees the engine within
/// one window instead of after the whole recording.
///
/// The raw callback rather than whisper-rs's `set_abort_callback_safe`: in
/// 0.14.4 that one registers a trampoline for the closure's own type but hands
/// it a boxed trait object, which is undefined behaviour.
///
/// # Safety
/// `flag` must outlive every `full()` call made with `params`.
pub unsafe fn abort_when(params: &mut whisper_rs::FullParams, flag: &AtomicBool) {
    unsafe extern "C" fn requested(flag: *mut std::ffi::c_void) -> bool {
        // SAFETY: the caller of `abort_when` keeps the flag alive through `full()`.
        unsafe { &*(flag as *const AtomicBool) }.load(std::sync::atomic::Ordering::Relaxed)
    }
    unsafe {
        params.set_abort_callback(Some(requested));
        params.set_abort_callback_user_data(flag as *const AtomicBool as *mut std::ffi::c_void);
    }
}

/// Segmented models check cancellation between fragments. An in-flight native
/// decode cannot be interrupted safely; its result is discarded if cancelled.
fn decode_sherpa(
    recognizer: &mut crate::sherpa::SherpaRecognizer,
    job: &Job,
    audio: &[f32],
    preview_rest: Option<&[f32]>,
    language: Option<&str>,
    model_id: Option<&str>,
    initial_prompt: Option<&str>,
) -> Decoded {
    // Reject languages outside the bundle's declared list before native decode.
    // Qwen can condition decoding on the chosen language; other multilingual
    // bundles still detect it themselves.
    let languages = model_id.and_then(crate::model::model_languages);
    let requested = language.filter(|value| !value.is_empty() && *value != "auto");
    if let (Some(id), Some(asked)) = (model_id, requested) {
        if !crate::model::model_supports_language(id, asked) {
            return Err(crate::model::language_unsupported_message(
                languages.unwrap_or_default(),
            ));
        }
    }
    // A monolingual model knows its language better than the request does;
    // for the rest we report what was asked for.
    let reported_language = match languages {
        Some([single]) => Some((*single).to_string()),
        _ => requested.map(str::to_string),
    };
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if model_id.and_then(|id| crate::model::model_engine(id).ok())
            == Some(crate::model::ModelEngine::SherpaNemoCtc)
        {
            recognizer.transcribe_gigaam(audio, || job.cancelled())
        } else if !recognizer.is_streaming() {
            let fragment_seconds =
                model_id.map_or(crate::vad::FRAGMENT_SECONDS, crate::model::fragment_seconds);
            recognizer.transcribe_segmented(
                audio,
                fragment_seconds,
                requested,
                initial_prompt,
                || job.cancelled(),
            )
        } else if let Some(rest) = preview_rest {
            log::info!(
                "session {}: final pass continues the live preview, {} of {} samples left",
                job.session_id,
                rest.len(),
                audio.len()
            );
            recognizer.finish_preview(16_000, rest, || job.cancelled())
        } else {
            // A streaming model carries context across any length itself.
            recognizer.transcribe_unless(16_000, audio, || job.cancelled())
        }
    })) {
        Ok(Ok(_)) if job.cancelled() => {
            Err("sherpa transcribe cancelled after inference".to_string())
        }
        Ok(Ok(text)) => Ok((text, reported_language)),
        Ok(Err(error)) => Err(error),
        Err(_) => Err("sherpa panicked".to_string()),
    }
}

/// Whether the app is busy right now — by human measure, not by its own.
///
/// While someone is dictating the command queue is empty: audio accumulates in
/// the recording and arrives as a single command at the very end. To the engine
/// that is indistinguishable from idling, and without this check the model would
/// leave memory right in the middle of a phrase. Transcribing a file holds the
/// engine the same way — long before `Transcribe`, already during decoding. Only
/// the application state knows about this, which is why this is the one place
/// where the engine thread looks outward.
fn app_is_busy(app: &AppHandle) -> bool {
    let Some(state) = app.try_state::<crate::state::AppState>() else {
        // The engine starts before the state is registered. So there is neither
        // a recording nor a file yet — and no busyness either.
        return false;
    };
    state.recorder.is_recording() || state.is_engine_busy()
}

fn complete_inference(
    session_id: u64,
    result: Result<InferenceResult, String>,
    event_tx: &EngineEventTx,
    reply: oneshot::Sender<Result<InferenceResult, String>>,
) {
    let _ = event_tx.blocking_send(EngineEvent::InferenceCompleted {
        session_id,
        result: result.clone(),
    });
    let _ = reply.send(result);
}

/// Append one whisper segment to the running transcript.
///
/// whisper.cpp prefixes most segments with a leading space (its tokenizer
/// emits `" word"` for word boundaries). The previous assembly pushed the
/// raw segment AND our own separator space, which left the final text with
/// a leading space on the first segment and a double space between
/// segments. We trim each segment and join with exactly one space so the
/// pasted text has no leading space and no internal double spaces.
fn push_segment(text: &mut String, segment: &str) {
    let seg = segment.trim();
    if seg.is_empty() {
        return;
    }
    if !text.is_empty() {
        text.push(' ');
    }
    text.push_str(seg);
}

use std::path::PathBuf;

/// Resolve the on-disk path for a model by name.
///
/// Convention: `<cache_dir>/sotto/models/ggml-<name>.bin`. The directory
/// itself is decided by [`crate::model::models_dir`], so the path is spelled
/// out in one place only.
pub fn resolve_model_path(model_name: &str) -> Result<PathBuf, String> {
    let models_dir = crate::model::models_dir()?;
    std::fs::create_dir_all(&models_dir).map_err(|e| format!("create models dir: {e}"))?;
    Ok(models_dir.join(format!("ggml-{model_name}.bin")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inference_failures_reach_both_dictation_and_file_callers() {
        for message in [
            "model not loaded",
            "http 401",
            "timeout after 1s",
            "cancelled",
        ] {
            let (events, mut received_events) = tmpsc::channel(1);
            let (reply, received_reply) = oneshot::channel();
            complete_inference(42, Err(message.into()), &events, reply);
            assert_eq!(
                received_reply.blocking_recv().unwrap().unwrap_err(),
                message
            );
            match received_events.blocking_recv().unwrap() {
                EngineEvent::InferenceCompleted { session_id, result } => {
                    assert_eq!(session_id, 42);
                    assert_eq!(result.unwrap_err(), message);
                }
                event => panic!("unexpected event: {event:?}"),
            }
        }
    }

    #[test]
    fn an_empty_success_remains_distinct_from_an_inference_failure() {
        let (events, mut received_events) = tmpsc::channel(1);
        let (reply, received_reply) = oneshot::channel();
        complete_inference(
            42,
            Ok(InferenceResult {
                session_id: 42,
                text: String::new(),
                language: None,
                model_id: Some("tiny".into()),
                stt_service: None,
                inference_time_ms: 7,
                audio_seconds: 0.1,
                speech_seconds: None,
            }),
            &events,
            reply,
        );
        let result = received_reply.blocking_recv().unwrap().unwrap();
        assert!(result.text.is_empty());
        assert_eq!(result.inference_time_ms, 7);
        assert!(matches!(
            received_events.blocking_recv(),
            Some(EngineEvent::InferenceCompleted { result: Ok(_), .. })
        ));
    }
    use std::thread;

    #[test]
    fn a_load_request_for_the_model_as_loaded_is_skipped() {
        use crate::model::ModelLoadSpec;
        let spec = |use_gpu| ModelLoadSpec::Whisper {
            path: "ggml-tiny.bin".into(),
            use_gpu,
        };
        let requested = ModelLoadReason::Requested;
        assert!(already_loaded(
            true,
            requested,
            Some(&spec(false)),
            &spec(false)
        ));
        // A device change must rebuild the context.
        assert!(!already_loaded(
            true,
            requested,
            Some(&spec(false)),
            &spec(true)
        ));
        assert!(already_loaded(
            true,
            ModelLoadReason::Restore,
            Some(&spec(false)),
            &spec(true)
        ));
        assert!(!already_loaded(false, requested, None, &spec(false)));
    }

    #[test]
    fn the_final_pass_takes_the_recording_after_the_preview() {
        let audio = [1.0, 2.0, 3.0, 4.0];
        // Trimmed audio starts 10 samples into the recording.
        assert_eq!(preview_rest(12, &audio, 10), Some(&audio[2..]));
        assert_eq!(preview_rest(10, &audio, 10), Some(&audio[..]));
        // The preview decoded past the trimmed end: nothing left to feed.
        assert_eq!(preview_rest(20, &audio, 10), Some(&audio[4..]));
        // It never reached the trimmed start: the gap is gone from `audio`.
        assert_eq!(preview_rest(5, &audio, 10), None);
    }

    #[test]
    fn a_late_chunk_of_the_previous_dictation_never_reaches_the_new_one() {
        // The audio tap is detached on stop, but up to a second of recorded
        // audio stays in its queue, and the forwarder drains it after the next
        // dictation has started. Without this rule the tail of the previous
        // phrase fed the recognizer, and the new one began with alien words.
        assert_eq!(preview_action(Some(7), 6), PreviewAction::Skip);

        // The first chunk of a new dictation signals to forget the previous
        // hypothesis.
        assert_eq!(preview_action(Some(6), 7), PreviewAction::Restart);
        assert_eq!(preview_action(None, 7), PreviewAction::Restart);

        // Our own we continue without resetting anything: a reset mid-phrase
        // would erase what has already been decoded.
        assert_eq!(preview_action(Some(7), 7), PreviewAction::Continue);
    }

    #[test]
    fn push_segment_trims_leading_and_double_spaces() {
        // whisper.cpp segments arrive with a leading space; the assembled
        // text must have no leading space and single spaces between words.
        let mut text = String::new();
        push_segment(&mut text, " Привет");
        push_segment(&mut text, " как дела");
        assert_eq!(text, "Привет как дела");
    }

    #[test]
    fn push_segment_skips_empty_and_whitespace_segments() {
        let mut text = String::new();
        push_segment(&mut text, "   ");
        assert_eq!(text, "");
        push_segment(&mut text, " Hello ");
        push_segment(&mut text, "");
        push_segment(&mut text, "  world");
        assert_eq!(text, "Hello world");
    }

    #[test]
    fn resolve_model_path_uses_the_models_directory_override() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = crate::test_support::EnvGuard::set("SOTTO_MODELS_DIR", dir.path());
        assert_eq!(
            resolve_model_path("large-v3-turbo").unwrap(),
            dir.path().join("ggml-large-v3-turbo.bin")
        );
    }

    #[test]
    fn resolve_model_path_creates_dir() {
        let dir = tempfile::tempdir().unwrap();
        let models = dir.path().join("models");
        let _guard = crate::test_support::EnvGuard::set("SOTTO_MODELS_DIR", &models);
        assert!(!models.exists());
        let path = resolve_model_path("test-model-temp").unwrap();
        assert_eq!(path.parent(), Some(models.as_path()));
        assert!(models.is_dir());
    }

    #[test]
    fn engine_thread_handles_shutdown() {
        // The full engine_thread_main requires a real `tauri::AppHandle`,
        // which is non-trivial to construct outside the `tauri::test`
        // harness. We instead exercise the same `cmd_rx.blocking_recv()`
        // loop with a Shutdown command and verify the thread terminates
        // cleanly — this catches regressions in the channel-wiring contract
        // (e.g. accidental blocking vs. recv switch).
        let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::channel::<EngineCommand>(1);
        let (event_tx, _event_rx) = tokio::sync::mpsc::channel::<EngineEvent>(1);

        let handle = thread::spawn(move || {
            // Mirror the engine_thread_main loop body (minus the AppHandle
            // usage). The point of this test is to verify Shutdown cleanly
            // exits the blocking_recv loop and the thread joins.
            while let Some(cmd) = cmd_rx.blocking_recv() {
                if matches!(cmd, EngineCommand::Shutdown) {
                    break;
                }
            }
        });

        cmd_tx.blocking_send(EngineCommand::Shutdown).unwrap();
        handle.join().expect("thread should exit cleanly");
        drop(event_tx);
    }

    #[test]
    fn inference_result_serializes_empty_text_for_ipc() {
        // A no-audio / silent recording or a session that produced no
        // segments must serialize cleanly with empty text. The frontend
        // uses this struct verbatim in `whisper-done` events.
        let r = InferenceResult {
            session_id: 1,
            text: String::new(),
            language: None,
            model_id: None,
            stt_service: None,
            inference_time_ms: 0,
            audio_seconds: 0.0,
            speech_seconds: None,
        };
        assert_eq!(
            serde_json::to_value(r).unwrap(),
            serde_json::json!({
                "session_id": 1,
                "text": "",
                "language": null,
                "model_id": null,
                "inference_time_ms": 0,
                "audio_seconds": 0.0,
            })
        );
    }
}
