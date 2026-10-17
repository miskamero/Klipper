-- Catalog data is separate from inspected media. Links distinguish an edit from its original.
CREATE TABLE provider_entries (
    source_id INTEGER NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
    external_id TEXT NOT NULL,
    revision TEXT NOT NULL,
    title TEXT,
    game TEXT,
    created_ms INTEGER,
    metadata_json TEXT NOT NULL,
    imported_ms INTEGER NOT NULL,
    PRIMARY KEY(source_id, external_id)
);
CREATE TABLE provider_links (
    source_id INTEGER NOT NULL,
    external_id TEXT NOT NULL,
    file_id INTEGER NOT NULL REFERENCES recordings(id),
    relation TEXT NOT NULL CHECK(relation IN ('primary','original')),
    file_size INTEGER,
    file_modified_ns TEXT,
    PRIMARY KEY(source_id, external_id, file_id, relation),
    FOREIGN KEY(source_id, external_id) REFERENCES provider_entries(source_id, external_id) ON DELETE CASCADE
);
CREATE INDEX provider_links_file ON provider_links(file_id);
-- Exact file grants confirmed during import, never folder-wide grants or persisted player IDs.
CREATE TABLE provider_access (
    source_id INTEGER NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
    location_id INTEGER NOT NULL REFERENCES locations(id),
    availability TEXT NOT NULL DEFAULT 'available' CHECK(availability IN ('available','missing','inaccessible')),
    PRIMARY KEY(source_id, location_id)
);
CREATE TABLE provider_imports (
    source_id INTEGER PRIMARY KEY REFERENCES sources(id) ON DELETE CASCADE,
    catalog_path TEXT NOT NULL,
    revision TEXT NOT NULL,
    imported_ms INTEGER NOT NULL
);
-- Reserved user overrides take precedence, without implementing an editor in this milestone.
CREATE TABLE recording_overrides (
    file_id INTEGER PRIMARY KEY REFERENCES recordings(id),
    title TEXT,
    game TEXT
);
CREATE TABLE provider_previews (
    source_id INTEGER NOT NULL,
    external_id TEXT NOT NULL,
    relation TEXT NOT NULL CHECK(relation IN ('primary','original')),
    path TEXT NOT NULL,
    revision TEXT NOT NULL,
    PRIMARY KEY(source_id, external_id, relation),
    FOREIGN KEY(source_id, external_id) REFERENCES provider_entries(source_id, external_id) ON DELETE CASCADE
);
CREATE VIEW library_locations AS
    SELECT l.id AS location_id,l.file_id,l.path,l.name,e.root_id,e.availability
    FROM locations l JOIN root_entries e ON e.location_id=l.id
    UNION ALL
    SELECT l.id,l.file_id,l.path,l.name,NULL,a.availability
    FROM locations l JOIN provider_access a ON a.location_id=l.id;
CREATE VIEW provider_display AS
    SELECT l.file_id,
    CASE WHEN count(DISTINCT e.title)=1 THEN max(e.title) END AS title,
    CASE WHEN count(DISTINCT e.game)=1 THEN max(e.game) END AS game,
    CASE WHEN count(DISTINCT e.created_ms)=1 THEN max(e.created_ms) END AS created_ms,
    count(DISTINCT e.title)>1 OR count(DISTINCT e.game)>1 AS conflict
    FROM provider_links l JOIN provider_entries e
    ON e.source_id=l.source_id AND e.external_id=l.external_id
    WHERE l.relation='primary' GROUP BY l.file_id;
PRAGMA user_version = 2;
