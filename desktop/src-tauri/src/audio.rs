//! Microphone capture, mono conversion and streaming resampling to 16 kHz.
//! Device calls are serialized by AudioWorker. Stop releases the native stream
//! before flushing the resampler and handing off the complete audio buffer.

use crate::audio_resampler::AudioResampler;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream, StreamConfig};

/// Target sample rate (Hz) and channel count for downstream consumers
/// (whisper ASR expects 16 kHz mono).
#[derive(Debug, Clone, Copy)]
pub struct AudioConfig {
    pub sample_rate_target: u32, // 16000
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            sample_rate_target: 16000,
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
    /// `None` when cpal cannot read the name. Kept as an absence rather than
    /// filled with a placeholder: a made-up name is indistinguishable from a
    /// real one, and [`device_ids`] must not hand out an id that
    /// [`AudioRecorder::start_selected`] can never match back to a device.
    pub name: Option<String>,
}

/// Stable ids for a device list, in enumeration order.
///
/// A name survives replugging and reordering, which an index does not — so a
/// readable, unique name is the id. It stops being an identifier the moment it
/// stops identifying: two microphones of the same model report the same name,
/// and some devices report none at all. Those fall back to the position, which
/// is at least correct for the current enumeration.
pub fn device_ids(devices: &[DeviceInfo]) -> Vec<String> {
    let mut seen: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for device in devices {
        if let Some(name) = device.name.as_deref() {
            *seen.entry(name).or_default() += 1;
        }
    }
    devices
        .iter()
        .enumerate()
        .map(|(index, device)| match device.name.as_deref() {
            Some(name) if seen.get(name) == Some(&1) => format!("name:{name}"),
            _ => format!("index:{index}"),
        })
        .collect()
}

// ============================================================================
// SendStream — wrap `cpal::Stream` so it can be stored in `AppState`.
// ============================================================================
//
// cpal 0.16's `Stream` is intentionally `!Send + !Sync` (see the
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

pub struct AudioRecorder {
    state: Mutex<RecorderState>,
    /// Shared by capture, stream-error callbacks and stop, so teardown makes
    /// subsequent callbacks stop writing without taking a mutex.
    is_recording: Arc<AtomicBool>,
    resampler: Arc<Mutex<Option<AudioResampler>>>,
    capture_error: Arc<Mutex<Option<String>>>,
    first_frame_ms: Arc<AtomicU64>,
    audio_buffer: Arc<Mutex<Vec<f32>>>, // Arc — callback needs 'static + Send
    /// An audio tap for the live preview. The full recording accumulates in
    /// `audio_buffer` as before — this queue merely duplicates chunks along the
    /// way. Bounded and non-blocking: the preview is allowed to fall behind and
    /// lose a chunk, the recording is not.
    live_tap: Arc<Mutex<Option<std::sync::mpsc::SyncSender<Vec<f32>>>>>,
    /// RMS level EMA, atomic bit-cast f32. Wrapped in Arc so the callback
    /// can update the SAME bit-cast the public `level()` reads.
    level_ema_bits: Arc<AtomicU32>,
    /// Rate after resampling, shared by captured PCM and the live tap.
    /// `0` until the first `start`.
    tap_sample_rate: AtomicU32,
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
            is_recording: Arc::new(AtomicBool::new(false)),
            resampler: Arc::new(Mutex::new(None)),
            capture_error: Arc::new(Mutex::new(None)),
            first_frame_ms: Arc::new(AtomicU64::new(u64::MAX)),
            audio_buffer: Arc::new(Mutex::new(Vec::with_capacity(Self::APPROX_CAPACITY))),
            level_ema_bits: Arc::new(AtomicU32::new(0.0_f32.to_bits())),
            live_tap: Arc::new(Mutex::new(None)),
            tap_sample_rate: AtomicU32::new(0),
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
                } else if let Some(index) = value
                    .strip_prefix("index:")
                    .and_then(|index| index.parse::<usize>().ok())
                {
                    // Position, for the devices a name cannot identify — see
                    // [`device_ids`].
                    devices.nth(index)
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
        // cpal wraps sample_rate in `cpal::SampleRate(pub u32)`.
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
        crate::mutex_recover::lock(&self.audio_buffer).clear();

        // Arc references for the callback closure. The callback MUST be
        // `'static + Send` for cpal's real-time thread. Each branch of
        // the SampleFormat match below MOVEs its own Arc clone into its
        // closure (a single closure owns the Arc, so multiple branches
        // each need their own clone — since `Arc` is not `Copy`).
        let target_rate = self.config.sample_rate_target;
        let channels_for_cb = channels;
        let sample_rate_for_cb = sample_rate_u32;
        *crate::mutex_recover::lock(&self.resampler) =
            Some(AudioResampler::new(sample_rate_u32, target_rate)?);
        *crate::mutex_recover::lock(&self.capture_error) = None;
        self.first_frame_ms.store(u64::MAX, Ordering::Relaxed);
        self.tap_sample_rate.store(target_rate, Ordering::Release);
        let capture_error = Arc::clone(&self.capture_error);
        let recording = Arc::clone(&self.is_recording);
        let err_cb = move |err: cpal::StreamError| {
            *crate::mutex_recover::lock(&capture_error) = Some(format!("microphone stream: {err}"));
            recording.store(false, Ordering::Release);
        };

        // `cpal::build_input_stream<T>` encodes the runtime sample format in
        // its `T` type parameter, so the match below builds one closure per
        // format. Exactly one arm runs, and each may move its own Arc clones.
        // Non-F32 arms allocate one `Vec<f32>` per callback for the converted
        // samples; F32 skips that step, and every format shares the mixdown,
        // resampling and buffering downstream.
        let is_recording_cb = Arc::clone(&self.is_recording);

        // Each non-F32 arm below differs only in the sample type `T` and the
        // per-sample `T -> f32` conversion. This macro keeps those two knobs
        // visible while removing the ~13-line closure boilerplate that was
        // copy-pasted once per format. The F32 arm stays separate because it
        // needs no conversion: it forwards `&[f32]` straight to
        // `process_samples`, which still allocates to downmix and resample.
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
                device.build_input_stream::<$sample, _, _>(&stream_config, cb, err_cb.clone(), None)
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
            _ => {
                return Err(format!(
                    "unsupported microphone sample format: {sample_format:?}"
                ))
            }
        };

        let stream = build_result.map_err(|e| format!("build_input_stream: {e}"))?;
        {
            let error = crate::mutex_recover::lock(&self.capture_error);
            if let Some(error) = error.as_ref() {
                return Err(error.clone());
            }
            // Arm before play so early samples are retained. Never overwrite
            // a stream-error callback's false flag after play returns.
            self.is_recording.store(true, Ordering::Release);
        }
        if let Err(error) = stream.play() {
            self.is_recording.store(false, Ordering::Release);
            return Err(format!("stream.play: {error}"));
        }

        // Wrap in SendStream so the Mutex<Option<SendStream>> can be held
        // in AppState (which requires Send + Sync). See SendStream doc.
        let send_stream = SendStream(stream);

        *crate::mutex_recover::lock(&self.stream) = Some(send_stream);
        *crate::mutex_recover::lock(&self.state) = RecorderState::Recording;
        Ok(())
    }

    /// Canonical drop-and-drain:
    ///   1. Flip `is_recording` so the callback early-exits next invocation.
    ///   2. Drop the stream before flushing. This relies on the backend
    ///      releasing its callbacks; the native repeated-session test checks
    ///      that contract for default and explicitly selected microphones.
    ///   3. Now safely lock the buffer and take ownership of the samples.
    ///      Wrap in `Arc` so callers (engine command sender) can move
    ///      the audio to a worker thread without copying.
    pub fn stop(&self) -> Result<Option<Arc<Vec<f32>>>, String> {
        // 1. Flip is_recording so the next callback early-exits.
        self.is_recording.store(false, Ordering::Release);
        // 2. Release the native stream before touching its final buffered PCM.
        let stream_opt = crate::mutex_recover::lock(&self.stream).take();
        if let Some(stream) = stream_opt {
            drop(stream);
        }
        let sinks = self.capture_sinks();
        flush_samples(&sinks)?;
        if let Some(error) = crate::mutex_recover::lock(&self.capture_error).take() {
            crate::mutex_recover::lock(&self.audio_buffer).clear();
            *crate::mutex_recover::lock(&self.state) = RecorderState::Error;
            return Err(error);
        }
        // 3. Lock the buffer (safe now — no callback is running) and
        //    take the samples.
        let buf_arc = Arc::clone(&self.audio_buffer);
        let mut buf = crate::mutex_recover::lock(&buf_arc);
        if buf.is_empty() {
            *crate::mutex_recover::lock(&self.state) = RecorderState::Idle;
            return Ok(None);
        }
        let taken = std::mem::take(&mut *buf);
        *crate::mutex_recover::lock(&self.state) = RecorderState::Stopped;
        Ok(Some(Arc::new(taken)))
    }

    /// A snapshot of the sinks for the callback. All three are `Arc`s, so the
    /// callback writes into exactly what the public methods read.
    fn capture_sinks(&self) -> CaptureSinks {
        CaptureSinks {
            buffer: Arc::clone(&self.audio_buffer),
            level_bits: Arc::clone(&self.level_ema_bits),
            live_tap: Arc::clone(&self.live_tap),
            resampler: Arc::clone(&self.resampler),
            capture_error: Arc::clone(&self.capture_error),
            first_frame_ms: Arc::clone(&self.first_frame_ms),
            started_at: Instant::now(),
        }
    }

    /// Attach the live audio tap and obtain a receiver of chunks.
    ///
    /// The capacity is given in chunks rather than seconds: the callback hands
    /// over one chunk per call, and a queue of a few dozen chunks is on the
    /// order of a second of audio at a typical cpal buffer size.
    pub fn attach_live_tap(&self, capacity_chunks: usize) -> std::sync::mpsc::Receiver<Vec<f32>> {
        let (tx, rx) = std::sync::mpsc::sync_channel(capacity_chunks.max(1));
        *crate::mutex_recover::lock(&self.live_tap) = Some(tx);
        rx
    }

    /// Detach the tap. The receiver on the other end will see the channel break
    /// and end its own loop.
    pub fn detach_live_tap(&self) {
        *crate::mutex_recover::lock(&self.live_tap) = None;
    }

    /// Rate of the samples coming out of [`Self::attach_live_tap`], or the
    /// configured target before the first `start` — nothing has been produced
    /// at any other rate yet, so that is the honest answer rather than a guess.
    pub fn tap_sample_rate(&self) -> u32 {
        match self.tap_sample_rate.load(Ordering::Acquire) {
            0 => self.config.sample_rate_target,
            rate => rate,
        }
    }

    /// Self-tests stream audio without retaining an ever-growing recording.
    pub fn discard_buffer(&self) {
        crate::mutex_recover::lock(&self.audio_buffer).clear();
    }

    /// Seconds of audio captured so far in this recording.
    pub fn recorded_seconds(&self) -> f64 {
        crate::mutex_recover::lock(&self.audio_buffer).len() as f64
            / self.config.sample_rate_target as f64
    }

    pub fn has_capture_error(&self) -> bool {
        crate::mutex_recover::lock(&self.capture_error).is_some()
    }

    pub fn first_frame_ms(&self) -> Option<u64> {
        let value = self.first_frame_ms.load(Ordering::Relaxed);
        (value != u64::MAX).then_some(value)
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
        *crate::mutex_recover::lock(&self.state)
    }

    /// Enumerate all available input devices on the default host.
    /// Wraps each device's `name()` in `DeviceInfo`. Returns an empty
    /// Vec if the host has no input devices (no error).
    pub fn list_devices() -> Vec<DeviceInfo> {
        let host = cpal::default_host();
        match host.input_devices() {
            Ok(devices) => devices
                .map(|d| DeviceInfo {
                    name: d.name().ok(),
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

/// Where the recording callback puts its output: the full recording, the level
/// meter, and an optional tap for the live preview. One struct rather than three
/// arguments — the callback holds them together for its entire lifetime.
#[derive(Clone)]
struct CaptureSinks {
    buffer: Arc<Mutex<Vec<f32>>>,
    level_bits: Arc<AtomicU32>,
    live_tap: Arc<Mutex<Option<std::sync::mpsc::SyncSender<Vec<f32>>>>>,
    resampler: Arc<Mutex<Option<AudioResampler>>>,
    capture_error: Arc<Mutex<Option<String>>>,
    first_frame_ms: Arc<AtomicU64>,
    started_at: Instant,
}

#[cfg(test)]
impl Default for CaptureSinks {
    fn default() -> Self {
        Self {
            buffer: Default::default(),
            level_bits: Default::default(),
            live_tap: Default::default(),
            resampler: Default::default(),
            capture_error: Default::default(),
            first_frame_ms: Arc::new(AtomicU64::new(u64::MAX)),
            started_at: Instant::now(),
        }
    }
}

fn append_samples(sinks: &CaptureSinks, samples: &[f32]) {
    if samples.is_empty() {
        return;
    }
    if let Some(tx) = crate::mutex_recover::lock(&sinks.live_tap).as_ref() {
        let _ = tx.try_send(samples.to_vec());
    }
    crate::mutex_recover::lock(&sinks.buffer).extend_from_slice(samples);
}

fn flush_samples(sinks: &CaptureSinks) -> Result<(), String> {
    if let Some(mut filter) = crate::mutex_recover::lock(&sinks.resampler).take() {
        append_samples(sinks, filter.finish()?);
    }
    Ok(())
}

fn process_samples(
    data: &[f32],
    channels: u16,
    sample_rate: u32,
    target_rate: u32,
    is_recording: &Arc<AtomicBool>,
    sinks: &CaptureSinks,
) {
    if !is_recording.load(Ordering::Acquire) || data.is_empty() {
        return;
    }
    if sinks.first_frame_ms.load(Ordering::Relaxed) == u64::MAX {
        sinks.first_frame_ms.store(
            sinks.started_at.elapsed().as_millis() as u64,
            Ordering::Relaxed,
        );
    }
    let rms = (data.iter().map(|s| s * s).sum::<f32>() / data.len() as f32).sqrt();
    let prev = f32::from_bits(sinks.level_bits.load(Ordering::Relaxed));
    sinks
        .level_bits
        .store((prev * 0.7 + rms * 0.3).to_bits(), Ordering::Release);
    let mono;
    let samples = if channels > 1 {
        mono = data
            .chunks_exact(channels as usize)
            .map(|frame| frame.iter().sum::<f32>() / channels as f32)
            .collect::<Vec<_>>();
        mono.as_slice()
    } else {
        data
    };
    let result = (|| {
        let mut guard = crate::mutex_recover::lock(&sinks.resampler);
        if guard.is_none() {
            *guard = Some(AudioResampler::new(sample_rate, target_rate)?);
        }
        append_samples(sinks, guard.as_mut().unwrap().push(samples)?);
        Ok::<_, String>(())
    })();
    if let Err(error) = result {
        *crate::mutex_recover::lock(&sinks.capture_error) = Some(error);
        is_recording.store(false, Ordering::Release);
    }
}

/// Enumerate available microphones with index+id+name+label.
/// Boot-blocking — called from `MainWindow.load()` via `Promise.all`.
///
/// Maps each device position to `index`/`id` (same value), and
/// copies the device `name` into both `name` and `label` fields.
/// The frontend consumes this via:
///   `microphones.map((mic) => ({
///      label: mic.name || mic.label || String(mic.id ?? mic.index),
///      value: mic.id ?? mic.index ?? null
///   }))`
/// Device enumeration is a WASAPI call, so it runs on the audio worker
/// like every other one. It used to run inline on the main thread, which
/// meant the settings page could freeze the UI on a sick audio stack.
#[tauri::command]
pub(crate) async fn list_microphones(
    state: tauri::State<'_, crate::state::AppState>,
) -> Result<Vec<serde_json::Value>, String> {
    let devices = state
        .audio
        .call(AudioRecorder::list_devices)
        .await
        .unwrap_or_default();
    // The id is built from the whole list at once, not per device: whether a
    // name identifies anything depends on the other names next to it.
    let ids = device_ids(&devices);
    Ok(devices
        .into_iter()
        .zip(ids)
        .enumerate()
        .map(|(i, (dev, id))| {
            // `name` stays null when cpal could not read one. A placeholder
            // belongs to the interface, which can say it in the user's own
            // language; here it would only be an English string pretending to
            // be a device name.
            serde_json::json!({
                "id": id,
                "index": i,
                "name": dev.name,
                "label": dev.name,
            })
        })
        .collect())
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
    fn list_devices_does_not_panic() {
        // CI / headless environments may have no audio devices. The
        // function must NOT panic — it should return an empty Vec.
        let _ = AudioRecorder::list_devices();
    }

    #[test]
    fn audio_config_defaults_to_16khz() {
        let cfg = AudioConfig::default();
        assert_eq!(cfg.sample_rate_target, 16000);
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

    /// Exercise native stream teardown, which feeding synthetic PCM cannot
    /// cover. Explicit selection must use enumeration even for the default
    /// microphone: CoreAudio installs a different disconnect listener there.
    #[test]
    #[ignore = "requires a real microphone and OS permission; captures only in memory"]
    fn native_microphone_repeated_sessions_preserve_duration() {
        use std::time::Duration;

        let name = cpal::default_host()
            .default_input_device()
            .expect("connect a microphone")
            .name()
            .expect("read microphone name");
        let selected = format!("name:{name}");
        for selection in [None, Some(selected.as_str())] {
            let recorder = AudioRecorder::new(AudioConfig::default()).unwrap();
            for session in 1..=5 {
                recorder.start_selected(selection).unwrap();
                let started = Instant::now();
                std::thread::sleep(Duration::from_secs(1));
                let elapsed = started.elapsed().as_secs_f64();
                let audio = recorder
                    .stop()
                    .unwrap()
                    .expect("microphone produced no PCM");
                let seconds = audio.len() as f64 / 16_000.0;
                let route = if selection.is_some() {
                    "selected"
                } else {
                    "default"
                };
                println!("{route} session {session}: wall={elapsed:.3}s, audio={seconds:.3}s");
                assert!(
                    (seconds - elapsed).abs() < elapsed * 0.25,
                    "{route} session {session}: captured {seconds:.3}s during {elapsed:.3}s"
                );
                // A surviving callback owns the buffer even if a recording
                // flag temporarily prevents it from appending more samples.
                assert_eq!(
                    Arc::strong_count(&recorder.audio_buffer),
                    1,
                    "{route} session {session}: native callback survived stop"
                );
                std::thread::sleep(Duration::from_millis(100));
                assert!(recorder.stop().unwrap().is_none());
            }
        }
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

    fn named(names: &[Option<&str>]) -> Vec<DeviceInfo> {
        names
            .iter()
            .map(|name| DeviceInfo {
                name: name.map(str::to_string),
            })
            .collect()
    }

    /// The whole point of a name id: it survives the device list being
    /// reordered, which is what replugging a USB microphone does.
    #[test]
    fn a_unique_name_is_the_identifier() {
        let ids = device_ids(&named(&[Some("USB Mic"), Some("Built-in")]));
        assert_eq!(ids, ["name:USB Mic", "name:Built-in"]);
        let reordered = device_ids(&named(&[Some("Built-in"), Some("USB Mic")]));
        assert_eq!(reordered[1], ids[0]);
    }

    /// Two microphones of one model report one name. It cannot tell them apart,
    /// so it must not pretend to: both fall back to their position, and the
    /// second one is selectable instead of silently resolving to the first.
    #[test]
    fn duplicate_names_fall_back_to_position() {
        let ids = device_ids(&named(&[Some("USB Mic"), Some("Other"), Some("USB Mic")]));
        assert_eq!(ids, ["index:0", "name:Other", "index:2"]);
    }

    /// An unreadable name used to become the literal "Unknown microphone",
    /// which `start_selected` then looked for among devices that report no name
    /// at all — and never found.
    #[test]
    fn a_nameless_device_is_still_selectable() {
        let ids = device_ids(&named(&[Some("USB Mic"), None]));
        assert_eq!(ids, ["name:USB Mic", "index:1"]);
    }

    #[test]
    fn the_live_tap_and_recording_get_16k_audio_from_44100_stereo() {
        let (live, sinks) = recording_sink(0.0);
        let (tx, rx) = std::sync::mpsc::sync_channel(100);
        *sinks.live_tap.lock().unwrap() = Some(tx);
        for chunk in vec![0.5; 88_200].chunks(882) {
            process_samples(chunk, 2, 44_100, 16_000, &live, &sinks);
        }
        flush_samples(&sinks).unwrap();
        let captured = sinks.buffer.lock().unwrap().clone();
        let preview: Vec<f32> = rx.try_iter().flatten().collect();
        assert_eq!(captured.len(), 16_000);
        assert_eq!(captured, preview);
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
            "the recording lost audio"
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
}
