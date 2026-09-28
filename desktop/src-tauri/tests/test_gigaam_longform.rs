//! Prepared public speech, isolated model copy and no application data.
#![cfg(any(windows, target_os = "macos"))]

mod common;

#[tokio::test]
#[ignore = "requires SOTTO_TEST_GIGAAM_DIR; downloads a checksum-pinned public speech fixture"]
async fn gigaam_longform_preserves_repeated_phrases_and_cancellation() {
    use sotto_lib::{model, sherpa::SherpaRecognizer};
    let source = std::path::PathBuf::from(
        std::env::var_os("SOTTO_TEST_GIGAAM_DIR")
            .expect("set SOTTO_TEST_GIGAAM_DIR to a GigaAM bundle"),
    );
    let models = tempfile::tempdir().unwrap();
    let entry = model::bundle_manifest_entry("gigaam-v3").unwrap();
    let bundle = models.path().join(entry.directory_name);
    std::fs::create_dir(&bundle).unwrap();
    for artifact in entry.artifacts {
        std::fs::copy(
            source.join(artifact.file_name),
            bundle.join(artifact.file_name),
        )
        .unwrap();
    }
    std::env::set_var("SOTTO_MODELS_DIR", models.path());
    model::verify_bundle_files("gigaam-v3").unwrap();
    let model::ModelLoadSpec::Sherpa { engine, files } =
        model::model_load_spec("gigaam-v3", false).unwrap()
    else {
        panic!("expected GigaAM Sherpa bundle");
    };
    let client = sotto_lib::http_client::builder().build().unwrap();
    let speech = common::speech_sample(&client, concat!(
        "https://huggingface.co/csukuangfj/sherpa-onnx-nemo-ctc-punct-giga-am-v3-russian-2025-12-16",
        "/resolve/4fb5407ff028a69fec516cdf4c10fac9ddea7c16/test_wavs/example.wav"),
        "d8aaaa18a5098d7c6de0595ae7ac1e64cacd0d4022af3595213bdaf23be77e69").await;
    let mut recognizer = SherpaRecognizer::open(engine, &files, 4).unwrap();
    let baseline = recognizer.transcribe(16_000, &speech).unwrap();
    assert!(baseline.contains("зелёный"), "{baseline}");
    let short = recognizer.transcribe_gigaam(&speech, || false).unwrap();
    assert_eq!(short, baseline.trim());
    let mut long = Vec::new();
    for _ in 0..12 {
        long.extend_from_slice(&speech);
        long.extend(std::iter::repeat_n(0.0, 8_000));
    }
    assert!(long.len() > 25 * 16_000);
    let result = recognizer.transcribe_gigaam(&long, || false).unwrap();
    assert_eq!(result.matches("зелёный").count(), 12, "{result}");
    assert!(recognizer.transcribe_gigaam(&long, || true).is_err());
    assert_eq!(
        recognizer.transcribe_gigaam(&speech, || false).unwrap(),
        short
    );
    std::env::remove_var("SOTTO_MODELS_DIR");
}
