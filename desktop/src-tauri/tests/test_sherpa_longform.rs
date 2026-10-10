//! Long recordings through the Sherpa models outside the GigaAM path: offline
//! ones in fragments, streaming ones in a single stream. The model directory is
//! only read; speech comes from public fixtures, never from application data.
#![cfg(any(windows, target_os = "macos"))]

mod common;

/// The fixture's word that must survive once per repetition, lowercased. The
/// Russian one has no «ё» or unstressed vowel that weaker models drop.
const RU: (&str, &str, &str) = (
    concat!(
        "https://huggingface.co/csukuangfj/sherpa-onnx-nemo-ctc-punct-giga-am-v3-russian-2025-12-16",
        "/resolve/4fb5407ff028a69fec516cdf4c10fac9ddea7c16/test_wavs/example.wav"
    ),
    "d8aaaa18a5098d7c6de0595ae7ac1e64cacd0d4022af3595213bdaf23be77e69",
    "грешн",
);
const EN: (&str, &str, &str) = (
    "https://raw.githubusercontent.com/ggml-org/whisper.cpp/a8d002cfd879315632a579e73f0148d06959de36/samples/jfk.wav",
    "59dfb9a4acb36fe2a2affc14bacbee2920ff435cb13cc314a08c13f66ba7860e",
    "americans",
);

#[tokio::test]
#[ignore = "requires SOTTO_TEST_MODELS_DIR with the bundles; downloads checksum-pinned public speech fixtures"]
async fn long_recordings_keep_every_repetition_with_the_remaining_models() {
    use sotto_lib::{model, sherpa::SherpaRecognizer};
    let models = std::env::var_os("SOTTO_TEST_MODELS_DIR")
        .expect("set SOTTO_TEST_MODELS_DIR to a directory of verified model bundles");
    std::env::set_var("SOTTO_MODELS_DIR", &models);
    let filter = std::env::var("SOTTO_TEST_SHERPA_MODEL").ok();
    let client = sotto_lib::http_client::builder().build().unwrap();
    let mut fixtures = Vec::new();
    for (url, sha256, word) in [RU, EN] {
        fixtures.push((common::speech_sample(&client, url, sha256).await, word));
    }
    let mut failures = Vec::new();
    let mut tested = 0;
    for id in [
        "parakeet-tdt-v3",
        "parakeet-tdt-v2-en",
        "parakeet-ultra",
        "zipformer-ru",
        "zipformer-ru-streaming",
        "nemotron-streaming",
        "parakeet-streaming-en",
        "canary-180m-flash",
        "moonshine-base-en",
        "sense-voice",
        "omnilingual-300m",
    ] {
        if filter.as_deref().is_some_and(|value| value != id) {
            continue;
        }
        tested += 1;
        model::verify_bundle_files(id).unwrap_or_else(|error| panic!("{id}: {error}"));
        let model::ModelLoadSpec::Sherpa { engine, files } =
            model::model_load_spec(id, false).unwrap()
        else {
            panic!("{id}: expected a Sherpa bundle");
        };
        let (speech, word) = &fixtures[usize::from(!model::model_supports_language(id, "ru"))];
        let mut long = Vec::new();
        for _ in 0..12 {
            long.extend_from_slice(speech);
            long.extend(std::iter::repeat_n(0.0, 8_000));
        }
        let fragment_seconds = model::fragment_seconds(id);
        assert!(long.len() > fragment_seconds * 16_000, "{id}");
        let mut recognizer = SherpaRecognizer::open(engine, &files, 4).unwrap();
        let single = recognizer.transcribe(16_000, speech).unwrap();
        // The split the engine thread makes for a final pass without a live preview.
        let mut decode = |audio: &[f32], cancelled: &dyn Fn() -> bool| {
            if recognizer.is_streaming() {
                recognizer.transcribe_unless(16_000, audio, cancelled)
            } else {
                recognizer.transcribe_segmented(audio, fragment_seconds, None, None, cancelled)
            }
        };
        let text = decode(&long, &|| false).unwrap();
        let found = text.to_lowercase().matches(word).count();
        eprintln!("{id}: {found}/12 of «{word}»");
        if found != 12 {
            failures.push(format!("{id}: {text}"));
        }
        // Cancel after the decoding has started, not before the first fragment.
        let checks = std::cell::Cell::new(0);
        let cancel_midway = || {
            checks.set(checks.get() + 1);
            checks.get() >= 3
        };
        assert!(decode(&long, &cancel_midway).is_err(), "{id}");
        // A short recording is still one pass, and cancellation left no state behind.
        assert_eq!(decode(speech, &|| false).unwrap(), single.trim(), "{id}");
    }
    assert!(tested > 0, "unknown SOTTO_TEST_SHERPA_MODEL filter");
    assert!(failures.is_empty(), "{failures:#?}");
    std::env::remove_var("SOTTO_MODELS_DIR");
}
