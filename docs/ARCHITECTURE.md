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
2. **`openairac-store`**: Embedded SQLite database (foreign keys + WAL) with versioned migrations (`PRAGMA user_version`, currently v3). Entity tables are keyed by `(id, valid_from)`: every ingestion appends a new temporal revision and closes the previous one (`valid_until` exclusive), so previous/current/future worlds are simultaneously queryable via `query_*_at(timestamp)`. **Payload revision is separated from provenance**: the payload comparison excludes the source snapshot id, so a new dataset snapshot does not re-revise unchanged entities; every accepted observation is logged in `entity_observations` instead. Preloaded future revisions can be corrected (same `valid_from`, replaced before becoming effective); effective history is immutable. Writers live at connection level so a whole ingestion runs in one transaction (`WorldStore::transact`). `validate()` performs structural integrity checks (provenance FK, runway→airport references, coordinate/temporal ranges, frequency bands, overlapping revisions).
3. **`openairac-magnetic`**: Pure-Rust implementation of the official NOAA/NCEI World Magnetic Model 2025 (degree N=12). Golden-tested against all 12 official NOAA WMM2025 test vectors (0.3 nT / 0.02°). Runway magnetic drift analysis is strictly separated: published designators are never overwritten by WMM predictions.
4. **`openairac-ingest`**: `DataProvider` trait (fetch + transactional parse/ingest), deterministic `IngestReport` diagnostics (seen/parsed/created/updated/unchanged/quarantined/rejected/warnings/errors/duration/checksum). OurAirports CSV importer (live HTTP fetch, fail-closed validation). FAA CIFP adapter with a layered pipeline: fixed-width decoding → raw ARINC 424 records → semantic interpretation → canonical entities; unsupported record types are explicit and their raw lines preserved.
5. **`openairac-procedures`**: Core procedure domain models (`Procedure`, `ProcedureTransition`, `ProcedureLeg`) and the full ARINC 424 `PathTerminator` enum; unknown terminators are preserved via `Unsupported(String)`. Leg interpretation is planned (v0.3).
6. **`openairac-routing`**: Geodesic WGS84 distance/bearing (`DirectRoute::between`) with validated `Coordinate`; graph policy is kept separate from geodesic math.
7. **`openairac-export-xplane`**: X-Plane 12 exporter implementing Laminar's XPFIX1200 / XPNAV1200 / XPAWY1101 specifications; row conventions are cross-checked against Laminar convert424toxplane v12.4 output on the same FAA CIFP input. Emits `earth_fix.dat` (VOR-DME/VORTAC/ILS facilities include their paired DME rows), `earth_nav.dat`, `earth_awy.dat` (endpoint typing 11/2/3 with referential integrity against the actually-exported fix/nav entities, airway-name merging) and a checksummed `manifest.json`. Fail-closed: rows missing source-provided values are skipped with diagnostics (stricter than convert424toxplane, which defaults unknown elevations to 0); files are STAGED and then swapped file-by-file — the multi-file swap is not atomic as a set (a crash mid-swap can leave a mixed layer); incomplete layers are refused without `--allow-empty`. This is NOT a globally installable layer yet and the CLI does not install into live simulators — the transactional backup/rollback install is designed (`InstallPlan`, manifest) but intentionally not implemented or exposed.
   - **Pipeline decision (investigated)**: Laminar's convert424toxplane v12.4 was downloaded and run against the real FAA CIFP cycle 2608 — it produces the complete XP12 file set and is the gold standard for conversion. The long-term preferred final pipeline is a full worldwide ARINC 424 master file fed to convert424toxplane (or shipped as `earth_424.dat`); the native exporter remains the canonical-store path and its fixtures are validated against the tool's output. Our exporter deliberately diverges from the tool where the tool fabricates (NDB elevation 0, synthesized glideslope geometry, blank-level airway defaults): we skip and diagnose instead.
9. **`openairac-cli`**: `doctor`, `magnetic`/`magvar`, `magdrift`, `sync` (live OurAirports fetch or `--fixture`), `status`, `validate`, `export xplane`.
