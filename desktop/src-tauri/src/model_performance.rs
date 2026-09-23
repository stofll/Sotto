//! What a model card says: catalogue speed, and memory and disk fit on this
//! machine. Nothing here is recorded: the speed comes from the bundled
//! catalogue, not from the user's own dictations.
use crate::{hardware_profile, model};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::OnceLock;

/// The measurement method the bundled references were taken with, engine
/// versions included. It is a label, not a lookup of `Cargo.toml`: after an
/// engine upgrade the old references no longer describe the new code, and a
/// stale label is what keeps them off the cards until they are measured again
/// (`reference_catalog_has_valid_metrics_and_unique_contexts` checks it).
const METHOD: &str = "sotto-stt-v1-whisper-0.14.4-sherpa-1.13.7";
/// The language setting that leaves the choice to the engine. Its measurement
/// is the one that fits any other language the model is asked for.
const GENERIC_LANGUAGE: &str = "auto";
/// Every catalogue reference is a processor measurement, and the cards are
/// meant to stay comparable between machines rather than to predict the
/// graphics card in front of the user.
const CATALOG_COMPUTE: &str = "cpu";
/// Apple Silicon keeps one memory pool for the processor and the graphics,
/// so free system memory is the budget whichever of them runs the model.
/// Elsewhere a graphics card has memory of its own that nothing here reads.
const UNIFIED_MEMORY: bool = cfg!(all(target_os = "macos", target_arch = "aarch64"));

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Profile {
    pub model_id: String,
    pub revision: String,
    pub hardware: String,
    pub method: String,
    pub compute: String,
}

impl Profile {
    pub fn new(model_id: &str, compute: &str) -> Self {
        Self {
            model_id: model_id.to_owned(),
            revision: revision(model_id),
            hardware: hardware_profile::fingerprint().to_owned(),
            method: METHOD.to_owned(),
            compute: compute.to_owned(),
        }
    }
}

pub fn revision(id: &str) -> String {
    if let Ok(manifest) = model::manifest_entry(id) {
        return manifest.sha256.to_owned();
    }
    if let Ok(manifest) = model::bundle_manifest_entry(id) {
        let mut hash = Sha256::new();
        for artifact in manifest.artifacts {
            hash.update(artifact.sha256);
        }
        return format!("{:x}", hash.finalize());
    }
    // A stat fingerprint avoids rereading multi-GB user files in the catalog.
    model::model_path(id)
        .ok()
        .and_then(|path| path.metadata().ok())
        .map(|meta| format!("{}:{:?}", meta.len(), meta.modified().ok()))
        .unwrap_or_else(|| "unknown".into())
}

/// What a model card says about speed: one bounded score, or nothing.
///
/// The card draws three fixed levels from `score` and explains them in a
/// sentence, so anything finer — sample counts, medians, the machine the
/// catalogue was measured on — would be carried across the bridge for no
/// reader. `source` stays because "not measured" is a state the interface
/// distinguishes.
#[derive(Clone, Debug, Serialize)]
pub struct SpeedAssessment {
    pub score: Option<f64>,
    pub source: &'static str,
}

#[derive(Clone, Debug, Serialize)]
pub struct MemoryAssessment {
    pub score: Option<f64>,
    pub status: &'static str,
    pub required_bytes: Option<u64>,
    pub available_bytes: Option<u64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Assessment {
    pub id: String,
    pub compute: String,
    pub speed: SpeedAssessment,
    pub memory: MemoryAssessment,
    pub download: DownloadAssessment,
}

#[derive(Clone, Debug, Serialize)]
pub struct DownloadAssessment {
    pub required_bytes: Option<u64>,
    pub available_bytes: Option<u64>,
    pub insufficient: bool,
}

fn download_assessment(id: &str, available_bytes: Option<u64>) -> DownloadAssessment {
    let expected = model::manifest_entry(id)
        .ok()
        .map(|entry| entry.expected_bytes)
        .or_else(|| {
            model::bundle_manifest_entry(id).ok().map(|entry| {
                entry
                    .artifacts
                    .iter()
                    .map(|artifact| artifact.expected_bytes)
                    .sum::<u64>()
            })
        });
    let required_bytes = expected.map(crate::model_download::required_free_space);
    DownloadAssessment {
        required_bytes,
        available_bytes,
        insufficient: required_bytes
            .zip(available_bytes)
            .is_some_and(|(required, available)| available < required),
    }
}

/// A bounded monotonic display scale, not a benchmark percentile or accuracy.
pub fn speed_score(rtf: f64) -> f64 {
    // Real-time processing is the midpoint; 4x faster fills 80% of the bar.
    (1.0 / (1.0 + rtf)).clamp(0.0, 1.0)
}

pub fn memory_assessment(
    required: Option<u64>,
    available: Option<u64>,
    loaded: bool,
    compute: &str,
) -> MemoryAssessment {
    let mut result = MemoryAssessment {
        score: None,
        status: "unknown",
        required_bytes: required,
        available_bytes: available,
    };
    if compute != "cpu" && !UNIFIED_MEMORY {
        result.status = if loaded { "loaded" } else { "gpu_unknown" };
        return result;
    }
    let (Some(required), Some(available)) = (required.filter(|n| *n > 0), available) else {
        if loaded {
            result.status = "loaded";
        }
        return result;
    };
    // A model already in memory holds part of what is in use, and unloading it
    // gives that back. Counting its own footprint into the budget is what
    // keeps the question the same one every other card answers — whether this
    // machine has room for this model — instead of leaving the one model that
    // demonstrably fits as the only card without a bar.
    let budget = (available + if loaded { required } else { 0 }) as f64 * 0.8;
    result.status = if loaded {
        "loaded"
    } else if budget < required as f64 {
        "low"
    } else {
        "enough"
    };
    result.score = Some(if budget <= 0.0 {
        0.0
    } else {
        (1.0 - required as f64 / budget).clamp(0.0, 1.0)
    });
    result
}

#[derive(Deserialize)]
pub struct Reference {
    pub model_id: String,
    pub revision: String,
    pub compute: String,
    pub language: String,
    pub rtf: f64,
    pub median_ms: f64,
    pub audio_seconds: f64,
    pub fixture_language: String,
    pub machine: String,
    pub method: String,
}

/// The catalogue measurement for `model_id` under the configured `language`,
/// falling back to the automatic-detection one.
///
/// The language is not decoration. Whisper pays for a detection pass, so its
/// `auto` rows run about twice its explicit-language rows — enough to move
/// `base` and `base.en` a whole level down the card. Sherpa models are
/// indifferent, and their two rows agree within a few percent. Showing the
/// `auto` number to someone who has chosen a language therefore understates
/// Whisper against Sherpa on one shared scale.
fn reference(model_id: &str, language: &str) -> Option<&'static Reference> {
    static REFERENCES: OnceLock<Vec<Reference>> = OnceLock::new();
    let references = REFERENCES.get_or_init(|| {
        serde_json::from_str(include_str!("model_reference.json")).unwrap_or_default()
    });
    let revision = revision(model_id);
    let measured = |language: &str| {
        references.iter().find(|value| {
            value.model_id == model_id
                && value.revision == revision
                && value.compute == CATALOG_COMPUTE
                && value.method == METHOD
                && value.language == language
                && value.rtf.is_finite()
                && value.rtf > 0.0
        })
    };
    measured(language).or_else(|| measured(GENERIC_LANGUAGE))
}

/// The catalogue number a model card shows.
///
/// One fixed processor context keeps the cards comparable between machines,
/// while the language follows the configured one because it changes Whisper's
/// cost by about a factor of two.
fn catalog_speed(id: &str, language: &str) -> SpeedAssessment {
    match reference(id, language) {
        Some(reference) => SpeedAssessment {
            score: Some(speed_score(reference.rtf)),
            source: "reference",
        },
        None => SpeedAssessment {
            score: None,
            source: "unknown",
        },
    }
}

pub fn assess(models: &[model::ModelInfo], gpu: bool, language: &str) -> Vec<Assessment> {
    let hardware = hardware_profile::snapshot();
    let disk_available = model::models_dir()
        .ok()
        .and_then(|dir| crate::model_download::available_bytes(&dir));

    models
        .iter()
        .map(|model| {
            let compute = hardware_profile::compute(model.cpu_only, gpu);
            Assessment {
                id: model.id.clone(),
                compute: compute.to_owned(),
                speed: catalog_speed(&model.id, language),
                download: download_assessment(&model.id, disk_available),
                memory: memory_assessment(
                    model.ram_bytes,
                    hardware.available_bytes,
                    model.loaded,
                    compute,
                ),
            }
        })
        .collect()
}

#[tauri::command]
pub async fn model_assessments(
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::state::AppState>,
) -> Result<Vec<Assessment>, String> {
    let current = crate::mutex_recover::lock(&state.engine_current_model).clone();
    tauri::async_runtime::spawn_blocking(move || {
        let config = crate::config::Config::load(&app)?;
        let selected = config.get_string("model").unwrap_or_default();
        let language = config
            .get_string("language")
            .unwrap_or_else(|| "auto".into());
        let gpu = crate::config::resolve_device(config.as_value()) != "cpu";
        let models = model::list_model_infos(&selected, current.as_deref());
        Ok(assess(&models, gpu, &language))
    })
    .await
    .map_err(|_| "PERFORMANCE_UNAVAILABLE".to_owned())?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn memory_keeps_unknown_zero_loaded_and_gpu_distinct() {
        assert_eq!(
            memory_assessment(Some(100), None, false, "cpu").status,
            "unknown"
        );
        assert_eq!(
            memory_assessment(Some(100), Some(0), false, "cpu").score,
            Some(0.0)
        );
        assert_eq!(
            memory_assessment(Some(100), Some(120), false, "cpu").status,
            "low"
        );
        assert_eq!(
            memory_assessment(Some(100), Some(200), false, "cpu").status,
            "enough"
        );
        // The model in memory is the one that demonstrably fits, so it gets a
        // bar like every other card: its own footprint counts into the budget.
        let loaded = memory_assessment(Some(100), Some(200), true, "cpu");
        assert_eq!(loaded.status, "loaded");
        assert_eq!(loaded.score, Some(1.0 - 100.0 / 240.0));
        // A graphics card keeps memory of its own that nothing here reads; a
        // shared pool is the same question as the processor's.
        let discrete = memory_assessment(Some(100), Some(200), false, "gpu_unverified");
        if UNIFIED_MEMORY {
            assert_eq!(discrete.status, "enough");
            assert_eq!(discrete.score, Some(1.0 - 100.0 / 160.0));
        } else {
            assert_eq!(discrete.status, "gpu_unknown");
            assert_eq!(discrete.score, None);
        }
    }

    #[test]
    fn the_configured_language_selects_the_reference_measured_with_it() {
        // Whisper pays for a detection pass, so the `auto` row must not stand
        // in while the language the user chose has a row of its own.
        let explicit = reference("tiny", "ru").expect("ru is measured");
        assert_eq!(explicit.language, "ru");
        let detected = reference("tiny", GENERIC_LANGUAGE).expect("auto is measured");
        assert!(
            detected.rtf > explicit.rtf * 1.5,
            "detection should cost Whisper real time: {} vs {}",
            detected.rtf,
            explicit.rtf
        );
        // A language the catalogue does not hold falls back to detection
        // rather than leaving the card blank.
        assert_eq!(
            reference("tiny", "de").expect("auto stands in").language,
            GENERIC_LANGUAGE
        );
        // Sherpa models do not run a detection pass, so their rows agree. The
        // catalogue exposes them only where their runtime is packaged.
        #[cfg(any(windows, target_os = "macos"))]
        {
            let (fixed, auto) = (
                reference("gigaam-v3", "ru").unwrap().rtf,
                reference("gigaam-v3", GENERIC_LANGUAGE).unwrap().rtf,
            );
            assert!((auto / fixed - 1.0).abs() < 0.2, "{auto} vs {fixed}");
        }
        assert!(reference("custom-missing", "ru").is_none());
    }

    #[test]
    fn scale_is_monotonic_and_not_a_percentage_of_accuracy() {
        assert!(speed_score(0.05) > speed_score(0.5));
        assert!(speed_score(0.5) > speed_score(2.0));
        assert_eq!(speed_score(1.0), 0.5);
    }
    #[test]
    fn catalog_speed_follows_the_configured_language() {
        let configured = catalog_speed("tiny", "ru");
        assert_eq!(configured.source, "reference");
        assert_eq!(
            configured.score,
            Some(speed_score(reference("tiny", "ru").unwrap().rtf))
        );
        assert!(configured.score > catalog_speed("tiny", GENERIC_LANGUAGE).score);
        let missing = catalog_speed("custom-missing", "ru");
        assert_eq!(missing.score, None);
        assert_eq!(missing.source, "unknown");
    }

    #[test]
    fn detection_overhead_does_not_demote_a_model_whose_language_is_set() {
        // `base` reads as the lowest level at `auto` and the middle one with a
        // language chosen. Showing the detection number to someone who will
        // never pay for detection understates Whisper against Sherpa.
        let configured = catalog_speed("base", "ru").score.unwrap();
        let detected = catalog_speed("base", GENERIC_LANGUAGE).score.unwrap();
        assert!(detected < 0.5, "auto should fall below the middle level");
        assert!(configured >= 0.5, "a chosen language should reach it");
    }

    #[test]
    fn download_capacity_distinguishes_full_and_unknown_disk() {
        assert!(download_assessment("tiny", Some(0)).insufficient);
        assert!(!download_assessment("tiny", None).insufficient);
        let required = download_assessment("tiny", None).required_bytes.unwrap();
        assert!(!download_assessment("tiny", Some(required)).insufficient);
        assert!(download_assessment("tiny", Some(required - 1)).insufficient);
        assert!(!download_assessment("custom-missing", Some(0)).insufficient);
    }

    #[test]
    fn reference_catalog_has_valid_metrics_and_unique_contexts() {
        let values: Vec<Reference> =
            serde_json::from_str(include_str!("model_reference.json")).unwrap();
        let mut seen = std::collections::HashSet::new();
        for value in values {
            assert_eq!(value.revision.len(), 64);
            assert!(value.revision.bytes().all(|byte| byte.is_ascii_hexdigit()));
            assert_eq!(value.method, METHOD);
            assert!(value.rtf.is_finite() && value.rtf > 0.0);
            assert!(value.audio_seconds.is_finite() && value.audio_seconds > 0.0);
            assert!(value.median_ms.is_finite() && value.median_ms > 0.0);
            assert!((value.rtf - value.median_ms / 1000.0 / value.audio_seconds).abs() < 1e-9);
            assert!(!value.machine.is_empty());
            assert!(matches!(value.fixture_language.as_str(), "ru" | "en"));
            assert!(seen.insert((value.model_id, value.compute, value.language)));
        }
    }

    #[test]
    fn reference_artifacts_match_the_platform_catalog() {
        let values: Vec<Reference> =
            serde_json::from_str(include_str!("model_reference.json")).unwrap();
        let mut checked = 0;
        for value in values {
            // References are shared across targets, but Linux deliberately
            // omits the Sherpa registry. Windows/macOS validate every entry.
            if !cfg!(any(windows, target_os = "macos"))
                && model::catalog_model(&value.model_id).is_none()
            {
                continue;
            }
            assert!(
                model::catalog_model(&value.model_id).is_some(),
                "{}",
                value.model_id
            );
            assert_eq!(
                value.revision,
                revision(&value.model_id),
                "{}",
                value.model_id
            );
            checked += 1;
        }
        assert!(
            checked > 0,
            "no reference artifacts checked for this target"
        );
    }
}
