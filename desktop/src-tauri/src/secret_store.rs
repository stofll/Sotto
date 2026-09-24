//! Native cross-platform storage for AI provider API keys.
//!
//! Persists AI provider API keys in the platform's secure storage:
//!
//! - **macOS**   → System Keychain via the `keyring` crate
//!   (`security-framework` backend).
//! - **Windows** → Credential Manager via the `keyring` crate
//!   (`wincred` / `windows-native` backend).
//! - **Linux**   → Secret Service (libsecret / GNOME Keyring / KWallet)
//!   via the `keyring` crate's D-Bus backend.
//!
//! - `service` = `"sotto"`
//! - `username` = key slot ref (legacy slots used the provider id)
//! - `password` = the raw API key
//!
//! Builds up to 0.1.3 stored keys under the pre-rename service
//! `"speech-to-text"`. A key found only there is moved to the current service
//! the first time it is read, so no one has to enter their keys again.
//! stofll/Sotto#40 tracks removing the move.
//!
//! Labels are not persisted by this module; save commands echo the label
//! supplied by the caller, while metadata reads return an empty label.

const SERVICE: &str = "sotto";
const LEGACY_SERVICE: &str = "speech-to-text";

/// Characters the long mask keeps from each end of the key, and the prefix the
/// short mask keeps. Each is revealed only by a key at least twice its length.
const HEAD: usize = 6;
const TAIL: usize = 4;
const SHORT_HEAD: usize = 2;

/// Held for the whole of every save, read and delete. Moving a legacy key
/// writes a value read a step earlier, so a save or delete landing in between
/// would be overwritten or undone.
static STORE: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// The operations this module needs from a credential store, so the move from
/// the legacy service can be tested without touching the real one.
trait Vault {
    fn get(&self, service: &str, slot: &str) -> Result<Option<String>, String>;
    fn set(&self, service: &str, slot: &str, secret: &str) -> Result<(), String>;
    /// `Ok(false)` when there was nothing to delete.
    fn delete(&self, service: &str, slot: &str) -> Result<bool, String>;
}

/// The platform credential store.
struct Keyring;

fn entry(service: &str, slot: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(service, slot).map_err(|error| format!("SECRET_STORE_INIT: {error}"))
}

impl Vault for Keyring {
    fn get(&self, service: &str, slot: &str) -> Result<Option<String>, String> {
        match entry(service, slot)?.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(format!("SECRET_STORE_GET: {error}")),
        }
    }

    fn set(&self, service: &str, slot: &str, secret: &str) -> Result<(), String> {
        entry(service, slot)?
            .set_password(secret)
            .map_err(|error| format!("SECRET_STORE_SAVE: {error}"))
    }

    fn delete(&self, service: &str, slot: &str) -> Result<bool, String> {
        match entry(service, slot)?.delete_credential() {
            Ok(()) => Ok(true),
            Err(keyring::Error::NoEntry) => Ok(false),
            Err(error) => Err(format!("SECRET_STORE_DELETE: {error}")),
        }
    }
}

fn mask_key(key: &str) -> String {
    if key.is_empty() {
        return String::new();
    }
    let chars: Vec<char> = key.chars().collect();
    let len = chars.len();
    // Never reveal more than half the key. The long mask exposes ten
    // characters, so it needs twenty to earn them: at eleven it printed ten of
    // the eleven, which hides a secret only in the sense that one character is
    // missing. Every provider's key is far longer than this threshold.
    if len < 2 * (HEAD + TAIL) {
        // Two characters need four to earn them, by the same half rule.
        if len >= 2 * SHORT_HEAD {
            let prefix: String = chars[..SHORT_HEAD].iter().collect();
            return format!("{prefix}…");
        }
        return "…".to_string();
    }
    let head: String = chars[..HEAD].iter().collect();
    let tail: String = chars[len - TAIL..].iter().collect();
    format!("{head}…{tail}")
}

pub fn save_key(provider: &str, key: &str) -> Result<bool, String> {
    save_key_in(&Keyring, provider, key)
}

fn save_key_in(vault: &impl Vault, slot: &str, key: &str) -> Result<bool, String> {
    if key.trim().is_empty() {
        return Err("SECRET_STORE_EMPTY_KEY: refusing to save an empty key".to_string());
    }
    let _store = crate::mutex_recover::lock(&STORE);
    vault.set(SERVICE, slot, key)?;
    // A replaced key must not survive under the old name.
    if let Err(error) = vault.delete(LEGACY_SERVICE, slot) {
        log::warn!("legacy API key {slot} not removed: {error}");
    }
    Ok(true)
}

pub fn get_key(provider: &str) -> Result<Option<String>, String> {
    get_key_in(&Keyring, provider)
}

fn get_key_in(vault: &impl Vault, slot: &str) -> Result<Option<String>, String> {
    let _store = crate::mutex_recover::lock(&STORE);
    if let Some(key) = vault.get(SERVICE, slot)? {
        return Ok(Some(key));
    }
    let Some(key) = vault.get(LEGACY_SERVICE, slot)? else {
        return Ok(None);
    };
    // The legacy entry goes only once the copy reads back: a failed move keeps
    // the key readable from the old service and is retried on the next read.
    let copied = vault
        .set(SERVICE, slot, &key)
        .and_then(|()| vault.get(SERVICE, slot));
    match copied {
        Ok(Some(copy)) if copy == key => {
            if let Err(error) = vault.delete(LEGACY_SERVICE, slot) {
                log::warn!("legacy API key {slot} not removed: {error}");
            }
        }
        Ok(_) => log::warn!("API key {slot} did not read back after the move"),
        Err(error) => log::warn!("API key {slot} not moved: {error}"),
    }
    Ok(Some(key))
}

pub fn get_key_meta(provider: &str) -> Result<Option<KeyMeta>, String> {
    let Some(raw) = get_key(provider)? else {
        return Ok(None);
    };
    Ok(Some(key_meta_from(&raw)))
}

/// Compose the `KeyMeta` snapshot for a stored raw key. Extracted from
/// `get_key_meta` so the composition is testable without touching the
/// platform credential store (`keyring`).
fn key_meta_from(raw: &str) -> KeyMeta {
    KeyMeta {
        available: true,
        // This module persists only the secret, not a label attribute.
        label: String::new(),
        masked: mask_key(raw),
    }
}

pub fn delete_key(provider: &str) -> Result<bool, String> {
    delete_key_in(&Keyring, provider)
}

fn delete_key_in(vault: &impl Vault, slot: &str) -> Result<bool, String> {
    let _store = crate::mutex_recover::lock(&STORE);
    let current = vault.delete(SERVICE, slot)?;
    let legacy = vault.delete(LEGACY_SERVICE, slot)?;
    Ok(current || legacy)
}

/// Delete every API key Sotto stored, under either service name. Run by the
/// uninstaller's data removal; returns the failures, if any.
///
/// Slot names live only in the configuration, which may be gone or stale, so
/// the credentials are enumerated instead. `keyring` names a Windows credential
/// `{slot}.{service}` and marks it with a `keyring v…` comment; both must match.
#[cfg(windows)]
pub fn purge() -> Vec<String> {
    purge_services(&[SERVICE, LEGACY_SERVICE])
}

#[cfg(windows)]
fn purge_services(services: &[&str]) -> Vec<String> {
    let credentials = match windows_credentials() {
        Ok(credentials) => credentials,
        Err(error) => return vec![error],
    };
    credentials
        .iter()
        .filter_map(|(target, user, comment)| owned_credential(services, target, user, comment))
        .filter_map(|(service, slot)| Keyring.delete(service, &slot).err())
        .collect()
}

#[cfg(any(windows, test))]
fn owned_credential<'a>(
    services: &[&'a str],
    target: &str,
    user: &str,
    comment: &str,
) -> Option<(&'a str, String)> {
    if !comment.starts_with("keyring v") {
        return None;
    }
    services
        .iter()
        .find(|service| target == format!("{user}.{service}"))
        .map(|service| (*service, user.to_string()))
}

/// Generic credentials of the current user as `(target, user, comment)`.
#[cfg(windows)]
fn windows_credentials() -> Result<Vec<(String, String, String)>, String> {
    use windows_sys::Win32::Security::Credentials::{
        CredEnumerateW, CredFree, CREDENTIALW, CRED_TYPE_GENERIC,
    };

    /// ERROR_NOT_FOUND: the user has no credentials at all.
    const NOT_FOUND: i32 = 1168;

    fn text(value: *const u16) -> String {
        if value.is_null() {
            return String::new();
        }
        // SAFETY: the API returns NUL-terminated strings that stay valid until
        // `CredFree`; the scan stops at the terminator.
        unsafe {
            let len = (0..).take_while(|&i| *value.add(i) != 0).count();
            String::from_utf16_lossy(std::slice::from_raw_parts(value, len))
        }
    }

    let mut count = 0u32;
    let mut list: *mut *mut CREDENTIALW = std::ptr::null_mut();
    // SAFETY: both out-pointers are valid; a null filter with no flags lists
    // the current user's credentials.
    if unsafe { CredEnumerateW(std::ptr::null(), 0, &mut count, &mut list) } == 0 {
        let error = std::io::Error::last_os_error();
        return if error.raw_os_error() == Some(NOT_FOUND) {
            Ok(Vec::new())
        } else {
            Err(format!("SECRET_STORE_ENUMERATE: {error}"))
        };
    }
    // SAFETY: on success `list` holds `count` valid credential pointers. They
    // are copied out here and the block is released once, below.
    let credentials = unsafe { std::slice::from_raw_parts(list, count as usize) }
        .iter()
        .map(|&credential| unsafe { &*credential })
        .filter(|credential| credential.Type == CRED_TYPE_GENERIC)
        .map(|credential| {
            (
                text(credential.TargetName),
                text(credential.UserName),
                text(credential.Comment),
            )
        })
        .collect();
    // SAFETY: `list` came from `CredEnumerateW` and is not used afterwards.
    unsafe { CredFree(list.cast()) };
    Ok(credentials)
}

/// Run a credential-store call on a blocking worker. The platform store is
/// synchronous and Keychain can wait on the user, so neither the main thread
/// (where synchronous commands run) nor an async runtime worker may make it.
async fn off_thread<T: Send + 'static>(
    call: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(call)
        .await
        .map_err(|error| format!("SECRET_STORE_WORKER: {error}"))?
}

/// [`get_key`] for async code.
pub async fn load_key(slot: &str) -> Result<Option<String>, String> {
    let slot = slot.to_owned();
    off_thread(move || get_key(&slot)).await
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct KeyMeta {
    pub available: bool,
    pub label: String,
    pub masked: String,
}

/// Save an API key into the platform secret store.
///
/// The frontend (API-keys / providers pages) invokes this with the
/// slot ref (`key_id`) as the storage username, matching how the AI
/// pipeline later resolves keys via `secret_store::get_key(api_key_ref)`.
///
/// `rename_all = "snake_case"` because the frontend sends snake_case
/// argument names (`key_id`) — Tauri's default is camelCase.
///
/// Returns `{ saved, label, masked }`. Labels are not portably stored in
/// the OS credential store, so the caller-supplied `label` is echoed back
/// for the in-memory UI state (it does not survive a restart).
#[tauri::command(rename_all = "snake_case")]
pub(crate) async fn save_api_key(
    key_id: String,
    key: String,
    label: Option<String>,
) -> Result<serde_json::Value, String> {
    let masked = off_thread(move || {
        save_key(&key_id, &key)?;
        Ok(get_key_meta(&key_id)?
            .map(|meta| meta.masked)
            .unwrap_or_default())
    })
    .await?;
    Ok(serde_json::json!({
        "saved": true,
        "label": label.unwrap_or_default(),
        "masked": masked,
    }))
}

/// Report whether a key exists for the given slot ref, with its mask.
/// Called at boot for every known slot and after edits.
#[tauri::command(rename_all = "snake_case")]
pub(crate) async fn has_api_key(key_id: String) -> Result<serde_json::Value, String> {
    match off_thread(move || get_key_meta(&key_id)).await? {
        Some(meta) => Ok(serde_json::json!({
            "available": meta.available,
            "label": meta.label,
            "masked": meta.masked,
        })),
        None => Ok(serde_json::json!({ "available": false, "label": "", "masked": "" })),
    }
}

/// Delete a stored API key. Returns `{ deleted }` (false if there was
/// no key in that slot — not an error).
#[tauri::command(rename_all = "snake_case")]
pub(crate) async fn delete_api_key(key_id: String) -> Result<serde_json::Value, String> {
    let deleted = off_thread(move || delete_key(&key_id)).await?;
    Ok(serde_json::json!({ "deleted": deleted }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mask_key_short_strings() {
        assert_eq!(mask_key(""), "");
        assert_eq!(mask_key("ab"), "…");
        assert_eq!(mask_key("abc"), "…");
        assert_eq!(mask_key("abcd"), "ab…");
        assert_eq!(mask_key("abcdefgh"), "ab…");
        assert_eq!(mask_key("abcdefghi"), "ab…");
        assert_eq!(mask_key("abcdefghij"), "ab…");
        // Eleven characters used to print ten of them.
        assert_eq!(mask_key("abcdefghijk"), "ab…");
        assert_eq!(mask_key("abcdefghijklmnopqrs"), "ab…");
        assert_eq!(mask_key("abcdefghijklmnopqrst"), "abcdef…qrst");
        assert_eq!(mask_key("a"), "…");
    }

    /// The point of the mask is what it withholds, so that is what is asserted:
    /// no key may show more than half of itself, at any length.
    #[test]
    fn mask_key_never_reveals_more_than_half_of_any_key() {
        for len in 1..64 {
            let key: String = ('a'..='z').cycle().take(len).collect();
            let revealed = mask_key(&key).chars().filter(|c| *c != '…').count();
            assert!(
                revealed * 2 <= len,
                "{len}-character key revealed {revealed} characters"
            );
        }
    }

    #[test]
    fn mask_key_unicode() {
        assert_eq!(mask_key("абвгдежзи"), "аб…");
        assert_eq!(mask_key("абвгдежзий"), "аб…");
        assert_eq!(mask_key("абвгдежзийк"), "аб…");
        assert_eq!(mask_key("абвгдежзийклмнопрсту"), "абвгде…рсту");
        assert_eq!(mask_key("🔑🔒🔐"), "…");
        assert_eq!(mask_key("🔑🔒🔐🗝"), "🔑🔒…");
    }

    /// An in-memory credential store; `failing_set` makes every write fail.
    #[derive(Default)]
    struct MemoryVault {
        entries: std::sync::Mutex<std::collections::HashMap<(String, String), String>>,
        failing_set: bool,
    }

    impl MemoryVault {
        fn with(service: &str, slot: &str, secret: &str) -> Self {
            let vault = Self::default();
            vault.put(service, slot, secret);
            vault
        }

        fn put(&self, service: &str, slot: &str, secret: &str) {
            self.entries
                .lock()
                .unwrap()
                .insert((service.into(), slot.into()), secret.into());
        }

        fn has(&self, service: &str, slot: &str) -> bool {
            self.entries
                .lock()
                .unwrap()
                .contains_key(&(service.into(), slot.into()))
        }
    }

    impl Vault for MemoryVault {
        fn get(&self, service: &str, slot: &str) -> Result<Option<String>, String> {
            Ok(self
                .entries
                .lock()
                .unwrap()
                .get(&(service.into(), slot.into()))
                .cloned())
        }

        fn set(&self, service: &str, slot: &str, secret: &str) -> Result<(), String> {
            if self.failing_set {
                return Err("store is read-only".into());
            }
            self.put(service, slot, secret);
            Ok(())
        }

        fn delete(&self, service: &str, slot: &str) -> Result<bool, String> {
            Ok(self
                .entries
                .lock()
                .unwrap()
                .remove(&(service.into(), slot.into()))
                .is_some())
        }
    }

    #[test]
    fn a_legacy_key_moves_to_the_current_service_on_first_read() {
        let vault = MemoryVault::with(LEGACY_SERVICE, "openai", "sk-legacy");

        assert_eq!(
            get_key_in(&vault, "openai").unwrap().as_deref(),
            Some("sk-legacy")
        );

        assert!(vault.has(SERVICE, "openai"));
        assert!(!vault.has(LEGACY_SERVICE, "openai"));
        assert_eq!(
            get_key_in(&vault, "openai").unwrap().as_deref(),
            Some("sk-legacy")
        );
    }

    #[test]
    fn a_failed_move_keeps_the_legacy_key_readable() {
        let vault = MemoryVault {
            failing_set: true,
            ..MemoryVault::with(LEGACY_SERVICE, "openai", "sk-legacy")
        };

        assert_eq!(
            get_key_in(&vault, "openai").unwrap().as_deref(),
            Some("sk-legacy")
        );
        assert!(vault.has(LEGACY_SERVICE, "openai"));
    }

    #[test]
    fn the_current_key_wins_over_a_stale_legacy_one() {
        let vault = MemoryVault::with(SERVICE, "openai", "sk-current");
        vault.put(LEGACY_SERVICE, "openai", "sk-stale");

        assert_eq!(
            get_key_in(&vault, "openai").unwrap().as_deref(),
            Some("sk-current")
        );
    }

    #[test]
    fn saving_and_deleting_cover_both_service_names() {
        let vault = MemoryVault::with(LEGACY_SERVICE, "openai", "sk-old");
        save_key_in(&vault, "openai", "sk-new").unwrap();
        assert!(!vault.has(LEGACY_SERVICE, "openai"));
        assert_eq!(
            get_key_in(&vault, "openai").unwrap().as_deref(),
            Some("sk-new")
        );

        vault.put(LEGACY_SERVICE, "openai", "sk-old");
        assert!(delete_key_in(&vault, "openai").unwrap());
        assert!(!vault.has(SERVICE, "openai"));
        assert!(!vault.has(LEGACY_SERVICE, "openai"));
        assert!(!delete_key_in(&vault, "openai").unwrap());
    }

    /// Pauses a read right after it finds the legacy key, the step before the
    /// move writes it, and runs `interleave` there.
    struct PausedMove<'a, F> {
        vault: &'a MemoryVault,
        interleave: F,
    }

    impl<F: Fn()> Vault for PausedMove<'_, F> {
        fn get(&self, service: &str, slot: &str) -> Result<Option<String>, String> {
            let value = self.vault.get(service, slot)?;
            if service == LEGACY_SERVICE {
                (self.interleave)();
            }
            Ok(value)
        }

        fn set(&self, service: &str, slot: &str, secret: &str) -> Result<(), String> {
            self.vault.set(service, slot, secret)
        }

        fn delete(&self, service: &str, slot: &str) -> Result<bool, String> {
            self.vault.delete(service, slot)
        }
    }

    /// Start `command` on another thread while a read is moving a legacy key,
    /// and give it time to finish before the move goes on. Unserialized, the
    /// command completes inside the move; serialized, it waits the move out.
    fn move_racing_with(command: impl Fn(&MemoryVault) + Sync) -> MemoryVault {
        let vault = MemoryVault::with(LEGACY_SERVICE, "openai", "sk-old");
        let (vault_ref, command) = (&vault, &command);
        std::thread::scope(|scope| {
            let paused = PausedMove {
                vault: vault_ref,
                interleave: || {
                    let (done, finished) = std::sync::mpsc::channel();
                    scope.spawn(move || {
                        command(vault_ref);
                        let _ = done.send(());
                    });
                    let _ = finished.recv_timeout(std::time::Duration::from_millis(200));
                },
            };
            assert_eq!(
                get_key_in(&paused, "openai").unwrap().as_deref(),
                Some("sk-old")
            );
        });
        vault
    }

    #[test]
    fn a_key_saved_during_a_move_is_not_overwritten() {
        let vault = move_racing_with(|vault| {
            save_key_in(vault, "openai", "sk-new").unwrap();
        });

        assert_eq!(
            get_key_in(&vault, "openai").unwrap().as_deref(),
            Some("sk-new")
        );
        assert!(!vault.has(LEGACY_SERVICE, "openai"));
    }

    #[test]
    fn a_key_deleted_during_a_move_stays_deleted() {
        let vault = move_racing_with(|vault| {
            assert!(delete_key_in(vault, "openai").unwrap());
        });

        assert!(!vault.has(SERVICE, "openai"));
        assert!(!vault.has(LEGACY_SERVICE, "openai"));
    }

    #[test]
    fn purge_matches_only_credentials_written_by_keyring_for_sotto() {
        let services = [SERVICE, LEGACY_SERVICE];
        let owned = |target, user, comment| owned_credential(&services, target, user, comment);
        assert_eq!(
            owned("openai.sotto", "openai", "keyring v3.6.3"),
            Some((SERVICE, "openai".to_string()))
        );
        assert_eq!(
            owned("profile-1.speech-to-text", "profile-1", "keyring v3.6.3"),
            Some((LEGACY_SERVICE, "profile-1".to_string()))
        );
        // Another program's credential with a similar name.
        assert_eq!(owned("openai.sotto", "openai", "saved by a browser"), None);
        assert_eq!(owned("openai.sotto", "someone", "keyring v3.6.3"), None);
        assert_eq!(owned("openai.sotto-sync", "openai", "keyring v3.6.3"), None);
    }

    /// Writes a credential under a service name of its own, so the real
    /// `sotto` and `speech-to-text` keys are never enumerated for deletion.
    #[cfg(windows)]
    #[test]
    #[ignore = "writes to the Windows Credential Manager; run on a Windows desktop"]
    fn native_purge_deletes_enumerated_credentials() {
        let service = format!("sotto-purge-test-{}", uuid::Uuid::new_v4());
        Keyring.set(&service, "slot-1", "secret").unwrap();
        Keyring.set(&service, "slot-2", "secret").unwrap();

        assert_eq!(purge_services(&[service.as_str()]), Vec::<String>::new());

        assert_eq!(Keyring.get(&service, "slot-1").unwrap(), None);
        assert_eq!(Keyring.get(&service, "slot-2").unwrap(), None);
    }

    #[test]
    fn empty_key_is_rejected() {
        let result = save_key("anthropic", "   ");
        assert!(result.is_err(), "empty/whitespace key must be rejected");
    }

    /// The composition in `get_key_meta` is what a broken `mask_key` (or a
    /// constant-substituted `KeyMeta`) would hide. Assert against a literal,
    /// not against `mask_key(&raw)`, so both sides cannot share one bug.
    #[test]
    fn key_meta_from_composes_available_label_and_masked() {
        let meta = key_meta_from("sk-ant-0123456789abcd");
        assert!(meta.available);
        assert_eq!(meta.label, "");
        assert_eq!(meta.masked, "sk-ant…abcd");
    }
}
