# Changelog

## 2.1.0 — 2026-08-20

### Worldwide Procedures Expansion I: France Official Procedure Tables

- **Source Document Taxonomy (`openairac-model`)**:
  - Explicit classification into `StructuredNavDataset`, `StructuredProcedurePublication`, `HumanReadableChart`, and `DerivedGeometry`.
  - Distinguishes authoritative structured procedure coding tables from non-navdata graphical charts.

- **French SIA Official Procedure Publication Ingestion (`openairac-ingest::sia_procedures`)**:
  - Parser for official DGAC / SIA France Section AD 2.24 database requirement tables (`DATA SID`, `DATA STAR`, `DATA RNP Approach`).
  - Lossless extraction of ARINC 424 coding fields: path terminators (`IF`, `TF`, `CF`, `DF`), sequence numbers, fly-over flags, magnetic/true tracks, distances, turn directions, altitude constraint windows (`Between`, `AtOrAbove`, `AtOrBelow`, `At`), speed limits (`MAX IAS`), navigation specifications (`RNAV 1`, `RNP 1`, `RNP APCH`), vertical angle, and TCH.
  - Scoped terminal fix coordinate resolution for French airports (e.g. `PG261`, `PG262`, `PG081`, `PG082`, `PO061`, `MN041`, `LL011`, `BO141`, `RW26L`, `RW08R`).
  - Provenance tracked under `FR_SIA_PROCEDURES` with Etalab Licence Ouverte v2.0 for public redistribution.

- **Spain ENAIRE Research & Legal Boundaries**:
  - Audited Spanish ENAIRE AIP procedure descriptions; classified as `LocalOnly` (prohibited from unauthorized public distribution).

- **Procedure Management CLI (`openairac procedures ...`)**:
  - Added `procedures list <ICAO>`, `procedures show <ICAO> <PROC>`, `procedures provenance <ICAO> <PROC>`, `procedures validate <ICAO>`, and `procedures import-sia <file>` with full `--json` support.

## 2.0.0 — 2026-08-20

### Core 2.0 Readiness / Distribution / Productization

- **Protocol v2 Compatibility Handshake (`openairac-service`)**:
  - Client-Core version handshake (`check_client_compatibility`).
  - `BootstrapIndex` publishing recommended downloadable bundles (`world-open`, `us`, `europe-open`) with cryptographic SHA-256 hashes.
  - System diagnostics generator (`generate_diagnostic_report`) with sanitized path export.

## 1.9.0 — 2026-08-20

### Advanced EFB / Georeferenced Raster Foundation & Flight Automation

- **Georeferenced Raster Engine (`openairac-charts::georaster`)**:
  - Implemented `GeoRasterAsset`, `GeoBounds`, `AffineTransform`, and `GeospatialValidator`.
  - 6-parameter affine coordinate transformation ($(\text{px}, \text{py}) \leftrightarrow (\text{lon}, \text{lat})$).
  - High-precision invertible round-trip validation ($\le 0.001\text{ px}$ tolerance).
  - Bounding box indexing and ownship pixel projection for FAA GeoTIFF products (VFR Sectional, TAC, IFR Enroute).
  - Strict refusal to fabricate artificial georeferencing on uncalibrated d-TPP terminal PDFs.

- **Deterministic EFB Domain Calculations (`openairac-charts::efb`)**:
  - Flight phase state machine (`FlightPhaseEngine`) with hysteresis dwell counters and teleport/slew protection.
  - Geodesic cross-track distance (`calculate_cross_track_nm`) with Left/Right/On-track side classification.
  - Planning Top-of-Descent (`calculate_planning_tod_nm`) based on standard 3.0° descent slope and deceleration buffer.
  - Runway wind component calculations (`calculate_runway_wind_components`) for headwind/tailwind and crosswind.
  - Contextual chart suggestion engine (`ChartSuggestion::for_phase`).

- **VATSIM Facility Mapping Semantic Audit (`openairac-online`)**:
  - Audited VATSIM Data API v3 `facilities[]` specification and corrected authoritative facility mapping.
  - Added `callsign_role_hint` and `is_consistent` validation on `OnlineController`.

## 1.8.0 — 2026-08-20

### Online Flying / VATSIM Live Network Awareness

- **Dedicated Online Network Subsystem (`openairac-online`)**:
  - Read-only real-time domain model (`OnlinePilot`, `OnlineController`, `OnlineAtis`, `OnlineEvent`, `OnlineServer`, `OnlinePrefile`, `NetworkSnapshot`).
  - 4-tier freshness classification (`Live`, `Delayed`, `Stale`, `Offline`).
  - Complete data isolation from canonical AIRAC navigation database (untrusted filed routes never pollute navdata).

- **Official VATSIM Data API v3 Provider (`VatsimProvider`)**:
  - Ingestion from `https://data.vatsim.net/v3/vatsim-data.json` at official 15-second cadence.
  - Parsing of thousands of live pilots, ATC stations, active ATIS broadcasts, connected servers, and prefiled plans.
  - Clean extraction of airport stations (`KJFK_DEL`, `EGLL_APP`) and enroute centers (`LON_CTR`, `NY_CTR`).

- **Official VATSIM Events API v2 Provider (`parse_vatsim_events_json`)**:
  - Ingestion from `https://events.vatsim.net/api/v2/events`.
  - Extraction of active and upcoming online events, time windows, participating airports, and routes.

- **Route & Airport Online Awareness Engine (`RouteOnlineAwareness`)**:
  - ATC along flight plan route (departure ATC, enroute ATC, arrival ATC).
  - Traffic filtering within configurable route corridor (default 50 NM) and destination vicinity (100 NM).
  - Airport operations summary (active controllers, ATIS broadcast, filed arrivals, filed departures, on-ground traffic).
  - Flight plan route event correlation.

- **Ephemeral Operational Cache & Security Sanitization (`OnlineCache`)**:
  - Ephemeral SQLite operational cache (`openairac_online.sqlite`) with bounded retention.
  - Strict input sanitization, coordinate bounds checking, and HTML entity escaping.

- **OpenAIRAC Online CLI (`openairac online ...`)**:
  - Added `openairac online providers`, `vatsim status`, `vatsim pilots`, `vatsim controllers`, `vatsim airport`, `vatsim atis`, `vatsim route`, `vatsim events` with full `--json` support.

## 1.7.0 — 2026-08-20

### OpenAIRAC Weather Subsystem & Flight Briefing

- **Dedicated Weather Subsystem (`openairac-weather`)**:
  - AviationWeather.gov Data API provider, METAR/TAF/SIGMET/PIREP parsing, route corridor hazard intersections, and preflight flight briefing.

## 1.6.0 — 2026-08-20

### Open Charts Foundation & First Real EFB Capability

- **Dedicated Open Charts Subsystem (`openairac-charts`)**:
  - Implemented decoupled chart domain model (`ChartDocument`, `NormalizedChartType`, `ChartMimeType`, `GeoreferenceStatus`, `ChartAssociation`).
  - Decoupled chart reference documents from machine-readable navigation procedures (`CanonicalProcedureLeg`).
  - Normalized categories across international authorities: `AirportDiagram`, `ParkingDocking`, `GroundMovement`, `Sid`, `Star`, `Approach`, `ApproachVisual`, `TakeoffMinima`, `AlternateMinima`, `RadarMinima`, `HotSpot`, `Holding`, `Obstacle`, `Noise`, `GeneralInfo`.

- **Content-Addressed Asset Storage & Security Cache (`ChartCache`)**:
  - Content-addressed SHA-256 local asset cache (`charts/cache/sha256/ab/<hash>.pdf`).
  - Atomic downloads via `.part` staging files.
  - File signature & magic header validation (`%PDF-` for PDF, `\x89PNG` for PNG, `\xff\xd8` for JPEG).
  - Strict path traversal prevention and 50 MB sanity guards.

- **Isolated Chart Catalog Storage (`ChartCatalog`)**:
  - Dedicated SQLite catalog (`openairac_charts.sqlite`) preserving complete schema isolation from Little Navmap navigation databases.
  - High-performance indexing by airport, AIRAC cycle, chart category, and procedure name.

- **Official FAA d-TPP Chart Provider (`FaaDtppProvider`)**:
  - Ingestion and streaming parser for official FAA `d-TPP_Metafile.xml`.
  - Lazy, on-demand downloading of individual PDF plates directly from FAA Aeronautical Information Services servers (`https://aeronav.faa.gov/d-tpp/<cycle>/<pdf_name>`).
  - Full indexing of 38 official charts for KJFK, plus KLAX, KORD, KATL nationwide.

- **France SIA eAIP Chart Provider (`FranceSiaChartProvider`)**:
  - Official index of Section AD 2.24 charts for French aerodromes (LFPG, LFPO, LFMN, LFLL, LFBO).
  - Demonstrates strict safety invariant: LFPG charts (ADC, APDC, GMC, SID, STAR, IAC) are fully available, while machine-readable navigation procedures remain absent (0) to match real public data truth.

- **Procedure-to-Chart Association Engine (`AssociationEngine`)**:
  - Matches canonical navigation procedures with published chart plates with explicit confidence ratings (`Exact`, `Likely`, `Ambiguous`, `Unmatched`).

- **OpenAIRAC CLI Commands (`openairac charts`)**:
  - `openairac charts providers`: list official chart providers.
  - `openairac charts sync [PROVIDER]`: sync metadata catalog.
  - `openairac charts airport <ICAO> [--json]`: list published charts for airport.
  - `openairac charts procedure <ICAO> <PROC> [--json]`: procedure-to-chart resolution.
  - `openairac charts fetch <CHART_ID>`: on-demand download and cache verification.
  - `openairac charts cache status`: cache statistics and storage metrics.

- **OpenAIRAC Map Charts Integration (`openairac-map` v0.2.0)**:
  - Added `OpenAiracChartsDock` panel with airport search, category grouping tabs, and offline pack downloader.
  - Implemented `ChartViewerWidget` with PDF plate display, zoom in/out, fit width, rotate, night mode, and external reader launch.
  - Integrated pinned charts bar and flight plan chart shortcuts (`FlightSuggestions`).

## 1.5.0 — 2026-08-20

### OpenAIRAC Map Foundation & Little Navmap SQLite Target Verification

- **Official OpenAIRAC Map Fork (`bobberdolle1/openairac-map`)**:
  - Created GPL-3.0 compliant fork of Little Navmap with preserved upstream credits and mergeability.
  - Established `NavigationProvider` abstraction separating OpenAIRAC, Simulator Scenery, and optional Navigraph data.
  - Set OpenAIRAC as the default and preferred navigation provider on fresh installations.
  - Integrated OpenAIRAC Provenance (`ProvenanceManager`) and Coverage Diagnostics (`CoverageManager`) into airport and navaid information panels.
  - Built atomic database replacement and rollback workflow in `OpenAiracDbManager`.

- **Database Identity & Physical Separation**:
  - Discontinued reliance on Navigraph database filenames: OpenAIRAC now compiles directly to `openairac.sqlite` (with `little_navmap_openairac.db` compatibility alias).
  - Strict physical isolation: OpenAIRAC and Navigraph databases are separate files and never overwrite each other.

- **Complete SQLite Schema v14.29 Implementation (`openairac-export-lnm`)**:
  - Full alignment with Little Navmap / atools 4.0.18 / 3.0.18 SQLite schema across all 31 tables.
  - Full export of Instrument Terminal Procedures (SIDs, STARs, Approaches) into `approach`, `approach_leg`, `transition`, and `transition_leg` tables.
  - Added spatial coordinate bounds and endpoint coordinates to `airway` table.

- **Unmodified Little Navmap 3.0.18 Acceptance**:
  - Verified against unmodified upstream binary (`LittleNavmap-win64-3.0.18`) on real cycle 2608 data: opens cleanly with zero integrity errors, valid metadata, and zero crashes.
  - Verified truthful missing procedure behavior for France SIA AIXM baseline (LFPG has 0 terminal procedures with explicit diagnostic reason; zero synthetic procedures fabricated).
  - Integrated deterministic acceptance test suite `real_acceptance.rs`.

- **Documentation & Future Roadmap**:
  - Published `docs/OPENAIRAC_INTEGRATION.md`, `docs/MAP_INTEGRATION.md`, `docs/UPSTREAM_SYNC.md`, and `docs/EFB_ROADMAP.md`.
  - Published comprehensive evidence matrix in `docs/OPENAIRAC_MAP_ACCEPTANCE.md`.

## 1.4.0 — 2026-08-20

### Reality Gate / First Real Non-US Aeronautical Dataset Integration

- **French SIA (DGAC France) AIXM 4.5 Ingestion**:
  - Implemented version-isolated `Aixm45Provider` in `openairac-ingest::aixm45`.
  - Verified end-to-end against real 34 MB DGAC/SIA AIRAC XML dataset: successfully ingested 940 French aerodromes (LFPG, LFPO, LFMN, LFLL, LFBO), 893 runways, 551 radio navaids (VOR, DME, NDB, TACAN), and 2,609 airway segments (e.g. UN874).
  - Provenance tracked under `FR_SIA` with Etalab Licence Ouverte v2.0.
  - Verified that public SIA AIXM 4.5 airspace dataset does not contain machine-readable procedure legs (published via eAIP PDF); reported truthfully with zero invented data.

- **AIXM 5.1.1 Codelist Completeness**:
  - Full specification alignment for all official `CodeSegmentPathType` / `CodeSegmentPathBaseType` path terminators (`AF`, `PI`, `PT`, `FT`, `IF`, `TF`, `CF`, `DF`, `FA`, `FC`, `FD`, `FM`, `CA`, `CD`, `CI`, `CR`, `VA`, `VD`, `VI`, `VM`, `VR`, `HA`, `HF`, `HM`, `RF`, `OTHER`).
  - Unknown `OTHER:*` terminators fail closed with typed unsupported diagnostics.

- **ProcedureValidator Aviation Semantics**:
  - Corrected altitude constraint validation: recognized standard missed-approach climbs in instrument approaches (`ProcedureKind::Approach`) as valid, eliminating false constraint warnings.
  - Enforced realistic airspeed constraint bounds (60–400 kts) and phase-aware profile gradient checks.

- **Automatic Version-Detecting AIXM Ingestion**:
  - `openairac_ingest::ingest_aixm_auto` seamlessly routes between AIXM 4.5 and AIXM 5.x parsers for both CLI `openairac import aixm` and automated background sync.

- **Provider Licensing Audit & Separation**:
  - Formally differentiated `DFS_INSPIRE` (open geodata under GeoNutzV, no terminal procedures) from `DFS_AIS` (full procedure portal requiring user account).

- **Target Maturity Calibration & Acceptance Matrix**:
  - Published `docs/REAL_WORLD_ACCEPTANCE.md` providing empirical verification status for every provider and simulator target.
  - Adjusted `xplane-gns430` and `kln90b` to `EXPERIMENTAL` status until live in-cockpit/gauge execution tests are performed on the verification workstation.

## 1.3.0 — 2026-08-20

### Worldwide Navdata Platform & Multi-Sim Expansion

- **Provider Federation & Licensing Policy Layer**:
  - First-class provider and licensing model with 4-tier `RedistributionPermission` (`PublicRedistribution`, `LocalOnly`, `MetadataOnly`, `Forbidden`).
  - Machine-readable global authority registry in `data/providers.yaml` (FAA, OurAirports, openflightmaps, DFS Germany, Eurocontrol EAD, BYOD).
  - Automated bundle builder distribution policy enforcement and local-only safeguards (`BundleScope::PublicRelease` vs `BundleScope::LocalDevelopment`).

- **Generic AIXM 5.x Ingestion**:
  - Full XML/GML DOM & streaming parser for AIXM 5.1/5.1.1 basic messages (`openairac-ingest::aixm`).
  - Support for `AirportHeliport`, `Runway`, `Navaid`, `DesignatedPoint`, `Route`, `StandardInstrumentDeparture`, `StandardInstrumentArrival`, `InstrumentApproachProcedure`.
  - Lossless translation of all standard ARINC/AIXM path terminators, altitude/speed constraint profiles, and vertical navigation geometry.

- **Local / BYOD Ingestion Workflows**:
  - User CLI workflow `openairac import aixm <file>` with explicit provenance and isolated local-only distribution tags.
  - Secure local compilation of personal AIP/EAD datasets without risk of public release contamination.

- **Worldwide Procedure Validation Layer**:
  - Comprehensive semantic and geometric validator in `openairac-procedures::validation`.
  - Fix coordinate resolution, sequence continuity, runway association, altitude gradient monotonicity, and geometric discontinuity detection (> 250 NM).

- **Coverage Inspector & Terminal Doctor**:
  - `openairac coverage [ICAO] [--json]` for instant airport metadata, runway inventories, procedures count, and data provenance.
  - `openairac doctor-airport <ICAO> [--json]` diagnostic tool analyzing missing elements, flyability status, and procedure validation issues.

- **Legacy Garmin GNS430 & KLN90B Exporters**:
  - `Gns430Exporter` generating complete `Airports.txt`, `Navaids.txt`, `Waypoints.txt`, `ATS.txt`, and `Proc/<ICAO>.txt` files (`docs/GNS430_COMPATIBILITY.md`).
  - `Kln90bExporter` generating standard clean-room MIT `.DAT` tables (`APT.DAT`, `NAV.DAT`, `WPT.DAT`, `AWY.DAT`, `FAS.DAT`) (`docs/KLN90B_COMPATIBILITY.md`).
  - Target descriptors and transactional installers for `xplane-gns430` and `kln90b` (SUPPORTED status).

- **Regional Bundle Filtering**:
  - Deterministic regional bundle packaging (`BundleFilter`) with presets for `world-open`, `us`, and `europe-open`.

## 1.2.0 — 2026-08-20

### X-Plane 12 complete dataset serialization

- **LPV FAS Guidance (`earth_nav.dat` Rows 14 & 16)**:
  - Decoded from ARINC 424 Path Point (`PP`) records in `openairac-ingest`.
  - Canonical `CanonicalLpvFas` model with migration `v11_lpv_fas.sql`.
  - Serialized rows 14 (FPAP) and 16 (LTP) per XPNAV1200 with sub-millidegree precision (100% channel and app type agreement on 4,709 golden approaches).
- **Procedural Holds (`earth_hold.dat`, HOLD1140)**:
  - Extracted from `HA`, `HF`, `HM` procedure legs, deduplicated on `(fix_ident, fix_icao_code, airport_ident, course, turn)`.
  - Serialized 8,914 holding patterns across the NAS.
- **Airport Operational Metadata (`earth_aptmeta.dat`, AptXP1210)**:
  - Derived FAA/ICAO region codes (`K1`-`K7`, `PA`, `PH`, `PR`, `CY`, `MM`), runway lengths, IFR classification, and transition altitudes.
  - 99.92% exact field match on 18,078 airports.
- **Minimum Sector Altitudes (`earth_msa.dat`, MSAXP1150)**:
  - Decoded from FAA CIFP `PS` records, modeled in `CanonicalMsa` with migration `v12_msa.sql`.
  - 99.98% exact match across 5,654 golden MSA records.
- **Grid MORA Matrix (`earth_mora.dat`, MORAXP1150)**:
  - Decoded from FAA CIFP `AS` records, modeled in `CanonicalMora` with migration `v13_mora.sql`.
  - 100% exact match across all 241 golden MORA blocks.

## 1.1.0 — 2026-08-20

### Simulator output

- MSFS: complete ARINC 424 → BGLComp leg mapping (all published path
  terminators); real cycle 2609: 199,966/199,966 legs written,
  2,190 departures, 1,916 arrivals, 10,232 approaches, 0 skipped
  terminators. SDK compile still requires a real `fspackagetool.exe`.
- PMDG classic FMC text exporter (`wpNavAPT/AID/FIX/RTE`, AIRNAV data
  file definition); real 2609: 8,172 runway rows, 17,647 navaids,
  70,085 fixes, 17,889 airway segments.
- Target registry with multi-platform detection (`%VAR%`/`~`
  expansion) and honest support states; `target list|detect|install|
  rollback|update-all` CLI. `update-all` exports once per family and
  updates every detected target with per-target rollback — never
  mixes worlds.
- Installer hardening (found by live verification + red team):
  cross-volume-safe swaps, automatic rollback on swap failure, undo
  of the last successful install (journal + backup retained at
  commit), recovery that treats Committed journals as a feature.
- Release Gate v2: registry-driven per-target export → install →
  validate → rollback → byte-identical round-trip. 14/14 PASS on real
  cycle 2609.

### Performance

- Bulk runway query and indexed lookups: full four-format export on a
  real cycle down from 124.5s to 20.4s (release build).

### Docs

- `docs/SUPPORTED_TARGETS.md` (new), `docs/1.1_CONSOLIDATION.md`
  (new), provenance matrix updated.


## 1.0.0 — 2026-08-19

First production release.

### Core engine

- Canonical temporal world store (`(id, valid_from)` revisions, migrations
  v1-v10) with source provenance, observations, tombstones, and
  close-absent full-snapshot semantics.
- AIRAC lifecycle: discovery, preload, time-driven activation,
  supersede/expire, rollback; cycle-effective instants are explicit —
  never inferred from wall clock or cycle numbers.
- Layered FAA CIFP decoder (EA/D/DB/PN/PA/PG/PC/PD/PE/PF/PI) with
  golden verification against convert424toxplane v12.4 on cycles 2608
  and 2609: 32,457/32,457 exported fixes value-identical, 1,379/1,379
  procedure chains identical across KSFO/KDEN/KJFK/KLAX/KORD.
- ILS: localizer + glideslope from PI records (front course, direct
  glideslope angle, station declination, elevation), PF-derived
  associations, PI-over-D merge, LOC/ILS row classification.
- OurAirports ingestion (airports, runways, navaids) through the
  atomic publication machinery.
- Multi-source reconciliation with region-scoped authority rules,
  membership intervals, and conflict detection.
- WMM2025 magnetic model with golden NOAA vectors.

### Distribution & update

- Deterministic content-addressed bundles with integrity fail-closed
  verification, staged validated install, and artifact-level rollback.
- Ed25519 release signing: offline key provisioning (`keygen`), stable
  key ids, multi-root rotation, fail-closed trust handling, embedded
  production trust root in the CLI (production installs reject
  unsigned bundles; `--allow-unsigned` is the explicit dev escape).
- Local update channel with deterministic decisions (NoUpdate /
  Preload / Activate / ReplacePreload / Reject*), current/next
  semantics with explicit effective instants.

### Simulator output

- X-Plane 12 navdata layer (XPNAV1200/XPFIX1200/XPAWY1101) with
  transactional journaled install (backup, swap, post-validation,
  crash recovery, rollback) and sim-world resolution.
- Generic export/target architecture (FormatExporter /
  GeneratedArtifactSet / TargetDescriptor / TargetInstaller) with
  honest support states.
- Experimental: X-Plane 11 (same layer), MSFS 2020/2024 navdata
  package sources (official fspackagetool path), Little Navmap
  SQLite nav database (open-source schema authority).

### Quality

- Machine-enforced release gate: validation errors, publication audit,
  procedure referential integrity, bundle determinism, signature
  fail-closed, installer post-validation, multi-format export
  verification, golden harnesses.

## Pre-1.0 (development)

AIRAC lifecycle and publications (v0.4), procedure fidelity and
geometry (v0.5), worldwide coverage and providers (v0.6), simulator
output and distribution maturity (v0.7), release-candidate hardening.
See git history for the full trail.
