# ✈️ OpenAIRAC Core

<div align="center">

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Language-Rust_2024-orange.svg)](https://www.rust-lang.org/)
[![NOAA WMM2025](https://img.shields.io/badge/Physics-NOAA_WMM2025-blue.svg)](https://www.ncei.noaa.gov/products/world-magnetic-model)
[![Target: X-Plane & MSFS](https://img.shields.io/badge/Simulators-X--Plane_12_|_MSFS-blue.svg)](https://github.com/bobberdolle1/open-airac/blob/main/docs/SIMULATOR_SETUP.md)
[![Desktop EFB](https://img.shields.io/badge/Desktop_App-OpenAIRAC_Map-green.svg)](https://github.com/bobberdolle1/openairac-map)
[![CI](https://github.com/bobberdolle1/open-airac/actions/workflows/ci.yml/badge.svg)](https://github.com/bobberdolle1/open-airac/actions/workflows/ci.yml)

**OpenAIRAC — open aeronautical navigation data infrastructure and temporal database engine for flight simulation.**

[📥 **OpenAIRAC Map Desktop App**](https://github.com/bobberdolle1/openairac-map) • [📖 **User Guide**](docs/USER_GUIDE.md) • [🏗️ **Architecture**](docs/ARCHITECTURE.md) • [🗄️ **Data Sources**](docs/DATA_SOURCES.md)

</div>

---

> ⚠️ **FOR FLIGHT SIMULATION ONLY — NEVER USE FOR REAL-WORLD AVIATION.**  
> OpenAIRAC is not certified, carries no operational guarantees, and must never be used for real flight planning or aircraft operations.

---

## 🧭 What OpenAIRAC Is

OpenAIRAC is an open navigation-data **engine and compilation platform**: it ingests official and open-access aeronautical datasets, stores them in a high-performance canonical temporal SQLite database, executes procedure validation and ATS graph routing, and powers both simulator installations and modern Electronic Flight Bags (EFBs).

For normal flight-sim users, OpenAIRAC powers **[OpenAIRAC Map](https://github.com/bobberdolle1/openairac-map)** — a standalone desktop EFB, moving map, and flight planner.

```text
  ┌────────────────────────────────────────────────────────────────────────┐
  │                           Data Ingestion                               │
  │   FAA CIFP • OurAirports • OpenFlightmaps • France SIA • Local BYOD   │
  └───────────────────────────────────┬────────────────────────────────────┘
                                      │
  ┌───────────────────────────────────▼────────────────────────────────────┐
  │                   Canonical Temporal Database (SQLite)                 │
  │      Revisions (id, valid_from) • Provenance Layer • WMM2025 Solver     │
  └───────────────────────────────────┬────────────────────────────────────┘
                                      │
  ┌───────────────────────────────────▼────────────────────────────────────┐
  │                 Routing Graph & Procedure Validation                   │
  │    Airway Network (Dijkstra/A*) • ARINC 424 Path Terminators • RNP/RNAV│
  └───────────────────┬───────────────────────────────┬────────────────────┘
                      │                               │
  ┌───────────────────▼──────────────┐  ┌─────────────▼────────────────────┐
  │       Simulator Exporters        │  │        AI Crew Gateway           │
  │  X-Plane 12/11 • MSFS • GNS430   │  │   REST API at 127.0.0.1:8989     │
  │  KLN90B • PMDG • Little Navmap   │  │ FlightdeckOS & Desktop Map EFB   │
  └──────────────────────────────────┘  └──────────────────────────────────┘
```

---

## ⚡ Current Release Truth (Core v2.12.0 / Product 3.3)

* **AIRAC Cycle Lifecycle Engine**: Automated discovery, effective date validation, preload scheduling, atomic activation, and temporal rollback.
* **Temporal Provenance & Policy Layer**: Machine-readable provider registry (`data/providers.yaml`) with strict policy enforcement (`PublicRedistribution`, `LocalOnly`, `MetadataOnly`, `Forbidden`).
* **Multi-Provider Data Fusion**: Global baseline fusion incorporating FAA CIFP, OurAirports, OpenFlightmaps, and France SIA datasets.
* **Worldwide Procedure Engine**: Full ARINC 424 path terminator evaluation (`IF`, `TF`, `CF`, `DF`, `FA`, `CA`, `VA`, `HA`, `HF`, `HM`, `RF`), altitude/speed constraints, and procedure validation.
* **High-Performance ATS Airway Routing**: Geodesic graph routing solver computing optimal enroute airways with airway directionality and level restrictions.
* **Local AIP Vault (BYOD)**: Secure local import mechanism (`openairac import aixm <file>`) for official national AIP datasets that require local-only simulation use (such as Russian CAICA).
* **AI Crew Gateway**: Integrated REST API at `http://127.0.0.1:8989/api/openairac/v1` supplying canonical flight telemetry snapshots (`OpenAiracSnapshotV2`), weather summaries, active legs, descent profiles, and procedure metadata.
* **Complete Simulator Exporter Suite**:
  - **X-Plane 12 / 11** (`earth_fix.dat`, `earth_nav.dat`, `earth_awy.dat`, `earth_hold.dat`, `earth_aptmeta.dat`, `earth_msa.dat`, `earth_mora.dat`) — **SUPPORTED**
  - **Garmin GNS430 / Classic GPS** (`Airports.txt`, `Navaids.txt`, `Waypoints.txt`, `ATS.txt`, `Proc/<ICAO>.txt`) — **SUPPORTED**
  - **Bendix/King KLN90B GPS** (`APT.DAT`, `NAV.DAT`, `WPT.DAT`, `AWY.DAT`, `FAS.DAT`) — **SUPPORTED**
  - **Little Navmap / OpenAIRAC Map** (SQLite schema) — **SUPPORTED**
  - **MSFS 2024 / 2020** (SimpleNavData BGL layout) — **EXPERIMENTAL**
  - **PMDG classic FMC** (`wpNav*.txt`) — **EXPERIMENTAL**

---

## 🛡️ Data & Redistribution Policy

OpenAIRAC strictly enforces licensing boundaries:

1. **Public Redistribution**: Open datasets (FAA CIFP, OurAirports, OpenFlightmaps, France SIA) are freely bundled in official releases.
2. **Local-Only (BYOD)**: Datasets from national authorities permitted for personal use but without third-party redistribution rights (e.g. Russian CAICA) are supported exclusively via the user's **Local AIP Vault** on their local machine.
3. **Strictly Forbidden**: Proprietary commercial datasets (Navigraph, Jeppesen, NavDataPro) are **NEVER** ingested, bundled, or redistributed.

---

## 🛠️ Workspace Architecture

```text
crates/
├── openairac-model/          # Canonical domain entities, provider policies & registry
├── openairac-magnetic/       # NOAA WMM2025 geomagnetic solver & runway drift engine
├── openairac-store/          # Temporal SQLite store with schema migrations (v1-v19)
├── openairac-ingest/         # CIFP ARINC 424, Generic AIXM 5.x & OurAirports parsers
├── openairac-procedures/     # ARINC path terminators & Worldwide Procedure Validator
├── openairac-routing/        # Canonical airway routing graph (Dijkstra/A*) + geodesics
├── openairac-integration/    # Full flight-planning join (procedures + enroute graph)
├── openairac-service/        # WorldQuery API, AI Crew Gateway & HTTP server
├── openairac-bundle/         # Deterministic content-addressed bundles & Ed25519 signing
├── openairac-export/         # Generic export architecture & target registry
├── openairac-export-xplane/  # X-Plane 12 complete dataset exporter
├── openairac-export-msfs/    # MSFS BGL package exporter
├── openairac-export-lnm/     # Little Navmap / OpenAIRAC Map SQLite database exporter
├── openairac-export-pmdg/    # PMDG classic FMC text exporter
├── openairac-plugin/         # X-Plane 12 C-ABI plugin for live SQLite status querying
└── openairac-cli/            # Unified CLI interface
```

---

## 📚 Documentation

* [📖 **User Guide**](docs/USER_GUIDE.md) — Master navigation and software guide.
* [🚀 **First Flight Tutorial**](docs/FIRST_FLIGHT_TUTORIAL.md) — Getting started with public data.
* [🎮 **Simulator Setup Guide**](docs/SIMULATOR_SETUP.md) — X-Plane and MSFS connection instructions.
* [🗄️ **Data & Providers Guide**](docs/DATA_AND_PROVIDERS.md) — AIRAC cycles and provider federation.
* [🇷🇺 **Russia / CAICA Guide**](docs/RUSSIA_CAICA_GUIDE.md) — Local AIP Vault import for Russian AIP.
* [🤖 **AI Crew Gateway**](docs/AI_CREW_GATEWAY.md) — HTTP API for FlightdeckOS and companion apps.
* [🔒 **Privacy & Security**](docs/PRIVACY_AND_SECURITY.md) — Data isolation and security model.

---

## 📜 License

Distributed under the **MIT License**. See [`LICENSE`](LICENSE) for details.
OpenAIRAC Map desktop application is distributed under the **GNU General Public License v3.0 (GPLv3)**.
