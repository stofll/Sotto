//! Opt-in regression for the buffered Parakeet export and its final chunk.
#![cfg(any(windows, target_os = "macos"))]

mod common;
use sotto_lib::{model, model_download, sherpa::SherpaRecognizer};
use std::sync::{atomic::AtomicBool, Arc};

#[tokio::test]
#[ignore = "downloads approximately 632 MB; optionally uses SOTTO_TEST_PARAKEET_DIR"]
async fn parakeet_buffered_streaming_preserves_tail_and_resets() {
    let models = tempfile::tempdir().unwrap();
    // A separate integration-test process isolates this environment override.
    std::env::set_var("SPEECH_TO_TEXT_MODELS_DIR", models.path());
    let id = "parakeet-streaming-en";
    let entry = model::bundle_manifest_entry(id).unwrap();
    let client = reqwest::Client::new();
    if let Some(cache) = std::env::var_os("SOTTO_TEST_PARAKEET_DIR") {
        let dest = models.path().join(entry.directory_name);
        std::fs::create_dir_all(&dest).unwrap();
        for artifact in entry.artifacts {
            std::fs::copy(
                std::path::Path::new(&cache).join(artifact.file_name),
                dest.join(artifact.file_name),
            )
            .unwrap();
        }
    } else {
        let spec = model_download::BundleDownloadSpec {
            model_id: id.into(),
            directory_name: entry.directory_name.into(),
            artifacts: entry
                .artifacts
                .iter()
                .map(|a| model_download::DownloadSpec {
                    model_id: id.into(),
                    file_name: a.file_name.into(),
                    url: a.download_url.into(),
                    expected_bytes: a.expected_bytes,
                    sha256: a.sha256.into(),
                })
                .collect(),
        };
        model_download::download_bundle_to_dir(
            &client,
            &spec,
            models.path(),
            &Arc::new(AtomicBool::new(false)),
            None,
            None,
        )
        .await
        .unwrap();
    }
    // Cached files must pass the same verification as downloaded files.
    model::verify_bundle_files(id).unwrap();
    let model::ModelLoadSpec::Sherpa { engine, files } = model::model_load_spec(id, false).unwrap()
    else {
        panic!("expected Sherpa");
    };
    let speech = common::speech_sample(&client,
        "https://raw.githubusercontent.com/ggml-org/whisper.cpp/a8d002cfd879315632a579e73f0148d06959de36/samples/jfk.wav",
        "59dfb9a4acb36fe2a2affc14bacbee2920ff435cb13cc314a08c13f66ba7860e").await;
    let mut recognizer = SherpaRecognizer::open(engine, &files, 8).unwrap();
    for _ in 0..2 {
        check_speech(&recognizer.transcribe(16_000, &speech).unwrap());
    }
    let SherpaRecognizer::Online(mut online) = recognizer else {
        panic!("expected streaming");
    };
    online.reset();
    for chunk in speech.chunks(1600) {
        online.feed(16_000, chunk).unwrap();
    }
    check_speech(&online.finish().unwrap());
    // A cancelled partial phrase must not leak into the next dictation.
    online.reset();
    online.feed(16_000, &speech[..16_000]).unwrap();
    online.reset();
    online.feed(16_000, &speech).unwrap();
    check_speech(&online.finish().unwrap());
}

fn check_speech(text: &str) {
    let text = text.to_lowercase();
    assert!(text.contains("fellow americans"), "missing opening: {text}");
    assert!(
        text.contains("ask what you can do for your country"),
        "missing final phrase: {text}"
    );
    assert_eq!(
        text.matches("fellow americans").count(),
        1,
        "stale phrase: {text}"
    );
}
