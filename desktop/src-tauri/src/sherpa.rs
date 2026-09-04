//! The small Sherpa-ONNX bridge used by the closed ONNX registry entries.
//!
//! `sherpa-rs` 0.6.8 ships a safe wrapper for transducers but none for the
//! NeMo CTC family, and that wrapper skips the null checks and the input
//! validation this crate relies on. So this module owns the handful of C
//! calls for both families instead: one recognizer type, two constructors.
//! The recognizer never leaves the engine thread — it holds a raw pointer and
//! deliberately does not implement `Send`/`Sync`. Callers must validate the
//! closed-registry SHA-256 manifest before constructing it, because
//! sherpa-onnx may throw a foreign C++ exception for an incompatible graph.

#[cfg(windows)]
use std::ffi::{CStr, CString};
#[cfg(windows)]
use std::os::raw::c_char;
#[cfg(windows)]
use std::path::Path;

#[cfg(windows)]
const PROVIDER_CPU: &[u8] = b"cpu\0";
#[cfg(windows)]
const DECODING_GREEDY: &[u8] = b"greedy_search\0";
#[cfg(windows)]
const LANG_EN: &[u8] = b"en\0";
#[cfg(windows)]
const LANG_AUTO: &[u8] = b"auto\0";

#[cfg(windows)]
#[derive(Debug)]
pub struct OfflineRecognizer {
    recognizer: *const sherpa_rs::sherpa_rs_sys::SherpaOnnxOfflineRecognizer,
}

#[cfg(windows)]
fn c_path(path: &Path) -> Result<CString, String> {
    CString::new(path.to_string_lossy().as_bytes())
        .map_err(|_| "SHERPA_INVALID_PATH: path contains NUL".to_string())
}

#[cfg(windows)]
impl OfflineRecognizer {
    /// Единственный вход для всех семейств sherpa: движок выбирает
    /// конструктор, роли файлов приезжают из манифеста. Ни поток движка, ни
    /// каталог не знают, из скольких графов состоит конкретное семейство.
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

    /// NeMo Canary: энкодер и декодер плюс языковая пара.
    ///
    /// `src_lang`/`tgt_lang` задаются при создании распознавателя, а не на
    /// каждой расшифровке, поэтому язык модели у нас закреплён английским —
    /// сменить его можно было бы только перезагрузкой модели. `use_pnc`
    /// включает пунктуацию и заглавные, без него текст приходит сплошной
    /// строкой.
    pub fn canary(
        encoder_path: &Path,
        decoder_path: &Path,
        tokens_path: &Path,
        num_threads: i32,
    ) -> Result<Self, String> {
        check_threads(num_threads)?;
        let encoder = c_path(encoder_path)?;
        let decoder = c_path(decoder_path)?;
        let tokens = c_path(tokens_path)?;

        let recognizer = unsafe {
            use sherpa_rs::sherpa_rs_sys as sys;
            let model_config = sys::SherpaOnnxOfflineModelConfig {
                canary: sys::SherpaOnnxOfflineCanaryModelConfig {
                    encoder: encoder.as_ptr(),
                    decoder: decoder.as_ptr(),
                    src_lang: LANG_EN.as_ptr() as *const c_char,
                    tgt_lang: LANG_EN.as_ptr() as *const c_char,
                    use_pnc: 1,
                },
                ..common_model_config(tokens.as_ptr(), num_threads)
            };
            create(model_config, None)
        };
        Self::from_raw(recognizer)
    }

    /// Moonshine: препроцессор, энкодер и декодер в двух видах — с кэшем и
    /// без. Первый шаг декодирования идёт по «холодному» графу, остальные —
    /// по кэшированному; sherpa переключает их сама.
    pub fn moonshine(
        preprocessor_path: &Path,
        encoder_path: &Path,
        uncached_decoder_path: &Path,
        cached_decoder_path: &Path,
        tokens_path: &Path,
        num_threads: i32,
    ) -> Result<Self, String> {
        check_threads(num_threads)?;
        let preprocessor = c_path(preprocessor_path)?;
        let encoder = c_path(encoder_path)?;
        let uncached = c_path(uncached_decoder_path)?;
        let cached = c_path(cached_decoder_path)?;
        let tokens = c_path(tokens_path)?;

        let recognizer = unsafe {
            use sherpa_rs::sherpa_rs_sys as sys;
            let model_config = sys::SherpaOnnxOfflineModelConfig {
                moonshine: sys::SherpaOnnxOfflineMoonshineModelConfig {
                    preprocessor: preprocessor.as_ptr(),
                    encoder: encoder.as_ptr(),
                    uncached_decoder: uncached.as_ptr(),
                    cached_decoder: cached.as_ptr(),
                },
                ..common_model_config(tokens.as_ptr(), num_threads)
            };
            create(model_config, None)
        };
        Self::from_raw(recognizer)
    }

    /// SenseVoice: один граф и таблица токенов, как у NeMo CTC, но со своим
    /// полем конфигурации. `auto` оставляет определение языка самой модели;
    /// `use_itn` включает обратную нормализацию — числа и даты приходят
    /// цифрами, а не словами.
    pub fn sense_voice(
        model_path: &Path,
        tokens_path: &Path,
        num_threads: i32,
    ) -> Result<Self, String> {
        check_threads(num_threads)?;
        let model = c_path(model_path)?;
        let tokens = c_path(tokens_path)?;

        let recognizer = unsafe {
            use sherpa_rs::sherpa_rs_sys as sys;
            let model_config = sys::SherpaOnnxOfflineModelConfig {
                sense_voice: sys::SherpaOnnxOfflineSenseVoiceModelConfig {
                    model: model.as_ptr(),
                    language: LANG_AUTO.as_ptr() as *const c_char,
                    use_itn: 1,
                },
                ..common_model_config(tokens.as_ptr(), num_threads)
            };
            create(model_config, None)
        };
        Self::from_raw(recognizer)
    }

    /// NeMo CTC (GigaAM): one graph plus its token table.
    pub fn nemo_ctc(
        model_path: &Path,
        tokens_path: &Path,
        num_threads: i32,
    ) -> Result<Self, String> {
        check_threads(num_threads)?;
        let model = c_path(model_path)?;
        let tokens = c_path(tokens_path)?;

        let recognizer = unsafe {
            use sherpa_rs::sherpa_rs_sys as sys;
            let model_config = sys::SherpaOnnxOfflineModelConfig {
                nemo_ctc: sys::SherpaOnnxOfflineNemoEncDecCtcModelConfig {
                    model: model.as_ptr(),
                },
                ..common_model_config(tokens.as_ptr(), num_threads)
            };
            create(model_config, None)
        };
        Self::from_raw(recognizer)
    }

    /// NeMo transducer (Parakeet TDT): three graphs — encoder, decoder and
    /// joiner — over the shared token table.
    ///
    /// `model_type` stays null on purpose so sherpa-onnx reads the variant
    /// out of the encoder's ONNX metadata. Spelling it as `"transducer"`,
    /// which is what `sherpa-rs`'s own wrapper defaults to, would mis-decode
    /// a TDT graph: those emit a duration alongside every symbol.
    pub fn transducer(
        encoder_path: &Path,
        decoder_path: &Path,
        joiner_path: &Path,
        tokens_path: &Path,
        num_threads: i32,
    ) -> Result<Self, String> {
        check_threads(num_threads)?;
        let encoder = c_path(encoder_path)?;
        let decoder = c_path(decoder_path)?;
        let joiner = c_path(joiner_path)?;
        let tokens = c_path(tokens_path)?;

        let recognizer = unsafe {
            use sherpa_rs::sherpa_rs_sys as sys;
            let model_config = sys::SherpaOnnxOfflineModelConfig {
                transducer: sys::SherpaOnnxOfflineTransducerModelConfig {
                    encoder: encoder.as_ptr(),
                    decoder: decoder.as_ptr(),
                    joiner: joiner.as_ptr(),
                },
                ..common_model_config(tokens.as_ptr(), num_threads)
            };
            // Unlike the CTC path, the transducer wants its feature
            // extractor spelled out: 80-dim log-mel at 16 kHz, which is what
            // every NeMo export expects.
            create(
                model_config,
                Some(sys::SherpaOnnxFeatureConfig {
                    sample_rate: 16_000,
                    feature_dim: 80,
                }),
            )
        };
        Self::from_raw(recognizer)
    }

    fn from_raw(
        recognizer: *const sherpa_rs::sherpa_rs_sys::SherpaOnnxOfflineRecognizer,
    ) -> Result<Self, String> {
        if recognizer.is_null() {
            return Err("SHERPA_CREATE_FAILED: offline recognizer returned null".to_string());
        }
        Ok(Self { recognizer })
    }

    pub fn transcribe(&mut self, sample_rate: u32, samples: &[f32]) -> Result<String, String> {
        validate_audio(sample_rate, samples)?;
        if samples.is_empty() {
            return Ok(String::new());
        }

        unsafe {
            use sherpa_rs::sherpa_rs_sys as sys;
            let stream = sys::SherpaOnnxCreateOfflineStream(self.recognizer);
            if stream.is_null() {
                return Err("SHERPA_STREAM_FAILED: offline stream returned null".to_string());
            }
            sys::SherpaOnnxAcceptWaveformOffline(
                stream,
                sample_rate as i32,
                samples.as_ptr(),
                samples.len() as i32,
            );
            sys::SherpaOnnxDecodeOfflineStream(self.recognizer, stream);
            let result_ptr = sys::SherpaOnnxGetOfflineStreamResult(stream);
            if result_ptr.is_null() {
                sys::SherpaOnnxDestroyOfflineStream(stream);
                return Err("SHERPA_RESULT_FAILED: offline result returned null".to_string());
            }
            let text = if (*result_ptr).text.is_null() {
                Err("SHERPA_RESULT_FAILED: result text returned null".to_string())
            } else {
                Ok(CStr::from_ptr((*result_ptr).text)
                    .to_string_lossy()
                    .into_owned())
            };
            sys::SherpaOnnxDestroyOfflineRecognizerResult(result_ptr);
            sys::SherpaOnnxDestroyOfflineStream(stream);
            text
        }
    }
}

/// Проверки, общие для обоих распознавателей. NaN в буфере sherpa не
/// проверяет, а на несовместимом входе отвечает падением процесса, поэтому
/// звук осматривается до C-границы.
#[cfg(windows)]
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

#[cfg(windows)]
fn check_threads(num_threads: i32) -> Result<(), String> {
    if num_threads <= 0 {
        return Err("SHERPA_INVALID_THREADS: num_threads must be positive".to_string());
    }
    Ok(())
}

/// Every field a family does not use must stay null/zero: sherpa-onnx picks
/// the implementation by the first non-empty model field.
///
/// # Safety
/// `tokens` must outlive the returned config.
#[cfg(windows)]
unsafe fn common_model_config(
    tokens: *const c_char,
    num_threads: i32,
) -> sherpa_rs::sherpa_rs_sys::SherpaOnnxOfflineModelConfig {
    use sherpa_rs::sherpa_rs_sys as sys;
    sys::SherpaOnnxOfflineModelConfig {
        tokens,
        num_threads,
        debug: 0,
        provider: PROVIDER_CPU.as_ptr() as *const c_char,
        ..std::mem::zeroed()
    }
}

/// # Safety
/// Every path inside `model_config` must outlive this call.
#[cfg(windows)]
unsafe fn create(
    model_config: sherpa_rs::sherpa_rs_sys::SherpaOnnxOfflineModelConfig,
    feat_config: Option<sherpa_rs::sherpa_rs_sys::SherpaOnnxFeatureConfig>,
) -> *const sherpa_rs::sherpa_rs_sys::SherpaOnnxOfflineRecognizer {
    use sherpa_rs::sherpa_rs_sys as sys;
    let config = sys::SherpaOnnxOfflineRecognizerConfig {
        model_config,
        decoding_method: DECODING_GREEDY.as_ptr() as *const c_char,
        feat_config: feat_config.unwrap_or_else(|| std::mem::zeroed()),
        ..std::mem::zeroed()
    };
    sys::SherpaOnnxCreateOfflineRecognizer(&config)
}

#[cfg(windows)]
impl Drop for OfflineRecognizer {
    fn drop(&mut self) {
        if !self.recognizer.is_null() {
            unsafe {
                sherpa_rs::sherpa_rs_sys::SherpaOnnxDestroyOfflineRecognizer(self.recognizer);
            }
            self.recognizer = std::ptr::null();
        }
    }
}

/// Keep the engine command surface cross-platform while making the
/// unsupported native runtime explicit. The bundle registry is empty on these
/// targets, so this is a defensive error path rather than a user-visible
/// model option.
#[cfg(not(windows))]
#[derive(Debug)]
pub struct OfflineRecognizer;

#[cfg(not(windows))]
const UNSUPPORTED: &str = "SHERPA_UNSUPPORTED_PLATFORM: ONNX models are Windows-only";

#[cfg(not(windows))]
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

    #[cfg(windows)]
    #[test]
    fn rejects_invalid_thread_count_before_ffi() {
        let err = OfflineRecognizer::nemo_ctc(Path::new("model.onnx"), Path::new("tokens.txt"), 0)
            .unwrap_err();
        assert!(err.contains("SHERPA_INVALID_THREADS"));
    }

    #[cfg(windows)]
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

    #[cfg(not(windows))]
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

/// Потоковый распознаватель sherpa.
///
/// Отличается от офлайнового не моделью, а разговором: аудио подаётся
/// кусками, распознаватель сам сообщает, набралось ли достаточно для шага
/// декодирования, и на любом шаге отдаёт текущую гипотезу. Поток живёт между
/// вызовами, поэтому он тут поле, а не локальная переменная.
#[cfg(windows)]
#[derive(Debug)]
pub struct OnlineRecognizer {
    recognizer: *const sherpa_rs::sherpa_rs_sys::SherpaOnnxOnlineRecognizer,
    stream: *const sherpa_rs::sherpa_rs_sys::SherpaOnnxOnlineStream,
}

#[cfg(windows)]
impl OnlineRecognizer {
    pub fn streaming_transducer(
        encoder_path: &Path,
        decoder_path: &Path,
        joiner_path: &Path,
        tokens_path: &Path,
        num_threads: i32,
    ) -> Result<Self, String> {
        check_threads(num_threads)?;
        let encoder = c_path(encoder_path)?;
        let decoder = c_path(decoder_path)?;
        let joiner = c_path(joiner_path)?;
        let tokens = c_path(tokens_path)?;

        let recognizer = unsafe {
            use sherpa_rs::sherpa_rs_sys as sys;
            let model_config = sys::SherpaOnnxOnlineModelConfig {
                transducer: sys::SherpaOnnxOnlineTransducerModelConfig {
                    encoder: encoder.as_ptr(),
                    decoder: decoder.as_ptr(),
                    joiner: joiner.as_ptr(),
                },
                tokens: tokens.as_ptr(),
                num_threads,
                debug: 0,
                provider: PROVIDER_CPU.as_ptr() as *const c_char,
                ..std::mem::zeroed()
            };
            let config = sys::SherpaOnnxOnlineRecognizerConfig {
                feat_config: sys::SherpaOnnxFeatureConfig {
                    sample_rate: 16_000,
                    feature_dim: 80,
                },
                model_config,
                decoding_method: DECODING_GREEDY.as_ptr() as *const c_char,
                // Границы диктовки задаёт горячая клавиша, а не тишина в
                // микрофоне: со включённым определением конца фразы
                // распознаватель обрывал бы паузу посреди мысли и начинал
                // фразу заново.
                enable_endpoint: 0,
                ..std::mem::zeroed()
            };
            sys::SherpaOnnxCreateOnlineRecognizer(&config)
        };
        if recognizer.is_null() {
            return Err("SHERPA_CREATE_FAILED: online recognizer returned null".to_string());
        }
        let stream = unsafe { sherpa_rs::sherpa_rs_sys::SherpaOnnxCreateOnlineStream(recognizer) };
        if stream.is_null() {
            unsafe { sherpa_rs::sherpa_rs_sys::SherpaOnnxDestroyOnlineRecognizer(recognizer) };
            return Err("SHERPA_STREAM_FAILED: online stream returned null".to_string());
        }
        Ok(Self { recognizer, stream })
    }

    /// Подать очередной кусок звука и продвинуть декодирование настолько,
    /// насколько данных уже хватает. До конца фразы не блокирует.
    pub fn feed(&mut self, sample_rate: u32, samples: &[f32]) -> Result<(), String> {
        validate_audio(sample_rate, samples)?;
        if samples.is_empty() {
            return Ok(());
        }
        unsafe {
            use sherpa_rs::sherpa_rs_sys as sys;
            sys::SherpaOnnxOnlineStreamAcceptWaveform(
                self.stream,
                sample_rate as i32,
                samples.as_ptr(),
                samples.len() as i32,
            );
            self.decode_ready();
        }
        Ok(())
    }

    /// Текущая гипотеза целиком. Текст растёт и может исправляться задним
    /// числом, поэтому вставлять его никуда нельзя — только показывать.
    pub fn text(&self) -> Result<String, String> {
        unsafe {
            use sherpa_rs::sherpa_rs_sys as sys;
            let result_ptr = sys::SherpaOnnxGetOnlineStreamResult(self.recognizer, self.stream);
            if result_ptr.is_null() {
                return Err("SHERPA_RESULT_FAILED: online result returned null".to_string());
            }
            let text = if (*result_ptr).text.is_null() {
                String::new()
            } else {
                CStr::from_ptr((*result_ptr).text)
                    .to_string_lossy()
                    .into_owned()
            };
            sys::SherpaOnnxDestroyOnlineRecognizerResult(result_ptr);
            Ok(text)
        }
    }

    /// Закрыть фразу: досчитать хвост и вернуть финальный текст.
    pub fn finish(&mut self) -> Result<String, String> {
        unsafe {
            use sherpa_rs::sherpa_rs_sys as sys;
            sys::SherpaOnnxOnlineStreamInputFinished(self.stream);
            self.decode_ready();
        }
        self.text()
    }

    /// Забыть накопленное и начать следующую диктовку с чистого листа.
    pub fn reset(&mut self) {
        unsafe {
            sherpa_rs::sherpa_rs_sys::SherpaOnnxOnlineStreamReset(self.recognizer, self.stream)
        };
    }

    /// # Safety
    /// Поток и распознаватель должны быть живы.
    unsafe fn decode_ready(&self) {
        use sherpa_rs::sherpa_rs_sys as sys;
        while sys::SherpaOnnxIsOnlineStreamReady(self.recognizer, self.stream) != 0 {
            sys::SherpaOnnxDecodeOnlineStream(self.recognizer, self.stream);
        }
    }
}

#[cfg(windows)]
impl Drop for OnlineRecognizer {
    fn drop(&mut self) {
        unsafe {
            use sherpa_rs::sherpa_rs_sys as sys;
            if !self.stream.is_null() {
                sys::SherpaOnnxDestroyOnlineStream(self.stream);
                self.stream = std::ptr::null();
            }
            if !self.recognizer.is_null() {
                sys::SherpaOnnxDestroyOnlineRecognizer(self.recognizer);
                self.recognizer = std::ptr::null();
            }
        }
    }
}

#[cfg(not(windows))]
#[derive(Debug)]
pub struct OnlineRecognizer;

#[cfg(not(windows))]
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

/// Распознаватель любого семейства sherpa — потоковый или нет.
///
/// Поток движка держит именно это: ему важно, что модель загружена и умеет
/// расшифровать буфер, а из скольких графов она собрана и по какому API
/// работает — дело этого модуля.
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

    /// Расшифровать готовый буфер целиком. Потоковый распознаватель идёт тем
    /// же путём, что и живой предпросмотр, только без пауз: подать всё,
    /// закрыть фразу, забрать текст.
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

    /// Забыть накопленную гипотезу. У непотокового распознавателя копить
    /// нечего, поэтому вызов безвреден.
    pub fn reset_preview(&mut self) {
        if let Self::Online(recognizer) = self {
            recognizer.reset();
        }
    }

    /// Живой предпросмотр: подать кусок и вернуть текущую гипотезу. У
    /// непотокового распознавателя гипотезы нет — он молчит до конца записи.
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
