# ✈️ OpenAIRAC

<div align="center">

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Rust](https://img.shields.io/badge/Language-Rust_2024-orange.svg)](https://www.rust-lang.org/)
[![NOAA WMM2025](https://img.shields.io/badge/Physics-NOAA_WMM2025-blue.svg)](https://www.ncei.noaa.gov/products/world-magnetic-model)
[![X-Plane 12](https://img.shields.io/badge/Simulator-X--Plane_12-blue)](https://www.x-plane.com/)
[![CI](https://github.com/bobberdolle1/open-airac/actions/workflows/ci.yml/badge.svg)](https://github.com/bobberdolle1/open-airac/actions/workflows/ci.yml)

**OpenAIRAC — free/open Navigraph-class navigation-data infrastructure for flight simulation.**

[Architecture](docs/ARCHITECTURE.md) • [Data Sources](docs/DATA_SOURCES.md) • [Roadmap](docs/ROADMAP.md)

</div>

---

## What OpenAIRAC is

OpenAIRAC is an open navigation-data **engine**: it ingests public
navigation datasets, stores them in a canonical temporal database, and
produces validated simulator navigation data — with the ambition of
being free/open Navigraph-class infrastructure.

## Why temporal navigation data matters

Flight-sim navigation data changes on the real-world 28-day AIRAC
cycle. Traditional tools replace whole datasets by hand. OpenAIRAC
stores every entity as `(id, valid_from)` revisions with source
provenance, so the engine can:

* answer "what was the world on any date" (`world_at(t)`),
* preload the next AIRAC cycle before it becomes effective,
* diff cycles, validate referential integrity, and roll back cleanly,
* never fabricate values: every missing field is skipped with a
  diagnostic, never guessed.

## What actually works today (v0.4)

| Feature | Status | Crate / Component |
| :--- | :---: | :--- |
| **NOAA WMM2025 Geomagnetic Field Solver** | **Implemented** | `openairac-magnetic` |
| **Runway Magnetic Drift Analysis** | **Implemented** | `openairac-magnetic` |
| **Canonical Domain Model & Provenance** | **Implemented** | `openairac-model` |
| **Temporal SQLite Store (revisioned `world_at`, schema v8)** | **Implemented** | `openairac-store` |
| **Transactional, Fail-Closed Ingestion + Diagnostics** | **Implemented** | `openairac-ingest` |
| **OurAirports Ingestion (live fetch: Airports/Runways/Navaids)** | **Implemented** | `openairac-ingest` |
| **FAA CIFP ARINC 424 Adapter (EA/D/DB/PN/ER + PA/PG/PC/PD/PE/PF)** | **Implemented** | `openairac-ingest` |
| **Canonical Airway Routing Graph (Dijkstra/A\*, MEA/cruise filters, exclusions)** | **Implemented** | `openairac-routing` |
| **SID / STAR / Approach Semantic Layer (ARINC 424 path terminators)** | **Implemented** | `openairac-procedures` |
| **Flight-Plan Integration (airport → SID → enroute → STAR → approach)** | **Implemented** | `openairac-integration` |
| **WorldQuery Service API (world_at / search / nearby / airways / procedures / plan / reconcile)** | **Implemented** | `openairac-service` |
| **AIRAC Cycle Catalog & Discovery (confirmed effective dates, fail-closed)** | **Implemented** | `openairac-store` / `openairac-ingest` |
| **Preload / Observe / Atomic Publication Application** | **Implemented** | `openairac-store` |
| **Differential Publications, Corrections & Tombstones** | **Implemented** | `openairac-store` / `openairac-ingest` |
| **Cycle Rollback (re-publication, immutable history)** | **Implemented** | `openairac-store` |
| **Multi-Source Entity Reconciliation (canonical identities, conflicts, resolved view)** | **Implemented** | `openairac-reconcile` |
| **Deterministic Data Bundles + Local Update Channel** | **Implemented** | `openairac-bundle` |
| **X-Plane 12 dat Exporter (`earth_fix`/`earth_nav`/`earth_awy`, staged & fail-closed)** | **Diagnostic** | `openairac-export-xplane` |

### Data sources that work today

* **OurAirports** — worldwide airports, runways, navaids (live HTTP
  fetch or offline fixtures), via the CLI (`openairac sync`).
* **FAA CIFP / ARINC 424** (cycle 2608 verified) — US airspace enroute
  and terminal records: waypoints (`EA`), VHF navaids (`D`/`DB`/`PN`),
  airways (`ER`), terminal airports/runways/waypoints
  (`PA`/`PG`/`PC`), and SID/STAR/approach legs (`PD`/`PE`/`PF`).
  Decoding is implemented and golden-tested at the library level
  (`openairac_ingest::faa_cifp::ingest_cifp`); CLI wiring:
  `openairac cycle discover` + `openairac sync --provider faa_cifp
  --cycle <id>` (cycle must be catalogued and its effective date
  confirmed). PA/PG terminal airports/runways are still explicit
  Unsupported records (v0.5 scope).

### Simulator output that works today

* **X-Plane 12** — `earth_fix.dat`, `earth_nav.dat`, `earth_awy.dat`
  per Laminar XPFIX1200/XPNAV1200/XPAWY1101 specs, with a checksummed
  `manifest.json` and staged sequential swap. The native exporter is a
  **diagnostic/validation tool**: it never fabricates values and does
  not install into a live simulator. The production path for complete
  X-Plane data (FMC procedures via `earth_424.dat`) is documented in
  [`docs/X_PLANE_STRATEGY.md`](docs/X_PLANE_STRATEGY.md).

### What is experimental

* The native X-Plane exporter's enroute layer (US coverage only,
  diagnostic use).
* CIFP terminal data end-to-end (decoding verified; not yet joined
  into simulator output).

### What is planned

* FAA PA/PG terminal airports/runways decoding, ILS associations,
  procedure geometry (RF arcs, holds) and remaining ARINC semantics
  (v0.5).
* Worldwide provider architecture and regional coverage (v0.6).
* MSFS 2024 navdata packager (v0.7).

### What is NOT production ready

* **No live simulator installation** — the exporter writes to a
  scratch directory only; it is not a drop-in Navigraph replacement
  yet.
* **No worldwide coverage** — CIFP covers the US; OurAirports has no
  procedure data.
* **No signed production bundles** — bundles are UnsignedDevelopment
  until a release trust root exists (signature interface designed).
* Flight Deck / EFB is a **separate future product**, after the engine
  is mature (see roadmap).

---

## 🛠️ Workspace Architecture

```text
crates/
├── openairac-model/          # Canonical domain entities, strongly typed IDs, provenance
├── openairac-magnetic/       # NOAA WMM2025 geomagnetic solver & runway drift engine
├── openairac-store/          # Temporal SQLite store with schema migrations
├── openairac-ingest/         # DataProvider abstraction, OurAirports & FAA CIFP parsers
├── openairac-procedures/     # ARINC 424 path-terminator semantic layer
├── openairac-routing/        # Canonical airway graph (Dijkstra/A*) + geodesics
├── openairac-integration/    # Full flight-planning (procedures + enroute join)
├── openairac-service/        # WorldQuery: UI-independent query boundary
├── openairac-export-xplane/  # X-Plane 12 navdata exporter (diagnostic path)
├── openairac-plugin/         # X-Plane 12 C-ABI plugin for live SQLite status querying
└── openairac-cli/            # Command-line interface
```

---

## 🚀 Usage & CLI Commands

### 1. Perform System & Database Health Check
```bash
openairac doctor --db ./data/world.openairac.sqlite
```

### 2. Calculate WMM2025 Magnetic Field & Variation
```bash
openairac magnetic --lat 80.0 --lon 0.0 --alt-ft 0 --date 2025-01-01
```

### 3. Inspect Runway Magnetic Drift
```bash
openairac magdrift --designator "09" --heading 96.7 --lat 55.97 --lon 37.41 --date 2026-08-12
```

### 4. Synchronize Navigation Data from OurAirports (live network fetch)
```bash
openairac sync --provider ourairports --db ./data/world.openairac.sqlite
```
Use `--fixture` for an offline sample dataset (CI / smoke testing).
FAA CIFP ingestion is available at the library level
(`openairac_ingest::faa_cifp`); CLI wiring is on the roadmap.

### 5. Inspect Database Status & Entity Counts
```bash
openairac status --db ./data/world.openairac.sqlite
```

### 6. Validate Canonical Store Integrity
```bash
openairac validate --db ./data/world.openairac.sqlite
```

### 7. Export X-Plane 12 Navigation Data (diagnostic)
```bash
openairac export xplane --db ./data/world.openairac.sqlite --out ./dist/xplane
```
The exporter is fail-closed and layer-aware: it writes `earth_fix.dat`,
`earth_nav.dat` and `earth_awy.dat` plus a checksummed `manifest.json`,
stages them and swaps them into place file-by-file (the multi-file swap is
not atomic as a set; a transactional backup/rollback install is designed
but intentionally not exposed). Records missing fields the X-Plane format
requires (ICAO region, elevation, slaved variation, service class, waypoint
type, localizer bearings, airway level, altitudes or endpoint references)
are skipped with diagnostics instead of being fabricated. An incomplete
layer — including a missing or empty `earth_awy.dat`, which breaks
X-Plane's referential integrity — is refused unless `--allow-empty` is
passed. **OpenAIRAC does not yet install into a live simulator
installation**; the output is for validation and testing. Row conventions
are cross-checked against Laminar convert424toxplane v12.4 output for the
same FAA CIFP input.
---

## 📄 License

Distributed under the **MIT License**. See [`LICENSE`](LICENSE) for details.
