//! Run one real dictation through one or more LLM models and report how much
//! of it survived.
//!
//! Not part of the app or the test suite — `examples/` is only built when asked
//! for by name. It exists because "does this model keep the author's words"
//! cannot be answered by a unit test: it needs the real provider, the real
//! system prompt, and a real transcript.
//!
//!   cargo run --example llm_fidelity_probe -- <text-file> [model ...]
//!
//! Provider, base URL, key reference and system prompt come from the installed
//! app's `config.json`, so the probe measures the same pipeline the user runs.
//! With no model arguments it uses the configured one. The API key is read
//! through `secret_store` and never printed.

use std::time::Instant;

use whisper_desktop_lib::ai::fidelity::{kept_word_ratio, word_count};
use whisper_desktop_lib::ai::{ai_process_text_with_status, step::AiConfig};
use whisper_desktop_lib::secret_store;

/// Print the app's own `log::warn!` lines to stderr. The fidelity guard reports
/// the exact share of words it saw there, which is the number this probe is
/// about — better to surface the real log line than to recompute it here and
/// risk the two disagreeing.
struct StderrLogger;

impl log::Log for StderrLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Info
    }
    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            eprintln!("   [{}] {}", record.level(), record.args());
        }
    }
    fn flush(&self) {}
}

fn config_path() -> std::path::PathBuf {
    let appdata = std::env::var("APPDATA").expect("APPDATA is set on Windows");
    std::path::Path::new(&appdata)
        .join("com.sotto.app")
        .join("config.json")
}

fn main() {
    // `set_logger` with a `'static` reference rather than `set_boxed_logger`:
    // the crate is built without log's `std` feature, which is what gates the
    // boxed variant.
    static LOGGER: StderrLogger = StderrLogger;
    let _ = log::set_logger(&LOGGER);
    log::set_max_level(log::LevelFilter::Info);

    let mut args = std::env::args().skip(1);
    let text_path = args
        .next()
        .expect("usage: llm_fidelity_probe <text-file|--list> [model ...]");
    let models: Vec<String> = args.collect();

    let raw = std::fs::read_to_string(config_path()).expect("read config.json");
    let root: serde_json::Value = serde_json::from_str(&raw).expect("parse config.json");
    let ai_value = root.get("ai_processing").cloned().unwrap_or_default();
    let base = AiConfig::from_ai_processing(&ai_value);

    let models = if models.is_empty() {
        vec![base.model.clone()]
    } else {
        models
    };

    let key = secret_store::get_key(&base.api_key_ref)
        .expect("read the API key")
        .expect("no API key stored for this profile");

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    // `--list` answers "which models does this key actually reach", which is
    // the first thing you need before comparing any of them.
    if text_path == "--list" {
        let url = format!("{}/models", base.base_url.clone().unwrap_or_default());
        let body = runtime.block_on(async {
            reqwest::Client::new()
                .get(&url)
                .bearer_auth(&key)
                .send()
                .await
                .expect("GET /models")
                .text()
                .await
                .expect("read /models body")
        });
        let parsed: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
        match parsed.get("data").and_then(|d| d.as_array()) {
            Some(items) => {
                for item in items {
                    println!("{}", item.get("id").and_then(|v| v.as_str()).unwrap_or("?"));
                }
            }
            None => println!("{body}"),
        }
        return;
    }

    let text = std::fs::read_to_string(&text_path).expect("read the dictation file");
    let text = text.trim();

    println!(
        "провайдер {} @ {}\nвход: {} симв., {} слов\n",
        base.provider,
        base.base_url.clone().unwrap_or_else(|| "-".to_string()),
        text.chars().count(),
        word_count(text),
    );

    for model in &models {
        let mut cfg = base.clone();
        cfg.model = model.clone();
        cfg.pipeline_mode = "hybrid".to_string();
        // The duration gate is about live recordings; a probe has no audio.
        cfg.audio_duration_seconds = None;
        cfg.llm_min_duration_seconds = 0.0;
        // Generous: we are measuring fidelity, not latency.
        cfg.llm_timeout_seconds = 90;

        let started = Instant::now();
        let outcome = runtime.block_on(ai_process_text_with_status(text, &cfg, Some(&key)));
        let status = &outcome.status;

        println!("── {model}");
        // On a fallback `outcome.text` is the untouched dictation, so measuring
        // it would report a flattering 100%. `output_length` keeps the size of
        // what the model actually sent in both branches.
        let model_chars = status
            .output_length
            .unwrap_or_else(|| outcome.text.chars().count());
        let char_share = model_chars as f64 / text.chars().count() as f64 * 100.0;
        println!("   модель вернула: {model_chars} симв. ({char_share:.0}% от входа)");
        if status.used {
            let ratio = kept_word_ratio(text, &outcome.text).unwrap_or(f64::NAN);
            println!(
                "   ПРИНЯТО:        {} слов, {:.0}% слов входа",
                word_count(&outcome.text),
                ratio * 100.0,
            );
        } else {
            println!(
                "   ОТБРОШЕНО:      {} / {}",
                status.error_type.clone().unwrap_or_default(),
                status.skipped_reason,
            );
        }
        if let Some(error) = &status.provider_error {
            println!("   ошибка:    {error}");
        }
        if let Some(snippet) = &status.response_snippet {
            println!("   ответ:     {snippet}");
        }
        // Каждый ответ рядом с входом: цифра говорит, сколько слов пропало,
        // а какие именно — видно только глазами в самом тексте.
        if status.used {
            let out_path = std::path::Path::new(&text_path)
                .with_extension(format!("{}.out.txt", model.replace(['/', ':'], "_")));
            std::fs::write(&out_path, &outcome.text).expect("write the answer");
            println!("   сохранено:      {}", out_path.display());
        }
        if let Some(usage) = &status.usage {
            println!(
                "   токены:    вход {} / выход {} / всего {}",
                usage.input_tokens, usage.output_tokens, usage.total_tokens
            );
        }
        println!("   время:     {:.1} с\n", started.elapsed().as_secs_f64());
    }
}
