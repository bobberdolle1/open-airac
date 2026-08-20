-- Migration v13: Grid MORA terrain clearance (ARINC 424 AS records)
CREATE TABLE IF NOT EXISTS mora (
    id TEXT NOT NULL,
    start_latitude TEXT NOT NULL,
    start_longitude TEXT NOT NULL,
    mora_values_json TEXT NOT NULL,
    source_snapshot_id TEXT NOT NULL,
    valid_from TEXT NOT NULL,
    valid_until TEXT,
    PRIMARY KEY (id, valid_from)
);
