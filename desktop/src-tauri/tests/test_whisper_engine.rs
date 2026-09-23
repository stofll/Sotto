//! Public engine contracts and opt-in real Whisper CPU inference smoke test.
//! Run the asset-backed test locally and in CI with:
//! `cargo test --locked --test test_whisper_engine -- --ignored --nocapture`.
//! This exercises model download/load/inference, not native UI or clipboard.

mod common;
use sotto_lib::model::ModelLoadSpec;
use sotto_lib::whisper::{
    resolve_model_path, EngineCommand, EngineEvent, InferenceResult, ModelLoadReason,
};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

/// Phase 4 / Batch 4 / PR 4.5: TranscribeCloud is constructible
/// from outside the engine crate (frontend bridge or other
/// crates can dispatch cloud STT without depending on private
/// types). Pins the public surface.
#[test]
fn transcribe_cloud_variant_compiles() {
    use sotto_lib::cloud_stt::{CloudSttProvider, CloudSttRequest};
    let (reply, _rx) = tokio::sync::oneshot::channel();
    let _cmd = EngineCommand::TranscribeCloud {
        session_id: 1,
        audio: Arc::new(vec![0.0_f32; 1600]),
        speech_timing: sotto_lib::whisper::SpeechTiming::Ready(None),
        cancel_flag: Arc::new(AtomicBool::new(false)),
        request: CloudSttRequest {
            provider: CloudSttProvider::Compatible,
            base_url: "https://api.groq.com/openai/v1".into(),
            api_key: "sk-test".into(),
            model: "whisper-large-v3-turbo".into(),
            language: Some("ru".into()),
            audio: Arc::new(vec![0.0_f32; 1600]),
            timeout_seconds: 45,
        },
        reply,
    };
}

#[test]
fn channels_accept_all_engine_command_variants() {
    // Constuct each EngineCommand variant to confirm they're still
    // constructible from a downstream crate (i.e. enums didn't gain a
    // private field that broke external construction).
    let (tx, _rx) = tokio::sync::mpsc::channel::<EngineCommand>(8);

    assert!(tx.try_send(EngineCommand::Shutdown).is_ok());
    assert!(tx
        .try_send(EngineCommand::UnloadModel {
            reply: tokio::sync::oneshot::channel().0,
        })
        .is_ok());
    assert!(tx
        .try_send(EngineCommand::SetModel {
            name: "medium".into(),
            spec: ModelLoadSpec::Whisper {
                path: resolve_model_path("medium").unwrap(),
                use_gpu: true,
            },
            reason: ModelLoadReason::Requested,
            reply: tokio::sync::oneshot::channel().0,
        })
        .is_ok());
    assert!(tx
        .try_send(EngineCommand::UnloadIdle {
            after: std::time::Duration::from_secs(300),
        })
        .is_ok());
    assert!(tx
        .try_send(EngineCommand::Transcribe {
            session_id: 1,
            audio: Arc::new(vec![0.0_f32; 16000]),
            speech_timing: sotto_lib::whisper::SpeechTiming::Ready(None),
            cancel_flag: Arc::new(AtomicBool::new(false)),
            language: None,
            initial_prompt: None,
            reply: tokio::sync::oneshot::channel().0,
        })
        .is_ok());
}

#[test]
fn channels_accept_all_engine_event_variants() {
    // Capacity is sized by the number of variants: with less, the last try_send
    // returned a queue-full error rather than "the channel rejected this
    // variant". While the result was discarded via `let _`, the test never
    // noticed.
    let (tx, _rx) = tokio::sync::mpsc::channel::<EngineEvent>(16);
    assert!(tx
        .try_send(EngineEvent::ModelLoading { name: "x".into() })
        .is_ok());
    assert!(tx
        .try_send(EngineEvent::ModelReady { name: "x".into() })
        .is_ok());
    assert!(tx
        .try_send(EngineEvent::ModelUnloaded { name: "x".into() })
        .is_ok());
    assert!(tx
        .try_send(EngineEvent::ModelRestored { name: "x".into() })
        .is_ok());
    assert!(tx
        .try_send(EngineEvent::ModelLoadFailed {
            name: "x".into(),
            error: "x".into(),
        })
        .is_ok());
    assert!(tx
        .try_send(EngineEvent::InferenceStarted { session_id: 1 })
        .is_ok());
    assert!(tx
        .try_send(EngineEvent::InferenceCompleted {
            session_id: 1,
            result: Ok(InferenceResult {
                session_id: 1,
                text: "x".into(),
                language: None,
                model_id: Some("medium".into()),
                stt_service: None,
                inference_time_ms: 0,
                audio_seconds: 0.0,
                speech_seconds: None,
            }),
        })
        .is_ok());
}

#[tokio::test]
#[ignore = "downloads approximately 75 MB of verified Whisper weights and a speech fixture"]
async fn whisper_download_load_and_recognize_speech() {
    use sotto_lib::model_download::{download_spec_to_dir, DownloadSpec};
    use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

    let models = tempfile::tempdir().unwrap();
    let client = sotto_lib::http_client::builder()
        .timeout(std::time::Duration::from_secs(180))
        .build()
        .unwrap();
    let spec = DownloadSpec::from_manifest("tiny").unwrap();
    download_spec_to_dir(
        &client,
        &spec,
        models.path(),
        &Arc::new(AtomicBool::new(false)),
        None,
        None,
    )
    .await
    .unwrap();
    let speech = common::speech_sample(&client,
        "https://raw.githubusercontent.com/ggml-org/whisper.cpp/a8d002cfd879315632a579e73f0148d06959de36/samples/jfk.wav",
        "59dfb9a4acb36fe2a2affc14bacbee2920ff435cb13cc314a08c13f66ba7860e").await;
    let path = models.path().join(&spec.file_name);
    let context = WhisperContext::new_with_params(
        path.to_str().unwrap(),
        WhisperContextParameters {
            use_gpu: false,
            ..Default::default()
        },
    )
    .unwrap();
    let mut state = context.create_state().unwrap();
    // Reusing a state must not leak the previous recognition into the next one.
    for _ in 0..2 {
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_language(Some("en"));
        params.set_n_threads(4);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        state.full(params, &speech).unwrap();
        let mut text = String::new();
        for segment in 0..state.full_n_segments().unwrap() {
            text.push_str(&state.full_get_segment_text(segment).unwrap());
        }
        let normalized: String = text
            .to_lowercase()
            .chars()
            .map(|c| if c.is_ascii_alphabetic() { c } else { ' ' })
            .collect();
        let words = normalized.split_whitespace().collect::<Vec<_>>().join(" ");
        assert!(
            words.contains("ask not what your country can do for you")
                && words.contains("what you can do for your country"),
            "unexpected fixture transcript: {text}"
        );
    }
}
