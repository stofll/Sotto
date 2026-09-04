-- WS 4b schema v2: why the LLM step falls back.
--
-- The `llm_fallbacks` counter in `stats_daily` says how often the LLM step
-- gave up, and nothing at all about why. Per-entry detail does live in
-- `history.ai_processing_json`, but history is pruned, so by the time anyone
-- asks "why is this failing?" the evidence is gone. This table is an
-- aggregate: a handful of rows per day, retained with the rest of the stats.
--
-- `http_status` is 0 rather than NULL when the provider never answered:
-- SQLite treats NULLs in a PRIMARY KEY as distinct, which would turn the
-- upsert into an append-only log.

CREATE TABLE IF NOT EXISTS llm_fallback_reasons (
    date TEXT NOT NULL,
    error_type TEXT NOT NULL,
    http_status INTEGER NOT NULL DEFAULT 0,
    count INTEGER NOT NULL DEFAULT 0,
    last_error TEXT NOT NULL DEFAULT '',
    PRIMARY KEY (date, error_type, http_status)
);
