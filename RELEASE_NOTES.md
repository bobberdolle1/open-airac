# OpenAIRAC 1.0.0 Release Notes

**Date:** 2026-08-19
**Active AIRAC on release date:** 2608
**Next cycle:** 2609 (effective 2026-09-03 0901Z) — prepublished as NEXT
only; it becomes ACTIVE exactly at its confirmed effective instant.

> OpenAIRAC is for FLIGHT SIMULATION ONLY. It is not certified and must
> never be used for real-world navigation.

## Supported

- **X-Plane 12** — navdata layer (fixes, navaids, airways) with
  transactional install, post-install validation, and rollback.
  Golden-verified against Laminar's convert424toxplane v12.4 on FAA
  cycles 2608 and 2609. Terminal procedures ship through the official
  converter path (`CIFP/$ICAO.dat`; see docs/X_PLANE_STRATEGY.md).

## Experimental

- **X-Plane 11** — same layer format; install path not machine-tested.
- **MSFS 2020 / MSFS 2024** — navdata package sources for the official
  fspackagetool pipeline; SDK compile + in-sim verification pending.
- **Little Navmap** — SQLite nav database from the open-source atools
  schema; in-app load not executed.

## Research (not shipped as targets)

Aerosoft CRJ / NavDataPro, PMDG legacy, Level-D, FeelThere/Wilco,
Flight1/FSBuild, DFD, FWD-FD, Fenix/TFDi. See docs/FORMATS.md for
provenance and blocking reasons.

## What is included

- `openairac-cli.exe` — the complete engine CLI (sync, cycles,
  validate, reconcile, bundle build/verify/install, update, export,
  release-gate, keygen).
- Documentation: README, INSTALLATION, SECURITY, DATA_SOURCES,
  FORMATS, X_PLANE_STRATEGY, 1.0_READINESS.

## Installation

See INSTALLATION.md. TL;DR: unpack the archive anywhere, run
`openairac-cli.exe --help`. X-Plane install is transactional; your
existing navdata is backed up automatically and restored on rollback.

## Signing

Release archives are accompanied by SHA-256 checksums and an Ed25519
signature from the OpenAIRAC production key.
Production key id: `a8f2ca4e06872bf4` (public key embedded in the
CLI; private key held off-repo — see docs/SECURITY.md).

## Known limitations

- MSFS and Little Navmap targets are EXPERIMENTAL and must not be
  treated as production-ready.
- SBAS/LPV in X-Plane additionally requires the flown aircraft to have
  an SBAS receiver assigned in Plane Maker (simulator-side setting).
