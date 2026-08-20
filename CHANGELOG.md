# Changelog

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
