//! Detect speech for silence trimming and estimated dictation time.
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
/// Keep pauses up to one second as part of a spoken phrase.
const MAX_SHORT_PAUSE_SAMPLES: usize = 16_000;
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

/// Estimated active speech for dictation statistics.
#[derive(Debug)]
pub enum SpeechTiming {
    Ready(Option<f64>),
    /// Detector pass running beside inference, so statistics never add to
    /// the wait for text.
    Running(std::thread::JoinHandle<Option<f64>>),
}

impl SpeechTiming {
    /// Collect the estimate once inference is done. By then a running pass
    /// has normally finished; a panicked one counts as no detection.
    pub fn resolve(self) -> Option<f64> {
        match self {
            Self::Ready(seconds) => seconds,
            Self::Running(handle) => handle.join().unwrap_or_else(|_| {
                log::warn!("speech analysis panicked");
                None
            }),
        }
    }

    fn start(audio: &Arc<Vec<f32>>) -> Self {
        let samples = Arc::clone(audio);
        std::thread::Builder::new()
            .name("speech-timing".into())
            .spawn(move || analyze_speech(&samples).map(|analysis| analysis.active_seconds))
            .map(Self::Running)
            .unwrap_or_else(|error| {
                log::warn!("speech analysis thread unavailable: {error}");
                Self::Ready(analyze_speech(audio).map(|analysis| analysis.active_seconds))
            })
    }
}

/// Analyze speech and optionally trim its outer bounds.
///
/// Without trimming nothing waits for the detector: timing runs on its own
/// thread while the engine transcribes.
/// Returns the original `Arc` untouched in every case where nothing is
/// trimmed, so the common path copies nothing.
pub fn prepare_dictation(
    config: Option<&Value>,
    audio: Arc<Vec<f32>>,
) -> (Arc<Vec<f32>>, SpeechTiming) {
    if !config.is_some_and(enabled) {
        let timing = SpeechTiming::start(&audio);
        return (audio, timing);
    }
    // One detector pass serves both metrics and trimming. Timing is independent
    // of the trim preference, and never removes internal pauses from STT audio.
    let analysis = analyze_speech(&audio);
    let timing = SpeechTiming::Ready(analysis.as_ref().map(|value| value.active_seconds));
    let Some((range, removed)) = trim_decision(audio.len(), analysis.map(|value| value.range))
    else {
        return (audio, timing);
    };
    log::info!(
        "trimmed {removed:.1}s of silence ({:.1}s → {:.1}s)",
        audio.len() as f32 / SAMPLE_RATE as f32,
        range.len() as f32 / SAMPLE_RATE as f32
    );
    (Arc::new(audio[range].to_vec()), timing)
}

/// The pure half of [`prepare_dictation`]: decide whether to trim to
/// `range` and report how many seconds that saves. `None` when nothing was
/// detected or the saving is below [`MIN_SAVING_SECONDS`].
fn trim_decision(audio_len: usize, range: Option<Range<usize>>) -> Option<(Range<usize>, f32)> {
    let range = range?;
    let removed = (audio_len - range.len()) as f32 / SAMPLE_RATE as f32;
    (removed >= MIN_SAVING_SECONDS).then_some((range, removed))
}

/// Outer bounds for STT and duration of phrases with short pauses for statistics.
struct SpeechAnalysis {
    range: Range<usize>,
    active_seconds: f64,
}

fn analyze_speech(samples: &[f32]) -> Option<SpeechAnalysis> {
    let started = std::time::Instant::now();
    if samples.len() < FRAME {
        return None;
    }
    // The detector holds a large amount of state; `default_boxed` builds it
    // on the heap instead of blowing a few kilobytes of stack.
    let mut detector = earshot::Detector::default_boxed();

    let frames = samples
        .as_chunks::<FRAME>()
        .0
        .iter()
        .enumerate()
        .filter_map(|(index, frame)| {
            (detector.predict_f32(frame) > SPEECH_THRESHOLD).then_some(index)
        });
    let analysis = analyze_speech_frames(samples.len(), frames);
    log::debug!(
        "speech analysis: audio_seconds={:.3} elapsed_ms={:.3}",
        samples.len() as f64 / SAMPLE_RATE as f64,
        started.elapsed().as_secs_f64() * 1000.0
    );
    analysis
}

fn analyze_speech_frames(
    total_len: usize,
    mut frames: impl Iterator<Item = usize>,
) -> Option<SpeechAnalysis> {
    let first = frames.next()?;
    let mut last = first;
    let mut phrase_start = first;
    let mut active_samples = 0;
    for frame in frames {
        if (frame - last - 1) * FRAME > MAX_SHORT_PAUSE_SAMPLES {
            active_samples += speech_range_from_frames(total_len, phrase_start, last).len();
            phrase_start = frame;
        }
        last = frame;
    }
    active_samples += speech_range_from_frames(total_len, phrase_start, last).len();
    Some(SpeechAnalysis {
        range: speech_range_from_frames(total_len, first, last),
        active_seconds: active_samples as f64 / SAMPLE_RATE as f64,
    })
}

/// Turn the first/last speech frame indices into a padded, input-clamped
/// sample range.
fn speech_range_from_frames(total_len: usize, first: usize, last: usize) -> Range<usize> {
    let padding = PADDING_MS * SAMPLE_RATE / 1000;
    let start = (first * FRAME).saturating_sub(padding);
    let end = ((last + 1) * FRAME + padding).min(total_len);
    start..end
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

    fn speech_range(samples: &[f32]) -> Option<Range<usize>> {
        analyze_speech(samples).map(|value| value.range)
    }

    fn trim_for_test(config: &Value, audio: Arc<Vec<f32>>) -> Arc<Vec<f32>> {
        prepare_dictation(Some(config), audio).0
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
        let out = trim_for_test(&json!({ "trim_silence": false }), Arc::clone(&audio));
        assert!(Arc::ptr_eq(&audio, &out));
    }

    #[test]
    fn silence_only_recording_survives_intact() {
        let audio = Arc::new(silence(5.0));
        let out = trim_for_test(&json!({}), Arc::clone(&audio));
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
        let out = trim_for_test(&json!({}), Arc::clone(&audio));
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
        let trimmed = trim_for_test(&json!({}), Arc::new(padded.clone()));
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

        let baseline = analyze_speech(&samples).unwrap().active_seconds;
        let mut spaced = samples.clone();
        spaced.extend(silence(4.0));
        spaced.extend_from_slice(&samples);
        let spaced_timing = analyze_speech(&spaced).unwrap().active_seconds;
        println!(
            "speech timing: original {baseline:.2}s, repeated with 4s pause {spaced_timing:.2}s"
        );
        assert!(
            (spaced_timing - baseline * 2.0).abs() < 1.0,
            "long internal pause must not count as speech"
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
            // about padding, and `prepare_dictation` would leave audio intact.
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
        assert_eq!(speech_range_from_frames(16_000, 0, 0), 0..4256);
        // Frames 20..20: start = 5120 - 4000, end = 21*256 + 4000.
        assert_eq!(speech_range_from_frames(16_000, 20, 20), 1120..9376);
        // The end clamps to the input length.
        assert_eq!(speech_range_from_frames(3_000, 10, 10), 0..3000);
    }

    #[test]
    fn trim_decision_respects_saving_threshold() {
        // Found nothing → never trim.
        assert_eq!(trim_decision(32_000, None), None);
        // 30000 of 32000 samples = 1.875 s ≥ 1 s → trim.
        assert_eq!(
            trim_decision(32_000, Some(1000..3000)),
            Some((1000..3000, 1.875))
        );
        // 14000 of 16000 = 0.875 s < 1 s → do not trim.
        assert_eq!(trim_decision(16_000, Some(1000..3000)), None);
        // Exactly 1.0 s saved — the tolerance boundary: trim.
        assert_eq!(
            trim_decision(17_000, Some(1000..2000)),
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

    #[test]
    fn timing_keeps_short_pauses_and_excludes_long_ones_with_margins() {
        // 1.024 s speech, 0.992 s pause, 1.024 s speech: one phrase.
        let short = analyze_speech_frames(48_640, (0..64).chain(126..190)).unwrap();
        assert_eq!(short.active_seconds, 3.04);
        // 1.024 s speech, 3.008 s pause, 1.024 s speech: two phrases,
        // retaining 250 ms on each side of the internal pause.
        let long = analyze_speech_frames(80_896, (0..64).chain(252..316)).unwrap();
        assert_eq!(long.range, 0..80_896);
        assert_eq!(long.active_seconds, 2.548);
    }

    #[test]
    fn timing_preserves_boundary_speech_and_requires_detection() {
        assert!(analyze_speech_frames(16_000, std::iter::empty()).is_none());
        let full = analyze_speech_frames(16_000, 0..62).unwrap();
        assert_eq!(full.active_seconds, 1.0);
        let single = analyze_speech_frames(16_000, std::iter::once(30)).unwrap();
        assert_eq!(single.active_seconds, 0.516);
        assert!(analyze_speech(&silence(3.0)).is_none());
    }

    #[test]
    fn timing_does_not_cut_internal_audio_or_depend_on_trim_preference() {
        let mut samples = tone(2.0);
        samples.extend(silence(3.0));
        samples.extend(tone(2.0));
        let audio = Arc::new(samples);
        let (untrimmed, timing) =
            prepare_dictation(Some(&json!({"trim_silence": false})), Arc::clone(&audio));
        assert!(Arc::ptr_eq(&audio, &untrimmed));
        assert!(matches!(timing, SpeechTiming::Running(_)));
        let timing = timing.resolve().expect("synthetic speech detected");
        assert!(
            timing > 2.0 && timing < 6.0,
            "unexpected active duration: {timing}"
        );
        let (trimmed, trimmed_timing) = prepare_dictation(Some(&json!({})), audio);
        assert!(matches!(trimmed_timing, SpeechTiming::Ready(Some(value)) if value == timing));
        assert!(trimmed.len() as f64 / SAMPLE_RATE as f64 >= timing + 1.0);
    }

    #[test]
    fn unavailable_config_times_speech_beside_inference_and_file_timing_stays_absent() {
        let audio = Arc::new(tone(2.0));
        let (prepared, timing) = prepare_dictation(None, Arc::clone(&audio));
        assert!(Arc::ptr_eq(&prepared, &audio));
        assert!(matches!(timing, SpeechTiming::Running(_)));
        assert!(timing.resolve().is_some());
        assert_eq!(SpeechTiming::Ready(None).resolve(), None);
    }

    #[test]
    #[ignore = "manual VAD latency measurement; run optimized with --release --nocapture"]
    fn analysis_latency() {
        for seconds in [60, 600] {
            for (name, audio) in [
                ("silence", silence(seconds as f32)),
                ("tone", tone(seconds as f32)),
            ] {
                let mut timings = Vec::new();
                for _ in 0..5 {
                    let started = std::time::Instant::now();
                    std::hint::black_box(analyze_speech(std::hint::black_box(&audio)));
                    timings.push(started.elapsed());
                }
                timings.sort();
                println!(
                    "VAD {name} {seconds}s: median {:.3}ms",
                    timings[2].as_secs_f64() * 1000.0
                );
            }
        }
    }
}
