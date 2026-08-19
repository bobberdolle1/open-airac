# Changelog

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
