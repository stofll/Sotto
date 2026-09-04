-- WS 4b schema v1: initial rusqlite migration.
-- Idempotent (CREATE TABLE IF NOT EXISTS) — safe to apply on a database that
-- already has these tables. New tables / columns go into v2.sql.

CREATE TABLE IF NOT EXISTS stats_totals (
    key TEXT PRIMARY KEY,
    value REAL NOT NULL
);

CREATE TABLE IF NOT EXISTS stats_daily (
    date TEXT PRIMARY KEY,
    count INTEGER NOT NULL DEFAULT 0,
    chars INTEGER NOT NULL DEFAULT 0,
    time_saved_seconds REAL NOT NULL DEFAULT 0,
    audio_seconds REAL NOT NULL DEFAULT 0,
    processing_seconds REAL NOT NULL DEFAULT 0,
    whisper_seconds REAL NOT NULL DEFAULT 0,
    format_seconds REAL NOT NULL DEFAULT 0,
    llm_seconds REAL NOT NULL DEFAULT 0,
    llm_attempts INTEGER NOT NULL DEFAULT 0,
    llm_used INTEGER NOT NULL DEFAULT 0,
    llm_fallbacks INTEGER NOT NULL DEFAULT 0,
    llm_input_tokens INTEGER NOT NULL DEFAULT 0,
    llm_output_tokens INTEGER NOT NULL DEFAULT 0,
    llm_tokens INTEGER NOT NULL DEFAULT 0,
    replacement_applications INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS history (
    id INTEGER PRIMARY KEY,
    timestamp REAL NOT NULL,
    text TEXT NOT NULL,
    raw_text TEXT NOT NULL DEFAULT '',
    formatted_text TEXT NOT NULL DEFAULT '',
    language TEXT,
    inference_time_ms INTEGER,
    session_id INTEGER,
    ai_processing_json TEXT,
    processing_stats_json TEXT,
    system_prompt TEXT,
    length INTEGER NOT NULL,
    pruned INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_history_timestamp ON history(timestamp);