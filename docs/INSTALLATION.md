# OpenAIRAC Installation & Setup Guide

This guide explains how to install, configure, and use **OpenAIRAC**.

> Status note: v0.4 (AIRAC Lifecycle, Reconciliation & Distribution). The
> CLI, the canonical temporal store (schema v8), WMM2025, OurAirports and
> FAA CIFP ingestion (incl. SID/STAR/approach legs), the AIRAC cycle
> catalog, publications/corrections/tombstones, rollback, multi-source
> reconciliation, and deterministic data bundles are functional. The CLI, the
> canonical temporal store (schema v4), WMM2025, OurAirports ingestion,
> the routing/procedures/integration layers, and the X-Plane 12
> navaid/fix/airway exporter are functional. FAA CIFP ingestion
> (incl. SID/STAR/approach legs) is implemented at the library level but
> not yet wired into the CLI. Simulator *installation* of the exported
> files (backup, swap, rollback) and MSFS support are planned — do not
> point the exporter at a live simulator installation yet.

---

## Building from Source (Developer Setup)

### Prerequisites

- Rust toolchain 1.85+ (edition 2024)

### Build & Run

```bash
# Clone the repository
git clone https://github.com/bobberdolle1/open-airac.git
cd open-airac

# Run tests
cargo test --workspace --all-features

# Build optimized binary
cargo build --release

# Initialize the local database (live OurAirports fetch)
./target/release/openairac sync --provider ourairports --db ./data/world.openairac.sqlite

# Offline smoke test
./target/release/openairac sync --fixture --db ./data/world.openairac.sqlite
```

---

## Typical Workflow

```powershell
# 1. Fetch current navigation data (airports, runways, navaids)
.\openairac.exe sync --provider ourairports --db .\data\world.openairac.sqlite

# 2. Check database integrity and counts
.\openairac.exe status --db .\data\world.openairac.sqlite

# 3. Validate canonical structural integrity (references, coordinates,
#    temporal ranges, frequencies, procedure sequences, terminators)
.\openairac.exe validate --db .\data\world.openairac.sqlite

# 4. Export X-Plane 12 dat files into a scratch directory (diagnostic)
.\openairac.exe export xplane --db .\data\world.openairac.sqlite --out .\dist\xplane

# 5. Health check
.\openairac.exe doctor --db .\data\world.openairac.sqlite
```

### Export behavior (fail-closed)

- The exporter writes `earth_fix.dat`, `earth_nav.dat`, and
  `earth_awy.dat` per Laminar's XPFIX1200 / XPNAV1200 / XPAWY1101
  specifications, staged and swapped in **sequentially file-by-file**
  (the multi-file swap is not atomic as a set), with a checksummed
  `manifest.json`.
- Records missing fields the format requires (e.g. ICAO region) are skipped
  with diagnostics — values are never invented.
- An export that would produce an empty nav layer is refused unless
  `--allow-empty` is passed. Never use `--allow-empty` against a simulator
  installation.

---

## Calculating Magnetic Variation (CLI Tool)

```powershell
.\openairac.exe magvar --lat 55.9726 --lon 37.4146 --date 2026-08-12
```

Output:

```text
WMM2025 Calculation Result:
  Date: 2026-08-12 (Decimal Year 2026.6110)
  Latitude: 55.9726°
  Longitude: 37.4146°
  Altitude: 0.0 ft
  Declination (MagVar): 11.2846°
  ...
```

Runway drift analysis:

```powershell
.\openairac.exe magdrift --designator 09 --heading 96.7 --lat 55.97 --lon 37.41 --date 2026-08-12
```
