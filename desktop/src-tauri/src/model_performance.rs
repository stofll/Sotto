//! Local, bounded observations. Recording never waits for persistence.
use crate::{hardware_profile, model};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::sync::{
    mpsc::{sync_channel, SyncSender},
    OnceLock,
};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{Emitter, Manager};

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
const RETENTION: u64 = 30 * 24 * 60 * 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunSource {
    Dictation,
    File,
}

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
        let mut hardware = hardware_profile::fingerprint().to_owned();
        if compute == "gpu_unverified" {
            // Without adapter/driver identity, do not reuse GPU-requested runs
            // across restarts where the effective device may have changed.
            static SESSION: OnceLock<String> = OnceLock::new();
            hardware.push_str(SESSION.get_or_init(|| format!("-{:?}", SystemTime::now())));
        }
        Self {
            model_id: model_id.to_owned(),
            revision: revision(model_id),
            hardware,
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

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Observation {
    pub profile: Profile,
    pub created: u64,
    pub source: RunSource,
    pub language: String,
    pub custom_prompt: String,
    pub cold: bool,
    pub audio_seconds: f64,
    pub inference_ms: f64,
    pub kind: String,
}

impl Observation {
    pub fn inference(
        profile: Profile,
        source: RunSource,
        language: &str,
        custom_prompt: &str,
        cold: bool,
        result: &crate::whisper::InferenceResult,
    ) -> Option<Self> {
        if result.text.trim().is_empty()
            || !result.audio_seconds.is_finite()
            || result.audio_seconds < 3.0
            || result.inference_time_ms == 0
        {
            return None;
        }
        Some(Self {
            profile,
            created: now(),
            source,
            language: language.to_owned(),
            custom_prompt: custom_prompt.to_owned(),
            cold,
            audio_seconds: result.audio_seconds,
            inference_ms: result.inference_time_ms as f64,
            kind: "inference".into(),
        })
    }

    pub fn load(profile: Profile, elapsed_ms: f64, success: bool) -> Self {
        Self {
            profile,
            created: now(),
            source: RunSource::Dictation,
            language: String::new(),
            custom_prompt: String::new(),
            cold: true,
            audio_seconds: 0.0,
            inference_ms: elapsed_ms,
            // Upstream load errors do not carry a reliable OOM category.
            kind: if success { "load" } else { "load_failed" }.into(),
        }
    }

    fn group(&self) -> String {
        format!(
            "{:?}:{}:{}:{}:{}:{}:{}",
            self.profile,
            self.source as u8,
            self.language,
            self.custom_prompt,
            self.cold,
            duration_band(self.audio_seconds),
            self.kind,
        )
    }
}

pub fn prompt_fingerprint(prompt: Option<&str>) -> String {
    prompt
        .map(|text| format!("{:x}", Sha256::digest(text.as_bytes())))
        .unwrap_or_default()
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn duration_band(seconds: f64) -> u8 {
    if seconds < 10.0 {
        0
    } else if seconds <= 30.0 {
        1
    } else {
        2
    }
}

enum Message {
    Observe(Observation),
    Reset(String, tokio::sync::oneshot::Sender<Result<(), String>>),
}

pub struct Recorder {
    tx: SyncSender<Message>,
}

impl Recorder {
    pub fn start(path: PathBuf, app: tauri::AppHandle) -> Self {
        let (tx, rx) = sync_channel(64);
        std::thread::spawn(move || {
            let Ok(mut db) = Connection::open(path) else {
                return;
            };
            let _ = db.busy_timeout(std::time::Duration::from_millis(100));
            let _ = db.execute(
                "DELETE FROM model_performance WHERE created < ?1",
                [now().saturating_sub(RETENTION)],
            );
            while let Ok(first) = rx.recv() {
                // A short transaction per bounded batch; no busy polling.
                let batch = std::iter::once(first)
                    .chain(rx.try_iter().take(31))
                    .collect();
                if apply_batch(&mut db, batch) {
                    let _ = app.emit("model-performance-changed", ());
                }
            }
        });
        Self { tx }
    }

    pub fn record(&self, observation: Observation) {
        let _ = self.tx.try_send(Message::Observe(observation));
    }

    pub async fn reset(&self, id: String) -> Result<(), String> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.tx
            .try_send(Message::Reset(id, tx))
            .map_err(|_| "PERFORMANCE_BUSY")?;
        rx.await.map_err(|_| "PERFORMANCE_UNAVAILABLE")?
    }
}

fn apply_batch(db: &mut Connection, batch: Vec<Message>) -> bool {
    let result: rusqlite::Result<()> = (|| {
        let transaction = db.transaction()?;
        for message in &batch {
            match message {
                Message::Observe(value) => insert(&transaction, value)?,
                Message::Reset(id, _) => clear(&transaction, id)?,
            }
        }
        transaction.commit()
    })();
    let committed = result.is_ok();
    // A reset is acknowledged only after the entire ordered batch commits.
    for message in batch {
        if let Message::Reset(_, reply) = message {
            let _ = reply.send(if committed {
                Ok(())
            } else {
                Err("PERFORMANCE_RESET_FAILED".into())
            });
        }
    }
    committed
}

pub fn record(app: &tauri::AppHandle, observation: Observation) {
    if let Some(recorder) = app.try_state::<Recorder>() {
        recorder.record(observation);
    }
}

fn insert(db: &Connection, value: &Observation) -> rusqlite::Result<()> {
    let profile = value.group();
    let payload = serde_json::to_string(value).expect("finite observation");
    db.execute("INSERT INTO model_performance(model_id, created, profile, payload) VALUES (?1, ?2, ?3, ?4)",
        params![value.profile.model_id, value.created, profile, payload])?;
    db.execute(
        "DELETE FROM model_performance WHERE created < ?1",
        [now().saturating_sub(RETENTION)],
    )?;
    db.execute("DELETE FROM model_performance WHERE profile = ?1 AND id NOT IN (SELECT id FROM model_performance WHERE profile = ?1 ORDER BY id DESC LIMIT 30)", [&profile])?;
    db.execute("DELETE FROM model_performance WHERE id NOT IN (SELECT id FROM model_performance ORDER BY id DESC LIMIT 2000)", [])?;
    Ok(())
}

fn read_payloads(db: &Connection) -> rusqlite::Result<Vec<String>> {
    let mut query = db.prepare(
        "SELECT payload FROM model_performance WHERE created >= ?1 ORDER BY id DESC LIMIT 2000",
    )?;
    let rows = query.query_map([now().saturating_sub(RETENTION)], |row| {
        row.get::<_, String>(0)
    })?;
    rows.collect()
}

fn decode_observations(payloads: Vec<String>) -> Vec<Observation> {
    payloads
        .into_iter()
        .filter_map(|json| serde_json::from_str(&json).ok())
        .collect()
}

#[cfg(test)]
fn read(db: &Connection) -> rusqlite::Result<Vec<Observation>> {
    read_payloads(db).map(decode_observations)
}

fn clear(db: &Connection, id: &str) -> rusqlite::Result<()> {
    db.execute("DELETE FROM model_performance WHERE model_id = ?1", [id])?;
    Ok(())
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
    pub load_failed: bool,
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
/// cost by about a factor of two. Dictation observations are still recorded —
/// `assess` reads them for `load_failed`, and the reset command clears them —
/// but they never reach the card: five samples from one machine, gathered
/// while the user was doing something else, are not a scale anyone can compare
/// models on.
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

pub fn assess(
    observations: &[Observation],
    models: &[model::ModelInfo],
    gpu: bool,
    language: &str,
) -> Vec<Assessment> {
    let hardware = hardware_profile::snapshot();
    let disk_available = model::models_dir()
        .ok()
        .and_then(|dir| crate::model_download::available_bytes(&dir));

    models
        .iter()
        .map(|model| {
            let compute = hardware_profile::compute(model.cpu_only, gpu);
            let profile = Profile::new(&model.id, compute);
            let last_load = observations.iter().find(|value| {
                value.profile == profile
                    && value.kind.starts_with("load")
                    && value.created >= now().saturating_sub(24 * 60 * 60)
            });
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
                load_failed: last_load.is_some_and(|value| value.kind == "load_failed"),
            }
        })
        .collect()
}

#[tauri::command]
pub async fn model_assessments(
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::state::AppState>,
) -> Result<Vec<Assessment>, String> {
    let db = state.db.clone();
    let current = crate::mutex_recover::lock(&state.engine_current_model).clone();
    tauri::async_runtime::spawn_blocking(move || {
        let config = crate::config::Config::load(&app)?;
        let selected = config.get_string("model").unwrap_or_default();
        let language = config
            .get_string("language")
            .unwrap_or_else(|| "auto".into());
        let gpu = crate::config::resolve_device(config.as_value()) != "cpu";
        let models = model::list_model_infos(&selected, current.as_deref());
        let payloads = read_payloads(&crate::mutex_recover::lock(&db))
            .map_err(|_| "PERFORMANCE_UNAVAILABLE".to_owned())?;
        let observations = decode_observations(payloads);
        Ok(assess(&observations, &models, gpu, &language))
    })
    .await
    .map_err(|_| "PERFORMANCE_UNAVAILABLE".to_owned())?
}

#[tauri::command]
pub async fn reset_model_assessment(
    id: String,
    state: tauri::State<'_, Recorder>,
) -> Result<(), String> {
    state.reset(id).await
}

#[cfg(test)]
mod tests {
    use super::*;
    fn profile() -> Profile {
        Profile {
            model_id: "tiny".into(),
            revision: "fixture".into(),
            hardware: "test".into(),
            method: METHOD.into(),
            compute: "cpu".into(),
        }
    }
    fn sample() -> Observation {
        Observation {
            profile: profile(),
            created: now(),
            source: RunSource::Dictation,
            language: "en".into(),
            custom_prompt: String::new(),
            cold: false,
            audio_seconds: 20.0,
            inference_ms: 2000.0,
            kind: "inference".into(),
        }
    }
    fn database() -> Connection {
        let db = Connection::open_in_memory().unwrap();
        crate::db::run_migrations(&db).unwrap();
        db
    }
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
    fn batches_preserve_reset_order_and_report_rollbacks() {
        let mut db = database();
        let (reply, mut response) = tokio::sync::oneshot::channel();
        let mut last = sample();
        last.inference_ms = 1234.0;
        assert!(apply_batch(
            &mut db,
            vec![
                Message::Observe(sample()),
                Message::Reset("tiny".into(), reply),
                Message::Observe(last)
            ]
        ));
        assert_eq!(response.try_recv().unwrap(), Ok(()));
        let values = read(&db).unwrap();
        assert_eq!(values.len(), 1);
        assert_eq!(values[0].inference_ms, 1234.0);
        db.execute_batch("CREATE TRIGGER reject_measurement BEFORE INSERT ON model_performance BEGIN SELECT RAISE(ABORT, 'synthetic failure'); END;").unwrap();
        let (reply, mut response) = tokio::sync::oneshot::channel();
        assert!(!apply_batch(
            &mut db,
            vec![
                Message::Reset("tiny".into(), reply),
                Message::Observe(sample())
            ]
        ));
        assert_eq!(
            response.try_recv().unwrap(),
            Err("PERFORMANCE_RESET_FAILED".into())
        );
        assert_eq!(read(&db).unwrap()[0].inference_ms, 1234.0);
    }
    #[test]
    fn persistence_is_bounded_and_old_corrupt_samples_are_ignored() {
        let db = database();
        for _ in 0..40 {
            insert(&db, &sample()).unwrap();
        }
        assert_eq!(read(&db).unwrap().len(), 30);
        let mut old = sample();
        old.created = now() - RETENTION - 1;
        insert(&db, &old).unwrap();
        assert_eq!(read(&db).unwrap().len(), 30);
        db.execute("INSERT INTO model_performance(model_id,created,profile,payload) VALUES ('tiny',?1,'bad','invalid')", [now()]).unwrap();
        assert_eq!(read(&db).unwrap().len(), 30);
    }
    #[test]
    fn scale_is_monotonic_and_not_a_percentage_of_accuracy() {
        assert!(speed_score(0.05) > speed_score(0.5));
        assert!(speed_score(0.5) > speed_score(2.0));
        assert_eq!(speed_score(1.0), 0.5);
    }
    #[test]
    fn observations_exclude_empty_short_and_invalid_results() {
        let mut result = crate::whisper::InferenceResult {
            session_id: 1,
            text: "synthetic speech".into(),
            language: Some("en".into()),
            model_id: Some("tiny".into()),
            stt_service: None,
            inference_time_ms: 200,
            audio_seconds: 4.0,
            speech_seconds: None,
        };
        let observe = |result: &crate::whisper::InferenceResult| {
            Observation::inference(profile(), RunSource::Dictation, "en", "", false, result)
        };
        assert!(observe(&result).is_some());
        result.audio_seconds = 2.9;
        assert!(observe(&result).is_none());
        result.audio_seconds = f64::NAN;
        assert!(observe(&result).is_none());
        result.audio_seconds = 4.0;
        result.text.clear();
        assert!(observe(&result).is_none());
        assert_ne!(
            prompt_fingerprint(Some("first")),
            prompt_fingerprint(Some("second"))
        );
    }

    #[test]
    fn persisted_samples_survive_reopen_and_reset_is_model_scoped() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("measurements.db");
        {
            let db = Connection::open(&path).unwrap();
            crate::db::run_migrations(&db).unwrap();
            for _ in 0..5 {
                insert(&db, &sample()).unwrap();
            }
            let mut other = sample();
            other.profile.model_id = "base".into();
            insert(&db, &other).unwrap();
        }
        let db = Connection::open(path).unwrap();
        assert_eq!(read(&db).unwrap().len(), 6);
        clear(&db, "tiny").unwrap();
        let remaining = read(&db).unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].profile.model_id, "base");
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
