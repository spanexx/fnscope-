#!/usr/bin/env bash
# DESIGN-TIME PLACEHOLDER (reconstructed 2026-11-26) — NOT EVIDENCE
#
# This is the design-time mirror of the fnscope-rust-backend simulation. It
# was written as a reconstruction of the intended pre-implementation scenarios,
# not backdated to a date when the Rust backend did not exist. It uses no
# binary, runs no cargo, and never prints PASS: a design sketch cannot prove a
# gate. The canonical post-impl simulation is ../simulate.sh.
set -euo pipefail

echo "[design-sim] DESIGN-TIME PLACEHOLDER — not evidence, not a gate"
echo "[design-sim] target behavior: fnscope scan tests/fixtures/rust_app"
echo "[design-sim] intended checks (as locked in PRD-TRD-fnscope-rust-backend.md):"
echo "[design-sim]   D1 manifest parses and contains at least 7 functions"
echo "[design-sim]   D2 Calculator.scale is id calculator-scale and calls add"
echo "[design-sim]   D3 classify.errors includes EMPTY_TEXT"
echo "[design-sim]   D4 report is side-effect with permission console:out"
echo "[design-sim]   D5 no non-null call edge dangles"
echo "[design-sim] no command was run; use ../simulate.sh for real observations"
echo "DESIGN_SIM_PLACEHOLDER"
