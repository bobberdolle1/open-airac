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
- **Status**: Implemented end-to-end

### 2. NOAA / NCEI World Magnetic Model 2025 (Public Domain)
- **Datasets**: `WMM2025.COF` coefficients (2025.0–2030.0 epoch)
- **Coverage**: Global geomagnetic field components and declination
- **Status**: Implemented end-to-end

### 3. FAA CIFP / ARINC 424 (Public Domain / US Government Work)
- **Datasets**: FAA Coded Instrument Flight Procedures
- **Coverage**: US airspace waypoints, navaids, airways, procedures
- **Status**: Experimental fixed-width parser adapter for waypoints and navaids
