CREATE TABLE release_notes_state (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    seen_version TEXT NOT NULL,
    cached_version TEXT,
    cached_notes TEXT
);
