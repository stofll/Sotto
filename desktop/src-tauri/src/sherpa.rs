//! The small Sherpa-ONNX bridge used by the closed ONNX registry entries.
//!
//! The heavy lifting is the upstream `sherpa-onnx` crate, which owns the C
//! handles through RAII types. What stays here is what that crate does not do:
//! the input validation the rest of this codebase relies on, the `SHERPA_*`
//! error vocabulary, and the mapping from a registry entry to the one config
//! field that selects a model family. Callers must validate the closed-registry
//! SHA-256 manifest before constructing a recognizer, because sherpa-onnx may
//! throw a foreign C++ exception for an incompatible graph.
//!
//! The recognizer never leaves the engine thread. The upstream types are marked
//! `Send + Sync`, so the wrappers here opt back out with a `PhantomData` — the
//! single-thread rule is checked by the compiler rather than by convention.

#[cfg(any(windows, target_os = "macos"))]
use std::marker::PhantomData;
#[cfg(any(windows, target_os = "macos"))]
use std::path::Path;

#[cfg(any(windows, target_os = "macos"))]
const PROVIDER_CPU: &str = "cpu";
#[cfg(any(windows, target_os = "macos"))]
const DECODING_GREEDY: &str = "greedy_search";
#[cfg(any(windows, target_os = "macos"))]
const LANG_EN: &str = "en";
#[cfg(any(windows, target_os = "macos"))]
const LANG_AUTO: &str = "auto";

#[cfg(any(windows, target_os = "macos"))]
pub struct OfflineRecognizer {
    recognizer: sherpa_onnx::OfflineRecognizer,
    _not_send: PhantomData<*const ()>,
}

/// The upstream recognizer has no `Debug`, and the engine's state does.
#[cfg(any(windows, target_os = "macos"))]
impl std::fmt::Debug for OfflineRecognizer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("OfflineRecognizer")
    }
}

/// Paths reach the C API as NUL-terminated strings, and the upstream wrapper
/// panics on an interior NUL rather than reporting it. Reject it here, where
/// there is an error type to report it with.
#[cfg(any(windows, target_os = "macos"))]
fn path_string(path: &Path) -> Result<String, String> {
    let path = path.to_string_lossy().into_owned();
    if path.contains('\0') {
        return Err("SHERPA_INVALID_PATH: path contains NUL".to_string());
    }
    Ok(path)
}

#[cfg(any(windows, target_os = "macos"))]
impl OfflineRecognizer {
    /// The single entry point for every sherpa family: the engine picks the
    /// constructor, file roles arrive from the manifest. Neither the engine
    /// thread nor the catalog knows how many graphs a given family consists of.
    pub fn open(
        engine: crate::model::ModelEngine,
        files: &crate::model::BundleFiles,
        num_threads: i32,
    ) -> Result<Self, String> {
        use crate::model::{ArtifactRole as R, ModelEngine};
        match engine {
            ModelEngine::Whisper => {
                Err("SHERPA_WRONG_ENGINE: whisper does not run through sherpa".to_string())
            }
            ModelEngine::SherpaNemoCtc => {
                Self::nemo_ctc(files.path(R::Model)?, files.path(R::Tokens)?, num_threads)
            }
            ModelEngine::SherpaTransducer => Self::transducer(
                files.path(R::Encoder)?,
                files.path(R::Decoder)?,
                files.path(R::Joiner)?,
                files.path(R::Tokens)?,
                num_threads,
            ),
            ModelEngine::SherpaCanary => Self::canary(
                files.path(R::Encoder)?,
                files.path(R::Decoder)?,
                files.path(R::Tokens)?,
                num_threads,
            ),
            ModelEngine::SherpaMoonshine => Self::moonshine(
                files.path(R::Preprocessor)?,
                files.path(R::Encoder)?,
                files.path(R::UncachedDecoder)?,
                files.path(R::CachedDecoder)?,
                files.path(R::Tokens)?,
                num_threads,
            ),
            ModelEngine::SherpaSenseVoice => {
                Self::sense_voice(files.path(R::Model)?, files.path(R::Tokens)?, num_threads)
            }
            ModelEngine::SherpaStreamingTransducer => {
                Err("SHERPA_WRONG_ENGINE: streaming models need the online recognizer".to_string())
            }
        }
    }

    /// NeMo Canary: encoder and decoder plus a language pair.
    ///
    /// `src_lang`/`tgt_lang` are set when the recognizer is created rather than
    /// per transcription, so the model's language is pinned to English here —
    /// changing it would require reloading the model. `use_pnc` turns on
    /// punctuation and capitalisation; without it the text arrives as one
    /// continuous string.
    pub fn canary(
        encoder_path: &Path,
        decoder_path: &Path,
        tokens_path: &Path,
        num_threads: i32,
    ) -> Result<Self, String> {
        check_threads(num_threads)?;
        let model_config = sherpa_onnx::OfflineModelConfig {
            canary: sherpa_onnx::OfflineCanaryModelConfig {
                encoder: Some(path_string(encoder_path)?),
                decoder: Some(path_string(decoder_path)?),
                src_lang: Some(LANG_EN.to_string()),
                tgt_lang: Some(LANG_EN.to_string()),
                use_pnc: true,
            },
            ..common_model_config(path_string(tokens_path)?, num_threads)
        };
        Self::create(model_config, None)
    }

    /// Moonshine: a preprocessor, an encoder and a decoder in two forms —
    /// cached and not. The first decoding step goes through the "cold" graph,
    /// the rest through the cached one; sherpa switches between them itself.
    pub fn moonshine(
        preprocessor_path: &Path,
        encoder_path: &Path,
        uncached_decoder_path: &Path,
        cached_decoder_path: &Path,
        tokens_path: &Path,
        num_threads: i32,
    ) -> Result<Self, String> {
        check_threads(num_threads)?;
        let model_config = sherpa_onnx::OfflineModelConfig {
            moonshine: sherpa_onnx::OfflineMoonshineModelConfig {
                preprocessor: Some(path_string(preprocessor_path)?),
                encoder: Some(path_string(encoder_path)?),
                uncached_decoder: Some(path_string(uncached_decoder_path)?),
                cached_decoder: Some(path_string(cached_decoder_path)?),
                // Newer sherpa releases accept a single merged decoder graph
                // instead. The catalog ships the two-graph export.
                merged_decoder: None,
            },
            ..common_model_config(path_string(tokens_path)?, num_threads)
        };
        Self::create(model_config, None)
    }

    /// SenseVoice: a single graph and a token table, like NeMo CTC, but with its
    /// own config field. `auto` leaves language detection to the model itself;
    /// `use_itn` enables inverse normalisation — numbers and dates arrive as
    /// digits rather than words.
    pub fn sense_voice(
        model_path: &Path,
        tokens_path: &Path,
        num_threads: i32,
    ) -> Result<Self, String> {
        check_threads(num_threads)?;
        let model_config = sherpa_onnx::OfflineModelConfig {
            sense_voice: sherpa_onnx::OfflineSenseVoiceModelConfig {
                model: Some(path_string(model_path)?),
                language: Some(LANG_AUTO.to_string()),
                use_itn: true,
            },
            ..common_model_config(path_string(tokens_path)?, num_threads)
        };
        Self::create(model_config, None)
    }

    /// NeMo CTC (GigaAM): one graph plus its token table.
    pub fn nemo_ctc(
        model_path: &Path,
        tokens_path: &Path,
        num_threads: i32,
    ) -> Result<Self, String> {
        check_threads(num_threads)?;
        let model_config = sherpa_onnx::OfflineModelConfig {
            nemo_ctc: sherpa_onnx::OfflineNemoEncDecCtcModelConfig {
                model: Some(path_string(model_path)?),
            },
            ..common_model_config(path_string(tokens_path)?, num_threads)
        };
        Self::create(model_config, None)
    }

    /// NeMo transducer (Parakeet TDT): three graphs — encoder, decoder and
    /// joiner — over the shared token table.
    ///
    /// `model_type` stays unset on purpose so sherpa-onnx reads the variant
    /// out of the encoder's ONNX metadata. Spelling it as `"transducer"`,
    /// which is what the wrapper crates default to, would mis-decode a TDT
    /// graph: those emit a duration alongside every symbol.
    pub fn transducer(
        encoder_path: &Path,
        decoder_path: &Path,
        joiner_path: &Path,
        tokens_path: &Path,
        num_threads: i32,
    ) -> Result<Self, String> {
        check_threads(num_threads)?;
        let model_config = sherpa_onnx::OfflineModelConfig {
            transducer: sherpa_onnx::OfflineTransducerModelConfig {
                encoder: Some(path_string(encoder_path)?),
                decoder: Some(path_string(decoder_path)?),
                joiner: Some(path_string(joiner_path)?),
            },
            ..common_model_config(path_string(tokens_path)?, num_threads)
        };
        // Unlike the CTC path, the transducer wants its feature extractor
        // spelled out: 80-dim log-mel at 16 kHz, which is what every NeMo
        // export expects.
        Self::create(model_config, Some((16_000, 80)))
    }

    /// # Panics
    /// Never: `create` reports a null recognizer as an error.
    fn create(
        model_config: sherpa_onnx::OfflineModelConfig,
        feat_config: Option<(i32, i32)>,
    ) -> Result<Self, String> {
        let mut config = sherpa_onnx::OfflineRecognizerConfig {
            model_config,
            decoding_method: Some(DECODING_GREEDY.to_string()),
            ..Default::default()
        };
        // `None` means the zeroed feature config every family except the
        // transducer was created with before this crate owned the C calls —
        // those models carry their own front end and read this back. The
        // remaining defaults (`max_active_paths`, `blank_penalty`, the LM) are
        // inert under greedy decoding with no LM configured.
        let (sample_rate, feature_dim) = feat_config.unwrap_or((0, 0));
        config.feat_config.sample_rate = sample_rate;
        config.feat_config.feature_dim = feature_dim;

        let recognizer = sherpa_onnx::OfflineRecognizer::create(&config)
            .ok_or_else(|| "SHERPA_CREATE_FAILED: offline recognizer returned null".to_string())?;
        Ok(Self {
            recognizer,
            _not_send: PhantomData,
        })
    }

    pub fn transcribe(&mut self, sample_rate: u32, samples: &[f32]) -> Result<String, String> {
        validate_audio(sample_rate, samples)?;
        if samples.is_empty() {
            return Ok(String::new());
        }

        // A `SHERPA_STREAM_FAILED` used to guard this: the C API can hand back
        // a null stream, and the old code checked for it. `create_stream` wraps
        // the pointer without checking and keeps it private, so the check is
        // gone — not dropped by choice. If the C call ever does return null,
        // the next line dereferences it inside the DLL instead of reporting it.
        let stream = self.recognizer.create_stream();
        stream.accept_waveform(sample_rate as i32, samples);
        self.recognizer.decode(&stream);
        stream
            .get_result()
            .map(|result| result.text)
            .ok_or_else(|| "SHERPA_RESULT_FAILED: offline result returned null".to_string())
    }
}

/// Every field a family does not use must stay unset: sherpa-onnx picks the
/// implementation by the first non-empty model field.
#[cfg(any(windows, target_os = "macos"))]
fn common_model_config(tokens: String, num_threads: i32) -> sherpa_onnx::OfflineModelConfig {
    sherpa_onnx::OfflineModelConfig {
        tokens: Some(tokens),
        num_threads,
        debug: false,
        provider: Some(PROVIDER_CPU.to_string()),
        ..Default::default()
    }
}

/// A recognizer holds native state the engine keeps on one thread. The
/// upstream types are marked `Send + Sync`, so nothing but the `PhantomData` in
/// these wrappers stops one from being moved across threads — assert at compile
/// time that it is still doing its job.
///
/// Inherent items win over trait items when their bounds hold, so `SEND` reads
/// `true` only for a type that really is `Send`. The `u32` line is the control:
/// if the trick ever stopped resolving that way, it would fail first, instead of
/// quietly passing everything.
#[cfg(any(windows, target_os = "macos"))]
const _: () = {
    struct IsSend<T>(PhantomData<T>);

    trait Fallback {
        const SEND: bool = false;
    }

    impl<T> Fallback for IsSend<T> {}

    impl<T: Send> IsSend<T> {
        const SEND: bool = true;
    }

    assert!(IsSend::<u32>::SEND);
    assert!(!IsSend::<OfflineRecognizer>::SEND);
    assert!(!IsSend::<OnlineRecognizer>::SEND);
    assert!(!IsSend::<SherpaRecognizer>::SEND);
};

/// Checks common to both recognizers. sherpa does not check for NaN in the
/// buffer and answers incompatible input by crashing the process, so the audio
/// is inspected before the C boundary.
#[cfg(any(windows, target_os = "macos"))]
fn validate_audio(sample_rate: u32, samples: &[f32]) -> Result<(), String> {
    if sample_rate == 0 || sample_rate > i32::MAX as u32 {
        return Err(format!("SHERPA_INVALID_SAMPLE_RATE: {sample_rate}"));
    }
    if samples.len() > i32::MAX as usize {
        return Err("SHERPA_AUDIO_TOO_LARGE: sample buffer exceeds C API limit".to_string());
    }
    if samples.iter().any(|sample| !sample.is_finite()) {
        return Err("SHERPA_INVALID_AUDIO: sample buffer contains NaN or infinity".to_string());
    }
    Ok(())
}

#[cfg(any(windows, target_os = "macos"))]
fn check_threads(num_threads: i32) -> Result<(), String> {
    if num_threads <= 0 {
        return Err("SHERPA_INVALID_THREADS: num_threads must be positive".to_string());
    }
    Ok(())
}

/// Keep the engine command surface cross-platform while making the
/// unsupported native runtime explicit. The bundle registry is empty on these
/// targets, so this is a defensive error path rather than a user-visible
/// model option.
#[cfg(not(any(windows, target_os = "macos")))]
#[derive(Debug)]
pub struct OfflineRecognizer;

#[cfg(not(any(windows, target_os = "macos")))]
const UNSUPPORTED: &str = "SHERPA_UNSUPPORTED_PLATFORM: ONNX models require Windows or macOS";

#[cfg(not(any(windows, target_os = "macos")))]
impl OfflineRecognizer {
    pub fn open(
        _engine: crate::model::ModelEngine,
        _files: &crate::model::BundleFiles,
        _num_threads: i32,
    ) -> Result<Self, String> {
        Err(UNSUPPORTED.to_string())
    }

    pub fn nemo_ctc(
        _model_path: &std::path::Path,
        _tokens_path: &std::path::Path,
        _num_threads: i32,
    ) -> Result<Self, String> {
        Err(UNSUPPORTED.to_string())
    }

    pub fn canary(
        _encoder_path: &std::path::Path,
        _decoder_path: &std::path::Path,
        _tokens_path: &std::path::Path,
        _num_threads: i32,
    ) -> Result<Self, String> {
        Err(UNSUPPORTED.to_string())
    }

    pub fn moonshine(
        _preprocessor_path: &std::path::Path,
        _encoder_path: &std::path::Path,
        _uncached_decoder_path: &std::path::Path,
        _cached_decoder_path: &std::path::Path,
        _tokens_path: &std::path::Path,
        _num_threads: i32,
    ) -> Result<Self, String> {
        Err(UNSUPPORTED.to_string())
    }

    pub fn sense_voice(
        _model_path: &std::path::Path,
        _tokens_path: &std::path::Path,
        _num_threads: i32,
    ) -> Result<Self, String> {
        Err(UNSUPPORTED.to_string())
    }

    pub fn transducer(
        _encoder_path: &std::path::Path,
        _decoder_path: &std::path::Path,
        _joiner_path: &std::path::Path,
        _tokens_path: &std::path::Path,
        _num_threads: i32,
    ) -> Result<Self, String> {
        Err(UNSUPPORTED.to_string())
    }

    pub fn transcribe(&mut self, _sample_rate: u32, _samples: &[f32]) -> Result<String, String> {
        Err(UNSUPPORTED.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn rejects_invalid_thread_count_before_ffi() {
        let err = OfflineRecognizer::nemo_ctc(Path::new("model.onnx"), Path::new("tokens.txt"), 0)
            .unwrap_err();
        assert!(err.contains("SHERPA_INVALID_THREADS"));
    }

    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn transducer_rejects_invalid_thread_count_before_ffi() {
        let err = OfflineRecognizer::transducer(
            Path::new("encoder.onnx"),
            Path::new("decoder.onnx"),
            Path::new("joiner.onnx"),
            Path::new("tokens.txt"),
            0,
        )
        .unwrap_err();
        assert!(err.contains("SHERPA_INVALID_THREADS"));
    }

    /// The upstream wrapper turns a path into a `CString` with `.unwrap()`, so
    /// an interior NUL has to be rejected on this side of the boundary.
    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn rejects_path_with_interior_nul_before_ffi() {
        let err =
            OfflineRecognizer::nemo_ctc(Path::new("mo\0del.onnx"), Path::new("tokens.txt"), 4)
                .unwrap_err();
        assert!(err.contains("SHERPA_INVALID_PATH"));
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    #[test]
    fn reports_unsupported_platform_without_ffi() {
        let err = OfflineRecognizer::nemo_ctc(
            std::path::Path::new("model.onnx"),
            std::path::Path::new("tokens.txt"),
            4,
        )
        .unwrap_err();
        assert!(err.contains("SHERPA_UNSUPPORTED_PLATFORM"));
    }
}

/// Streaming sherpa recognizer.
///
/// It differs from the offline one not by the model but by the conversation:
/// audio is fed in chunks, the recognizer itself reports whether enough has
/// accumulated for a decoding step, and at any step it returns the current
/// hypothesis. The stream lives between calls, which is why it is a field here
/// rather than a local variable.
#[cfg(any(windows, target_os = "macos"))]
pub struct OnlineRecognizer {
    // Declared before the recognizer so it is dropped first: the stream is
    // borrowed from the recognizer for the whole of its life.
    stream: sherpa_onnx::OnlineStream,
    recognizer: sherpa_onnx::OnlineRecognizer,
    /// The rate the phrase is being fed at, remembered from the first chunk.
    /// sherpa aborts the process — not returns an error — when a stream is
    /// handed a second sample rate, so the tail padding below must use the
    /// rate the stream already has rather than the model's own.
    input_rate: Option<u32>,
    _not_send: PhantomData<*const ()>,
}

#[cfg(any(windows, target_os = "macos"))]
impl std::fmt::Debug for OnlineRecognizer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("OnlineRecognizer")
    }
}

#[cfg(any(windows, target_os = "macos"))]
impl OnlineRecognizer {
    pub fn streaming_transducer(
        encoder_path: &Path,
        decoder_path: &Path,
        joiner_path: &Path,
        tokens_path: &Path,
        num_threads: i32,
    ) -> Result<Self, String> {
        check_threads(num_threads)?;
        let config = sherpa_onnx::OnlineRecognizerConfig {
            // The default feature config is the 16 kHz, 80-dim log-mel front
            // end every streaming zipformer export expects.
            model_config: sherpa_onnx::OnlineModelConfig {
                transducer: sherpa_onnx::OnlineTransducerModelConfig {
                    encoder: Some(path_string(encoder_path)?),
                    decoder: Some(path_string(decoder_path)?),
                    joiner: Some(path_string(joiner_path)?),
                },
                tokens: Some(path_string(tokens_path)?),
                num_threads,
                debug: false,
                provider: Some(PROVIDER_CPU.to_string()),
                ..Default::default()
            },
            decoding_method: Some(DECODING_GREEDY.to_string()),
            // The bounds of a dictation are set by the hotkey, not by silence
            // in the microphone: with endpoint detection enabled the recognizer
            // would cut a pause mid-thought and start the phrase over.
            enable_endpoint: false,
            ..Default::default()
        };

        let recognizer = sherpa_onnx::OnlineRecognizer::create(&config)
            .ok_or_else(|| "SHERPA_CREATE_FAILED: online recognizer returned null".to_string())?;
        let stream = recognizer.create_stream();
        Ok(Self {
            stream,
            recognizer,
            input_rate: None,
            _not_send: PhantomData,
        })
    }

    /// Feed the next chunk of audio and advance decoding as far as the
    /// available data allows. Does not block until the end of the phrase.
    pub fn feed(&mut self, sample_rate: u32, samples: &[f32]) -> Result<(), String> {
        validate_audio(sample_rate, samples)?;
        if samples.is_empty() {
            return Ok(());
        }
        self.input_rate.get_or_insert(sample_rate);
        self.stream.accept_waveform(sample_rate as i32, samples);
        self.decode_ready();
        Ok(())
    }

    /// The current hypothesis in full. The text grows and may be corrected
    /// retroactively, so it must never be inserted anywhere — only displayed.
    ///
    /// The only accessor upstream offers serialises the result to JSON and
    /// parses it back, tokens and timestamps included, so the live preview pays
    /// one parse per chunk over a hypothesis that grows for the whole dictation
    /// — where the previous binding read a single C string. A failed parse
    /// arrives here as the same `SHERPA_RESULT_FAILED` as a null result.
    pub fn text(&self) -> Result<String, String> {
        self.recognizer
            .get_result(&self.stream)
            .map(|result| result.text)
            .ok_or_else(|| "SHERPA_RESULT_FAILED: online result returned null".to_string())
    }

    /// Close the phrase: finish the tail and return the final text.
    ///
    /// The last words are padded with silence first. A cache-aware model
    /// decodes a chunk only once the look-ahead behind it has arrived, so
    /// closing the stream on the final syllable leaves that chunk undecoded
    /// and the phrase ends mid-word — «для своєї краї» instead of «країни».
    /// The padding is what the audio would have contained had the person
    /// stopped talking a moment before releasing the hotkey.
    pub fn finish(&mut self) -> Result<String, String> {
        if let Some(rate) = self.input_rate {
            // Longer than the widest look-ahead among the streaming models in
            // the catalog (560 ms), so a chunk is never left waiting.
            let samples = vec![0.0_f32; (rate as usize) * 3 / 5];
            self.stream.accept_waveform(rate as i32, &samples);
        }
        self.stream.input_finished();
        self.decode_ready();
        self.text()
    }

    /// Forget what was accumulated and start the next dictation from scratch.
    pub fn reset(&mut self) {
        self.recognizer.reset(&self.stream);
        // The next phrase may arrive at another rate — from a file rather than
        // from the microphone — and the padding must follow it, not the last
        // one.
        self.input_rate = None;
    }

    fn decode_ready(&self) {
        while self.recognizer.is_ready(&self.stream) {
            self.recognizer.decode(&self.stream);
        }
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
#[derive(Debug)]
pub struct OnlineRecognizer;

#[cfg(not(any(windows, target_os = "macos")))]
impl OnlineRecognizer {
    pub fn streaming_transducer(
        _encoder_path: &std::path::Path,
        _decoder_path: &std::path::Path,
        _joiner_path: &std::path::Path,
        _tokens_path: &std::path::Path,
        _num_threads: i32,
    ) -> Result<Self, String> {
        Err(UNSUPPORTED.to_string())
    }

    pub fn feed(&mut self, _sample_rate: u32, _samples: &[f32]) -> Result<(), String> {
        Err(UNSUPPORTED.to_string())
    }

    pub fn text(&self) -> Result<String, String> {
        Err(UNSUPPORTED.to_string())
    }

    pub fn finish(&mut self) -> Result<String, String> {
        Err(UNSUPPORTED.to_string())
    }

    pub fn reset(&mut self) {}
}

/// A recognizer of any sherpa family — streaming or not.
///
/// This is what the engine thread holds: what matters to it is that the model
/// is loaded and can transcribe a buffer, while how many graphs it is assembled
/// from and which API it uses is this module's business.
#[derive(Debug)]
pub enum SherpaRecognizer {
    Offline(OfflineRecognizer),
    Online(OnlineRecognizer),
}

impl SherpaRecognizer {
    pub fn open(
        engine: crate::model::ModelEngine,
        files: &crate::model::BundleFiles,
        num_threads: i32,
    ) -> Result<Self, String> {
        use crate::model::{ArtifactRole as R, ModelEngine};
        if engine == ModelEngine::SherpaStreamingTransducer {
            return Ok(Self::Online(OnlineRecognizer::streaming_transducer(
                files.path(R::Encoder)?,
                files.path(R::Decoder)?,
                files.path(R::Joiner)?,
                files.path(R::Tokens)?,
                num_threads,
            )?));
        }
        OfflineRecognizer::open(engine, files, num_threads).map(Self::Offline)
    }

    /// Transcribe a complete buffer. A streaming recognizer takes the same path
    /// as the live preview, only without pauses: feed everything, close the
    /// phrase, take the text.
    pub fn transcribe(&mut self, sample_rate: u32, samples: &[f32]) -> Result<String, String> {
        match self {
            Self::Offline(recognizer) => recognizer.transcribe(sample_rate, samples),
            Self::Online(recognizer) => {
                recognizer.reset();
                recognizer.feed(sample_rate, samples)?;
                let text = recognizer.finish()?;
                recognizer.reset();
                Ok(text)
            }
        }
    }

    pub const fn is_streaming(&self) -> bool {
        matches!(self, Self::Online(_))
    }

    /// Forget the accumulated hypothesis. A non-streaming recognizer has
    /// nothing to accumulate, so the call is harmless.
    pub fn reset_preview(&mut self) {
        if let Self::Online(recognizer) = self {
            recognizer.reset();
        }
    }

    /// Live preview: feed a chunk and return the current hypothesis. A
    /// non-streaming recognizer has no hypothesis — it stays silent until the
    /// recording ends.
    pub fn feed_preview(
        &mut self,
        sample_rate: u32,
        samples: &[f32],
    ) -> Result<Option<String>, String> {
        match self {
            Self::Offline(_) => Ok(None),
            Self::Online(recognizer) => {
                recognizer.feed(sample_rate, samples)?;
                recognizer.text().map(Some)
            }
        }
    }
}
