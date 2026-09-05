//! WS 4a2 — cpal audio capture. Replaces `audio/recorder.py`.
//!
//! Architecture: a single `AudioRecorder` lives in `AppState` (symmetric
//! to the whisper engine). The cpal stream callback runs on a cpal-managed
//! real-time thread and does ONLY:
//!
//!   1. Convert samples to f32 (if not f32 already — see conversion
//!      helpers `i16_to_f32`/`u8_to_f32` and the inline
//!      I32/F64 branches in `AudioRecorder::start`)
//!   2. Mono mixdown if multi-channel
//!   3. Update the RMS level (atomic bit-cast f32 — EMA-smoothed)
//!   4. Resample 48 kHz → 16 kHz with a 3-tap moving-average pre-filter
//!      (anti-aliasing) + 3:1 decimation
//!   5. Append samples to `Arc<Mutex<Vec<f32>>>`
//!
//! No allocations in the F32 hot path; non-F32 paths allocate a `Vec<f32>`
//! per callback for the converted samples (callback frequency is ~10ms
//! so this is bounded pressure). No `app.emit`, no `println!` in the
//! callback. See `process_samples` for the contract.
//!
//! `stop()` follows the canonical drop-and-drain: take the `Stream` out
//! of the `Mutex<Option<Stream>>` and `drop` it (cpal joins the callback
//! thread synchronously). After that it is safe to lock the buffer and
//! take ownership of the recorded samples.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream, StreamConfig};

/// Target sample rate (Hz) and channel count for downstream consumers
/// (whisper ASR expects 16 kHz mono).
#[derive(Debug, Clone, Copy)]
pub struct AudioConfig {
    pub sample_rate_target: u32, // 16000
    pub channels_target: u16,    // 1 (mono)
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            sample_rate_target: 16000,
            channels_target: 1,
        }
    }
}

/// Recorder lifecycle. Mirrors the AppFsm in `state.rs` but is internal
/// to the audio module so the rest of the app does not depend on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecorderState {
    Idle,
    Recording,
    Stopped, // audio buffered, engine call pending
    Error,
}

// ============================================================================
// Sample format → f32 conversion
// ============================================================================
//
// Each `cpal::SampleFormat` variant (I16, U8, F32, I32, F64) maps to a
// deterministic f32 range. We mirror the ranges used by Python's
// `sounddevice`/NumPy defaults so existing tests pass.
//
/// Convert an I16 PCM sample to a normalized f32 in [-1.0, 1.0).
/// -32768 maps to -1.0; 32767 to ~0.99997; 0 to 0.0.
#[inline]
pub fn i16_to_f32(s: i16) -> f32 {
    s as f32 / 32768.0
}

/// Convert a U8 PCM sample to a normalized f32.
/// 0 maps to -1.0; 128 maps to ~0.0; 255 maps to ~0.969.
#[inline]
pub fn u8_to_f32(s: u8) -> f32 {
    (s as f32 - 128.0) / 128.0
}

/// Lightweight device metadata returned by `AudioRecorder::list_devices`.
/// Wraps `cpal::Device::name()` so the rest of the app doesn't depend on
/// the cpal type (and so we can include extra metadata later).
#[derive(Debug, Clone, serde::Serialize)]
pub struct DeviceInfo {
    pub name: String,
}

// ============================================================================
// SendStream — wrap `cpal::Stream` so it can be stored in `AppState`.
// ============================================================================
//
// cpal 0.15's `Stream` is intentionally `!Send + !Sync` (see the
// `NotSendSyncAcrossAllPlatforms` phantom in `cpal/src/platform/mod.rs`)
// — the design accommodates Android's AAudio API which requires the
// stream to be owned by a single thread. macOS / Linux / Windows hosts
// have no such restriction, but the type-level constraint is uniform.
//
// We DO need `AudioRecorder: Send + Sync` because:
//   * `tauri::State<AppState>` requires `AppState: Send + Sync + 'static`.
//   * `AppState` holds `recorder: Arc<AudioRecorder>`.
//   * `Arc<T>: Send + Sync` requires `T: Send + Sync`.
//
// The operations we perform on the Stream are: `.play()` (once, on
// start) and `drop` (on stop). Both are safe to invoke from any thread —
// cpal internally serializes via the host audio backend. We also never
// share the Stream reference across threads (it's always owned by the
// `Mutex<Option<SendStream>>` on this side, with the callback running
// on cpal's audio thread holding only its own Arc-clones of shared
// atomic state).
//
// `unsafe impl Send` is therefore sound for our usage. We do NOT add
// `unsafe impl Sync` — we don't need it (Mutex already synchronizes), and
// staying Sync-less on the newtype is the conservative default.
pub struct SendStream(cpal::Stream);
unsafe impl Send for SendStream {}
impl std::ops::Deref for SendStream {
    type Target = cpal::Stream;
    fn deref(&self) -> &cpal::Stream {
        &self.0
    }
}
impl std::ops::DerefMut for SendStream {
    fn deref_mut(&mut self) -> &mut cpal::Stream {
        &mut self.0
    }
}
impl Drop for SendStream {
    fn drop(&mut self) {
        // Just delegate — cpal's own Drop joins the audio thread.
    }
}

// ============================================================================
// Resampler — 3-tap moving-average pre-filter + 3:1 decimation.
// ============================================================================
//
// Used to downsample 48 kHz (a common device rate) to 16 kHz (whisper
// target). The 3-tap moving-average is a minimal anti-aliasing low-pass:
// its stopband rolls off by ~10·log(9) ≈ -9.5 dB at Nyquist/3, which is
// enough for speech ASR (no useful content above 4 kHz; whisper operates
// at 8 kHz Nyquist).
pub fn resample_3_to_1(input: &[f32]) -> Vec<f32> {
    if input.len() < 3 {
        return Vec::new();
    }
    let n = input.len() / 3;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let idx = i * 3;
        let avg = (input[idx] + input[idx + 1] + input[idx + 2]) / 3.0;
        out.push(avg);
    }
    out
}

/// Block-of-2 averaging resampler. Used for 32 kHz → 16 kHz downsample
/// when the device's native rate is 32 kHz (rare on macOS, possible on
/// some Linux ALSA devices or 2× oversampled USB mics).
///
/// Pattern mirrors `resample_3_to_1`: `n = input.len() / 2`, then
/// `output[i] = (input[2i] + input[2i+1]) / 2.0`. Returns `Vec::new()`
/// for `len() < 2` (matches the existing "len < R → empty" convention).
pub fn resample_2_to_1(input: &[f32]) -> Vec<f32> {
    if input.len() < 2 {
        return Vec::new();
    }
    let n = input.len() / 2;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let idx = i * 2;
        out.push((input[idx] + input[idx + 1]) / 2.0);
    }
    out
}

/// Block-of-6 averaging resampler. Used for 96 kHz → 16 kHz downsample
/// (high-end USB mics, Focusrite-style interfaces on macOS at 96 kHz).
///
/// Pattern mirrors `resample_3_to_1`: `n = input.len() / 6`, then
/// `output[i] = sum(input[6i..6i+6]) / 6.0`. Returns `Vec::new()` for
/// `len() < 6`.
pub fn resample_6_to_1(input: &[f32]) -> Vec<f32> {
    if input.len() < 6 {
        return Vec::new();
    }
    let n = input.len() / 6;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let idx = i * 6;
        let sum: f32 = input[idx..idx + 6].iter().sum();
        out.push(sum / 6.0);
    }
    out
}

// ============================================================================
// AudioRecorder
// ============================================================================
//
// Public API used by the Tauri command layer (`start_recording`,
// `stop_recording`, `get_audio_level`, `list_audio_devices`).
//
// Concurrency: every field behind a Mutex/Atomic; `audio_buffer` is wrapped
// in `Arc` so the cpal callback (which needs `'static + Send`) can hold a
// reference without keeping the AudioRecorder pinned in its borrow
// checker. `is_recording` is an AtomicBool with acquire/release ordering so
// the callback can early-exit promptly when `stop()` flips it.
pub struct AudioRecorder {
    state: Mutex<RecorderState>,
    /// Public-ish "do we currently have a live recording" flag. The cpal
    /// callback uses ITS OWN Arc<AtomicBool> (cb_is_recording) so it can
    /// early-exit without taking a Mutex; we mirror to this on start/stop
    /// so external callers (Tauri commands) can poll cheaply.
    is_recording: AtomicBool,
    audio_buffer: Arc<Mutex<Vec<f32>>>, // Arc — callback needs 'static + Send
    /// An audio tap for the live preview. The full recording accumulates in
    /// `audio_buffer` as before — this queue merely duplicates chunks along the
    /// way. Bounded and non-blocking: the preview is allowed to fall behind and
    /// lose a chunk, the recording is not.
    live_tap: Arc<Mutex<Option<std::sync::mpsc::SyncSender<Vec<f32>>>>>,
    /// RMS level EMA, atomic bit-cast f32. Wrapped in Arc so the callback
    /// can update the SAME bit-cast the public `level()` reads.
    level_ema_bits: Arc<AtomicU32>,
    stream: Mutex<Option<SendStream>>,
    config: AudioConfig,
}

impl AudioRecorder {
    /// Enough for five minutes of recording at 48 kHz. This is a hint to the
    /// allocator, not a limit: `Vec` grows on its own, and the device's real
    /// sample rate is learned by
    /// `start()`.
    const APPROX_CAPACITY: usize = 48_000 * 60 * 5;

    /// Create a new `AudioRecorder`. The default input device is queried
    /// lazily via `start()`, so a missing or broken device does not prevent
    /// `new()` from succeeding.
    ///
    /// This used to query the default device — precisely in order to refine the
    /// buffer capacity. The gain: `Vec` might avoid one reallocation. The cost:
    /// `default_input_device()` followed by `default_input_config()` go into
    /// WASAPI, and on a machine where the audio stack formally exists but is not
    /// operational, that query takes down the whole process —
    /// STATUS_ACCESS_VIOLATION with no stack and no message. That is how the
    /// Windows CI job (#51) crashed, where there is neither an audio device nor
    /// an interactive session: enumerating devices went through, querying the
    /// configuration did not.
    ///
    /// A recorder constructor is not the place to touch the native stack:
    /// recording may never happen, yet crashing already can.
    pub fn new(config: AudioConfig) -> Result<Self, String> {
        Ok(Self {
            state: Mutex::new(RecorderState::Idle),
            is_recording: AtomicBool::new(false),
            audio_buffer: Arc::new(Mutex::new(Vec::with_capacity(Self::APPROX_CAPACITY))),
            level_ema_bits: Arc::new(AtomicU32::new(0.0_f32.to_bits())),
            live_tap: Arc::new(Mutex::new(None)),
            stream: Mutex::new(None),
            config,
        })
    }

    /// Start the cpal input stream. Idempotent within a single recording
    /// session: calling `start()` while already recording returns an
    /// error. The stream is `play()`ed before this returns, so callbacks
    /// start firing immediately.
    pub fn start(&self, device_index: Option<usize>) -> Result<(), String> {
        self.start_selected(device_index.map(|i| i.to_string()).as_deref())
    }

    pub fn start_selected(&self, selection: Option<&str>) -> Result<(), String> {
        if self.is_recording.load(Ordering::Acquire) {
            return Err("already recording".into());
        }

        let host = cpal::default_host();
        let device = match selection {
            Some(value) => {
                let mut devices = host
                    .input_devices()
                    .map_err(|e| format!("input_devices: {e}"))?;
                if let Some(name) = value.strip_prefix("name:") {
                    devices.find(|device| device.name().ok().as_deref() == Some(name))
                } else if let Ok(index) = value.parse::<usize>() {
                    devices.nth(index)
                } else {
                    devices.find(|device| device.name().ok().as_deref() == Some(value))
                }.ok_or_else(|| format!("Selected microphone is disconnected: {value}. Select an available microphone in Settings."))?
            }
            None => host.default_input_device().ok_or_else(|| {
                "No default input device. Connect a microphone or select one in Settings."
                    .to_string()
            })?,
        };
        let supported = device
            .default_input_config()
            .map_err(|e| format!("default_input_config: {e}"))?;
        // cpal 0.15 wraps sample_rate in `cpal::SampleRate(pub u32)`.
        // We extract the inner u32 everywhere it's used (preallocated
        // buffer sizing, StreamConfig, process_samples).
        let sample_rate_u32 = supported.sample_rate().0;
        let channels = supported.channels();
        let sample_format = supported.sample_format();
        let stream_config = StreamConfig {
            channels,
            sample_rate: supported.sample_rate(),
            buffer_size: cpal::BufferSize::Default,
        };

        // Clear stale samples from a previous session so that `stop()`
        // either returns None (empty) or returns ONLY this session's audio.
        self.audio_buffer
            .lock()
            .expect("audio_buffer lock in start()")
            .clear();

        // Arc references for the callback closure. The callback MUST be
        // `'static + Send` for cpal's real-time thread. Each branch of
        // the SampleFormat match below MOVEs its own Arc clone into its
        // closure (a single closure owns the Arc, so multiple branches
        // each need their own clone — since `Arc` is not `Copy`).
        let target_rate = self.config.sample_rate_target;
        let channels_for_cb = channels;
        let sample_rate_for_cb = sample_rate_u32;

        // Error callback takes StreamError by value in cpal 0.15.
        let err_cb = |err: cpal::StreamError| {
            log::error!("cpal stream error: {err}");
        };

        // CALLBACK CLOSURE DESIGN (per glm-5.2 review):
        // `cpal::build_input_stream<T>` is generic over T — the runtime
        // SampleFormat is encoded in the `T` type parameter at compile
        // time. We dispatch by building a different closure for each
        // common sample format. The cpal stream is single-consumer per
        // branch (one callback instance per stream), so we do NOT need a
        // Mutex around the data callback. Each closure captures Arc-clones
        // of the shared state by `move`.
        //
        // For non-F32 sample types, the callback allocates a `Vec<f32>` to
        // hold the per-callback converted samples — this is a one-shot
        // allocation per ~10ms callback window and is GC-style pressure
        // that the audio thread can absorb. The F32 path is allocation-
        // free (move semantics on &[f32]).
        // We select ONE of the closures based on sample_format at start
        // time. The selected closure gets an Arc-clone of the live
        // `is_recording` and `level_ema_bits` flags. After `stream.play()`
        // we flip the shared `is_recording_cb` once, and the closure
        // observes the new value on its next invocation.
        //
        // cpal 0.15's `build_input_stream<T>` requires the sample
        // type `T` at compile time but selects the runtime sample
        // format from `T::FORMAT` — so each match arm uses a fresh
        // closure of type `FnMut(&[T], &InputCallbackInfo)` for that T.
        // Branches are mutually exclusive (only one is built), so each
        // arm is free to move its own clone of the Arcs.
        let is_recording_cb: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));

        // Each non-F32 arm below differs only in the sample type `T` and the
        // per-sample `T -> f32` conversion. This macro keeps those two knobs
        // visible while removing the ~13-line closure boilerplate that was
        // copy-pasted once per format. The F32 arm stays separate because it
        // is allocation-free: it forwards `&[f32]` straight to
        // `process_samples` with no per-callback `Vec<f32>`.
        macro_rules! build_converting_stream {
            ($sample:ty, $to_f32:expr) => {{
                let cb_is_recording = Arc::clone(&is_recording_cb);
                let cb_sinks = self.capture_sinks();
                let cb = move |data: &[$sample], _: &cpal::InputCallbackInfo| {
                    let converted: Vec<f32> = data.iter().map($to_f32).collect();
                    process_samples(
                        &converted,
                        channels_for_cb,
                        sample_rate_for_cb,
                        target_rate,
                        &cb_is_recording,
                        &cb_sinks,
                    );
                };
                device.build_input_stream::<$sample, _, _>(&stream_config, cb, err_cb, None)
            }};
        }

        let build_result: Result<Stream, cpal::BuildStreamError> = match sample_format {
            SampleFormat::F32 => {
                let cb_is_recording = Arc::clone(&is_recording_cb);
                let cb_sinks = self.capture_sinks();
                let cb = move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    process_samples(
                        data,
                        channels_for_cb,
                        sample_rate_for_cb,
                        target_rate,
                        &cb_is_recording,
                        &cb_sinks,
                    );
                };
                device.build_input_stream::<f32, _, _>(&stream_config, cb, err_cb, None)
            }
            SampleFormat::I16 => build_converting_stream!(i16, |&s| i16_to_f32(s)),
            SampleFormat::I32 => build_converting_stream!(i32, |&s| s as f32 / 2147483648.0),
            SampleFormat::U8 => build_converting_stream!(u8, |&s| u8_to_f32(s)),
            SampleFormat::F64 => build_converting_stream!(f64, |&s| s as f32),
            // Other SampleFormat variants (I8, I64, U16, U32, U64) are
            // exotic — record no audio but build a stream so the device
            // doesn't enter an error state. The fallback attempts F32;
            // mismatched-format streams on cpal typically fail to build
            // (we surface that error) or record garbage (logged).
            _ => {
                log::error!(
                    "unsupported sample format: {sample_format:?}; attempting F32 fallback"
                );
                let cb = move |_data: &[f32], _: &cpal::InputCallbackInfo| {};
                device.build_input_stream::<f32, _, _>(&stream_config, cb, err_cb, None)
            }
        };

        let stream = build_result.map_err(|e| format!("build_input_stream: {e}"))?;
        stream.play().map_err(|e| format!("stream.play: {e}"))?;

        // Wrap in SendStream so the Mutex<Option<SendStream>> can be held
        // in AppState (which requires Send + Sync). See SendStream doc.
        let send_stream = SendStream(stream);

        // Flip the local "is_recording" now that the stream is live so
        // the callback (which captured its own copy via Arc) starts
        // gating and writing audio. Using Release ordering pairs with
        // Acquire on the callback side.
        is_recording_cb.store(true, Ordering::Release);
        self.is_recording.store(true, Ordering::Release);

        *self.stream.lock().expect("stream lock in start()") = Some(send_stream);
        *self.state.lock().expect("state lock in start()") = RecorderState::Recording;
        Ok(())
    }

    /// Canonical drop-and-drain:
    ///   1. Flip `is_recording` so the callback early-exits next invocation.
    ///   2. Take the `Stream` out of the Option and drop it — cpal joins
    ///      the callback thread synchronously, so this returns when the
    ///      callback can no longer be running.
    ///   3. Now safely lock the buffer and take ownership of the samples.
    ///      Wrap in `Arc` so callers (engine command sender) can move
    ///      the audio to a worker thread without copying.
    pub fn stop(&self) -> Result<Option<Arc<Vec<f32>>>, String> {
        // 1. Flip is_recording so the next callback early-exits.
        self.is_recording.store(false, Ordering::Release);
        // 2. Take Stream out of Option → drop (cpal joins callback).
        let stream_opt = self.stream.lock().expect("stream lock in stop()").take();
        if let Some(stream) = stream_opt {
            drop(stream);
        }
        // 3. Lock the buffer (safe now — no callback is running) and
        //    take the samples.
        let buf_arc = Arc::clone(&self.audio_buffer);
        let mut buf = buf_arc.lock().expect("audio_buffer lock in stop()");
        if buf.is_empty() {
            *self.state.lock().expect("state lock in stop()/Idle") = RecorderState::Idle;
            return Ok(None);
        }
        let taken = std::mem::take(&mut *buf);
        *self.state.lock().expect("state lock in stop()/Stopped") = RecorderState::Stopped;
        Ok(Some(Arc::new(taken)))
    }

    /// A snapshot of the sinks for the callback. All three are `Arc`s, so the
    /// callback writes into exactly what the public methods read.
    fn capture_sinks(&self) -> CaptureSinks {
        CaptureSinks {
            buffer: Arc::clone(&self.audio_buffer),
            level_bits: Arc::clone(&self.level_ema_bits),
            live_tap: Arc::clone(&self.live_tap),
        }
    }

    /// Attach the live audio tap and obtain a receiver of chunks.
    ///
    /// The capacity is given in chunks rather than seconds: the callback hands
    /// over one chunk per call, and a queue of a few dozen chunks is on the
    /// order of a second of audio at a typical cpal buffer size.
    pub fn attach_live_tap(&self, capacity_chunks: usize) -> std::sync::mpsc::Receiver<Vec<f32>> {
        let (tx, rx) = std::sync::mpsc::sync_channel(capacity_chunks.max(1));
        if let Ok(mut guard) = self.live_tap.lock() {
            *guard = Some(tx);
        }
        rx
    }

    /// Detach the tap. The receiver on the other end will see the channel break
    /// and end its own loop.
    pub fn detach_live_tap(&self) {
        if let Ok(mut guard) = self.live_tap.lock() {
            *guard = None;
        }
    }

    /// Self-tests stream audio without retaining an ever-growing recording.
    pub fn discard_buffer(&self) {
        crate::mutex_recover::lock(&self.audio_buffer).clear();
    }

    pub fn is_recording(&self) -> bool {
        self.is_recording.load(Ordering::Acquire)
    }

    /// Returns the EMA-smoothed RMS level (0.0..~1.0) of the most recent
    /// audio callback. Atomic bit-cast f32 — cheap to poll at 25 Hz.
    pub fn level(&self) -> f32 {
        f32::from_bits(self.level_ema_bits.load(Ordering::Acquire))
    }

    pub fn state(&self) -> RecorderState {
        *self.state.lock().expect("state lock in state()")
    }

    /// Enumerate all available input devices on the default host.
    /// Wraps each device's `name()` in `DeviceInfo`. Returns an empty
    /// Vec if the host has no input devices (no error).
    pub fn list_devices() -> Vec<DeviceInfo> {
        let host = cpal::default_host();
        match host.input_devices() {
            Ok(devices) => devices
                .map(|d| DeviceInfo {
                    name: d.name().unwrap_or_else(|_| "Unknown microphone".into()),
                })
                .collect(),
            Err(e) => {
                log::warn!("input_devices failed: {e}");
                Vec::new()
            }
        }
    }
}

/// Map a raw EMA-smoothed RMS level to a perceptual 0.0..1.0 range for a
/// VU meter. The raw RMS of normal speech at typical mic gain sits around
/// 0.005..0.05 (≈ -46..-26 dBFS) — far below any linear [0,1] threshold,
/// which is why the overlay bars and the mic-test meter looked dead: the
/// signal was real but an order of magnitude too small to cross the visual
/// thresholds. We remap a dB window (-50 dBFS → 0.0, -20 dBFS → 1.0) so
/// silence (~-60 dBFS) clamps to 0 and speech lands in the visible mid-to-
/// upper range. Both the overlay (`audio-level`) and the mic test
/// (`microphone-test-level`) emit through this so they stay consistent.
pub fn display_level(raw_rms: f32) -> f32 {
    const FLOOR_DB: f32 = -50.0;
    const CEIL_DB: f32 = -20.0;
    // Treat non-positive levels AND NaN as silence. Written with an explicit
    // NaN check + `<=` rather than `!(raw_rms > 1e-6)` so clippy's
    // `neg_cmp_on_partial_ord` stays quiet while keeping the NaN→0.0 behaviour.
    if raw_rms.is_nan() || raw_rms <= 1e-6 {
        return 0.0;
    }
    let db = 20.0 * raw_rms.log10();
    ((db - FLOOR_DB) / (CEIL_DB - FLOOR_DB)).clamp(0.0, 1.0)
}

/// Process f32 samples: gate on `is_recording`, update RMS, mono mixdown,
/// Where the recording callback puts its output: the full recording, the level
/// meter, and an optional tap for the live preview. One struct rather than three
/// arguments — the callback holds them together for its entire lifetime.
#[derive(Clone, Default)]
struct CaptureSinks {
    buffer: Arc<Mutex<Vec<f32>>>,
    level_bits: Arc<AtomicU32>,
    live_tap: Arc<Mutex<Option<std::sync::mpsc::SyncSender<Vec<f32>>>>>,
}

/// resample, append to buffer. Called from cpal callback (real-time
/// thread). MUST keep it brief — no allocation beyond what cpal already
/// pre-allocated via `Vec<f32>` for non-F32 sample types.
fn process_samples(
    data: &[f32],
    channels: u16,
    sample_rate: u32,
    target_rate: u32,
    is_recording: &Arc<AtomicBool>,
    sinks: &CaptureSinks,
) {
    let CaptureSinks {
        buffer,
        level_bits,
        live_tap,
    } = sinks;
    if !is_recording.load(Ordering::Acquire) {
        return;
    }
    // RMS update — EMA-smoothed for stable VU meter feel.
    let rms = if data.is_empty() {
        0.0
    } else {
        let sum_sq: f32 = data.iter().map(|&s| s * s).sum();
        (sum_sq / data.len() as f32).sqrt()
    };
    let prev = f32::from_bits(sinks.level_bits.load(Ordering::Acquire));
    let new_level = prev * 0.7 + rms * 0.3;
    level_bits.store(new_level.to_bits(), Ordering::Release);

    // Mono mixdown if stereo (or multi-channel). For stereo this is the
    // classic (L+R)/2 average. We always average; never take just one
    // channel — that would clip or bias.
    //
    // Uses `chunks_exact` to avoid silently mixing a partial trailing
    // frame (fewer than `channels` samples) into an incorrect mono sample.
    // A misaligned cpal driver will trigger a log::warn! below.
    let mono: Vec<f32> = if channels > 1 {
        if !data.len().is_multiple_of(channels as usize) {
            log::warn!(
                "process_samples: data length {} not divisible by {} channels; dropping {} trailing samples",
                data.len(),
                channels,
                data.len() % channels as usize
            );
        }
        data.chunks_exact(channels as usize)
            .map(|frame| frame.iter().sum::<f32>() / channels as f32)
            .collect()
    } else {
        data.to_vec()
    };

    // Resample if device rate != target. Block-of-R averaging where R is
    // the integer ratio (1, 2, 3, or 6). Anything else (e.g. 44.1 kHz
    // devices where ratio ≈ 2.76) falls through with `mono` and a
    // log::error — async polyphase resampling is out of scope for WS 4a2b.
    let ratio = sample_rate as f32 / target_rate as f32;
    let final_audio: Vec<f32> = if ratio == 1.0 {
        mono
    } else if (ratio - 3.0).abs() < 0.01 {
        resample_3_to_1(&mono)
    } else if (ratio - 2.0).abs() < 0.01 {
        resample_2_to_1(&mono)
    } else if (ratio - 6.0).abs() < 0.01 {
        resample_6_to_1(&mono)
    } else {
        log::error!(
            "unsupported sample rate ratio {:.2} ({} -> {}); using raw audio, ASR will degrade",
            ratio,
            sample_rate,
            target_rate
        );
        mono
    };

    // The preview tap comes before writing to the buffer and only via a
    // non-blocking send: a full queue means the preview is not keeping up, and
    // it is the preview that should be dropped, not the dictation. The clone
    // happens only when a tap is attached.
    if let Ok(guard) = live_tap.lock() {
        if let Some(tx) = guard.as_ref() {
            let _ = tx.try_send(final_audio.clone());
        }
    }

    // Append to buffer. The lock is held only for the duration of the
    // extend — never across `.await` or any other blocking call.
    if let Ok(mut buf) = buffer.lock() {
        buf.extend_from_slice(&final_audio);
    }
}

// ============================================================================
// Tests
// ============================================================================
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn i16_normalizes_signed_range() {
        // -32768 → -1.0, 32767 → ≈ 0.99997, 0 → 0.0
        let cases = [
            (0i16, 0.0_f32),
            (32767i16, 32767.0_f32 / 32768.0),
            (-32768i16, -1.0_f32),
            (16384i16, 0.5_f32),
        ];
        for (input, expected) in cases {
            let actual = i16_to_f32(input);
            assert!(
                (actual - expected).abs() < 0.001,
                "i16 {input} → {actual}, expected {expected}"
            );
        }
    }

    #[test]
    fn u8_normalizes_unsigned_range() {
        // 0 → -1.0, 128 → ≈ 0.0, 255 → ≈ 0.968
        let cases = [
            (0u8, -1.0_f32),
            (128u8, 0.0_f32), // (128 - 128) / 128 ≈ 0
            (255u8, (255.0_f32 - 128.0) / 128.0),
        ];
        for (input, expected) in cases {
            let actual = u8_to_f32(input);
            assert!((actual - expected).abs() < 0.01);
        }
    }

    #[test]
    fn resample_3_to_1_decimates_correctly() {
        let input: Vec<f32> = (0..30).map(|i| i as f32).collect();
        let output = resample_3_to_1(&input);
        assert_eq!(output.len(), 10);
        // First 3-sample average = (0+1+2)/3 = 1.0
        assert!((output[0] - 1.0).abs() < 0.001);
        // Second = (3+4+5)/3 = 4.0
        assert!((output[1] - 4.0).abs() < 0.001);
    }

    #[test]
    fn resample_short_input_returns_empty() {
        assert_eq!(resample_3_to_1(&[]).len(), 0);
        assert_eq!(resample_3_to_1(&[1.0, 2.0]).len(), 0);
    }

    #[test]
    fn resample_2_to_1_deterministic() {
        // 6 samples → 3 samples (block-of-2 averaging).
        let input: Vec<f32> = vec![0.0, 1.0, 0.0, 1.0, 0.0, 1.0];
        let output = resample_2_to_1(&input);
        assert_eq!(output.len(), 3);
        // block[0] = (0+1)/2 = 0.5
        assert!((output[0] - 0.5).abs() < 1e-5);
        // block[1] = (0+1)/2 = 0.5
        assert!((output[1] - 0.5).abs() < 1e-5);
        // block[2] = (0+1)/2 = 0.5
        assert!((output[2] - 0.5).abs() < 1e-5);
    }

    #[test]
    fn resample_2_to_1_short_input() {
        // len=1 < 2 → empty (matches resample_3_to_1's "len < R → empty" pattern).
        let output = resample_2_to_1(&[1.0]);
        assert_eq!(output.len(), 0);
    }

    #[test]
    fn resample_2_to_1_empty() {
        let output = resample_2_to_1(&[]);
        assert_eq!(output.len(), 0);
    }

    #[test]
    fn resample_6_to_1_deterministic() {
        let input: Vec<f32> = (0..12).map(|i| i as f32).collect();
        let output = resample_6_to_1(&input);
        assert_eq!(output.len(), 2);
        // block[0] = (0+1+2+3+4+5)/6 = 2.5
        assert!((output[0] - 2.5).abs() < 1e-5);
        // block[1] = (6+7+8+9+10+11)/6 = 8.5
        assert!((output[1] - 8.5).abs() < 1e-5);
    }

    #[test]
    fn resample_6_to_1_short_input() {
        // len=5 < 6 → empty.
        let output = resample_6_to_1(&[1.0, 2.0, 3.0, 4.0, 5.0]);
        assert_eq!(output.len(), 0);
    }

    #[test]
    fn resample_6_to_1_empty() {
        let output = resample_6_to_1(&[]);
        assert_eq!(output.len(), 0);
    }

    #[test]
    fn list_devices_does_not_panic() {
        // CI / headless environments may have no audio devices. The
        // function must NOT panic — it should return an empty Vec.
        let _ = AudioRecorder::list_devices();
    }

    #[test]
    fn audio_config_defaults_to_16khz_mono() {
        let cfg = AudioConfig::default();
        assert_eq!(cfg.sample_rate_target, 16000);
        assert_eq!(cfg.channels_target, 1);
    }

    #[test]
    fn stop_on_never_started_recorder_returns_ok_none() {
        // Behavioral: a freshly-constructed recorder has an empty
        // audio_buffer. stop() should return Ok(None) (no audio) without
        // panicking on the missing Stream (None branch is exercised).
        //
        // This doubles as a check that the constructor does not touch the audio
        // device: the test must pass on a machine with no audio and no
        // interactive session, where the native query crashed the process (#51).
        let recorder = AudioRecorder::new(AudioConfig::default())
            .expect("AudioRecorder::new should succeed even without a real device");
        let result = recorder.stop();
        assert!(
            result.is_ok(),
            "stop() should not error on never-started recorder"
        );
        let audio = result.unwrap();
        assert!(
            audio.is_none(),
            "stop() on never-started recorder should return None (empty buffer)"
        );
        assert!(
            !recorder.is_recording(),
            "is_recording should be false after stop()"
        );
        assert_eq!(
            recorder.state(),
            RecorderState::Idle,
            "FSM should be Idle after stop() on empty buffer"
        );
    }

    #[test]
    fn display_level_maps_speech_into_visible_range() {
        // Silence-ish clamps to 0 (below the -50 dBFS floor).
        assert_eq!(display_level(0.0), 0.0);
        assert_eq!(display_level(0.001), 0.0); // -60 dBFS → clamped

        // A typical observed speech EMA (~0.013 ≈ -37.7 dBFS) must land
        // well above the overlay/mic active thresholds (0.055 / 0.08).
        let speech = display_level(0.013);
        assert!(
            speech > 0.3 && speech < 0.6,
            "speech RMS 0.013 should map mid-range, got {speech}"
        );
        // Loud speech saturates toward 1.0.
        assert!(
            display_level(0.1) >= 0.99,
            "-20 dBFS should hit the ceiling"
        );
        // Monotonic: louder input never maps to a lower display level.
        assert!(display_level(0.03) > display_level(0.01));
    }

    #[test]
    fn level_ema_converges_to_input() {
        // Verify the EMA math used by `process_samples` (prev * 0.7 + rms * 0.3).
        // After many iterations with a constant input, the EMA should
        // converge to within a tight tolerance of the input.
        let mut prev = 0.0_f32;
        let rms = 0.5_f32;
        for _ in 0..100 {
            prev = prev * 0.7 + rms * 0.3;
        }
        assert!(
            (prev - rms).abs() < 0.001,
            "EMA should converge to input after 100 iterations: prev={prev}, rms={rms}"
        );
    }

    #[test]
    fn mono_mixdown_drops_partial_frame_without_undefined_behavior() {
        // Input: stereo buffer with 5 samples (1 complete frame + 1 trailing sample).
        // `chunks_exact` must drop the trailing sample rather than mixing it
        // into a single-sample frame (which would produce wrong amplitude).
        let data: Vec<f32> = vec![0.4, 0.6, 0.8, 0.2, 0.1];
        let channels: u16 = 2;
        let sample_rate: u32 = 16000;
        let target_rate: u32 = 16000; // same rate → no resampling

        let (is_recording, sinks) = recording_sink(0.0);

        process_samples(
            &data,
            channels,
            sample_rate,
            target_rate,
            &is_recording,
            &sinks,
        );

        let result = sinks.buffer.lock().unwrap();
        // Expected: 2 mono frames from the two complete stereo frames.
        // Frame 0: (0.4 + 0.6) / 2 = 0.5
        // Frame 1: (0.8 + 0.2) / 2 = 0.5
        // Trailing 0.1 is dropped.
        assert_eq!(
            result.len(),
            2,
            "expected 2 mono samples from 5 interleaved stereo samples; got {:?}",
            *result
        );
        assert!(
            (result[0] - 0.5).abs() < 1e-6,
            "first mono frame expected 0.5, got {}",
            result[0]
        );
        assert!(
            (result[1] - 0.5).abs() < 1e-6,
            "second mono frame expected 0.5, got {}",
            result[1]
        );
    }

    /// Exactly R samples is one block, not "too short". Catches swapping
    /// `len < R` for `==`/`<=` in all three resamplers.
    #[test]
    fn resample_helpers_respect_the_ratio_boundary() {
        assert_eq!(resample_3_to_1(&[1.0, 2.0, 3.0]), vec![2.0]);
        assert_eq!(resample_2_to_1(&[1.0, 2.0]), vec![1.5]);
        assert_eq!(resample_6_to_1(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0]), vec![3.5]);
    }

    /// Non-periodic input: the block index must advance (`i * 2`), otherwise all
    /// blocks collapse into the first one.
    #[test]
    fn resample_2_to_1_uses_distinct_blocks() {
        assert_eq!(resample_2_to_1(&[0.0, 1.0, 2.0, 3.0]), vec![0.5, 2.5]);
    }

    #[test]
    fn display_level_nan_is_silence() {
        assert_eq!(display_level(f32::NAN), 0.0);
    }

    /// Sinks for a test: recording and level as in production, the tap
    /// disconnected. A test that needs the tap installs it itself.
    fn test_sinks(level: f32) -> CaptureSinks {
        CaptureSinks {
            level_bits: Arc::new(AtomicU32::new(level.to_bits())),
            ..CaptureSinks::default()
        }
    }

    /// The tap receives exactly what the recording does: already downmixed to
    /// mono and resampled to the target rate.
    #[test]
    fn the_live_tap_gets_the_same_audio_as_the_recording() {
        let is_recording = Arc::new(AtomicBool::new(true));
        let sinks = test_sinks(0.0);
        let (tx, rx) = std::sync::mpsc::sync_channel(4);
        *sinks.live_tap.lock().unwrap() = Some(tx);

        // Stereo at 32 kHz: the callback must downmix to mono and halve the rate.
        let data = vec![1.0_f32, 1.0, 0.5, 0.5, 0.25, 0.25, 0.75, 0.75];
        process_samples(&data, 2, 32_000, 16_000, &is_recording, &sinks);

        let chunk = rx.try_recv().expect("ответвление не получило звук");
        assert_eq!(chunk, *sinks.buffer.lock().unwrap());
        assert_eq!(chunk.len(), 2);
    }

    /// The core invariant: the preview may fall behind, the dictation may not.
    #[test]
    fn a_full_live_queue_never_costs_the_recording() {
        let is_recording = Arc::new(AtomicBool::new(true));
        let sinks = test_sinks(0.0);
        // A one-chunk queue: the second call overflows it.
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        *sinks.live_tap.lock().unwrap() = Some(tx);

        let data = vec![0.5_f32; 4];
        for _ in 0..3 {
            process_samples(&data, 1, 16_000, 16_000, &is_recording, &sinks);
        }

        assert_eq!(
            sinks.buffer.lock().unwrap().len(),
            12,
            "запись потеряла звук"
        );
        assert_eq!(rx.try_recv().map(|c| c.len()), Ok(4));
        // Overflow simply drops chunks without blocking the callback.
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn an_unattached_tap_changes_nothing() {
        let is_recording = Arc::new(AtomicBool::new(true));
        let sinks = test_sinks(0.0);
        let data = vec![0.5_f32; 4];
        process_samples(&data, 1, 16_000, 16_000, &is_recording, &sinks);
        assert_eq!(sinks.buffer.lock().unwrap().len(), 4);
    }

    /// The recording flag and the sinks `process_samples` writes through.
    /// `level` seeds the EMA so the test can tell "smoothed from the previous
    /// level" apart from "computed from scratch".
    fn recording_sink(level: f32) -> (Arc<AtomicBool>, CaptureSinks) {
        (Arc::new(AtomicBool::new(true)), test_sinks(level))
    }

    #[test]
    fn process_samples_updates_rms_ema() {
        // Non-zero prev so `prev * 0.7` is not the trivial 0 case.
        let (is_recording, sinks) = recording_sink(1.0);
        let data = vec![0.5_f32, 0.5, 0.5, 0.5];
        process_samples(&data, 1, 16_000, 16_000, &is_recording, &sinks);
        // RMS of [0.5,0.5,0.5,0.5] = 0.5; EMA = 1.0*0.7 + 0.5*0.3 = 0.85.
        let level = f32::from_bits(sinks.level_bits.load(Ordering::Acquire));
        assert!((level - 0.85).abs() < 1e-6, "level {level}");
        assert_eq!(&*sinks.buffer.lock().unwrap(), &[0.5, 0.5, 0.5, 0.5]);
    }

    #[test]
    fn process_samples_resamples_32k_to_16k() {
        let (is_recording, sinks) = recording_sink(0.0);
        let data = vec![0.0_f32, 1.0, 0.0, 1.0, 0.0, 1.0];
        process_samples(&data, 1, 32_000, 16_000, &is_recording, &sinks);
        assert_eq!(
            sinks.buffer.lock().unwrap().len(),
            3,
            "32 kHz → 16 kHz halves the sample count"
        );
    }

    #[test]
    fn process_samples_resamples_48k_to_16k() {
        let (is_recording, sinks) = recording_sink(0.0);
        let data: Vec<f32> = (0..9).map(|i| i as f32).collect();
        process_samples(&data, 1, 48_000, 16_000, &is_recording, &sinks);
        assert_eq!(
            sinks.buffer.lock().unwrap().len(),
            3,
            "48 kHz → 16 kHz divides the sample count by 3"
        );
    }

    #[test]
    fn process_samples_resamples_96k_to_16k() {
        let (is_recording, sinks) = recording_sink(0.0);
        let data: Vec<f32> = (0..12).map(|i| i as f32).collect();
        process_samples(&data, 1, 96_000, 16_000, &is_recording, &sinks);
        assert_eq!(
            sinks.buffer.lock().unwrap().len(),
            2,
            "96 kHz → 16 kHz divides the sample count by 6"
        );
    }
}
