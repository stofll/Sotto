CREATE TABLE IF NOT EXISTS model_performance (
    id INTEGER PRIMARY KEY,
    model_id TEXT NOT NULL,
    created INTEGER NOT NULL,
    profile TEXT NOT NULL,
    payload TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS model_performance_model ON model_performance(model_id, created);
