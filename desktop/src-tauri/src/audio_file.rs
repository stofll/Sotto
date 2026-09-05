//! Decoding an audio file on disk into the one buffer shape the engine
//! accepts: 16 kHz, mono, `f32`.
//!
//! The microphone path never needed this — cpal hands us raw PCM at a rate
//! we chose, and `audio.rs` decimates it with fixed integer ratios. A file
//! arrives in whatever container, codec, channel count and sample rate the
//! person who recorded it happened to use, so this module owns the three
//! steps that path never had: demux + decode (symphonia), downmix to mono,
//! and arbitrary-ratio resampling (rubato).
//!
//! Everything here is synchronous and CPU-bound. Callers run it on
//! `spawn_blocking` — decoding an hour of MP3 on the async runtime's
//! thread would stall every other task.

use std::path::Path;

use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Async, FixedAsync, Indexing, Resampler, SincInterpolationParameters};
use symphonia::core::audio::GenericAudioBufferRef;
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

/// The rate the whisper/sherpa engines expect. Not configurable: whisper.cpp
/// is trained at 16 kHz and resamples internally (badly) if given anything
/// else.
pub const TARGET_RATE: u32 = 16_000;

/// Longest file we will decode, in seconds.
///
/// This is a memory guard, not a policy: the decoded buffer is `f32`, so
/// three hours is 3 × 3600 × 16000 × 4 B ≈ 691 MB — and the source buffer
/// at the file's own rate exists alongside it for part of the run. Beyond
/// this the app is a likelier cause of the user's next out-of-memory than
/// whatever they were transcribing.
pub const MAX_DURATION_SECONDS: f64 = 3.0 * 3600.0;

/// Chunk size fed to the resampler. Large enough that the per-call overhead
/// is irrelevant, small enough that the scratch buffers stay in cache.
const RESAMPLE_CHUNK: usize = 1024;

#[derive(Debug, Clone)]
pub struct DecodedAudio {
    /// 16 kHz mono, ready for `EngineCommand::Transcribe`.
    pub samples: Vec<f32>,
    /// Duration of `samples`, in seconds. Derived from the sample count
    /// rather than from container metadata: the metadata is what a
    /// truncated or mis-muxed file lies about, and this number ends up in
    /// the LLM duration gate.
    pub audio_seconds: f64,
}

/// Decode `path` into 16 kHz mono `f32`.
///
/// Errors are localized and meant to be shown verbatim — the caller has no
/// more context to add, and symphonia's own messages ("unsupported codec")
/// tell a person nothing about which of their files is the problem.
pub fn decode_to_pcm16k_mono(path: &Path) -> Result<DecodedAudio, String> {
    decode_with_limit(path, MAX_DURATION_SECONDS)
}

/// The body of [`decode_to_pcm16k_mono`], with the duration cap injected.
///
/// The cap exists to stop a 10-hour file from exhausting memory, and the
/// only honest test of it would need a 10-hour file. Taking the limit as an
/// argument lets a test use a fraction of a second instead — the guard is
/// the same code either way.
fn decode_with_limit(path: &Path, max_seconds: f64) -> Result<DecodedAudio, String> {
    let file = std::fs::File::open(path).map_err(|e| {
        crate::ui_text::t("Не удалось открыть файл: {p0}").replace("{p0}", &e.to_string())
    })?;

    // The extension is a hint, not a decision: symphonia probes the actual
    // bytes and will happily decode an .mp3 that is really a .m4a. The hint
    // only saves it some guessing.
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let stream = MediaSourceStream::new(Box::new(file), Default::default());
    let mut format = symphonia::default::get_probe()
        .probe(
            &hint,
            stream,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|e| describe_symphonia_error(&e))?;

    // A video file (or a multi-track recording) has tracks we must not feed
    // to an audio decoder; `default_track` picks the one the container
    // itself marks as the audio track to play.
    let track = format
        .default_track(TrackType::Audio)
        .ok_or_else(|| crate::ui_text::t("В файле нет звуковой дорожки."))?;
    let track_id = track.id;
    let codec_params = track
        .codec_params
        .as_ref()
        .and_then(|params| params.audio())
        .ok_or_else(|| crate::ui_text::t("В файле нет звуковой дорожки."))?
        .clone();

    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(&codec_params, &AudioDecoderOptions::default())
        .map_err(|e| describe_symphonia_error(&e))?;

    let mut mono = Vec::<f32>::new();
    let mut interleaved = Vec::<f32>::new();
    // Taken from the decoded buffers, not from `codec_params`: for some
    // containers the header rate is absent or stale, and the decoder is the
    // one that knows what it actually produced.
    let mut source_rate: Option<u32> = None;

    loop {
        let packet = match format.next_packet() {
            Ok(Some(packet)) => packet,
            // End of stream.
            Ok(None) => break,
            // A truncated file ends mid-packet rather than politely. What
            // was decoded up to here is still the user's recording, so keep
            // it instead of throwing the whole transcription away.
            Err(SymphoniaError::IoError(ref e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                log::warn!("audio_file: stream ended early, keeping what decoded");
                break;
            }
            Err(e) => return Err(describe_symphonia_error(&e)),
        };

        if packet.track_id != track_id {
            continue;
        }

        let decoded = match decoder.decode(&packet) {
            Ok(decoded) => decoded,
            // Both are per-packet conditions the symphonia docs call
            // recoverable: skip the packet, keep the stream. Dropping a
            // frame costs ~20 ms of audio; aborting costs the whole file.
            Err(SymphoniaError::DecodeError(msg)) => {
                log::warn!("audio_file: skipping malformed packet: {msg}");
                continue;
            }
            Err(SymphoniaError::ResetRequired) => {
                decoder.reset();
                continue;
            }
            Err(e) => return Err(describe_symphonia_error(&e)),
        };

        append_downmixed(&decoded, &mut interleaved, &mut mono);
        let rate = *source_rate.get_or_insert(decoded.spec().rate());

        // Checked inside the loop, not from the container's duration
        // field, so the guard holds for a file whose header understates its
        // length. Bailing here also means we never allocate the full buffer
        // for a 10-hour file before noticing.
        if rate > 0 && mono.len() as f64 / f64::from(rate) > max_seconds {
            return Err(crate::ui_text::t("Файл длиннее {p0} часов.")
                .replace("{p0}", &format!("{:.0}", max_seconds / 3600.0)));
        }
    }

    // Checked before the rate, and it is the check that fires: a file whose
    // header parses but whose data chunk is empty yields no packets, so
    // there is no rate either. Ordering them the other way round reports
    // "corrupt" for a file that is merely empty, and leaves this branch
    // unreachable — which is how it read before a mutation test caught it.
    if mono.is_empty() {
        return Err(crate::ui_text::t("В файле нет звука."));
    }

    // Reached only when packets decoded but none carried a usable rate.
    let source_rate = source_rate.filter(|rate| *rate > 0).ok_or_else(|| {
        crate::ui_text::t("Не удалось прочитать звук из файла — возможно, он повреждён.")
    })?;

    let samples = if source_rate == TARGET_RATE {
        mono
    } else {
        resample_to_target(&mono, source_rate)?
    };

    Ok(DecodedAudio {
        audio_seconds: samples.len() as f64 / f64::from(TARGET_RATE),
        samples,
    })
}

/// Average all channels of one decoded buffer into `mono`.
///
/// Averaging rather than taking the first channel: a stereo interview with
/// one speaker per channel loses a speaker outright if you pick a side, and
/// a mid/side-ish recording can leave you with the quiet one.
///
/// `interleaved` is a scratch buffer owned by the caller so the per-packet
/// allocation happens once per file rather than once per packet.
fn append_downmixed(
    decoded: &GenericAudioBufferRef<'_>,
    interleaved: &mut Vec<f32>,
    mono: &mut Vec<f32>,
) {
    let channels = decoded.spec().channels().count();
    if channels == 0 {
        return;
    }
    decoded.copy_to_vec_interleaved(interleaved);
    mono.reserve(interleaved.len() / channels);
    for frame in interleaved.chunks_exact(channels) {
        mono.push(frame.iter().sum::<f32>() / channels as f32);
    }
}

/// Resample mono `samples` from `source_rate` to [`TARGET_RATE`].
///
/// Sinc interpolation rather than linear: every common source rate here is
/// a downsample (44.1 or 48 kHz → 16 kHz), and downsampling without an
/// anti-aliasing filter folds everything above 8 kHz back into the speech
/// band as noise — which whisper hears as words. rubato scales the sinc
/// cutoff by the ratio automatically when the ratio is below 1, so the
/// filter is correct for the direction we always go.
fn resample_to_target(samples: &[f32], source_rate: u32) -> Result<Vec<f32>, String> {
    let ratio = f64::from(TARGET_RATE) / f64::from(source_rate);
    let params = SincInterpolationParameters::default();
    let mut resampler = Async::<f32>::new_sinc(
        ratio,
        // The ratio is fixed for the whole file; no headroom needed.
        1.0,
        &params,
        RESAMPLE_CHUNK,
        1,
        FixedAsync::Input,
    )
    .map_err(|e| {
        log::error!("audio_file: resampler construction failed: {e}");
        crate::ui_text::t("Не удалось преобразовать частоту дискретизации файла.")
    })?;

    // The exact length the whole conversion should produce. Used to trim
    // the tail below: the flush pushes out more frames than the signal
    // actually contains, and a few hundred samples of filter ring at the
    // end is not audio.
    let expected_len = (samples.len() as f64 * ratio).round() as usize;
    // The filter's group delay shows up as leading samples that belong
    // before the start of the signal. Dropping them keeps the output
    // aligned with the input, which is what makes `audio_seconds` and any
    // future timestamps mean the same thing as in the source file.
    let delay = resampler.output_delay();

    let mut out = Vec::<f32>::with_capacity(expected_len + resampler.output_frames_max());
    let mut out_chunk = vec![0.0f32; resampler.output_frames_max()];
    let mut in_chunk = vec![0.0f32; resampler.input_frames_next()];
    let mut pos = 0usize;

    // One pass over the input, then feed silence until the delayed tail has
    // been pushed out. `expected_len + delay` is the point past which every
    // remaining output frame is filter ring.
    while out.len() < expected_len + delay {
        let needed = resampler.input_frames_next();
        if in_chunk.len() < needed {
            in_chunk.resize(needed, 0.0);
        }
        let available = samples.len().saturating_sub(pos);
        let mut indexing = Indexing::new();

        if available >= needed {
            in_chunk[..needed].copy_from_slice(&samples[pos..pos + needed]);
            pos += needed;
        } else {
            // Final partial chunk, or a pure-silence flush chunk once the
            // input is exhausted. Either way the buffer must still be
            // `needed` frames long; `partial_len` tells the resampler how
            // much of it is real.
            in_chunk[..available].copy_from_slice(&samples[pos..]);
            in_chunk[available..needed].fill(0.0);
            pos = samples.len();
            indexing.partial_len = Some(available);
        }

        let input = InterleavedSlice::new(&in_chunk[..needed], 1, needed)
            .map_err(|e| format!("resample input buffer: {e}"))?;
        let mut output =
            InterleavedSlice::new_mut(&mut out_chunk, 1, resampler.output_frames_max())
                .map_err(|e| format!("resample output buffer: {e}"))?;

        let (_read, written) = resampler
            .process_into_buffer(&input, &mut output, Some(&indexing))
            .map_err(|e| {
                log::error!("audio_file: resample failed: {e}");
                crate::ui_text::t("Не удалось преобразовать частоту дискретизации файла.")
            })?;

        if written == 0 {
            // Defensive: a resampler that stops producing would otherwise
            // spin here forever on the silence flush.
            break;
        }
        out.extend_from_slice(&out_chunk[..written]);
    }

    // Drop the leading group delay, then cut to the true length.
    let start = delay.min(out.len());
    out.drain(..start);
    out.truncate(expected_len);
    Ok(out)
}

/// Turn a symphonia error into something a person can act on.
///
/// The distinction that matters to a user is "this file is broken" vs
/// "this app cannot read this kind of file" — the first means try another
/// copy, the second means convert it. symphonia's own strings blur the two.
fn describe_symphonia_error(error: &SymphoniaError) -> String {
    match error {
        SymphoniaError::Unsupported(what) => {
            log::warn!("audio_file: unsupported: {what}");
            crate::ui_text::t(
                "Этот формат не поддерживается. Сконвертируйте файл в wav, mp3 или m4a.",
            )
        }
        SymphoniaError::DecodeError(what) => {
            log::warn!("audio_file: decode error: {what}");
            crate::ui_text::t("Не удалось прочитать звук из файла — возможно, он повреждён.")
        }
        SymphoniaError::LimitError(what) => {
            log::warn!("audio_file: limit reached: {what}");
            crate::ui_text::t("Не удалось прочитать звук из файла — возможно, он повреждён.")
        }
        SymphoniaError::IoError(e) => {
            log::warn!("audio_file: io error: {e}");
            crate::ui_text::t("Не удалось открыть файл: {p0}").replace("{p0}", &e.to_string())
        }
        other => {
            log::warn!("audio_file: {other}");
            crate::ui_text::t("Не удалось прочитать звук из файла — возможно, он повреждён.")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A WAV file built in memory, so the tests do not depend on committed
    /// binary fixtures for the one format we can encode ourselves.
    fn wav_bytes(samples: &[f32], rate: u32, channels: u16) -> Vec<u8> {
        let pcm = crate::wav::f32_to_pcm16(samples);
        if channels == 1 {
            return crate::wav::encode_pcm16_mono(&pcm, rate);
        }
        // `wav.rs` only encodes mono, so patch the header for the
        // multi-channel cases: channel count and the two derived rate
        // fields. Interleaving is the caller's job.
        let mut bytes = crate::wav::encode_pcm16_mono(&pcm, rate);
        bytes[22..24].copy_from_slice(&channels.to_le_bytes());
        let byte_rate = rate * u32::from(channels) * 2;
        bytes[28..32].copy_from_slice(&byte_rate.to_le_bytes());
        let block_align = channels * 2;
        bytes[32..34].copy_from_slice(&block_align.to_le_bytes());
        bytes
    }

    fn write_temp(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("sotto-audio-file-tests");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    fn sine(rate: u32, seconds: f64, freq: f64) -> Vec<f32> {
        let count = (rate as f64 * seconds) as usize;
        (0..count)
            .map(|i| {
                let t = i as f64 / f64::from(rate);
                (std::f64::consts::TAU * freq * t).sin() as f32 * 0.5
            })
            .collect()
    }

    #[test]
    fn decodes_16k_mono_wav_unchanged_in_length() {
        let samples = sine(16_000, 1.0, 440.0);
        let path = write_temp("mono16k.wav", &wav_bytes(&samples, 16_000, 1));

        let decoded = decode_to_pcm16k_mono(&path).expect("wav must decode");

        assert_eq!(
            decoded.samples.len(),
            16_000,
            "a 1 s 16 kHz file must stay 16000 samples — no resampling should happen"
        );
        assert!(
            (decoded.audio_seconds - 1.0).abs() < 0.01,
            "audio_seconds must follow the sample count, got {}",
            decoded.audio_seconds
        );
    }

    #[test]
    fn resamples_44100_to_16000() {
        let samples = sine(44_100, 1.0, 440.0);
        let path = write_temp("mono44k.wav", &wav_bytes(&samples, 44_100, 1));

        let decoded = decode_to_pcm16k_mono(&path).expect("44.1 kHz wav must decode");

        // The whole point of the module: whatever came in, 16 kHz comes out.
        assert!(
            (decoded.samples.len() as i64 - 16_000).abs() <= 2,
            "1 s at 44.1 kHz must resample to ~16000 samples, got {}",
            decoded.samples.len()
        );
        assert!(
            (decoded.audio_seconds - 1.0).abs() < 0.01,
            "audio_seconds must be ~1 s, got {}",
            decoded.audio_seconds
        );
    }

    #[test]
    fn resampling_preserves_the_signal_not_just_the_length() {
        // A length-only assertion passes for a resampler that outputs
        // silence, or noise. Check that a 440 Hz tone is still a 440 Hz
        // tone by correlating the output against a locally generated
        // reference at the target rate.
        let path = write_temp(
            "tone44k.wav",
            &wav_bytes(&sine(44_100, 0.5, 440.0), 44_100, 1),
        );
        let decoded = decode_to_pcm16k_mono(&path).expect("tone must decode");

        let reference = sine(16_000, 0.5, 440.0);
        let n = decoded.samples.len().min(reference.len());
        // Skip the first and last 10 ms: filter edges are not the signal.
        let skip = 160;
        let dot: f64 = (skip..n - skip)
            .map(|i| f64::from(decoded.samples[i]) * f64::from(reference[i]))
            .sum();
        let energy_a: f64 = (skip..n - skip)
            .map(|i| f64::from(decoded.samples[i]).powi(2))
            .sum();
        let energy_b: f64 = (skip..n - skip)
            .map(|i| f64::from(reference[i]).powi(2))
            .sum();
        let correlation = dot / (energy_a.sqrt() * energy_b.sqrt());

        assert!(
            correlation > 0.99,
            "resampled tone must still be the same tone, correlation was {correlation}"
        );
    }

    /// Committed fixtures: 0.5 s of a 440 Hz tone at 44.1 kHz mono, one per
    /// container/codec pair, made once with ffmpeg. Regenerating them is not
    /// part of any build — they are inputs, and a test that generates its own
    /// input with the same library it is testing proves nothing.
    fn fixture(name: &str) -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/audio")
            .join(name)
    }

    /// Every compressed format we claim to support, decoded end to end.
    ///
    /// Lossy codecs are not compared sample-by-sample — the point is that
    /// the demuxer feature, the codec feature and the resampler all line up,
    /// which is exactly what a missing Cargo feature breaks. A wrong or
    /// absent feature shows up here as "unsupported format", not as a subtle
    /// quality regression.
    #[test]
    fn decodes_every_supported_container() {
        for name in [
            "tone.mp3",
            "tone.m4a",
            "tone.ogg",
            "tone.flac",
            "tone_alac.m4a",
        ] {
            let decoded = decode_to_pcm16k_mono(&fixture(name))
                .unwrap_or_else(|e| panic!("{name} must decode, got: {e}"));

            assert!(
                (decoded.audio_seconds - 0.5).abs() < 0.06,
                "{name}: 0.5 s in must be ~0.5 s out, got {}",
                decoded.audio_seconds
            );
            let peak = decoded
                .samples
                .iter()
                .fold(0.0f32, |acc, s| acc.max(s.abs()));
            assert!(
                peak > 0.1,
                "{name}: decoded to near-silence (peak {peak}) — the codec \
                 produced no signal"
            );
        }
    }

    /// The container says 44.1 kHz; the engine only accepts 16 kHz. This is
    /// the one property every fixture must share, and the one a "just pass
    /// the samples through" regression would break for real files while the
    /// synthetic WAV tests still passed.
    #[test]
    fn compressed_fixtures_come_out_at_the_target_rate() {
        for name in ["tone.mp3", "tone.m4a", "tone.ogg", "tone.flac"] {
            let decoded = decode_to_pcm16k_mono(&fixture(name))
                .unwrap_or_else(|e| panic!("{name} must decode, got: {e}"));
            let implied_rate = decoded.samples.len() as f64 / decoded.audio_seconds;

            assert!(
                (implied_rate - f64::from(TARGET_RATE)).abs() < 1.0,
                "{name}: must be resampled to {TARGET_RATE} Hz, implied {implied_rate}"
            );
            assert!(
                decoded.samples.len() < 12_000,
                "{name}: 0.5 s at 16 kHz is ~8000 samples; {} means the 44.1 kHz \
                 source was never resampled",
                decoded.samples.len()
            );
        }
    }

    #[test]
    fn refuses_a_file_longer_than_the_cap() {
        // 1 s of audio against a 0.1 s cap. The real cap is three hours; the
        // guard is the same line, and this is the only way to reach it
        // without a three-hour fixture.
        let path = write_temp("long.wav", &wav_bytes(&sine(16_000, 1.0, 440.0), 16_000, 1));

        let error = decode_with_limit(&path, 0.1).expect_err("a file over the cap must be refused");

        assert!(
            error.contains("длиннее") || error.contains("longer"),
            "the refusal must say the file is too long, got: {error}"
        );
    }

    #[test]
    fn accepts_a_file_inside_the_cap() {
        // The other half of the guard: a cap that never fires must not
        // change the result. Without this, a cap of `0.0` would pass the
        // test above and reject everything.
        let path = write_temp(
            "short.wav",
            &wav_bytes(&sine(16_000, 1.0, 440.0), 16_000, 1),
        );

        let decoded = decode_with_limit(&path, 10.0).expect("a file under the cap must decode");

        assert_eq!(decoded.samples.len(), 16_000);
    }

    #[test]
    fn downmixes_stereo_by_averaging_channels() {
        // Left is a tone, right is its exact inverse. Averaging cancels
        // them to silence; taking either channel alone would not. That
        // makes this test fail loudly if the downmix ever becomes
        // "pick channel 0".
        let tone = sine(16_000, 0.5, 440.0);
        let mut interleaved = Vec::with_capacity(tone.len() * 2);
        for sample in &tone {
            interleaved.push(*sample);
            interleaved.push(-*sample);
        }
        let path = write_temp("stereo.wav", &wav_bytes(&interleaved, 16_000, 2));

        let decoded = decode_to_pcm16k_mono(&path).expect("stereo wav must decode");

        assert_eq!(
            decoded.samples.len(),
            tone.len(),
            "downmix must halve the sample count, not the frame count"
        );
        let peak = decoded
            .samples
            .iter()
            .fold(0.0f32, |acc, s| acc.max(s.abs()));
        assert!(
            peak < 0.01,
            "L and -L must average to silence; peak was {peak}"
        );
    }

    #[test]
    fn rejects_a_file_that_is_not_audio() {
        let path = write_temp(
            "garbage.wav",
            b"this is not a wav file at all, not even close",
        );

        let error = decode_to_pcm16k_mono(&path).expect_err("garbage must not decode");

        assert!(
            error.contains("поддерживается") || error.contains("format"),
            "the error must say the format is unsupported, got: {error}"
        );
    }

    #[test]
    fn rejects_a_missing_file() {
        let path = std::env::temp_dir().join("sotto-no-such-file-4f9a.wav");
        let _ = std::fs::remove_file(&path);

        let error = decode_to_pcm16k_mono(&path).expect_err("a missing file must not decode");

        assert!(
            error.contains("открыть") || error.contains("open"),
            "the error must say the file could not be opened, got: {error}"
        );
    }

    #[test]
    fn rejects_a_silent_but_empty_stream() {
        // Zero data frames: a valid header describing no audio. The engine
        // would return an empty transcription for this, which reads to the
        // user as "the app is broken" rather than "the file is empty".
        let path = write_temp("empty.wav", &wav_bytes(&[], 16_000, 1));

        let error = decode_to_pcm16k_mono(&path).expect_err("an empty file must not decode");

        // The exact message, not just "some error": an earlier version of
        // this test asserted only that decoding failed, and passed while
        // the empty-audio guard it was meant to cover was disabled — the
        // failure came from a different branch reporting "corrupt file".
        assert_eq!(
            error,
            crate::ui_text::t("В файле нет звука."),
            "an empty file must be reported as empty, not as corrupt"
        );
    }

    #[test]
    fn audio_seconds_matches_the_sample_count_exactly() {
        // 2.5 s at 48 kHz — a rate that is not a whole multiple of 16 kHz
        // after the sinc filter's rounding, which is where an off-by-a-chunk
        // in the resample loop would show up.
        let path = write_temp("48k.wav", &wav_bytes(&sine(48_000, 2.5, 220.0), 48_000, 1));

        let decoded = decode_to_pcm16k_mono(&path).expect("48 kHz wav must decode");

        assert_eq!(
            decoded.audio_seconds,
            decoded.samples.len() as f64 / f64::from(TARGET_RATE),
            "audio_seconds must be derived from the sample count, never from metadata"
        );
        assert!(
            (decoded.audio_seconds - 2.5).abs() < 0.01,
            "2.5 s in must be 2.5 s out, got {}",
            decoded.audio_seconds
        );
    }

    #[test]
    fn max_duration_is_three_hours() {
        assert_eq!(MAX_DURATION_SECONDS, 10_800.0);
    }

    /// A truncated file is still the user's recording: whatever decoded before
    /// the break must come back rather than be lost to an error wholesale.
    #[test]
    fn truncated_file_keeps_what_decoded() {
        let mut bytes = wav_bytes(&sine(16_000, 0.5, 440.0), 16_000, 1);
        bytes.truncate(bytes.len() - 20);
        let path = write_temp("truncated.wav", &bytes);

        let decoded =
            decode_to_pcm16k_mono(&path).expect("a truncated wav must keep its partial audio");
        assert!(
            !decoded.samples.is_empty(),
            "some audio must survive a truncated tail"
        );
    }

    #[test]
    fn downmixes_identical_channels_to_the_same_signal() {
        // L == R: the average is the signal itself. `sum / channels` is caught
        // here rather than at L == -R (there the sum is zero and `*`/`%` give
        // the same zero).
        let tone = sine(16_000, 0.5, 440.0);
        let mut interleaved = Vec::with_capacity(tone.len() * 2);
        for sample in &tone {
            interleaved.push(*sample);
            interleaved.push(*sample);
        }
        let path = write_temp("stereo_same.wav", &wav_bytes(&interleaved, 16_000, 2));

        let decoded = decode_to_pcm16k_mono(&path).expect("stereo wav must decode");

        assert_eq!(decoded.samples.len(), tone.len());
        let peak = decoded
            .samples
            .iter()
            .fold(0.0f32, |acc, s| acc.max(s.abs()));
        assert!(
            (peak - 0.5).abs() < 0.1,
            "averaging identical channels must keep the 0.5-amplitude signal, peak {peak}"
        );
    }
}
