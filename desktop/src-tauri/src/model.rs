//! Native Whisper model catalogue and on-disk cache helpers.
//!
//! Phase 4 stores whisper.cpp/GGML files in one platform-native cache directory.
//! The public model id remains compatible with the existing frontend (`turbo`),
//! while the corresponding GGML filename uses the upstream name
//! `ggml-large-v3-turbo-q8_0.bin`.

use std::path::{Path, PathBuf};

use serde::Serialize;
use sha2::Digest;

const MIN_VALID_MODEL_BYTES: u64 = 10 * 1024 * 1024;

/// Every GGML model in the catalog comes from one family; bundles have theirs.
const WHISPER_FAMILY: &str = "Whisper";

/// Languages of multilingual Whisper, ISO 639-1 (with a couple of exceptions
/// such as `yue`, as the model itself names them).
///
/// The list is spelled out in full rather than folded into "multilingual": the
/// user does not ask "are there many languages" but "is mine there", and only
/// an enumeration can answer that.
const WHISPER_LANGUAGES: &[&str] = &[
    "en", "zh", "de", "es", "ru", "ko", "fr", "ja", "pt", "tr", "pl", "ca", "nl", "ar", "sv", "it",
    "id", "hi", "fi", "vi", "he", "uk", "el", "ms", "cs", "ro", "da", "hu", "ta", "no", "th", "ur",
    "hr", "bg", "lt", "la", "mi", "ml", "cy", "sk", "te", "fa", "lv", "bn", "sr", "az", "sl", "kn",
    "et", "mk", "br", "eu", "is", "hy", "ne", "mn", "bs", "kk", "sq", "sw", "gl", "mr", "pa", "si",
    "km", "sn", "yo", "so", "af", "oc", "ka", "be", "tg", "sd", "gu", "am", "yi", "lo", "uz", "fo",
    "ht", "ps", "tk", "nn", "mt", "sa", "lb", "my", "bo", "tl", "mg", "as", "tt", "haw", "ln",
    "ha", "ba", "jw", "su", "yue",
];

/// The twenty-five European languages of Parakeet TDT v3 — exactly the ones the
/// model was trained on, not everything found in its token vocabulary.
///
/// Available on Windows/macOS, like the whole sherpa bundle catalog: on other
/// platforms it is not built and the constant would be dead.
#[cfg(any(windows, target_os = "macos"))]
const PARAKEET_V3_LANGUAGES: &[&str] = &[
    "bg", "hr", "cs", "da", "nl", "en", "et", "fi", "fr", "de", "el", "hu", "it", "lv", "lt", "mt",
    "pl", "pt", "ro", "sk", "sl", "es", "sv", "ru", "uk",
];

/// Authoritative manifest entry for one GGML model. The Rust
/// downloader (PR 1.1) treats this struct as the only source of truth
/// for `url`, `expected_bytes`, and `sha256`. The public `id` is
/// the value used by config / UI; `file_name` is the upstream
/// `ggml-*.bin` filename on the Hugging Face mirror.
///
/// `Copy` so it can live in a `&'static [ModelManifestEntry]`
/// alongside its lifetime-free string fields.
#[derive(Debug, Clone, Copy)]
pub struct ModelManifestEntry {
    pub public_id: &'static str,
    pub file_name: &'static str,
    pub download_url: &'static str,
    pub expected_bytes: u64,
    pub sha256: &'static str,
    pub recommended: bool,
}

/// A closed, multi-file model bundle. Unlike user supplied Whisper `.bin`
/// files, Sherpa/ONNX models are never auto-discovered: the C API can abort
/// the process when given an incompatible graph, so every artifact is pinned
/// to an exact URL, size and SHA-256.
/// What a bundle file *is*, so the loader can find it without knowing the
/// upstream naming of any particular model. A CTC bundle has `Model` +
/// `Tokens`; a transducer has `Encoder` + `Decoder` + `Joiner` + `Tokens`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArtifactRole {
    Model,
    Encoder,
    Decoder,
    Joiner,
    /// Moonshine splits the decoder into two graphs and moves feature
    /// extraction into a separate preprocessor.
    Preprocessor,
    CachedDecoder,
    UncachedDecoder,
    Tokens,
}

#[derive(Debug, Clone, Copy)]
pub struct BundleArtifactManifestEntry {
    pub role: ArtifactRole,
    pub file_name: &'static str,
    pub download_url: &'static str,
    pub expected_bytes: u64,
    pub sha256: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct BundleModelManifestEntry {
    pub public_id: &'static str,
    pub directory_name: &'static str,
    pub artifacts: &'static [BundleArtifactManifestEntry],
    pub engine: ModelEngine,
    /// The family under whose heading the model stands in the catalog. It is a
    /// proper name and arrives from the backend ready-made: a new family must
    /// not require a frontend change to get a heading.
    pub family: &'static str,
    /// The languages the model supports, if its list is closed. A model asked
    /// about a foreign language does not fail — it silently produces garbage, so
    /// the pair is rejected up front. `None` — the model is multilingual and
    /// imposes no restrictions.
    pub languages: Option<&'static [&'static str]>,
    pub label: &'static str,
    pub size: &'static str,
    pub ram: &'static str,
    pub recommended: bool,
}

#[cfg(any(windows, target_os = "macos"))]
const GIGAAM_V3_ARTIFACTS: &[BundleArtifactManifestEntry] = &[
    BundleArtifactManifestEntry {
        role: ArtifactRole::Model,
        file_name: "model.int8.onnx",
        download_url: "https://huggingface.co/csukuangfj/sherpa-onnx-nemo-ctc-giga-am-v3-russian-2025-12-16/resolve/f376a99ee8be93b61f9e969d2ac827c4d228dac3/model.int8.onnx",
        expected_bytes: 224_721_476,
        sha256: "f86ebfa0429ced91be6054fc344827e9c6c2572f3c318416cd974b06f66437ec",
    },
    BundleArtifactManifestEntry {
        role: ArtifactRole::Tokens,
        file_name: "tokens.txt",
        download_url: "https://huggingface.co/csukuangfj/sherpa-onnx-nemo-ctc-giga-am-v3-russian-2025-12-16/resolve/f376a99ee8be93b61f9e969d2ac827c4d228dac3/tokens.txt",
        expected_bytes: 196,
        sha256: "17cc514451bcceac9c280068c71502f8448f99e9fb1456b8d0761651fd0392f2",
    },
];

#[cfg(any(windows, target_os = "macos"))]
const PARAKEET_TDT_V3_ARTIFACTS: &[BundleArtifactManifestEntry] = &[
    BundleArtifactManifestEntry {
        role: ArtifactRole::Encoder,
        file_name: "encoder.int8.onnx",
        download_url: concat!(
            "https://huggingface.co/csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8/resolve/2bda32ec70b097a55adaa07d9a7173915b43cc78",
            "/encoder.int8.onnx"
        ),
        expected_bytes: 652_184_281,
        sha256: "acfc2b4456377e15d04f0243af540b7fe7c992f8d898d751cf134c3a55fd2247",
    },
    BundleArtifactManifestEntry {
        role: ArtifactRole::Decoder,
        file_name: "decoder.int8.onnx",
        download_url: concat!(
            "https://huggingface.co/csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8/resolve/2bda32ec70b097a55adaa07d9a7173915b43cc78",
            "/decoder.int8.onnx"
        ),
        expected_bytes: 11_845_275,
        sha256: "179e50c43d1a9de79c8a24149a2f9bac6eb5981823f2a2ed88d655b24248db4e",
    },
    BundleArtifactManifestEntry {
        role: ArtifactRole::Joiner,
        file_name: "joiner.int8.onnx",
        download_url: concat!(
            "https://huggingface.co/csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8/resolve/2bda32ec70b097a55adaa07d9a7173915b43cc78",
            "/joiner.int8.onnx"
        ),
        expected_bytes: 6_355_277,
        sha256: "3164c13fc2821009440d20fcb5fdc78bff28b4db2f8d0f0b329101719c0948b3",
    },
    BundleArtifactManifestEntry {
        role: ArtifactRole::Tokens,
        file_name: "tokens.txt",
        download_url: concat!(
            "https://huggingface.co/csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8/resolve/2bda32ec70b097a55adaa07d9a7173915b43cc78",
            "/tokens.txt"
        ),
        expected_bytes: 93_939,
        sha256: "d58544679ea4bc6ac563d1f545eb7d474bd6cfa467f0a6e2c1dc1c7d37e3c35d",
    },
];

#[cfg(any(windows, target_os = "macos"))]
const PARAKEET_TDT_V2_EN_ARTIFACTS: &[BundleArtifactManifestEntry] = &[
    BundleArtifactManifestEntry {
        role: ArtifactRole::Encoder,
        file_name: "encoder.int8.onnx",
        download_url: concat!(
            "https://huggingface.co/csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8/resolve/1ab9323565ddb038682214b292f588070a538ce2",
            "/encoder.int8.onnx"
        ),
        expected_bytes: 652_184_296,
        sha256: "a32b12d17bbbc309d0686fbbcc2987b5e9b8333a7da83fa6b089f0a2acd651ab",
    },
    BundleArtifactManifestEntry {
        role: ArtifactRole::Decoder,
        file_name: "decoder.int8.onnx",
        download_url: concat!(
            "https://huggingface.co/csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8/resolve/1ab9323565ddb038682214b292f588070a538ce2",
            "/decoder.int8.onnx"
        ),
        expected_bytes: 7_257_753,
        sha256: "b6bb64963457237b900e496ee9994b59294526439fbcc1fecf705b31a15c6b4e",
    },
    BundleArtifactManifestEntry {
        role: ArtifactRole::Joiner,
        file_name: "joiner.int8.onnx",
        download_url: concat!(
            "https://huggingface.co/csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8/resolve/1ab9323565ddb038682214b292f588070a538ce2",
            "/joiner.int8.onnx"
        ),
        expected_bytes: 1_739_080,
        sha256: "7946164367946e7f9f29a122407c3252b680dbae9a51343eb2488d057c3c43d2",
    },
    BundleArtifactManifestEntry {
        role: ArtifactRole::Tokens,
        file_name: "tokens.txt",
        download_url: concat!(
            "https://huggingface.co/csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8/resolve/1ab9323565ddb038682214b292f588070a538ce2",
            "/tokens.txt"
        ),
        expected_bytes: 9_384,
        sha256: "ec182b70dd42113aff6c5372c75cac58c952443eb22322f57bbd7f53977d497d",
    },
];

#[cfg(any(windows, target_os = "macos"))]
const CANARY_180M_ARTIFACTS: &[BundleArtifactManifestEntry] = &[
    BundleArtifactManifestEntry {
        role: ArtifactRole::Encoder,
        file_name: "encoder.int8.onnx",
        download_url: concat!("https://huggingface.co/csukuangfj/sherpa-onnx-nemo-canary-180m-flash-en-es-de-fr-int8/resolve/9077164e0d3dd1d5353743e89ceaa1d3a770838c", "/encoder.int8.onnx"),
        expected_bytes: 132_678_643,
        sha256: "7a75b4e2a5857a6dcc0819503bbe3fad66943db4a3ccf21d3f27c633667d303f",
    },
    BundleArtifactManifestEntry {
        role: ArtifactRole::Decoder,
        file_name: "decoder.int8.onnx",
        download_url: concat!("https://huggingface.co/csukuangfj/sherpa-onnx-nemo-canary-180m-flash-en-es-de-fr-int8/resolve/9077164e0d3dd1d5353743e89ceaa1d3a770838c", "/decoder.int8.onnx"),
        expected_bytes: 74_437_848,
        sha256: "e41a2ab9c0c2fe81a1e8ade5a45fb02a74bc4db7d1f91b89a54a25e2cf79cba2",
    },
    BundleArtifactManifestEntry {
        role: ArtifactRole::Tokens,
        file_name: "tokens.txt",
        download_url: concat!("https://huggingface.co/csukuangfj/sherpa-onnx-nemo-canary-180m-flash-en-es-de-fr-int8/resolve/9077164e0d3dd1d5353743e89ceaa1d3a770838c", "/tokens.txt"),
        expected_bytes: 53_555,
        sha256: "2dae6fc7815f9640645e0c765522b278ee0cef49b482d91f6913e334628d3e77",
    },
];

#[cfg(any(windows, target_os = "macos"))]
const SENSE_VOICE_ARTIFACTS: &[BundleArtifactManifestEntry] = &[
    BundleArtifactManifestEntry {
        role: ArtifactRole::Model,
        file_name: "model.int8.onnx",
        download_url: concat!("https://huggingface.co/csukuangfj/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17/resolve/2365baeacb507f821a0c8120fcee3d484dba7a07", "/model.int8.onnx"),
        expected_bytes: 239_233_841,
        sha256: "c71f0ce00bec95b07744e116345e33d8cbbe08cef896382cf907bf4b51a2cd51",
    },
    BundleArtifactManifestEntry {
        role: ArtifactRole::Tokens,
        file_name: "tokens.txt",
        download_url: concat!("https://huggingface.co/csukuangfj/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17/resolve/2365baeacb507f821a0c8120fcee3d484dba7a07", "/tokens.txt"),
        expected_bytes: 315_894,
        sha256: "f449eb28dc567533d7fa59be34e2abca8784f771850c78a47fb731a31429a1dc",
    },
];

#[cfg(any(windows, target_os = "macos"))]
const MOONSHINE_BASE_EN_ARTIFACTS: &[BundleArtifactManifestEntry] = &[
    BundleArtifactManifestEntry {
        role: ArtifactRole::Preprocessor,
        file_name: "preprocess.onnx",
        download_url: concat!("https://huggingface.co/csukuangfj/sherpa-onnx-moonshine-base-en-int8/resolve/052b0798ad1bf046a140fdd4efcd9426530fa3f5", "/preprocess.onnx"),
        expected_bytes: 14_077_290,
        sha256: "ffa630d395c5ccf76f5d4954be5b882df76aaf6491519ec01fd82ea7a3819fb2",
    },
    BundleArtifactManifestEntry {
        role: ArtifactRole::Encoder,
        file_name: "encode.int8.onnx",
        download_url: concat!("https://huggingface.co/csukuangfj/sherpa-onnx-moonshine-base-en-int8/resolve/052b0798ad1bf046a140fdd4efcd9426530fa3f5", "/encode.int8.onnx"),
        expected_bytes: 50_311_494,
        sha256: "7e38770f776f2e5583a53b052936005df2ba5c833d7e09c2a5fd796b94bf73e2",
    },
    BundleArtifactManifestEntry {
        role: ArtifactRole::UncachedDecoder,
        file_name: "uncached_decode.int8.onnx",
        download_url: concat!("https://huggingface.co/csukuangfj/sherpa-onnx-moonshine-base-en-int8/resolve/052b0798ad1bf046a140fdd4efcd9426530fa3f5", "/uncached_decode.int8.onnx"),
        expected_bytes: 122_120_451,
        sha256: "c01f4b35093bcac20d352d23a75a539e772964579f9d024a90e5e6f09cae9987",
    },
    BundleArtifactManifestEntry {
        role: ArtifactRole::CachedDecoder,
        file_name: "cached_decode.int8.onnx",
        download_url: concat!("https://huggingface.co/csukuangfj/sherpa-onnx-moonshine-base-en-int8/resolve/052b0798ad1bf046a140fdd4efcd9426530fa3f5", "/cached_decode.int8.onnx"),
        expected_bytes: 99_983_837,
        sha256: "2db74e51cedf64a8b1be3c8192e0bb5e4923af0e90bd9e87f8e8771873f8ea03",
    },
    BundleArtifactManifestEntry {
        role: ArtifactRole::Tokens,
        file_name: "tokens.txt",
        download_url: concat!("https://huggingface.co/csukuangfj/sherpa-onnx-moonshine-base-en-int8/resolve/052b0798ad1bf046a140fdd4efcd9426530fa3f5", "/tokens.txt"),
        expected_bytes: 436_688,
        sha256: "1165c2aeb9f72f457a83be2d459a09054f27490acd9b41bd43794dfd25e296ea",
    },
];

#[cfg(any(windows, target_os = "macos"))]
const ZIPFORMER_RU_ARTIFACTS: &[BundleArtifactManifestEntry] = &[
    BundleArtifactManifestEntry {
        role: ArtifactRole::Encoder,
        file_name: "encoder.int8.onnx",
        download_url: concat!("https://huggingface.co/csukuangfj/sherpa-onnx-zipformer-ru-int8-2025-04-20/resolve/641de8d322c05b9087ad2927ccda4bda3cccc159", "/encoder.int8.onnx"),
        expected_bytes: 70_876_638,
        sha256: "eb6c12fbad810d5bc3e427802e604604c69b5943a91feebc43424dd09d9ec407",
    },
    BundleArtifactManifestEntry {
        role: ArtifactRole::Decoder,
        file_name: "decoder.onnx",
        download_url: concat!("https://huggingface.co/csukuangfj/sherpa-onnx-zipformer-ru-int8-2025-04-20/resolve/641de8d322c05b9087ad2927ccda4bda3cccc159", "/decoder.onnx"),
        expected_bytes: 2_093_080,
        sha256: "dcbe1ffa0211e77ca6d3a80164df13fbda3ec00e47d12b9f449f89572df12136",
    },
    BundleArtifactManifestEntry {
        role: ArtifactRole::Joiner,
        file_name: "joiner.int8.onnx",
        download_url: concat!("https://huggingface.co/csukuangfj/sherpa-onnx-zipformer-ru-int8-2025-04-20/resolve/641de8d322c05b9087ad2927ccda4bda3cccc159", "/joiner.int8.onnx"),
        expected_bytes: 259_417,
        sha256: "93f2e1d12b78d53e7802f1606488c14bb3d764b15fadf5ef6c022f6ba1fa40f7",
    },
    BundleArtifactManifestEntry {
        role: ArtifactRole::Tokens,
        file_name: "tokens.txt",
        download_url: concat!("https://huggingface.co/csukuangfj/sherpa-onnx-zipformer-ru-int8-2025-04-20/resolve/641de8d322c05b9087ad2927ccda4bda3cccc159", "/tokens.txt"),
        expected_bytes: 6_388,
        sha256: "93bbbc0bae6b78c0bbb743d4aa9fded3bb5ff3aac5f0200e3a769a5a05e0fdf6",
    },
];

#[cfg(any(windows, target_os = "macos"))]
const ZIPFORMER_RU_STREAMING_ARTIFACTS: &[BundleArtifactManifestEntry] = &[
    BundleArtifactManifestEntry {
        role: ArtifactRole::Encoder,
        file_name: "encoder.int8.onnx",
        download_url: concat!("https://huggingface.co/csukuangfj/sherpa-onnx-streaming-zipformer-small-ru-vosk-int8-2025-08-16/resolve/31fa603e4f31279c6e1f7600fed13dc4312663ab", "/encoder.int8.onnx"),
        expected_bytes: 26_214_060,
        sha256: "e0db705e94ec35d803b1df4f40cda23d064e1142977c80ab288430b109777a9d",
    },
    BundleArtifactManifestEntry {
        role: ArtifactRole::Decoder,
        file_name: "decoder.onnx",
        download_url: concat!("https://huggingface.co/csukuangfj/sherpa-onnx-streaming-zipformer-small-ru-vosk-int8-2025-08-16/resolve/31fa603e4f31279c6e1f7600fed13dc4312663ab", "/decoder.onnx"),
        expected_bytes: 2_093_080,
        sha256: "89b3088a9e20e1ef7f2e85ce1a3478afe6a9c4ac57369cabcc4beb8e95328ea0",
    },
    BundleArtifactManifestEntry {
        role: ArtifactRole::Joiner,
        file_name: "joiner.int8.onnx",
        download_url: concat!("https://huggingface.co/csukuangfj/sherpa-onnx-streaming-zipformer-small-ru-vosk-int8-2025-08-16/resolve/31fa603e4f31279c6e1f7600fed13dc4312663ab", "/joiner.int8.onnx"),
        expected_bytes: 259_417,
        sha256: "b55784b071ab7512eab4c7c44e4f5478284ef33c83562cc6a249b972515a31e5",
    },
    BundleArtifactManifestEntry {
        role: ArtifactRole::Tokens,
        file_name: "tokens.txt",
        download_url: concat!("https://huggingface.co/csukuangfj/sherpa-onnx-streaming-zipformer-small-ru-vosk-int8-2025-08-16/resolve/31fa603e4f31279c6e1f7600fed13dc4312663ab", "/tokens.txt"),
        expected_bytes: 6_388,
        sha256: "93bbbc0bae6b78c0bbb743d4aa9fded3bb5ff3aac5f0200e3a769a5a05e0fdf6",
    },
];

#[cfg(any(windows, target_os = "macos"))]
const PARAKEET_STREAMING_EN_ARTIFACTS: &[BundleArtifactManifestEntry] = &[
    BundleArtifactManifestEntry {
        role: ArtifactRole::Encoder,
        file_name: "encoder.int8.onnx",
        download_url: concat!("https://huggingface.co/csukuangfj2/sherpa-onnx-nemo-parakeet-unified-en-0.6b-int8-streaming-560ms/resolve/7551fd26fc810cc1e4e043e608db4d13b59be31e", "/encoder.int8.onnx"),
        expected_bytes: 654_046_389,
        sha256: "e566c3f014598a41724f2df028779a2d4cf7943cbefa324964f6a72e8ee255fb",
    },
    BundleArtifactManifestEntry {
        role: ArtifactRole::Decoder,
        file_name: "decoder.int8.onnx",
        download_url: concat!("https://huggingface.co/csukuangfj2/sherpa-onnx-nemo-parakeet-unified-en-0.6b-int8-streaming-560ms/resolve/7551fd26fc810cc1e4e043e608db4d13b59be31e", "/decoder.int8.onnx"),
        expected_bytes: 7_257_777,
        sha256: "34fea72425d2506600772ba191a6d3f99c0710abdb68d9a3dc89fa8cb2aa473a",
    },
    BundleArtifactManifestEntry {
        role: ArtifactRole::Joiner,
        file_name: "joiner.int8.onnx",
        download_url: concat!("https://huggingface.co/csukuangfj2/sherpa-onnx-nemo-parakeet-unified-en-0.6b-int8-streaming-560ms/resolve/7551fd26fc810cc1e4e043e608db4d13b59be31e", "/joiner.int8.onnx"),
        expected_bytes: 1_735_860,
        sha256: "869f43f7d24595c55581ad3bf249a935fb8a71389fbdaa7504b9f46f93140f8a",
    },
    BundleArtifactManifestEntry {
        role: ArtifactRole::Tokens,
        file_name: "tokens.txt",
        download_url: concat!("https://huggingface.co/csukuangfj2/sherpa-onnx-nemo-parakeet-unified-en-0.6b-int8-streaming-560ms/resolve/7551fd26fc810cc1e4e043e608db4d13b59be31e", "/tokens.txt"),
        expected_bytes: 8_952,
        sha256: "dc0b4584ab2e4ddbf888425c076c61b736e7356a015250db7d307e6f1a8188ff",
    },
];

#[cfg(any(windows, target_os = "macos"))]
pub const BUNDLE_MODEL_MANIFEST: &[BundleModelManifestEntry] = &[
    BundleModelManifestEntry {
        public_id: "gigaam-v3",
        directory_name: "gigaam-v3",
        artifacts: GIGAAM_V3_ARTIFACTS,
        engine: ModelEngine::SherpaNemoCtc,
        family: "GigaAM",
        languages: Some(&["ru"]),
        label: "GigaAM v3",
        size: "214 MB",
        ram: "~0.5 GB",
        recommended: false,
    },
    BundleModelManifestEntry {
        public_id: "canary-180m-flash",
        directory_name: "canary-180m-flash",
        artifacts: CANARY_180M_ARTIFACTS,
        engine: ModelEngine::SherpaCanary,
        family: "Canary",
        // The model knows four languages, but the language pair is set when
        // the recognizer is created rather than per transcription. Ours is
        // pinned to English — see `sherpa::OfflineRecognizer::canary`.
        languages: Some(&["en"]),
        label: "Canary 180M Flash",
        size: "198 MB",
        ram: "~0.6 GB",
        recommended: false,
    },
    BundleModelManifestEntry {
        public_id: "moonshine-base-en",
        directory_name: "moonshine-base-en",
        artifacts: MOONSHINE_BASE_EN_ARTIFACTS,
        engine: ModelEngine::SherpaMoonshine,
        family: "Moonshine",
        languages: Some(&["en"]),
        label: "Moonshine base",
        size: "274 MB",
        ram: "~0.8 GB",
        recommended: false,
    },
    BundleModelManifestEntry {
        public_id: "sense-voice",
        directory_name: "sense-voice",
        artifacts: SENSE_VOICE_ARTIFACTS,
        engine: ModelEngine::SherpaSenseVoice,
        family: "SenseVoice",
        languages: Some(&["zh", "en", "ja", "ko", "yue"]),
        label: "SenseVoice small",
        size: "229 MB",
        ram: "~0.7 GB",
        recommended: false,
    },
    BundleModelManifestEntry {
        public_id: "zipformer-ru",
        directory_name: "zipformer-ru",
        artifacts: ZIPFORMER_RU_ARTIFACTS,
        engine: ModelEngine::SherpaTransducer,
        family: "Zipformer",
        languages: Some(&["ru"]),
        label: "Zipformer",
        size: "70 MB",
        ram: "~0.4 GB",
        recommended: false,
    },
    BundleModelManifestEntry {
        public_id: "zipformer-ru-streaming",
        directory_name: "zipformer-ru-streaming",
        artifacts: ZIPFORMER_RU_STREAMING_ARTIFACTS,
        engine: ModelEngine::SherpaStreamingTransducer,
        family: "Zipformer",
        languages: Some(&["ru"]),
        // "small" comes from the model's own upstream name, not our invention:
        // that is how it differs from the plain Zipformer of the same name.
        label: "Zipformer small",
        size: "27 MB",
        ram: "~0.3 GB",
        recommended: false,
    },
    BundleModelManifestEntry {
        public_id: "parakeet-streaming-en",
        directory_name: "parakeet-streaming-en",
        artifacts: PARAKEET_STREAMING_EN_ARTIFACTS,
        engine: ModelEngine::SherpaStreamingTransducer,
        family: "Parakeet",
        languages: Some(&["en"]),
        // Upstream publishes three variants with different look-ahead: 240, 560
        // and 1120 ms. The middle one is taken — at 240 ms the text appears
        // sooner but is rewritten retroactively more often, and it is precisely
        // the rewriting that grates in a live preview.
        label: "Parakeet unified",
        size: "632 MB",
        ram: "~1.4 GB",
        recommended: false,
    },
    BundleModelManifestEntry {
        public_id: "parakeet-tdt-v2-en",
        directory_name: "parakeet-tdt-v2-en",
        artifacts: PARAKEET_TDT_V2_EN_ARTIFACTS,
        engine: ModelEngine::SherpaTransducer,
        family: "Parakeet",
        languages: Some(&["en"]),
        label: "Parakeet TDT v2",
        size: "631 MB",
        ram: "~1.4 GB",
        recommended: false,
    },
    BundleModelManifestEntry {
        public_id: "parakeet-tdt-v3",
        directory_name: "parakeet-tdt-v3",
        artifacts: PARAKEET_TDT_V3_ARTIFACTS,
        engine: ModelEngine::SherpaTransducer,
        family: "Parakeet",
        languages: Some(PARAKEET_V3_LANGUAGES),
        label: "Parakeet TDT v3",
        size: "639 MB",
        ram: "~1.4 GB",
        recommended: false,
    },
];

/// GigaAM is not exposed on targets where its downloaded Sherpa runtime is
/// not packaged. Whisper remains available on those targets.
#[cfg(not(any(windows, target_os = "macos")))]
pub const BUNDLE_MODEL_MANIFEST: &[BundleModelManifestEntry] = &[];

pub fn bundle_manifest() -> &'static [BundleModelManifestEntry] {
    BUNDLE_MODEL_MANIFEST
}

pub fn bundle_manifest_entry(model_id: &str) -> Result<&'static BundleModelManifestEntry, String> {
    let id = normalize_model_id(model_id)?;
    BUNDLE_MODEL_MANIFEST
        .iter()
        .find(|entry| entry.public_id == id)
        .ok_or_else(|| format!("UNKNOWN_MODEL: {model_id}"))
}

/// All supported GGML artifacts. SHA-256 + byte counts are sourced
/// from the upstream `ggerganov/whisper.cpp` Hugging Face repo; see
/// `references/whisper-cpp-manifest.md` for the exact lookup.
///
/// `expected_bytes` is what Hugging Face's API reports as the LFS
/// payload size; the Rust downloader uses it as the *only* pre-check
/// (in addition to the streaming SHA-256) so a truncated or
/// transparently-replaced payload is rejected before being renamed
/// onto the final path.
///
/// `medium` and `turbo` point at the upstream `q8_0` builds rather than
/// f16: same checkpoint at 8-bit weights, roughly half the file and half
/// the resident memory, with a quality delta that measurements put inside
/// the noise for models this size. The small models stay f16 on purpose —
/// there quantisation costs the most and saves tens of megabytes. There is
/// no upstream `ggml-large-v3-q8_0.bin`, so `large-v3` also stays f16.
pub const MODEL_MANIFEST: &[ModelManifestEntry] = &[
    ModelManifestEntry {
        public_id: "tiny",
        file_name: "ggml-tiny.bin",
        download_url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.bin",
        expected_bytes: 77_691_713,
        sha256: "be07e048e1e599ad46341c8d2a135645097a538221678b7acdd1b1919c6e1b21",
        recommended: false,
    },
    ModelManifestEntry {
        public_id: "base",
        file_name: "ggml-base.bin",
        download_url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.bin",
        expected_bytes: 147_951_465,
        sha256: "60ed5bc3dd14eea856493d334349b405782ddcaf0028d4b5df4088345fba2efe",
        recommended: false,
    },
    ModelManifestEntry {
        public_id: "small",
        file_name: "ggml-small.bin",
        download_url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.bin",
        expected_bytes: 487_601_967,
        sha256: "1be3a9b2063867b937e64e2ec7483364a79917e157fa98c5d94b5c1fffea987b",
        recommended: false,
    },
    ModelManifestEntry {
        public_id: "medium",
        file_name: "ggml-medium-q8_0.bin",
        download_url:
            "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-medium-q8_0.bin",
        expected_bytes: 823_369_779,
        sha256: "42a1ffcbe4167d224232443396968db4d02d4e8e87e213d3ee2e03095dea6502",
        recommended: false,
    },
    ModelManifestEntry {
        public_id: "large-v3",
        file_name: "ggml-large-v3.bin",
        download_url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3.bin",
        expected_bytes: 3_095_033_483,
        sha256: "64d182b440b98d5203c4f9bd541544d84c605196c4f7b845dfa11fb23594d1e2",
        recommended: false,
    },
    ModelManifestEntry {
        public_id: "turbo",
        file_name: "ggml-large-v3-turbo-q8_0.bin",
        download_url:
            "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q8_0.bin",
        expected_bytes: 874_188_075,
        sha256: "317eb69c11673c9de1e1f0d459b253999804ec71ac4c23c17ecf5fbe24e259a1",
        recommended: true,
    },
    // English-only Whisper builds. Their multilingual head is trained away, so
    // Russian speech turns into garbage — their language is pinned.
    ModelManifestEntry {
        public_id: "tiny.en",
        file_name: "ggml-tiny.en-q8_0.bin",
        download_url:
            "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.en-q8_0.bin",
        expected_bytes: 43_550_795,
        sha256: "5bc2b3860aa151a4c6e7bb095e1fcce7cf12c7b020ca08dcec0c6d018bb7dd94",
        recommended: false,
    },
    ModelManifestEntry {
        public_id: "base.en",
        file_name: "ggml-base.en-q8_0.bin",
        download_url:
            "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en-q8_0.bin",
        expected_bytes: 81_781_811,
        sha256: "a4d4a0768075e13cfd7e19df3ae2dbc4a68d37d36a7dad45e8410c9a34f8c87e",
        recommended: false,
    },
    ModelManifestEntry {
        public_id: "small.en",
        file_name: "ggml-small.en-q8_0.bin",
        download_url:
            "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.en-q8_0.bin",
        expected_bytes: 264_477_561,
        sha256: "67a179f608ea6114bd3fdb9060e762b588a3fb3bd00c4387971be4d177958067",
        recommended: false,
    },
    ModelManifestEntry {
        public_id: "medium.en",
        file_name: "ggml-medium.en-q8_0.bin",
        download_url:
            "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-medium.en-q8_0.bin",
        expected_bytes: 823_382_461,
        sha256: "43fa2cd084de5a04399a896a9a7a786064e221365c01700cea4666005218f11c",
        recommended: false,
    },
];

/// Return the static manifest as a slice. Mirrors `MODELS` /
/// `list_models` semantically but exposes the download metadata
/// (URL, expected size, SHA-256) instead of the UI summary.
pub fn model_manifest() -> &'static [ModelManifestEntry] {
    MODEL_MANIFEST
}

/// Look up a single manifest entry by its public id (the same
/// string the rest of the app uses for config + UI). Returns
/// `Err(UNKNOWN_MODEL: …)` so the Tauri command layer (PR 1.2)
/// can return the same error string as the engine side.
pub fn manifest_entry(model_id: &str) -> Result<&'static ModelManifestEntry, String> {
    let id = normalize_model_id(model_id)?;
    MODEL_MANIFEST
        .iter()
        .find(|entry| entry.public_id == id)
        .ok_or_else(|| format!("UNKNOWN_MODEL: {model_id}"))
}

#[derive(Debug, Clone, Copy)]
struct ModelDefinition {
    id: &'static str,
    file_stem: &'static str,
    label: &'static str,
    size: &'static str,
    ram: &'static str,
    /// See [`BundleModelManifestEntry::languages`]: English-only Whisper builds
    /// have their multilingual head trained away.
    languages: Option<&'static [&'static str]>,
    recommended: bool,
}

const MODELS: &[ModelDefinition] = &[
    ModelDefinition {
        id: "tiny",
        file_stem: "tiny",
        label: "Whisper tiny",
        size: "75 MB",
        ram: "~0.4 GB",
        languages: Some(WHISPER_LANGUAGES),
        recommended: false,
    },
    ModelDefinition {
        id: "base",
        file_stem: "base",
        label: "Whisper base",
        size: "142 MB",
        ram: "~0.6 GB",
        languages: Some(WHISPER_LANGUAGES),
        recommended: false,
    },
    ModelDefinition {
        id: "small",
        file_stem: "small",
        label: "Whisper small",
        size: "466 MB",
        ram: "~1.4 GB",
        languages: Some(WHISPER_LANGUAGES),
        recommended: false,
    },
    ModelDefinition {
        id: "medium",
        file_stem: "medium-q8_0",
        label: "Whisper medium",
        size: "785 MB",
        ram: "~2.3 GB",
        languages: Some(WHISPER_LANGUAGES),
        recommended: false,
    },
    ModelDefinition {
        id: "large-v3",
        file_stem: "large-v3",
        label: "Whisper large-v3",
        size: "3.1 GB",
        ram: "~6.0 GB",
        languages: Some(WHISPER_LANGUAGES),
        recommended: false,
    },
    ModelDefinition {
        id: "turbo",
        file_stem: "large-v3-turbo-q8_0",
        label: "Whisper turbo",
        size: "834 MB",
        ram: "~1.7 GB",
        languages: Some(WHISPER_LANGUAGES),
        recommended: true,
    },
    ModelDefinition {
        id: "tiny.en",
        file_stem: "tiny.en-q8_0",
        label: "Whisper tiny.en",
        size: "42 MB",
        ram: "~0.3 GB",
        languages: Some(&["en"]),
        recommended: false,
    },
    ModelDefinition {
        id: "base.en",
        file_stem: "base.en-q8_0",
        label: "Whisper base.en",
        size: "78 MB",
        ram: "~0.4 GB",
        languages: Some(&["en"]),
        recommended: false,
    },
    ModelDefinition {
        id: "small.en",
        file_stem: "small.en-q8_0",
        label: "Whisper small.en",
        size: "252 MB",
        ram: "~1.0 GB",
        languages: Some(&["en"]),
        recommended: false,
    },
    ModelDefinition {
        id: "medium.en",
        file_stem: "medium.en-q8_0",
        label: "Whisper medium.en",
        size: "785 MB",
        ram: "~2.3 GB",
        languages: Some(&["en"]),
        recommended: false,
    },
];

#[derive(Debug, Clone, Serialize)]
pub struct ModelInfo {
    pub id: String,
    pub label: String,
    pub size: String,
    pub ram: String,
    pub recommended: bool,
    pub downloaded: bool,
    pub selected: bool,
    pub loaded: bool,
    /// The file was found in the models directory rather than being one of
    /// the catalogue entries. The UI must not offer to download it (there is
    /// no URL) or to delete it (the app did not put it there).
    pub local: bool,
    /// Inference engine family (`whisper.cpp` or `sherpa-onnx`).
    pub engine: String,
    /// Actual backend used by the selected model. GigaAM is CPU-only;
    /// Whisper follows the configured CPU/GPU setting at load time.
    pub compute_backend: String,
    pub cpu_only: bool,
    /// The model's family ("Whisper", "GigaAM", …) — the group heading in the
    /// catalog. `None` for a file found in the models folder: the family of a
    /// foreign `.bin` is unknown.
    pub family: Option<String>,
    /// The model's closed language list; `None` means multilingual. It drives
    /// both the catalog filter and the speech language choice in settings.
    pub languages: Option<Vec<String>>,
    /// Whether the model can show text as the dictation goes.
    pub streaming: bool,
    /// Weight quantisation: `q8_0`, `int8`, `f16`. `None` means unknown, which
    /// happens only for a foreign file from the models folder.
    pub quantization: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelEngine {
    Whisper,
    SherpaNemoCtc,
    SherpaTransducer,
    SherpaCanary,
    SherpaMoonshine,
    SherpaSenseVoice,
    /// A streaming transducer: the only family that can return text as speech
    /// goes rather than after the recording stops.
    SherpaStreamingTransducer,
}

impl ModelEngine {
    pub const fn wire_name(self) -> &'static str {
        match self {
            Self::Whisper => "whisper.cpp",
            _ => "sherpa-onnx",
        }
    }

    /// Sherpa runs on ONNX Runtime's CPU provider only: the GPU setting does
    /// not apply to it, and the UI must not claim otherwise.
    pub const fn is_sherpa(self) -> bool {
        !matches!(self, Self::Whisper)
    }

    /// Whether the engine returns text as speech goes. The live preview picks a
    /// model only among these; the rest stay silent until the recording ends.
    pub const fn is_streaming(self) -> bool {
        matches!(self, Self::SherpaStreamingTransducer)
    }

    /// The files without which a family will not load. The single place this
    /// knowledge lives: it is what the load spec is assembled from and what the
    /// tests check the manifest's completeness against.
    pub const fn required_roles(self) -> &'static [ArtifactRole] {
        match self {
            Self::Whisper => &[],
            Self::SherpaNemoCtc | Self::SherpaSenseVoice => {
                &[ArtifactRole::Model, ArtifactRole::Tokens]
            }
            Self::SherpaTransducer => &[
                ArtifactRole::Encoder,
                ArtifactRole::Decoder,
                ArtifactRole::Joiner,
                ArtifactRole::Tokens,
            ],
            Self::SherpaCanary => &[
                ArtifactRole::Encoder,
                ArtifactRole::Decoder,
                ArtifactRole::Tokens,
            ],
            Self::SherpaMoonshine => &[
                ArtifactRole::Preprocessor,
                ArtifactRole::Encoder,
                ArtifactRole::UncachedDecoder,
                ArtifactRole::CachedDecoder,
                ArtifactRole::Tokens,
            ],
            Self::SherpaStreamingTransducer => &[
                ArtifactRole::Encoder,
                ArtifactRole::Decoder,
                ArtifactRole::Joiner,
                ArtifactRole::Tokens,
            ],
        }
    }
}

#[derive(Debug, Clone)]
pub enum ModelLoadSpec {
    Whisper {
        path: PathBuf,
        use_gpu: bool,
    },
    /// Any sherpa family in one shape: the engine plus files by role. There
    /// used to be a variant per family here, and adding a new one meant editing
    /// both this and the engine thread, even though neither is obliged to know
    /// anything about families.
    Sherpa {
        engine: ModelEngine,
        files: BundleFiles,
    },
}

/// Paths to a bundle's files, laid out by role.
#[derive(Debug, Clone)]
pub struct BundleFiles(Vec<(ArtifactRole, PathBuf)>);

impl BundleFiles {
    pub fn path(&self, role: ArtifactRole) -> Result<&Path, String> {
        self.0
            .iter()
            .find(|(candidate, _)| *candidate == role)
            .map(|(_, path)| path.as_path())
            .ok_or_else(|| format!("MODEL_BUNDLE_INCOMPLETE: no {role:?}"))
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelStatus {
    pub state: String,
    pub model_loaded: bool,
    pub model: Option<String>,
    pub requested_model: Option<String>,
    pub path: Option<String>,
    pub last_error: Option<String>,
}

pub fn normalize_model_id(value: &str) -> Result<&'static str, String> {
    catalog_model(value)
        .map(|model| model.id)
        .ok_or_else(|| format!("UNKNOWN_MODEL: {value}"))
}

pub struct CatalogModel {
    pub id: &'static str,
    pub label: &'static str,
    pub engine: ModelEngine,
}

/// Static product metadata only; never discovers files or reads user state.
pub fn catalog_model(value: &str) -> Option<CatalogModel> {
    let normalized = value.trim().to_ascii_lowercase();
    let public_id = match normalized.as_str() {
        // Keep compatibility with existing config/UI values and accept the
        // native whisper.cpp name at command boundaries.
        "large-v3-turbo" => "turbo",
        other => other,
    };
    MODELS
        .iter()
        .find(|model| model.id == public_id)
        .map(|model| CatalogModel {
            id: model.id,
            label: model.label,
            engine: ModelEngine::Whisper,
        })
        .or_else(|| {
            BUNDLE_MODEL_MANIFEST
                .iter()
                .find(|model| model.public_id == public_id)
                .map(|model| CatalogModel {
                    id: model.public_id,
                    label: model.label,
                    engine: model.engine,
                })
        })
}

fn definition(model_id: &str) -> Result<&'static ModelDefinition, String> {
    let id = normalize_model_id(model_id)?;
    MODELS
        .iter()
        .find(|model| model.id == id)
        .ok_or_else(|| format!("UNKNOWN_MODEL: {model_id}"))
}

/// The cache directory from before the app was renamed to Sotto. It exists only
/// for the migration: those who installed early builds have already-downloaded
/// models here — gigabytes of them, and losing those to a rename is not on.
const LEGACY_CACHE_DIR: &str = "whisper-desktop";
const CACHE_DIR: &str = "sotto";

pub fn models_dir() -> Result<PathBuf, String> {
    if let Some(dir) = crate::portable::data_dir() {
        return Ok(dir.join("models"));
    }
    if let Ok(override_dir) = std::env::var("SPEECH_TO_TEXT_MODELS_DIR") {
        if !override_dir.trim().is_empty() {
            return Ok(PathBuf::from(override_dir));
        }
    }
    let cache =
        dirs::cache_dir().ok_or_else(|| "MODEL_CACHE_UNAVAILABLE: no cache dir".to_string())?;
    Ok(migrate_legacy_cache(&cache).join("models"))
}

/// Return the cache directory, migrating the old one along the way if it has
/// not been migrated yet.
///
/// A rename is a single move within one volume, so no copy and no half-state
/// arises. If the rename failed (permissions, an open file, a read-only volume)
/// we return the old path: working models matter more than a tidy directory
/// name, and the attempt can be repeated on the next launch.
fn migrate_legacy_cache(cache: &Path) -> PathBuf {
    let current = cache.join(CACHE_DIR);
    let legacy = cache.join(LEGACY_CACHE_DIR);
    if current.exists() || !legacy.is_dir() {
        return current;
    }
    match std::fs::rename(&legacy, &current) {
        Ok(()) => {
            log::info!("model cache moved from {LEGACY_CACHE_DIR} to {CACHE_DIR}");
            current
        }
        Err(error) => {
            log::warn!("model cache stays at {LEGACY_CACHE_DIR}: {error}");
            legacy
        }
    }
}

pub fn model_path(model_id: &str) -> Result<PathBuf, String> {
    if let Ok(model) = definition(model_id) {
        return Ok(models_dir()?.join(format!("ggml-{}.bin", model.file_stem)));
    }
    // Not in the catalogue — a file the user dropped into the models
    // directory, whose id *is* its file stem.
    let stem = local_model_stem(model_id)?;
    Ok(models_dir()?.join(format!("{stem}.bin")))
}

pub fn model_engine(model_id: &str) -> Result<ModelEngine, String> {
    match normalize_model_id(model_id) {
        Ok(id) => Ok(BUNDLE_MODEL_MANIFEST
            .iter()
            .find(|entry| entry.public_id == id)
            .map_or(ModelEngine::Whisper, |entry| entry.engine)),
        Err(_) => {
            local_model_stem(model_id)?;
            Ok(ModelEngine::Whisper)
        }
    }
}

/// Explain the model's restriction in the UI language. It lives next to the
/// rule itself so a new monolingual model does not ship without a message.
pub fn language_unsupported_message(languages: &[&str]) -> String {
    match languages {
        ["en"] => crate::ui_text::t("Эта модель распознаёт только английскую речь."),
        ["ru"] => crate::ui_text::t("Эта модель распознаёт только русскую речь."),
        _ => crate::ui_text::t("Эта модель не поддерживает выбранный язык."),
    }
}

/// The model's closed language list, if its list is closed. Multilingual models
/// in the catalog return `None`.
pub fn model_languages(model_id: &str) -> Option<&'static [&'static str]> {
    let id = normalize_model_id(model_id).ok()?;
    MODELS
        .iter()
        .find(|model| model.id == id)
        .and_then(|model| model.languages)
        .or_else(|| {
            BUNDLE_MODEL_MANIFEST
                .iter()
                .find(|entry| entry.public_id == id)
                .and_then(|entry| entry.languages)
        })
}

/// Whether the model can transcribe this language. `auto` and an empty string
/// mean "let the model decide" and always pass.
pub fn model_supports_language(model_id: &str, language: &str) -> bool {
    if language.is_empty() || language == "auto" {
        return true;
    }
    model_languages(model_id).is_none_or(|languages| languages.contains(&language))
}

pub fn model_load_spec(model_id: &str, use_gpu: bool) -> Result<ModelLoadSpec, String> {
    let engine = model_engine(model_id)?;
    if engine == ModelEngine::Whisper {
        return Ok(ModelLoadSpec::Whisper {
            path: model_path(model_id)?,
            use_gpu,
        });
    }
    let entry = bundle_manifest_entry(model_id)?;
    let dir = models_dir()?.join(entry.directory_name);
    // We collect only the roles the engine expects: a surplus file in a bundle
    // is silently ignored, a missing one stops the load here rather than at the
    // C boundary, where sherpa answers by crashing the process.
    let mut files = Vec::with_capacity(engine.required_roles().len());
    for role in engine.required_roles() {
        files.push((*role, dir.join(artifact_named(entry, *role)?)));
    }
    Ok(ModelLoadSpec::Sherpa {
        engine,
        files: BundleFiles(files),
    })
}

/// File names are upstream's business, roles are ours: the loader asks for
/// the encoder, not for `encoder.int8.onnx`.
fn artifact_named(
    entry: &'static BundleModelManifestEntry,
    role: ArtifactRole,
) -> Result<&'static str, String> {
    entry
        .artifacts
        .iter()
        .find(|artifact| artifact.role == role)
        .map(|artifact| artifact.file_name)
        .ok_or_else(|| {
            format!(
                "MODEL_BUNDLE_INCOMPLETE: {} has no {role:?}",
                entry.public_id
            )
        })
}

/// Validate a local-model id before it is turned into a path.
///
/// The id round-trips through the frontend, so it is untrusted input that
/// ends up in `models_dir().join(...)`. Restricting it to a flat filename
/// keeps a crafted id from naming a file outside the models directory.
fn local_model_stem(model_id: &str) -> Result<&str, String> {
    let acceptable = !model_id.is_empty()
        && model_id.len() <= 128
        && !model_id.starts_with('.')
        && !model_id.contains("..")
        && model_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    if acceptable {
        Ok(model_id)
    } else {
        Err(format!("UNKNOWN_MODEL: {model_id}"))
    }
}

/// GGML files sitting in the models directory that the catalogue does not
/// account for: quantised turbo builds, Russian fine-tunes, anything the
/// user downloaded themselves.
///
/// Returns them sorted by id so the model list has a stable order. A models
/// directory that cannot be read yields an empty list — a missing directory
/// is the normal first-run state, not an error worth surfacing.
fn discover_local_models() -> Vec<(String, u64)> {
    match models_dir() {
        Ok(dir) => discover_local_models_in(&dir),
        Err(_) => Vec::new(),
    }
}

fn discover_local_models_in(dir: &Path) -> Vec<(String, u64)> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };

    // File names the catalogue already owns; a partially-downloaded catalogue
    // model must not reappear as a second, "local" entry.
    let catalogue: Vec<String> = MODELS
        .iter()
        .map(|model| format!("ggml-{}.bin", model.file_stem))
        .collect();

    let mut found: Vec<(String, u64)> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension()?.to_str()? != "bin" {
                return None;
            }
            let file_name = path.file_name()?.to_str()?.to_string();
            if catalogue.contains(&file_name) {
                return None;
            }
            if !is_model_file_ready(&path) {
                return None;
            }
            let stem = path.file_stem()?.to_str()?;
            // Same validation as the inbound path, so a file the app cannot
            // later resolve by id never makes it into the list.
            local_model_stem(stem).ok()?;
            // Catalogue ids (including aliases such as `turbo`) win in
            // `model_path`. Publishing `turbo.bin` as a local model would
            // therefore create a duplicate UI id and selecting it would load
            // the catalogue's `ggml-large-v3-turbo-q8_0.bin` instead.
            if normalize_model_id(stem).is_ok() {
                return None;
            }
            let bytes = path.metadata().ok()?.len();
            Some((stem.to_string(), bytes))
        })
        .collect();
    found.sort_by(|a, b| a.0.cmp(&b.0));
    found
}

/// `ggml-` is a whisper.cpp file-naming convention, not part of the model's
/// name — showing it in the picker is noise.
fn local_model_label(stem: &str) -> &str {
    stem.strip_prefix("ggml-").unwrap_or(stem)
}

fn human_size(bytes: u64) -> String {
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    let bytes = bytes as f64;
    if bytes >= GB {
        format!("{:.1} GB", bytes / GB)
    } else {
        format!("{:.0} MB", bytes / MB)
    }
}

pub fn is_model_file_ready(path: &Path) -> bool {
    path.is_file()
        && path
            .metadata()
            .map(|metadata| metadata.len() >= MIN_VALID_MODEL_BYTES)
            .unwrap_or(false)
}

pub fn is_downloaded(model_id: &str) -> bool {
    match model_engine(model_id) {
        Ok(ModelEngine::Whisper) => model_path(model_id)
            .map(|path| is_model_file_ready(&path))
            .unwrap_or(false),
        Ok(_) => bundle_is_ready(model_id),
        Err(_) => false,
    }
}

fn bundle_is_ready(model_id: &str) -> bool {
    let Ok(dir) = models_dir() else {
        return false;
    };
    bundle_is_ready_at(model_id, &dir)
}

/// Path-closed variant of [`bundle_is_ready`]: tests the readiness check
/// against an explicit models root without touching `models_dir()`.
fn bundle_is_ready_at(model_id: &str, root: &Path) -> bool {
    let Ok(entry) = bundle_manifest_entry(model_id) else {
        return false;
    };
    let dir = root.join(entry.directory_name);
    entry.artifacts.iter().all(|artifact| {
        let path = dir.join(artifact.file_name);
        path.is_file()
            && std::fs::metadata(&path)
                .map(|metadata| metadata.len() == artifact.expected_bytes)
                .unwrap_or(false)
    })
}

/// Validate a closed-registry bundle immediately before crossing the Sherpa
/// C FFI boundary. This is intentionally synchronous and hash-based: an
/// invalid ONNX graph can throw a foreign C++ exception that Rust cannot catch.
pub fn verify_bundle_files(model_id: &str) -> Result<(), String> {
    verify_bundle_files_at(model_id, &models_dir()?)
}

fn verify_bundle_files_at(model_id: &str, root: &Path) -> Result<(), String> {
    let entry = bundle_manifest_entry(model_id)?;
    let dir = root.join(entry.directory_name);
    for artifact in entry.artifacts {
        verify_artifact(
            &dir.join(artifact.file_name),
            artifact.expected_bytes,
            artifact.sha256,
        )?;
    }
    Ok(())
}

/// Check one bundle file: present, of the right size, with the right content.
///
/// A separate function, because otherwise there is nothing to cover the content
/// check with a test: GigaAM's first artifact weighs 224 MB, and execution never
/// reaches the second — the one that would fit in a test. Because of that the
/// SHA-256 branch ran in no test at all: it could have been deleted outright and
/// the whole suite would still be green.
///
/// The check goes from cheap to expensive: presence, then size, and only then
/// the hash. Size rejects a truncated download without reading two hundred
/// megabytes.
fn verify_artifact(path: &Path, expected_bytes: u64, sha256: &str) -> Result<(), String> {
    let metadata = std::fs::metadata(path)
        .map_err(|error| format!("MODEL_MISSING: {}: {error}", path.display()))?;
    if metadata.len() != expected_bytes {
        return Err(format!(
            "MODEL_SIZE_MISMATCH: {} expected {} bytes, got {}",
            path.display(),
            expected_bytes,
            metadata.len()
        ));
    }
    let mut file = std::fs::File::open(path)
        .map_err(|error| format!("MODEL_OPEN_FAILED: {}: {error}", path.display()))?;
    use std::io::Read;
    let mut hasher = sha2::Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| format!("MODEL_READ_FAILED: {}: {error}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let actual = format!("{:x}", hasher.finalize());
    if !actual.eq_ignore_ascii_case(sha256) {
        return Err(format!(
            "MODEL_SHA256_MISMATCH: {} expected {}, got {}",
            path.display(),
            sha256,
            actual
        ));
    }
    Ok(())
}

/// Verify an existing closed-registry bundle and remove it when it is
/// incomplete or corrupt. Returns `true` only for a complete, hash-verified
/// final directory; `false` means the caller should download a fresh bundle.
/// This helper is intentionally path-closed and is run off the async runtime
/// by the download command.
pub fn recover_bundle_if_needed(model_id: &str) -> Result<bool, String> {
    recover_bundle_if_needed_at(model_id, &models_dir()?)
}

fn recover_bundle_if_needed_at(model_id: &str, root: &Path) -> Result<bool, String> {
    let entry = bundle_manifest_entry(model_id)?;
    let final_dir = root.join(entry.directory_name);
    let metadata = match std::fs::symlink_metadata(&final_dir) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(format!(
                "MODEL_BUNDLE_STAT_FAILED: {}: {error}",
                final_dir.display()
            ));
        }
    };
    if metadata.file_type().is_dir() && verify_bundle_files_at(model_id, root).is_ok() {
        return Ok(true);
    }
    remove_bundle_path(&final_dir)?;
    Ok(false)
}

/// Remove one closed-registry bundle path without following a symlink.
///
/// The caller must have resolved the path from [`BundleModelManifestEntry`].
/// Keeping this operation separate from verification lets the download command
/// recover from an incomplete final directory (for example after a power
/// loss) while still refusing to touch arbitrary user files.
pub fn remove_bundle_path(path: &Path) -> Result<bool, String> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(format!("MODEL_DELETE_FAILED: {}: {error}", path.display()));
        }
    };
    if metadata.file_type().is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
    .map_err(|error| format!("MODEL_DELETE_FAILED: {}: {error}", path.display()))?;
    Ok(true)
}

/// The catalogue plus whatever GGML files the user put in the models
/// directory themselves — see [`discover_local_models`]. Discovery is what
/// makes quantised turbo builds and third-party Whisper fine-tunes usable
/// without touching the inference code.
/// Quantisation markers that occur in the manifest's file names.
const QUANTIZATION_TAGS: &[&str] = &["q8_0", "q6_k", "q5_1", "q5_0", "q4_1", "q4_0", "int8"];

/// The precision of whisper.cpp GGML builds with no marker in the name.
const GGML_DEFAULT_PRECISION: &str = "f16";

/// Quantisation from the file name.
///
/// We read the name rather than keep yet another field beside it: the names in
/// the manifest are ours and are nailed down as constants together with the size
/// and the checksum, whereas a separate quantisation field would be forgotten on
/// the very first build swap — and would lie without breaking anything.
pub fn quantization_of(file_name: &str) -> Option<&'static str> {
    QUANTIZATION_TAGS
        .iter()
        .copied()
        .find(|tag| file_name.contains(tag))
}

pub fn list_models(selected: &str, loaded_model_id: Option<&str>) -> Vec<ModelInfo> {
    let selected = normalize_model_id(selected).unwrap_or(selected);
    let catalogue = MODELS.iter().map(|model| ModelInfo {
        id: model.id.to_string(),
        label: model.label.to_string(),
        size: model.size.to_string(),
        ram: model.ram.to_string(),
        recommended: model.recommended,
        downloaded: is_downloaded(model.id),
        selected: model.id == selected,
        loaded: loaded_model_id == Some(model.id),
        local: false,
        engine: ModelEngine::Whisper.wire_name().to_string(),
        compute_backend: "CPU/GPU".to_string(),
        cpu_only: false,
        family: Some(WHISPER_FAMILY.to_string()),
        languages: model
            .languages
            .map(|codes| codes.iter().map(|c| c.to_string()).collect()),
        streaming: false,
        // With no marker in the name a GGML build ships at its source
        // precision.
        quantization: Some(
            quantization_of(model.file_stem)
                .unwrap_or(GGML_DEFAULT_PRECISION)
                .to_string(),
        ),
    });
    let bundles = BUNDLE_MODEL_MANIFEST.iter().map(|model| ModelInfo {
        id: model.public_id.to_string(),
        label: model.label.to_string(),
        size: model.size.to_string(),
        ram: model.ram.to_string(),
        recommended: model.recommended,
        downloaded: is_downloaded(model.public_id),
        selected: model.public_id == selected,
        loaded: loaded_model_id == Some(model.public_id),
        local: false,
        engine: model.engine.wire_name().to_string(),
        compute_backend: "CPU".to_string(),
        cpu_only: true,
        family: Some(model.family.to_string()),
        languages: model
            .languages
            .map(|codes| codes.iter().map(|c| c.to_string()).collect()),
        streaming: model.engine.is_streaming(),
        // By the first marked artifact: inside a bundle the heavy graphs are
        // quantised while the vocabulary and preprocessor stay as they are.
        quantization: model
            .artifacts
            .iter()
            .find_map(|artifact| quantization_of(artifact.file_name))
            .map(str::to_string),
    });
    let local = discover_local_models().into_iter().map(|(stem, bytes)| {
        ModelInfo {
            id: stem.clone(),
            label: local_model_label(&stem).to_string(),
            size: human_size(bytes),
            // Peak RAM depends on the architecture and quantisation baked
            // into the file, neither of which is readable from the outside.
            ram: "—".to_string(),
            recommended: false,
            downloaded: true,
            selected: stem == selected,
            loaded: loaded_model_id == Some(stem.as_str()),
            local: true,
            engine: ModelEngine::Whisper.wire_name().to_string(),
            compute_backend: "CPU/GPU".to_string(),
            cpu_only: false,
            family: None,
            languages: None,
            streaming: false,
            // A marker in the name is the only thing that can be learned about
            // a foreign file without opening it; its absence is more honest
            // than a guess.
            quantization: quantization_of(&stem).map(str::to_string),
        }
    });
    catalogue.chain(bundles).chain(local).collect()
}

/// Delete a model's file: a catalogue download, a bundle directory, or a file
/// the user put in the models directory themselves.
///
/// Own files used to be refused here, on the grounds that the app cannot get
/// one back the way it can re-download a catalogue model. But the file is in
/// the app's directory, the app is what shows it, and the only way out was a
/// file manager — a refusal that protects nobody from anything. What the
/// difference actually deserves is a different sentence in the confirmation,
/// and that is where it now lives.
///
/// The id of an own file is its file stem, validated by `local_model_stem`
/// through `model_path`, so nothing outside the models directory is
/// reachable from here.
pub fn delete_cached_model(model_id: &str) -> Result<bool, String> {
    if model_engine(model_id)? == ModelEngine::SherpaNemoCtc {
        let entry = bundle_manifest_entry(model_id)?;
        let path = models_dir()?.join(entry.directory_name);
        return remove_bundle_path(&path);
    }
    let path = model_path(model_id)?;
    if !path.exists() {
        return Ok(false);
    }
    std::fs::remove_file(&path)
        .map_err(|error| format!("MODEL_DELETE_FAILED: {}: {error}", path.display()))?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A broken cache migration costs gigabytes of traffic: a user of earlier
    /// builds has already-downloaded models in the old directory.
    mod cache_migration {
        use super::*;

        #[test]
        fn a_legacy_directory_moves_to_the_new_name() {
            let root = tempfile::tempdir().unwrap();
            let legacy = root.path().join(LEGACY_CACHE_DIR).join("models");
            std::fs::create_dir_all(&legacy).unwrap();
            std::fs::write(legacy.join("ggml-turbo.bin"), b"weights").unwrap();

            let resolved = migrate_legacy_cache(root.path());

            assert_eq!(resolved, root.path().join(CACHE_DIR));
            assert!(resolved.join("models").join("ggml-turbo.bin").is_file());
            assert!(!root.path().join(LEGACY_CACHE_DIR).exists());
        }

        /// An already-migrated directory must not be touched: the old one may
        /// be left over from a build installed alongside, and its contents would
        /// overwrite the new.
        #[test]
        fn an_existing_new_directory_wins_over_the_legacy_one() {
            let root = tempfile::tempdir().unwrap();
            std::fs::create_dir_all(root.path().join(CACHE_DIR).join("models")).unwrap();
            std::fs::create_dir_all(root.path().join(LEGACY_CACHE_DIR).join("models")).unwrap();

            let resolved = migrate_legacy_cache(root.path());

            assert_eq!(resolved, root.path().join(CACHE_DIR));
            assert!(root.path().join(LEGACY_CACHE_DIR).exists());
        }

        #[test]
        fn a_clean_install_just_gets_the_new_path() {
            let root = tempfile::tempdir().unwrap();
            assert_eq!(
                migrate_legacy_cache(root.path()),
                root.path().join(CACHE_DIR)
            );
        }
    }

    #[test]
    fn turbo_aliases_resolve_to_public_id() {
        assert_eq!(normalize_model_id("turbo").unwrap(), "turbo");
        assert_eq!(normalize_model_id("large-v3-turbo").unwrap(), "turbo");
    }

    #[test]
    fn unknown_model_is_rejected() {
        assert!(normalize_model_id("not-a-model").is_err());
    }

    #[test]
    fn turbo_uses_upstream_ggml_filename() {
        let path = model_path("turbo").unwrap();
        assert!(path
            .to_string_lossy()
            .ends_with("ggml-large-v3-turbo-q8_0.bin"));
    }

    #[test]
    fn catalogue_has_one_recommended_model() {
        assert_eq!(MODELS.iter().filter(|model| model.recommended).count(), 1);
    }

    #[test]
    fn list_models_marks_turbo_alias_selected() {
        let models = list_models("large-v3-turbo", None);
        assert_eq!(models.iter().filter(|model| model.selected).count(), 1);
        assert!(models
            .iter()
            .any(|model| model.id == "turbo" && model.selected));
    }

    #[test]
    fn readiness_rejects_partial_files() {
        let file = tempfile::NamedTempFile::new().unwrap();
        file.as_file().set_len(MIN_VALID_MODEL_BYTES - 1).unwrap();
        assert!(!is_model_file_ready(file.path()));
        file.as_file().set_len(MIN_VALID_MODEL_BYTES).unwrap();
        assert!(is_model_file_ready(file.path()));
    }

    #[test]
    fn manifest_has_unique_public_ids_and_files() {
        use std::collections::HashSet;
        let manifest = model_manifest();
        let mut ids = HashSet::new();
        let mut files = HashSet::new();
        for entry in manifest {
            assert!(
                ids.insert(entry.public_id),
                "duplicate public_id: {}",
                entry.public_id
            );
            assert!(
                files.insert(entry.file_name),
                "duplicate file_name: {}",
                entry.file_name
            );
            assert!(
                entry.expected_bytes > 0,
                "{}: expected_bytes must be > 0",
                entry.public_id
            );
            assert_eq!(
                entry.sha256.len(),
                64,
                "{}: sha256 must be 64 hex chars",
                entry.public_id
            );
            assert!(
                entry.download_url.ends_with(entry.file_name),
                "{}: download_url must end with file_name",
                entry.public_id
            );
        }
    }

    #[test]
    fn manifest_entry_round_trip_for_known_models() {
        for known in [
            "tiny",
            "base",
            "small",
            "medium",
            "large-v3",
            "turbo",
            "tiny.en",
            "base.en",
            "small.en",
            "medium.en",
        ] {
            let entry = manifest_entry(known).expect("known model");
            assert_eq!(entry.public_id, known);
            assert_eq!(
                model_path(known)
                    .unwrap()
                    .file_name()
                    .unwrap()
                    .to_string_lossy(),
                entry.file_name
            );
        }
    }

    #[test]
    fn manifest_turbo_alias_resolves() {
        let entry = manifest_entry("large-v3-turbo").unwrap();
        assert_eq!(entry.public_id, "turbo");
    }

    /// An English-only Whisper build produces garbage rather than an error on
    /// Russian speech, so its language is pinned as firmly as GigaAM's.
    #[test]
    fn quantization_is_read_from_the_file_name() {
        // A separate field next to the URL would be forgotten on the very first
        // build swap and would lie without breaking anything. A file name cannot
        // lie: it is what the model is downloaded by.
        assert_eq!(quantization_of("medium-q8_0"), Some("q8_0"));
        assert_eq!(quantization_of("large-v3-turbo-q8_0"), Some("q8_0"));
        assert_eq!(quantization_of("encoder.int8.onnx"), Some("int8"));
        // No marker means source precision, and there is nothing to invent on
        // the file's behalf.
        assert_eq!(quantization_of("large-v3"), None);
        assert_eq!(quantization_of("tokens.txt"), None);
    }

    #[test]
    fn every_catalogue_model_says_how_it_is_quantised() {
        // The question "what exactly am I downloading" is about size and
        // precision at once, and blank space instead of an answer is worse than
        // the answer "f16".
        for model in list_models("tiny", None) {
            if model.local {
                continue;
            }
            assert!(
                model.quantization.is_some(),
                "{} не сообщает квантование",
                model.id
            );
        }

        let whisper = list_models("tiny", None);
        let large = whisper.iter().find(|m| m.id == "large-v3").unwrap();
        assert_eq!(large.quantization.as_deref(), Some("f16"));
        let turbo = whisper.iter().find(|m| m.id == "turbo").unwrap();
        assert_eq!(turbo.quantization.as_deref(), Some("q8_0"));
        // Inside a bundle the heavy graphs are quantised while the vocabulary
        // stays as it is — the marker is taken from the first marked artifact.
        // sherpa bundles exist only on Windows/macOS: on other platforms the
        // catalog does not contain them and there is nothing to check here.
        #[cfg(any(windows, target_os = "macos"))]
        {
            let gigaam = whisper.iter().find(|m| m.id == "gigaam-v3").unwrap();
            assert_eq!(gigaam.quantization.as_deref(), Some("int8"));
        }
    }

    #[test]
    fn english_whisper_builds_are_pinned_to_english() {
        assert_eq!(model_languages("small.en"), Some(&["en"][..]));
        assert_eq!(model_languages("medium.en"), Some(&["en"][..]));
        // Multilingual builds list their languages explicitly: "multilingual"
        // is not an answer to "is my language in there".
        assert!(model_supports_language("small", "de"));
        assert!(model_supports_language("turbo", "ru"));
        assert!(!model_supports_language("small.en", "de"));
        assert_eq!(model_engine("small.en").unwrap(), ModelEngine::Whisper);
        assert!(!model_supports_language("small.en", "ru"));
        assert!(model_supports_language("small.en", "en"));
        // A multilingual model restricts nothing; "auto" passes everywhere.
        assert!(model_supports_language("small.en", "auto"));
    }

    #[test]
    fn a_one_language_model_explains_its_own_limit() {
        assert!(language_unsupported_message(&["en"]).contains("англ"));
        assert!(language_unsupported_message(&["ru"]).contains("русск"));
        // For a model with several languages there is no way to list them in
        // the error — the message is generic but still about the language.
        assert!(language_unsupported_message(&["zh", "en"]).contains("язык"));
    }

    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn parakeet_is_a_multilingual_transducer_bundle() {
        assert_eq!(
            model_engine("parakeet-tdt-v3").unwrap(),
            ModelEngine::SherpaTransducer
        );
        // A multilingual model must not inherit GigaAM's language restriction:
        // with it an English dictation would be rejected.
        // Twenty-five European languages rather than "multilingual": Russian is
        // in the list while, say, Japanese is not, and that matters.
        assert!(model_supports_language("parakeet-tdt-v3", "ru"));
        assert!(model_supports_language("parakeet-tdt-v3", "de"));
        assert!(!model_supports_language("parakeet-tdt-v3", "ja"));
        assert_eq!(model_languages("gigaam-v3"), Some(&["ru"][..]));
        let entry = bundle_manifest_entry("parakeet-tdt-v3").unwrap();
        assert_eq!(entry.family, "Parakeet");
        assert_eq!(entry.artifacts.len(), 4);
        for role in [
            ArtifactRole::Encoder,
            ArtifactRole::Decoder,
            ArtifactRole::Joiner,
            ArtifactRole::Tokens,
        ] {
            assert!(
                artifact_named(entry, role).is_ok(),
                "трансдьюсеру нужен {role:?}"
            );
        }
    }

    /// Roles, not file names: the loader asks the manifest where the encoder is,
    /// and swapping a name in the manifest must reach the paths.
    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn transducer_load_spec_resolves_every_artifact_by_role() {
        let entry = bundle_manifest_entry("parakeet-tdt-v3").unwrap();
        let spec = model_load_spec("parakeet-tdt-v3", true).unwrap();
        let ModelLoadSpec::Sherpa { engine, files } = spec else {
            panic!("трансдьюсер загружен не как sherpa: {spec:?}");
        };
        assert_eq!(engine, ModelEngine::SherpaTransducer);
        // The names are literals rather than fetched via `artifact_named`:
        // checking a function against itself would pass even with roles swapped.
        let name =
            |path: &std::path::Path| path.file_name().unwrap().to_string_lossy().into_owned();
        assert_eq!(
            name(files.path(ArtifactRole::Encoder).unwrap()),
            "encoder.int8.onnx"
        );
        assert_eq!(
            name(files.path(ArtifactRole::Decoder).unwrap()),
            "decoder.int8.onnx"
        );
        assert_eq!(
            name(files.path(ArtifactRole::Joiner).unwrap()),
            "joiner.int8.onnx"
        );
        assert_eq!(
            name(files.path(ArtifactRole::Tokens).unwrap()),
            "tokens.txt"
        );
        assert!(files
            .path(ArtifactRole::Encoder)
            .unwrap()
            .starts_with(models_dir().unwrap().join(entry.directory_name)));
    }

    /// Streaming is a property of the engine and must reach the catalog: the
    /// live preview picks a model by exactly this trait.
    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn only_the_streaming_family_is_marked_streaming() {
        assert!(ModelEngine::SherpaStreamingTransducer.is_streaming());
        assert!(!ModelEngine::SherpaTransducer.is_streaming());
        assert!(!ModelEngine::Whisper.is_streaming());

        let models = list_models("turbo", None);
        let streaming = models
            .iter()
            .find(|m| m.id == "zipformer-ru-streaming")
            .unwrap();
        let offline = models.iter().find(|m| m.id == "zipformer-ru").unwrap();
        assert!(streaming.streaming);
        assert!(!offline.streaming);
        // Both Russian, both in one family — only the engine tells them apart.
        assert_eq!(streaming.family.as_deref(), Some("Zipformer"));
        assert_eq!(offline.family.as_deref(), Some("Zipformer"));
        assert!(!models.iter().any(|m| m.id == "turbo" && m.streaming));
    }

    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn every_bundle_pins_unique_roles_and_verifiable_artifacts() {
        use std::collections::HashSet;
        let mut ids = HashSet::new();
        let mut dirs = HashSet::new();
        for entry in bundle_manifest() {
            assert!(ids.insert(entry.public_id), "дубль id: {}", entry.public_id);
            assert!(
                dirs.insert(entry.directory_name),
                "дубль каталога: {}",
                entry.directory_name
            );
            let mut roles = HashSet::new();
            for artifact in entry.artifacts {
                assert!(
                    roles.insert(artifact.role),
                    "{}: роль {:?} встречается дважды",
                    entry.public_id,
                    artifact.role
                );
                assert_eq!(
                    artifact.sha256.len(),
                    64,
                    "{}: sha256 должен быть 64 hex-символа",
                    entry.public_id
                );
                assert!(
                    artifact.expected_bytes > 0,
                    "{}: нулевой размер артефакта",
                    entry.public_id
                );
                assert!(
                    artifact.download_url.ends_with(artifact.file_name),
                    "{}: ссылка не заканчивается именем файла",
                    entry.public_id
                );
            }
            for role in entry.engine.required_roles() {
                assert!(
                    artifact_named(entry, *role).is_ok(),
                    "{}: не хватает роли {role:?}",
                    entry.public_id
                );
            }
        }
    }

    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn gigaam_is_a_closed_cpu_only_bundle() {
        assert_eq!(normalize_model_id("gigaam-v3").unwrap(), "gigaam-v3");
        assert_eq!(
            model_engine("gigaam-v3").unwrap(),
            ModelEngine::SherpaNemoCtc
        );
        let entry = bundle_manifest_entry("gigaam-v3").unwrap();
        assert_eq!(entry.artifacts.len(), 2);
        assert_eq!(entry.artifacts[0].file_name, "model.int8.onnx");
        assert!(entry.artifacts[0]
            .download_url
            .contains("f376a99ee8be93b61f9e969d2ac827c4d228dac3"));
        assert_eq!(entry.artifacts[0].expected_bytes, 224_721_476);
        assert_eq!(
            entry.artifacts[0].sha256,
            "f86ebfa0429ced91be6054fc344827e9c6c2572f3c318416cd974b06f66437ec"
        );
        assert_eq!(entry.artifacts[1].file_name, "tokens.txt");
        assert!(entry.artifacts[1]
            .download_url
            .contains("f376a99ee8be93b61f9e969d2ac827c4d228dac3"));
        assert_eq!(entry.artifacts[1].expected_bytes, 196);
        assert_eq!(
            entry.artifacts[1].sha256,
            "17cc514451bcceac9c280068c71502f8448f99e9fb1456b8d0761651fd0392f2"
        );
        assert!(entry
            .artifacts
            .iter()
            .all(|artifact| artifact.sha256.len() == 64));
        let models = list_models("gigaam-v3", None);
        let gigaam = models.iter().find(|model| model.id == "gigaam-v3").unwrap();
        assert!(gigaam.cpu_only);
        assert_eq!(gigaam.engine, "sherpa-onnx");
        assert_eq!(gigaam.compute_backend, "CPU");
        assert_eq!(gigaam.family.as_deref(), Some("GigaAM"));
    }

    // ------------------------------------------------------------------
    // Bundle artifact verification
    //
    // Before these tests the SHA-256 branch never ran once: it cannot be
    // reached through `verify_bundle_files_at` because GigaAM's first artifact
    // weighs 224 MB. The hash is the only thing that tells a fully downloaded
    // file from a substituted one, so "there is a test for it" is no formality
    // here.
    // ------------------------------------------------------------------

    /// sha256("hello") — the reference for the tests below.
    const HELLO_SHA: &str = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";

    #[test]
    fn an_artifact_that_matches_is_accepted() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("artifact.bin");
        std::fs::write(&path, b"hello").unwrap();

        assert!(verify_artifact(&path, 5, HELLO_SHA).is_ok());
    }

    /// The case of the hash in the manifest must not decide a download's fate.
    #[test]
    fn the_expected_hash_is_compared_case_insensitively() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("artifact.bin");
        std::fs::write(&path, b"hello").unwrap();

        assert!(verify_artifact(&path, 5, &HELLO_SHA.to_uppercase()).is_ok());
    }

    /// The main case: the file arrived complete and at the right size, but the
    /// content is wrong. Neither presence nor size catches that — only the hash.
    #[test]
    fn an_artifact_of_the_right_size_but_wrong_content_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("artifact.bin");
        std::fs::write(&path, b"world").unwrap();

        let error = verify_artifact(&path, 5, HELLO_SHA).unwrap_err();
        assert!(
            error.starts_with("MODEL_SHA256_MISMATCH"),
            "подменённый файл принят как годный: {error}"
        );
    }

    #[test]
    fn an_artifact_of_the_wrong_size_is_rejected_before_hashing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("artifact.bin");
        std::fs::write(&path, b"hell").unwrap();

        let error = verify_artifact(&path, 5, HELLO_SHA).unwrap_err();
        assert!(error.starts_with("MODEL_SIZE_MISMATCH"), "got: {error}");
    }

    #[test]
    fn a_missing_artifact_is_reported_as_missing() {
        let dir = tempfile::tempdir().unwrap();
        let error = verify_artifact(&dir.path().join("nope.bin"), 5, HELLO_SHA).unwrap_err();
        assert!(error.starts_with("MODEL_MISSING"), "got: {error}");
    }

    /// An empty file is the typical result of a broken download: it exists, it
    /// opens, and it is zero bytes long.
    #[test]
    fn an_empty_artifact_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("artifact.bin");
        std::fs::write(&path, b"").unwrap();

        assert!(verify_artifact(&path, 5, HELLO_SHA).is_err());
    }

    // Here and below the platform restriction is there for the same reason as
    // in `gigaam_is_a_closed_cpu_only_bundle`: BUNDLE_MODEL_MANIFEST is empty
    // outside Windows/macOS, so "gigaam-v3" is simply an unknown model there.
    // The test would not pass, and if it did it would pass for nothing.
    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn bundle_recovery_removes_final_dir_when_artifact_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let final_dir = dir.path().join("gigaam-v3");
        std::fs::create_dir_all(&final_dir).unwrap();
        std::fs::write(final_dir.join("tokens.txt"), b"partial").unwrap();

        assert!(!recover_bundle_if_needed_at("gigaam-v3", dir.path()).unwrap());
        assert!(!final_dir.exists());
    }

    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn bundle_recovery_removes_final_dir_when_artifact_size_is_wrong() {
        let dir = tempfile::tempdir().unwrap();
        let final_dir = dir.path().join("gigaam-v3");
        std::fs::create_dir_all(&final_dir).unwrap();
        std::fs::write(final_dir.join("model.int8.onnx"), b"wrong-size").unwrap();
        std::fs::write(final_dir.join("tokens.txt"), b"partial").unwrap();

        assert!(!recover_bundle_if_needed_at("gigaam-v3", dir.path()).unwrap());
        assert!(!final_dir.exists());
    }

    // ------------------------------------------------------------------
    // Local (user-supplied) model discovery
    // ------------------------------------------------------------------

    /// Create a `.bin` in `dir` that passes the readiness size check.
    fn write_model_file(dir: &Path, name: &str) {
        let file = std::fs::File::create(dir.join(name)).unwrap();
        file.set_len(MIN_VALID_MODEL_BYTES).unwrap();
    }

    #[test]
    fn discovery_finds_unknown_ggml_files() {
        let dir = tempfile::tempdir().unwrap();
        write_model_file(dir.path(), "ggml-large-v3-turbo-q5_0.bin");
        write_model_file(dir.path(), "russian-finetune.bin");

        let found: Vec<String> = discover_local_models_in(dir.path())
            .into_iter()
            .map(|(stem, _)| stem)
            .collect();
        assert_eq!(found, ["ggml-large-v3-turbo-q5_0", "russian-finetune"]);
    }

    #[test]
    fn discovery_skips_catalogue_files_and_non_models() {
        let dir = tempfile::tempdir().unwrap();
        // Already in the catalogue — must not appear a second time as a
        // "local" entry.
        write_model_file(dir.path(), "ggml-large-v3-turbo-q8_0.bin");
        // Not a GGML file.
        write_model_file(dir.path(), "notes.txt");
        // A partial download: too small to be a usable model.
        std::fs::File::create(dir.path().join("half-done.bin")).unwrap();

        assert!(discover_local_models_in(dir.path()).is_empty());
    }

    #[test]
    fn discovery_skips_local_files_whose_stem_is_a_catalogue_id_or_alias() {
        let dir = tempfile::tempdir().unwrap();
        write_model_file(dir.path(), "tiny.bin");
        write_model_file(dir.path(), "turbo.bin");
        write_model_file(dir.path(), "large-v3-turbo.bin");

        assert!(discover_local_models_in(dir.path()).is_empty());
    }

    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn discovery_skips_local_file_that_collides_with_sherpa_id() {
        let dir = tempfile::tempdir().unwrap();
        write_model_file(dir.path(), "gigaam-v3.bin");
        assert!(discover_local_models_in(dir.path()).is_empty());
    }

    #[test]
    fn discovery_tolerates_a_missing_directory() {
        // First run: the models directory does not exist yet.
        assert!(discover_local_models_in(Path::new("no/such/dir")).is_empty());
    }

    #[test]
    fn local_model_ids_cannot_escape_the_models_dir() {
        // The id round-trips through the frontend before being joined onto
        // the models directory.
        for hostile in ["../../secrets", "a/b", "a\\b", ".hidden", "", "..", "a?b"] {
            assert!(
                local_model_stem(hostile).is_err(),
                "{hostile:?} should be rejected"
            );
            assert!(model_path(hostile).is_err(), "{hostile:?} resolved a path");
        }
        assert!(local_model_stem("ggml-turbo-q5_0").is_ok());
    }

    #[test]
    fn local_model_path_uses_the_stem_verbatim() {
        // Catalogue ids map id → file_stem; local ids ARE the file stem.
        let path = model_path("russian-finetune").unwrap();
        assert!(path.to_string_lossy().ends_with("russian-finetune.bin"));
    }

    /// There used to be a refusal here: the app had not downloaded this file
    /// and could not bring it back. The refusal is gone — and with it the model
    /// the list showed but offered no way to remove from that same list. The one
    /// thing that must not happen is an error on an already-missing file: the UI
    /// would show it as "deletion failed".
    #[test]
    fn deleting_an_absent_own_file_is_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let _g = EnvGuard::set(MODELS_DIR_ENV, dir.path());
        assert_eq!(delete_cached_model("russian-finetune"), Ok(false));
    }

    #[test]
    fn local_label_drops_the_ggml_prefix() {
        assert_eq!(local_model_label("ggml-turbo-q5_0"), "turbo-q5_0");
        assert_eq!(local_model_label("russian-finetune"), "russian-finetune");
    }

    #[test]
    fn human_size_switches_unit_at_a_gigabyte() {
        assert_eq!(human_size(500 * 1024 * 1024), "500 MB");
        assert_eq!(human_size(1024 * 1024 * 1024), "1.0 GB");
        assert_eq!(human_size(1_624_555_275), "1.5 GB");
    }

    #[test]
    fn catalogue_entries_are_not_marked_local() {
        let dir = tempfile::tempdir().unwrap();
        let _g = EnvGuard::set(MODELS_DIR_ENV, dir.path());
        assert!(list_models("turbo", None).iter().all(|model| !model.local));
    }

    #[test]
    fn manifest_and_ui_summary_cover_the_same_models() {
        // The UI summary (list_models) and the downloader manifest
        // (model_manifest) must stay in sync — a model id present in
        // one but not the other produces a broken Settings state
        // (user can select a model that the downloader cannot fetch).
        let ui_ids: std::collections::HashSet<&'static str> =
            MODELS.iter().map(|model| model.id).collect();
        let manifest_ids: std::collections::HashSet<&'static str> =
            MODEL_MANIFEST.iter().map(|entry| entry.public_id).collect();
        assert_eq!(ui_ids, manifest_ids);
    }

    // `models_dir()` honours `SPEECH_TO_TEXT_MODELS_DIR`, so tests exercising
    // `models_dir`-bound functions point it at a temp dir. `EnvGuard` restores
    // the previous value and serializes against every other env-var test.
    use crate::test_support::EnvGuard;

    const MODELS_DIR_ENV: &str = "SPEECH_TO_TEXT_MODELS_DIR";

    // ------------------------------------------------------------------
    // Bundle readiness / recovery / discovery / deletion via paths
    // ------------------------------------------------------------------

    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn bundle_is_ready_at_requires_every_artifact_at_exact_size() {
        let dir = tempfile::tempdir().unwrap();
        let bundle = dir.path().join("gigaam-v3");
        std::fs::create_dir_all(&bundle).unwrap();
        // tokens.txt at the right size, model missing → not ready.
        std::fs::write(bundle.join("tokens.txt"), vec![0u8; 196]).unwrap();
        assert!(!bundle_is_ready_at("gigaam-v3", dir.path()));

        // Model present but the wrong size → not ready (catches `&&` → `||`).
        std::fs::File::create(bundle.join("model.int8.onnx"))
            .unwrap()
            .set_len(100)
            .unwrap();
        assert!(!bundle_is_ready_at("gigaam-v3", dir.path()));

        // Model at exactly the manifest size → ready (catches `==` → `!=`).
        std::fs::File::create(bundle.join("model.int8.onnx"))
            .unwrap()
            .set_len(224_721_476)
            .unwrap();
        assert!(bundle_is_ready_at("gigaam-v3", dir.path()));
    }

    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn bundle_recovery_returns_false_for_a_missing_dir() {
        let dir = tempfile::tempdir().unwrap();
        // No bundle at all is the "needs download" case, not an error.
        assert!(!recover_bundle_if_needed_at("gigaam-v3", dir.path()).unwrap());
    }

    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn bundle_recovery_wrapper_respects_models_dir() {
        let dir = tempfile::tempdir().unwrap();
        let _g = EnvGuard::set(MODELS_DIR_ENV, dir.path());
        let final_dir = dir.path().join("gigaam-v3");
        std::fs::create_dir_all(&final_dir).unwrap();
        std::fs::write(final_dir.join("tokens.txt"), b"partial").unwrap();

        // Corrupt bundle under the models dir → removed, and the wrapper
        // reports "needs download".
        assert!(!recover_bundle_if_needed("gigaam-v3").unwrap());
        assert!(!final_dir.exists());
    }

    #[test]
    fn remove_bundle_path_removes_and_reports_missing() {
        let dir = tempfile::tempdir().unwrap();
        // Missing path → false.
        assert!(!remove_bundle_path(&dir.path().join("nope")).unwrap());
        // Directory → true and removed.
        let bundle = dir.path().join("bundle");
        std::fs::create_dir_all(&bundle).unwrap();
        std::fs::write(bundle.join("artifact"), b"x").unwrap();
        assert!(remove_bundle_path(&bundle).unwrap());
        assert!(!bundle.exists());
    }

    #[test]
    fn discover_local_models_wrapper_reads_the_models_dir() {
        let dir = tempfile::tempdir().unwrap();
        let _g = EnvGuard::set(MODELS_DIR_ENV, dir.path());
        // Empty dir → no local models.
        assert!(discover_local_models().is_empty());

        write_model_file(dir.path(), "russian-finetune.bin");
        let found: Vec<String> = discover_local_models()
            .into_iter()
            .map(|(stem, _)| stem)
            .collect();
        assert_eq!(found, ["russian-finetune"]);
    }

    #[test]
    fn is_downloaded_distinguishes_present_from_absent() {
        let dir = tempfile::tempdir().unwrap();
        let _g = EnvGuard::set(MODELS_DIR_ENV, dir.path());
        assert!(
            !is_downloaded("tiny"),
            "absent model must not be downloaded"
        );

        std::fs::File::create(dir.path().join("ggml-tiny.bin"))
            .unwrap()
            .set_len(MIN_VALID_MODEL_BYTES)
            .unwrap();
        assert!(is_downloaded("tiny"), "ready model must be downloaded");
    }

    #[test]
    fn delete_cached_model_removes_existing_and_reports_missing() {
        let dir = tempfile::tempdir().unwrap();
        let _g = EnvGuard::set(MODELS_DIR_ENV, dir.path());
        // Absent → false, nothing touched.
        assert!(!delete_cached_model("tiny").unwrap());

        // Present → true and removed.
        std::fs::File::create(dir.path().join("ggml-tiny.bin"))
            .unwrap()
            .set_len(MIN_VALID_MODEL_BYTES)
            .unwrap();
        assert!(delete_cached_model("tiny").unwrap());
        assert!(!dir.path().join("ggml-tiny.bin").exists());
    }

    /// A user's own file is unknown to the catalog, and deletion used to refuse
    /// on it: the list showed a model that could not be removed from that list.
    #[test]
    fn delete_cached_model_removes_a_file_the_user_added() {
        let dir = tempfile::tempdir().unwrap();
        let _g = EnvGuard::set(MODELS_DIR_ENV, dir.path());
        let own = dir.path().join("ggml-my-finetune.bin");
        std::fs::File::create(&own)
            .unwrap()
            .set_len(MIN_VALID_MODEL_BYTES)
            .unwrap();
        // This is exactly how the file is named in the model list.
        assert_eq!(
            discover_local_models_in(dir.path()),
            vec![("ggml-my-finetune".to_string(), MIN_VALID_MODEL_BYTES)],
        );

        assert!(delete_cached_model("ggml-my-finetune").unwrap());
        assert!(!own.exists());
        assert!(discover_local_models_in(dir.path()).is_empty());
    }

    /// The identifier of a user's own file comes from the frontend, and the path
    /// is assembled from it by concatenation. Escaping the models directory must
    /// run into the same check as a download does, not into a lucky coincidence
    /// of names.
    #[test]
    fn delete_cached_model_refuses_to_walk_out_of_the_models_directory() {
        let dir = tempfile::tempdir().unwrap();
        let _g = EnvGuard::set(MODELS_DIR_ENV, dir.path());
        let outsider = dir.path().parent().unwrap().join("outsider.bin");
        std::fs::write(&outsider, b"not a model").unwrap();

        assert!(delete_cached_model("../outsider").is_err());
        assert!(delete_cached_model("/etc/passwd").is_err());
        assert!(outsider.exists());
    }

    // ------------------------------------------------------------------
    // `list_models` distinguishes models by id, not just list length
    // ------------------------------------------------------------------

    #[test]
    fn list_models_marks_whisper_selected_and_loaded_by_id() {
        let models = list_models("turbo", Some("turbo"));
        let turbo = models.iter().find(|m| m.id == "turbo").unwrap();
        assert!(turbo.selected, "turbo must be selected when requested");
        assert!(turbo.loaded, "turbo must be loaded when reported loaded");
        let tiny = models.iter().find(|m| m.id == "tiny").unwrap();
        assert!(!tiny.selected && !tiny.loaded, "tiny must be neither");
    }

    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn list_models_marks_bundle_selected_and_loaded_by_id() {
        let models = list_models("gigaam-v3", Some("gigaam-v3"));
        let gigaam = models.iter().find(|m| m.id == "gigaam-v3").unwrap();
        assert!(gigaam.selected, "gigaam must be selected when requested");
        assert!(gigaam.loaded, "gigaam must be loaded when reported loaded");
    }

    #[test]
    fn list_models_marks_local_model_selected_and_loaded_by_id() {
        let dir = tempfile::tempdir().unwrap();
        let _g = EnvGuard::set(MODELS_DIR_ENV, dir.path());
        write_model_file(dir.path(), "russian-finetune.bin");

        let models = list_models("russian-finetune", Some("russian-finetune"));
        let local = models.iter().find(|m| m.id == "russian-finetune").unwrap();
        assert!(local.local, "discovered file must be marked local");
        assert!(
            local.selected,
            "local model must be selected when requested"
        );
        assert!(
            local.loaded,
            "local model must be loaded when reported loaded"
        );
    }

    #[test]
    fn min_valid_model_bytes_boundary_is_ten_mib() {
        // Pin the threshold with a literal, not the `MIN_VALID_MODEL_BYTES`
        // constant — a test that reuses the constant cannot see it change.
        const TEN_MIB: u64 = 10 * 1024 * 1024;
        let file = tempfile::NamedTempFile::new().unwrap();
        file.as_file().set_len(TEN_MIB - 1).unwrap();
        assert!(
            !is_model_file_ready(file.path()),
            "one byte under 10 MiB must not be ready"
        );
        file.as_file().set_len(TEN_MIB).unwrap();
        assert!(
            is_model_file_ready(file.path()),
            "exactly 10 MiB must be ready"
        );
    }
}
