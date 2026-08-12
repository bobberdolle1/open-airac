# OpenAIRAC Product Roadmap

## Milestone v0.2 — Foundation Reboot (Current Milestone)
- [x] NOAA WMM2025 spherical harmonic solver & reference vector tests
- [x] Runway magnetic drift detector & wrap analyzer
- [x] Canonical domain model with strongly-typed IDs & provenance
- [x] Temporal SQLite store with schema migrations (`v1_init.sql`)
- [x] Abstract DataProvider & OurAirports ingestion engine
- [x] Offline ingestion fixtures & malformed record rejection
- [x] Experimental FAA CIFP ARINC 424 fixed-width parser adapter
- [x] Geodesic WGS84 great-circle distance direct routing
- [x] Database-backed X-Plane 12 dat exporter (`earth_fix.dat`, `earth_nav.dat`)
- [x] X-Plane 12 C-API plugin for local DB status querying
- [x] CLI commands (`doctor`, `magnetic`, `magdrift`, `sync`, `status`, `export xplane`)
- [x] CI workflows and automated workspace validation

## Milestone v0.3 — Airway Routing & Complete Procedure Engine (Next Milestone)
- [ ] Airway segment graph ingestion and shortest-path airway routing solver
- [ ] Full ARINC 424 SID / STAR / Approach leg interpretation engine
- [ ] Automatic AIRAC cycle change detection & differential sync
- [ ] X-Plane 12 `earth_awy.dat` airway exporter

## Milestone v0.4 — Simulator Integration & EFB
- [ ] MSFS 2024 Scenery/NavData BGL packager
- [ ] OpenAIRAC Flight Deck / EFB web interface
- [ ] Little Navmap connector
