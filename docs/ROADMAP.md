# OpenAIRAC Product Roadmap

## Milestone v0.2 — Foundation Reboot (Current Milestone)
- [x] NOAA WMM2025 spherical harmonic solver, golden-tested against all 12 official NOAA test vectors
- [x] Runway magnetic drift detector & wrap analyzer (published data never overwritten)
- [x] Canonical domain model with strongly-typed IDs & provenance (`license_id`)
- [x] Temporal SQLite store with real revisioning (`(id, valid_from)`), preloading, and versioned migrations
- [x] Transactional, fail-closed ingestion with deterministic diagnostics (reject/quarantine/unchanged)
- [x] Live OurAirports ingestion (HTTP fetch) + offline fixtures
- [x] Experimental FAA CIFP layered decoder (EA/D/DB/PN; explicit unsupported records; real-cycle fixtures)
- [x] Structural store validation (`openairac validate`, `WorldStore::validate`)
- [x] Geodesic WGS84 great-circle distance direct routing
- [x] X-Plane 12 dat exporter per Laminar XPFIX1200/XPNAV1200 specs (staged atomic install, fail-closed ILS, golden fixtures)
- [x] X-Plane 12 C-API plugin for local DB status querying
- [x] CLI commands (`doctor`, `magnetic`, `magdrift`, `sync`, `status`, `validate`, `export xplane`)
- [x] CI workflows and automated workspace validation (fmt/check/clippy -D warnings/tests)

## Milestone v0.3 — Airway Routing & Complete Procedure Engine (Next Milestone)
- [ ] Airway segment graph ingestion and shortest-path airway routing solver
- [ ] Full ARINC 424 SID / STAR / Approach leg interpretation engine
- [ ] Automatic AIRAC cycle change detection & differential sync
- [ ] X-Plane 12 `earth_awy.dat` airway exporter

## Milestone v0.4 — Simulator Integration & EFB
- [ ] MSFS 2024 Scenery/NavData BGL packager
- [ ] OpenAIRAC Flight Deck / EFB web interface
- [ ] Little Navmap connector
