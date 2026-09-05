//! Config helpers. Originally just `load_hotkey`; extended in Task 13 with
//! a full `Config` struct so `set_hotkey` can persist via `config::save`
//! directly instead of round-tripping through the Python sidecar's
//! `save_config` RPC.

use serde_json::{Map, Value};
use std::fs;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

const DEFAULT_HOTKEY: &str = "ctrl+shift+space";

/// Value of the `device` config key meaning "run inference on the GPU".
pub const DEVICE_GPU: &str = "gpu";
/// Value of the `device` config key meaning "run inference on the CPU".
pub const DEVICE_CPU: &str = "cpu";

/// Load the saved hotkey from `config.json` in the app config dir.
/// Returns the default if the file is missing or unreadable.
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

// ---------------------------------------------------------------------------
// `Config` struct — used by `set_hotkey` to persist settings without the
// Python sidecar (WS 4a1, Task 13). Single-writer: the `set_hotkey` Tauri
// command. No locking required for v1 because the only writer runs from
// the main thread's IPC dispatcher; concurrent `get_config` reads go
// through a clone of the in-memory JSON. If a future command needs to
// mutate config from another thread, swap `Mutex<Value>` in here.
// ---------------------------------------------------------------------------

/// In-memory mirror of `config.json`. Cheap to clone (the `Value` inside
/// is reference-counted via `serde_json::Value::Map` internals for the
/// common small-config case; for very large configs, clone is still
/// cheap relative to the on-disk read).
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
        if !path.exists() {
            return Ok(Self {
                data: Value::Object(Map::new()),
            });
        }
        let raw = fs::read_to_string(path).map_err(|e| format!("read config.json: {e}"))?;
        let data: Value =
            serde_json::from_str(&raw).map_err(|e| format!("parse config.json: {e}"))?;
        Ok(Self { data })
    }

    /// Read-only accessor for a single key.
    pub fn get(&self, key: &str) -> Option<Value> {
        self.data.get(key).cloned()
    }

    /// Set a single key. Mutates in memory; persist with `save`.
    pub fn set(&mut self, key: &str, value: Value) -> Result<(), String> {
        let map = self
            .data
            .as_object_mut()
            .ok_or_else(|| "config root is not a JSON object".to_string())?;
        map.insert(key.to_string(), value);
        Ok(())
    }

    /// Convenience setter for keys that should default to `String` when
    /// present (returns `None` if the key is absent or not a string).
    pub fn get_string(&self, key: &str) -> Option<String> {
        self.get(key)
            .and_then(|v| v.as_str().map(|s| s.to_string()))
    }

    /// Borrow the underlying `serde_json::Value`. Used by callers that
    /// need to walk the on-disk JSON tree (e.g. `build_cloud_stt_request`
    /// which composes a request from multiple fields under
    /// `ai_processing`).
    pub fn as_value(&self) -> &Value {
        &self.data
    }

    /// Write the in-memory config back to `config.json`. Pretty-printed
    /// for human readability. Atomic-ish: writes to a sibling tmp file
    /// then renames, so a crash mid-write leaves the previous config
    /// intact.
    pub fn save(&self, app: &AppHandle) -> Result<(), String> {
        validate(&self.data)?;
        self.save_at(&config_path(app)?)
    }

    /// Write the in-memory config to an explicit path. See [`Self::save`]
    /// for the atomic-ish semantics.
    fn save_at(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("create config dir: {e}"))?;
        }
        let pretty = serde_json::to_string_pretty(&self.data)
            .map_err(|e| format!("serialize config: {e}"))?;
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, &pretty).map_err(|e| format!("write config tmp: {e}"))?;
        fs::rename(&tmp, path).map_err(|e| format!("rename config tmp: {e}"))?;
        Ok(())
    }

    /// Apply an RFC 7396 JSON Merge Patch to the in-memory config.
    /// The patch is merged into `self.data` (objects recurse, null
    /// removes keys, scalars/arrays replace atomically). Call `save`
    /// afterwards to persist.
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
/// - If both are objects, recurse for each key: null in patch → delete;
///   otherwise merge recursively.
/// - Otherwise, `patch` replaces `target` (arrays replace atomically;
///   they are NOT merged element-wise).
pub fn merge_json_patch(target: Value, patch: Value) -> Value {
    match (target, patch) {
        (Value::Object(mut t), Value::Object(p)) => {
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

/// Ключ: через сколько минут простоя выгружать модель из оперативной памяти.
pub const MODEL_UNLOAD_KEY: &str = "model_unload_after_minutes";

/// Сколько минут простоя ждут, если в конфиге ничего не сказано.
///
/// Модель большого размера держит в памяти несколько гигабайт, и между
/// диктовками они не нужны никому. Пять минут — компромисс: диктуют
/// очередями, и внутри очереди повторная загрузка обошлась бы дороже
/// освобождённой памяти.
pub const DEFAULT_MODEL_UNLOAD_MINUTES: u64 = 5;

/// Значения, которые предлагает интерфейс. Конфиг правят и руками, поэтому
/// [`model_unload_after_minutes`] принимает любое число из диапазона, а не
/// только эти четыре.
pub const MODEL_UNLOAD_CHOICES: [u64; 4] = [0, 5, 10, 30];

/// Сутки простоя — это уже «никогда», просто записанное числом. Верхняя
/// граница нужна не пользователю, а таймеру: `Duration` из тысяч минут
/// ничем не лучше отключённой выгрузки, а выглядит как работающая настройка.
const MAX_MODEL_UNLOAD_MINUTES: u64 = 24 * 60;

/// Через сколько минут простоя выгружать модель. `0` — не выгружать.
///
/// Отсутствие ключа — не «никогда», а значение по умолчанию: выгрузка
/// включена, и старые конфиги получают её вместе с обновлением.
pub fn model_unload_after_minutes(config: &Value) -> u64 {
    let minutes = config
        .get(MODEL_UNLOAD_KEY)
        .and_then(Value::as_u64)
        .unwrap_or(DEFAULT_MODEL_UNLOAD_MINUTES);
    minutes.min(MAX_MODEL_UNLOAD_MINUTES)
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
    let mut cfg = Config::load_at(path)?;
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
/// command is not the only writer: `Config::set` + [`Config::save`] reaches
/// the same file, and a validator that only guards one door guards nothing.
///
/// Only config-only rules belong here. A rule that needs runtime state — what
/// the engine currently has loaded, what a device reports — cannot be decided
/// from a `Value` and stays with the caller that owns that state.
///
/// Deliberately not called from [`Config::save_at`]: `migrate_legacy_device`
/// writes through it to *repair* an old config, and a repair must not be
/// blocked by the very invariant it may be fixing.
pub fn validate(candidate: &Value) -> Result<(), String> {
    validate_speech_route(candidate)
}

/// GigaAM v3 only knows Russian. Pairing it with another language does not
/// fail loudly — it mis-decodes every dictation — so it is refused at the
/// point the pair is written.
fn validate_speech_route(candidate: &Value) -> Result<(), String> {
    let Some(model) = candidate.get("model").and_then(Value::as_str) else {
        return Ok(());
    };
    let Some(languages) = crate::model::model_languages(model) else {
        return Ok(());
    };
    // Язык не записан вместе с моделью — берём первый из её списка: именно
    // на него настройки переключатся сами.
    let language = candidate
        .get("language")
        .and_then(Value::as_str)
        .unwrap_or_else(|| languages.first().copied().unwrap_or("auto"));
    if crate::model::model_supports_language(model, language) {
        return Ok(());
    }
    Err(crate::model::language_unsupported_message(languages))
}

/// Apply a JSON Merge Patch to the on-disk config and return
/// the new value. Used by the `save_config` Tauri command.
pub fn save_with_merge_patch(app: &AppHandle, patch: Value) -> Result<Value, String> {
    save_with_merge_patch_at(&config_path(app)?, patch)
}

/// Path-closed variant of [`save_with_merge_patch`]: loads, patches, saves,
/// and returns the value now on disk. Rejects before touching the disk, so a
/// refused patch leaves the previous config intact.
fn save_with_merge_patch_at(path: &Path, patch: Value) -> Result<Value, String> {
    let mut cfg = Config::load_at(path)?;
    cfg.apply_merge_patch(&patch)?;
    validate(cfg.as_value())?;
    cfg.save_at(path)?;
    Ok(cfg.as_value().clone())
}

// ---------------------------------------------------------------------------
// Tests — exercise the in-memory Config + atomic-save guarantees without
// touching `tauri::AppHandle`. We mock `config_path` via a free function
// to keep this dependency-free; production code uses Tauri's
// `app_config_dir()`.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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

    // Единственное правило `validate` — про GigaAM, а GigaAM намеренно
    // Windows-only (см. sherpa-rs в Cargo.toml). Вне Windows «gigaam-v3» —
    // неизвестная модель, `validate_speech_route` выходит на первой же
    // проверке, и тесты либо падают, либо зеленеют вхолостую. Отсюда
    // `#[cfg(windows)]` здесь и у двух тестов ниже.
    #[cfg(windows)]
    #[test]
    fn validate_refuses_a_russian_only_model_in_another_language() {
        let bad = json!({ "model": "gigaam-v3", "language": "en" });
        assert!(validate(&bad).is_err());
    }

    #[cfg(windows)]
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
    ///
    /// Windows-only не по существу шва, а потому, что отвергнуть нечего:
    /// единственная пара, которую `validate` бракует, — это GigaAM с чужим
    /// языком. Появится правило без привязки к платформе — снять `cfg`.
    #[cfg(windows)]
    #[test]
    fn merge_patch_refuses_an_invalid_pair_and_leaves_the_file_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let before = json!({ "model": "gigaam-v3", "language": "ru", "theme": "dark" });
        std::fs::write(&path, before.to_string()).unwrap();

        let refused = save_with_merge_patch_at(&path, json!({ "language": "en" }));

        assert!(refused.is_err());
        assert_eq!(Config::load_at(&path).unwrap().as_value(), &before);
    }

    /// Ключа нет — выгрузка всё равно включена: иначе обновление тихо
    /// оставило бы всех, кто уже пользуется приложением, без неё.
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

    /// Значение не из списка интерфейса — тоже значение: конфиг правят руками.
    #[test]
    fn a_hand_written_interval_is_taken_as_written() {
        assert_eq!(
            model_unload_after_minutes(&json!({ MODEL_UNLOAD_KEY: 2 })),
            2
        );
    }

    /// Мусор и отрицательные числа не выключают выгрузку, а откатывают её к
    /// умолчанию: «не смогли прочитать» — это не «просили никогда».
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
}
