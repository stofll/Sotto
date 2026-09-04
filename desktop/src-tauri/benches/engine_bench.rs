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
//! Quick compile check (CI):
//!   cargo bench --no-run --package sotto

use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use sotto_lib::cloud_stt::{audio_to_wav_bytes, build_multipart_body};
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

/// A 5-second buffer at 16 kHz = 80 000 samples. This is the canonical
/// "moderately long recording" used by the cloud STT path.
const FIVE_SEC_SAMPLES: usize = 5 * 16_000; // 80_000

// ---------------------------------------------------------------------------
// Benchmark 1 – WAV encoding throughput
// ---------------------------------------------------------------------------

fn wav_encoding_throughput(c: &mut Criterion) {
    let audio = synth_audio(FIVE_SEC_SAMPLES);
    let num_bytes = audio.len() * 2 + 44; // PCM i16 + WAV header

    let mut group = c.benchmark_group("wav_encoding");
    group.throughput(Throughput::Bytes(num_bytes as u64));
    group.throughput(Throughput::Elements(audio.len() as u64));

    group.bench_function("5s_audio", |b| {
        b.iter(|| {
            let wav = audio_to_wav_bytes(criterion::black_box(&audio));
            criterion::black_box(wav);
        });
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Benchmark 2 – Multipart body construction with and without language
// ---------------------------------------------------------------------------

fn multipart_body_construction(c: &mut Criterion) {
    let audio = synth_audio(FIVE_SEC_SAMPLES);
    let wav_bytes = audio_to_wav_bytes(&audio);
    let model = "whisper-large-v3-turbo";

    let mut group = c.benchmark_group("multipart_body");

    group.bench_function("without_language", |b| {
        b.iter(|| {
            let (body, ct) = build_multipart_body(
                criterion::black_box(model),
                criterion::black_box(None),
                criterion::black_box(&wav_bytes),
            );
            criterion::black_box((body, ct));
        });
    });

    group.bench_function("with_language", |b| {
        b.iter(|| {
            let (body, ct) = build_multipart_body(
                criterion::black_box(model),
                criterion::black_box(Some("ru")),
                criterion::black_box(&wav_bytes),
            );
            criterion::black_box((body, ct));
        });
    });

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
        // Непустой: диспетчер клонирует результат вместе с id модели, и
        // пустой `None` занизил бы стоимость клона на одну строку.
        model_id: Some("ggml-large-v3-turbo".into()),
        inference_time_ms: 1234,
        audio_seconds: 12.0,
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

/// Inline equivalent of `mutex_recover::lock()` — recovers from a poisoned
/// mutex instead of panicking. We replicate the logic here (rather than
/// calling the private `sotto_lib::mutex_recover::lock`) so the
/// benchmark can compare overhead without modifying production visibility.
fn recover_lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

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
            let guard = recover_lock(&mutex);
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
    targets = wav_encoding_throughput,
              multipart_body_construction,
              inference_result_clone,
              mutex_recover_lock_overhead
);
criterion_main!(engine);
