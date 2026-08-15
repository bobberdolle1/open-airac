-- OpenAIRAC Canonical Temporal World Database Schema (v4_procedure_legs.sql)
-- Changes from v3:
--   * waypoints gain terminal_area_ident (ARINC 5.6 for PC terminal
--     waypoints).
--   * New procedure_legs table: canonical ARINC 424 PD/PE/PF records with
--     lossless raw preservation.

ALTER TABLE waypoints ADD COLUMN terminal_area_ident TEXT;

CREATE TABLE IF NOT EXISTS procedure_legs (
    id TEXT NOT NULL,
    airport_ident TEXT NOT NULL,
    icao_code TEXT NOT NULL,
    procedure_kind TEXT NOT NULL,
    procedure_ident TEXT NOT NULL,
    route_type TEXT NOT NULL,
    transition_ident TEXT NOT NULL,
    sequence_number INTEGER NOT NULL,
    fix_ident TEXT NOT NULL,
    fix_icao_code TEXT NOT NULL,
    fix_section TEXT NOT NULL,
    waypoint_description TEXT NOT NULL,
    turn_direction TEXT,
    rnp_nm REAL,
    path_terminator TEXT NOT NULL,
    recommended_navaid TEXT,
    arc_radius_nm REAL,
    course_a_deg REAL,
    distance_a_nm REAL,
    course_b_deg REAL,
    distance_b_nm REAL,
    altitude_descriptor TEXT,
    altitude_1_ft INTEGER,
    altitude_2_ft INTEGER,
    speed_limit_kts INTEGER,
    course_c_deg INTEGER,
    vertical_angle_deg REAL,
    msa_center_fix TEXT,
    route_qualifiers TEXT NOT NULL,
    raw TEXT NOT NULL,
    source_snapshot_id TEXT NOT NULL,
    valid_from TEXT NOT NULL,
    valid_until TEXT,
    PRIMARY KEY (id, valid_from),
    FOREIGN KEY (source_snapshot_id) REFERENCES source_snapshots(id)
);

CREATE INDEX IF NOT EXISTS idx_procedure_legs_airport
    ON procedure_legs(airport_ident, procedure_ident, transition_ident, sequence_number);
