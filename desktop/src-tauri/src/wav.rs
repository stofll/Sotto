//! Minimal RIFF/WAVE encoder.
//!
//! Two callers with nothing else in common: the audio cues in
//! [`crate::sounds`], which synthesise tones to hand to `PlaySoundW`, and the
//! debug recording dump in [`crate::debug`], which writes captured microphone
//! audio to disk. Both need 16-bit mono PCM wrapped in a 44-byte header, and
//! neither justifies a dependency.

/// Wrap 16-bit mono PCM in a complete RIFF/WAVE image.
pub fn encode_pcm16_mono(samples: &[i16], sample_rate: u32) -> Vec<u8> {
    let data_bytes = samples.len() * 2;
    let mut wav = Vec::with_capacity(44 + data_bytes);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&((36 + data_bytes) as u32).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes()); // PCM fmt chunk size
    wav.extend_from_slice(&1u16.to_le_bytes()); // format: PCM
    wav.extend_from_slice(&1u16.to_le_bytes()); // channels: mono
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&(sample_rate * 2).to_le_bytes()); // byte rate
    wav.extend_from_slice(&2u16.to_le_bytes()); // block align
    wav.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(data_bytes as u32).to_le_bytes());
    for sample in samples {
        wav.extend_from_slice(&sample.to_le_bytes());
    }
    wav
}

/// Convert the pipeline's `f32` samples to 16-bit PCM.
///
/// Clamped, not wrapped: a sample above 1.0 (which a hot microphone does
/// produce) would otherwise wrap to full-scale negative and turn a loud
/// passage into a burst of noise.
pub fn f32_to_pcm16(samples: &[f32]) -> Vec<i16> {
    samples
        .iter()
        .map(|&sample| (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_describes_the_payload() {
        let wav = encode_pcm16_mono(&[0, 1, -1], 16_000);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(&wav[36..40], b"data");
        assert_eq!(
            u32::from_le_bytes(wav[40..44].try_into().unwrap()) as usize,
            6
        );
        assert_eq!(
            u32::from_le_bytes(wav[4..8].try_into().unwrap()) as usize,
            wav.len() - 8
        );
        assert_eq!(u32::from_le_bytes(wav[24..28].try_into().unwrap()), 16_000);
        // Byte rate at offset 28..32 = sample_rate * channels * bytes/sample.
        assert_eq!(u32::from_le_bytes(wav[28..32].try_into().unwrap()), 32_000);
        // Block align at offset 32..34 = 1 channel × 2 bytes.
        assert_eq!(u16::from_le_bytes(wav[32..34].try_into().unwrap()), 2);
    }

    #[test]
    fn empty_input_is_a_valid_header() {
        let wav = encode_pcm16_mono(&[], 16_000);
        assert_eq!(wav.len(), 44);
        assert_eq!(u32::from_le_bytes(wav[40..44].try_into().unwrap()), 0);
    }

    #[test]
    fn conversion_clamps_instead_of_wrapping() {
        // A sample over 1.0 wrapping would flip a loud passage to full-scale
        // negative — audible as a burst of noise, not as clipping.
        let pcm = f32_to_pcm16(&[0.0, 1.0, -1.0, 4.2, -4.2]);
        assert_eq!(pcm, [0, i16::MAX, -i16::MAX, i16::MAX, -i16::MAX]);
    }
}
