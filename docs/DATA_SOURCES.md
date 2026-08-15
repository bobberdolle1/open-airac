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

## Ingested Data Sources (v0.2)

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
- **Status**: Experimental. Layered decoder (fixed-width → raw records →
  canonical entities) with explicit unsupported-record reporting.
  Supported today: `EA` enroute waypoints, `D` VHF navaids (VOR, VOR-DME,
  VORTAC, DME-only, TACAN-only, ILS localizers), `DB`/`PN` NDBs.
  Everything else is preserved raw and reported as unsupported.
- **Known gaps**: ILS localizer bearings are not decoded from `D` records
  (export of ILS rows is therefore refused until a bearing source exists);
  terminal waypoints/procedures (PD/PE/PF) are future work.
