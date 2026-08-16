-- OpenAIRAC Canonical Temporal World Database Schema (v10_ils_associations.sql)
-- Changes from v9:
--   * ils_associations: verified ILS approach -> localizer/glideslope
--     associations derived from FAA CIFP PF records. ILS category is
--     NOT published in CIFP and is deliberately absent.

CREATE TABLE IF NOT EXISTS ils_associations (
    airport_ident TEXT NOT NULL,
    icao_code TEXT NOT NULL,
    approach_ident TEXT NOT NULL,
    runway_end TEXT NOT NULL,
    localizer_ident TEXT NOT NULL,
    localizer_region TEXT NOT NULL,
    localizer_bearing_mag_deg REAL NOT NULL,
    glideslope_angle_deg REAL NOT NULL,
    source_snapshot_id TEXT NOT NULL,
    PRIMARY KEY (airport_ident, approach_ident),
    FOREIGN KEY (source_snapshot_id) REFERENCES source_snapshots(id)
);

CREATE INDEX IF NOT EXISTS idx_ils_associations_localizer
    ON ils_associations(localizer_ident, airport_ident);
