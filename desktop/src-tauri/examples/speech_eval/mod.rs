//! Dataset runs alongside the public fixture benchmark. No app, history or network.
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sotto_lib::{formatter, model, model_download, model_performance, sherpa, whisper};
use std::{collections::HashMap, error::Error, fs, io::Write, path::Path, time::Instant};

type Decode = Box<dyn FnMut(&[f32], &str) -> Result<String, String>>;

fn options(args: impl Iterator<Item = String>) -> Result<HashMap<String, String>, String> {
    let mut args = args;
    let mut out = HashMap::new();
    while let Some(key) = args.next() {
        if !matches!(
            key.as_str(),
            "--mode"
                | "--dataset"
                | "--input"
                | "--split"
                | "--source"
                | "--language"
                | "--model"
                | "--models-dir"
                | "--output"
                | "--config"
                | "--limit"
                | "--hints"
                | "--fragment-seconds"
                | "--beam-size"
        ) {
            return Err(format!("Unknown evaluation option: {key}"));
        }
        let value = args
            .next()
            .filter(|v| !v.starts_with("--"))
            .ok_or("Missing option value")?;
        if out.insert(key, value).is_some() {
            return Err("Duplicate evaluation option".into());
        }
    }
    for key in ["--mode", "--dataset", "--output", "--config", "--split"] {
        if !out.contains_key(key) {
            return Err(format!("Required: {key}"));
        }
    }
    if !matches!(out["--mode"].as_str(), "stt" | "format" | "regressions")
        || !matches!(out["--split"].as_str(), "development" | "holdout")
    {
        return Err(
            "Mode must be stt/format/regressions; split must be development/holdout".into(),
        );
    }
    if out["--mode"] == "stt" && (!out.contains_key("--model") || !out.contains_key("--models-dir"))
    {
        return Err("STT requires --model and --models-dir (verified, no downloads)".into());
    }
    if out
        .get("--language")
        .is_some_and(|v| !matches!(v.as_str(), "ru" | "en"))
    {
        return Err("Language must be ru/en".into());
    }
    if let Some(limit) = out.get("--limit") {
        limit
            .parse::<usize>()
            .ok()
            .filter(|n| *n > 0)
            .ok_or("Limit must be positive")?;
    }
    if out
        .get("--hints")
        .is_some_and(|value| !matches!(value.as_str(), "empty" | "config"))
    {
        return Err("Hints must be empty/config".into());
    }
    if let Some(value) = out.get("--fragment-seconds") {
        if out["--mode"] != "stt"
            || out.get("--model").map(String::as_str) != Some("parakeet-ultra")
        {
            return Err("Fragment override is an STT probe for parakeet-ultra only".into());
        }
        let limit = model::fragment_seconds("parakeet-ultra");
        value
            .parse::<usize>()
            .ok()
            .filter(|seconds| (1..=limit).contains(seconds))
            .ok_or_else(|| format!("Fragment seconds must be in 1..={limit}"))?;
    }
    if let Some(value) = out.get("--beam-size") {
        let supported = out.get("--model").is_some_and(|id| {
            id == "parakeet-ultra"
                || model::catalog_model(id)
                    .is_some_and(|entry| entry.engine == model::ModelEngine::Whisper)
        });
        if out["--mode"] != "stt" || !supported {
            return Err("Beam override requires STT with Whisper or parakeet-ultra".into());
        }
        value
            .parse::<i32>()
            .ok()
            .filter(|size| (1..=8).contains(size))
            .ok_or("Beam size must be in 1..=8")?;
    }
    Ok(out)
}

fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn safe_config(config: Value) -> Result<Value, Box<dyn Error>> {
    let object = config
        .as_object()
        .ok_or("Formatter config must be an object")?;
    if object.keys().any(|key| {
        !matches!(
            key.as_str(),
            "language" | "text_formatting" | "replacement_rules" | "replacements_paused"
        )
    }) {
        return Err(
            "Use a formatter-only config; unknown root fields could contain credentials".into(),
        );
    }
    let parsed: formatter::FormatterConfig = serde_json::from_value(config)?;
    Ok(serde_json::to_value(parsed)?)
}

fn model_directory(path: &Path) -> Result<std::path::PathBuf, std::io::Error> {
    // Qwen's native tokenizer appends forward-slash filenames, which Windows
    // verbatim paths reject. Canonicalize only for the storage identity check.
    std::path::absolute(path)
}

pub(super) fn code_revision() -> Result<Value, Box<dyn Error>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let head = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&root)
        .output()?;
    if !head.status.success() {
        return Err("Cannot identify code revision".into());
    }
    let mut files = Vec::new();
    fn visit(path: &Path, files: &mut Vec<std::path::PathBuf>) -> std::io::Result<()> {
        for entry in fs::read_dir(path)? {
            let path = entry?.path();
            if path.is_dir() {
                visit(&path, files)?;
            } else {
                files.push(path);
            }
        }
        Ok(())
    }
    visit(&root.join("desktop/src-tauri/src"), &mut files)?;
    visit(&root.join("desktop/src-tauri/examples"), &mut files)?;
    visit(&root.join("desktop/src-tauri/build_support"), &mut files)?;
    files.extend([
        root.join("desktop/src-tauri/Cargo.lock"),
        root.join("desktop/src-tauri/Cargo.toml"),
        root.join("desktop/src-tauri/build.rs"),
        root.join("scripts/sherpa-runtime.lock"),
        root.join("scripts/ggml-baseline.cmake"),
        root.join(".cargo/config.toml"),
        root.join("rust-toolchain.toml"),
    ]);
    files.sort();
    let mut hash = Sha256::new();
    for path in files {
        hash.update(path.strip_prefix(&root)?.to_string_lossy().as_bytes());
        hash.update(fs::read(path)?);
    }
    Ok(
        json!({"git_head": String::from_utf8(head.stdout)?.trim(), "native_source_sha256": format!("{:x}", hash.finalize()), "executable_sha256":sha(&fs::read(std::env::current_exe()?)?), "cargo_lock_sha256": sha(&fs::read(root.join("desktop/src-tauri/Cargo.lock"))?)}),
    )
}

fn vocabulary_prompt(
    args: &HashMap<String, String>,
    config: &Value,
) -> Result<Option<String>, Box<dyn Error>> {
    if args["--mode"] != "stt" || args.get("--hints").is_none_or(|value| value == "empty") {
        return Ok(None);
    }
    let entry = model::catalog_model(&args["--model"]).ok_or("Unknown catalog model")?;
    if !matches!(
        entry.engine,
        model::ModelEngine::Whisper | model::ModelEngine::SherpaQwen3Asr
    ) {
        return Ok(None);
    }
    let cfg: formatter::FormatterConfig = serde_json::from_value(config.clone())?;
    Ok(cfg.text_formatting.recognition_prompt())
}

async fn open_model(
    id: &str,
    threads: i32,
    prompt: Option<String>,
    fragment_override: Option<usize>,
    beam_size: Option<i32>,
) -> Result<Decode, Box<dyn Error>> {
    let entry = model::catalog_model(id).ok_or("Unknown catalog model")?;
    if entry.engine == model::ModelEngine::Whisper {
        let spec = model_download::DownloadSpec::from_manifest(id)?;
        model_download::verify_file(&model::model_path(id)?, &spec).await?;
    } else {
        model::verify_bundle_files(id)?;
    }
    match model::model_load_spec(id, false)? {
        model::ModelLoadSpec::Whisper { path, .. } => {
            let ctx = whisper_rs::WhisperContext::new_with_params(
                path.to_str().ok_or("Invalid path")?,
                whisper_rs::WhisperContextParameters {
                    use_gpu: false,
                    ..Default::default()
                },
            )?;
            let mut state = ctx.create_state()?;
            let multilingual = ctx.is_multilingual();
            Ok(Box::new(move |samples, language| {
                let strategy = match beam_size {
                    Some(beam_size) => whisper_rs::SamplingStrategy::BeamSearch {
                        beam_size,
                        patience: -1.0,
                    },
                    None => whisper_rs::SamplingStrategy::Greedy { best_of: 1 },
                };
                let params = whisper::decode_params(
                    strategy,
                    Some(language),
                    multilingual,
                    threads,
                    prompt.as_deref(),
                );
                state.full(params, samples).map_err(|e| e.to_string())?;
                let count = state.full_n_segments().map_err(|e| e.to_string())?;
                let mut text = String::new();
                for i in 0..count {
                    let segment = state.full_get_segment_text(i).map_err(|e| e.to_string())?;
                    whisper::push_segment(&mut text, &segment);
                }
                Ok(text)
            }))
        }
        model::ModelLoadSpec::Sherpa { engine, files } => {
            let mut recognizer = open_sherpa(engine, &files, threads, beam_size)?;
            let fragment_seconds = fragment_override.unwrap_or_else(|| model::fragment_seconds(id));
            Ok(Box::new(move |samples, language| {
                if engine == model::ModelEngine::SherpaNemoCtc {
                    recognizer.transcribe_gigaam(samples, || false)
                } else if recognizer.is_streaming() {
                    recognizer.transcribe_unless(16000, samples, || false)
                } else {
                    recognizer.transcribe_segmented(
                        samples,
                        fragment_seconds,
                        Some(language),
                        prompt.as_deref(),
                        || false,
                    )
                }
            }))
        }
    }
}

fn open_sherpa(
    engine: model::ModelEngine,
    files: &model::BundleFiles,
    threads: i32,
    beam_size: Option<i32>,
) -> Result<sherpa::SherpaRecognizer, String> {
    if let Some(size) = beam_size {
        #[cfg(any(windows, target_os = "macos"))]
        {
            use model::ArtifactRole as R;
            return Ok(sherpa::SherpaRecognizer::Offline(
                sherpa::OfflineRecognizer::transducer_with_beam(
                    files.path(R::Encoder)?,
                    files.path(R::Decoder)?,
                    files.path(R::Joiner)?,
                    files.path(R::Tokens)?,
                    threads,
                    Some(size),
                )?,
            ));
        }
        #[cfg(not(any(windows, target_os = "macos")))]
        {
            let _ = size;
            return Err("Sherpa beam probes require Windows or macOS".into());
        }
    }
    sherpa::SherpaRecognizer::open(engine, files, threads)
}

fn pcm(path: &Path, audio: &Value) -> Result<Vec<f32>, Box<dyn Error>> {
    let bytes = fs::read(path)?;
    if sha(&bytes) != audio["sha256"].as_str().ok_or("Missing audio hash")? {
        return Err("Audio checksum mismatch".into());
    }
    if bytes.get(..4) != Some(b"RIFF") || bytes.get(8..12) != Some(b"WAVE") {
        return Err("Not a WAV".into());
    }
    let mut at = 12;
    let mut format_ok = false;
    let mut samples = None;
    while at + 8 <= bytes.len() {
        let size = u32::from_le_bytes(bytes[at + 4..at + 8].try_into()?) as usize;
        let end = (at + 8)
            .checked_add(size)
            .filter(|end| *end <= bytes.len())
            .ok_or("Truncated WAV")?;
        let chunk = &bytes[at + 8..end];
        match &bytes[at..at + 4] {
            b"fmt " => {
                format_ok = chunk.len() >= 16
                    && chunk[..4] == [1, 0, 1, 0]
                    && chunk[4..8] == 16000u32.to_le_bytes()
                    && chunk[14..16] == [16, 0];
            }
            b"data" => {
                if !size.is_multiple_of(2) {
                    return Err("Odd PCM payload".into());
                }
                samples = Some(
                    chunk
                        .chunks_exact(2)
                        .map(|b| f32::from(i16::from_le_bytes([b[0], b[1]])) / 32768.0)
                        .collect::<Vec<_>>(),
                );
            }
            _ => {}
        }
        at = end + (size & 1);
    }
    let samples = samples
        .filter(|_| format_ok)
        .ok_or("Requires PCM16 mono 16kHz WAV")?;
    if (samples.len() as f64 / 16000.0
        - audio["duration_seconds"]
            .as_f64()
            .ok_or("Missing duration")?)
    .abs()
        > 1e-6
    {
        return Err("Duration mismatch".into());
    }
    Ok(samples)
}

pub async fn run(args: impl Iterator<Item = String>) -> Result<(), Box<dyn Error>> {
    if cfg!(debug_assertions) {
        return Err("Evaluation timings require --release".into());
    }
    let args = options(args)?;
    let dataset = Path::new(&args["--dataset"]).canonicalize()?;
    let output = Path::new(&args["--output"]);
    let output_parent = output
        .parent()
        .ok_or("Output needs a parent directory")?
        .canonicalize()?;
    if output_parent.starts_with(&dataset) {
        return Err("Results must be outside the frozen dataset".into());
    }
    std::env::set_var("SOTTO_DATA_DIR", output_parent.join("isolated-data"));
    if let Some(path) = args.get("--models-dir") {
        let directory = model_directory(Path::new(path))?;
        let resolved = directory.canonicalize()?;
        std::env::set_var("SOTTO_MODELS_DIR", &directory);
        if model::models_dir()?.canonicalize()? != resolved {
            return Err(
                "Portable model storage overrides --models-dir; use a separate runner directory"
                    .into(),
            );
        }
    }
    let config = safe_config(serde_json::from_slice(&fs::read(&args["--config"])?)?)?;
    let prompt = vocabulary_prompt(&args, &config)?;
    let input = args
        .get("--input")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| dataset.join("manifest.jsonl"));
    let bytes = fs::read(&input)?;
    let regressions = args["--mode"] == "regressions";
    let rows: Vec<Value> = if regressions {
        serde_json::from_slice(&bytes)?
    } else {
        std::str::from_utf8(&bytes)?
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(serde_json::from_str)
            .collect::<Result<_, _>>()?
    };
    let rows: Vec<_> = rows
        .into_iter()
        .filter(|r| {
            regressions
                || (r["split"] == args["--split"]
                    && args.get("--source").is_none_or(|s| r["source"] == *s)
                    && args.get("--language").is_none_or(|l| r["language"] == *l)
                    && (args["--mode"] != "stt" || !r["audio"].is_null()))
        })
        .take(
            args.get("--limit")
                .map(|n| n.parse().expect("validated"))
                .unwrap_or(usize::MAX),
        )
        .collect();
    if rows.is_empty() {
        return Err("No selected records".into());
    }
    let mut ids = std::collections::HashSet::new();
    for row in &rows {
        if !ids.insert(row["id"].as_str().ok_or("Missing record ID")?) {
            return Err("Duplicate selected record ID".into());
        }
    }
    if let Some(id) = args.get("--model") {
        for r in &rows {
            if !model::model_supports_language(
                id,
                r["language"].as_str().ok_or("Unknown language")?,
            ) {
                return Err("Model does not support a selected language".into());
            }
        }
    }
    let threads = std::thread::available_parallelism()
        .map(|n| n.get().min(8))
        .unwrap_or(4) as i32;
    let mut system = sysinfo::System::new();
    system.refresh_cpu_all();
    system.refresh_memory();
    let fragment_override = args
        .get("--fragment-seconds")
        .map(|value| value.parse::<usize>())
        .transpose()?;
    let metadata = json!({
        "type": "run", "schema": 1, "code": code_revision()?,
        "input_sha256": sha(&bytes), "config": config, "options": args,
        "compute": "cpu", "threads": threads, "os": std::env::consts::OS,
        "cpu": system.cpus().first().map(|c| c.brand()), "memory_bytes": system.total_memory(),
        "records": rows.len(),
        "fragment_seconds_override": fragment_override,
        "beam_size_override": args.get("--beam-size").map(|value| value.parse::<i32>().expect("validated")),
        "created_unix": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs(),
        "model_profile": args.get("--model").map(|id| model_performance::Profile::new(id, "cpu")),
        "vocabulary_prompt": prompt, "hints_mode": args.get("--hints").map(String::as_str).unwrap_or("empty"),
        "hints_supported": args.get("--model").and_then(|id| model::catalog_model(id)).map(|entry|
            matches!(entry.engine, model::ModelEngine::Whisper | model::ModelEngine::SherpaQwen3Asr))
    });
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(output)?;
    writeln!(file, "{metadata}")?;
    file.sync_data()?;
    let started = Instant::now();
    let mut decode = if args["--mode"] == "stt" {
        Some(
            open_model(
                &args["--model"],
                threads,
                prompt,
                fragment_override,
                args.get("--beam-size")
                    .map(|value| value.parse().expect("validated")),
            )
            .await?,
        )
    } else {
        None
    };
    writeln!(
        file,
        "{}",
        json!({"type":"load","load_ms":started.elapsed().as_secs_f64()*1000.0,"includes_weight_verification":true})
    )?;
    for (index, row) in rows.iter().enumerate() {
        let mut cfg = config.clone();
        if regressions {
            cfg = row
                .get("config")
                .cloned()
                .unwrap_or_else(|| json!({"language":"ru"}));
        } else {
            cfg["language"] = row["language"].clone();
        }
        let input = if regressions {
            row["input"].as_str()
        } else {
            row["observed"]["raw_text"]
                .as_str()
                .or_else(|| row["reference"]["asr_text"].as_str())
        }
        .unwrap_or("");
        let (text, elapsed_ms, repeat) = if let Some(decode) = decode.as_mut() {
            let path = dataset
                .join(row["audio"]["path"].as_str().ok_or("Missing audio path")?)
                .canonicalize()?;
            if !path.starts_with(&dataset) {
                return Err("Audio escapes dataset".into());
            }
            let audio = pcm(&path, &row["audio"])?;
            // I/O and hashing are excluded from inference timing.
            let started = Instant::now();
            let text = decode(&audio, row["language"].as_str().ok_or("Unknown language")?)?;
            let ms = started.elapsed().as_secs_f64() * 1000.0;
            let repeat = if index == 0 {
                let start = Instant::now();
                let second = decode(&audio, row["language"].as_str().unwrap())?;
                Some(json!({"elapsed_ms":start.elapsed().as_secs_f64()*1000.0,"text":second}))
            } else {
                None
            };
            (text, ms, repeat)
        } else {
            let started = Instant::now();
            let text = formatter::format_transcription_with_config_value(&cfg, input);
            (text, started.elapsed().as_secs_f64() * 1000.0, None)
        };
        let result = json!({
            "type": "record", "id": row["id"], "source": row["source"],
            "language": cfg["language"], "split": row["split"],
            "audio_seconds": row["audio"]["duration_seconds"], "audio_sha256": row["audio"]["sha256"],
            "input": input, "text": text, "elapsed_ms": elapsed_ms,
            "first_after_load": index == 0, "repeat": repeat,
            "expected": row["transcription_expected"].as_str().or_else(|| row["expected"].as_str()),
            "preview": if regressions { Some(formatter::format_with_config_value(&cfg, input)) } else { None },
            "preview_expected": row["expected"]
        });
        writeln!(file, "{result}")?;
        file.sync_data()?;
        if index % 20 == 0 {
            eprintln!("evaluation: {}/{} records saved", index + 1, rows.len());
        }
    }
    writeln!(file, "{}", json!({"type":"complete","records":rows.len()}))?;
    file.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(windows)]
    #[test]
    fn native_model_paths_do_not_gain_a_verbatim_prefix() {
        let input = Path::new(env!("CARGO_MANIFEST_DIR"));
        let output = model_directory(input).unwrap();
        assert!(output.is_absolute());
        assert_eq!(
            output.canonicalize().unwrap(),
            input.canonicalize().unwrap()
        );
        assert!(!output.to_string_lossy().starts_with(r"\\?\"));
    }
    #[test]
    fn prompts_reuse_the_application_dictionary_and_only_reach_supported_models() {
        let cfg = safe_config(
            json!({"text_formatting":{"enabled":false,"custom_words":["  Cargo  ","cargo",""]}}),
        )
        .unwrap();
        let args = HashMap::from([
            ("--mode".to_string(), "stt".to_string()),
            ("--model".to_string(), "turbo".to_string()),
            ("--hints".to_string(), "config".to_string()),
        ]);
        assert_eq!(
            vocabulary_prompt(&args, &cfg).unwrap().as_deref(),
            Some("Cargo")
        );
        // Sherpa models are only in the catalog where the Sherpa runtime ships.
        let sherpa = cfg!(any(windows, target_os = "macos"));
        if sherpa {
            let mut qwen = args.clone();
            qwen.insert("--model".to_string(), "qwen3-asr-0.6b".to_string());
            assert_eq!(
                vocabulary_prompt(&qwen, &cfg).unwrap().as_deref(),
                Some("Cargo")
            );
        }
        let ultra = sherpa.then_some(("--model", "parakeet-ultra"));
        for (key, value) in [("--hints", "empty"), ("--mode", "format")]
            .into_iter()
            .chain(ultra)
        {
            let mut changed = args.clone();
            changed.insert(key.to_string(), value.to_string());
            assert!(vocabulary_prompt(&changed, &cfg).unwrap().is_none());
        }
    }
    #[test]
    fn formatter_config_cannot_export_provider_secrets_or_silently_fall_back() {
        assert!(safe_config(json!({"api_key":"synthetic-secret"})).is_err());
        assert!(safe_config(json!([])).is_err());
        assert!(safe_config(json!({"text_formatting":{"enabled":"yes"}})).is_err());
        let cfg = safe_config(json!({"language":"en","text_formatting":{"remove_fillers":false}}))
            .unwrap();
        assert_eq!(cfg["text_formatting"]["remove_fillers"], false);
    }
    #[test]
    fn rejects_ambiguous_runs_before_io() {
        let base = [
            "--mode",
            "format",
            "--dataset",
            "data",
            "--output",
            "out",
            "--config",
            "cfg",
            "--split",
            "development",
        ];
        assert!(options(base.into_iter().map(String::from)).is_ok());
        for tail in [
            vec!["--split", "holdout"],
            vec!["--limit", "0"],
            vec!["--language", "auto"],
            vec!["--model"],
            vec!["--hints", "unknown"],
        ] {
            assert!(options(base.into_iter().chain(tail).map(String::from)).is_err());
        }
        assert!(options(["--mode", "stt"].into_iter().map(String::from)).is_err());
    }
    #[test]
    fn decoder_probes_are_explicit_bounded_and_model_specific() {
        let base = [
            "--mode",
            "stt",
            "--dataset",
            "data",
            "--output",
            "out",
            "--config",
            "cfg",
            "--split",
            "development",
            "--model",
            "parakeet-ultra",
            "--models-dir",
            "models",
        ];
        for model in ["parakeet-ultra", "turbo"] {
            let mut args = base;
            args[11] = model;
            assert!(options(
                args.into_iter()
                    .chain(["--beam-size", "4"])
                    .map(str::to_owned)
            )
            .is_ok());
        }
        for value in ["0", "9", "-1", "NaN"] {
            assert!(options(
                base.into_iter()
                    .chain(["--beam-size", value])
                    .map(str::to_owned)
            )
            .is_err());
        }
        for (index, value) in [
            (1, "format"),
            (1, "regressions"),
            (11, "gigaam-multilingual"),
            (11, "unknown"),
        ] {
            let mut args = base;
            args[index] = value;
            assert!(options(
                args.into_iter()
                    .chain(["--beam-size", "4"])
                    .map(str::to_owned)
            )
            .is_err());
        }
        assert!(!options(base.into_iter().map(str::to_owned))
            .unwrap()
            .contains_key("--beam-size"));
    }

    // Fragment probes target Parakeet Ultra, a Sherpa model absent on Linux.
    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn ultra_accepts_an_explicit_shorter_fragment_limit() {
        let args = options(
            [
                "--mode",
                "stt",
                "--dataset",
                "dataset",
                "--output",
                "output",
                "--config",
                "config",
                "--split",
                "development",
                "--model",
                "parakeet-ultra",
                "--models-dir",
                "models",
                "--fragment-seconds",
                "30",
            ]
            .into_iter()
            .map(str::to_owned),
        )
        .unwrap();
        assert_eq!(args["--fragment-seconds"], "30");
    }

    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn fragment_probes_reject_invalid_limits_and_unsupported_modes_or_models() {
        let base = [
            "--mode",
            "stt",
            "--dataset",
            "dataset",
            "--output",
            "output",
            "--config",
            "config",
            "--split",
            "development",
            "--model",
            "parakeet-ultra",
            "--models-dir",
            "models",
        ];
        for limit in ["0", "121", "-1", "invalid"] {
            assert!(options(
                base.into_iter()
                    .chain(["--fragment-seconds", limit])
                    .map(str::to_owned)
            )
            .is_err());
        }
        for (index, value) in [
            (1, "format"),
            (1, "regressions"),
            (11, "gigaam-v3"),
            (11, "whisper-base"),
        ] {
            let mut changed = base;
            changed[index] = value;
            assert!(options(
                changed
                    .into_iter()
                    .chain(["--fragment-seconds", "30"])
                    .map(str::to_owned)
            )
            .is_err());
        }
        assert!(options(
            base.into_iter()
                .chain(["--fragment-seconds", "120"])
                .map(str::to_owned)
        )
        .is_ok());
        assert!(options(base.into_iter().map(str::to_owned)).is_ok());
    }
}
