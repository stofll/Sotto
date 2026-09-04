-- Product telemetry is deliberately isolated from the user-facing statistics
-- tables.  `stats_daily.count` means successful local history entries and must
-- not be reinterpreted as starts, failures, or anonymous product events.
CREATE TABLE IF NOT EXISTS telemetry_meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

-- The worker persists typed, allowlisted events before attempting HTTP.  The
-- payload is JSON only at this storage boundary; callers cannot submit an
-- arbitrary event name or payload through the telemetry API.
CREATE TABLE IF NOT EXISTS telemetry_outbox (
    event_id TEXT PRIMARY KEY,
    event_name TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    created_at REAL NOT NULL,
    attempts INTEGER NOT NULL DEFAULT 0,
    next_attempt_at REAL NOT NULL DEFAULT 0,
    last_error TEXT NOT NULL DEFAULT ''
);

CREATE INDEX IF NOT EXISTS idx_telemetry_outbox_pending
    ON telemetry_outbox(next_attempt_at, created_at);
