-- OpenAIRAC Canonical Temporal World Database Schema (v8_conflict_dedup.sql)
-- Changes from v7:
--   * reconciliation_conflicts gains conflict_key: a stable, normalized
--     dedup key (entity_table + canonical-or-empty + ORDERED source
--     refs + category + field-or-empty). SQLite UNIQUE indexes treat
--     NULLs as distinct, so the previous nullable-column dedup index
--     could not deduplicate ambiguity conflicts (canonical_id = NULL).
--     The key is computed in Rust; legacy rows are backfilled by the
--     migration code in WorldStore::migrate.

ALTER TABLE reconciliation_conflicts ADD COLUMN conflict_key TEXT;

DROP INDEX IF EXISTS idx_reconciliation_conflicts_dedup;

CREATE UNIQUE INDEX IF NOT EXISTS idx_reconciliation_conflicts_key
    ON reconciliation_conflicts(conflict_key)
    WHERE conflict_key IS NOT NULL;
