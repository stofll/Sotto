//! An LLM answer that arrives after the dictation was pasted without it.
//!
//! The paste never waits past the LLM timeout: the user is mid-sentence in
//! another window and the local text is good enough to go on with. But the
//! request was paid for, and cancelling it threw away an answer that was
//! often a second away. So the request keeps running (see
//! [`crate::ai::step::LateAnswer`]); when its answer passes the same checks
//! as one on time, it replaces the text in the history entry, and the overlay
//! says so if the user asked to be told.

use std::sync::{Arc, Mutex};

use rusqlite::Connection;
use tauri::{AppHandle, Emitter, Manager};

use crate::ai::step::{CallOutcome, LateAnswer, LateAnswerMode};

/// What a late answer leaves in its history entry.
#[derive(Debug, PartialEq)]
struct LateWrite {
    /// The new text, or `None` to keep the entry's: a check turned the answer
    /// down, and only the status, with the answer as a variant, is stored.
    text: Option<String>,
    /// Tell the overlay the text changed.
    notify: bool,
}

/// `None` when there is nothing to store: the answer was neither kept nor
/// offered as a variant.
fn late_write(outcome: CallOutcome, mode: LateAnswerMode) -> Option<LateWrite> {
    let used = outcome.status.used;
    if !used && outcome.status.rejected_text.is_none() {
        return None;
    }
    Some(LateWrite {
        text: used.then_some(outcome.text),
        notify: used && mode == LateAnswerMode::Notify,
    })
}

/// Wait for `late` in the background and store it in history entry
/// `entry_id`. Returns at once; nothing here can hold up the paste.
pub(crate) fn follow(app: AppHandle, db: Arc<Mutex<Connection>>, entry_id: u64, late: LateAnswer) {
    tauri::async_runtime::spawn(async move {
        let mode = late.mode();
        let Some(outcome) = late.wait().await else {
            log::info!("late LLM answer for history entry {entry_id} never arrived");
            return;
        };
        if !outcome.status.used {
            log::info!(
                "late LLM answer for history entry {entry_id} was not kept: {}",
                outcome.status.skipped_reason
            );
        }
        let ai_json = crate::ai_processing_json(Some(&outcome.status)).unwrap_or_default();
        let llm_seconds = outcome.status.elapsed_seconds;
        let Some(LateWrite { text, notify }) = late_write(outcome, mode) else {
            return;
        };
        let stored = crate::run_db_op(db, move |conn| {
            crate::history::store_late_answer(
                conn,
                entry_id,
                text.as_deref(),
                &ai_json,
                llm_seconds,
            )
        })
        .await;
        match stored {
            Ok(true) => {}
            Ok(false) => return,
            Err(error) => {
                log::warn!("late LLM answer could not be stored: {error}");
                return;
            }
        }
        let _ = app.emit(
            "history-updated",
            serde_json::json!({ "entry_id": entry_id }),
        );
        if notify {
            let _ = app.emit(
                "llm-late-answer",
                serde_json::json!({ "entry_id": entry_id }),
            );
        }
    });
}

/// «Копировать» on the overlay's late-answer note: the entry's text as it is
/// now, read from history rather than carried over IPC.
#[tauri::command]
pub(crate) async fn copy_history_entry(
    app: AppHandle,
    state: tauri::State<'_, crate::state::AppState>,
    id: u64,
) -> Result<(), String> {
    let entry = crate::run_db_op(state.db.clone(), move |conn| {
        crate::history::read_history_entry(conn, id)
    })
    .await?
    .ok_or_else(|| format!("entry not found: {id}"))?;
    crate::clipboard::copy_to_clipboard(&app, &entry.text)
}

/// «Открыть в истории»: bring the main window forward on the history page,
/// scrolled to the entry.
#[tauri::command]
pub(crate) fn open_history_entry(app: AppHandle, id: u64) -> Result<(), String> {
    crate::show_main_window(&app)?;
    if let Some(window) = app.get_webview_window("main") {
        window
            .emit("open-history-entry", id)
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::step::AiStatus;

    fn outcome(used: bool, rejected: Option<&str>) -> CallOutcome {
        CallOutcome {
            text: "Ответ.".to_string(),
            status: AiStatus {
                used,
                rejected_text: rejected.map(str::to_string),
                ..AiStatus::default()
            },
            late: None,
        }
    }

    #[test]
    fn a_kept_late_answer_replaces_the_text_and_notifies_when_asked() {
        assert_eq!(
            late_write(outcome(true, None), LateAnswerMode::Notify),
            Some(LateWrite {
                text: Some("Ответ.".to_string()),
                notify: true
            })
        );
        assert!(
            !late_write(outcome(true, None), LateAnswerMode::Silent)
                .unwrap()
                .notify
        );
    }

    /// A turned-down late answer is stored as a variant, quietly: the text
    /// the user pasted did not change.
    #[test]
    fn a_turned_down_late_answer_keeps_the_text_without_a_notice() {
        assert_eq!(
            late_write(outcome(false, Some("Вариант.")), LateAnswerMode::Notify),
            Some(LateWrite {
                text: None,
                notify: false
            })
        );
    }

    #[test]
    fn a_late_answer_with_nothing_to_offer_stores_nothing() {
        assert_eq!(
            late_write(outcome(false, None), LateAnswerMode::Notify),
            None
        );
    }
}
