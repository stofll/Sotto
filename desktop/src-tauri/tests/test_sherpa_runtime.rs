//! Opt-in native runtime smoke test. Downloads the two smallest catalog bundles
//! into a temporary directory and verifies their pinned hashes before using FFI.
//! Run: cargo test --locked --test test_sherpa_runtime -- --ignored
//! Silence exercises inference and stream reset; this is not an accuracy test.
#![cfg(any(windows, target_os = "macos"))]

use sotto_lib::model::{self, ModelLoadSpec};
use sotto_lib::model_download::{download_bundle_to_dir, BundleDownloadSpec, DownloadSpec};
use sotto_lib::sherpa::SherpaRecognizer;
use std::sync::{atomic::AtomicBool, Arc};

#[tokio::test]
#[ignore = "downloads approximately 100 MB of verified ONNX models"]
async fn sherpa_download_load_infer_and_reload() {
    let models = tempfile::tempdir().unwrap();
    // This integration-test executable has only one test, so its environment
    // cannot race with the application/unit tests or touch the user's cache.
    std::env::set_var("SPEECH_TO_TEXT_MODELS_DIR", models.path());
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(180))
        .build()
        .unwrap();
    let cancel = Arc::new(AtomicBool::new(false));
    let silence = vec![0.0_f32; 16_000];
    for id in ["zipformer-ru", "zipformer-ru-streaming"] {
        let entry = model::bundle_manifest_entry(id).unwrap();
        let spec = BundleDownloadSpec {
            model_id: id.into(),
            directory_name: entry.directory_name.into(),
            artifacts: entry
                .artifacts
                .iter()
                .map(|artifact| DownloadSpec {
                    model_id: id.into(),
                    file_name: artifact.file_name.into(),
                    url: artifact.download_url.into(),
                    expected_bytes: artifact.expected_bytes,
                    sha256: artifact.sha256.into(),
                })
                .collect(),
        };
        download_bundle_to_dir(&client, &spec, models.path(), &cancel, None, None)
            .await
            .unwrap();
        assert!(model::is_downloaded(id));
        model::verify_bundle_files(id).unwrap();
        let ModelLoadSpec::Sherpa { engine, files } = model::model_load_spec(id, false).unwrap()
        else {
            panic!("{id} must resolve to Sherpa");
        };
        for _ in 0..2 {
            let mut recognizer = SherpaRecognizer::open(engine, &files, 2).unwrap();
            recognizer.transcribe(16_000, &silence).unwrap();
            recognizer.reset_preview();
            let preview = recognizer.feed_preview(16_000, &silence).unwrap();
            assert_eq!(preview.is_some(), id == "zipformer-ru-streaming");
            recognizer.transcribe(16_000, &silence).unwrap();
        }
    }
    std::env::remove_var("SPEECH_TO_TEXT_MODELS_DIR");
}
