# OpenAIRAC Data Sources & Provenance

## Data License & Provenance Policy

OpenAIRAC strictly separates application source code (MIT License) from ingested third-party aviation data. Ingesting navigation data into OpenAIRAC does not change the license of the underlying data.

Every ingested dataset is recorded in SQLite under `source_snapshots` with:
- `provider`: Data provider identifier (e.g. `OurAirports`, `FAA_CIFP`)
- `dataset`: Name of dataset (e.g. `airports`, `runways`, `navaids`)
- `provider_revision`: Revision date or AIRAC cycle if applicable
- `retrieved_at`: ISO timestamp of ingestion
- `source_uri`: Exact URI data was retrieved from
- `content_sha256`: SHA-256 hash of raw dataset content
- `license_notes`: License classification and terms

## Ingested Data Sources (v0.3)

### 1. OurAirports (Public Domain / CC0)
- **Datasets**: `airports.csv`, `runways.csv`, `navaids.csv`
- **Coverage**: Worldwide open airport, runway end, and navaid data
- **Status**: Implemented end-to-end (live HTTP fetch + transactional ingest).
  Ingest is fail-closed: records with invalid coordinates are rejected,
  composite facilities not yet representable (NDB-DME) are quarantined,
  and runways whose airport is missing from the store are quarantined.
- **Known gap**: OurAirports navaids carry no ICAO region code, so the
  X-Plane exporter cannot emit enroute navaid rows for them (records are
  skipped with diagnostics instead of writing an invalid region).

### 2. NOAA / NCEI World Magnetic Model 2025 (Public Domain)
- **Datasets**: `WMM2025.COF` coefficients (2025.0–2030.0 epoch);
  official test vectors from
  https://www.ncei.noaa.gov/sites/default/files/2025-02/WMM2025_TEST_VALUES.txt
- **Coverage**: Global geomagnetic field components and declination
- **Status**: Implemented end-to-end; solver verified against all 12 official
  NOAA test vectors (X/Y/Z/H/F within 0.3 nT, I/D within 0.02°).

### 3. FAA CIFP / ARINC 424 (Public Domain / US Government Work)
- **Datasets**: FAA Coded Instrument Flight Procedures (FAACIFP18,
  https://aeronav.faa.gov/Upload_313-d/cifp/)
- **Coverage**: US airspace waypoints, navaids, airways, procedures
- **Status**: Implemented at the library level
  (`openairac_ingest::faa_cifp::ingest_cifp`), golden-tested against real
  cycle 2608 records cross-checked with convert424toxplane v12.4 output.
  Supported record classes:
  - `EA` enroute waypoints,
  - `D` VHF navaids (VOR, VOR-DME, VORTAC, DME-only, TACAN-only, ILS
    localizers and their DME-ILS components), `DB`/`PN` NDBs,
  - `ER` airway records chained into airway segments,
  - `PA` terminal airports, `PG` terminal runways,
  - `PC` terminal waypoints (parent airport attached as
    `terminal_area_ident`),
  - `PD`/`PE`/`PF` SID/STAR/approach procedure legs with the full
    verified column map (recommended navaid, arc radius, course/distance
    pairs, altitude bands, speed limits, MSA center fix, route
    qualifiers). The raw record is always preserved (`raw` column).
  Everything else is preserved raw and reported as unsupported.
- **Known gaps**:
  - ILS localizer bearings/glideslope angles are not decodable from `D`
    records; they live in `PF` approach records. The exporter therefore
    still refuses ILS rows until PF data is joined in (planned v0.5).
  - CIFP ingestion is not yet wired into the CLI `sync` command
    (library-level only).
  - Terminal record support is decoding/verification level: procedure
    geometry rendering (RF arcs etc.) is not implemented.

## Not yet ingested

- ICAO AIP / worldwide procedure sources (regional coverage expansion,
  roadmap v0.6).
- MSFS 2024 navdata inputs (roadmap v0.7).
