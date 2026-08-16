-- OpenAIRAC Canonical Temporal World Database Schema (v9_runway_width.sql)
-- Changes from v8:
--   * runways.width_ft becomes nullable: FAA CIFP PG records do not
--     publish runway width in a verified field, and OpenAIRAC never
--     fabricates values.

CREATE TABLE runways_v9 (
    id TEXT NOT NULL,
    airport_id TEXT,
    airport_ident TEXT NOT NULL,
    official_designator TEXT NOT NULL,
    computed_magnetic_designator TEXT,
    true_heading_deg REAL,
    length_ft INTEGER NOT NULL,
    width_ft INTEGER,
    surface TEXT,
    le_ident TEXT NOT NULL,
    le_lat REAL NOT NULL,
    le_lon REAL NOT NULL,
    le_elevation_ft REAL,
    he_ident TEXT NOT NULL,
    he_lat REAL NOT NULL,
    he_lon REAL NOT NULL,
    he_elevation_ft REAL,
    source_snapshot_id TEXT NOT NULL,
    valid_from TEXT NOT NULL,
    valid_until TEXT,
    PRIMARY KEY (id, valid_from),
    FOREIGN KEY (source_snapshot_id) REFERENCES source_snapshots(id)
);

INSERT INTO runways_v9
    SELECT id, airport_id, airport_ident, official_designator,
           computed_magnetic_designator, true_heading_deg, length_ft,
           width_ft, surface, le_ident, le_lat, le_lon, le_elevation_ft,
           he_ident, he_lat, he_lon, he_elevation_ft, source_snapshot_id,
           valid_from, valid_until
    FROM runways;

DROP TABLE runways;
ALTER TABLE runways_v9 RENAME TO runways;

CREATE INDEX IF NOT EXISTS idx_runways_airport ON runways(airport_id, valid_from);
CREATE INDEX IF NOT EXISTS idx_runways_valid ON runways(valid_from, valid_until);
