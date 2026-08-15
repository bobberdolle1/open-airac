-- OpenAIRAC Canonical Temporal World Database Schema (v5_airac_lifecycle.sql)
-- Changes from v4:
--   * airac_cycles: the cycle catalog (metadata only; temporal data rows
--     remain the single source of truth for world_at(t)).
--   * cycle_snapshots: cycles <-> source snapshots (M:N).
--   * cycle_events: append-only audit/journal of schedule/observe/rollback
--     intents and facts. Deliberately carries no world_revision_id:
--     rollback re-publishes rows from multiple historical snapshots and
--     per-row provenance lives in those rows' own source_snapshot_id.
--   * dataset_versions: append-only observed dataset publications with
--     revision_kind (Baseline/Correction) and coverage
--     (FullSnapshot/Partial); close_absent semantics depend on coverage
--     alone.
--   * entity_aliases: natural-key identity index for source
--     reconciliation reporting.

CREATE TABLE IF NOT EXISTS airac_cycles (
    id TEXT PRIMARY KEY,
    effective_from TEXT,
    effective_until TEXT,
    status TEXT NOT NULL,
    source_uri TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    notes TEXT
);

CREATE TABLE IF NOT EXISTS cycle_snapshots (
    cycle_id TEXT NOT NULL,
    source_snapshot_id TEXT NOT NULL,
    PRIMARY KEY (cycle_id, source_snapshot_id),
    FOREIGN KEY (cycle_id) REFERENCES airac_cycles(id),
    FOREIGN KEY (source_snapshot_id) REFERENCES source_snapshots(id)
);

CREATE TABLE IF NOT EXISTS cycle_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    at TEXT NOT NULL,
    kind TEXT NOT NULL,
    cycle_id TEXT NOT NULL,
    restored_cycle_id TEXT,
    notes TEXT,
    FOREIGN KEY (cycle_id) REFERENCES airac_cycles(id),
    FOREIGN KEY (restored_cycle_id) REFERENCES airac_cycles(id)
);

CREATE INDEX IF NOT EXISTS idx_cycle_events_cycle ON cycle_events(cycle_id, at);

CREATE TABLE IF NOT EXISTS dataset_versions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    provider TEXT NOT NULL,
    dataset TEXT NOT NULL,
    airac_cycle TEXT,
    content_sha256 TEXT NOT NULL,
    retrieved_at TEXT NOT NULL,
    revision_kind TEXT NOT NULL,
    coverage TEXT NOT NULL,
    notes TEXT
);

CREATE INDEX IF NOT EXISTS idx_dataset_versions_lookup
    ON dataset_versions(provider, dataset, airac_cycle, retrieved_at);

CREATE TABLE IF NOT EXISTS entity_aliases (
    entity_table TEXT NOT NULL,
    natural_key TEXT NOT NULL,
    provider TEXT NOT NULL,
    entity_id TEXT NOT NULL,
    PRIMARY KEY (entity_table, natural_key, provider)
);
