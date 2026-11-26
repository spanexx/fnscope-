#!/usr/bin/env bash
# RECONSTRUCTED 2026-11-26 — DESIGN-TIME PLACEHOLDER, NOT A HISTORICAL ARTIFACT.
#
# No pre-impl simulation script exists in the evidence for the J21/J22 adapter
# registry pack (.reports/goal-build-fnscope.md iterations 20-24 and
# .reports/reviews/J21.md record no such file). This placeholder stands in for
# the design-time slot only. It makes no assertions, reports no verdict, and
# must never be quoted as pre-impl evidence.
#
# The design-time checks it would have described, had a pre-impl sim been
# written, are the ones the canonical post-impl sim now enforces:
#   - every documented built-in suffix resolves to its adapter id;
#   - an unregistered suffix resolves to None;
#   - a foreign adapter can be injected into core via scan_with_registry;
#   - core contains no per-language branch and only one lookup per file.
# Those were verified only after implementation, by the orchestrator's real
# gate (`ADAPTER_GATE_STATUS=0`, 101 tests) and by the J22 re-review.
set -u
export LC_ALL=C

printf 'PRE_SIM_PLACEHOLDER (RECONSTRUCTED 2026-11-26)\n'
printf 'No pre-impl simulation artifact exists; this file asserts nothing.\n'
printf 'Use ../simulate.sh for the canonical post-impl verdict.\n'
exit 0
