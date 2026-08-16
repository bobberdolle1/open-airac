#!/usr/bin/env bash
# OpenAIRAC release gate. Fails (non-zero exit) on ANY violation.
#
# Usage:
#   scripts/release-gate.sh [--cifp <file> --effective <RFC3339> [--converter <exe>]]
#
# Without data arguments: static + unit gate only (CI-safe).
# With --cifp: full data gate - ingests the CIFP into a temp world,
# runs the machine-enforced gate, and (when --converter is given) the
# golden compatibility harnesses against convert424toxplane.
#
# The gate must be run from the repository root.
set -euo pipefail

CIFP=""
EFFECTIVE=""
CONVERTER=""
while [ $# -gt 0 ]; do
  case "$1" in
    --cifp) CIFP="$2"; shift 2 ;;
    --effective) EFFECTIVE="$2"; shift 2 ;;
    --converter) CONVERTER="$2"; shift 2 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

echo "== Stage 1: static + unit =="
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features

if [ -z "$CIFP" ]; then
  echo "== Stage 1 complete (no data arguments; skipping data gate) =="
  exit 0
fi

if [ -z "$EFFECTIVE" ]; then
  echo "error: --cifp requires --effective <RFC3339>" >&2
  exit 2
fi

WORK=$(mktemp -d)
DB="$WORK/world.sqlite"
trap 'rm -rf "$WORK"' EXIT

echo "== Stage 2: ingest $CIFP (effective $EFFECTIVE) =="
cargo run -q -p openairac-reconcile --example gate_ingest -- "$DB" "$CIFP" "$EFFECTIVE"

echo "== Stage 3: machine-enforced release gate =="
cargo run -q -p openairac-cli -- release-gate --db "$DB" --effective "$EFFECTIVE" --out "$WORK/gate"

if [ -n "$CONVERTER" ]; then
  echo "== Stage 4: golden compatibility harnesses (converter) =="
  cargo run -q -p openairac-export-xplane --example golden_compat -- \
    "$CIFP" "$CONVERTER" "$WORK/golden" "$EFFECTIVE"
  cargo run -q -p openairac-export-xplane --example golden_procedures -- \
    "$CIFP" "$CONVERTER" "$WORK/golden" "$EFFECTIVE" KSFO KDEN KJFK KLAX KORD
fi

echo "RELEASE GATE: PASS"
