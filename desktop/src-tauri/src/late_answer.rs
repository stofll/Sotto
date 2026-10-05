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

use crate::ai::step::{LateAnswer, LateAnswerMode};

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
            return;
        }
        let ai_json = crate::ai_processing_json(Some(&outcome.status)).unwrap_or_default();
        let llm_seconds = outcome.status.elapsed_seconds;
        let text = outcome.text;
        let stored = crate::run_db_op(db, move |conn| {
            crate::history::store_late_answer(conn, entry_id, &text, &ai_json, llm_seconds)
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
        if mode == LateAnswerMode::Notify {
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
