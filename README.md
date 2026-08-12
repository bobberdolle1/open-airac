# ✈️ OpenAIRAC

<div align="center">

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Rust](https://img.shields.io/badge/Language-Rust_2024-orange.svg)](https://www.rust-lang.org/)
[![NOAA WMM2025](https://img.shields.io/badge/Physics-NOAA_WMM2025-blue.svg)](https://www.ncei.noaa.gov/products/world-magnetic-model)
[![X-Plane 12](https://img.shields.io/badge/Simulator-X--Plane_12-blue)](https://www.x-plane.com/)
[![CI](https://github.com/bobberdolle1/open-airac/actions/workflows/ci.yml/badge.svg)](https://github.com/bobberdolle1/open-airac/actions/workflows/ci.yml)

**The open navigation data engine for flight simulation. Install once. Navigate forever.**

[Architecture](docs/ARCHITECTURE.md) • [Data Sources](docs/DATA_SOURCES.md) • [Roadmap](docs/ROADMAP.md)

</div>

---

## ⚡ Product Philosophy

> **"Install once. Navigation updates itself."**

Traditional flight simulation navigation relies on expensive recurring subscriptions and rigid manual 28-day AIRAC package updates.

**OpenAIRAC decouples data sources from the runtime engine:**
1. **Canonical Temporal Database:** Embedded SQLite engine storing navigation entities with `valid_from`, `valid_until`, and SHA-256 source snapshot provenance.
2. **NOAA WMM2025 Geomagnetic Physics:** Official NOAA/NCEI World Magnetic Model 2025 (N=12 spherical harmonic expansion) for magnetic declination and field components.
3. **Dual Runway Designator Modeling:** Distinguishes between published official designators (`official_designator = "09"`) and magnetic drift candidates (`computed_magnetic_designator = "08"`), alerting users when redesignation is suggested.
4. **Open Data Ingestion Engine:** Abstract provider pipeline supporting **OurAirports** (airports, runways, navaids) and an experimental **FAA CIFP** ARINC 424 fixed-width parser adapter.

---

## 📊 Feature Status Table (v0.2 Foundation)

| Feature | Status | Crate / Component |
| :--- | :---: | :--- |
| **NOAA WMM2025 Geomagnetic Field Solver** | **Implemented** | `openairac-magnetic` |
| **Runway Magnetic Drift Analysis** | **Implemented** | `openairac-magnetic` |
| **Canonical Domain Model & Provenance** | **Implemented** | `openairac-model` |
| **Embedded Temporal SQLite Store & Migrations** | **Implemented** | `openairac-store` |
| **OurAirports Ingestion (Airports/Runways/Navaids)** | **Implemented** | `openairac-ingest` |
| **Experimental FAA CIFP ARINC 424 Adapter** | **Experimental** | `openairac-ingest` |
| **Geodesic Direct Route Engine (WGS84)** | **Implemented** | `openairac-routing` |
| **X-Plane 12 dat Exporter (`earth_fix`/`earth_nav`)** | **Implemented** | `openairac-export-xplane` |
| **X-Plane 12 C-ABI Status Bridge Plugin** | **Implemented** | `openairac-plugin` |
| **Production CLI (`doctor`, `magnetic`, `magdrift`, `sync`, `status`, `export`)** | **Implemented** | `openairac-cli` |
| **Full SID / STAR / Approach Leg Execution** | *Planned* | `openairac-procedures` |
| **MSFS 2024 Packager** | *Planned* | Roadmap |
| **Flight Deck EFB Interface** | *Planned* | Roadmap |

---

## 🛠️ Workspace Architecture

```text
crates/
├── openairac-model/          # Canonical domain entities, strongly typed IDs, provenance
├── openairac-magnetic/       # NOAA WMM2025 geomagnetic solver & runway drift engine
├── openairac-store/          # Temporal SQLite store with schema migrations
├── openairac-ingest/         # DataProvider abstraction, OurAirports & FAA CIFP parsers
├── openairac-procedures/     # Procedure domain models & ARINC 424 Path Terminators
├── openairac-routing/        # Geodesic WGS84 distance & direct route calculation
├── openairac-export-xplane/  # Database-backed X-Plane 12 navdata exporter
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

### 4. Synchronize Navigation Data from OurAirports
```bash
openairac sync --provider ourairports --db ./data/world.openairac.sqlite
```

### 5. Inspect Database Status & Entity Counts
```bash
openairac status --db ./data/world.openairac.sqlite
```

### 6. Export X-Plane 12 Navigation Data (`earth_fix.dat`, `earth_nav.dat`)
```bash
openairac export xplane --db ./data/world.openairac.sqlite --out ./dist/xplane
```

---

## 📄 License

Distributed under the **MIT License**. See [`LICENSE`](LICENSE) for details.
