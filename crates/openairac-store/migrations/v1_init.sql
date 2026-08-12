-- OpenAIRAC Canonical Temporal World Database Schema (v1_init.sql)
PRAGMA foreign_keys = ON;

-- Source Snapshots table
CREATE TABLE IF NOT EXISTS source_snapshots (
    id TEXT PRIMARY KEY,
    provider TEXT NOT NULL,
    dataset TEXT NOT NULL,
    provider_revision TEXT,
    airac_cycle TEXT,
    effective_from TEXT,
    effective_until TEXT,
    retrieved_at TEXT NOT NULL,
    source_uri TEXT NOT NULL,
    content_sha256 TEXT NOT NULL,
    license_notes TEXT,
    parser_version TEXT NOT NULL
);

-- World Revisions table
CREATE TABLE IF NOT EXISTS world_revisions (
    id TEXT PRIMARY KEY,
    created_at TEXT NOT NULL,
    source_snapshot_id TEXT NOT NULL,
    schema_version TEXT NOT NULL,
    notes TEXT,
    FOREIGN KEY (source_snapshot_id) REFERENCES source_snapshots(id)
);

-- Canonical Airports
CREATE TABLE IF NOT EXISTS airports (
    id TEXT PRIMARY KEY,
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
    FOREIGN KEY (source_snapshot_id) REFERENCES source_snapshots(id)
);

CREATE INDEX IF NOT EXISTS idx_airports_ident ON airports(ident);
CREATE INDEX IF NOT EXISTS idx_airports_valid ON airports(valid_from, valid_until);

-- Canonical Runways
CREATE TABLE IF NOT EXISTS runways (
    id TEXT PRIMARY KEY,
    airport_ident TEXT NOT NULL,
    official_designator TEXT NOT NULL,
    computed_magnetic_designator TEXT NOT NULL,
    true_heading_deg REAL NOT NULL,
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
    FOREIGN KEY (source_snapshot_id) REFERENCES source_snapshots(id)
);

CREATE INDEX IF NOT EXISTS idx_runways_airport ON runways(airport_ident);
CREATE INDEX IF NOT EXISTS idx_runways_designator ON runways(official_designator);

-- Canonical Navaids
CREATE TABLE IF NOT EXISTS navaids (
    id TEXT PRIMARY KEY,
    ident TEXT NOT NULL,
    name TEXT NOT NULL,
    navaid_type TEXT NOT NULL,
    frequency_khz INTEGER NOT NULL,
    latitude_deg REAL NOT NULL,
    longitude_deg REAL NOT NULL,
    elevation_ft REAL,
    associated_airport TEXT,
    magnetic_variation_deg REAL,
    source_snapshot_id TEXT NOT NULL,
    valid_from TEXT NOT NULL,
    valid_until TEXT,
    FOREIGN KEY (source_snapshot_id) REFERENCES source_snapshots(id)
);

CREATE INDEX IF NOT EXISTS idx_navaids_ident ON navaids(ident);
CREATE INDEX IF NOT EXISTS idx_navaids_type ON navaids(navaid_type);

-- Canonical Waypoints/Fixes
CREATE TABLE IF NOT EXISTS waypoints (
    id TEXT PRIMARY KEY,
    ident TEXT NOT NULL,
    name TEXT NOT NULL,
    latitude_deg REAL NOT NULL,
    longitude_deg REAL NOT NULL,
    datum TEXT NOT NULL,
    region TEXT,
    source_snapshot_id TEXT NOT NULL,
    valid_from TEXT NOT NULL,
    valid_until TEXT,
    FOREIGN KEY (source_snapshot_id) REFERENCES source_snapshots(id)
);

CREATE INDEX IF NOT EXISTS idx_waypoints_ident ON waypoints(ident);
