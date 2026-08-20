-- Migration v12: Minimum Sector Altitude (MSA / Section PS records)
CREATE TABLE IF NOT EXISTS msa (
    id TEXT NOT NULL,
    airport_ident TEXT NOT NULL,
    icao_code TEXT NOT NULL,
    center_fix TEXT NOT NULL,
    center_icao_code TEXT NOT NULL,
    center_section TEXT NOT NULL,
    fix_type INTEGER NOT NULL,
    magnetic_true_indicator TEXT NOT NULL,
    sectors_json TEXT NOT NULL,
    source_snapshot_id TEXT NOT NULL,
    valid_from TEXT NOT NULL,
    valid_until TEXT,
    PRIMARY KEY (id, valid_from)
);

CREATE INDEX IF NOT EXISTS idx_msa_lookup ON msa (airport_ident, valid_from, valid_until);
