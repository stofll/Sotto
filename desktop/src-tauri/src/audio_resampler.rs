//! Stateful mono PCM resampling. The same filter processes files and microphone
//! chunks, retaining fractional positions and flushing the delayed tail once.

use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Async, FixedAsync, Indexing, Resampler, SincInterpolationParameters};

const CHUNK: usize = 1024;

pub struct AudioResampler {
    filter: Option<Async<f32>>,
    ratio: f64,
    input: Vec<f32>,
    output: Vec<f32>,
    ready: Vec<f32>,
    pending: usize,
    delay: usize,
    received: usize,
    emitted: usize,
}

impl AudioResampler {
    pub fn new(source_rate: u32, target_rate: u32) -> Result<Self, String> {
        if source_rate == 0 || target_rate == 0 {
            return Err("sample rate must be positive".into());
        }
        let ratio = f64::from(target_rate) / f64::from(source_rate);
        let filter = if source_rate == target_rate {
            None
        } else {
            Some(
                Async::<f32>::new_sinc(
                    ratio,
                    1.0,
                    &SincInterpolationParameters::default(),
                    CHUNK,
                    1,
                    FixedAsync::Input,
                )
                .map_err(|e| format!("resampler: {e}"))?,
            )
        };
        let frames = filter.as_ref().map_or(CHUNK, |f| f.output_frames_max());
        let delay = filter.as_ref().map_or(0, |f| f.output_delay());
        Ok(Self {
            filter,
            ratio,
            input: vec![0.0; CHUNK],
            output: vec![0.0; frames],
            ready: Vec::with_capacity(frames),
            pending: 0,
            delay,
            received: 0,
            emitted: 0,
        })
    }

    pub fn push(&mut self, mut samples: &[f32]) -> Result<&[f32], String> {
        self.ready.clear();
        self.received += samples.len();
        if self.filter.is_none() {
            self.ready.extend_from_slice(samples);
            self.emitted += samples.len();
            return Ok(&self.ready);
        }
        while !samples.is_empty() {
            let count = samples.len().min(CHUNK - self.pending);
            self.input[self.pending..self.pending + count].copy_from_slice(&samples[..count]);
            self.pending += count;
            samples = &samples[count..];
            if self.pending == CHUNK {
                self.process(None)?;
                self.pending = 0;
            }
        }
        Ok(&self.ready)
    }

    pub fn finish(&mut self) -> Result<&[f32], String> {
        self.ready.clear();
        let expected = (self.received as f64 * self.ratio).round() as usize;
        while self.emitted < expected {
            self.input[self.pending..].fill(0.0);
            self.process(Some(self.pending))?;
            self.pending = 0;
        }
        Ok(&self.ready)
    }

    fn process(&mut self, partial: Option<usize>) -> Result<(), String> {
        let filter = self.filter.as_mut().expect("resampling requires a filter");
        let input = InterleavedSlice::new(&self.input, 1, CHUNK).map_err(|e| e.to_string())?;
        let frames = self.output.len();
        let mut output =
            InterleavedSlice::new_mut(&mut self.output, 1, frames).map_err(|e| e.to_string())?;
        let mut indexing = Indexing::new();
        indexing.partial_len = partial;
        let (_, written) = filter
            .process_into_buffer(&input, &mut output, Some(&indexing))
            .map_err(|e| e.to_string())?;
        if written == 0 {
            return Err("resampler produced no frames".into());
        }
        let skip = self.delay.min(written);
        self.delay -= skip;
        let expected = (self.received as f64 * self.ratio).round() as usize;
        let take = (written - skip).min(expected.saturating_sub(self.emitted));
        self.ready
            .extend_from_slice(&self.output[skip..skip + take]);
        self.emitted += take;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn convert(samples: &[f32], rate: u32, chunk: usize) -> Vec<f32> {
        let mut filter = AudioResampler::new(rate, 16_000).unwrap();
        let mut output = Vec::new();
        for part in samples.chunks(chunk) {
            output.extend_from_slice(filter.push(part).unwrap());
        }
        output.extend_from_slice(filter.finish().unwrap());
        assert!(
            filter.finish().unwrap().is_empty(),
            "flushing twice must not repeat the tail"
        );
        output
    }

    #[test]
    fn preserves_duration_and_tone_at_every_device_rate() {
        for rate in [16_000, 32_000, 44_100, 48_000, 96_000] {
            let input: Vec<_> = (0..rate)
                .map(|i| (std::f32::consts::TAU * 440.0 * i as f32 / rate as f32).sin())
                .collect();
            let output = convert(&input, rate, 127);
            assert_eq!(output.len(), 16_000, "rate {rate}");
            let crossings = output[1000..15000]
                .windows(2)
                .filter(|w| w[0] <= 0.0 && w[1] > 0.0)
                .count();
            assert!(
                (crossings as f64 * 16000.0 / 14000.0 - 440.0).abs() < 2.0,
                "rate {rate}, crossings {crossings}"
            );
        }
    }

    #[test]
    fn callback_boundaries_do_not_change_audio_or_lose_the_tail() {
        let input: Vec<_> = (0..44_789)
            .map(|i| if i > 43_000 { 0.5 } else { 0.0 })
            .collect();
        let whole = convert(&input, 44_100, input.len());
        for chunk in [1, 128, 441, 1024, 2048] {
            assert_eq!(convert(&input, 44_100, chunk), whole, "chunk {chunk}");
        }
        assert!(whole[whole.len() - 100..whole.len() - 20]
            .iter()
            .all(|v| *v > 0.4));
    }

    #[test]
    fn empty_and_sub_block_recordings_have_exact_lengths() {
        assert!(convert(&[], 44_100, 1).is_empty());
        assert_eq!(convert(&[0.5; 100], 44_100, 17).len(), 36);
        assert!(AudioResampler::new(0, 16_000).is_err());
    }
}
