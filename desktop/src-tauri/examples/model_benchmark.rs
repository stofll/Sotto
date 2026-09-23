//! Explicit developer benchmark. Only verified public speech and model files.
//! Usage: cargo run --release --locked --example model_benchmark -- tiny base
#[path = "../tests/common/mod.rs"]
mod common;

use sotto_lib::{model, model_download, model_performance, sherpa};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{atomic::AtomicBool, Arc};
use std::time::Instant;
type Transcribe<'a> = Box<dyn FnMut(&str) -> Result<String, String> + 'a>;

#[derive(Debug, PartialEq)]
struct Options {
    ids: Vec<String>,
    language: Option<String>,
    output: Option<PathBuf>,
    resume: bool,
    no_download: bool,
}

impl Options {
    fn parse(args: impl IntoIterator<Item = String>) -> Result<Self, String> {
        let mut args = args.into_iter();
        let mut options = Self {
            ids: Vec::new(),
            language: None,
            output: None,
            resume: false,
            no_download: false,
        };
        while let Some(arg) = args.next() {
            if arg == "--language" {
                if options.language.is_some() {
                    return Err("Pass --language only once".into());
                }
                let language = args.next().ok_or("--language requires ru or en")?;
                if !matches!(language.as_str(), "ru" | "en") {
                    return Err("Fixture language must be ru or en".into());
                }
                options.language = Some(language);
            } else if arg == "--output" {
                if options.output.is_some() {
                    return Err("Pass --output only once".into());
                }
                let path = args
                    .next()
                    .filter(|s| !s.starts_with('-') && !s.is_empty())
                    .ok_or("--output requires a file path")?;
                options.output = Some(path.into());
            } else if arg == "--resume" {
                options.resume = true;
            } else if arg == "--no-download" {
                options.no_download = true;
            } else if arg.starts_with('-') {
                return Err(format!("Unknown option: {arg}"));
            } else {
                options.ids.push(arg);
            }
        }
        if options.ids.is_empty() {
            return Err("Pass catalog model IDs explicitly".into());
        }
        let mut unique = std::collections::HashSet::new();
        if options.ids.iter().any(|id| !unique.insert(id)) {
            return Err("Pass each model ID only once".into());
        }
        // Validate every case before downloading fixtures or model weights.
        for id in &options.ids {
            options.fixture_language(id)?;
        }
        Ok(options)
    }

    fn fixture_language(&self, id: &str) -> Result<&str, String> {
        model::catalog_model(id).ok_or_else(|| format!("Unknown catalog model: {id}"))?;
        let language = self.language.as_deref().unwrap_or_else(|| {
            if model::model_supports_language(id, "ru") {
                "ru"
            } else {
                "en"
            }
        });
        if !model::model_supports_language(id, language) {
            return Err(format!("{id} does not support fixture language {language}"));
        }
        Ok(language)
    }
}

fn read_output(
    path: &Path,
    resume: bool,
) -> Result<Vec<serde_json::Value>, Box<dyn std::error::Error>> {
    if resume {
        let values: Vec<serde_json::Value> = serde_json::from_slice(&std::fs::read(path)?)?;
        return Ok(values);
    }
    // Refuse to overwrite an earlier run, even if another process created it meanwhile.
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(b"[]\n")?;
    file.sync_all()?;
    Ok(Vec::new())
}

fn checkpoint(path: &Path, values: &[serde_json::Value]) -> Result<(), Box<dyn std::error::Error>> {
    let mut file = tempfile::NamedTempFile::new_in(
        path.parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new(".")),
    )?;
    serde_json::to_writer_pretty(&mut file, values)?;
    file.write_all(b"\n")?;
    file.as_file().sync_all()?;
    file.persist(path)?;
    Ok(())
}

fn validate_resume(
    values: &[serde_json::Value],
    machine: &str,
    hardware: &str,
    method: &str,
) -> Result<(), String> {
    for value in values {
        if value["benchmark_schema"] != 1
            || value["machine"] != machine
            || value["hardware"] != hardware
            || value["method"] != method
            || value["compute"] != "cpu"
        {
            return Err("This result file belongs to a different machine or benchmark method. Use a new --output file.".into());
        }
        let valid_times = value["runs_ms"].as_array().is_some_and(|runs| {
            runs.len() == 5
                && runs
                    .iter()
                    .all(|v| v.as_f64().is_some_and(|n| n.is_finite() && n > 0.0))
        });
        if !valid_times
            || value["samples"] != 5
            || !value["rtf"]
                .as_f64()
                .is_some_and(|n| n.is_finite() && n > 0.0)
        {
            return Err("Incomplete benchmark result; use a new --output file.".into());
        }
    }
    Ok(())
}

fn completed(
    values: &[serde_json::Value],
    profile: &model_performance::Profile,
    fixture_language: &str,
    language: &str,
) -> bool {
    values.iter().any(|value| {
        value["model_id"] == profile.model_id
            && value["revision"] == profile.revision
            && value["fixture_language"] == fixture_language
            && value["language"] == language
    })
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let options = Options::parse(std::env::args().skip(1))?;
    if cfg!(debug_assertions) {
        return Err("Reference measurements require --release".into());
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/model-benchmark");
    std::fs::create_dir_all(&root)?;
    // Set before model APIs run; this process never loads the application config.
    std::env::set_var("SOTTO_MODELS_DIR", root.join("models"));
    std::env::set_var("SOTTO_DATA_DIR", root.join("data"));
    let models_dir = root.join("models");
    std::fs::create_dir_all(&models_dir)?;
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
    let output_path = options
        .output
        .clone()
        .unwrap_or_else(|| root.join("references.json"));
    let mut output = read_output(&output_path, options.resume)?;
    let context = model_performance::Profile::new(&options.ids[0], "cpu");
    validate_resume(&output, &machine, &context.hardware, &context.method)?;
    eprintln!("{machine}\nResults: {}", output_path.display());
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(600))
        .build()?;
    let cancel = Arc::new(AtomicBool::new(false));
    let mut fixtures = std::collections::HashMap::new();
    for (index, id) in options.ids.iter().enumerate() {
        let entry = model::catalog_model(id).ok_or("Unknown catalog model")?;
        let language = options.fixture_language(id)?;
        let profile = model_performance::Profile::new(id, "cpu");
        if [language, "auto"]
            .iter()
            .all(|requested| completed(&output, &profile, language, requested))
        {
            eprintln!(
                "[{}/{}] {id}: already measured, skipping",
                index + 1,
                options.ids.len()
            );
            continue;
        }
        eprintln!("[{}/{}] Preparing {id}", index + 1, options.ids.len());
        if entry.engine == model::ModelEngine::Whisper {
            let spec = model_download::DownloadSpec::from_manifest(id)?;
            let path = models_dir.join(&spec.file_name);
            if model_download::verify_file(&path, &spec).await.is_err() {
                if options.no_download {
                    return Err(format!(
                        "{id}: verified model not present in benchmark cache (--no-download)"
                    )
                    .into());
                }
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
            let manifest = model::bundle_manifest_entry(id)?;
            if !model::is_downloaded(id) {
                if options.no_download {
                    return Err(format!(
                        "{id}: model not present in benchmark cache (--no-download)"
                    )
                    .into());
                }
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
            model::verify_bundle_files(id)?;
        }
        if !fixtures.contains_key(language) {
            let (url, hash) = if language == "ru" {
                ("https://huggingface.co/csukuangfj/sherpa-onnx-nemo-ctc-punct-giga-am-v3-russian-2025-12-16/resolve/4fb5407ff028a69fec516cdf4c10fac9ddea7c16/test_wavs/example.wav", "d8aaaa18a5098d7c6de0595ae7ac1e64cacd0d4022af3595213bdaf23be77e69")
            } else {
                ("https://raw.githubusercontent.com/ggml-org/whisper.cpp/a8d002cfd879315632a579e73f0148d06959de36/samples/jfk.wav", "59dfb9a4acb36fe2a2affc14bacbee2920ff435cb13cc314a08c13f66ba7860e")
            };
            fixtures.insert(
                language.to_owned(),
                common::speech_sample(&client, url, hash).await,
            );
        }
        let speech = &fixtures[language];
        let load_start = Instant::now();
        let mut transcribe: Transcribe<'_> = match model::model_load_spec(id, false)? {
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
            if completed(&output, &profile, language, requested_language) {
                continue;
            }
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
                eprintln!(
                    "  {id}/{requested_language}: {} {:.2}s",
                    if run == 0 {
                        "warm-up".to_owned()
                    } else {
                        format!("run {run}/5")
                    },
                    seconds
                );
            }
            let runs_ms: Vec<_> = times.iter().map(|s| s * 1000.0).collect();
            times.sort_by(f64::total_cmp);
            let seconds = speech.len() as f64 / 16000.0;
            let result = serde_json::json!({
                "benchmark_schema": 1, "hardware": profile.hardware, "runs_ms": runs_ms,
                "measured_at_unix": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs(),
                "rtfx": seconds / times[2],
                "model_id": id, "revision": profile.revision, "method": profile.method,
                "compute": "cpu", "language": requested_language, "rtf": times[2] / seconds,
                "machine": machine, "fixture_language": language, "audio_seconds": seconds,
                "median_ms": times[2] * 1000.0, "min_ms": times[0] * 1000.0, "max_ms": times[4] * 1000.0,
                "load_ms": load_ms, "samples": 5,
            });
            println!("{result}");
            output.push(result);
            checkpoint(&output_path, &output)?;
            eprintln!(
                "  Saved: median {:.2}s for {:.2}s audio ({:.1}x real time)",
                times[2],
                seconds,
                seconds / times[2]
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Options, String> {
        Options::parse(args.iter().map(|arg| (*arg).to_owned()))
    }

    #[test]
    fn default_keeps_russian_when_supported() {
        let options = parse(&["tiny"]).unwrap();
        assert_eq!(options.fixture_language("tiny").unwrap(), "ru");
    }

    #[test]
    fn explicit_english_overrides_multilingual_default() {
        let options = parse(&["tiny", "--language", "en", "base"]).unwrap();
        for id in &options.ids {
            assert_eq!(options.fixture_language(id).unwrap(), "en");
        }
    }

    #[test]
    fn output_options_and_duplicate_models() {
        let options = parse(&[
            "--output",
            "pilot.json",
            "--resume",
            "--no-download",
            "tiny",
        ])
        .unwrap();
        assert_eq!(options.output, Some(PathBuf::from("pilot.json")));
        assert!(options.resume && options.no_download);
        for args in [
            vec!["tiny", "tiny"],
            vec!["tiny", "--output"],
            vec!["tiny", "--output", "--resume"],
            vec!["tiny", "--output", "a", "--output", "b"],
        ] {
            assert!(parse(&args).is_err());
        }
    }

    #[test]
    fn checkpoints_survive_reopen_and_fresh_runs_cannot_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("run.json");
        assert!(read_output(&path, false).unwrap().is_empty());
        let records = vec![serde_json::json!({"case": "first"})];
        checkpoint(&path, &records).unwrap();
        assert!(read_output(&path, false).is_err());
        assert_eq!(read_output(&path, true).unwrap(), records);
        let records = vec![
            serde_json::json!({"case": "first"}),
            serde_json::json!({"case": "second"}),
        ];
        checkpoint(&path, &records).unwrap();
        assert_eq!(read_output(&path, true).unwrap(), records);
        std::fs::write(&path, "broken").unwrap();
        assert!(read_output(&path, true).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "broken");
    }

    #[test]
    fn resume_requires_complete_matching_measurements() {
        let profile = model_performance::Profile::new("tiny", "cpu");
        let record = serde_json::json!({
            "benchmark_schema": 1, "machine": "test", "hardware": profile.hardware,
            "method": profile.method, "compute": "cpu", "model_id": "tiny",
            "revision": profile.revision, "fixture_language": "ru", "language": "auto",
            "runs_ms": [100.0, 110.0, 105.0, 100.0, 105.0], "samples": 5, "rtf": 0.1,
        });
        let rows = vec![record.clone()];
        assert!(validate_resume(&rows, "test", &profile.hardware, &profile.method).is_ok());
        assert!(validate_resume(&rows, "other", &profile.hardware, &profile.method).is_err());
        assert!(completed(&rows, &profile, "ru", "auto"));
        assert!(!completed(&rows, &profile, "en", "auto"));
        assert!(!completed(&rows, &profile, "ru", "ru"));
        let mut revised = profile.clone();
        revised.revision = "new".into();
        assert!(!completed(&rows, &revised, "ru", "auto"));
        for field in ["benchmark_schema", "runs_ms", "rtf", "samples"] {
            let mut incomplete = record.clone();
            incomplete.as_object_mut().unwrap().remove(field);
            assert!(
                validate_resume(&[incomplete], "test", &profile.hardware, &profile.method).is_err()
            );
        }
    }

    #[test]
    fn invalid_cases_fail_before_downloads() {
        for args in [
            vec![],
            vec!["--language", "en"],
            vec!["tiny", "--language"],
            vec!["tiny", "--language", "auto"],
            vec!["tiny", "--language", "de"],
            vec!["tiny", "--language", "en", "--language", "ru"],
            vec!["tiny", "--unknown"],
            vec!["not-a-catalog-model"],
        ] {
            assert!(parse(&args).is_err(), "accepted {args:?}");
        }
    }

    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn streaming_models_share_english_but_parakeet_rejects_russian() {
        let options = parse(&[
            "--language",
            "en",
            "parakeet-streaming-en",
            "nemotron-streaming",
        ])
        .unwrap();
        for id in &options.ids {
            assert_eq!(options.fixture_language(id).unwrap(), "en");
        }
        assert!(parse(&["--language", "ru", "parakeet-streaming-en"]).is_err());
        let options = parse(&["parakeet-streaming-en"]).unwrap();
        assert_eq!(
            options.fixture_language("parakeet-streaming-en").unwrap(),
            "en"
        );
    }
}
