//! Performance baseline for the speech-to-text Rust engine (Track B#5).
//!
//! These benchmarks measure throughput and overhead of core data-path
//! operations. They do NOT require a GPU, model files, or external
//! services — all data is synthetic.
//!
//! Run via:
//!   cargo bench --package sotto
//!   cargo bench --package sotto --bench engine_bench
//!
//! Compile/lint check (CI):
//!   cargo clippy --locked --all-targets -- -D warnings

use std::sync::{atomic::AtomicBool, Arc, Mutex};
use std::time::Duration;

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use sotto_lib::audio_samples::{convert_samples, i16_to_f32, mono_samples};
use sotto_lib::cloud_stt::multipart_upload;
use sotto_lib::formatter::format_transcription_with_config_value;
use sotto_lib::mutex_recover::lock;
use sotto_lib::whisper::InferenceResult;

#[path = "support/formatting.rs"]
mod formatting_cases;
#[path = "support/history.rs"]
mod history_cases;
// Use the same storage queries as the command, without exposing native history
// commands as library API merely for a benchmark.
#[allow(dead_code)]
#[path = "../src/history/query.rs"]
mod history_query;
// Criterion compiles cfg(test) modules without their #[test] function bodies.
#[allow(dead_code, unused_imports)]
#[path = "../src/vad.rs"]
mod vad;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Generate `num_samples` of mono 16 kHz f32 audio (a synthetic sine wave).
fn synth_audio(num_samples: usize) -> Vec<f32> {
    (0..num_samples)
        .map(|i| (i as f32 * 440.0 * 2.0 * std::f32::consts::PI / 16_000.0).sin())
        .collect()
}

// ---------------------------------------------------------------------------
// Production streaming WAV/multipart encoder, without network latency
// ---------------------------------------------------------------------------

fn streaming_upload_encoding(c: &mut Criterion) {
    let mut group = c.benchmark_group("streaming_upload");
    for seconds in [5, 60, 600] {
        let audio = Arc::new(synth_audio(seconds * 16_000));
        for (label, language) in [("auto", None), ("ru", Some("ru"))] {
            let size = multipart_upload(Arc::clone(&audio), "whisper-large-v3-turbo", language)
                .unwrap()
                .content_length;
            group.throughput(Throughput::Bytes(size as u64));
            group.bench_function(BenchmarkId::new(label, seconds), |b| {
                b.iter(|| {
                    let upload = multipart_upload(
                        Arc::clone(criterion::black_box(&audio)),
                        criterion::black_box("whisper-large-v3-turbo"),
                        criterion::black_box(language),
                    )
                    .unwrap();
                    // Drop each encoded chunk before requesting the next one,
                    // matching the encoder's bounded production buffering.
                    for chunk in upload {
                        criterion::black_box(chunk);
                    }
                });
            });
        }
    }
    group.finish();
}

// ---------------------------------------------------------------------------
// Live formatter: repeated settings, disabled formatting and cache invalidation
// ---------------------------------------------------------------------------

fn live_formatter(c: &mut Criterion) {
    let mut group = c.benchmark_group("live_formatter");
    group.sample_size(20);
    group.throughput(Throughput::Bytes(formatting_cases::TEXT.len() as u64));
    let defaults = formatting_cases::config(0, true, 0);
    group.bench_function("default_settings", |b| {
        b.iter(|| {
            criterion::black_box(format_transcription_with_config_value(
                criterion::black_box(&defaults),
                criterion::black_box(formatting_cases::TEXT),
            ));
        });
    });
    for terms in [0, 100, 1000] {
        let config = formatting_cases::config(terms, true, 50);
        // Dictionary initialization and the first pipeline build are separate
        // cold-call measurements; these cases exercise repeated dictations.
        criterion::black_box(format_transcription_with_config_value(
            &config,
            formatting_cases::TEXT,
        ));
        group.bench_function(BenchmarkId::new("same_settings", terms), |b| {
            b.iter(|| {
                criterion::black_box(format_transcription_with_config_value(
                    criterion::black_box(&config),
                    criterion::black_box(formatting_cases::TEXT),
                ));
            });
        });
    }
    let disabled = formatting_cases::config(1000, false, 50);
    group.bench_function("disabled_1000_terms_50_rules", |b| {
        b.iter(|| {
            criterion::black_box(format_transcription_with_config_value(
                criterion::black_box(&disabled),
                criterion::black_box(formatting_cases::TEXT),
            ));
        });
    });
    for relevant_change in [false, true] {
        let first = formatting_cases::config(100, true, 50);
        let mut second = first.clone();
        let label = if relevant_change {
            second["replacement_rules"][0]["replace"] = "итоговый результат".into();
            "formatter_setting_change"
        } else {
            second["theme"] = "dark".into();
            second["history_max_entries"] = 10000.into();
            "unrelated_setting_change"
        };
        let settings = [first, second];
        let mut index = 0;
        group.bench_function(label, |b| {
            b.iter(|| {
                index ^= 1;
                criterion::black_box(format_transcription_with_config_value(
                    criterion::black_box(&settings[index]),
                    criterion::black_box(formatting_cases::TEXT),
                ));
            });
        });
    }
    group.finish();
}

// ---------------------------------------------------------------------------
// History: bounded result serialization with larger retained datasets
// ---------------------------------------------------------------------------

fn history_page(c: &mut Criterion) {
    use history_query::{list_history_page_from, HistoryQuery, RetentionPolicy};

    let mut group = c.benchmark_group("history_page");
    group.sample_size(20);
    for rows in [1000, 10000, 100000] {
        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        history_cases::populate(&mut conn, rows);
        let policy = RetentionPolicy {
            max_age_seconds: 0,
            max_entries: 0,
        };
        for (label, query) in [
            ("first", HistoryQuery::default()),
            (
                "last",
                HistoryQuery {
                    page: (rows as u64 - 1) / 50,
                    ..HistoryQuery::default()
                },
            ),
            (
                "unicode_search",
                HistoryQuery {
                    query: "якорь".into(),
                    ..HistoryQuery::default()
                },
            ),
        ] {
            let result = list_history_page_from(&conn, policy, &query).unwrap();
            group.throughput(Throughput::Elements(result.entries.len() as u64));
            group.bench_function(BenchmarkId::new(label, rows), |b| {
                b.iter(|| {
                    let result = list_history_page_from(
                        criterion::black_box(&conn),
                        policy,
                        criterion::black_box(&query),
                    )
                    .unwrap();
                    criterion::black_box(serde_json::to_vec(&result).unwrap());
                });
            });
        }
    }
    group.finish();
}

// ---------------------------------------------------------------------------
// Capture scratch buffers and cancellable VAD preparation
// ---------------------------------------------------------------------------

fn capture_sample_conversion(c: &mut Criterion) {
    let floats = synth_audio(960);
    let integers: Vec<i16> = floats
        .iter()
        .map(|sample| (sample * 32767.0) as i16)
        .collect();
    let mut converted = Vec::new();
    let mut mono = Vec::new();
    convert_samples(&integers, &mut converted, |&sample| i16_to_f32(sample));
    criterion::black_box(mono_samples(&converted, 2, &mut mono));
    let mut group = c.benchmark_group("capture_samples");
    group.throughput(Throughput::Elements(floats.len() as u64));
    group.bench_function("i16_stereo_960", |b| {
        b.iter(|| {
            convert_samples(criterion::black_box(&integers), &mut converted, |&sample| {
                i16_to_f32(sample)
            });
            criterion::black_box(mono_samples(&converted, 2, &mut mono));
        });
    });
    group.bench_function("f32_stereo_960", |b| {
        b.iter(|| {
            criterion::black_box(mono_samples(criterion::black_box(&floats), 2, &mut mono));
        });
    });
    group.finish();
}

fn vad_preparation(c: &mut Criterion) {
    let config = serde_json::json!({"trim_silence": true});
    let active = Arc::new(AtomicBool::new(false));
    let mut group = c.benchmark_group("vad_prepare");
    group.sample_size(10);
    for seconds in [60, 600] {
        let audio = Arc::new(synth_audio(seconds * 16_000));
        group.throughput(Throughput::Elements(audio.len() as u64));
        group.bench_function(BenchmarkId::from_parameter(seconds), |b| {
            b.iter(|| {
                criterion::black_box(vad::prepare_dictation_unless(
                    Some(criterion::black_box(&config)),
                    Arc::clone(&audio),
                    Arc::clone(&active),
                ));
            });
        });
    }
    group.finish();
    // This exercises the early return, so audio throughput would be misleading.
    let audio = Arc::new(synth_audio(600 * 16_000));
    let cancelled = Arc::new(AtomicBool::new(true));
    c.bench_function("vad_cancelled_before_prepare", |b| {
        b.iter(|| {
            criterion::black_box(vad::prepare_dictation_unless(
                Some(criterion::black_box(&config)),
                Arc::clone(&audio),
                Arc::clone(&cancelled),
            ));
        });
    });
}

// ---------------------------------------------------------------------------
// Benchmark 3 – InferenceResult clone overhead
// ---------------------------------------------------------------------------

fn inference_result_clone(c: &mut Criterion) {
    let result = InferenceResult {
        session_id: 42,
        text: "Внимание! Шёпот — это тестовая система. Проверяем производительность клонирования InferenceResult в диспетчере. Этот текст достаточно длинный, чтобы клонирование строки доминировало над простым копированием полей. Если диспетчер клонирует этот результат многократно, мы хотим это заметить.".into(),
        language: Some("ru".into()),
        // Non-empty: the dispatcher clones the result along with the model id,
        // and an empty `None` would understate the clone cost by one string.
        model_id: Some("ggml-large-v3-turbo".into()),
        stt_service: None,
        inference_time_ms: 1234,
        audio_seconds: 12.0,
        speech_seconds: None,
    };

    c.bench_function("inference_result_clone", |b| {
        b.iter(|| {
            let cloned = criterion::black_box(&result).clone();
            criterion::black_box(cloned);
        });
    });
}

// ---------------------------------------------------------------------------
// Benchmark 4 – Mutex recovery lock overhead on a healthy mutex
// ---------------------------------------------------------------------------

fn mutex_recover_lock_overhead(c: &mut Criterion) {
    let mutex: Mutex<u64> = Mutex::new(0u64);

    let mut group = c.benchmark_group("mutex_lock");

    group.bench_function("std_mutex_lock", |b| {
        b.iter(|| {
            let guard = mutex.lock().unwrap();
            criterion::black_box(*guard);
        });
    });

    group.bench_function("mutex_recover_lock", |b| {
        b.iter(|| {
            let guard = lock(&mutex);
            criterion::black_box(*guard);
        });
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Criterion harness — one group so we have a single `cargo bench` target
// ---------------------------------------------------------------------------

criterion_group!(
    name = engine;
    config = Criterion::default()
        .warm_up_time(Duration::from_millis(500))
        .measurement_time(Duration::from_secs(2))
        .sample_size(100);
    targets = streaming_upload_encoding,
              live_formatter,
              history_page,
              capture_sample_conversion,
              vad_preparation,
              inference_result_clone,
              mutex_recover_lock_overhead
);
criterion_main!(engine);
