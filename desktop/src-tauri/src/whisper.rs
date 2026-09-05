//! Whisper engine — owning-thread pattern.
//!
//! The WhisperContext and WhisperState are NOT Send (they contain FFI raw
//! pointers). Therefore they live inside a dedicated `std::thread::spawn`
//! thread, NOT in `tauri::State`. Commands come in via `mpsc::Receiver`,
//! results go out via `mpsc::Sender`.

use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tauri::{AppHandle, Manager};
use tokio::sync::{mpsc as tmpsc, oneshot};

/// Кто просит загрузить модель — и, следовательно, кому об этом знать.
///
/// Загрузку, начатую пользователем, показывают: он её ждёт. Возврат модели,
/// выгруженной по простою, идёт параллельно записи, и те же события затёрли
/// бы «Идёт запись» надписью «Загружаю модель» — про диктовку, которая в
/// этот момент прекрасно пишется.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelLoadReason {
    /// Пользователь выбрал модель, сменил устройство, запустил приложение.
    Requested,
    /// Модель вернули в память после выгрузки по простою.
    Restore,
}

#[derive(Debug)]
pub enum EngineCommand {
    Transcribe {
        session_id: u64,
        audio: Arc<Vec<f32>>,
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
        reply: oneshot::Sender<InferenceResult>,
    },
    /// Phase 4 / Batch 4 / PR 4.5: cloud STT path. Bypasses
    /// whisper and uploads to an OpenAI-compatible provider.
    /// Reply uses the same `InferenceResult` shape so the
    /// dispatcher does not need a new branch.
    TranscribeCloud {
        session_id: u64,
        audio: Arc<Vec<f32>>,
        cancel_flag: Arc<AtomicBool>,
        request: crate::cloud_stt::CloudSttRequest,
        reply: oneshot::Sender<InferenceResult>,
    },
    SetModel {
        name: String,
        spec: crate::model::ModelLoadSpec,
        /// Показывать ли загрузку. См. [`ModelLoadReason`].
        reason: ModelLoadReason,
        reply: oneshot::Sender<Result<(), String>>,
    },
    /// Кусок звука для живого предпросмотра.
    ///
    /// Отдельная команда, а не побочный эффект `Transcribe`: предпросмотр
    /// идёт во время записи, когда расшифровывать ещё нечего. Непотоковая
    /// модель эту команду молча проглатывает.
    PreviewChunk {
        session_id: u64,
        samples: Vec<f32>,
    },
    /// Забыть накопленную гипотезу перед новой диктовкой.
    PreviewReset {
        session_id: u64,
    },
    /// Drop the loaded model and free its memory. The reply fires only after
    /// the contexts are gone, so a caller about to delete the model files
    /// knows nothing is holding them open any more.
    UnloadModel {
        reply: oneshot::Sender<()>,
    },
    /// Выгрузить модель, если движок простаивает дольше `after`.
    ///
    /// Решает сам движок, а не тот, кто прислал команду: между решением и
    /// его исполнением очередь успевает принять диктовку, и «выгрузи» без
    /// проверки выгрузило бы модель ровно перед тем, как она понадобится.
    /// Здесь же простой и измеряется — по последней команде, а не по
    /// внешним признакам занятости.
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
    /// Модель выгружена из памяти по простою.
    ModelUnloaded {
        name: String,
    },
    /// Модель вернулась в память после выгрузки по простою.
    ///
    /// Отдельно от `ModelReady`: тот ведёт конечный автомат интерфейса
    /// («загружаю» → «готово»), а здесь нужно только обновить списки —
    /// состояние диктовки в этот момент принадлежит самой диктовке.
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
    /// Растущая гипотеза во время диктовки. Показывать можно, вставлять
    /// нельзя: следующий кусок звука вправе переписать уже показанное.
    PreviewText {
        session_id: u64,
        text: String,
    },
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct InferenceResult {
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
    /// stats. 0.0 on error/short-circuit paths (no audio was transcribed).
    #[serde(default)]
    pub audio_seconds: f64,
}

/// Что делать с куском живого предпросмотра.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewAction {
    /// Кусок опоздавшей диктовки — выбросить.
    Skip,
    /// Первый кусок новой диктовки — забыть прошлую гипотезу и начать с нуля.
    Restart,
    /// Продолжение текущей.
    Continue,
}

/// Кому принадлежит кусок предпросмотра.
///
/// Ответвление звука отцепляют при остановке, но в его очереди остаётся до
/// секунды уже записанного, и поток-пересыльщик честно дочитывает её —
/// иногда уже после того, как началась следующая диктовка. Без этой
/// проверки такой кусок докармливает распознаватель хвостом прошлой фразы,
/// и первая гипотеза новой начинается с чужих слов.
///
/// Номера сессий растут, поэтому меньший номер — это всегда прошлое.
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
    let mut current_ctx: Option<whisper_rs::WhisperContext> = None;
    let mut current_state: Option<whisper_rs::WhisperState> = None;
    // Sherpa's C recognizer is !Send/!Sync and stays on this engine thread.
    let mut current_sherpa: Option<crate::sherpa::SherpaRecognizer> = None;
    // Последняя отправленная гипотеза. Куски приходят по несколько десятков
    // в секунду, а текст меняется куда реже: без этой памяти overlay получал
    // бы полсотни одинаковых событий в секунду.
    let mut last_preview = String::new();
    let mut preview_session: Option<u64> = None;
    // Когда движок в последний раз работал. Отсюда считается простой, по
    // которому модель уходит из памяти (`UnloadIdle`).
    let mut last_activity = std::time::Instant::now();

    while let Some(cmd) = cmd_rx.blocking_recv() {
        // Отметка ставится на приходе команды, а не на её завершении: у
        // длинных веток есть короткие выходы через `continue`, и хвост
        // цикла до них не доживает. Расшифровки, которые сами длятся
        // дольше простоя, доставляют отметку ещё раз в конце — их начало
        // к моменту следующей проверки уже слишком старое.
        //
        // Сама проверка простоя работой не считается: иначе движок
        // отодвигал бы свой таймер каждым тиком и не дожил бы до него.
        if !matches!(cmd, EngineCommand::UnloadIdle { .. }) {
            last_activity = std::time::Instant::now();
        }
        match cmd {
            EngineCommand::Transcribe {
                session_id,
                audio,
                cancel_flag,
                language,
                initial_prompt,
                reply,
            } => {
                use std::panic::AssertUnwindSafe;
                let _ = event_tx.blocking_send(EngineEvent::InferenceStarted { session_id });
                let started = std::time::Instant::now();
                let audio_seconds = audio.len() as f64 / 16000.0;
                let model_id = crate::mutex_recover::lock(&engine_current_model).clone();
                log::info!(
                    "session {session_id}: transcribe start — {} samples ({:.2}s @16k), lang={:?}",
                    audio.len(),
                    audio_seconds,
                    language
                );

                // Phase 4 / Batch 6 / P0: pre-`full()` cancel check.
                // If `cancel_recording(session_id)` was invoked AFTER
                // the Tauri command queued this `Transcribe`, the
                // flag has already been flipped. Bail BEFORE
                // consuming the expensive `.full()` C call — the
                // engine still emits `InferenceCompleted` so the
                // dispatcher unblocks and clears the registry.
                if cancel_flag.load(std::sync::atomic::Ordering::Relaxed) {
                    short_circuit_error(
                        session_id,
                        "transcribe cancelled before .full()".to_string(),
                        model_id.clone(),
                        &event_tx,
                        reply,
                    );
                    continue;
                }

                // Sherpa has no segment-level cancellation. Honour a flag
                // before and after the blocking call; an in-flight call is
                // intentionally best-effort and cannot be interrupted safely.
                if let Some(recognizer) = current_sherpa.as_mut() {
                    // A monolingual bundle asked for another language does
                    // not fail, it mis-decodes — so refuse the pair here.
                    // Multilingual bundles impose no rule and detect the
                    // language themselves.
                    let languages = model_id.as_deref().and_then(crate::model::model_languages);
                    let requested = language
                        .as_deref()
                        .filter(|value| !value.is_empty() && *value != "auto");
                    let supported = match (model_id.as_deref(), requested) {
                        (Some(id), Some(asked)) => crate::model::model_supports_language(id, asked),
                        _ => true,
                    };
                    if !supported {
                        short_circuit_error(
                            session_id,
                            crate::model::language_unsupported_message(
                                languages.unwrap_or_default(),
                            ),
                            model_id.clone(),
                            &event_tx,
                            reply,
                        );
                        continue;
                    }
                    // Одноязычная модель знает свой язык лучше запроса; у
                    // остальных отчитываемся тем, что просили.
                    let reported_language = match languages {
                        Some([single]) => Some((*single).to_string()),
                        _ => requested.map(str::to_string),
                    };
                    let panic_result =
                        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            recognizer.transcribe(16_000, &audio)
                        }));
                    let result: Result<InferenceResult, String> = match panic_result {
                        Ok(Ok(_text)) if cancel_flag.load(std::sync::atomic::Ordering::Relaxed) => {
                            Err("sherpa transcribe cancelled after inference".to_string())
                        }
                        Ok(Ok(text)) => Ok(InferenceResult {
                            session_id,
                            text,
                            language: reported_language.clone(),
                            model_id: model_id.clone(),
                            inference_time_ms: started.elapsed().as_millis() as u64,
                            audio_seconds,
                        }),
                        Ok(Err(error)) => Err(error),
                        Err(_) => Err("sherpa panicked".to_string()),
                    };
                    let _ = event_tx.blocking_send(EngineEvent::InferenceCompleted {
                        session_id,
                        result: result.clone(),
                    });
                    let _ = reply.send(result.unwrap_or(InferenceResult {
                        session_id,
                        text: String::new(),
                        language: None,
                        model_id: model_id.clone(),
                        inference_time_ms: 0,
                        audio_seconds: 0.0,
                    }));
                    continue;
                }

                // Lazy create_state on first Transcribe after SetModel.
                // WhisperState is a thin handle into WhisperContext; creating
                // it on demand avoids paying the cost upfront in SetModel.
                if current_state.is_none() {
                    if let Some(ctx) = current_ctx.as_ref() {
                        match ctx.create_state() {
                            Ok(s) => current_state = Some(s),
                            Err(e) => {
                                let err = format!("create_state: {e}");
                                let _ = event_tx.blocking_send(EngineEvent::InferenceCompleted {
                                    session_id,
                                    result: Err(err.clone()),
                                });
                                let _ = reply.send(InferenceResult {
                                    session_id,
                                    text: String::new(),
                                    language: None,
                                    model_id: model_id.clone(),
                                    inference_time_ms: 0,
                                    audio_seconds: 0.0,
                                });
                                continue;
                            }
                        }
                    } else {
                        // Translators: keep this message self-contained —
                        // it surfaces in the recording overlay verbatim
                        // when the user presses the hotkey before
                        // downloading a model. Tell them WHAT is missing,
                        // WHERE to get it, and HOW (click Загрузить).
                        let err = crate::ui_text::t(
                            "Модель не загружена. Откройте «Настройки → Модели» и выберите модель.",
                        );
                        let _ = event_tx.blocking_send(EngineEvent::InferenceCompleted {
                            session_id,
                            result: Err(err.clone()),
                        });
                        let _ = reply.send(InferenceResult {
                            session_id,
                            text: String::new(),
                            language: None,
                            model_id: model_id.clone(),
                            inference_time_ms: 0,
                            audio_seconds: 0.0,
                        });
                        continue;
                    }
                }

                let mut params =
                    whisper_rs::FullParams::new(whisper_rs::SamplingStrategy::Greedy {
                        best_of: 1,
                    });
                // Set the decode language explicitly. whisper.cpp's default is
                // "en", which produces garbage (or empty) output for other
                // languages; an empty/absent/"auto" value means auto-detect.
                match language.as_deref() {
                    Some(lang) if !lang.is_empty() && lang != "auto" => {
                        params.set_language(Some(lang));
                    }
                    _ => params.set_language(Some("auto")),
                }
                // Thread count for the CPU parts of the graph — which is all
                // of it when the context was built with `use_gpu = false`.
                // whisper.cpp's default is min(4, hw) — bump to available
                // parallelism so turbo isn't needlessly slow on many-core CPUs.
                let n_threads = std::thread::available_parallelism()
                    .map(|n| n.get().min(8) as i32)
                    .unwrap_or(4);
                params.set_n_threads(n_threads);
                // Custom vocabulary, if any. `set_initial_prompt` panics on
                // an interior null byte (it builds a CString), and config
                // JSON can carry one, so the string is sanitised first.
                if let Some(prompt) = initial_prompt.as_deref() {
                    let sanitized: String = prompt.chars().filter(|c| *c != '\0').collect();
                    if !sanitized.trim().is_empty() {
                        params.set_initial_prompt(&sanitized);
                    }
                }
                // Silence whisper.cpp's own stdout/stderr chatter — in a
                // windowed app there is no console and it only adds noise.
                params.set_print_special(false);
                params.set_print_progress(false);
                params.set_print_realtime(false);
                params.set_print_timestamps(false);

                // catch_unwind protects against Rust panics. C++ ggml
                // SIGSEGV from malformed input would still abort the
                // process — that is mitigated by the SHA-256 model
                // check + PCM finite-32 guard elsewhere (R3).
                // After a panic, the FFI state may be half-broken, so
                // we drop it; the next Transcribe recreates via
                // ctx.create_state() above.
                log::info!(
                    "session {session_id}: calling whisper .full() on {} threads",
                    n_threads
                );
                let panic_result = std::panic::catch_unwind(AssertUnwindSafe(|| {
                    // Re-borrow inside the closure so the borrow is released
                    // before we touch `current_state` directly below.
                    current_state
                        .as_mut()
                        .expect("state set above; lazy-create branch")
                        .full(params, &audio)
                }));
                log::info!(
                    "session {session_id}: whisper .full() returned in {}ms (ok={})",
                    started.elapsed().as_millis(),
                    panic_result.is_ok()
                );
                let result: Result<InferenceResult, String> = match panic_result {
                    Ok(Ok(_rc)) => {
                        let elapsed = started.elapsed().as_millis() as u64;
                        // Text extraction: iterate segments. After a
                        // successful .full(), current_state is intact.
                        // NOTE: whisper-rs 0.14 returns
                        // `Result<c_int, WhisperError>` from full_n_segments
                        // (not a bare usize), so we unwrap cautiously.
                        let state = current_state
                            .as_mut()
                            .expect("state preserved across successful .full()");
                        match state.full_n_segments() {
                            Ok(n_segments) => {
                                let mut text = String::new();
                                for i in 0..n_segments {
                                    // Per-interval cancellation: between
                                    // segments, bail out if cancel_flag has
                                    // been set. The caller sets this before
                                    // the next paste to skip noisy tails.
                                    // This is best-effort — a long .full()
                                    // call cannot be interrupted mid-C++
                                    // execution without engine redesign.
                                    if cancel_flag.load(std::sync::atomic::Ordering::Relaxed) {
                                        break;
                                    }
                                    match state.full_get_segment_text(i) {
                                        Ok(seg) => push_segment(&mut text, &seg),
                                        Err(e) => {
                                            log::warn!("segment {i} read failed: {e}");
                                        }
                                    }
                                }
                                Ok(InferenceResult {
                                    session_id,
                                    text,
                                    language: None,
                                    model_id: model_id.clone(),
                                    inference_time_ms: elapsed,
                                    audio_seconds,
                                })
                            }
                            Err(e) => Err(format!("n_segments: {e}")),
                        }
                    }
                    Ok(Err(e)) => Err(format!("whisper error: {e}")),
                    Err(_) => {
                        // After a panic the FFI state is half-broken — drop it
                        // so the next Transcribe recreates via
                        // ctx.create_state() above.
                        current_state = None;
                        Err("whisper panicked".to_string())
                    }
                };

                // Always emit InferenceCompleted (success or error) so the
                // dispatcher / UI can react. Always reply on the oneshot
                // (even on error with empty payload) so the Tauri command
                // never blocks forever.
                let completed_event = EngineEvent::InferenceCompleted {
                    session_id,
                    result: result.clone().map_err(|e| e.clone()),
                };
                let _ = event_tx.blocking_send(completed_event);
                let reply_payload = match &result {
                    Ok(r) => r.clone(),
                    Err(_) => InferenceResult {
                        session_id,
                        text: String::new(),
                        language: None,
                        model_id: model_id.clone(),
                        inference_time_ms: 0,
                        audio_seconds: 0.0,
                    },
                };
                let _ = reply.send(reply_payload);
                last_activity = std::time::Instant::now();
            }
            EngineCommand::SetModel {
                name,
                spec,
                reason,
                reply,
            } => {
                if reason == ModelLoadReason::Requested {
                    let _ =
                        event_tx.blocking_send(EngineEvent::ModelLoading { name: name.clone() });
                }
                log::info!("loading model {name} ({spec:?}, {reason:?})");
                let result: Result<(), String> = (|| {
                    // CRITICAL drop order: state FIRST (state holds raw
                    // pointers into ctx internals; dropping ctx first
                    // creates dangling pointers → use-after-free on next
                    // state.full()). Reverse order matters here.
                    current_state = None; // drop old state first
                    current_sherpa = None;
                    // Do not leave a failed load pointing at the previous
                    // Whisper context: the shared model slot is cleared on
                    // error below, so retaining it would transcribe with a
                    // model the UI no longer considers loaded.
                    current_ctx = None;
                    match spec {
                        crate::model::ModelLoadSpec::Whisper { path, use_gpu } => {
                            let path_str = path
                                .to_str()
                                .ok_or_else(|| "invalid model path encoding".to_string())?;
                            let ctx_params = whisper_rs::WhisperContextParameters {
                                use_gpu,
                                ..Default::default()
                            };
                            let new_ctx =
                                whisper_rs::WhisperContext::new_with_params(path_str, ctx_params)
                                    .map_err(|e| format!("model load: {e}"))?;
                            current_ctx = Some(new_ctx);
                        }
                        crate::model::ModelLoadSpec::Sherpa { engine, files } => {
                            let recognizer = crate::sherpa::SherpaRecognizer::open(
                                engine,
                                &files,
                                sherpa_threads(),
                            )?;
                            current_ctx = None;
                            current_sherpa = Some(recognizer);
                        }
                    }
                    Ok(())
                })();
                match &result {
                    Ok(()) => {
                        *crate::mutex_recover::lock(&engine_current_model) = Some(name.clone());
                    }
                    Err(_) => {
                        *crate::mutex_recover::lock(&engine_current_model) = None;
                    }
                }
                match result {
                    Ok(()) => {
                        let _ = reply.send(Ok(()));
                        let _ = event_tx.blocking_send(match reason {
                            ModelLoadReason::Requested => EngineEvent::ModelReady { name },
                            ModelLoadReason::Restore => EngineEvent::ModelRestored { name },
                        });
                    }
                    Err(err_msg) => {
                        // Провалившийся возврат молчит здесь не потому, что
                        // он не важен, а потому, что о нём скажет расшифровка,
                        // которая идёт следом: она упрётся в пустой движок и
                        // покажет ровно ту же беду словами про модель. Плашка
                        // «не удалось загрузить» посреди записи объяснила бы
                        // её раньше времени и не тому, кто её вызвал.
                        if reason == ModelLoadReason::Requested {
                            let _ = event_tx.blocking_send(EngineEvent::ModelLoadFailed {
                                name,
                                error: err_msg.clone(),
                            });
                        } else {
                            log::warn!("возврат модели {name} в память не удался: {err_msg}");
                        }
                        let _ = reply.send(Err(err_msg));
                    }
                }
            }
            EngineCommand::PreviewChunk {
                session_id,
                samples,
            } => {
                let Some(recognizer) = current_sherpa.as_mut() else {
                    continue;
                };
                match preview_action(preview_session, session_id) {
                    PreviewAction::Skip => continue,
                    PreviewAction::Restart => {
                        preview_session = Some(session_id);
                        last_preview.clear();
                        recognizer.reset_preview();
                    }
                    PreviewAction::Continue => {}
                }
                match recognizer.feed_preview(16_000, &samples) {
                    // Непотоковая модель гипотезы не отдаёт — молчим, а не
                    // шлём пустой текст: пустая строка стёрла бы уже
                    // показанное в overlay.
                    Ok(None) => {}
                    Ok(Some(text)) => {
                        if text != last_preview {
                            last_preview.clone_from(&text);
                            let _ = event_tx
                                .blocking_send(EngineEvent::PreviewText { session_id, text });
                        }
                    }
                    Err(error) => {
                        log::warn!("session {session_id}: live preview failed: {error}");
                    }
                }
            }
            EngineCommand::PreviewReset { session_id } => {
                // Сессию запоминаем даже без распознавателя: она решает
                // судьбу опоздавших кусков, а не только чистку состояния.
                preview_session = Some(session_id);
                last_preview.clear();
                if let Some(recognizer) = current_sherpa.as_mut() {
                    recognizer.reset_preview();
                }
            }
            EngineCommand::UnloadModel { reply } => {
                *crate::mutex_recover::lock(&engine_current_model) = None;
                current_state = None;
                current_sherpa = None;
                current_ctx = None;
                let _ = reply.send(());
            }
            EngineCommand::UnloadIdle { after } => {
                let loaded = crate::mutex_recover::lock(&engine_current_model).clone();
                let Some(name) = loaded else {
                    continue;
                };
                let idle = last_activity.elapsed();
                if idle < after {
                    continue;
                }
                if app_is_busy(&app_handle) {
                    continue;
                }
                log::info!(
                    "выгружаю модель {name}: простой {} c при пороге {} c",
                    idle.as_secs(),
                    after.as_secs()
                );
                // Порядок как в `UnloadModel`: состояние держит сырые
                // указатели внутрь контекста и умирает первым.
                *crate::mutex_recover::lock(&engine_current_model) = None;
                current_state = None;
                current_sherpa = None;
                current_ctx = None;
                let _ = event_tx.blocking_send(EngineEvent::ModelUnloaded { name });
            }
            EngineCommand::TranscribeCloud {
                session_id,
                audio,
                cancel_flag,
                request,
                reply,
            } => {
                let audio_seconds = audio.len() as f64 / 16000.0;
                // Phase 4 / Batch 4 / PR 4.5: cloud STT path.
                // Bypasses whisper entirely. The engine thread
                // still owns the lifecycle (InferenceStarted /
                // InferenceCompleted) so the dispatcher does not
                // need a new branch.
                let _ = event_tx.blocking_send(EngineEvent::InferenceStarted { session_id });
                let started = std::time::Instant::now();
                let cloud_model_id = Some(request.model.clone());

                if cancel_flag.load(std::sync::atomic::Ordering::Relaxed) {
                    short_circuit_error(
                        session_id,
                        "cloud transcribe cancelled before request".to_string(),
                        cloud_model_id.clone(),
                        &event_tx,
                        reply,
                    );
                    continue;
                }

                // The HTTP call is async (reqwest). The engine
                // thread is a `std::thread`, so we drive it via a
                // short-lived current-thread tokio runtime.
                let request_for_call = request.clone();
                let cancel_flag_for_call = Arc::clone(&cancel_flag);
                let join_outcome: Result<
                    std::thread::JoinHandle<Result<crate::cloud_stt::CloudSttResult, String>>,
                    String,
                > = std::thread::Builder::new()
                        .name("cloud-stt-call".to_string())
                        .spawn(move || {
                            let rt = tokio::runtime::Builder::new_current_thread()
                                .enable_all()
                                .build()
                                .map_err(|error| format!("runtime: {error}"))?;
                            rt.block_on(async move {
                                // Cancel poll: every 100 ms while
                                // the HTTP call is in flight, recheck
                                // the flag. We cannot interrupt
                                // reqwest directly, but once the
                                // timeout-driven request returns we
                                // drop the result if the flag was
                                // flipped during the call.
                                loop {
                                    if cancel_flag_for_call
                                        .load(std::sync::atomic::Ordering::Relaxed)
                                    {
                                        return Err(
                                            "cloud transcribe cancelled mid-request".to_string()
                                        );
                                    }
                                    tokio::select! {
                                        _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => continue,
                                        result = crate::cloud_stt::transcribe(request_for_call.clone()) => return result,
                                    }
                                }
                            })
                        })
                        .map_err(|error| format!("spawn cloud-stt thread: {error}"));
                let outcome: Result<crate::cloud_stt::CloudSttResult, String> = match join_outcome {
                    Ok(handle) => handle
                        .join()
                        .map_err(|_| "cloud-stt thread panicked".to_string())
                        .and_then(|value| value),
                    Err(error) => Err(error),
                };

                let result: Result<InferenceResult, String> = match outcome {
                    Ok(cloud_result) => {
                        if cancel_flag.load(std::sync::atomic::Ordering::Relaxed) {
                            Err("cloud transcribe cancelled after response".to_string())
                        } else {
                            Ok(InferenceResult {
                                session_id,
                                text: cloud_result.text,
                                language: None,
                                model_id: cloud_model_id.clone(),
                                inference_time_ms: started.elapsed().as_millis() as u64,
                                audio_seconds,
                            })
                        }
                    }
                    Err(error) => Err(error),
                };

                let completed_event = EngineEvent::InferenceCompleted {
                    session_id,
                    result: result.clone().map_err(|e| e.clone()),
                };
                let _ = event_tx.blocking_send(completed_event);
                let reply_payload = match &result {
                    Ok(r) => r.clone(),
                    Err(_) => InferenceResult {
                        session_id,
                        text: String::new(),
                        language: None,
                        model_id: cloud_model_id.clone(),
                        inference_time_ms: 0,
                        audio_seconds: 0.0,
                    },
                };
                let _ = reply.send(reply_payload);
                last_activity = std::time::Instant::now();
            }
            EngineCommand::Shutdown => break,
        }
    }
    log::info!("whisper engine thread exiting");
}

/// Занято ли приложение прямо сейчас — по человеческим меркам, не по своим.
///
/// Пока диктуют, очередь команд пуста: звук копится в записи и придёт одной
/// командой в самом конце. Для движка это неотличимо от простоя, и без этой
/// проверки модель успевала бы уйти из памяти ровно посередине фразы.
/// Расшифровка файла держит движок так же — задолго до `Transcribe`, ещё на
/// раскодировании. Знает об этом только состояние приложения, поэтому здесь
/// поток движка единственный раз смотрит наружу.
fn app_is_busy(app: &AppHandle) -> bool {
    let Some(state) = app.try_state::<crate::state::AppState>() else {
        // Движок стартует раньше, чем состояние регистрируют. Значит, ни
        // записи, ни файла ещё нет — и занятости тоже.
        return false;
    };
    state.recorder.is_recording() || state.is_engine_busy()
}

/// Emit `InferenceCompleted(Err(msg))` AND reply on the oneshot
/// with an empty `InferenceResult` so the Tauri command never
/// blocks. Used for every "skip .full()" arm in the Transcribe
/// handler (cancel pre-.full(), model-not-loaded, create_state
/// failure).
fn short_circuit_error(
    session_id: u64,
    message: String,
    model_id: Option<String>,
    event_tx: &tokio::sync::mpsc::Sender<EngineEvent>,
    reply: oneshot::Sender<InferenceResult>,
) {
    let _ = event_tx.blocking_send(EngineEvent::InferenceCompleted {
        session_id,
        result: Err(message),
    });
    let _ = reply.send(InferenceResult {
        session_id,
        text: String::new(),
        language: None,
        model_id,
        inference_time_ms: 0,
        audio_seconds: 0.0,
    });
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
/// itself — including the move from the pre-rename `whisper-desktop` — is
/// decided by [`crate::model::models_dir`]. This function used to spell the
/// same path out a second time, which is exactly the copy that the rename
/// would have left behind pointing at the old directory.
pub fn resolve_model_path(model_name: &str) -> Result<PathBuf, String> {
    let models_dir = crate::model::models_dir()?;
    std::fs::create_dir_all(&models_dir).map_err(|e| format!("create models dir: {e}"))?;
    Ok(models_dir.join(format!("ggml-{model_name}.bin")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn a_late_chunk_of_the_previous_dictation_never_reaches_the_new_one() {
        // Ответвление звука отцепляют при остановке, но в его очереди
        // остаётся до секунды записанного, и пересыльщик дочитывает её уже
        // после старта следующей диктовки. Без этого правила хвост прошлой
        // фразы докармливал распознаватель, и новая начиналась с чужих слов.
        assert_eq!(preview_action(Some(7), 6), PreviewAction::Skip);

        // Первый кусок новой диктовки — сигнал забыть прошлую гипотезу.
        assert_eq!(preview_action(Some(6), 7), PreviewAction::Restart);
        assert_eq!(preview_action(None, 7), PreviewAction::Restart);

        // Своё продолжаем, ничего не сбрасывая: сброс посреди фразы стёр бы
        // уже разобранное.
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
    fn inference_result_has_required_fields() {
        let r = InferenceResult {
            session_id: 42,
            text: "hello".into(),
            language: Some("en".into()),
            model_id: Some("large-v3".into()),
            inference_time_ms: 100,
            audio_seconds: 3.0,
        };
        assert_eq!(r.session_id, 42);
        assert_eq!(r.text, "hello");
        assert_eq!(r.model_id.as_deref(), Some("large-v3"));
    }

    #[test]
    fn resolve_model_path_uses_the_models_directory_override() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = crate::test_support::EnvGuard::set("SPEECH_TO_TEXT_MODELS_DIR", dir.path());
        assert_eq!(
            resolve_model_path("large-v3-turbo").unwrap(),
            dir.path().join("ggml-large-v3-turbo.bin")
        );
    }

    #[test]
    fn resolve_model_path_creates_dir() {
        let dir = tempfile::tempdir().unwrap();
        let models = dir.path().join("models");
        let _guard = crate::test_support::EnvGuard::set("SPEECH_TO_TEXT_MODELS_DIR", &models);
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
    fn inference_result_supports_empty_text() {
        // A no-audio / silent recording or a session that produced no
        // segments must serialize cleanly with empty text. The frontend
        // uses this struct verbatim in `whisper-done` events.
        let r = InferenceResult {
            session_id: 1,
            text: String::new(),
            language: None,
            model_id: None,
            inference_time_ms: 0,
            audio_seconds: 0.0,
        };
        assert_eq!(r.text.len(), 0);
        assert_eq!(r.session_id, 1);
        assert_eq!(r.language, None);
        assert_eq!(r.inference_time_ms, 0);
    }
}
