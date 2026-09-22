-- Zero defaults preserve the original duration-based estimate for older data.
ALTER TABLE stats_daily ADD COLUMN excluded_silence_seconds REAL NOT NULL DEFAULT 0;
ALTER TABLE stats_daily ADD COLUMN speech_timed_count INTEGER NOT NULL DEFAULT 0;
