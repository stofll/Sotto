//! Integration smoke tests for the whisper engine module.
//!
//! Most assertions here are *compile-time* checks of the public API surface
//! (`EngineCommand`, `EngineEvent`, `InferenceResult`). The full
//! `engine_thread_main` + real-model path requires:
//!
//! 1. `tauri::test::mock_app()` which needs the `test` feature on `tauri`.
//! 2. A real GGML model file on disk, path supplied by whoever runs it.
//!
//! The actual end-to-end test is gated behind `#[ignore]` and documented
//! in `engine_full_transcribe_real_model_ignored`. To run:
//!
//! ```bash
//! cd desktop/src-tauri
//! cargo test --test test_whisper_engine -- --ignored
//! ```
//!
//! (Requires model file + `tauri/test` feature enabled in dev.)

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
fn api_surface_compiles_across_crate_boundary() {
    // This exercises the public API the same way an end-user (frontend
    // bridge or other crate) would. If a field is renamed or removed,
    // this test fails to compile — catching renames early.
    let r = InferenceResult {
        session_id: 1,
        text: "hello".into(),
        language: Some("en".into()),
        model_id: Some("medium".into()),
        inference_time_ms: 123,
        audio_seconds: 1.0,
    };
    assert_eq!(r.session_id, 1);
    assert_eq!(r.text, "hello");
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
                inference_time_ms: 0,
                audio_seconds: 0.0,
            }),
        })
        .is_ok());
}

/// Real-model end-to-end: requires a local GGML model file and the
/// `tauri/test` cargo feature. Marked `#[ignore]` so CI (without model
/// and without Tauri test feature) doesn't fail.
#[test]
#[ignore = "requires a local GGML model file AND tauri/test feature"]
fn engine_full_transcribe_real_model_ignored() {
    // Body intentionally empty — see module docs for the exact recipe to
    // flesh this out in WS 4a2 (when real cpal audio + clipboard
    // plumbing are also in place). Until then, the smoke tests above
    // cover everything that's testable without external assets.
}
