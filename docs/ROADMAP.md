# OpenAIRAC Product Roadmap

## Milestone v0.2 — Foundation Reboot (Shipped)
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

## Milestone v0.3 — Airway Routing & Procedures Foundation (Shipped)
- [x] Canonical airway routing graph (Dijkstra/A\*) with temporal validity, direction semantics, MEA/cruise-altitude filtering, RNAV gating, and exclusions
- [x] FAA CIFP terminal record classes: PA/PG/PC terminal waypoints and PD/PE/PF procedure legs (cycle 2608 verified, lossless raw preservation)
- [x] ARINC 424 SID / STAR / Approach semantic layer with typed path-terminator interpretation (fail-closed on unsupported semantics)
- [x] Flight-plan integration: airport → SID → enroute → STAR → approach with procedure identity and transitions preserved
- [x] Data-quality validation: endpoint existence, procedure fix references, sequence duplicates, altitude bands, terminator membership, disconnected components
- [x] WorldQuery service boundary (world_at / search / nearby / airways / procedures / plan)
- [x] X-Plane production-path strategy documented (convert424toxplane for earth_424.dat; native exporter = diagnostics)
- [ ] Automatic AIRAC cycle change detection & differential sync
- [ ] X-Plane 12 `earth_awy.dat` airway exporter (native)

## Milestone v0.4 — Simulator Integration & EFB (Next Milestone)
- [ ] MSFS 2024 Scenery/NavData BGL packager
- [ ] OpenAIRAC Flight Deck / EFB web interface
- [ ] Little Navmap connector
