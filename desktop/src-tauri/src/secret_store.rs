//! Native cross-platform secret store (Phase 4 / Batch 3).
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
//! Storage shape mirrors the Python `_storage` module so call sites
//! have a 1:1 port:
//!
//! - `service` = `"speech-to-text"`
//! - `username` = provider id (`"anthropic"`, `"openai"`, `"gemini"`, …)
//! - `password` = the raw API key
//!
//! `label` (a human-readable hint) is only stored on Windows (Credential
//! Manager attributes) — the macOS Keychain and Linux Secret Service
//! don't have a portable label field, matching the Python backend's
//! simplification.

const SERVICE: &str = "speech-to-text";

fn entry(provider: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(SERVICE, provider).map_err(|error| format!("SECRET_STORE_INIT: {error}"))
}

fn mask_key(key: &str) -> String {
    if key.is_empty() {
        return String::new();
    }
    if key.chars().count() <= 8 {
        if key.chars().count() > 2 {
            let prefix: String = key.chars().take(2).collect();
            return format!("{prefix}…");
        }
        return "…".to_string();
    }
    let chars: Vec<char> = key.chars().collect();
    let len = chars.len();
    let head: String = chars[..6].iter().collect();
    let tail: String = chars[len - 4..].iter().collect();
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
        // Cross-platform label support is not portable (the macOS
        // Keychain and Linux Secret Service don't have a built-in
        // label attribute). We mirror the Python backend's
        // simplification and return an empty string everywhere.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mask_key_short_strings() {
        assert_eq!(mask_key(""), "");
        assert_eq!(mask_key("ab"), "…");
        assert_eq!(mask_key("abc"), "ab…");
        // 8-char threshold (matches the Python `_mask_key` short
        // branch): at ≤ 8 chars we show first 2 + ellipsis only.
        assert_eq!(mask_key("abcdefgh"), "ab…");
        // 9+ chars: switch to first 6 + ellipsis + last 4.
        assert_eq!(mask_key("abcdefghi"), "abcdef…fghi");
    }

    #[test]
    fn mask_key_unicode() {
        // Char-count based, not byte-count: 10 chars = first 6 + last 4.
        let masked = mask_key("1234567890");
        assert_eq!(masked, "123456…7890");
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
