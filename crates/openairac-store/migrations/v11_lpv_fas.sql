-- Migration v11: LPV / LP Final Approach Segment (FAS) data (ARINC 424 PP records)
CREATE TABLE IF NOT EXISTS lpv_fas (
    id TEXT NOT NULL,
    airport_ident TEXT NOT NULL,
    icao_code TEXT NOT NULL,
    approach_ident TEXT NOT NULL,
    runway_ident TEXT NOT NULL,
    ref_path_ident TEXT NOT NULL,
    gnss_channel INTEGER NOT NULL,
    app_type TEXT NOT NULL,
    ltp_latitude REAL NOT NULL,
    ltp_longitude REAL NOT NULL,
    fpap_latitude REAL NOT NULL,
    fpap_longitude REAL NOT NULL,
    bearing_true_deg REAL NOT NULL,
    elevation_ft INTEGER NOT NULL,
    length_offset_m REAL NOT NULL,
    tch_ft REAL NOT NULL,
    gpa_deg REAL NOT NULL,
    source_snapshot_id TEXT NOT NULL,
    valid_from TEXT NOT NULL,
    valid_until TEXT,
    PRIMARY KEY (id, valid_from)
);

CREATE INDEX IF NOT EXISTS idx_lpv_fas_lookup ON lpv_fas (airport_ident, valid_from, valid_until);
