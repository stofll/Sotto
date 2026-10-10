//! Opt-in inference on pinned public speech with isolated, verified models.
#![cfg(any(windows, target_os = "macos"))]

mod common;
use sotto_lib::{model, model_download, sherpa::SherpaRecognizer};
use std::sync::{atomic::AtomicBool, Arc};

#[tokio::test]
#[ignore = "downloads about 2.5 GB, or copies from SOTTO_TEST_CATALOG_DIR"]
async fn expanded_catalog_recognizes_speech_and_reuses_models_after_cancellation() {
    let models = tempfile::tempdir().unwrap();
    std::env::set_var("SOTTO_MODELS_DIR", models.path());
    let source = std::env::var_os("SOTTO_TEST_CATALOG_DIR").map(std::path::PathBuf::from);
    let filter = std::env::var("SOTTO_TEST_CATALOG_MODEL").ok();
    let client = sotto_lib::http_client::builder().build().unwrap();
    let poetry = common::speech_sample(&client, concat!(
        "https://huggingface.co/csukuangfj/sherpa-onnx-nemo-ctc-punct-giga-am-v3-russian-2025-12-16",
        "/resolve/4fb5407ff028a69fec516cdf4c10fac9ddea7c16/test_wavs/example.wav"),
        "d8aaaa18a5098d7c6de0595ae7ac1e64cacd0d4022af3595213bdaf23be77e69").await;
    let qwen_speech = common::speech_sample(
        &client,
        concat!(
            "https://huggingface.co/csukuangfj2/sherpa-onnx-qwen3-asr-0.6B-int8-2026-03-25",
            "/resolve/68818b2313fe77bd06f6a7c5068ff3ef59d02b8a/test_wavs/ru1.wav"
        ),
        "e48b22f32d4d1c38f0a94a58acfc43bb8f5b7fc3b0ac01ea49372040ca831acf",
    )
    .await;
    let english = common::speech_sample(&client,
        "https://raw.githubusercontent.com/ggml-org/whisper.cpp/a8d002cfd879315632a579e73f0148d06959de36/samples/jfk.wav",
        "59dfb9a4acb36fe2a2affc14bacbee2920ff435cb13cc314a08c13f66ba7860e").await;
    let mut tested = 0;
    for id in [
        "parakeet-ultra",
        "gigaam-multilingual",
        "gigaam-multilingual-large",
        "qwen3-asr-0.6b",
    ] {
        if filter.as_deref().is_some_and(|value| value != id) {
            continue;
        }
        tested += 1;
        // The pinned Qwen export also misspells words in its own prose sample
        // in a direct upstream run. Check recognizable reference anchors and
        // repeated phrases here, not an unsupported claim of exact accuracy.
        let (speech, first_word, last_word) = if id == "qwen3-asr-0.6b" {
            (&qwen_speech, "живущий", "вол")
        } else {
            (&poetry, "лукомор", "зел")
        };
        let entry = model::bundle_manifest_entry(id).unwrap();
        if let Some(source) = &source {
            let target = models.path().join(entry.directory_name);
            std::fs::create_dir(&target).unwrap();
            for artifact in entry.artifacts {
                std::fs::copy(
                    source.join(id).join(artifact.file_name),
                    target.join(artifact.file_name),
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
        model::verify_bundle_files(id).unwrap();
        if id == "qwen3-asr-0.6b" {
            let vocab = models.path().join(id).join("vocab.json");
            let bytes = std::fs::read(&vocab).unwrap();
            let mut corrupt = bytes.clone();
            corrupt[0] ^= 1;
            std::fs::write(&vocab, corrupt).unwrap();
            assert!(model::verify_bundle_files(id)
                .unwrap_err()
                .contains("SHA256"));
            std::fs::write(&vocab, bytes).unwrap();
            model::verify_bundle_files(id).unwrap();
        }
        let model::ModelLoadSpec::Sherpa { engine, files } =
            model::model_load_spec(id, false).unwrap()
        else {
            panic!("expected sherpa")
        };
        let start = std::time::Instant::now();
        let mut recognizer = SherpaRecognizer::open(engine, &files, 4).unwrap();
        eprintln!("{id} load: {:.2}s", start.elapsed().as_secs_f64());
        let fragment_seconds = model::fragment_seconds(id);
        let baseline = recognizer
            .transcribe_segmented(speech, fragment_seconds, Some("ru"), None, || false)
            .unwrap();
        eprintln!("{id} RU: {baseline}");
        assert!(
            baseline.to_lowercase().contains(first_word),
            "{id}: {baseline}"
        );
        assert!(
            baseline.to_lowercase().contains(last_word),
            "{id}: {baseline}"
        );
        let en = recognizer
            .transcribe_segmented(
                &english,
                fragment_seconds,
                Some("en"),
                Some("Americans, country"),
                || false,
            )
            .unwrap();
        eprintln!("{id} EN: {en}");
        assert!(en.to_lowercase().contains("country"), "{id}: {en}");
        assert!(!recognizer.is_streaming());
        assert_eq!(recognizer.feed_preview(16_000, speech).unwrap(), None);
        let mut long = Vec::new();
        if id == "qwen3-asr-0.6b" {
            // This generative export collapses repeated speech even upstream.
            // Distinct bookends verify decoding after a long pause without
            // treating that known accuracy limitation as a stitching failure.
            long.extend_from_slice(speech);
            long.extend(std::iter::repeat_n(0.0, 30 * 16_000));
            long.extend_from_slice(&english);
        } else {
            for _ in 0..12 {
                long.extend_from_slice(speech);
                long.extend(std::iter::repeat_n(0.0, 8_000));
            }
        }
        // Long enough to cross a fragment boundary of this model.
        assert!(long.len() > fragment_seconds * 16_000);
        let result = recognizer
            .transcribe_segmented(&long, fragment_seconds, None, None, || false)
            .unwrap();
        if id == "qwen3-asr-0.6b" {
            let result = result.to_lowercase();
            let start = result.find(first_word).expect("Russian opening was lost");
            let end = result.find("country").expect("English ending was lost");
            assert!(start < end, "{id}: {result}");
        } else {
            assert_eq!(
                result.to_lowercase().matches(last_word).count(),
                12,
                "{id}: {result}"
            );
        }
        // Cancellation immediately after the first decode must discard its prefix.
        let checks = std::cell::Cell::new(0);
        assert!(recognizer
            .transcribe_segmented(&long, fragment_seconds, Some("ru"), None, || {
                checks.set(checks.get() + 1);
                checks.get() >= 3
            })
            .unwrap_err()
            .contains("cancelled"));
        assert_eq!(
            recognizer
                .transcribe_segmented(speech, fragment_seconds, Some("ru"), None, || false)
                .unwrap(),
            baseline
        );
        if id == "qwen3-asr-0.6b" {
            let hints = "Санкт-Петербург, ".repeat(1000);
            let text = recognizer
                .transcribe_segmented(speech, fragment_seconds, Some("ru"), Some(&hints), || false)
                .unwrap();
            assert!(
                text.to_lowercase().contains(last_word),
                "large hints lost audio: {text}"
            );
            assert!(recognizer
                .transcribe_segmented(
                    &vec![0.0; 16_000],
                    fragment_seconds,
                    None,
                    Some("OpenAI"),
                    || false
                )
                .unwrap()
                .is_empty());
        }
        drop(recognizer);
        let mut reopened = SherpaRecognizer::open(engine, &files, 4).unwrap();
        assert_eq!(
            reopened
                .transcribe_segmented(speech, fragment_seconds, Some("ru"), None, || false)
                .unwrap(),
            baseline
        );
    }
    assert!(tested > 0, "unknown SOTTO_TEST_CATALOG_MODEL filter");
    std::env::remove_var("SOTTO_MODELS_DIR");
}
