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

1. **`openairac-model`**: Defines strongly typed domain entities (`CanonicalAirport`, `CanonicalRunway`, `CanonicalNavaid`, `CanonicalWaypoint`), strongly-typed IDs (`AirportId`, `NavaidId`), frequency units (`FrequencyKhz`), and temporal source provenance metadata (`SourceSnapshot`, `TemporalValidity`).
2. **`openairac-store`**: Embedded SQLite database (`rusqlite` with foreign keys enabled) managing schema migrations (`v1_init.sql`), source snapshots, world revisions, and temporal entity queries (`query_airports_at`, `query_navaids_at`, `query_waypoints_at`).
3. **`openairac-magnetic`**: Pure-Rust implementation of the official NOAA/NCEI World Magnetic Model 2025 (degree N=12 spherical harmonic expansion). Includes runway magnetic drift analysis comparing true headings against WMM declination to suggest runway redesignations.
4. **`openairac-ingest`**: Abstract `DataProvider` trait separating network fetch, parsing, and database transactions. Includes OurAirports CSV importer (airports, runways, navaids) and an experimental FAA CIFP ARINC 424 fixed-width parser adapter.
5. **`openairac-procedures`**: Core procedure domain models (`Procedure`, `ProcedureTransition`, `ProcedureLeg`) and ARINC 424 `PathTerminator` enum (`IF`, `TF`, `CF`, `DF`, `RF`, `VA`, `VI`, etc.).
6. **`openairac-routing`**: Geodesic WGS84 great-circle distance and initial true bearing calculation (`DirectRoute::between`).
7. **`openairac-export-xplane`**: Database-backed X-Plane 12 dat file exporter (`earth_fix.dat` and `earth_nav.dat` v1200 spec).
8. **`openairac-plugin`**: Native X-Plane 12 C-API plugin exposing `OpenAIRAC_QueryWorldStatus` for reading local SQLite database status without runtime file mutation.
9. **`openairac-cli`**: Command-line interface providing `doctor`, `magnetic`, `magdrift`, `sync`, `status`, and `export xplane` commands.
