# Changelog

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
