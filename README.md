# ✈️ OpenAIRAC

<div align="center">

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Rust](https://img.shields.io/badge/Language-Rust_2024-orange.svg)](https://www.rust-lang.org/)
[![Tauri 2.0](https://img.shields.io/badge/UI-Tauri_2.0-blue.svg)](https://tauri.app/)
[![X-Plane 12](https://img.shields.io/badge/Simulator-X--Plane_12-blue)](https://www.x-plane.com/)
[![MSFS 2024](https://img.shields.io/badge/Simulator-MSFS_2024-green)](https://www.flightsimulator.com/)
[![GitHub Stars](https://img.shields.io/github/stars/bobberdolle1/open-airac?style=social)](https://github.com/bobberdolle1/open-airac)

**Zero-cost, math-driven navigation engine, dynamic World Magnetic Model solver, and next-gen EFB flight planner for flight simulators.**

[Key Features](#-key-features) • [The Philosophy](#-the-iron-philosophy) • [Architecture](#-architecture) • [Tech Stack](#-tech-stack-2026) • [Comparison](#-navigraph-vs-openairac) • [Quickstart](#-quickstart)

</div>

---

## ⚡ The Iron Philosophy

> **"Install once, navigate forever."**

The traditional flight simulation navigation cycle relies on expensive monthly subscriptions (e.g. Navigraph, Aerosoft) and rigid 28-day manual file updates. If you use a database from 2024 in 2026, runway designations drift out of alignment with magnetic headings, radio frequencies expire, and procedures break.

**OpenAIRAC breaks this monopoly:**
1. **Dynamic Magnetic Calculation:** Uses the official **World Magnetic Model (WMM)** to automatically calculate magnetic variation for any coordinate and year. Runway designations ($09/27 \to 10/28$) and ILS/VOR headings dynamically adapt over time.
2. **Open Aviation Ingestion:** Continuously aggregates public domain & open data from **FAA CIFP**, **OurAirports**, **OpenAIP**, and **Eurocontrol AIXM**.
3. **Zero-Touch Background Sync:** Completely automated data pipeline via GitHub Actions. Install the lightweight CLI once; your simulator's `Custom Data` stays up-to-date seamlessly.
4. **Integrated Modern EFB:** A GPU-accelerated, lightning-fast EFB and Flight Planner designed to replace legacy tools like Little Navmap.

---

## ✨ Key Features

- 🛰️ **Dynamic WMM Pole Variation Solver**: Automatically adjusts VOR courses, ILS alignment, and runway numbers based on Earth's shifting magnetic field ($1900 - 2030+$).
- 🔄 **Universal Sim Support**: Generates native navigation datasets for **X-Plane 12** (`earth_fix.dat`, `earth_nav.dat`, `earth_awy.dat`, `CIFP/*.dat`) and **MSFS 2024** (BGL / NavData XML).
- 🧩 **Procedural ARINC 424 Leg Engine**: Supports complex SID, STAR, and Approach leg types (`IF`, `TF`, `CF`, `DF`, `RF` arcs, `VA`, `VI`).
- 🗺️ **Next-Gen EFB & Flight Planner**: Built with Tauri 2.0, Rust, and MapLibre GL. 60 FPS vector map rendering, auto-routing, airway joining, and real-time telemetry (X-Plane UDP / SimConnect).
- 🛠️ **Custom Procedure Studio**: Visual editor allowing virtual airlines and creators to draw custom SIDs/STARs, create fictional or classic approaches (e.g. Kai Tak 13 IGS), or add regional airstrips.

---

## 🏗️ Architecture

```mermaid
graph TD
    subgraph Open Data Sources
        FAA[FAA CIFP (ARINC 424)]
        OA[OurAirports Open Data]
        AIP[OpenAIP / AIXM 5.1]
    end

    subgraph OpenAIRAC Core Engine (Rust)
        PARSER[ARINC 424 & AIXM Parser]
        WMM[WMM2025 Magnetic Solver]
        PROC[ARINC 424 Procedure Engine]
        ROUTE[A* / Contraction Hierarchy Autorouter]
    end

    subgraph Exporters & UI
        XP12[X-Plane 12 Native .dat Exporter]
        MSFS[MSFS 2024 NavData Packager]
        EFB[Tauri 2.0 EFB & Vector Flight Planner]
    end

    FAA --> PARSER
    OA --> PARSER
    AIP --> PARSER

    PARSER --> WMM
    WMM --> PROC
    PROC --> ROUTE

    ROUTE --> XP12
    ROUTE --> MSFS
    ROUTE --> EFB
```

---

## 🛠️ Tech Stack (2026 Edition)

| Component | Technology | Rationale |
| :--- | :--- | :--- |
| **Core Engine** | **Rust (2024 Edition)** | Maximum speed, memory safety, zero-cost GIS abstractions, instant parsing of 1M+ waypoints. |
| **Magnetic Physics** | **NOAA WMM C/Rust Bindings** | High-precision geomagnetic field calculation ($1900 - 2030+$). |
| **Desktop EFB / GUI** | **Tauri 2.0 + React 19 + TypeScript** | ~15 MB RAM footprint (vs Electron's 500 MB), instant startup, native OS integration. |
| **Map & Vector Render**| **MapLibre GL JS / Deck.gl** | 60 FPS WebGL/WebGPU hardware-accelerated vector mapping and chart rendering. |
| **Telemetry Bridge** | **`simconnect` & `xplane-sdk` Crates** | Low-latency bi-directional flight data streaming from MSFS 2024 & X-Plane 12. |
| **CI/CD Data Bot** | **GitHub Actions** | Automated 28-day build bot fetching FAA & OpenAIP releases without hosting costs. |

---

## ⚔️ Navigraph vs OpenAIRAC

| Feature | Navigraph / Commercial | OpenAIRAC |
| :--- | :---: | :---: |
| **Cost** | ~$10/month (~€90/yr) | **100% Free & Open Source (MIT)** |
| **Updates** | Manual / App Download | **Install Once & Forget (Automated)** |
| **Magnetic Declination**| Static per cycle | **Dynamic WMM2025 Calculations** |
| **Custom Procedures** | Not Allowed | **Included Procedure Studio** |
| **Integrated Flight Planner**| Web / Charts App | **Built-in Rust/Tauri Vector EFB** |
| **Community Driven** | Proprietary | **Open Source (GitHub)** |

---

## 🚀 Quickstart

### Installation via CLI (Rust)

```bash
# Install the OpenAIRAC CLI tool
cargo install openairac-cli

# Run one-touch initial setup for your simulators
openairac sync --sim xp12 --path "F:/SteamLibrary/steamapps/common/X-Plane 12"
```

### Build from Source

Prerequisites:
- [Rust](https://www.rust-lang.org/) (version 1.85+)
- Node.js (v22+) & `pnpm` (for the EFB UI)

```bash
# Clone the repository
git clone https.github.com/bobberdolle1/open-airac.git
cd open-airac

# Build the Rust Core & CLI
cargo build --release

# Run the OpenAIRAC Engine
cargo run --package openairac-cli -- sync --help
```

---

## 🤝 Contributing

Contributions are welcome! Whether you are an aviation enthusiast, Rust developer, or UI designer, feel free to open issues or submit pull requests.

1. Fork the Project
2. Create your Feature Branch (`git checkout -b feature/AmazingFeature`)
3. Commit your Changes (`git commit -m 'Add some AmazingFeature'`)
4. Push to the Branch (`git push origin feature/AmazingFeature`)
5. Open a Pull Request

---

## 📄 License

Distributed under the **MIT License**. See [`LICENSE`](LICENSE) for more information.

<div align="center">
  <sub>Built with ❤️ for the global flight simulation community by <a href="https://github.com/bobberdolle1">bobberdolle1</a>.</sub>
</div>
