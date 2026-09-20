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
//! Keep the service name stable so existing credentials remain readable:
//!
//! - `service` = `"speech-to-text"`
//! - `username` = key slot ref (legacy slots used the provider id)
//! - `password` = the raw API key
//!
//! Labels are not persisted by this module; save commands echo the label
//! supplied by the caller, while metadata reads return an empty label.

const SERVICE: &str = "speech-to-text";

/// Characters the long mask keeps from each end of the key, and the prefix the
/// short mask keeps. Each is revealed only by a key at least twice its length.
const HEAD: usize = 6;
const TAIL: usize = 4;
const SHORT_HEAD: usize = 2;

fn entry(provider: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(SERVICE, provider).map_err(|error| format!("SECRET_STORE_INIT: {error}"))
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
    if key.trim().is_empty() {
        return Err("SECRET_STORE_EMPTY_KEY: refusing to save an empty key".to_string());
    }
    let entry = entry(provider)?;
    entry
        .set_password(key)
        .map_err(|error| format!("SECRET_STORE_SAVE: {error}"))?;
    Ok(true)
}

pub fn get_key(provider: &str) -> Result<Option<String>, String> {
    let entry = entry(provider)?;
    match entry.get_password() {
        Ok(value) => Ok(Some(value)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(format!("SECRET_STORE_GET: {error}")),
    }
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
    let entry = entry(provider)?;
    match entry.delete_credential() {
        Ok(()) => Ok(true),
        Err(keyring::Error::NoEntry) => Ok(false),
        Err(error) => Err(format!("SECRET_STORE_DELETE: {error}")),
    }
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
pub(crate) fn save_api_key(
    key_id: String,
    key: String,
    label: Option<String>,
) -> Result<serde_json::Value, String> {
    save_key(&key_id, &key)?;
    let masked = get_key_meta(&key_id)?
        .map(|meta| meta.masked)
        .unwrap_or_default();
    Ok(serde_json::json!({
        "saved": true,
        "label": label.unwrap_or_default(),
        "masked": masked,
    }))
}

/// Report whether a key exists for the given slot ref, with its mask.
/// Called at boot for every known slot and after edits.
#[tauri::command(rename_all = "snake_case")]
pub(crate) fn has_api_key(key_id: String) -> Result<serde_json::Value, String> {
    match get_key_meta(&key_id)? {
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
pub(crate) fn delete_api_key(key_id: String) -> Result<serde_json::Value, String> {
    let deleted = delete_key(&key_id)?;
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
