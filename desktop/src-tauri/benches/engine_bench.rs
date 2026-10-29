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

use std::sync::{Arc, Mutex};
use std::time::Duration;

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use sotto_lib::cloud_stt::multipart_upload;
use sotto_lib::mutex_recover::lock;
use sotto_lib::whisper::InferenceResult;

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
              inference_result_clone,
              mutex_recover_lock_overhead
);
criterion_main!(engine);
