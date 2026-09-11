//! Local, bounded observations. Recording never waits for persistence.
use crate::{hardware_profile, model};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::{
    mpsc::{sync_channel, SyncSender},
    OnceLock,
};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{Emitter, Manager};

const METHOD: &str = "sotto-stt-v1-whisper-0.14.4-sherpa-1.13.7";
const RETENTION: u64 = 30 * 24 * 60 * 60;
const MIN_SAMPLES: usize = 5;

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

#[derive(Clone, Debug, Serialize)]
pub struct SpeedAssessment {
    pub score: Option<f64>,
    pub source: &'static str,
    pub samples: usize,
    pub median_ms: Option<f64>,
    pub audio_min: Option<f64>,
    pub audio_max: Option<f64>,
    pub unstable: bool,
    pub cold: bool,
    pub reference: Option<String>,
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
    pub load_failed: bool,
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
    if loaded {
        result.status = "loaded";
    } else if compute != "cpu" {
        result.status = "gpu_unknown";
    } else if let (Some(required), Some(available)) = (required.filter(|n| *n > 0), available) {
        let budget = available as f64 * 0.8;
        result.status = if budget < required as f64 {
            "low"
        } else {
            "enough"
        };
        result.score = Some(if budget <= 0.0 {
            0.0
        } else {
            (1.0 - required as f64 / budget).clamp(0.0, 1.0)
        });
    }
    result
}

#[derive(Deserialize)]
pub struct Reference {
    pub model_id: String,
    pub revision: String,
    pub compute: String,
    pub language: String,
    pub rtf: f64,
    pub machine: String,
    pub method: String,
}

fn reference(profile: &Profile, language: &str) -> Option<&'static Reference> {
    static REFERENCES: OnceLock<Vec<Reference>> = OnceLock::new();
    let references = REFERENCES.get_or_init(|| {
        serde_json::from_str(include_str!("model_reference.json")).unwrap_or_default()
    });
    references.iter().find(|value| {
        value.model_id == profile.model_id
            && value.revision == profile.revision
            && value.compute == profile.compute
            && value.method == METHOD
            && value.language == language
            && value.rtf.is_finite()
            && value.rtf > 0.0
    })
}

fn speed(
    profile: &Profile,
    observations: &[Observation],
    language: &str,
    prompt: &str,
) -> SpeedAssessment {
    let mut result = SpeedAssessment {
        score: None,
        source: "unknown",
        samples: 0,
        median_ms: None,
        audio_min: None,
        audio_max: None,
        unstable: false,
        cold: false,
        reference: None,
    };
    if let Some(reference) = reference(profile, language) {
        result.score = Some(speed_score(reference.rtf));
        result.source = "reference";
        result.reference = Some(reference.machine.clone());
    }
    let candidates: Vec<_> = observations
        .iter()
        .filter(|value| {
            value.profile == *profile
                && value.kind == "inference"
                && value.source == RunSource::Dictation
                && value.language == language
                && value.custom_prompt == prompt
                && value.audio_seconds.is_finite()
                && value.audio_seconds >= 3.0
                && value.inference_ms.is_finite()
                && value.inference_ms > 0.0
        })
        .collect();
    // Prefer a representative 10–30s warm group, then the most populated group.
    let mut groups = Vec::new();
    for cold in [false, true] {
        for band in [1, 0, 2] {
            let group: Vec<_> = candidates
                .iter()
                .copied()
                .filter(|value| value.cold == cold && duration_band(value.audio_seconds) == band)
                .take(30)
                .collect();
            groups.push(group);
        }
    }
    let group = groups
        .iter()
        .find(|group| group.len() >= MIN_SAMPLES)
        .or_else(|| groups.iter().max_by_key(|group| group.len()));
    let Some(group) = group.filter(|group| !group.is_empty()) else {
        return result;
    };
    result.samples = group.len();
    if group.len() < MIN_SAMPLES {
        return result;
    }
    let mut ratios: Vec<_> = group
        .iter()
        .map(|value| value.inference_ms / 1000.0 / value.audio_seconds)
        .collect();
    ratios.sort_by(f64::total_cmp);
    let mut times: Vec<_> = group.iter().map(|value| value.inference_ms).collect();
    times.sort_by(f64::total_cmp);
    let median = |values: &[f64]| (values[(values.len() - 1) / 2] + values[values.len() / 2]) / 2.0;
    result.unstable = ratios[(ratios.len() - 1) * 3 / 4] > ratios[(ratios.len() - 1) / 4] * 3.0;
    if result.unstable && result.source == "reference" {
        return result;
    }
    result.score = (!result.unstable).then(|| speed_score(median(&ratios)));
    result.source = "personal";
    result.median_ms = Some(median(&times));
    result.audio_min = group
        .iter()
        .map(|value| value.audio_seconds)
        .reduce(f64::min);
    result.audio_max = group
        .iter()
        .map(|value| value.audio_seconds)
        .reduce(f64::max);
    result.cold = group[0].cold;
    result
}

pub fn assess(
    observations: &[Observation],
    models: &[model::ModelInfo],
    gpu: bool,
    language: &str,
    prompt: &str,
) -> Vec<Assessment> {
    let hardware = hardware_profile::snapshot();

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
                speed: speed(&profile, observations, language, prompt),
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
        let prompt = prompt_fingerprint(crate::custom_words_prompt(&config).as_deref());
        let models = model::list_models(&selected, current.as_deref());
        let payloads = read_payloads(&crate::mutex_recover::lock(&db))
            .map_err(|_| "PERFORMANCE_UNAVAILABLE".to_owned())?;
        let observations = decode_observations(payloads);
        Ok(assess(&observations, &models, gpu, &language, &prompt))
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

/// Developer benchmark output uses the same scale and revision contract.
pub fn write_references(path: &Path, values: &[serde_json::Value]) -> std::io::Result<()> {
    std::fs::write(path, serde_json::to_vec_pretty(values)?)
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
        assert_eq!(
            memory_assessment(Some(100), Some(20), true, "cpu").status,
            "loaded"
        );
        assert_eq!(
            memory_assessment(Some(100), Some(200), false, "gpu_unverified").score,
            None
        );
    }
    #[test]
    fn measurements_need_matching_context_and_five_runs() {
        let mut values = vec![sample(); 4];
        assert_eq!(speed(&profile(), &values, "en", "").source, "unknown");
        values.push(sample());
        let result = speed(&profile(), &values, "en", "");
        assert_eq!(result.source, "personal");
        assert_eq!(result.median_ms, Some(2000.0));
        assert_eq!(result.audio_min, Some(20.0));
        assert_eq!(speed(&profile(), &values, "ru", "").score, None);
        assert_eq!(speed(&profile(), &values, "en", "different").score, None);
        let mut changed = profile();
        changed.compute = "gpu_unverified".into();
        assert_eq!(speed(&changed, &values, "en", "").score, None);
        for value in &mut values {
            value.source = RunSource::File;
        }
        assert_eq!(speed(&profile(), &values, "en", "").score, None);
    }
    #[test]
    fn durations_cold_runs_and_outliers_do_not_create_false_confidence() {
        let mut values = vec![sample(); 5];
        values[0].cold = true;
        values[1].audio_seconds = 50.0;
        assert_ne!(speed(&profile(), &values, "en", "").source, "personal");
        let mut values = vec![sample(); 8];
        for value in &mut values[4..] {
            value.inference_ms *= 10.0;
        }
        let result = speed(&profile(), &values, "en", "");
        assert!(result.unstable);
        assert_eq!(result.score, None);
    }

    #[test]
    fn unstable_personal_runs_keep_an_available_reference() {
        let mut context = profile();
        context.revision = revision("tiny");
        let expected = speed(&context, &[], "ru", "");
        assert_eq!(expected.source, "reference");
        let mut values = vec![sample(); 8];
        for (index, value) in values.iter_mut().enumerate() {
            value.profile = context.clone();
            value.language = "ru".into();
            if index >= 4 {
                value.inference_ms *= 10.0;
            }
        }
        let result = speed(&context, &values, "ru", "");
        assert!(result.unstable);
        assert_eq!(result.samples, 8);
        assert_eq!(result.source, "reference");
        assert_eq!(result.score, expected.score);
        assert_eq!(result.median_ms, None);
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
            inference_time_ms: 200,
            audio_seconds: 4.0,
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
        assert_eq!(
            speed(&profile(), &read(&db).unwrap(), "en", "").source,
            "personal"
        );
        clear(&db, "tiny").unwrap();
        let remaining = read(&db).unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].profile.model_id, "base");
        assert_eq!(speed(&profile(), &remaining, "en", "").source, "unknown");
    }

    #[test]
    fn reference_catalog_has_valid_exact_artifacts_and_unique_contexts() {
        let values: Vec<Reference> =
            serde_json::from_str(include_str!("model_reference.json")).unwrap();
        let mut seen = std::collections::HashSet::new();
        for value in values {
            assert!(model::catalog_model(&value.model_id).is_some());
            assert_eq!(value.revision, revision(&value.model_id));
            assert_eq!(value.method, METHOD);
            assert!(value.rtf.is_finite() && value.rtf > 0.0);
            assert!(!value.machine.is_empty());
            assert!(seen.insert((value.model_id, value.compute, value.language)));
        }
    }
}
