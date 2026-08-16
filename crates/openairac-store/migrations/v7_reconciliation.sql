-- OpenAIRAC Canonical Temporal World Database Schema (v7_reconciliation.sql)
-- Changes from v6:
--   * canonical_identities: stable canonical reconciliation identities,
--     derived deterministically from natural identity keys. Provider
--     entity ids are NEVER canonical ids.
--   * source_memberships: relationships ABOVE provider-native records,
--     keyed by (provider, entity_table, entity_id, valid_from) — one
--     membership per source revision interval. Provider rows remain
--     independently queryable.
--   * identity_continuity: explicit same-facility links across natural
--     identity changes (airport ident change, runway renumbering).
--   * reconciliation_conflicts: persisted, structured diagnostics —
--     field/identity/geometry/ambiguity — never silent resolution.
--   * entity_aliases (v5) is retained as a lower-level natural-key
--     lookup index, NOT as identity proof.

CREATE TABLE IF NOT EXISTS canonical_identities (
    canonical_id TEXT PRIMARY KEY,
    entity_table TEXT NOT NULL,
    identity_key TEXT NOT NULL,
    kind_hint TEXT,
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS source_memberships (
    canonical_id TEXT NOT NULL,
    provider TEXT NOT NULL,
    entity_table TEXT NOT NULL,
    entity_id TEXT NOT NULL,
    valid_from TEXT NOT NULL,
    valid_until TEXT,
    confidence TEXT NOT NULL,
    match_method TEXT NOT NULL,
    evidence TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'Active',
    PRIMARY KEY (provider, entity_table, entity_id, valid_from),
    FOREIGN KEY (canonical_id) REFERENCES canonical_identities(canonical_id)
);

CREATE INDEX IF NOT EXISTS idx_source_memberships_canonical
    ON source_memberships(canonical_id, entity_table);

CREATE TABLE IF NOT EXISTS identity_continuity (
    prev_canonical_id TEXT NOT NULL,
    next_canonical_id TEXT NOT NULL,
    evidence TEXT NOT NULL,
    PRIMARY KEY (prev_canonical_id, next_canonical_id)
);

CREATE TABLE IF NOT EXISTS reconciliation_conflicts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    entity_table TEXT NOT NULL,
    canonical_id TEXT,
    ref_a TEXT NOT NULL,
    ref_b TEXT NOT NULL,
    category TEXT NOT NULL,
    field_name TEXT,
    value_a TEXT,
    value_b TEXT,
    severity TEXT NOT NULL,
    evidence TEXT NOT NULL,
    detected_at TEXT NOT NULL,
    resolved TEXT
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_reconciliation_conflicts_dedup
    ON reconciliation_conflicts(entity_table, canonical_id, ref_a, ref_b, category, field_name);
