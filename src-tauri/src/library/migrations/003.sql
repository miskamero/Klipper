-- Survives an interrupted reset so old previews cannot be reused after recording IDs restart.
CREATE TABLE library_state (
    id INTEGER PRIMARY KEY CHECK(id=1),
    cache_cleanup_pending INTEGER NOT NULL DEFAULT 0 CHECK(cache_cleanup_pending IN (0,1))
);
INSERT INTO library_state(id) VALUES(1);
PRAGMA user_version = 3;
