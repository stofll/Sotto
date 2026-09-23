//! AI provider subsystem.
//!
//! Composes the provider implementations (Anthropic, OpenAI, Gemini,
//! OpenCode Go, OpenAI-compatible), the orchestrator
//! (`ai_process_text_with_status`), and the reasoning-tag stripper
//! into a single Rust module that the dispatcher and the
//! `preview_history_ai_processing` Tauri command call directly.
//!
//! Provider requests and error handling are covered by `tests/ai_mock_test.rs`.
//!
//! Secret keys come from the `secret_store` module — the API here
//! takes plain `&str` keys, the caller (dispatcher) looks them up.

pub mod fidelity;
pub mod models;
pub mod providers;
pub mod reasoning;
pub mod step;

pub use providers::{
    AnthropicProvider, GeminiProvider, OpenAIProvider, OpenCodeGoProvider, Provider, ProviderError,
    ProviderErrorType, ProviderInfo, UsageInfo,
};
pub use step::{ai_process_text, ai_process_text_with_status, AiStatus};

use tauri::AppHandle;

/// Shared core for the two explicit-LLM commands (`test_ai_prompt`,
/// `process_text_ai`). Both run the SAME orchestrator the live
/// dispatcher and history-retry use, but force `pipeline_mode = "hybrid"`
/// and `llm_min_duration_seconds = 0` so the LLM step always runs — the
/// user pressed a button explicitly, there is no recording-duration gate.
///
/// Returns the `AiRunResult` shape the frontend expects:
/// `{ available, output, message?, fallback, provider_error?, skipped_reason?, ai_processing }`.
// The provider/model/key/url/prompt/profile fields mirror the Tauri command
// IPC signatures below; collapsing them into a struct would change the
// frontend invoke contract, so keep the flat arg list.
#[allow(clippy::too_many_arguments)]
async fn run_ai_prompt(
    provider: Option<String>,
    model: Option<String>,
    api_key_ref: Option<String>,
    base_url: Option<String>,
    system_prompt: Option<String>,
    profile_id: Option<String>,
    profile_name: Option<String>,
    language: String,
    text: &str,
) -> Result<serde_json::Value, String> {
    let provider = provider.unwrap_or_default();
    if provider.trim().is_empty() {
        return Ok(
            serde_json::json!({ "available": false, "message": crate::ui_text::t("Не выбран провайдер.") }),
        );
    }
    let api_key_ref = api_key_ref.unwrap_or_default();
    let cfg = step::AiConfig {
        pipeline_mode: "hybrid".to_string(),
        provider,
        model: model.unwrap_or_default(),
        profile_id: profile_id.unwrap_or_default(),
        profile_name: profile_name.unwrap_or_default(),
        api_key_ref: api_key_ref.clone(),
        system_prompt: system_prompt.unwrap_or_default(),
        language,
        base_url: base_url.filter(|value| !value.trim().is_empty()),
        audio_duration_seconds: None,
        llm_min_duration_seconds: 0.0,
        llm_timeout_seconds: 30,
    };
    let api_key = if api_key_ref.is_empty() {
        None
    } else {
        crate::secret_store::load_key(&api_key_ref)
            .await
            .map_err(|e| format!("secret_store get_key({api_key_ref}): {e}"))?
    };
    let outcome = ai_process_text_with_status(text, &cfg, api_key.as_deref()).await;
    let status = &outcome.status;
    let message = if status.used {
        serde_json::Value::Null
    } else if let Some(err) = &status.provider_error {
        serde_json::json!(err)
    } else if !status.skipped_reason.is_empty() {
        serde_json::json!(status.skipped_reason)
    } else {
        serde_json::json!(crate::ui_text::t("LLM не вернула результат."))
    };
    Ok(serde_json::json!({
        "available": status.used,
        "output": outcome.text,
        "fallback": status.fallback,
        "provider_error": status.provider_error,
        "skipped_reason": if status.skipped_reason.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::json!(status.skipped_reason)
        },
        "message": message,
        // The provider's response body. It is assembled in `send_request` and
        // reaches `AiStatus`, but this response used to be built by hand and the
        // field never made it in — that is, the only source of truth about "the
        // response has the wrong shape" was lost at the final step, already
        // outside the HTTP layer.
        "http_status": status.http_status,
        "response_snippet": status.response_snippet,
        "ai_processing": {
            "attempted": status.attempted,
            "used": status.used,
            "skipped_reason": status.skipped_reason,
        },
    }))
}

/// Run one LLM request through the active profile — used by the "Тест"
/// buttons in the LLM and Providers pages. Falls back to a fixed sample
/// sentence when the caller supplies no text.
#[tauri::command(rename_all = "snake_case")]
#[allow(clippy::too_many_arguments)] // IPC command signature; see run_ai_prompt
pub(crate) async fn test_ai_prompt(
    app: AppHandle,
    provider: Option<String>,
    model: Option<String>,
    api_key_ref: Option<String>,
    base_url: Option<String>,
    system_prompt: Option<String>,
    profile_id: Option<String>,
    profile_name: Option<String>,
    text: Option<String>,
) -> Result<serde_json::Value, String> {
    use tauri::Manager;
    app.state::<crate::telemetry::Telemetry>()
        .begin_usage_session(crate::telemetry::SessionTrigger::Llm);
    let sample = text
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| {
            // Speech language: a sample dictation.
            "ну в общем нужно сегодня встретиться с командой и обсудить следующие шаги".to_string()
        });
    run_ai_prompt(
        provider,
        model,
        api_key_ref,
        base_url,
        system_prompt,
        profile_id,
        profile_name,
        crate::speech_language(crate::config::Config::load(&app).ok().as_ref()),
        &sample,
    )
    .await
}

/// Process arbitrary user-supplied text through the active profile's LLM
/// — the "Обработать текст" panel in the LLM page.
#[tauri::command(rename_all = "snake_case")]
#[allow(clippy::too_many_arguments)] // IPC command signature; see run_ai_prompt
pub(crate) async fn process_text_ai(
    app: AppHandle,
    text: String,
    provider: Option<String>,
    model: Option<String>,
    api_key_ref: Option<String>,
    base_url: Option<String>,
    system_prompt: Option<String>,
    profile_id: Option<String>,
    profile_name: Option<String>,
) -> Result<serde_json::Value, String> {
    use tauri::Manager;
    app.state::<crate::telemetry::Telemetry>()
        .begin_usage_session(crate::telemetry::SessionTrigger::Llm);
    if text.trim().is_empty() {
        return Ok(
            serde_json::json!({ "available": false, "message": crate::ui_text::t("Вставьте текст для обработки.") }),
        );
    }
    run_ai_prompt(
        provider,
        model,
        api_key_ref,
        base_url,
        system_prompt,
        profile_id,
        profile_name,
        crate::speech_language(crate::config::Config::load(&app).ok().as_ref()),
        &text,
    )
    .await
}

/// Which key `fetch_provider_models` sends: the stored one whenever the ref
/// resolved to something, and only otherwise the value handed in.
///
/// The order matters. A profile being edited passes both — its ref, and
/// whatever sits in the wizard's field from an earlier visit — and the store
/// is the truth about what that profile actually authenticates with.
fn model_request_key(stored: String, passed: Option<String>) -> String {
    if !stored.trim().is_empty() {
        return stored;
    }
    passed
        .map(|value| value.trim().to_string())
        .unwrap_or_default()
}

/// Ask a provider which models it currently serves.
///
/// The key normally comes from the secret store: the frontend holds a ref and
/// not the value. `api_key` is the exception this was widened for — in the
/// «Новый профиль» wizard the key has been typed but not yet saved, so there is
/// no ref to look up, and without it the «запросить модели» button would be
/// dead in exactly the flow that needs it most. The value crosses the same IPC
/// boundary as `save_api_key`, which the wizard calls moments later with the
/// same string; a ref, when it resolves, still wins.
///
/// Errors come back as plain strings for display next to the model field.
/// A failed list is not a failed configuration — the user can still type a
/// model id — so the caller must not treat it as fatal.
#[tauri::command(rename_all = "snake_case")]
pub(crate) async fn fetch_provider_models(
    provider: String,
    base_url: Option<String>,
    api_key_ref: Option<String>,
    api_key: Option<String>,
) -> Result<Vec<String>, String> {
    let stored = match api_key_ref.filter(|value| !value.trim().is_empty()) {
        Some(reference) => crate::secret_store::load_key(&reference)
            .await
            .map_err(|e| format!("secret_store get_key({reference}): {e}"))?
            .unwrap_or_default(),
        None => String::new(),
    };
    let api_key = model_request_key(stored, api_key);
    models::fetch_models(&provider, base_url.as_deref(), &api_key).await
}

#[cfg(test)]
mod model_request_key_tests {
    use super::model_request_key;

    /// The wizard's case: nothing is stored under the ref yet, because the key
    /// is still only in the field.
    #[test]
    fn falls_back_to_the_value_handed_in() {
        assert_eq!(
            model_request_key(String::new(), Some("sk-typed".into())),
            "sk-typed"
        );
        assert_eq!(
            model_request_key("   ".into(), Some("sk-typed".into())),
            "sk-typed"
        );
    }

    /// An existing profile passes both. The store is what it authenticates
    /// with, so a stale field must not quietly take over.
    #[test]
    fn the_stored_key_wins_when_there_is_one() {
        assert_eq!(
            model_request_key("sk-stored".into(), Some("sk-typed".into())),
            "sk-stored"
        );
    }

    /// A local server needs no key at all, and neither side has one.
    #[test]
    fn an_empty_result_is_a_valid_answer() {
        assert_eq!(model_request_key(String::new(), None), "");
        assert_eq!(model_request_key(String::new(), Some("  ".into())), "");
    }
}
