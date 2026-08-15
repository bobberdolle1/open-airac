-- OpenAIRAC Canonical Temporal World Database Schema (v6_publications.sql)
-- Changes from v5:
--   * tombstones: first-class removal facts (provider-published), keyed
--     by (entity_table, entity_id, effective_from). Absence in a
--     differential publication is NEVER a tombstone.
--   * dataset_versions gains publication_id (publication identity;
--     same identity + different checksum is a conflict unless the
--     publication is a Correction) and valid_from (the effective
--     instant this publication's entities carry).
--   * publication_applications: per-publication audit of what was
--     applied (rows_closed) — validation uses it to prove differential
--     publications never ran close_absent semantics.

CREATE TABLE IF NOT EXISTS tombstones (
    entity_table TEXT NOT NULL,
    entity_id TEXT NOT NULL,
    effective_from TEXT NOT NULL,
    provider TEXT NOT NULL,
    dataset TEXT NOT NULL,
    source_snapshot_id TEXT NOT NULL,
    reason TEXT,
    PRIMARY KEY (entity_table, entity_id, effective_from),
    FOREIGN KEY (source_snapshot_id) REFERENCES source_snapshots(id)
);

CREATE INDEX IF NOT EXISTS idx_tombstones_entity
    ON tombstones(entity_table, entity_id);

ALTER TABLE dataset_versions ADD COLUMN publication_id TEXT;
ALTER TABLE dataset_versions ADD COLUMN valid_from TEXT;

CREATE INDEX IF NOT EXISTS idx_dataset_versions_publication
    ON dataset_versions(publication_id, retrieved_at);

CREATE TABLE IF NOT EXISTS publication_applications (
    publication_id TEXT NOT NULL,
    valid_from TEXT NOT NULL,
    kind TEXT NOT NULL,
    coverage TEXT NOT NULL,
    rows_closed INTEGER NOT NULL,
    applied_at TEXT NOT NULL,
    PRIMARY KEY (publication_id, valid_from, applied_at)
);
