# OpenAIRAC Architecture Specification (v0.2 Foundation)

## Overview

OpenAIRAC is an open navigation-data engine designed to provide continuously maintainable navigation data for flight simulation without relying on rigid manual monthly AIRAC cycle overwrites.

## Dependency Direction & Core Boundaries

```text
[Data Providers (OurAirports / FAA CIFP)]
                    │
                    ▼
[Canonical Navigation Model (openairac-model)]
                    │
                    ▼
[Temporal Database Store (openairac-store)]
                    │
                    ├──► [Geomagnetic Physics Engine (openairac-magnetic)]
                    ├──► [Geodesic Routing Engine (openairac-routing)]
                    └──► [Procedure Representation (openairac-procedures)]
                    │
                    ▼
[Exporters / CLI / Plugins (openairac-export-xplane, openairac-cli, openairac-plugin)]
```

## Crate Layout & Responsibilities

1. **`openairac-model`**: Strongly typed domain entities (`CanonicalAirport`, `CanonicalRunway`, `CanonicalNavaid`, `CanonicalWaypoint`), strongly-typed IDs (`AirportId`, `RunwayId`, `NavaidId`, `WaypointId`, `SourceSnapshotId`, `WorldRevisionId`), frequency units (`FrequencyKhz`), and temporal source provenance (`SourceSnapshot` incl. `license_id`, `TemporalValidity`).
2. **`openairac-store`**: Embedded SQLite database (foreign keys + WAL) with versioned migrations (`PRAGMA user_version`). Entity tables are keyed by `(id, valid_from)`: every ingestion appends a new temporal revision and closes the previous one (`valid_until` exclusive), so previous/current/future worlds are simultaneously queryable via `query_*_at(timestamp)`. Writers live at connection level so a whole ingestion runs in one transaction (`WorldStore::transact`). `validate()` performs structural integrity checks (provenance FK, runway→airport references, coordinate/temporal ranges, frequency bands, overlapping revisions).
3. **`openairac-magnetic`**: Pure-Rust implementation of the official NOAA/NCEI World Magnetic Model 2025 (degree N=12). Golden-tested against all 12 official NOAA WMM2025 test vectors (0.3 nT / 0.02°). Runway magnetic drift analysis is strictly separated: published designators are never overwritten by WMM predictions.
4. **`openairac-ingest`**: `DataProvider` trait (fetch + transactional parse/ingest), deterministic `IngestReport` diagnostics (seen/parsed/created/updated/unchanged/quarantined/rejected/warnings/errors/duration/checksum). OurAirports CSV importer (live HTTP fetch, fail-closed validation). FAA CIFP adapter with a layered pipeline: fixed-width decoding → raw ARINC 424 records → semantic interpretation → canonical entities; unsupported record types are explicit and their raw lines preserved.
5. **`openairac-procedures`**: Core procedure domain models (`Procedure`, `ProcedureTransition`, `ProcedureLeg`) and the full ARINC 424 `PathTerminator` enum; unknown terminators are preserved via `Unsupported(String)`. Leg interpretation is planned (v0.3).
6. **`openairac-routing`**: Geodesic WGS84 distance/bearing (`DirectRoute::between`) with validated `Coordinate`; graph policy is kept separate from geodesic math.
7. **`openairac-export-xplane`**: X-Plane 12 `earth_fix.dat` (XPFIX1200) and `earth_nav.dat` (XPNAV1200) exporter, implemented against Laminar's published specifications. Fail-closed: rows missing required fields are skipped with diagnostics, files are generated in a staging directory and swapped in atomically, and empty-layer exports are refused unless `--allow-empty`.
8. **`openairac-plugin`**: Native X-Plane 12 C-API plugin exposing `OpenAIRAC_QueryWorldStatus` for reading local SQLite database status without runtime file mutation.
9. **`openairac-cli`**: `doctor`, `magnetic`/`magvar`, `magdrift`, `sync` (live OurAirports fetch or `--fixture`), `status`, `validate`, `export xplane`.
