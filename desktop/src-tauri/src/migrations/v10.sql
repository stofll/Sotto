-- Commit the legacy import marker with its rows so a failed file rename
-- cannot cause another import on the next launch.
CREATE TABLE legacy_imports (
    file_name TEXT PRIMARY KEY
);
