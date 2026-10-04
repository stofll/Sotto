//! Qwen3 stream options for the pinned multilingual export.

#[cfg(any(windows, target_os = "macos"))]
pub(super) fn language_name(code: &str) -> Option<&'static str> {
    Some(match code {
        "zh" => "Chinese",
        "en" => "English",
        "yue" => "Cantonese",
        "ar" => "Arabic",
        "de" => "German",
        "fr" => "French",
        "es" => "Spanish",
        "pt" => "Portuguese",
        "id" => "Indonesian",
        "it" => "Italian",
        "ko" => "Korean",
        "ru" => "Russian",
        "th" => "Thai",
        "vi" => "Vietnamese",
        "ja" => "Japanese",
        "tr" => "Turkish",
        "hi" => "Hindi",
        "ms" => "Malay",
        "nl" => "Dutch",
        "sv" => "Swedish",
        "da" => "Danish",
        "fi" => "Finnish",
        "pl" => "Polish",
        "cs" => "Czech",
        "fil" => "Filipino",
        "fa" => "Persian",
        "el" => "Greek",
        "hu" => "Hungarian",
        "mk" => "Macedonian",
        "ro" => "Romanian",
        _ => return None,
    })
}

pub(super) fn bounded_hotwords(prompt: &str) -> String {
    // Byte-level BPE cannot produce more tokens than UTF-8 bytes. This cap
    // reserves space in the 2048-token cache for audio and 512 output tokens.
    // Keep whole terms and strip NUL before the upstream CString conversion.
    const MAX_BYTES: usize = 1024;
    let mut result = String::new();
    for term in prompt.split(',') {
        let term: String = term.chars().filter(|&c| c != '\0').collect();
        let term = term.trim();
        if term.is_empty() {
            continue;
        }
        let separator = if result.is_empty() { "" } else { ", " };
        if result.len() + separator.len() + term.len() > MAX_BYTES {
            break;
        }
        result.push_str(separator);
        result.push_str(term);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hints_preserve_whole_terms_and_cannot_overflow_context_or_panic_ffi() {
        assert_eq!(
            bounded_hotwords(" , Git\0Hub, Санкт-Петербург, "),
            "GitHub, Санкт-Петербург"
        );
        let first = "я".repeat(500);
        let prompt = format!("{first}, OpenAI, слишком длинное слово, Tail");
        assert_eq!(bounded_hotwords(&prompt), format!("{first}, OpenAI"));
        assert_eq!(bounded_hotwords(&"я".repeat(513)), "");
        assert_eq!(bounded_hotwords(""), "");
    }

    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn every_catalog_language_can_be_forced_without_enabling_unsupported_languages() {
        for code in crate::model::model_languages("qwen3-asr-0.6b").unwrap() {
            assert!(language_name(code).is_some(), "{code}");
        }
        assert_eq!(language_name("ru"), Some("Russian"));
        assert_eq!(language_name("auto"), None);
        assert_eq!(language_name("uk"), None);
    }
}
