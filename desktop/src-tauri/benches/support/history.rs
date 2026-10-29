//! Synthetic history only. Database setup is outside every timed region.

use rusqlite::{params, Connection};

pub fn populate(conn: &mut Connection, count: usize) {
    // These are the production migrations that define the queried columns.
    conn.execute_batch(concat!(
        include_str!("../../src/migrations/v1.sql"),
        include_str!("../../src/migrations/v3.sql"),
        include_str!("../../src/migrations/v11.sql"),
    ))
    .unwrap();
    let body = "Синтетическая диктовка для проверки истории проекта. ".repeat(3);
    let stats =
        r#"{"audio_seconds":12.0,"whisper_seconds":0.8,"llm_seconds":0.0,"total_seconds":0.8}"#;
    let tx = conn.transaction().unwrap();
    {
        let mut insert = tx
            .prepare(
                "INSERT INTO history (id, timestamp, text, raw_text, formatted_text, language, \
                 inference_time_ms, ai_processing_json, processing_stats_json, system_prompt, \
                 transcription_model, length) VALUES (?1, ?2, ?3, ?3, ?3, 'ru', 800, ?4, ?5, \
                 'Сохраните смысл исходного текста.', 'benchmark-model', ?6)",
            )
            .unwrap();
        for index in 0..count {
            let marker = if index % 100 == 0 {
                "ЯКОРЬ needle "
            } else {
                ""
            };
            let text = format!("{marker}{index}: {body}");
            let status = match index % 3 {
                0 => r#"{"attempted":true,"used":true,"fallback":false}"#,
                1 => r#"{"attempted":true,"used":false,"fallback":true}"#,
                _ => r#"{"attempted":false,"used":false,"fallback":false}"#,
            };
            insert
                .execute(params![
                    index as i64 + 1,
                    // Ties exercise the stable timestamp/id ordering.
                    2_000_000_000.0 + (index / 2) as f64,
                    text,
                    status,
                    stats,
                    text.chars().count() as i64,
                ])
                .unwrap();
        }
    }
    tx.commit().unwrap();
}
