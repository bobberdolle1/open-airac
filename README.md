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

> **OpenAIRAC is for FLIGHT SIMULATION ONLY.** It is not certified,
> not for real-world navigation, and must never be used for planning or
> flying real aircraft. The data sources are public datasets (US
> Government CIFP, OurAirports) that carry no operational guarantees.
> See [docs/SECURITY.md](docs/SECURITY.md) for the signing model and
> [docs/DATA_SOURCES.md](docs/DATA_SOURCES.md) for licensing.

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

## What actually works today (v1.2 worldwide platform)

OpenAIRAC provides a complete worldwide aeronautical data engine:

* **AIRAC cycle lifecycle**: Automated discovery, confirmed effective date validation, preload scheduling, atomic activation, and temporal rollback.
* **Worldwide Provider Federation**: Machine-readable provider registry (`data/providers.yaml`), strict redistribution policy enforcement (`PublicRedistribution`, `LocalOnly`, `MetadataOnly`, `Forbidden`), and cryptographic provenance.
* **Generic AIXM 5.x Ingestion**: Full XML/GML parser for aerodromes, runways, radio navaids, designated points, routes, SIDs, STARs, and instrument approaches.
* **Local / BYOD Ingestion**: CLI workflows (`openairac import aixm <file>`) allowing personal use of official AIP datasets without accidental public redistribution.
* **Worldwide Procedure Validation Layer**: Comprehensive semantic and geometric validator detecting sequence gaps, unresolvable fixes, unbound runways, and geometric discontinuities (> 250 NM).
* **Coverage Inspector & Terminal Doctor**: `openairac coverage [ICAO]` and `openairac doctor-airport <ICAO>` CLI commands with formatted text and machine-readable JSON output.
* **Multi-Simulator Exporter Suite**:
  - **X-Plane 12 / 11** (`earth_fix.dat`, `earth_nav.dat`, `earth_awy.dat`, `earth_hold.dat`, `earth_aptmeta.dat`, `earth_msa.dat`, `earth_mora.dat`) — **SUPPORTED**
  - **Garmin GNS430 / Classic GPS** (`Airports.txt`, `Navaids.txt`, `Waypoints.txt`, `ATS.txt`, `Proc/<ICAO>.txt`) — **SUPPORTED**
  - **Bendix/King KLN90B GPS** (`APT.DAT`, `NAV.DAT`, `WPT.DAT`, `AWY.DAT`, `FAS.DAT`) — **SUPPORTED**
  - **Little Navmap** (SQLite v14.29 schema) — **SUPPORTED**
  - **MSFS 2024 / 2020** (SimpleNavData package layout) — **EXPERIMENTAL**
  - **PMDG classic FMC** (`wpNav*.txt`) — **EXPERIMENTAL**
  - **Aerosoft CRJ / NavDataPro** — **RESEARCH**
* **Deterministic Release Bundles & Update Channels**: Content-addressed bundles, Ed25519 cryptographic signing, and regional bundle filtering (`world-open`, `us`, `europe-open`).

Run the release gate:

```bash
cargo test --workspace
openairac release-gate --db ./data/world.openairac.sqlite --effective 2026-08-13T09:01:00Z --out ./gate
```

### Data sources supported

* **FAA CIFP / ARINC 424** — Full US enroute and terminal procedure semantics.
* **FAA AIXM 5.1** — US NASR AIXM 5.1 aeronautical data.
* **OurAirports** — Worldwide airport, runway, and radio navaid basic metadata (CC0).
* **OpenFlightmaps** — European open VFR and enroute aeronautical data.
* **DFS Germany Open Data** — German AIP dataset (GeoNutzV).
* **Eurocontrol EAD (BYOD)** — European AIS database for authenticated personal use.
* **User BYOD** — Local user-supplied AIXM 5.x and ARINC 424 files.
* **Navigraph / Jeppesen** — Commercial proprietary: strictly **FORBIDDEN** from ingestion or redistribution.

---

## 🛠️ Workspace Architecture

```text
crates/
├── openairac-model/          # Canonical domain entities, provider policies & registry
├── openairac-magnetic/       # NOAA WMM2025 geomagnetic solver & runway drift engine
├── openairac-store/          # Temporal SQLite store with schema migrations (v1-v13)
├── openairac-ingest/         # CIFP ARINC 424, Generic AIXM 5.x & OurAirports parsers
├── openairac-procedures/     # ARINC path terminators & Worldwide Procedure Validator
├── openairac-routing/        # Canonical airway routing graph (Dijkstra/A*) + geodesics
├── openairac-integration/    # Full flight-planning join (procedures + enroute graph)
├── openairac-service/        # WorldQuery API: coverage inspector & terminal doctor
├── openairac-bundle/         # Deterministic content-addressed bundles & signing
├── openairac-export/         # Generic export architecture, target registry & installer
├── openairac-export-xplane/  # X-Plane 12 complete dataset exporter
├── openairac-export-msfs/    # MSFS BGL package exporter
├── openairac-export-lnm/     # Little Navmap SQLite database exporter
├── openairac-export-pmdg/    # PMDG classic FMC text exporter
├── openairac-plugin/         # X-Plane 12 C-ABI plugin for live SQLite status querying
└── openairac-cli/            # Unified CLI interface
```

---

## 🚀 CLI Commands

### 1. Coverage Inspector & Terminal Doctor
```bash
# Inspect worldwide data coverage summary
openairac coverage

# Inspect airport-specific terminal data, runways, procedures, and source provenance
openairac coverage EDDF
openairac coverage EDDF --json

# Run full diagnostic health check on an airport's terminal procedures
openairac doctor-airport EDDF
openairac doctor-airport EDDF --json
```

### 2. Import Local / BYOD AIXM Dataset
```bash
openairac import aixm ./data/samples/aixm5_sample.xml --provider BYOD_AIXM --namespace byod
```

### 3. Build & Sign Deterministic Bundles
```bash
# Build official public release bundle (fails closed if local-only/forbidden sources exist)
openairac bundle build --db ./data/world.openairac.sqlite --out ./bundles

# Verify bundle integrity and authenticity
openairac bundle verify --bundle ./bundles/<bundle_id>
```

### 4. Export Simulator Navigation Datasets
```bash
# Export X-Plane 12 Custom Data
openairac export xplane --out ./dist/xplane

# Export Garmin GNS430 / Classic GPS
openairac export gns430 --out ./dist/gns430

# Export Bendix/King KLN90B GPS (.DAT files)
openairac export kln90b --out ./dist/kln90b

# Export Little Navmap SQLite Database
openairac export lnm --out ./dist/lnm

# Export MSFS SimpleNavData Package
openairac export msfs --out ./dist/msfs
```

---

## 📄 License

Distributed under the **MIT License**. See [`LICENSE`](LICENSE) for details.
