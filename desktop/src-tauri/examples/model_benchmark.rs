//! Explicit developer benchmark. Only verified public speech and model files.
//! Usage: cargo run --release --locked --example model_benchmark -- tiny base
#[path = "../tests/common/mod.rs"]
mod common;

use sotto_lib::{model, model_download, model_performance, sherpa};
use std::sync::{atomic::AtomicBool, Arc};
use std::time::Instant;
type Transcribe<'a> = Box<dyn FnMut(&str) -> Result<String, String> + 'a>;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    if cfg!(debug_assertions) {
        return Err("Reference measurements require --release".into());
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/model-benchmark");
    std::fs::create_dir_all(&root)?;
    // Set before model APIs run; this process never loads the application config.
    std::env::set_var("SPEECH_TO_TEXT_MODELS_DIR", root.join("models"));
    std::env::set_var("SOTTO_CONFIG_DIR", root.join("config"));
    let models_dir = root.join("models");
    std::fs::create_dir_all(&models_dir)?;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(600))
        .build()?;
    let cancel = Arc::new(AtomicBool::new(false));
    let ru = common::speech_sample(&client,
        "https://huggingface.co/csukuangfj/sherpa-onnx-nemo-ctc-punct-giga-am-v3-russian-2025-12-16/resolve/4fb5407ff028a69fec516cdf4c10fac9ddea7c16/test_wavs/example.wav",
        "d8aaaa18a5098d7c6de0595ae7ac1e64cacd0d4022af3595213bdaf23be77e69").await;
    let en = common::speech_sample(&client,
        "https://raw.githubusercontent.com/ggml-org/whisper.cpp/a8d002cfd879315632a579e73f0148d06959de36/samples/jfk.wav",
        "59dfb9a4acb36fe2a2affc14bacbee2920ff435cb13cc314a08c13f66ba7860e").await;
    let threads = std::thread::available_parallelism()
        .map(|n| n.get().min(8))
        .unwrap_or(4) as i32;
    let mut system = sysinfo::System::new();
    system.refresh_cpu_all();
    let machine = format!(
        "{} / {} / CPU / {} threads",
        system
            .cpus()
            .first()
            .map(|cpu| cpu.brand())
            .unwrap_or("unknown"),
        std::env::consts::OS,
        threads
    );
    let ids: Vec<_> = std::env::args().skip(1).collect();
    if ids.is_empty() {
        return Err("Pass catalog model IDs explicitly".into());
    }
    let mut output = Vec::new();
    for id in ids {
        eprintln!("Preparing {id}");
        let entry = model::catalog_model(&id).ok_or("Unknown catalog model")?;
        let language = if model::model_supports_language(&id, "ru") {
            "ru"
        } else {
            "en"
        };
        if !model::model_supports_language(&id, language) {
            return Err("No fixture for model language".into());
        }
        let speech = if language == "ru" { &ru } else { &en };
        if entry.engine == model::ModelEngine::Whisper {
            let spec = model_download::DownloadSpec::from_manifest(&id)?;
            let path = models_dir.join(&spec.file_name);
            if model_download::verify_file(&path, &spec).await.is_err() {
                model_download::download_spec_to_dir(
                    &client,
                    &spec,
                    &models_dir,
                    &cancel,
                    None,
                    None,
                )
                .await?;
            }
        } else {
            let manifest = model::bundle_manifest_entry(&id)?;
            if !model::is_downloaded(&id) {
                let spec = model_download::BundleDownloadSpec {
                    model_id: id.clone(),
                    directory_name: manifest.directory_name.into(),
                    artifacts: manifest
                        .artifacts
                        .iter()
                        .map(|a| model_download::DownloadSpec {
                            model_id: id.clone(),
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
                    &models_dir,
                    &cancel,
                    None,
                    None,
                )
                .await?;
            }
            model::verify_bundle_files(&id)?;
        }
        let load_start = Instant::now();
        let mut transcribe: Transcribe<'_> = match model::model_load_spec(&id, false)? {
            model::ModelLoadSpec::Whisper { path, .. } => {
                let ctx = whisper_rs::WhisperContext::new_with_params(
                    path.to_str().ok_or("Invalid path")?,
                    whisper_rs::WhisperContextParameters {
                        use_gpu: false,
                        ..Default::default()
                    },
                )?;
                let mut state = ctx.create_state()?;
                Box::new(move |language| {
                    let mut params =
                        whisper_rs::FullParams::new(whisper_rs::SamplingStrategy::Greedy {
                            best_of: 1,
                        });
                    params.set_n_threads(threads);
                    params.set_language(Some(language));
                    params.set_print_special(false);
                    params.set_print_progress(false);
                    params.set_print_realtime(false);
                    params.set_print_timestamps(false);
                    state.full(params, speech).map_err(|e| e.to_string())?;
                    let count = state.full_n_segments().map_err(|e| e.to_string())?;
                    (0..count)
                        .map(|i| state.full_get_segment_text(i).map_err(|e| e.to_string()))
                        .collect()
                })
            }
            model::ModelLoadSpec::Sherpa { engine, files } => {
                let mut recognizer = sherpa::SherpaRecognizer::open(engine, &files, threads)?;
                Box::new(move |_| recognizer.transcribe(16000, speech))
            }
        };
        let load_ms = load_start.elapsed().as_secs_f64() * 1000.0;
        for requested_language in [language, "auto"] {
            let mut times = Vec::new();
            for run in 0..6 {
                let started = Instant::now();
                let text = transcribe(requested_language)?;
                let seconds = started.elapsed().as_secs_f64();
                if text.trim().is_empty() {
                    return Err(format!("{id}: empty speech result").into());
                }
                if run > 0 {
                    times.push(seconds);
                }
            }
            times.sort_by(f64::total_cmp);
            let seconds = speech.len() as f64 / 16000.0;
            let profile = model_performance::Profile::new(&id, "cpu");
            let result = serde_json::json!({
                "model_id": id, "revision": profile.revision, "method": profile.method,
                "compute": "cpu", "language": requested_language, "rtf": times[2] / seconds,
                "machine": machine, "fixture_language": language, "audio_seconds": seconds,
                "median_ms": times[2] * 1000.0, "min_ms": times[0] * 1000.0, "max_ms": times[4] * 1000.0,
                "load_ms": load_ms, "samples": 5,
            });
            println!("{result}");
            output.push(result);
            model_performance::write_references(&root.join("references.json"), &output)?;
        }
    }
    Ok(())
}
