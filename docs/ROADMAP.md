# OpenAIRAC Product Roadmap

## Milestone v0.2 — Foundation Reboot (Shipped)
- [x] NOAA WMM2025 spherical harmonic solver, golden-tested against all 12 official NOAA test vectors
- [x] Runway magnetic drift detector & wrap analyzer (published data never overwritten)
- [x] Canonical domain model with strongly-typed IDs & provenance (`license_id`)
- [x] Temporal SQLite store with real revisioning (`(id, valid_from)`), preloading, and versioned migrations
- [x] Transactional, fail-closed ingestion with deterministic diagnostics (reject/quarantine/unchanged)
- [x] Live OurAirports ingestion (HTTP fetch) + offline fixtures
- [x] FAA CIFP layered decoder (EA/D/DB/PN/ER; explicit unsupported records; real-cycle fixtures)
- [x] Structural store validation (`openairac validate`, `WorldStore::validate`)
- [x] Geodesic WGS84 great-circle distance direct routing
- [x] X-Plane 12 dat exporter per Laminar XPFIX1200/XPNAV1200/XPAWY1101 specs (staged sequential swap, fail-closed ILS, golden fixtures)
- [x] X-Plane 12 C-API plugin for local DB status querying
- [x] CLI commands (`doctor`, `magnetic`/`magvar`, `magdrift`, `sync`, `status`, `validate`, `export xplane`)
- [x] CI workflows and automated workspace validation (fmt/check/clippy -D warnings/tests)

## Milestone v0.3 — Routing & Procedures Foundation (Shipped)
- [x] Canonical airway routing graph (Dijkstra/A\*) with temporal validity, direction semantics, MEA/cruise-altitude filtering, RNAV gating, and exclusions
- [x] FAA CIFP terminal record classes: PA/PG/PC terminal waypoints and PD/PE/PF procedure legs (cycle 2608 verified, lossless raw preservation)
- [x] ARINC 424 SID / STAR / Approach semantic layer with typed path-terminator interpretation (fail-closed on unsupported semantics)
- [x] Flight-plan integration: airport → SID → enroute → STAR → approach with procedure identity and transitions preserved
- [x] Data-quality validation: endpoint existence, procedure fix references, sequence duplicates, altitude bands, terminator membership, disconnected components
- [x] WorldQuery service boundary (world_at / search / nearby / airways / procedures / plan)
- [x] X-Plane production-path strategy documented (convert424toxplane for earth_424.dat; native exporter = diagnostics)
- [x] X-Plane 12 `earth_awy.dat` native exporter (XPAWY1101, endpoint typing 11/2/3, referential integrity)

## Milestone v0.4 — AIRAC Lifecycle & Update Automation (Next Milestone)

The engine exists but updates are still manual: the user fetches a
dataset once. v0.4 makes the AIRAC/navigation-data product self-
updating.

- [ ] Automatic AIRAC cycle change detection (source polling, cycle metadata)
- [ ] Current/next cycle preload and timed activation (`valid_from` futures)
- [ ] Differential updates: ingest only changed records, keep full provenance
- [ ] Source reconciliation: cross-provider identity matching and conflict reporting
- [ ] Release/update distribution: versioned data bundles with checksums

## Milestone v0.5 — Procedure Fidelity & Geometry

Complete the terminal-procedure domain before growing the map.

- [ ] RF arc geometry and other derived procedure-leg geometry (rendering layer)
- [ ] Remaining ARINC 424 semantics: holds geometry, vertical-path (GP) angles, course-C DF legs, MSA sectors
- [ ] ILS localizer/glideslope data joined from PF records into exportable form
- [ ] Procedure/runway association validation and approach transition completeness
- [ ] Data quality/confidence scoring (per-airport, per-source)

## Milestone v0.6 — Worldwide Coverage & Providers

- [ ] Worldwide provider architecture (provider registry replacing the two hardcoded adapters)
- [ ] Regional coverage expansion beyond US CIFP (ICAO AIP sources, community providers)
- [ ] Provider failover and per-region confidence reporting

## Milestone v0.7 — Simulator Output & Distribution

- [ ] Complete X-Plane production pipeline (earth_424.dat via verified conversion; earth_awy.dat native)
- [ ] Transactional simulator installation (backup, controlled swap, rollback — designed in v0.2, not yet shipped)
- [ ] MSFS 2024 navdata support (BGL/navdata packager)
- [ ] Simulator compatibility validation harness (golden fixtures per simulator)
- [ ] Update distribution: signed release channel for engine data products

## Future (separate product/application)

- **OpenAIRAC Flight Deck / EFB** — developed separately only after the
  OpenAIRAC navdata engine is mature. Not part of any engine milestone.
- Little Navmap connector (engine maturity permitting)
