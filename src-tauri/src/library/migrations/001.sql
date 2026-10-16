CREATE TABLE sources (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    kind TEXT NOT NULL,
    external_key TEXT NOT NULL,
    UNIQUE(kind, external_key)
);
CREATE TABLE roots (
    id INTEGER PRIMARY KEY REFERENCES sources(id) ON DELETE CASCADE,
    path TEXT NOT NULL UNIQUE,
    identity TEXT,
    generation INTEGER NOT NULL DEFAULT 0,
    scan_state TEXT NOT NULL DEFAULT 'idle',
    error TEXT,
    last_scan INTEGER
);
CREATE TABLE recordings (
    id INTEGER PRIMARY KEY,
    identity TEXT UNIQUE,
    size INTEGER NOT NULL CHECK(size >= 0),
    modified_ms INTEGER,
    modified_ns TEXT,
    status TEXT NOT NULL DEFAULT 'pending' CHECK(status IN ('pending','ready','error')),
    error TEXT
);
CREATE TABLE locations (
    id INTEGER PRIMARY KEY,
    file_id INTEGER NOT NULL REFERENCES recordings(id),
    path TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL
);
CREATE INDEX locations_file ON locations(file_id);
CREATE TABLE root_entries (
    root_id INTEGER NOT NULL REFERENCES roots(id) ON DELETE CASCADE,
    location_id INTEGER NOT NULL REFERENCES locations(id),
    generation INTEGER NOT NULL,
    availability TEXT NOT NULL CHECK(availability IN ('available','missing','unverified','inaccessible')),
    PRIMARY KEY(root_id, location_id)
);
CREATE INDEX entries_location ON root_entries(location_id);
CREATE TABLE media (
    file_id INTEGER PRIMARY KEY REFERENCES recordings(id),
    duration REAL,
    video_codec TEXT,
    width INTEGER,
    height INTEGER,
    frame_rate REAL,
    audio_count INTEGER NOT NULL,
    payload TEXT NOT NULL
);
-- Future provider importers enrich the same physical recording IDs. No provider schema is assumed.
CREATE TABLE source_recordings (
    source_id INTEGER NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
    external_id TEXT NOT NULL,
    file_id INTEGER NOT NULL REFERENCES recordings(id),
    title TEXT,
    game TEXT,
    recorded_at INTEGER,
    tags_json TEXT NOT NULL DEFAULT '[]',
    extra_json TEXT NOT NULL DEFAULT '{}',
    PRIMARY KEY(source_id, external_id)
);
CREATE INDEX source_recordings_file ON source_recordings(file_id);
PRAGMA user_version = 1;
