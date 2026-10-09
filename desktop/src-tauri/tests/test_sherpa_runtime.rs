//! Opt-in native runtime smoke test. Downloads the two smallest catalog bundles
//! into a temporary directory and verifies their pinned hashes before using FFI.
//! Run: cargo test --locked --test test_sherpa_runtime -- --ignored
//! Silence exercises reset; speech checks the streaming decoder's final word.
#![cfg(any(windows, target_os = "macos"))]

mod common;
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
    std::env::set_var("SOTTO_MODELS_DIR", models.path());
    let client = sotto_lib::http_client::builder()
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
        if id == "zipformer-ru-streaming" {
            let mut recognizer = SherpaRecognizer::open(engine, &files, 2).unwrap();
            let speech = common::speech_sample(&client, concat!(
                "https://huggingface.co/csukuangfj/sherpa-onnx-nemo-ctc-punct-giga-am-v3-russian-2025-12-16",
                "/resolve/4fb5407ff028a69fec516cdf4c10fac9ddea7c16/test_wavs/example.wav"),
                "d8aaaa18a5098d7c6de0595ae7ac1e64cacd0d4022af3595213bdaf23be77e69").await;
            let text = recognizer.transcribe(16_000, &speech).unwrap();
            // The last word is the whole point: a cache-aware model decodes a
            // chunk only once the look-ahead behind it arrives, so without the
            // silence `finish` pads the phrase with, the recording ends on
            // «у лукоморье туб» and the final word never appears.
            assert!(
                text.contains("зелёный"),
                "streaming transcript lost its tail: {text}"
            );
            // Cancelled mid-decode, the final pass gives up instead of
            // decoding the rest, and the recognizer serves the next one intact.
            let checks = std::cell::Cell::new(0);
            let cancelled = recognizer.transcribe_unless(16_000, &speech, || {
                checks.set(checks.get() + 1);
                checks.get() > 2
            });
            assert!(cancelled.is_err(), "a cancelled decode returned text");
            assert_eq!(recognizer.transcribe(16_000, &speech).unwrap(), text);
            // A dictation feeds the live preview chunk by chunk, then transcribes
            // the whole recording on the same recognizer. Whatever the preview
            // left undecoded must not come back in front of the final text.
            // The hotkey is released right after the last word, so the recording
            // ends on speech rather than on the sample's trailing silence.
            let end = speech.iter().rposition(|s| s.abs() > 0.02).unwrap() + 1_600;
            let spoken = &speech[..end.min(speech.len())];
            let clean = recognizer.transcribe(16_000, spoken).unwrap();
            recognizer.reset_preview();
            for chunk in spoken.chunks(1600) {
                recognizer.feed_preview(16_000, chunk).unwrap();
            }
            let after_preview = recognizer.transcribe(16_000, spoken).unwrap();
            assert_eq!(
                after_preview, clean,
                "the preview's tail leaked into the final text"
            );
        }
    }
    std::env::remove_var("SOTTO_MODELS_DIR");
}
