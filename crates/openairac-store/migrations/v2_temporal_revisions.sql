-- -------------------------------------------------------- source_snapshots
-- v1 predates the license_id column; add it for databases upgraded in place.
ALTER TABLE source_snapshots ADD COLUMN license_id TEXT;

-- OpenAIRAC Canonical Temporal World Database Schema (v2_temporal_revisions.sql)
-- Changes from v1:
--   * Entity tables are keyed by (id, valid_from): multiple temporal revisions
--     of the same entity coexist, so previous / current / future revisions are
--     simultaneously queryable via world_at(timestamp).
--   * valid_until is exclusive: a row is valid while valid_until > t.
--   * Runways reference the canonical airport id.
--   * Waypoints gain is_enroute and the ARINC 424 field 5.42 waypoint type.
--   * Navaids gain ILS-specific fields (runway, localizer bearings, GS angle).

-- ---------------------------------------------------------------- airports
CREATE TABLE airports_v2 (
    id TEXT NOT NULL,
    ident TEXT NOT NULL,
    name TEXT NOT NULL,
    airport_type TEXT NOT NULL,
    latitude_deg REAL NOT NULL,
    longitude_deg REAL NOT NULL,
    elevation_ft REAL,
    iso_country TEXT,
    municipality TEXT,
    source_snapshot_id TEXT NOT NULL,
    valid_from TEXT NOT NULL,
    valid_until TEXT,
    PRIMARY KEY (id, valid_from),
    FOREIGN KEY (source_snapshot_id) REFERENCES source_snapshots(id)
);

INSERT INTO airports_v2
    SELECT id, ident, name, airport_type, latitude_deg, longitude_deg,
           elevation_ft, iso_country, municipality, source_snapshot_id,
           valid_from, valid_until
    FROM airports;

DROP TABLE airports;
ALTER TABLE airports_v2 RENAME TO airports;

CREATE INDEX IF NOT EXISTS idx_airports_ident ON airports(ident);
CREATE INDEX IF NOT EXISTS idx_airports_valid ON airports(valid_from, valid_until);

-- ---------------------------------------------------------------- runways
CREATE TABLE runways_v2 (
    id TEXT NOT NULL,
    airport_id TEXT,
    airport_ident TEXT NOT NULL,
    official_designator TEXT NOT NULL,
    computed_magnetic_designator TEXT,
    true_heading_deg REAL,
    length_ft INTEGER NOT NULL,
    width_ft INTEGER NOT NULL,
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

INSERT INTO runways_v2
    SELECT id, NULL, airport_ident, official_designator,
           computed_magnetic_designator, true_heading_deg, length_ft, width_ft,
           surface, le_ident, le_lat, le_lon, le_elevation_ft, he_ident,
           he_lat, he_lon, he_elevation_ft, source_snapshot_id, valid_from,
           valid_until
    FROM runways;

DROP TABLE runways;
ALTER TABLE runways_v2 RENAME TO runways;

CREATE INDEX IF NOT EXISTS idx_runways_airport ON runways(airport_ident);
CREATE INDEX IF NOT EXISTS idx_runways_airport_id ON runways(airport_id);
CREATE INDEX IF NOT EXISTS idx_runways_designator ON runways(official_designator);

-- ---------------------------------------------------------------- navaids
CREATE TABLE navaids_v2 (
    id TEXT NOT NULL,
    ident TEXT NOT NULL,
    name TEXT NOT NULL,
    navaid_type TEXT NOT NULL,
    frequency_khz INTEGER NOT NULL,
    latitude_deg REAL NOT NULL,
    longitude_deg REAL NOT NULL,
    elevation_ft REAL,
    region TEXT,
    associated_airport TEXT,
    magnetic_variation_deg REAL,
    associated_runway TEXT,
    localizer_bearing_true_deg REAL,
    localizer_bearing_mag_deg REAL,
    glideslope_angle_deg REAL,
    source_snapshot_id TEXT NOT NULL,
    valid_from TEXT NOT NULL,
    valid_until TEXT,
    PRIMARY KEY (id, valid_from),
    FOREIGN KEY (source_snapshot_id) REFERENCES source_snapshots(id)
);

INSERT INTO navaids_v2
    SELECT id, ident, name, navaid_type, frequency_khz, latitude_deg,
           longitude_deg, elevation_ft, NULL, associated_airport,
           magnetic_variation_deg, NULL, NULL, NULL, NULL, source_snapshot_id,
           valid_from, valid_until
    FROM navaids;

DROP TABLE navaids;
ALTER TABLE navaids_v2 RENAME TO navaids;

CREATE INDEX IF NOT EXISTS idx_navaids_ident ON navaids(ident);
CREATE INDEX IF NOT EXISTS idx_navaids_type ON navaids(navaid_type);

-- --------------------------------------------------------------- waypoints
CREATE TABLE waypoints_v2 (
    id TEXT NOT NULL,
    ident TEXT NOT NULL,
    name TEXT NOT NULL,
    latitude_deg REAL NOT NULL,
    longitude_deg REAL NOT NULL,
    datum TEXT NOT NULL,
    region TEXT,
    is_enroute INTEGER NOT NULL DEFAULT 1,
    waypoint_type INTEGER,
    source_snapshot_id TEXT NOT NULL,
    valid_from TEXT NOT NULL,
    valid_until TEXT,
    PRIMARY KEY (id, valid_from),
    FOREIGN KEY (source_snapshot_id) REFERENCES source_snapshots(id)
);

INSERT INTO waypoints_v2
    SELECT id, ident, name, latitude_deg, longitude_deg, datum, region, 1,
           NULL, source_snapshot_id, valid_from, valid_until
    FROM waypoints;

DROP TABLE waypoints;
ALTER TABLE waypoints_v2 RENAME TO waypoints;

CREATE INDEX IF NOT EXISTS idx_waypoints_ident ON waypoints(ident);
