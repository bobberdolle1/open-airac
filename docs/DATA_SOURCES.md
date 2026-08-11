# 📡 OpenAIRAC Data Sources & License Provenance

OpenAIRAC aggregates open aeronautical data from multiple international providers. Each data source is isolated in a separate **Ingest Adapter** to respect licensing terms and maintain strict data provenance.

---

## Data Sources Matrix

| Source | Data Content | License / Terms | Integration Strategy |
| :--- | :--- | :--- | :--- |
| **FAA CIFP** | US Instrument Procedures, Waypoints, Airways, SIDs/STARs | **Public Domain (US Govt)** | Native ARINC 424 ingestion via `arinc424` Rust crate. |
| **OurAirports** | Global Airports, Runways, Radio Frequencies, Navaids | **Public Domain / CC0** | Core global airport baseline. |
| **Open Flightmaps** | European VFR/IFR Airspaces, Navaids, Procedures | **openflightmaps License / ODbL** | Isolated adapter (`nav-ingest-ofm`). Requires attribution. |
| **OpenAIP** | Global Airspaces, Airports, Navaids | **CC BY-NC 4.0** | Optional non-commercial adapter (`nav-ingest-openaip`). Keeps MIT core separate. |

---

## License Isolation Principle

1. The **OpenAIRAC Core Engine** and storage architecture are licensed under **MIT**.
2. Data ingest adapters operate as isolated plugins.
3. Every record stored in the Canonical DB contains a `source` tag with full provenance tracking.
