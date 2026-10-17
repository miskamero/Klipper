-- A selected display claim survives later, unresolved source disagreements.
CREATE TABLE provider_choices (
    file_id INTEGER NOT NULL REFERENCES recordings(id) ON DELETE CASCADE,
    field TEXT NOT NULL CHECK(field IN ('title','game','created_ms','preview')),
    source_id INTEGER NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
    external_id TEXT NOT NULL,
    value_json TEXT NOT NULL CHECK(json_valid(value_json)),
    revision TEXT NOT NULL,
    PRIMARY KEY(file_id,field)
);
-- Preserve meaningful values already visible before this migration. Ambiguous fields stay unset.
INSERT INTO provider_choices
SELECT p.file_id,'title',e.source_id,e.external_id,json_quote(p.title),'migration-4'
FROM provider_display p JOIN provider_links l ON l.file_id=p.file_id AND l.relation='primary'
JOIN provider_entries e ON e.source_id=l.source_id AND e.external_id=l.external_id AND e.title=p.title
WHERE p.title IS NOT NULL AND NOT EXISTS(SELECT 1 FROM provider_entries prior JOIN provider_links pl ON pl.source_id=prior.source_id AND pl.external_id=prior.external_id WHERE pl.file_id=p.file_id AND pl.relation='primary' AND prior.title=p.title AND (prior.source_id,prior.external_id)<(e.source_id,e.external_id)) GROUP BY p.file_id;
INSERT INTO provider_choices
SELECT p.file_id,'game',e.source_id,e.external_id,json_quote(p.game),'migration-4'
FROM provider_display p JOIN provider_links l ON l.file_id=p.file_id AND l.relation='primary'
JOIN provider_entries e ON e.source_id=l.source_id AND e.external_id=l.external_id AND e.game=p.game
WHERE p.game IS NOT NULL AND NOT EXISTS(SELECT 1 FROM provider_entries prior JOIN provider_links pl ON pl.source_id=prior.source_id AND pl.external_id=prior.external_id WHERE pl.file_id=p.file_id AND pl.relation='primary' AND prior.game=p.game AND (prior.source_id,prior.external_id)<(e.source_id,e.external_id)) GROUP BY p.file_id;
INSERT INTO provider_choices
SELECT p.file_id,'created_ms',e.source_id,e.external_id,json_quote(p.created_ms),'migration-4'
FROM provider_display p JOIN provider_links l ON l.file_id=p.file_id AND l.relation='primary'
JOIN provider_entries e ON e.source_id=l.source_id AND e.external_id=l.external_id AND e.created_ms=p.created_ms
WHERE p.created_ms IS NOT NULL AND NOT EXISTS(SELECT 1 FROM provider_entries prior JOIN provider_links pl ON pl.source_id=prior.source_id AND pl.external_id=prior.external_id WHERE pl.file_id=p.file_id AND pl.relation='primary' AND prior.created_ms=p.created_ms AND (prior.source_id,prior.external_id)<(e.source_id,e.external_id)) GROUP BY p.file_id;
WITH candidates AS (
SELECT l.file_id,p.source_id,p.external_id,json_array(p.path,p.revision,l.file_size,l.file_modified_ns) AS value_json,
row_number() OVER(PARTITION BY l.file_id ORDER BY p.source_id,p.external_id,p.relation) AS preference
FROM provider_previews p JOIN provider_links l ON l.source_id=p.source_id AND l.external_id=p.external_id AND l.relation=p.relation
JOIN recordings f ON f.id=l.file_id WHERE l.file_size=f.size AND l.file_modified_ns IS f.modified_ns
)
INSERT INTO provider_choices
SELECT c.file_id,'preview',c.source_id,c.external_id,c.value_json,'migration-4' FROM candidates c
WHERE c.preference=1 AND NOT EXISTS(SELECT 1 FROM candidates other WHERE other.file_id=c.file_id AND other.value_json<>c.value_json);
DROP VIEW provider_display;
CREATE VIEW provider_display AS
SELECT l.file_id,
coalesce((SELECT json_extract(value_json,'$') FROM provider_choices c WHERE c.file_id=l.file_id AND c.field='title'),CASE WHEN count(DISTINCT e.title)=1 THEN max(e.title) END) AS title,
coalesce((SELECT json_extract(value_json,'$') FROM provider_choices c WHERE c.file_id=l.file_id AND c.field='game'),CASE WHEN count(DISTINCT e.game)=1 THEN max(e.game) END) AS game,
coalesce((SELECT json_extract(value_json,'$') FROM provider_choices c WHERE c.file_id=l.file_id AND c.field='created_ms'),CASE WHEN count(DISTINCT e.created_ms)=1 THEN max(e.created_ms) END) AS created_ms,
count(DISTINCT e.title)>1 OR count(DISTINCT e.game)>1 AS conflict
FROM provider_links l JOIN provider_entries e ON e.source_id=l.source_id AND e.external_id=l.external_id
WHERE l.relation='primary' GROUP BY l.file_id;
PRAGMA user_version=4;
