//! AI provider subsystem (Phase 4 / Batch 3 / PR 3.2).
//!
//! Composes the provider implementations (Anthropic, OpenAI, Gemini,
//! OpenCode Go, OpenAI-compatible), the orchestrator
//! (`ai_process_text_with_status`), and the reasoning-tag stripper
//! into a single Rust module that the dispatcher and the
//! `preview_history_ai_processing` Tauri command call directly.
//!
//! No Python sidecar round-trip. The pipeline parity with
//! `ai_processor._step.ai_process_text_with_status` is covered by
//! unit tests in this module and by the smoke test in
//! `tests/smoke_test.rs`.
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
