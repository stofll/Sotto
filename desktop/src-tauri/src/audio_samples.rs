//! Reusable conversion and mono buffers owned by a microphone callback.

/// Convert signed 16-bit PCM to [-1.0, 1.0).
#[inline]
pub fn i16_to_f32(sample: i16) -> f32 {
    sample as f32 / 32768.0
}

/// Convert unsigned 8-bit PCM to [-1.0, 127.0 / 128.0].
#[inline]
pub fn u8_to_f32(sample: u8) -> f32 {
    (sample as f32 - 128.0) / 128.0
}

/// Replace the previous callback's converted samples, retaining its capacity.
pub fn convert_samples<T>(data: &[T], converted: &mut Vec<f32>, to_f32: impl FnMut(&T) -> f32) {
    converted.clear();
    converted.extend(data.iter().map(to_f32));
}

/// Borrow mono input directly, or replace the reusable interleaved mixdown.
/// A trailing incomplete frame is discarded, as in the original capture path.
pub fn mono_samples<'a>(data: &'a [f32], channels: u16, mono: &'a mut Vec<f32>) -> &'a [f32] {
    if channels <= 1 {
        return data;
    }
    mono.clear();
    mono.extend(
        data.chunks_exact(channels as usize)
            .map(|frame| frame.iter().sum::<f32>() / channels as f32),
    );
    mono
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversion_replaces_previous_samples_and_reuses_capacity() {
        let mut buffer = Vec::with_capacity(8);
        let allocation = buffer.as_ptr();
        convert_samples(&[i16::MIN, 0, i16::MAX], &mut buffer, |&s| i16_to_f32(s));
        assert_eq!(buffer, [-1.0, 0.0, 32767.0 / 32768.0]);
        convert_samples(&[16384], &mut buffer, |&s| i16_to_f32(s));
        assert_eq!(buffer, [0.5]);
        convert_samples(&[0u8, 128, 255], &mut buffer, |&s| u8_to_f32(s));
        assert_eq!(buffer, [-1.0, 0.0, 127.0 / 128.0]);
        convert_samples::<i16>(&[], &mut buffer, |&s| i16_to_f32(s));
        assert!(buffer.is_empty());
        assert_eq!(buffer.as_ptr(), allocation);
        assert_eq!(buffer.capacity(), 8);
    }

    #[test]
    fn mixdown_replaces_previous_frames_and_drops_partial_frame() {
        let mut mono = Vec::with_capacity(8);
        let allocation = mono.as_ptr();
        assert_eq!(
            mono_samples(&[0.25, 0.75, 1.0, -1.0, 0.5], 2, &mut mono),
            [0.5, 0.0]
        );
        assert_eq!(mono_samples(&[-0.5, -0.5], 2, &mut mono), [-0.5]);
        assert!(mono_samples(&[0.5], 2, &mut mono).is_empty());
        assert_eq!(mono.as_ptr(), allocation);
        assert_eq!(mono.capacity(), 8);
    }

    #[test]
    fn mono_input_is_borrowed_without_scratch_copy() {
        let input = [0.25, -0.5, 1.0];
        let mut scratch = Vec::new();
        for channels in [0, 1] {
            let output = mono_samples(&input, channels, &mut scratch);
            assert_eq!(output, input);
            assert_eq!(output.as_ptr(), input.as_ptr());
        }
        assert_eq!(scratch.capacity(), 0);
    }

    #[test]
    fn larger_callbacks_grow_once_and_keep_all_complete_frames() {
        let mut converted = Vec::new();
        let mut mono = Vec::new();
        let input: Vec<i16> = [16384, -16384, 0, 32767].repeat(1024);
        convert_samples(&input, &mut converted, |&s| i16_to_f32(s));
        let output = mono_samples(&converted, 2, &mut mono);
        assert_eq!(output.len(), 2048);
        assert_eq!(
            output[0..4],
            [0.0, 32767.0 / 65536.0, 0.0, 32767.0 / 65536.0]
        );
        let capacities = (converted.capacity(), mono.capacity());
        let allocations = (converted.as_ptr(), mono.as_ptr());
        for samples in [&input[..64], &input[..1024], &input[..]] {
            convert_samples(samples, &mut converted, |&s| i16_to_f32(s));
            assert_eq!(
                mono_samples(&converted, 2, &mut mono).len(),
                samples.len() / 2
            );
            assert_eq!((converted.capacity(), mono.capacity()), capacities);
            assert_eq!((converted.as_ptr(), mono.as_ptr()), allocations);
        }
    }
}
