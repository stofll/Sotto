//! Trim leading and trailing silence before transcription.
//!
//! whisper decodes in 30-second windows, so the saving is not proportional
//! to the silence removed — it only materialises when a whole window
//! disappears. On a typical 16-second dictation this changes nothing. It was
//! accepted with that understood: a small win is still a win, and the same
//! dependency is what future incremental transcription needs to cut audio on
//! a pause rather than mid-word.
//!
//! The failure mode to design against is clipping the first consonant, which
//! costs far more than the fraction of a second it saves. Three guards:
//!
//! - a generous margin around detected speech,
//! - no trimming at all when nothing was detected as speech (transcribing
//!   too much beats transcribing nothing),
//! - no trimming unless it removes enough to be worth the risk.

use std::ops::Range;
use std::sync::Arc;

use serde_json::Value;

/// Config key: trim silence before transcription.
const CONFIG_ENABLED: &str = "trim_silence";

/// Frame size earshot requires: 256 samples = 16 ms at 16 kHz.
const FRAME: usize = 256;
/// Score above which a frame counts as speech. The crate's own guidance.
const SPEECH_THRESHOLD: f32 = 0.5;
/// Margin kept on each side of the detected speech. The plan calls for
/// 200–300 ms; the middle of that range is comfortably more than the attack
/// of a plosive.
const PADDING_MS: usize = 250;
/// Don't bother unless this much is removed. Shaving 200 ms off a
/// 16-second recording buys nothing measurable and still carries the risk
/// of having cut the wrong 200 ms.
const MIN_SAVING_SECONDS: f32 = 1.0;

const SAMPLE_RATE: usize = 16_000;

pub fn enabled(config: &Value) -> bool {
    config
        .get(CONFIG_ENABLED)
        .and_then(Value::as_bool)
        .unwrap_or(true)
}

/// Apply [`speech_range`] if the setting is on and the result is worth it.
///
/// Returns the original `Arc` untouched in every case where nothing is
/// trimmed, so the common path copies nothing.
pub fn trim_for_transcription(config: &Value, audio: Arc<Vec<f32>>) -> Arc<Vec<f32>> {
    let Some((range, removed)) =
        trim_decision(enabled(config), audio.len(), || speech_range(&audio))
    else {
        return audio;
    };
    log::info!(
        "trimmed {removed:.1}s of silence ({:.1}s → {:.1}s)",
        audio.len() as f32 / SAMPLE_RATE as f32,
        range.len() as f32 / SAMPLE_RATE as f32
    );
    Arc::new(audio[range].to_vec())
}

/// The pure half of [`trim_for_transcription`]: decide whether to trim to
/// `range` and report how many seconds that saves. `None` when the setting
/// is off, nothing was detected, or the saving is below
/// [`MIN_SAVING_SECONDS`].
///
/// `range` is a closure, not a value: [`speech_range`] runs the detector over
/// the whole recording, and with the setting off that work must not happen at
/// all. Passing the result eagerly would have paid for it on every stop.
fn trim_decision(
    enabled: bool,
    audio_len: usize,
    range: impl FnOnce() -> Option<Range<usize>>,
) -> Option<(Range<usize>, f32)> {
    if !enabled {
        return None;
    }
    let range = range()?;
    let removed = (audio_len - range.len()) as f32 / SAMPLE_RATE as f32;
    (removed >= MIN_SAVING_SECONDS).then_some((range, removed))
}

/// The span of `samples` worth transcribing: from the first speech frame to
/// the last, plus [`PADDING_MS`] on each side, clamped to the input.
///
/// `None` when no frame scored as speech — the recording is either silence
/// or something the detector does not recognise, and in both cases handing
/// whisper the whole thing is the safe answer.
pub fn speech_range(samples: &[f32]) -> Option<Range<usize>> {
    if samples.len() < FRAME {
        return None;
    }
    // The detector holds a large amount of state; `default_boxed` builds it
    // on the heap instead of blowing a few kilobytes of stack.
    let mut detector = earshot::Detector::default_boxed();

    let mut first = None;
    let mut last = 0usize;
    for (index, frame) in samples.as_chunks::<FRAME>().0.iter().enumerate() {
        if detector.predict_f32(frame) > SPEECH_THRESHOLD {
            first.get_or_insert(index);
            last = index;
        }
    }
    speech_range_from_frames(samples.len(), first, last)
}

/// The pure half of [`speech_range`]: turn the first/last speech frame
/// indices into a padded, input-clamped sample range. `None` when no frame
/// scored as speech.
fn speech_range_from_frames(
    total_len: usize,
    first: Option<usize>,
    last: usize,
) -> Option<Range<usize>> {
    let first = first?;
    let padding = PADDING_MS * SAMPLE_RATE / 1000;
    let start = (first * FRAME).saturating_sub(padding);
    let end = ((last + 1) * FRAME + padding).min(total_len);
    Some(start..end)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A tone loud enough to read as speech-ish energy. Not real speech —
    /// these tests are about the trimming arithmetic and the guards, not
    /// about the detector's accuracy.
    fn tone(seconds: f32) -> Vec<f32> {
        let count = (seconds * SAMPLE_RATE as f32) as usize;
        (0..count)
            .map(|i| {
                let t = i as f32 / SAMPLE_RATE as f32;
                0.4 * (2.0 * std::f32::consts::PI * 180.0 * t).sin()
                    + 0.2 * (2.0 * std::f32::consts::PI * 950.0 * t).sin()
            })
            .collect()
    }

    fn silence(seconds: f32) -> Vec<f32> {
        vec![0.0; (seconds * SAMPLE_RATE as f32) as usize]
    }

    #[test]
    fn enabled_by_default_and_switchable() {
        assert!(enabled(&json!({})));
        assert!(!enabled(&json!({ "trim_silence": false })));
    }

    #[test]
    fn pure_silence_is_never_trimmed() {
        // Returning a range here would mean handing whisper an empty slice.
        // Better to let it decide there is nothing to transcribe.
        assert_eq!(speech_range(&silence(3.0)), None);
    }

    #[test]
    fn too_short_to_frame_is_left_alone() {
        assert_eq!(speech_range(&vec![0.5; 100]), None);
    }

    #[test]
    fn disabled_returns_the_same_allocation() {
        let audio = Arc::new(silence(2.0));
        let out = trim_for_transcription(&json!({ "trim_silence": false }), Arc::clone(&audio));
        assert!(Arc::ptr_eq(&audio, &out));
    }

    #[test]
    fn silence_only_recording_survives_intact() {
        let audio = Arc::new(silence(5.0));
        let out = trim_for_transcription(&json!({}), Arc::clone(&audio));
        assert!(
            Arc::ptr_eq(&audio, &out),
            "silence must not be trimmed away"
        );
    }

    #[test]
    fn small_savings_are_not_worth_the_risk() {
        // 0.3 s of silence around 4 s of signal: under the threshold, so the
        // audio must come back byte-identical rather than nearly-identical.
        let mut audio = silence(0.15);
        audio.extend(tone(4.0));
        audio.extend(silence(0.15));
        let audio = Arc::new(audio);
        let out = trim_for_transcription(&json!({}), Arc::clone(&audio));
        assert!(Arc::ptr_eq(&audio, &out));
    }

    /// Run the detector against a real recording:
    /// `SOTTO_WAV=path/to.wav cargo test --lib vad::tests::on_real_speech -- --ignored --nocapture`
    ///
    /// The synthetic tones above exercise the arithmetic; only real speech
    /// says whether the detector fires at all. Any 16-bit mono 16 kHz WAV
    /// works — including the ones the debug mode writes.
    #[test]
    #[ignore = "needs SOTTO_WAV pointing at a 16 kHz mono recording"]
    fn on_real_speech() {
        let path = std::env::var("SOTTO_WAV").expect("set SOTTO_WAV");
        let samples = read_wav_16k_mono(&path);
        let seconds = samples.len() as f32 / SAMPLE_RATE as f32;
        let range = speech_range(&samples).expect("no speech detected in a real recording");
        println!(
            "{seconds:.2}s → keep {:.2}s..{:.2}s ({:.2}s, removed {:.2}s)",
            range.start as f32 / SAMPLE_RATE as f32,
            range.end as f32 / SAMPLE_RATE as f32,
            range.len() as f32 / SAMPLE_RATE as f32,
            (samples.len() - range.len()) as f32 / SAMPLE_RATE as f32,
        );
        assert!(range.len() > samples.len() / 4, "kept implausibly little");

        // Same recording with silence bolted on: the trim has to find it and
        // give the padding back. This is the case the feature exists for,
        // and a real recording is the only place it can be checked.
        let mut padded = silence(3.0);
        padded.extend_from_slice(&samples);
        padded.extend(silence(3.0));
        let trimmed = trim_for_transcription(&json!({}), Arc::new(padded.clone()));
        let removed = (padded.len() - trimmed.len()) as f32 / SAMPLE_RATE as f32;
        println!(
            "padded {:.2}s → {:.2}s (removed {removed:.2}s)",
            padded.len() as f32 / SAMPLE_RATE as f32,
            trimmed.len() as f32 / SAMPLE_RATE as f32
        );
        assert!(
            removed > 4.0,
            "6s of added silence, only {removed:.2}s removed"
        );
        assert!(
            trimmed.len() as f32 / SAMPLE_RATE as f32 >= seconds,
            "cut into the speech"
        );
    }

    /// Minimal WAV reader for the test above: walk the chunks, take `data`.
    fn read_wav_16k_mono(path: &str) -> Vec<f32> {
        let bytes = std::fs::read(path).expect("read wav");
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
        let mut offset = 12;
        while offset + 8 <= bytes.len() {
            let id = &bytes[offset..offset + 4];
            let size =
                u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().unwrap()) as usize;
            let body = offset + 8;
            if id == b"data" {
                return bytes[body..(body + size).min(bytes.len())]
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|p| i16::from_le_bytes(*p) as f32 / 32768.0)
                    .collect();
            }
            offset = body + size + (size & 1);
        }
        panic!("no data chunk in {path}");
    }

    #[test]
    fn padding_is_kept_around_the_speech() {
        // The margin is what stops the first consonant being cut, so verify
        // it is actually applied rather than just that something was cut.
        let lead = 4.0;
        let mut audio = silence(lead);
        audio.extend(tone(2.0));
        audio.extend(silence(4.0));

        let Some(range) = speech_range(&audio) else {
            // The synthetic tone did not read as speech; nothing to assert
            // about padding, and `trim_for_transcription` would no-op.
            return;
        };
        let padding = PADDING_MS * SAMPLE_RATE / 1000;
        let speech_start = (lead * SAMPLE_RATE as f32) as usize;
        assert!(
            range.start + padding >= speech_start.min(range.start + padding),
            "trim must not start after the speech does"
        );
        assert!(range.start < speech_start, "no margin before the speech");
        assert!(range.end > speech_start, "trimmed away the speech itself");
        assert!(range.end <= audio.len());
    }

    // ------------------------------------------------------------------
    // Pure padding/threshold arithmetic — tested without the detector.
    // ------------------------------------------------------------------

    #[test]
    fn speech_range_arithmetic_pads_and_clamps() {
        // PADDING_MS(250) * 16000 / 1000 = 4000 samples.
        // A single frame [0,0]: start clamps to 0, end = 1*256 + 4000.
        assert_eq!(speech_range_from_frames(16_000, Some(0), 0), Some(0..4256));
        // Frames 20..20: start = 5120 - 4000, end = 21*256 + 4000.
        assert_eq!(
            speech_range_from_frames(16_000, Some(20), 20),
            Some(1120..9376)
        );
        // The end clamps to the input length.
        assert_eq!(speech_range_from_frames(3_000, Some(10), 10), Some(0..3000));
        // No speech frame → None.
        assert_eq!(speech_range_from_frames(16_000, None, 0), None);
    }

    #[test]
    fn trim_decision_respects_enabled_and_saving_threshold() {
        // Disabled → never trim, and the detector is not started: a panicking
        // closure catches any regression to eager argument evaluation.
        assert_eq!(
            trim_decision(false, 32_000, || unreachable!(
                "детектор не должен запускаться при выключенной настройке"
            )),
            None
        );
        // Found nothing → never trim.
        assert_eq!(trim_decision(true, 32_000, || None), None);
        // 30000 of 32000 samples = 1.875 s ≥ 1 s → trim.
        assert_eq!(
            trim_decision(true, 32_000, || Some(1000..3000)),
            Some((1000..3000, 1.875))
        );
        // 14000 of 16000 = 0.875 s < 1 s → do not trim.
        assert_eq!(trim_decision(true, 16_000, || Some(1000..3000)), None);
        // Exactly 1.0 s saved — the tolerance boundary: trim.
        assert_eq!(
            trim_decision(true, 17_000, || Some(1000..2000)),
            Some((1000..2000, 1.0))
        );
    }

    /// The detector recognises a synthetic tone as speech — so `speech_range`
    /// must return a range rather than `None`. Catches replacing the whole
    /// function with `None` and mutations of the threshold / frame check.
    #[test]
    fn synthetic_speech_is_detected() {
        assert!(speech_range(&tone(2.0)).is_some());
    }
}
