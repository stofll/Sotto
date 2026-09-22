//! Decoding an audio file on disk into the one buffer shape the engine
//! accepts: 16 kHz, mono, `f32`.
//!
//! Files need container decoding and channel mixdown before the stateful
//! resampler shared with microphone capture normalizes their sample rate.
//!
//! Everything here is synchronous and CPU-bound. Callers run it on
//! `spawn_blocking` — decoding an hour of MP3 on the async runtime's
//! thread would stall every other task.

use std::path::Path;
use std::sync::{atomic::AtomicBool, Arc};

use symphonia::core::audio::GenericAudioBufferRef;
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use tauri::{AppHandle, Emitter, Manager};

/// The rate the whisper/sherpa engines expect. Not configurable: whisper.cpp
/// is trained at 16 kHz and resamples internally (badly) if given anything
/// else.
pub const TARGET_RATE: u32 = 16_000;

/// Longest file we will decode, in seconds.
///
/// This is a memory guard, not a policy: the decoded buffer is `f32`, so
/// three hours is 3 × 3600 × 16000 × 4 B ≈ 691 MB. Source-rate PCM is
/// resampled one packet at a time instead of retained for the whole file. Beyond
/// this the app is a likelier cause of the user's next out-of-memory than
/// whatever they were transcribing.
pub const MAX_DURATION_SECONDS: f64 = 3.0 * 3600.0;

#[derive(Debug, Clone)]
pub struct DecodedAudio {
    /// 16 kHz mono, ready for `EngineCommand::Transcribe`.
    pub samples: Vec<f32>,
    /// Duration of `samples`, in seconds. Derived from the sample count
    /// rather than from container metadata: the metadata is what a
    /// truncated or mis-muxed file lies about, and this number ends up in
    /// the LLM duration gate.
    pub audio_seconds: f64,
}

/// Decode `path` into 16 kHz mono `f32`.
///
/// Errors are localized and meant to be shown verbatim — the caller has no
/// more context to add, and symphonia's own messages ("unsupported codec")
/// tell a person nothing about which of their files is the problem.
pub fn decode_to_pcm16k_mono(path: &Path) -> Result<DecodedAudio, String> {
    decode_with_limit(path, MAX_DURATION_SECONDS)
}

/// The body of [`decode_to_pcm16k_mono`], with the duration cap injected.
///
/// The cap exists to stop a 10-hour file from exhausting memory, and the
/// only honest test of it would need a 10-hour file. Taking the limit as an
/// argument lets a test use a fraction of a second instead — the guard is
/// the same code either way.
fn decode_with_limit(path: &Path, max_seconds: f64) -> Result<DecodedAudio, String> {
    let file = std::fs::File::open(path).map_err(|e| {
        crate::ui_text::t("Не удалось открыть файл: {p0}").replace("{p0}", &e.to_string())
    })?;

    // The extension is a hint, not a decision: symphonia probes the actual
    // bytes and will happily decode an .mp3 that is really a .m4a. The hint
    // only saves it some guessing.
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let stream = MediaSourceStream::new(Box::new(file), Default::default());
    let mut format = symphonia::default::get_probe()
        .probe(
            &hint,
            stream,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|e| describe_symphonia_error(&e))?;

    // A video file (or a multi-track recording) has tracks we must not feed
    // to an audio decoder; `default_track` picks the one the container
    // itself marks as the audio track to play.
    let track = format
        .default_track(TrackType::Audio)
        .ok_or_else(|| crate::ui_text::t("В файле нет звуковой дорожки."))?;
    let track_id = track.id;
    let codec_params = track
        .codec_params
        .as_ref()
        .and_then(|params| params.audio())
        .ok_or_else(|| crate::ui_text::t("В файле нет звуковой дорожки."))?
        .clone();

    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(&codec_params, &AudioDecoderOptions::default())
        .map_err(|e| describe_symphonia_error(&e))?;

    let mut mono = Vec::<f32>::new();
    let mut samples = Vec::<f32>::new();
    let mut filter: Option<crate::audio_resampler::AudioResampler> = None;
    let mut source_frames = 0usize;
    let mut interleaved = Vec::<f32>::new();
    // Taken from the decoded buffers, not from `codec_params`: for some
    // containers the header rate is absent or stale, and the decoder is the
    // one that knows what it actually produced.
    let mut source_rate: Option<u32> = None;

    loop {
        let packet = match format.next_packet() {
            Ok(Some(packet)) => packet,
            // End of stream.
            Ok(None) => break,
            // A truncated file ends mid-packet rather than politely. What
            // was decoded up to here is still the user's recording, so keep
            // it instead of throwing the whole transcription away.
            Err(SymphoniaError::IoError(ref e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                log::warn!("audio_file: stream ended early, keeping what decoded");
                break;
            }
            Err(e) => return Err(describe_symphonia_error(&e)),
        };

        if packet.track_id != track_id {
            continue;
        }

        let decoded = match decoder.decode(&packet) {
            Ok(decoded) => decoded,
            // Both are per-packet conditions the symphonia docs call
            // recoverable: skip the packet, keep the stream. Dropping a
            // frame costs ~20 ms of audio; aborting costs the whole file.
            Err(SymphoniaError::DecodeError(msg)) => {
                log::warn!("audio_file: skipping malformed packet: {msg}");
                continue;
            }
            Err(SymphoniaError::ResetRequired) => {
                decoder.reset();
                continue;
            }
            Err(e) => return Err(describe_symphonia_error(&e)),
        };

        let rate = decoded.spec().rate();
        if let Some(refusal) = rate_refusal(rate, source_rate) {
            return Err(refusal);
        }
        if filter.is_none() {
            source_rate = Some(rate);
            filter = Some(
                crate::audio_resampler::AudioResampler::new(rate, TARGET_RATE)
                    .map_err(describe_resampling_error)?,
            );
        }
        mono.clear();
        append_downmixed(&decoded, &mut interleaved, &mut mono);
        source_frames += mono.len();
        // Count decoded frames, not container metadata or padded resampler output.
        if source_frames as f64 / f64::from(rate) > max_seconds {
            return Err(crate::ui_text::t("Файл длиннее {p0} часов.")
                .replace("{p0}", &format!("{:.0}", max_seconds / 3600.0)));
        }
        samples.extend_from_slice(
            filter
                .as_mut()
                .unwrap()
                .push(&mono)
                .map_err(describe_resampling_error)?,
        );
    }

    if source_frames == 0 {
        return Err(crate::ui_text::t("В файле нет звука."));
    }
    // Flush once even for a truncated final packet, preserving the filter tail
    // and the exact rounded duration instead of padding to a whole block.
    samples.extend_from_slice(
        filter
            .as_mut()
            .unwrap()
            .finish()
            .map_err(describe_resampling_error)?,
    );

    Ok(DecodedAudio {
        audio_seconds: samples.len() as f64 / f64::from(TARGET_RATE),
        samples,
    })
}

/// Average all channels of one decoded buffer into `mono`.
///
/// Averaging rather than taking the first channel: a stereo interview with
/// one speaker per channel loses a speaker outright if you pick a side, and
/// a mid/side-ish recording can leave you with the quiet one.
///
/// `interleaved` is a scratch buffer owned by the caller so the per-packet
/// allocation happens once per file rather than once per packet.
fn append_downmixed(
    decoded: &GenericAudioBufferRef<'_>,
    interleaved: &mut Vec<f32>,
    mono: &mut Vec<f32>,
) {
    let channels = decoded.spec().channels().count();
    if channels == 0 {
        return;
    }
    decoded.copy_to_vec_interleaved(interleaved);
    mono.reserve(interleaved.len() / channels);
    for frame in interleaved.chunks_exact(channels) {
        mono.push(frame.iter().sum::<f32>() / channels as f32);
    }
}

/// Why this packet's rate cannot continue the file, or `None` if it can.
///
/// A rate that changes partway through is a joined recording, not a damaged
/// one: two files concatenated, or a chained Ogg stream. One filter holds one
/// ratio for the whole run and cannot follow the change, so such a file is
/// still refused — but refused for what it is. Reporting damage sends the user
/// looking for corruption that re-encoding to a single stream would not reveal,
/// and re-encoding is the fix.
fn rate_refusal(rate: u32, established: Option<u32>) -> Option<String> {
    if rate == 0 {
        return Some(crate::ui_text::t(
            "Не удалось прочитать звук из файла — возможно, он повреждён.",
        ));
    }
    if established.is_some_and(|previous| previous != rate) {
        return Some(crate::ui_text::t(
            "В файле меняется частота дискретизации. Перекодируйте его в один поток.",
        ));
    }
    None
}

fn describe_resampling_error(error: String) -> String {
    log::error!("decode: resampling to {TARGET_RATE} failed: {error}");
    crate::ui_text::t("Не удалось преобразовать частоту дискретизации файла.")
}

/// Turn a symphonia error into something a person can act on.
///
/// The distinction that matters to a user is "this file is broken" vs
/// "this app cannot read this kind of file" — the first means try another
/// copy, the second means convert it. symphonia's own strings blur the two.
fn describe_symphonia_error(error: &SymphoniaError) -> String {
    match error {
        SymphoniaError::Unsupported(what) => {
            log::warn!("audio_file: unsupported: {what}");
            crate::ui_text::t(
                "Этот формат не поддерживается. Сконвертируйте файл в wav, mp3 или m4a.",
            )
        }
        SymphoniaError::DecodeError(what) => {
            log::warn!("audio_file: decode error: {what}");
            crate::ui_text::t("Не удалось прочитать звук из файла — возможно, он повреждён.")
        }
        SymphoniaError::LimitError(what) => {
            log::warn!("audio_file: limit reached: {what}");
            crate::ui_text::t("Не удалось прочитать звук из файла — возможно, он повреждён.")
        }
        SymphoniaError::IoError(e) => {
            log::warn!("audio_file: io error: {e}");
            crate::ui_text::t("Не удалось открыть файл: {p0}").replace("{p0}", &e.to_string())
        }
        other => {
            log::warn!("audio_file: {other}");
            crate::ui_text::t("Не удалось прочитать звук из файла — возможно, он повреждён.")
        }
    }
}

/// Open the system file picker and return the chosen audio file's path.
///
/// The dialog lives in Rust rather than in the webview so the extension
/// filter and the picker permission stay on this side: the frontend can
/// ask for a file, but it cannot ask for an arbitrary one.
///
/// `Ok(None)` means the user closed the dialog — a normal outcome, not an
/// error, and the panel must not show anything for it.
#[tauri::command]
pub(crate) async fn pick_audio_file(app: AppHandle) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;

    // `blocking_pick_file` on a blocking worker: the docs are explicit that
    // it must not run on the main thread, and a Tauri command's async task
    // is not a safe place to park a modal either.
    let picked = tokio::task::spawn_blocking(move || {
        app.dialog()
            .file()
            .add_filter(
                crate::ui_text::t("Аудио"),
                &["wav", "mp3", "m4a", "mp4", "ogg", "oga", "opus", "flac"],
            )
            .blocking_pick_file()
    })
    .await
    .map_err(|e| {
        log::error!("pick_audio_file: dialog task failed: {e}");
        crate::ui_text::t("Не удалось открыть диалог выбора файла.")
    })?;

    Ok(picked.map(|file| file.to_string()))
}

/// What the "Прикрепить аудио" panel gets back from a file transcription.
///
/// Deliberately not `InferenceResult`: the panel shows the *processed*
/// text, and it needs the intermediate stages to render the "Whisper без
/// обработки" disclosure the same way history does.
#[derive(Debug, serde::Serialize)]
pub struct TranscribeFileResult {
    /// The text to show and copy — formatted, and LLM-cleaned when the
    /// configuration calls for it.
    text: String,
    /// Straight from the engine, before any formatting.
    raw_text: String,
    /// After local formatting, before the LLM.
    formatted_text: String,
    /// `None` when the LLM never ran (disabled, or the mode is local-only).
    /// That is a normal outcome for a file, not a failure — the panel shows
    /// "Распознано" for it, not an error.
    ai_status: Option<crate::ai::step::AiStatus>,
    audio_seconds: f64,
    inference_time_ms: u64,
    language: Option<String>,
}

/// Why a file transcription stopped, in the shape the terminal handler needs.
///
/// The point of the type is that the body below can go back to using `?`.
/// Before it existed, every early return had to remember its own telemetry
/// call — eleven of them — and a forgotten one loses the operation from the
/// failure rate silently, without so much as a warning.
#[derive(Debug)]
struct FileFailure {
    stage: crate::telemetry::FailureStage,
    reason: crate::telemetry::FailureReason,
    message: String,
}

impl FileFailure {
    fn new(
        stage: crate::telemetry::FailureStage,
        reason: crate::telemetry::FailureReason,
        message: String,
    ) -> Self {
        Self {
            stage,
            reason,
            message,
        }
    }
}

/// Errors that arrive as a bare message from somewhere further down land in
/// the generic bucket rather than claiming a stage they cannot know.
impl From<String> for FileFailure {
    fn from(message: String) -> Self {
        Self::new(
            crate::telemetry::FailureStage::Stt,
            crate::telemetry::FailureReason::EngineError,
            message,
        )
    }
}

/// What a completed run carries out of the inner function: the engine's
/// result and the post-processed text, both of which the terminal event and
/// the panel's payload are built from.
struct FileRun {
    inference: crate::whisper::InferenceResult,
    processed: crate::ProcessedTranscription,
}

/// Transcribe an audio file the user attached, without touching the
/// focused window, the history, or the statistics.
///
/// This runs the same engine and the same post-processing as dictation;
/// the only difference is where the samples come from and where the text
/// goes. Everything unusual about it is defensive, and each guard below
/// exists because the engine is a single shared resource that the
/// dictation path assumes it owns.
///
/// The body lives in [`transcribe_file_inner`]; this wrapper is the single
/// place a terminal telemetry event is emitted, on either outcome.
#[tauri::command]
pub(crate) async fn transcribe_audio_file(
    app: AppHandle,
    state: tauri::State<'_, crate::state::AppState>,
    path: String,
) -> Result<TranscribeFileResult, String> {
    let telemetry = app.state::<crate::telemetry::Telemetry>().clone();
    telemetry.begin_usage_session(crate::telemetry::SessionTrigger::File);

    // Read once, up front, because every terminal event needs it. The guards
    // that fire before the engine is even claimed used to report `local`
    // whatever the user had configured, which quietly mislabelled every
    // engine-busy failure in cloud mode.
    let config = crate::config::Config::load(&app).ok();
    let pipeline_mode = crate::telemetry_pipeline_mode(config.as_ref());

    match transcribe_file_inner(&app, &state, &path, config.as_ref(), &pipeline_mode).await {
        Ok(run) => {
            record_file_run(&telemetry, &pipeline_mode, config.as_ref(), &run);
            let FileRun {
                inference,
                processed,
            } = run;
            Ok(TranscribeFileResult {
                text: processed.final_text,
                raw_text: processed.raw_text,
                formatted_text: processed.formatted_text,
                ai_status: processed.ai_status,
                audio_seconds: inference.audio_seconds,
                inference_time_ms: inference.inference_time_ms,
                language: inference.language,
            })
        }
        Err(failure) => {
            // A deliberate cancellation is not a reliability failure, and
            // counting it as one would make the failure rate meaningless.
            if matches!(
                failure.reason,
                crate::telemetry::FailureReason::UserCancelled
            ) {
                telemetry.record_cancelled(crate::telemetry::Source::File, &pipeline_mode);
            } else {
                telemetry.record_failed(
                    crate::telemetry::Source::File,
                    &pipeline_mode,
                    failure.stage,
                    failure.reason,
                );
            }
            Err(failure.message)
        }
    }
}

/// Emit the completed event for a run that produced text.
///
/// An empty `final_text` is not an error the caller sees — the panel still
/// gets its (empty) result — but it means the post-processor threw away the
/// whole transcript, which is a failure worth counting.
fn record_file_run(
    telemetry: &crate::telemetry::Telemetry,
    pipeline_mode: &str,
    config: Option<&crate::config::Config>,
    run: &FileRun,
) {
    if run.processed.final_text.trim().is_empty() {
        telemetry.record_failed(
            crate::telemetry::Source::File,
            pipeline_mode,
            crate::telemetry::FailureStage::PostProcess,
            crate::telemetry::FailureReason::EmptyAfterProcessing,
        );
        return;
    }
    let (formatting_enabled, replacement_rules) = config
        .map(crate::telemetry_formatting)
        .unwrap_or((false, 0));
    telemetry.record_completed(crate::telemetry::Outcome {
        source: crate::telemetry::Source::File,
        pipeline_mode,
        recording_mode: crate::telemetry::RecordingMode::NotApplicable,
        stt_model: run.inference.model_id.as_deref(),
        stt_service: run.inference.stt_service,
        audio_seconds: run.inference.audio_seconds,
        stt_millis: run.inference.inference_time_ms,
        chars: run.processed.final_text.chars().count(),
        ai_status: run.processed.ai_status.as_ref(),
        compute: config
            .map(|config| crate::telemetry_compute(config, run.inference.model_id.as_deref())),
        formatting_enabled,
        replacement_rules,
        paste_result: crate::telemetry::PasteResult::NotApplicable,
    });
}

async fn transcribe_file_inner(
    app: &AppHandle,
    state: &crate::state::AppState,
    path: &str,
    config: Option<&crate::config::Config>,
    pipeline_mode: &str,
) -> Result<FileRun, FileFailure> {
    // 1. Claim the engine. The guard releases on every exit path below,
    //    including the `?`s — a hand-written release would not.
    let engine_claim = state.claim_engine().ok_or_else(|| {
        FileFailure::new(
            crate::telemetry::FailureStage::Start,
            crate::telemetry::FailureReason::EngineBusy,
            crate::ui_text::t("Идёт транскрипция файла — дождитесь её окончания."),
        )
    })?;

    // 2. A dictation in flight owns the engine too, just through a
    //    different mechanism (it was queued before we claimed).
    if !matches!(
        *crate::mutex_recover::lock(&state.app_fsm),
        crate::state::AppFsm::Idle
    ) {
        return Err(FileFailure::new(
            crate::telemetry::FailureStage::Start,
            crate::telemetry::FailureReason::EngineBusy,
            crate::ui_text::t("Завершите текущую запись."),
        ));
    }

    // 3. The sherpa recognizers have no VAD of their own. They are fine on a
    //    dictation-length utterance and degrade badly across an hour-long
    //    recording, so refuse rather than hand back mush the user would blame
    //    on the file. Only checked for the local path — cloud STT does not
    //    touch the loaded model at all.
    if pipeline_mode != "cloud" {
        // Cloned out of the guard rather than read through it: `model_engine`
        // is unrelated code, and holding an engine lock across it is how the
        // next deadlock gets written.
        let loaded = crate::mutex_recover::lock(&state.engine_current_model).clone();
        let is_sherpa = loaded.as_deref().is_some_and(|model| {
            crate::model::model_engine(model).is_ok_and(|engine| engine.is_sherpa())
        });
        if is_sherpa {
            return Err(FileFailure::new(
                crate::telemetry::FailureStage::Stt,
                crate::telemetry::FailureReason::EngineError,
                crate::ui_text::t(
                    "Эта модель не умеет расшифровывать файлы — выберите модель Whisper в «Настройки → Модели».",
                ),
            ));
        }
    }

    // 4. Decode off the async runtime: symphonia and the resampler are
    //    CPU-bound, and an hour of MP3 would stall every other task.
    let decode_path = std::path::PathBuf::from(path);
    let decode_failed = |message: String| {
        FileFailure::new(
            crate::telemetry::FailureStage::Decode,
            crate::telemetry::FailureReason::Decode,
            message,
        )
    };
    let decoded = tokio::task::spawn_blocking(move || decode_to_pcm16k_mono(&decode_path))
        .await
        .map_err(|e| {
            log::error!("transcribe_audio_file: decode task panicked: {e}");
            decode_failed(crate::ui_text::t(
                "Не удалось прочитать звук из файла — возможно, он повреждён.",
            ))
        })?
        .map_err(decode_failed)?;

    log::info!(
        "file transcription: {:.1}s of audio decoded from {path}",
        decoded.audio_seconds
    );

    let session_id = state.next_session_id();
    let audio = Arc::new(decoded.samples);
    let cancel_flag = Arc::new(AtomicBool::new(false));

    // 5. Claimed before the command is queued; the guard releases on every
    //    exit path below.
    let session_guard = state.claim_file_session(session_id, Arc::clone(&cancel_flag));
    // The frontend needs the id to be able to cancel; the result carries it
    // too late to be useful.
    let _ = app.emit(
        "file-transcription-started",
        serde_json::json!({ "session_id": session_id }),
    );

    // The same queue orders restoration before file transcription as well.
    crate::restore_unloaded_model(app, state);

    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
    // Same branch as `stop_recording`: a cloud-configured user has no local
    // model loaded, and sending `Transcribe` would fail with «модель не
    // загружена» for a reason that has nothing to do with their setup.
    let command = if pipeline_mode == "cloud" {
        // Built before the move: the request borrows the samples that the
        // command is about to take ownership of.
        let request = crate::build_cloud_stt_request(app, &audio).map_err(|error| {
            FileFailure::new(
                crate::telemetry::FailureStage::Queue,
                crate::telemetry::FailureReason::CloudConfiguration,
                error,
            )
        })?;
        crate::whisper::EngineCommand::TranscribeCloud {
            session_id,
            audio,
            speech_timing: crate::vad::SpeechTiming::Ready(None),
            cancel_flag,
            request,
            reply: reply_tx,
        }
    } else {
        crate::whisper::EngineCommand::Transcribe {
            source: crate::model_performance::RunSource::File,
            session_id,
            audio,
            speech_timing: crate::vad::SpeechTiming::Ready(None),
            cancel_flag,
            language: config.and_then(|cfg| cfg.get_string("language")),
            initial_prompt: config.and_then(crate::custom_words_prompt),
            reply: reply_tx,
        }
    };

    // `send`, not `try_send`: the queue is ours by the claim above, and a
    // full-channel error here would be a lie about what went wrong.
    state.engine_cmd_tx.send(command).await.map_err(|e| {
        log::error!("file transcription {session_id}: engine channel closed: {e}");
        FileFailure::new(
            crate::telemetry::FailureStage::Queue,
            crate::telemetry::FailureReason::EngineQueue,
            format!("engine: {e}"),
        )
    })?;

    let inference = reply_rx.await.map_err(|e| {
        log::error!("file transcription {session_id}: engine dropped the reply: {e}");
        FileFailure::new(
            crate::telemetry::FailureStage::Stt,
            crate::telemetry::FailureReason::EngineError,
            crate::ui_text::t("Движок не ответил. Попробуйте ещё раз."),
        )
    })?;
    let inference = file_inference_result(inference, state.is_cancelled(session_id))?;
    if inference.text.trim().is_empty() {
        return Err(FileFailure::new(
            crate::telemetry::FailureStage::Stt,
            crate::telemetry::FailureReason::EmptyTranscript,
            crate::ui_text::t("В файле не распознана речь."),
        ));
    }

    // The engine is genuinely free from here on — what remains is local
    // formatting and, in hybrid mode, an LLM round-trip that can take the
    // better part of a minute. Holding the claim across it would refuse the
    // user's dictation for no reason at all.
    drop(engine_claim);
    let processed = await_file_processing(
        crate::post_process_transcription(app, &inference),
        state.wait_cancelled(session_id),
    )
    .await
    .ok_or_else(|| file_cancelled(crate::telemetry::FailureStage::PostProcess))?;
    // This is the file's delivery point: a concurrent cancel either wins here
    // or arrives after a completed result. It never cancels a later dictation.
    if !state.begin_commit(session_id) {
        return Err(file_cancelled(crate::telemetry::FailureStage::PostProcess));
    }
    drop(session_guard);

    Ok(FileRun {
        inference,
        processed,
    })
}

fn file_cancelled(stage: crate::telemetry::FailureStage) -> FileFailure {
    FileFailure::new(
        stage,
        crate::telemetry::FailureReason::UserCancelled,
        crate::ui_text::t("Транскрипция отменена."),
    )
}

fn file_inference_result(
    result: Result<crate::whisper::InferenceResult, String>,
    cancelled: bool,
) -> Result<crate::whisper::InferenceResult, FileFailure> {
    if cancelled {
        return Err(file_cancelled(crate::telemetry::FailureStage::Stt));
    }
    result.map_err(FileFailure::from)
}

async fn await_file_processing<T>(
    processing: impl std::future::Future<Output = T>,
    cancellation: impl std::future::Future<Output = ()>,
) -> Option<T> {
    tokio::select! {
        biased;
        _ = cancellation => None,
        result = processing => Some(result),
    }
}

/// Cancel an active file session during STT or optional post-processing.
/// The file guard keeps its registration alive after releasing the engine,
/// so this can stop the LLM request without affecting another dictation.
#[tauri::command(rename_all = "snake_case")]
pub(crate) async fn cancel_audio_file(
    state: tauri::State<'_, crate::state::AppState>,
    session_id: u64,
) -> Result<(), String> {
    if !state.is_dispatch_skipped(session_id) {
        log::info!("cancel_audio_file: session {session_id} is no longer in flight, ignoring");
        return Ok(());
    }
    state.request_cancel(session_id);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A joined recording and a damaged one need different answers: only one of
    /// them is fixed by re-encoding, and the message is the only place the user
    /// learns which they have.
    #[test]
    fn a_changed_rate_is_refused_as_a_joined_file_not_a_damaged_one() {
        let damaged = rate_refusal(0, None).expect("a zero rate is unusable");
        assert_eq!(rate_refusal(0, Some(44_100)).as_deref(), Some(&*damaged));

        let changed = rate_refusal(48_000, Some(44_100)).expect("the rate changed");
        assert_ne!(
            changed, damaged,
            "a mid-file rate change must not be reported as corruption"
        );

        // The first packet establishes the rate, and every later packet that
        // agrees with it continues the file.
        assert_eq!(rate_refusal(44_100, None), None);
        assert_eq!(rate_refusal(44_100, Some(44_100)), None);
    }

    #[test]
    fn engine_errors_retain_their_reason_unless_the_file_was_cancelled() {
        for message in ["HTTP 401", "request timed out", "model not loaded"] {
            let failure = file_inference_result(Err(message.into()), false).unwrap_err();
            assert_eq!(failure.message, message);
            assert!(matches!(
                failure.reason,
                crate::telemetry::FailureReason::EngineError
            ));
            let cancelled = file_inference_result(Err(message.into()), true).unwrap_err();
            assert!(matches!(
                cancelled.reason,
                crate::telemetry::FailureReason::UserCancelled
            ));
        }
    }

    #[tokio::test]
    async fn cancellation_drops_an_in_flight_file_post_processor() {
        struct InFlight(Arc<AtomicBool>);
        impl Drop for InFlight {
            fn drop(&mut self) {
                self.0.store(true, std::sync::atomic::Ordering::Release);
            }
        }
        let dropped = Arc::new(AtomicBool::new(false));
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let active = InFlight(Arc::clone(&dropped));
        let processing = async move {
            let _active = active;
            started_tx.send(()).unwrap();
            std::future::pending::<String>().await
        };
        let result = await_file_processing(processing, async {
            started_rx.await.unwrap();
        })
        .await;
        assert!(result.is_none());
        assert!(dropped.load(std::sync::atomic::Ordering::Acquire));
        assert_eq!(
            await_file_processing(async { "result" }, std::future::pending()).await,
            Some("result")
        );
        assert!(await_file_processing(async { "already ready" }, async {})
            .await
            .is_none());
    }

    /// A WAV file built in memory, so the tests do not depend on committed
    /// binary fixtures for the one format we can encode ourselves.
    fn wav_bytes(samples: &[f32], rate: u32, channels: u16) -> Vec<u8> {
        let pcm = crate::wav::f32_to_pcm16(samples);
        if channels == 1 {
            return crate::wav::encode_pcm16_mono(&pcm, rate);
        }
        // `wav.rs` only encodes mono, so patch the header for the
        // multi-channel cases: channel count and the two derived rate
        // fields. Interleaving is the caller's job.
        let mut bytes = crate::wav::encode_pcm16_mono(&pcm, rate);
        bytes[22..24].copy_from_slice(&channels.to_le_bytes());
        let byte_rate = rate * u32::from(channels) * 2;
        bytes[28..32].copy_from_slice(&byte_rate.to_le_bytes());
        let block_align = channels * 2;
        bytes[32..34].copy_from_slice(&block_align.to_le_bytes());
        bytes
    }

    fn write_temp(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("sotto-audio-file-tests");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    fn sine(rate: u32, seconds: f64, freq: f64) -> Vec<f32> {
        let count = (rate as f64 * seconds) as usize;
        (0..count)
            .map(|i| {
                let t = i as f64 / f64::from(rate);
                (std::f64::consts::TAU * freq * t).sin() as f32 * 0.5
            })
            .collect()
    }

    #[test]
    fn decodes_16k_mono_wav_unchanged_in_length() {
        let samples = sine(16_000, 1.0, 440.0);
        let path = write_temp("mono16k.wav", &wav_bytes(&samples, 16_000, 1));

        let decoded = decode_to_pcm16k_mono(&path).expect("wav must decode");

        assert_eq!(
            decoded.samples.len(),
            16_000,
            "a 1 s 16 kHz file must stay 16000 samples — no resampling should happen"
        );
        assert!(
            (decoded.audio_seconds - 1.0).abs() < 0.01,
            "audio_seconds must follow the sample count, got {}",
            decoded.audio_seconds
        );
    }

    #[test]
    fn packet_resampling_preserves_partial_tails_at_high_rates() {
        let dir = tempfile::tempdir().unwrap();
        for rate in [44_100, 48_000, 96_000] {
            // A non-block-aligned stereo tail exercises both downmix and flush.
            let frames = rate as usize + 317;
            let mut interleaved = Vec::with_capacity(frames * 2);
            for i in 0..frames {
                let sample = if i > frames - 500 { 0.5 } else { 0.0 };
                interleaved.extend_from_slice(&[sample, sample]);
            }
            let path = dir.path().join(format!("tail-{rate}.wav"));
            std::fs::write(&path, wav_bytes(&interleaved, rate, 2)).unwrap();
            let decoded = decode_to_pcm16k_mono(&path).unwrap();
            let expected = (frames as f64 * 16_000.0 / f64::from(rate)).round() as usize;
            assert_eq!(decoded.samples.len(), expected, "rate {rate}");
            assert!(
                decoded.samples[expected - 50..expected - 20]
                    .iter()
                    .all(|v| *v > 0.4),
                "lost tail at {rate}"
            );
            // Container truncation must still preserve all complete decoded packets.
            let mut bytes = std::fs::read(&path).unwrap();
            bytes.truncate(bytes.len() - 40);
            std::fs::write(&path, bytes).unwrap();
            let partial = decode_to_pcm16k_mono(&path).unwrap();
            assert!(!partial.samples.is_empty());
            assert!(partial.samples.len() <= expected);
        }
    }

    #[test]
    fn resamples_44100_to_16000() {
        let samples = sine(44_100, 1.0, 440.0);
        let path = write_temp("mono44k.wav", &wav_bytes(&samples, 44_100, 1));

        let decoded = decode_to_pcm16k_mono(&path).expect("44.1 kHz wav must decode");

        // The whole point of the module: whatever came in, 16 kHz comes out.
        assert!(
            (decoded.samples.len() as i64 - 16_000).abs() <= 2,
            "1 s at 44.1 kHz must resample to ~16000 samples, got {}",
            decoded.samples.len()
        );
        assert!(
            (decoded.audio_seconds - 1.0).abs() < 0.01,
            "audio_seconds must be ~1 s, got {}",
            decoded.audio_seconds
        );
    }

    #[test]
    fn resampling_preserves_the_signal_not_just_the_length() {
        // A length-only assertion passes for a resampler that outputs
        // silence, or noise. Check that a 440 Hz tone is still a 440 Hz
        // tone by correlating the output against a locally generated
        // reference at the target rate.
        let path = write_temp(
            "tone44k.wav",
            &wav_bytes(&sine(44_100, 0.5, 440.0), 44_100, 1),
        );
        let decoded = decode_to_pcm16k_mono(&path).expect("tone must decode");

        let reference = sine(16_000, 0.5, 440.0);
        let n = decoded.samples.len().min(reference.len());
        // Skip the first and last 10 ms: filter edges are not the signal.
        let skip = 160;
        let dot: f64 = (skip..n - skip)
            .map(|i| f64::from(decoded.samples[i]) * f64::from(reference[i]))
            .sum();
        let energy_a: f64 = (skip..n - skip)
            .map(|i| f64::from(decoded.samples[i]).powi(2))
            .sum();
        let energy_b: f64 = (skip..n - skip)
            .map(|i| f64::from(reference[i]).powi(2))
            .sum();
        let correlation = dot / (energy_a.sqrt() * energy_b.sqrt());

        assert!(
            correlation > 0.99,
            "resampled tone must still be the same tone, correlation was {correlation}"
        );
    }

    /// Committed fixtures: 0.5 s of a 440 Hz tone at 44.1 kHz mono, one per
    /// container/codec pair, made once with ffmpeg. Regenerating them is not
    /// part of any build — they are inputs, and a test that generates its own
    /// input with the same library it is testing proves nothing.
    fn fixture(name: &str) -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/audio")
            .join(name)
    }

    /// Every compressed format we claim to support, decoded end to end.
    ///
    /// Lossy codecs are not compared sample-by-sample — the point is that
    /// the demuxer feature, the codec feature and the resampler all line up,
    /// which is exactly what a missing Cargo feature breaks. A wrong or
    /// absent feature shows up here as "unsupported format", not as a subtle
    /// quality regression.
    #[test]
    fn decodes_every_supported_container() {
        for name in [
            "tone.mp3",
            "tone.m4a",
            "tone.ogg",
            "tone.flac",
            "tone_alac.m4a",
        ] {
            let decoded = decode_to_pcm16k_mono(&fixture(name))
                .unwrap_or_else(|e| panic!("{name} must decode, got: {e}"));

            assert!(
                (decoded.audio_seconds - 0.5).abs() < 0.06,
                "{name}: 0.5 s in must be ~0.5 s out, got {}",
                decoded.audio_seconds
            );
            let peak = decoded
                .samples
                .iter()
                .fold(0.0f32, |acc, s| acc.max(s.abs()));
            assert!(
                peak > 0.1,
                "{name}: decoded to near-silence (peak {peak}) — the codec \
                 produced no signal"
            );
        }
    }

    /// The container says 44.1 kHz; the engine only accepts 16 kHz. This is
    /// the one property every fixture must share, and the one a "just pass
    /// the samples through" regression would break for real files while the
    /// synthetic WAV tests still passed.
    #[test]
    fn compressed_fixtures_come_out_at_the_target_rate() {
        for name in ["tone.mp3", "tone.m4a", "tone.ogg", "tone.flac"] {
            let decoded = decode_to_pcm16k_mono(&fixture(name))
                .unwrap_or_else(|e| panic!("{name} must decode, got: {e}"));
            let implied_rate = decoded.samples.len() as f64 / decoded.audio_seconds;

            assert!(
                (implied_rate - f64::from(TARGET_RATE)).abs() < 1.0,
                "{name}: must be resampled to {TARGET_RATE} Hz, implied {implied_rate}"
            );
            assert!(
                decoded.samples.len() < 12_000,
                "{name}: 0.5 s at 16 kHz is ~8000 samples; {} means the 44.1 kHz \
                 source was never resampled",
                decoded.samples.len()
            );
        }
    }

    #[test]
    fn refuses_a_file_longer_than_the_cap() {
        // 1 s of audio against a 0.1 s cap. The real cap is three hours; the
        // guard is the same line, and this is the only way to reach it
        // without a three-hour fixture.
        let path = write_temp("long.wav", &wav_bytes(&sine(16_000, 1.0, 440.0), 16_000, 1));

        let error = decode_with_limit(&path, 0.1).expect_err("a file over the cap must be refused");

        assert!(
            error.contains("длиннее") || error.contains("longer"),
            "the refusal must say the file is too long, got: {error}"
        );
    }

    #[test]
    fn accepts_a_file_inside_the_cap() {
        // The other half of the guard: a cap that never fires must not
        // change the result. Without this, a cap of `0.0` would pass the
        // test above and reject everything.
        let path = write_temp(
            "short.wav",
            &wav_bytes(&sine(16_000, 1.0, 440.0), 16_000, 1),
        );

        let decoded = decode_with_limit(&path, 10.0).expect("a file under the cap must decode");

        assert_eq!(decoded.samples.len(), 16_000);
    }

    #[test]
    fn downmixes_stereo_by_averaging_channels() {
        // Left is a tone, right is its exact inverse. Averaging cancels
        // them to silence; taking either channel alone would not. That
        // makes this test fail loudly if the downmix ever becomes
        // "pick channel 0".
        let tone = sine(16_000, 0.5, 440.0);
        let mut interleaved = Vec::with_capacity(tone.len() * 2);
        for sample in &tone {
            interleaved.push(*sample);
            interleaved.push(-*sample);
        }
        let path = write_temp("stereo.wav", &wav_bytes(&interleaved, 16_000, 2));

        let decoded = decode_to_pcm16k_mono(&path).expect("stereo wav must decode");

        assert_eq!(
            decoded.samples.len(),
            tone.len(),
            "downmix must halve the sample count, not the frame count"
        );
        let peak = decoded
            .samples
            .iter()
            .fold(0.0f32, |acc, s| acc.max(s.abs()));
        assert!(
            peak < 0.01,
            "L and -L must average to silence; peak was {peak}"
        );
    }

    #[test]
    fn rejects_a_file_that_is_not_audio() {
        let path = write_temp(
            "garbage.wav",
            b"this is not a wav file at all, not even close",
        );

        let error = decode_to_pcm16k_mono(&path).expect_err("garbage must not decode");

        assert!(
            error.contains("поддерживается") || error.contains("format"),
            "the error must say the format is unsupported, got: {error}"
        );
    }

    #[test]
    fn rejects_a_missing_file() {
        let path = std::env::temp_dir().join("sotto-no-such-file-4f9a.wav");
        let _ = std::fs::remove_file(&path);

        let error = decode_to_pcm16k_mono(&path).expect_err("a missing file must not decode");

        assert!(
            error.contains("открыть") || error.contains("open"),
            "the error must say the file could not be opened, got: {error}"
        );
    }

    #[test]
    fn rejects_a_silent_but_empty_stream() {
        // Zero data frames: a valid header describing no audio. The engine
        // would return an empty transcription for this, which reads to the
        // user as "the app is broken" rather than "the file is empty".
        let path = write_temp("empty.wav", &wav_bytes(&[], 16_000, 1));

        let error = decode_to_pcm16k_mono(&path).expect_err("an empty file must not decode");

        // The exact message, not just "some error": an earlier version of
        // this test asserted only that decoding failed, and passed while
        // the empty-audio guard it was meant to cover was disabled — the
        // failure came from a different branch reporting "corrupt file".
        assert_eq!(
            error,
            crate::ui_text::t("В файле нет звука."),
            "an empty file must be reported as empty, not as corrupt"
        );
    }

    #[test]
    fn audio_seconds_matches_the_sample_count_exactly() {
        // 2.5 s at 48 kHz — a rate that is not a whole multiple of 16 kHz
        // after the sinc filter's rounding, which is where an off-by-a-chunk
        // in the resample loop would show up.
        let path = write_temp("48k.wav", &wav_bytes(&sine(48_000, 2.5, 220.0), 48_000, 1));

        let decoded = decode_to_pcm16k_mono(&path).expect("48 kHz wav must decode");

        assert_eq!(
            decoded.audio_seconds,
            decoded.samples.len() as f64 / f64::from(TARGET_RATE),
            "audio_seconds must be derived from the sample count, never from metadata"
        );
        assert!(
            (decoded.audio_seconds - 2.5).abs() < 0.01,
            "2.5 s in must be 2.5 s out, got {}",
            decoded.audio_seconds
        );
    }

    #[test]
    fn max_duration_is_three_hours() {
        assert_eq!(MAX_DURATION_SECONDS, 10_800.0);
    }

    /// A truncated file is still the user's recording: whatever decoded before
    /// the break must come back rather than be lost to an error wholesale.
    #[test]
    fn truncated_file_keeps_what_decoded() {
        let mut bytes = wav_bytes(&sine(16_000, 0.5, 440.0), 16_000, 1);
        bytes.truncate(bytes.len() - 20);
        let path = write_temp("truncated.wav", &bytes);

        let decoded =
            decode_to_pcm16k_mono(&path).expect("a truncated wav must keep its partial audio");
        assert!(
            !decoded.samples.is_empty(),
            "some audio must survive a truncated tail"
        );
    }

    #[test]
    fn downmixes_identical_channels_to_the_same_signal() {
        // L == R: the average is the signal itself. `sum / channels` is caught
        // here rather than at L == -R (there the sum is zero and `*`/`%` give
        // the same zero).
        let tone = sine(16_000, 0.5, 440.0);
        let mut interleaved = Vec::with_capacity(tone.len() * 2);
        for sample in &tone {
            interleaved.push(*sample);
            interleaved.push(*sample);
        }
        let path = write_temp("stereo_same.wav", &wav_bytes(&interleaved, 16_000, 2));

        let decoded = decode_to_pcm16k_mono(&path).expect("stereo wav must decode");

        assert_eq!(decoded.samples.len(), tone.len());
        let peak = decoded
            .samples
            .iter()
            .fold(0.0f32, |acc, s| acc.max(s.abs()));
        assert!(
            (peak - 0.5).abs() < 0.1,
            "averaging identical channels must keep the 0.5-amplitude signal, peak {peak}"
        );
    }
}
