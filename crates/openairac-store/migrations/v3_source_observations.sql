-- OpenAIRAC Canonical Temporal World Database Schema (v3_source_observations.sql)
-- Changes from v2:
--   * Navaids gain source-provided XPNAV1200 fields: slaved variation,
--     service volume/class, and a paired-DME flag (rows 12 vs 13).
--   * New entity_observations table: every accepted record observation is
--     logged per source snapshot, separating entity payload revision from
--     source-observation/provenance. Re-ingesting an unchanged entity does
--     NOT create a new payload revision.
--   * New airway_legs table for ARINC 424 ER airway segments.

ALTER TABLE navaids ADD COLUMN slaved_variation_deg REAL;
ALTER TABLE navaids ADD COLUMN service_volume_nm INTEGER;
ALTER TABLE navaids ADD COLUMN dme_paired INTEGER NOT NULL DEFAULT 0;

CREATE TABLE IF NOT EXISTS entity_observations (
    source_snapshot_id TEXT NOT NULL,
    entity_table TEXT NOT NULL,
    entity_id TEXT NOT NULL,
    valid_from TEXT NOT NULL,
    PRIMARY KEY (source_snapshot_id, entity_table, entity_id, valid_from),
    FOREIGN KEY (source_snapshot_id) REFERENCES source_snapshots(id)
);

CREATE INDEX IF NOT EXISTS idx_entity_observations_entity
    ON entity_observations(entity_table, entity_id);

CREATE TABLE IF NOT EXISTS airway_legs (
    id TEXT NOT NULL,
    route_ident TEXT NOT NULL,
    route_type TEXT NOT NULL,
    level TEXT,
    sequence_number INTEGER NOT NULL,
    start_fix TEXT NOT NULL,
    start_icao_code TEXT NOT NULL,
    end_fix TEXT NOT NULL,
    end_icao_code TEXT NOT NULL,
    direction TEXT NOT NULL,
    minimum_altitude_ft INTEGER,
    maximum_altitude_ft INTEGER,
    source_snapshot_id TEXT NOT NULL,
    valid_from TEXT NOT NULL,
    valid_until TEXT,
    PRIMARY KEY (id, valid_from),
    FOREIGN KEY (source_snapshot_id) REFERENCES source_snapshots(id)
);

CREATE INDEX IF NOT EXISTS idx_airway_legs_route ON airway_legs(route_ident, sequence_number);
