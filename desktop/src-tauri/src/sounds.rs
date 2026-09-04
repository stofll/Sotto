//! Short audio cues for the dictation cycle.
//!
//! Dictation happens while looking at some other window, so the only
//! feedback the overlay gives is feedback nobody sees. These are the
//! non-visual half: a blip when the recording arms, when it stops, when the
//! text lands, and when something went wrong.
//!
//! The waveforms are synthesised here rather than shipped as assets — a few
//! sine segments with a click-free envelope cost less than a WAV in the
//! bundle, and keep the "no new dependencies" property of the whole feature.
//!
//! Playback is Windows-only for now (`PlaySoundW` from winmm, already
//! reachable through `windows-sys`). On other platforms [`play`] is a no-op:
//! the app is Windows-first, and a half-working cue is worse than none.

use serde_json::Value;

/// Which moment in the dictation cycle a cue marks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cue {
    /// Recording armed and the microphone is live.
    Start,
    /// Recording stopped, transcription is running. Only played in toggle
    /// mode — under push-to-talk the user's own key release already marks
    /// this moment, and a blip there would just precede [`Cue::Done`] by a
    /// second or two of nothing.
    Stop,
    /// Final text pasted into the focused window.
    Done,
    /// The cycle produced no text — a failure, or a recording that turned
    /// out to be silence. Both leave the user waiting for an insert that
    /// never comes, so both get the same cue.
    Error,
}

/// Config key toggling the whole feature.
const CONFIG_ENABLED: &str = "sound_feedback";
/// Config key for cue loudness, `0.0`–`1.0`.
const CONFIG_VOLUME: &str = "sound_volume";
/// Quiet enough to sit under speech without being startling, loud enough to
/// carry over laptop speakers.
const DEFAULT_VOLUME: f32 = 0.35;

/// Sample rate of the generated PCM. 22.05 kHz is plenty for tones under
/// 1.5 kHz and halves the buffer against 44.1.
const SAMPLE_RATE: u32 = 22_050;
/// Fade applied to both ends of every segment. Without it the waveform
/// starts and stops at a non-zero sample and the speaker clicks.
const FADE_MS: f32 = 8.0;

/// `(frequency Hz, duration ms)` segments per cue.
///
/// Start rises and Stop falls, so the pair is distinguishable without
/// having to learn it. Done is brighter and shorter — it fires while the
/// user is already moving on. Error sits well below the others.
fn segments(cue: Cue) -> &'static [(f32, u32)] {
    match cue {
        Cue::Start => &[(587.33, 55), (880.00, 70)],
        Cue::Stop => &[(880.00, 55), (587.33, 70)],
        Cue::Done => &[(1046.50, 45), (1396.91, 75)],
        Cue::Error => &[(311.13, 90), (233.08, 150)],
    }
}

/// Read the cue settings out of an already-loaded config value.
///
/// Returns `None` when cues are switched off. Volume is clamped rather than
/// rejected: a hand-edited `config.json` should not be able to produce a
/// waveform that clips.
fn volume_from_config(config: &Value) -> Option<f32> {
    let enabled = config
        .get(CONFIG_ENABLED)
        .and_then(Value::as_bool)
        .unwrap_or(true);
    if !enabled {
        return None;
    }
    let volume = config
        .get(CONFIG_VOLUME)
        .and_then(Value::as_f64)
        .map(|v| v as f32)
        .unwrap_or(DEFAULT_VOLUME);
    Some(volume.clamp(0.0, 1.0))
}

/// Play `cue`, unless the user has turned cues off.
///
/// Never blocks the caller and never fails loudly: a cue that does not come
/// out is not worth interrupting a dictation over. Safe to call from any
/// thread.
pub fn play(app: &tauri::AppHandle, cue: Cue) {
    let Ok(config) = crate::config::Config::load(app) else {
        return;
    };
    let Some(volume) = volume_from_config(config.as_value()) else {
        return;
    };
    play_at_volume(cue, volume);
}

/// Play `cue` at an explicit volume, bypassing the config lookup. Used by
/// the settings UI to preview a cue the user is about to enable.
pub fn play_at_volume(cue: Cue, volume: f32) {
    let wav = render_wav(segments(cue), volume.clamp(0.0, 1.0));
    submit(wav);
}

/// Hand a rendered WAV to the playback backend.
#[cfg(windows)]
fn submit(wav: Vec<u8>) {
    use std::sync::mpsc::{sync_channel, SyncSender};
    use std::sync::OnceLock;
    use windows_sys::Win32::Media::Audio::{PlaySoundW, SND_MEMORY, SND_NODEFAULT};

    static WORKER: OnceLock<SyncSender<Vec<u8>>> = OnceLock::new();

    // One long-lived worker rather than a thread per cue. `PlaySoundW` is
    // called *synchronously* here on purpose: with `SND_ASYNC` the API keeps
    // reading from the buffer after returning, so the buffer would have to
    // outlive the call — and winmm has one playback slot per process anyway,
    // so an async cue would be cut off by the next one regardless.
    let tx = WORKER.get_or_init(|| {
        // Bounded and small: cues are only meaningful near the moment they
        // describe. A backlog means playback is wedged, and the right
        // response is to drop cues, not to queue them up.
        let (tx, rx) = sync_channel::<Vec<u8>>(2);
        std::thread::Builder::new()
            .name("sound-cues".to_string())
            .spawn(move || {
                while let Ok(wav) = rx.recv() {
                    // SAFETY: `wav` is a complete in-memory RIFF/WAVE image
                    // that outlives the synchronous call. `SND_NODEFAULT`
                    // stops winmm from substituting the system default sound
                    // if it cannot parse it.
                    let played = unsafe {
                        PlaySoundW(wav.as_ptr() as *const u16, 0, SND_MEMORY | SND_NODEFAULT)
                    };
                    if played == 0 {
                        // No error code to report — winmm only says yes or
                        // no. Worth a line anyway: silent cues otherwise look
                        // like a config problem to whoever reports them.
                        log::warn!("PlaySoundW rejected a cue");
                    }
                }
            })
            .ok();
        tx
    });
    let _ = tx.try_send(wav);
}

#[cfg(not(windows))]
fn submit(_wav: Vec<u8>) {}

/// Render `segments` into a complete 16-bit mono RIFF/WAVE image.
fn render_wav(segments: &[(f32, u32)], volume: f32) -> Vec<u8> {
    crate::wav::encode_pcm16_mono(&render_pcm(segments, volume), SAMPLE_RATE)
}

/// Synthesise the PCM body: one sine per segment, each faded in and out.
fn render_pcm(segments: &[(f32, u32)], volume: f32) -> Vec<i16> {
    let fade_samples = (FADE_MS / 1000.0 * SAMPLE_RATE as f32) as usize;
    let mut out = Vec::new();
    for &(freq, duration_ms) in segments {
        let count = (duration_ms as f32 / 1000.0 * SAMPLE_RATE as f32) as usize;
        // A fade longer than half the segment would never reach full
        // amplitude and the two ramps would fight each other.
        let fade = fade_samples.min(count / 2);
        for i in 0..count {
            let envelope = if fade == 0 {
                1.0
            } else if i < fade {
                i as f32 / fade as f32
            } else if i >= count - fade {
                // `count - 1 - i`, not `count - i`: the ramp has to reach
                // exactly zero on the last sample, otherwise the segment
                // ends on a step and clicks — which is the whole point of
                // the envelope.
                (count - 1 - i) as f32 / fade as f32
            } else {
                1.0
            };
            let phase = 2.0 * std::f32::consts::PI * freq * (i as f32 / SAMPLE_RATE as f32);
            let value = phase.sin() * envelope * volume;
            out.push((value * i16::MAX as f32) as i16);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn cues_are_on_by_default() {
        assert_eq!(volume_from_config(&json!({})), Some(DEFAULT_VOLUME));
    }

    #[test]
    fn disabling_returns_no_volume() {
        assert_eq!(
            volume_from_config(&json!({ "sound_feedback": false })),
            None
        );
    }

    #[test]
    fn volume_is_clamped_not_rejected() {
        // A hand-edited config must not be able to produce a clipping
        // waveform (or a negative one, which would invert the phase).
        assert_eq!(
            volume_from_config(&json!({ "sound_volume": 4.0 })),
            Some(1.0)
        );
        assert_eq!(
            volume_from_config(&json!({ "sound_volume": -1.0 })),
            Some(0.0)
        );
    }

    #[test]
    fn wav_header_describes_the_payload() {
        let wav = render_wav(&[(440.0, 100)], 0.5);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(&wav[36..40], b"data");
        let declared = u32::from_le_bytes(wav[40..44].try_into().unwrap()) as usize;
        assert_eq!(declared, wav.len() - 44);
        // 100 ms of 16-bit mono at 22.05 kHz.
        assert_eq!(declared, 2205 * 2);
        let riff_size = u32::from_le_bytes(wav[4..8].try_into().unwrap()) as usize;
        assert_eq!(riff_size, wav.len() - 8);
    }

    #[test]
    fn envelope_starts_and_ends_at_silence() {
        // Non-zero endpoints are what make a short tone click.
        let pcm = render_pcm(&[(440.0, 100)], 1.0);
        assert_eq!(pcm[0], 0);
        assert_eq!(*pcm.last().unwrap(), 0);
        assert!(pcm.iter().any(|&s| s.abs() > i16::MAX / 2));
    }

    #[test]
    fn zero_volume_renders_silence() {
        let pcm = render_pcm(&[(440.0, 50)], 0.0);
        assert!(!pcm.is_empty());
        assert!(pcm.iter().all(|&s| s == 0));
    }

    /// Actually audible; run with
    /// `cargo test --lib sounds::tests::audition -- --ignored --nocapture`
    /// when tweaking the tones. Nothing about a waveform's correctness on
    /// paper tells you whether it sounds like feedback or like a fault.
    #[test]
    #[ignore = "plays audio through the default output device"]
    fn audition() {
        for cue in [Cue::Start, Cue::Stop, Cue::Done, Cue::Error] {
            println!("{cue:?}");
            play_at_volume(cue, DEFAULT_VOLUME);
            std::thread::sleep(std::time::Duration::from_millis(700));
        }
    }

    #[test]
    fn every_cue_renders() {
        for cue in [Cue::Start, Cue::Stop, Cue::Done, Cue::Error] {
            let wav = render_wav(segments(cue), DEFAULT_VOLUME);
            assert!(wav.len() > 44, "{cue:?} produced no samples");
        }
    }
}
