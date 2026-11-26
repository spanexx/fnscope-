#!/usr/bin/env bash
# RECONSTRUCTED 2026-11-26 — DESIGN-TIME PLACEHOLDER, NOT A HISTORICAL ARTIFACT.
#
# No pre-impl simulation script exists in the evidence for the final-five pack
# (.reports/goal-build-fnscope.md iterations 35-42 records no such file; the
# reconstruction re-dispatch is iterations 45-46). This placeholder stands in
# for the design-time slot only. It makes no assertions, reports no verdict,
# and must never be quoted as pre-impl evidence.
#
# The design-time checks it would have described, had a pre-impl sim been
# written, are the same ones the canonical post-impl sim now enforces:
#   - each of the five fixtures is present and contains its documented
#     callables for .kt, .cs, .rb, .php and .swift;
#   - each fixture contributes valid, resolvable call edges;
#   - no edge endpoint is a dangling name;
#   - the Kotlin grammar does not link a second tree-sitter runtime;
#   - registration and default globs make the five languages reachable.
# Those were verified only after implementation, in the shipped simulate.sh.
set -u
export LC_ALL=C

printf 'PRE_SIM_PLACEHOLDER (RECONSTRUCTED 2026-11-26)\n'
printf 'No pre-impl simulation artifact exists; this file asserts nothing.\n'
printf 'Use ../simulate.sh for the canonical post-impl verdict.\n'
exit 0
