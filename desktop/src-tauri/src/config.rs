//! JSON configuration, migrations, merge patches and live settings updates.

use serde_json::{Map, Value};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::SystemTime;
use tauri::{AppHandle, Manager};

const DEFAULT_HOTKEY: &str = "ctrl+shift+space";

// The application permits one process, but settings and hotkey commands can
// run concurrently. Serialize their entire read-modify-write operation.
static CONFIG_WRITES: Mutex<()> = Mutex::new(());

fn with_locked_config<T>(
    path: &Path,
    update: impl FnOnce(&mut Config, &Path) -> Result<T, String>,
) -> Result<T, String> {
    let _writer = crate::mutex_recover::lock(&CONFIG_WRITES);
    let mut config = Config::load_at(path)?;
    update(&mut config, path)
}

/// The last contents read or written for each config file, with the stamp
/// of the file they match.
///
/// A single dictation asks for the config several times, and most of those
/// reads sit between the transcript and the paste. A changed stamp — a hand
/// edit or another tool writing the file — sends the next load to disk.
static LOADED: LazyLock<Mutex<HashMap<PathBuf, (FileStamp, Value)>>> =
    LazyLock::new(Default::default);

#[derive(Clone, Copy, PartialEq, Eq)]
struct FileStamp {
    modified: SystemTime,
    len: u64,
}

fn file_stamp(path: &Path) -> Option<FileStamp> {
    let meta = fs::metadata(path).ok()?;
    Some(FileStamp {
        modified: meta.modified().ok()?,
        len: meta.len(),
    })
}

fn remember(path: &Path, stamp: FileStamp, data: &Value) {
    crate::mutex_recover::lock(&LOADED).insert(path.to_owned(), (stamp, data.clone()));
}

/// Value of the `device` config key meaning "run inference on the GPU".
pub const DEVICE_GPU: &str = "gpu";
/// Value of the `device` config key meaning "run inference on the CPU".
pub const DEVICE_CPU: &str = "cpu";

/// Load the saved hotkey from `config.json` in the app config dir.
/// Returns the default if the file or key is missing; read/parse errors propagate.
pub fn load_hotkey(app: &AppHandle) -> Result<String, String> {
    let config = Config::load(app)?;
    Ok(hotkey_from(&config))
}

/// Read the hotkey out of an already-loaded `Config`, falling back to the
/// default when the key is absent. Extracted from [`load_hotkey`] so the
/// fallback is testable without an `AppHandle`.
fn hotkey_from(config: &Config) -> String {
    config
        .get_string("hotkey")
        .unwrap_or_else(|| DEFAULT_HOTKEY.to_string())
}

/// Accept old numeric indexes as well as persistent device names.
pub fn microphone_selection(value: Option<Value>) -> Option<String> {
    match value {
        Some(Value::String(s)) if !s.is_empty() => Some(s),
        Some(Value::Number(n)) if n.is_u64() => Some(n.to_string()),
        _ => None,
    }
}

/// Where the app reads and writes `config.json`.
///
/// `pub` so startup can record it in the log. When the config reads as
/// empty the first question is always *which* file was opened, and
/// answering it from outside the process means re-deriving Tauri's
/// `app_config_dir()` by hand and hoping the derivation matches.
pub fn config_path(app: &AppHandle) -> Result<PathBuf, String> {
    if let Some(dir) = crate::portable::data_dir() {
        return Ok(dir.join("config.json"));
    }
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("app_config_dir: {e}"))?;
    Ok(dir.join("config.json"))
}

/// Owned snapshot of `config.json`; cloning copies the JSON tree.
/// Disk writers load their snapshot inside `with_locked_config`.
#[derive(Debug, Clone)]
pub struct Config {
    data: Value,
}

impl Config {
    /// Load `config.json` from the app config dir. Returns an empty config
    /// (`{}`) if the file does not exist — first-run case.
    pub fn load(app: &AppHandle) -> Result<Self, String> {
        Self::load_at(&config_path(app)?)
    }

    /// Load a config from an explicit path. Returns an empty config (`{}`)
    /// if the file does not exist — first-run case.
    fn load_at(path: &Path) -> Result<Self, String> {
        // Stamped before reading: a write that lands in between leaves an
        // older stamp next to newer contents, which costs one more read
        // rather than serving stale settings.
        let stamp = file_stamp(path);
        if let Some(stamp) = stamp {
            if let Some((cached, data)) = crate::mutex_recover::lock(&LOADED).get(path) {
                if *cached == stamp {
                    return Ok(Self { data: data.clone() });
                }
            }
        }
        if !path.exists() {
            return Ok(Self {
                data: Value::Object(Map::new()),
            });
        }
        let raw = fs::read_to_string(path).map_err(|e| format!("read config.json: {e}"))?;
        let mut data: Value =
            serde_json::from_str(&raw).map_err(|e| format!("parse config.json: {e}"))?;
        crate::dictionaries::migrate(&mut data);
        crate::overlay_preferences::migrate(&mut data);
        if let Some(stamp) = stamp {
            remember(path, stamp, &data);
        }
        Ok(Self { data })
    }

    /// Read-only accessor for a single key.
    pub fn get(&self, key: &str) -> Option<Value> {
        self.data.get(key).cloned()
    }

    /// Set a single key in this owned snapshot.
    pub fn set(&mut self, key: &str, value: Value) -> Result<(), String> {
        let map = self
            .data
            .as_object_mut()
            .ok_or_else(|| "config root is not a JSON object".to_string())?;
        map.insert(key.to_string(), value);
        Ok(())
    }

    /// Read a string value, or `None` if the key is absent or has another type.
    pub fn get_string(&self, key: &str) -> Option<String> {
        self.data.get(key)?.as_str().map(str::to_owned)
    }

    /// Borrow the underlying `serde_json::Value`. Used by callers that
    /// need to walk the on-disk JSON tree (e.g. `build_cloud_stt_request`
    /// which composes a request from multiple fields under
    /// `ai_processing`).
    pub fn as_value(&self) -> &Value {
        &self.data
    }

    /// Replace the file via a sibling temporary file. Production writers hold
    /// CONFIG_WRITES from snapshot loading through this rename and runtime sync.
    fn save_at(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("create config dir: {e}"))?;
        }
        let pretty = serde_json::to_string_pretty(&self.data)
            .map_err(|e| format!("serialize config: {e}"))?;
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, &pretty).map_err(|e| format!("write config tmp: {e}"))?;
        fs::rename(&tmp, path).map_err(|e| format!("rename config tmp: {e}"))?;
        if let Some(stamp) = file_stamp(path) {
            remember(path, stamp, &self.data);
        }
        Ok(())
    }

    /// Apply an RFC 7396 JSON Merge Patch to the in-memory config.
    /// The patch is merged into `self.data` (objects recurse, null
    /// removes keys, scalars/arrays replace atomically).
    pub fn apply_merge_patch(&mut self, patch: &Value) -> Result<(), String> {
        let merged = merge_json_patch(self.as_value().clone(), patch.clone());
        self.data = merged;
        Ok(())
    }
}

/// RFC 7396 JSON Merge Patch.
///
/// Returns a new `Value` that is the result of applying `patch` onto
/// `target`. Semantics:
/// - If `patch` is `null`, return `null` (delete the whole target).
/// - An object patch treats a non-object target as an empty object, then
///   recurses for each key: null in patch → delete; otherwise merge recursively.
/// - Otherwise, `patch` replaces `target` (arrays replace atomically;
///   they are NOT merged element-wise).
pub fn merge_json_patch(target: Value, patch: Value) -> Value {
    match (target, patch) {
        (target, Value::Object(p)) => {
            let mut t = match target {
                Value::Object(object) => object,
                _ => Map::new(),
            };
            for (k, v) in p {
                if v.is_null() {
                    t.remove(&k);
                } else {
                    let existing = t.remove(&k).unwrap_or(Value::Null);
                    t.insert(k, merge_json_patch(existing, v));
                }
            }
            Value::Object(t)
        }
        (_, p) => p,
    }
}

/// Resolve the `device` config key to the canonical `"gpu"` / `"cpu"`.
///
/// GPU is the default: whisper.cpp itself falls back to CPU at runtime when
/// the Vulkan/Metal device cannot be initialised, so an explicit `"cpu"` is
/// only for the cases where that fallback does not trigger (a driver that
/// initialises but then misbehaves) or for A/B-ing a suspected GPU problem.
///
/// `"cuda"` is the legacy spelling: the setting predates the Vulkan backend
/// and the app has never actually used CUDA. It is read as `"gpu"` here, and
/// [`migrate_legacy_device`] rewrites it on startup so the on-disk value and
/// the UI stop disagreeing.
pub fn resolve_device(config: &Value) -> &'static str {
    match config.get("device").and_then(Value::as_str) {
        Some(DEVICE_CPU) => DEVICE_CPU,
        _ => DEVICE_GPU,
    }
}

/// Convenience wrapper: does [`resolve_device`] say "use the GPU"?
pub fn device_uses_gpu(config: &Value) -> bool {
    resolve_device(config) == DEVICE_GPU
}

/// Key: after how many idle minutes the model is unloaded from RAM.
pub const MODEL_UNLOAD_KEY: &str = "model_unload_after_minutes";

/// How many idle minutes to wait when the config says nothing.
///
/// A large model holds several gigabytes in memory, and between dictations
/// nobody needs them. Five minutes is the compromise: people dictate in bursts,
/// and within a burst reloading would cost more than the freed memory is worth.
pub const DEFAULT_MODEL_UNLOAD_MINUTES: u64 = 5;

/// The values the interface offers. The config is also edited by hand, so
/// [`model_unload_after_minutes`] accepts any number in the range, not just
/// these four.
pub const MODEL_UNLOAD_CHOICES: [u64; 4] = [0, 5, 10, 30];

/// A day of idling is already "never", just written as a number. The upper
/// bound exists for the timer rather than the user: a `Duration` of thousands of
/// minutes is no better than unloading being off, yet it looks like a working
/// setting.
const MAX_MODEL_UNLOAD_MINUTES: u64 = 24 * 60;

/// After how many idle minutes to unload the model. `0` — never unload.
///
/// A missing key does not mean "never" but the default: unloading is on, and
/// old configs receive it along with the update.
pub fn model_unload_after_minutes(config: &Value) -> u64 {
    let minutes = config
        .get(MODEL_UNLOAD_KEY)
        .and_then(Value::as_u64)
        .unwrap_or(DEFAULT_MODEL_UNLOAD_MINUTES);
    minutes.min(MAX_MODEL_UNLOAD_MINUTES)
}

/// Key: after how many minutes a recording stops by itself.
pub const RECORDING_LIMIT_KEY: &str = "recording_limit_minutes";

/// A recording left running in toggle mode grows by about 230 MB an hour and
/// then goes to the engine whole. The values are duplicated in
/// `src/pages/recordingLimitSettings.ts`.
pub const DEFAULT_RECORDING_LIMIT_MINUTES: u64 = 15;

/// Longer than any dictation, and still a bound on memory.
const MAX_RECORDING_LIMIT_MINUTES: u64 = 24 * 60;

/// After how many minutes of recording to stop and transcribe. `0` — never.
pub fn recording_limit_minutes(config: &Value) -> u64 {
    config
        .get(RECORDING_LIMIT_KEY)
        .and_then(Value::as_u64)
        .unwrap_or(DEFAULT_RECORDING_LIMIT_MINUTES)
        .min(MAX_RECORDING_LIMIT_MINUTES)
}

/// One-shot startup migration of the compute-device settings.
///
/// - `device: "cuda"` → `"gpu"` (see [`resolve_device`]).
/// - `compute_type` is dropped: it is a faster-whisper leftover with no
///   whisper.cpp equivalent — quantisation is baked into the GGML file, so
///   the setting never did anything.
///
/// Only writes when something actually changed, so the common case does not
/// touch the disk. Errors are the caller's to log and ignore: a failed
/// migration leaves a config that [`resolve_device`] still reads correctly.
pub fn migrate_legacy_device(app: &AppHandle) -> Result<bool, String> {
    migrate_legacy_device_at(&config_path(app)?)
}

/// Path-closed variant of [`migrate_legacy_device`] so the migration can be
/// exercised against a temp file without an `AppHandle`.
fn migrate_legacy_device_at(path: &Path) -> Result<bool, String> {
    with_locked_config(path, migrate_legacy_device_config)
}

fn migrate_legacy_device_config(cfg: &mut Config, path: &Path) -> Result<bool, String> {
    let mut changed = false;
    if cfg.get_string("device").as_deref() == Some("cuda") {
        cfg.set("device", Value::String(DEVICE_GPU.to_string()))?;
        changed = true;
    }
    if let Some(map) = cfg.data.as_object_mut() {
        changed |= map.remove("compute_type").is_some();
    }
    if changed {
        cfg.save_at(path)?;
    }
    Ok(changed)
}

/// Invariants a config must hold no matter who writes it.
///
/// This lives here rather than inside the settings command because the
/// hotkey updates reach the same file and must enforce the same rules.
///
/// Only config-only rules belong here. A rule that needs runtime state — what
/// the engine currently has loaded, what a device reports — cannot be decided
/// from a `Value` and stays with the caller that owns that state.
///
/// Deliberately not called from [`Config::save_at`]: `migrate_legacy_device`
/// writes through it to *repair* an old config, and a repair must not be
/// blocked by the very invariant it may be fixing.
pub fn validate(candidate: &Value) -> Result<(), String> {
    // The interface colour is free-form — the presets in the UI are shortcuts,
    // not the permitted set — so only the notation is checked here.
    if let Some(accent) = candidate.get("ui_accent") {
        let valid = accent.as_str().is_some_and(|value| {
            value.len() == 7
                && value.starts_with('#')
                && value[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
        });
        if !valid {
            return Err("Invalid ui_accent".into());
        }
    }
    crate::overlay_preferences::validate(candidate)?;
    validate_speech_route(candidate)?;
    crate::dictionaries::validate(candidate)
}

/// Reject a speech language outside the selected model's declared languages.
fn validate_speech_route(candidate: &Value) -> Result<(), String> {
    let Some(model) = candidate.get("model").and_then(Value::as_str) else {
        return Ok(());
    };
    let Some(languages) = crate::model::model_languages(model) else {
        return Ok(());
    };
    // The language is not stored next to the model — take the first from its
    // list: that is exactly the one the settings will switch to on their own.
    let language = candidate
        .get("language")
        .and_then(Value::as_str)
        .unwrap_or_else(|| languages.first().copied().unwrap_or("auto"));
    if crate::model::model_supports_language(model, language) {
        return Ok(());
    }
    Err(crate::model::language_unsupported_message(languages))
}

/// Exercise serialized persistence without native settings effects.
#[cfg(test)]
fn save_with_merge_patch_at(path: &Path, patch: Value) -> Result<Value, String> {
    with_locked_config(path, |cfg, path| {
        cfg.apply_merge_patch(&patch)?;
        validate(cfg.as_value())?;
        cfg.save_at(path)?;
        Ok(cfg.as_value().clone())
    })
}

/// The part of `save_config` that needs no `AppHandle`: merge `patch` into a
/// copy of `current`, let `check` refuse the result, validate and write it.
/// A patch with a hotkey rebinds it first and restores the previous binding
/// if the write fails. `current` is left as it was; the saved copy is
/// returned.
fn persist_patch(
    current: &Config,
    path: &Path,
    patch: &Value,
    check: impl FnOnce(&Config) -> Result<(), String>,
    replace_binding: impl FnOnce(&str, &str) -> Result<crate::hotkey::BindingRollback, String>,
) -> Result<Config, String> {
    let mut candidate = current.clone();
    candidate.apply_merge_patch(patch)?;
    check(&candidate)?;
    validate(candidate.as_value())?;
    if patch.get("hotkey").is_some() {
        persist_with_hotkey(&candidate, path, &hotkey_from(current), replace_binding)?;
    } else {
        candidate.save_at(path)?;
    }
    Ok(candidate)
}

fn persist_with_hotkey(
    config: &Config,
    path: &Path,
    old: &str,
    replace_binding: impl FnOnce(&str, &str) -> Result<crate::hotkey::BindingRollback, String>,
) -> Result<(), String> {
    let hotkey = hotkey_from(config);
    let rollback = replace_binding(old, &hotkey)?;
    if let Err(error) = config.save_at(path) {
        return match rollback() {
            Ok(()) => Err(error),
            Err(rollback) => Err(format!("{error}; restore previous shortcut: {rollback}")),
        };
    }
    Ok(())
}

/// Return the full on-disk config as a JSON value.
#[tauri::command]
pub(crate) fn get_config(app: AppHandle) -> Result<Value, String> {
    let cfg = Config::load(&app)?;
    Ok(cfg.as_value().clone())
}

/// Save a JSON Merge Patch to the on-disk config.
///
/// The frontend sends a partial config object
/// (`patch`) which is merged per RFC 7396: null removes keys,
/// scalars/arrays replace atomically, objects recurse.
/// Changing `device` (CPU / GPU) additionally triggers a model reload:
/// `use_gpu` is a *context* parameter in whisper.cpp, so it only takes
/// effect when the context is created. Without the reload the setting
/// would appear to save and change nothing until the next restart.
///
/// The reload runs detached so the settings UI is not blocked for the
/// second-plus a large model takes to load; the frontend already reacts to
/// the `whisper-loading` / `whisper-ready` events the engine emits.
#[tauri::command]
pub(crate) async fn save_config(
    app: AppHandle,
    state: tauri::State<'_, crate::state::AppState>,
    patch: Value,
) -> Result<Value, String> {
    let state = state.inner().clone();
    // Neither disk I/O nor waiting for a writer/native menu may block the UI
    // thread or an async runtime worker.
    tauri::async_runtime::spawn_blocking(move || {
        with_locked_config(&config_path(&app)?, |current_config, path| {
            save_config_locked(&app, &state, current_config, path, patch)
        })
    })
    .await
    .map_err(|error| format!("config worker: {error}"))?
}

fn save_config_locked(
    app: &AppHandle,
    state: &crate::state::AppState,
    current_config: &mut Config,
    path: &Path,
    patch: Value,
) -> Result<Value, String> {
    let device_before = resolve_device(current_config.as_value());
    // The configured-model half of the GigaAM language rule lives in
    // `config::validate`, which every writer goes through. This half cannot:
    // it asks what the engine has loaded right now, which no `Value` knows.
    let check_loaded_model = |candidate: &Config| {
        if patch.get("language").is_none() && patch.get("model").is_none() {
            return Ok(());
        }
        let language = candidate
            .get_string("language")
            .unwrap_or_else(|| "ru".to_string());
        let loaded_model = crate::mutex_recover::lock(&state.engine_current_model).clone();
        match loaded_model.as_deref() {
            Some(model) if !crate::model::model_supports_language(model, &language) => {
                let languages = crate::model::model_languages(model).unwrap_or_default();
                Err(crate::model::language_unsupported_message(languages))
            }
            _ => Ok(()),
        }
    };
    let saved = persist_patch(
        current_config,
        path,
        &patch,
        check_loaded_model,
        |old, new| crate::hotkey::re_register_with_rollback(app, state, old, new),
    )?
    .as_value()
    .clone();
    let device_after = resolve_device(&saved);
    apply_runtime_config(app, &saved, &patch);

    if device_before != device_after {
        // Nothing to reload if no model is loaded — whatever loads next
        // reads the new setting through `load_model_into_engine`.
        let loaded = crate::mutex_recover::lock(&state.engine_current_model).clone();
        if let Some(model) = loaded {
            log::info!("device changed {device_before} → {device_after}, reloading {model}");
            let reload_app = app.clone();
            let reload_tx = state.engine_cmd_tx.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(error) = crate::load_model_into_engine(
                    &reload_app,
                    &reload_tx,
                    &model,
                    crate::whisper::ModelLoadReason::Requested,
                )
                .await
                {
                    log::warn!("reload after device change failed: {error}");
                }
            });
        }
    }

    Ok(saved)
}

/// Everything that has to happen inside the running app once a setting has
/// been written.
///
/// One place, rather than a `touches_X -> do_Y` chain growing in the middle of
/// the save command: a new live setting adds a branch here next to its
/// neighbours instead of another `if` three screens into an unrelated
/// function. Nothing here can fail the save — the value is already on disk,
/// so a subsystem that refuses to pick it up is logged, not propagated.
fn apply_runtime_config(app: &AppHandle, saved: &Value, patch: &Value) {
    use tauri::Manager;
    if patch.get("overlay").is_some() {
        crate::overlay::configure(saved);
    }
    // Waiting for a restart here would keep capturing events after the user
    // opted out, which is the one thing the switch must not do.
    if patch.get(crate::telemetry::enabled_config_key()).is_some() {
        app.state::<crate::telemetry::Telemetry>()
            .set_enabled(crate::telemetry::enabled_from_value(saved));
    }
    if patch
        .get(crate::telemetry::session_timeout_config_key())
        .is_some()
    {
        app.state::<crate::telemetry::Telemetry>()
            .set_session_timeout_minutes(crate::telemetry::session_timeout_minutes_from_value(
                saved,
            ));
    }
    if patch.get("auto_start").is_some() {
        crate::apply_autostart(app);
    }
    if patch.get(crate::ui_text::CONFIG_KEY).is_some() {
        crate::ui_text::set_from_config(saved);
        // Refresh native menu labels while retaining the existing tray icon.
        if let Err(error) = crate::tray::build_tray(app) {
            log::warn!("tray menu language update failed: {error}");
        }
    }
    // Unconditional: cheap, and the point of turning up logging is usually to
    // catch the thing that is happening right now.
    crate::structured_log::set_level(crate::debug::log_level_from_config(saved));
    #[cfg(windows)]
    crate::windows::overlay_diag::configure(saved);
}

// ---------------------------------------------------------------------------
// Configuration tests use owned JSON and temporary paths. The opt-in tray
// regression uses a native AppHandle without application setup or persistence.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// `save_config` with a hotkey patch, minus the `AppHandle`: the same
    /// writer lock and [`persist_patch`], with the native rebind stubbed.
    fn change_hotkey_at(
        path: &Path,
        hotkey: &str,
        replace_binding: impl FnOnce(&str, &str) -> Result<crate::hotkey::BindingRollback, String>,
    ) -> Result<(), String> {
        with_locked_config(path, |config, path| {
            persist_patch(
                config,
                path,
                &json!({ "hotkey": hotkey }),
                |_| Ok(()),
                replace_binding,
            )
            .map(|_| ())
        })
    }

    #[test]
    fn concurrent_settings_and_hotkey_writers_preserve_independent_changes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let start = std::sync::Barrier::new(9);
        std::thread::scope(|scope| {
            for writer in 0..8 {
                let path = &path;
                let start = &start;
                scope.spawn(move || {
                    start.wait();
                    for update in 0..12 {
                        save_with_merge_patch_at(
                            path,
                            json!({
                                format!("writer_{writer}_{update}"): true
                            }),
                        )
                        .unwrap();
                    }
                });
            }
            start.wait();
            for _ in 0..12 {
                change_hotkey_at(&path, "ctrl+shift+a", |_, _| Ok(Box::new(|| Ok(())))).unwrap();
            }
        });
        let saved = Config::load_at(&path).unwrap();
        for writer in 0..8 {
            for update in 0..12 {
                assert_eq!(
                    saved.get(&format!("writer_{writer}_{update}")),
                    Some(json!(true))
                );
            }
        }
        assert_eq!(hotkey_from(&saved), "ctrl+shift+a");
        assert!(!path.with_extension("json.tmp").exists());
    }

    #[test]
    fn failed_hotkey_write_restores_binding_and_preserves_disk_before_retry() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let original = json!({"hotkey": "ctrl+space", "theme": "dark"});
        fs::write(&path, original.to_string()).unwrap();
        fs::create_dir(path.with_extension("json.tmp")).unwrap();
        let binding = std::rc::Rc::new(std::cell::RefCell::new("ctrl+space".to_string()));
        let changes = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let replace = |old: &str, new: &str| -> Result<crate::hotkey::BindingRollback, String> {
            assert_eq!(old, *binding.borrow());
            changes
                .borrow_mut()
                .push((old.to_string(), new.to_string()));
            *binding.borrow_mut() = new.to_string();
            let binding = binding.clone();
            let changes = changes.clone();
            let previous = old.to_string();
            Ok(Box::new(move || {
                changes
                    .borrow_mut()
                    .push((binding.borrow().clone(), previous.clone()));
                *binding.borrow_mut() = previous;
                Ok(())
            }))
        };
        assert!(change_hotkey_at(&path, "ctrl+shift+a", replace).is_err());
        fs::remove_dir(path.with_extension("json.tmp")).unwrap();
        assert_eq!(Config::load_at(&path).unwrap().as_value(), &original);
        change_hotkey_at(&path, "ctrl+shift+a", replace).unwrap();
        assert_eq!(*binding.borrow(), "ctrl+shift+a");
        assert_eq!(
            *changes.borrow(),
            vec![
                ("ctrl+space".into(), "ctrl+shift+a".into()),
                ("ctrl+shift+a".into(), "ctrl+space".into()),
                ("ctrl+space".into(), "ctrl+shift+a".into()),
            ]
        );
        assert_eq!(
            Config::load_at(&path).unwrap().as_value(),
            &json!({
                "hotkey": "ctrl+shift+a", "theme": "dark"
            })
        );
    }

    #[test]
    fn failed_hotkey_write_runs_the_snapshot_undo_even_when_old_was_absent() {
        for old in ["invalid", "ctrl+alt+shift+f23"] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("config.json");
            let original = json!({"hotkey": old});
            fs::write(&path, original.to_string()).unwrap();
            fs::create_dir(path.with_extension("json.tmp")).unwrap();
            let binding = std::rc::Rc::new(std::cell::RefCell::new(None::<String>));
            let error = change_hotkey_at(&path, "ctrl+alt+shift+f23", |_, new| {
                let previous = binding.borrow().clone();
                *binding.borrow_mut() = Some(new.to_string());
                let binding = binding.clone();
                Ok(Box::new(move || {
                    *binding.borrow_mut() = previous;
                    Ok(())
                }))
            })
            .unwrap_err();
            assert!(error.contains("write config tmp"));
            assert!(
                binding.borrow().is_none(),
                "rollback restores actual absence, not the old setting string"
            );
            assert_eq!(Config::load_at(&path).unwrap().as_value(), &original);
        }
    }

    #[test]
    fn a_refused_patch_neither_rebinds_nor_writes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let original = json!({"hotkey": "ctrl+space", "language": "ru"});
        fs::write(&path, original.to_string()).unwrap();
        let save = |patch: Value, check: fn(&Config) -> Result<(), String>| {
            with_locked_config(&path, |config, path| {
                persist_patch(config, path, &patch, check, |_, _| {
                    panic!("must not rebind")
                })
                .map(|saved| saved.as_value().clone())
            })
        };

        let error = save(json!({"hotkey": "ctrl+shift+a", "language": "en"}), |_| {
            Err("the loaded model is Russian-only".into())
        })
        .unwrap_err();
        assert_eq!(error, "the loaded model is Russian-only");
        assert_eq!(Config::load_at(&path).unwrap().as_value(), &original);

        // Without a hotkey in the patch the binding is not touched at all.
        let saved = save(json!({"theme": "light"}), |_| Ok(())).unwrap();
        assert_eq!(saved["theme"], "light");
        assert_eq!(saved["hotkey"], "ctrl+space");
    }

    #[test]
    fn rejected_hotkey_never_changes_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let original = json!({"hotkey": "ctrl+space", "theme": "light"});
        fs::write(&path, original.to_string()).unwrap();
        let error =
            change_hotkey_at(&path, "ctrl+shift+a", |_, _| Err("reserved".into())).unwrap_err();
        assert_eq!(error, "reserved");
        assert_eq!(Config::load_at(&path).unwrap().as_value(), &original);
    }

    #[test]
    fn corrupt_config_does_not_touch_the_hotkey_and_rollback_failure_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        fs::write(&path, "{").unwrap();
        let error =
            change_hotkey_at(&path, "ctrl+a", |_, _| panic!("must not rebind")).unwrap_err();
        assert!(error.contains("parse config"));
        fs::write(&path, "{}").unwrap();
        fs::create_dir(path.with_extension("json.tmp")).unwrap();
        let error = change_hotkey_at(&path, "ctrl+a", |_, _| {
            Ok(Box::new(|| Err("native restore failed".into())))
        })
        .unwrap_err();
        assert!(error.contains("write config tmp"));
        assert!(error.contains("native restore failed"));
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "creates a native tray icon; requires a Windows desktop session"]
    fn language_changes_preserve_one_native_tray() {
        use tauri::tray::TrayIcon;

        // No application setup, plugins or windows: this test never opens the
        // user's config, database, microphone, model cache or global hotkey.
        let mut context = tauri::generate_context!();
        context.config_mut().identifier = "com.sotto.tray-test".into();
        context.config_mut().app.windows.clear();
        let app = tauri::Builder::default()
            .any_thread()
            .build(context)
            .expect("create isolated native app");
        let handle = app.handle();
        let tray_resources = || {
            let resources = handle.resources_table();
            resources
                .names()
                .filter_map(|(id, _)| resources.get::<TrayIcon>(id).ok().map(|_| id))
                .collect::<Vec<_>>()
        };

        crate::tray::build_tray(handle).unwrap();
        let original = tray_resources();
        assert_eq!(original.len(), 1);
        for language in ["en", "ru", "en", "en", "ru"] {
            let patch = json!({ "ui_language": language });
            apply_runtime_config(handle, &patch, &patch);
            assert_eq!(
                tray_resources(),
                original,
                "changing language must retain the original native tray resource"
            );
        }

        drop(handle.remove_tray_by_id("main-tray").unwrap());
        assert!(tray_resources().is_empty());
        assert!(handle.tray_by_id("main-tray").is_none());
    }

    #[test]
    fn microphone_selection_accepts_legacy_indexes_and_names() {
        assert_eq!(microphone_selection(Some(json!(1))), Some("1".into()));
        assert_eq!(
            microphone_selection(Some(json!("name:USB Mic"))),
            Some("name:USB Mic".into())
        );
        assert_eq!(microphone_selection(Some(Value::Null)), None);
        assert_eq!(microphone_selection(Some(json!(-1))), None);
    }

    fn make_config() -> Config {
        Config { data: json!({}) }
    }

    #[test]
    fn set_then_get_returns_value() {
        let mut c = make_config();
        c.set("hotkey", json!("ctrl+shift+a")).unwrap();
        assert_eq!(c.get_string("hotkey").as_deref(), Some("ctrl+shift+a"));
    }

    #[test]
    fn get_missing_key_returns_none() {
        let c = make_config();
        assert!(c.get("hotkey").is_none());
        assert!(c.get_string("hotkey").is_none());
    }

    #[test]
    fn set_overwrites_previous_value() {
        let mut c = make_config();
        c.set("hotkey", json!("ctrl+shift+a")).unwrap();
        c.set("hotkey", json!("ctrl+shift+b")).unwrap();
        assert_eq!(c.get_string("hotkey").as_deref(), Some("ctrl+shift+b"));
    }

    #[test]
    fn set_preserves_unrelated_keys() {
        let mut c = Config {
            data: json!({"theme": "dark", "hotkey": "ctrl+space"}),
        };
        c.set("hotkey", json!("ctrl+shift+a")).unwrap();
        assert_eq!(c.get_string("theme").as_deref(), Some("dark"));
        assert_eq!(c.get_string("hotkey").as_deref(), Some("ctrl+shift+a"));
    }

    #[test]
    fn set_rejects_non_object_root() {
        // Defensive: future code might call `Config::load` on a file
        // where someone hand-wrote `"hotkey"` (no object wrapper). The
        // setter must surface the error rather than panic.
        let mut c = Config {
            data: json!("not an object"),
        };
        let result = c.set("hotkey", json!("ctrl+shift+a"));
        assert!(result.is_err());
    }

    // ------------------------------------------------------------------
    // RFC 7396 JSON Merge Patch tests
    // ------------------------------------------------------------------

    #[test]
    fn merge_patch_replaces_scalar() {
        let target = json!({ "theme": "dark", "hotkey": "ctrl+space" });
        let patch = json!({ "hotkey": "alt+tab" });
        let result = merge_json_patch(target, patch);
        assert_eq!(result, json!({ "theme": "dark", "hotkey": "alt+tab" }));
    }

    #[test]
    fn merge_patch_recurses_into_object() {
        let target = json!({
            "ai_processing": {
                "provider": "anthropic",
                "model": "claude-3-haiku",
                "timeout": 12
            }
        });
        let patch = json!({
            "ai_processing": {
                "model": "claude-3-opus",
                "temperature": 0.5
            }
        });
        let result = merge_json_patch(target, patch);
        assert_eq!(
            result,
            json!({
                "ai_processing": {
                    "provider": "anthropic",
                    "model": "claude-3-opus",
                    "timeout": 12,
                    "temperature": 0.5
                }
            })
        );
    }

    #[test]
    fn merge_patch_null_removes_key() {
        let target = json!({ "theme": "dark", "hotkey": "ctrl+space", "experimental": true });
        let patch = json!({ "experimental": null });
        let result = merge_json_patch(target, patch);
        assert_eq!(result, json!({ "theme": "dark", "hotkey": "ctrl+space" }));
    }

    #[test]
    fn merge_patch_replaces_array_whole() {
        // Per RFC 7396: arrays are replaced atomically, NOT merged
        // element-wise.
        let target = json!({ "tags": ["a", "b", "c"] });
        let patch = json!({ "tags": ["x", "y"] });
        let result = merge_json_patch(target, patch);
        assert_eq!(result, json!({ "tags": ["x", "y"] }));
    }

    #[test]
    fn merge_patch_adds_new_key() {
        let target = json!({ "theme": "dark" });
        let patch = json!({ "hotkey": "ctrl+space" });
        let result = merge_json_patch(target, patch);
        assert_eq!(result, json!({ "theme": "dark", "hotkey": "ctrl+space" }));
    }

    #[test]
    fn merge_patch_deletes_null_members_when_creating_an_object() {
        for target in [Value::Null, json!(42), json!([1, 2]), json!("old")] {
            assert_eq!(
                merge_json_patch(target, json!({ "removed": null, "kept": true })),
                json!({ "kept": true })
            );
        }
        assert_eq!(
            merge_json_patch(json!({}), json!({ "a": { "bb": { "ccc": null } } })),
            json!({ "a": { "bb": {} } })
        );
    }

    // ------------------------------------------------------------------
    // Compute device resolution
    // ------------------------------------------------------------------

    #[test]
    fn device_defaults_to_gpu_when_unset_or_unknown() {
        assert_eq!(resolve_device(&json!({})), DEVICE_GPU);
        assert_eq!(resolve_device(&json!({ "device": "wat" })), DEVICE_GPU);
        assert_eq!(resolve_device(&json!({ "device": null })), DEVICE_GPU);
        assert!(device_uses_gpu(&json!({})));
    }

    #[test]
    fn device_cpu_is_honoured() {
        assert_eq!(resolve_device(&json!({ "device": "cpu" })), DEVICE_CPU);
        assert!(!device_uses_gpu(&json!({ "device": "cpu" })));
    }

    #[test]
    fn legacy_cuda_reads_as_gpu() {
        // Configs written before the Vulkan backend say "cuda". The app has
        // never used CUDA, so this must not be mistaken for a CPU request.
        assert_eq!(resolve_device(&json!({ "device": "cuda" })), DEVICE_GPU);
        assert!(device_uses_gpu(&json!({ "device": "cuda" })));
    }

    #[test]
    fn apply_merge_patch_mutates_config_in_place() {
        let mut cfg = Config {
            data: json!({ "theme": "dark" }),
        };
        cfg.apply_merge_patch(&json!({ "hotkey": "ctrl+space" }))
            .unwrap();
        assert_eq!(cfg.get_string("hotkey").as_deref(), Some("ctrl+space"));
        assert_eq!(cfg.get_string("theme").as_deref(), Some("dark"));
    }

    // ------------------------------------------------------------------
    // Path-closed `_at` helpers (extracted so the logic is testable
    // without `tauri::AppHandle`)
    // ------------------------------------------------------------------

    #[test]
    fn load_at_missing_file_yields_empty_config() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = Config::load_at(&dir.path().join("nope.json")).unwrap();
        assert!(cfg.as_value().as_object().unwrap().is_empty());
    }

    #[test]
    fn dictionary_migration_is_read_only_until_a_successful_save() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let original = json!({"text_formatting": {"custom_words": ["Tauri"], "enabled_presets": ["development"]}}).to_string();
        fs::write(&path, &original).unwrap();
        let loaded = Config::load_at(&path).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
        assert_eq!(
            crate::custom_words_prompt(&loaded)
                .unwrap()
                .split(", ")
                .next(),
            Some("Tauri")
        );
        let saved = save_with_merge_patch_at(&path, json!({"theme": "light"})).unwrap();
        assert_eq!(
            saved["text_formatting"]["dictionary_sets"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(Config::load_at(&path).unwrap().as_value(), &saved);
        assert_eq!(saved["text_formatting"]["custom_words"], json!([]));
        assert_eq!(
            saved["text_formatting"]["enabled_presets"],
            json!(["development"])
        );
    }

    #[test]
    fn invalid_dictionary_save_leaves_disk_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let original = json!({"text_formatting": {"custom_words": ["Tauri"]}}).to_string();
        fs::write(&path, &original).unwrap();
        let result = save_with_merge_patch_at(
            &path,
            json!({"text_formatting": {"dictionary_sets": [
                {"id": "a", "name": "A", "enabled": true, "words": ["Rust", "rust"]}
            ]}}),
        );
        assert!(result.is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
    }

    #[test]
    fn failed_dictionary_write_can_be_retried_without_losing_original_words() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let original = json!({"text_formatting": {"custom_words": ["Tauri"]}}).to_string();
        fs::write(&path, &original).unwrap();
        let patch = json!({"text_formatting": {"dictionary_sets": [
            {"id": "a", "name": "Work", "enabled": true, "words": ["Tauri", "Claude Code"]}
        ]}});
        let blocked_tmp = path.with_extension("json.tmp");
        fs::create_dir(&blocked_tmp).unwrap();
        assert!(save_with_merge_patch_at(&path, patch.clone()).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
        fs::remove_dir(&blocked_tmp).unwrap();
        let saved = save_with_merge_patch_at(&path, patch).unwrap();
        assert_eq!(
            saved["text_formatting"]["dictionary_sets"][0]["words"],
            json!(["Tauri", "Claude Code"])
        );
        assert_eq!(Config::load_at(&path).unwrap().as_value(), &saved);
    }

    #[test]
    fn disabled_dictionary_sets_do_not_reach_whisper_prompt() {
        let cfg = Config {
            data: json!({"text_formatting": {"enabled": false, "dictionary_sets": [
                {"id": "a", "name": "A", "enabled": false, "words": ["Hidden"]},
                {"id": "b", "name": "B", "enabled": true, "words": ["Claude Code", "Tauri"]}
            ]}}),
        };
        assert_eq!(
            crate::custom_words_prompt(&cfg).as_deref(),
            Some("Claude Code, Tauri")
        );
    }

    #[test]
    fn repeated_loads_are_served_from_memory_until_the_file_changes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        fs::write(&path, r#"{"theme":"dark"}"#).unwrap();
        assert_eq!(
            Config::load_at(&path).unwrap().get_string("theme").unwrap(),
            "dark"
        );
        let modified = fs::metadata(&path).unwrap().modified().unwrap();
        let rewrite = |contents: &str, modified: SystemTime| {
            fs::write(&path, contents).unwrap();
            fs::File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_modified(modified)
                .unwrap();
        };

        // Same stamp: the file is not read again.
        rewrite(r#"{"theme":"lite"}"#, modified);
        assert_eq!(
            Config::load_at(&path).unwrap().get_string("theme").unwrap(),
            "dark"
        );

        // A hand edit moves the stamp and is picked up.
        let edited = modified + std::time::Duration::from_secs(2);
        rewrite(r#"{"theme":"lite"}"#, edited);
        assert_eq!(
            Config::load_at(&path).unwrap().get_string("theme").unwrap(),
            "lite"
        );

        // A save updates memory along with the file.
        save_with_merge_patch_at(&path, json!({"theme": "light"})).unwrap();
        let saved = fs::metadata(&path).unwrap().modified().unwrap();
        rewrite(
            &fs::read_to_string(&path).unwrap().replace("light", "LIGHT"),
            saved,
        );
        assert_eq!(
            Config::load_at(&path).unwrap().get_string("theme").unwrap(),
            "light"
        );
    }

    #[test]
    fn load_at_rejects_broken_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(&path, "{ not json").unwrap();
        assert!(Config::load_at(&path).is_err());
    }

    #[test]
    fn save_at_writes_readable_json_without_tmp_leftover() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let cfg = Config {
            data: json!({ "hotkey": "ctrl+shift+a", "theme": "dark" }),
        };
        cfg.save_at(&path).unwrap();

        // Round-trips through the same parser the app uses.
        let reloaded = Config::load_at(&path).unwrap();
        assert_eq!(reloaded.as_value(), cfg.as_value());
        // The atomic rename must not leave the sibling tmp behind.
        assert!(!dir.path().join("config.json.tmp").exists());
    }

    #[test]
    fn hotkey_from_returns_configured_value_or_default() {
        let cfg = Config {
            data: json!({ "hotkey": "alt+space" }),
        };
        assert_eq!(hotkey_from(&cfg), "alt+space");
        assert_eq!(hotkey_from(&Config { data: json!({}) }), DEFAULT_HOTKEY);
    }

    #[test]
    fn migrate_legacy_device_rewrites_cuda_to_gpu_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(&path, json!({ "device": "cuda" }).to_string()).unwrap();

        assert_eq!(migrate_legacy_device_at(&path), Ok(true));
        // The migration must actually reach the disk, not just flip a flag.
        let on_disk = Config::load_at(&path).unwrap();
        assert_eq!(on_disk.get_string("device").as_deref(), Some(DEVICE_GPU));
    }

    #[test]
    fn migrate_legacy_device_drops_compute_type() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(
            &path,
            json!({ "device": "cpu", "compute_type": "int8" }).to_string(),
        )
        .unwrap();

        assert_eq!(migrate_legacy_device_at(&path), Ok(true));
        let on_disk = Config::load_at(&path).unwrap();
        assert_eq!(on_disk.get_string("device").as_deref(), Some(DEVICE_CPU));
        assert!(on_disk.get("compute_type").is_none());
    }

    #[test]
    fn migrate_legacy_device_touches_nothing_when_already_clean() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        // A config that needs no migration: the file must not be written,
        // and the call must report "nothing changed".
        let before = json!({ "device": "cpu" }).to_string();
        std::fs::write(&path, &before).unwrap();

        assert_eq!(migrate_legacy_device_at(&path), Ok(false));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
    }

    #[test]
    fn save_with_merge_patch_at_applies_and_returns_disk_value() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(&path, json!({ "theme": "dark" }).to_string()).unwrap();

        let returned = save_with_merge_patch_at(&path, json!({ "hotkey": "ctrl+x" })).unwrap();
        assert_eq!(returned["hotkey"], json!("ctrl+x"));
        assert_eq!(returned["theme"], json!("dark"));

        // The returned value must match what actually landed on disk.
        let on_disk = Config::load_at(&path).unwrap();
        assert_eq!(on_disk.as_value(), &returned);
    }

    // GigaAM is in the Sherpa catalog on both supported desktop platforms.
    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn validate_refuses_a_russian_only_model_in_another_language() {
        let bad = json!({ "model": "gigaam-v3", "language": "en" });
        assert!(validate(&bad).is_err());
    }

    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn validate_allows_the_languages_a_russian_only_model_can_serve() {
        for language in ["ru", "auto"] {
            let candidate = json!({ "model": "gigaam-v3", "language": language });
            assert!(validate(&candidate).is_ok(), "rejected {language}");
        }
        // A missing language is the Russian default, not a violation.
        assert!(validate(&json!({ "model": "gigaam-v3" })).is_ok());
    }

    #[test]
    fn validate_ignores_models_that_are_not_language_locked() {
        assert!(validate(&json!({ "model": "large-v3", "language": "en" })).is_ok());
        assert!(validate(&json!({ "language": "en" })).is_ok());
    }

    /// The seam exists so that *every* writer is covered, not just the
    /// settings command. A refused patch must also leave the previous config
    /// intact rather than half-applying it.
    #[test]
    fn merge_patch_refuses_an_invalid_pair_and_leaves_the_file_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let before = json!({ "model": "small.en", "language": "en", "theme": "dark" });
        std::fs::write(&path, before.to_string()).unwrap();

        let refused = save_with_merge_patch_at(&path, json!({ "language": "ru" }));

        assert!(refused.is_err());
        assert_eq!(Config::load_at(&path).unwrap().as_value(), &before);
    }

    #[test]
    fn recording_limit_defaults_caps_and_turns_off() {
        assert_eq!(
            recording_limit_minutes(&json!({})),
            DEFAULT_RECORDING_LIMIT_MINUTES
        );
        assert_eq!(
            recording_limit_minutes(&json!({ RECORDING_LIMIT_KEY: 0 })),
            0
        );
        assert_eq!(
            recording_limit_minutes(&json!({ RECORDING_LIMIT_KEY: 30 })),
            30
        );
        assert_eq!(
            recording_limit_minutes(&json!({ RECORDING_LIMIT_KEY: 100_000 })),
            24 * 60
        );
        assert_eq!(
            recording_limit_minutes(&json!({ RECORDING_LIMIT_KEY: "5" })),
            DEFAULT_RECORDING_LIMIT_MINUTES
        );
    }

    /// No key still means unloading is on: otherwise the update would quietly
    /// leave everyone already using the app without it.
    #[test]
    fn a_config_without_the_key_still_unloads_after_five_minutes() {
        assert_eq!(
            model_unload_after_minutes(&json!({})),
            DEFAULT_MODEL_UNLOAD_MINUTES
        );
    }

    #[test]
    fn zero_minutes_means_never_unload() {
        assert_eq!(
            model_unload_after_minutes(&json!({ MODEL_UNLOAD_KEY: 0 })),
            0
        );
    }

    /// A value outside the UI list is still a value: configs are edited by hand.
    #[test]
    fn a_hand_written_interval_is_taken_as_written() {
        assert_eq!(
            model_unload_after_minutes(&json!({ MODEL_UNLOAD_KEY: 2 })),
            2
        );
    }

    /// Garbage and negative numbers do not disable unloading but roll it back to
    /// the default: "we could not read it" is not "you asked for never".
    #[test]
    fn unreadable_values_fall_back_to_the_default() {
        for value in [json!("пять"), json!(-5), json!(null), json!(5.5)] {
            assert_eq!(
                model_unload_after_minutes(&json!({ MODEL_UNLOAD_KEY: value })),
                DEFAULT_MODEL_UNLOAD_MINUTES
            );
        }
    }

    #[test]
    fn an_absurd_interval_is_capped_at_a_day() {
        assert_eq!(
            model_unload_after_minutes(&json!({ MODEL_UNLOAD_KEY: 100_000 })),
            MAX_MODEL_UNLOAD_MINUTES
        );
    }
    #[test]
    fn retired_overlay_palettes_migrate_without_blocking_unrelated_saves() {
        for palette in ["accent", "coal", "amber"] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("config.json");
            let original = json!({"overlay": {"palette": palette, "form": "bead", "size": "l"}});
            std::fs::write(&path, original.to_string()).unwrap();
            let before = std::fs::read(&path).unwrap();

            let loaded = Config::load_at(&path).unwrap();
            assert_eq!(loaded.as_value()["overlay"]["palette"], "copper");
            assert_eq!(std::fs::read(&path).unwrap(), before);
            assert!(save_with_merge_patch_at(&path, json!({"ui_accent": "invalid"})).is_err());
            assert_eq!(std::fs::read(&path).unwrap(), before);

            let saved = save_with_merge_patch_at(&path, json!({"theme": "light"})).unwrap();
            assert_eq!(saved["theme"], "light");
            assert_eq!(
                saved["overlay"],
                json!({"palette": "copper", "form": "bead", "size": "l"})
            );
            assert_eq!(Config::load_at(&path).unwrap().as_value(), &saved);
        }
    }

    #[test]
    fn overlay_patch_preserves_siblings_and_invalid_writes_leave_disk_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        save_with_merge_patch_at(
            &path,
            json!({"overlay":{"form":"bead","size":"l"}, "ui_accent":"#5b8def"}),
        )
        .unwrap();
        let next = save_with_merge_patch_at(&path, json!({"overlay":{"edge_offset":0}})).unwrap();
        assert_eq!(next["overlay"]["form"], "bead");
        assert_eq!(next["overlay"]["size"], "l");
        let before = std::fs::read(&path).unwrap();
        for patch in [
            json!({"overlay":{"size":"xl"}}),
            json!({"ui_accent":"crimson"}),
        ] {
            assert!(save_with_merge_patch_at(&path, patch).is_err());
            assert_eq!(std::fs::read(&path).unwrap(), before);
        }
        let reset = save_with_merge_patch_at(&path, json!({"overlay":{"size":null}})).unwrap();
        assert!(reset["overlay"].get("size").is_none());
        assert_eq!(
            crate::overlay_preferences::OverlayPreferences::from_config(&reset).size,
            "m"
        );
    }
}
