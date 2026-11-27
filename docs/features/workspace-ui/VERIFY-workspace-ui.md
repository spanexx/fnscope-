# VERIFY — workspace UI

**Status:** implementation verified, awaiting the human PR/merge gate.
**Date:** 2026-11-27
**Branch:** `feature/workspace-ui` (local; `main` untouched)

## Gate evidence

Run in WSL Debian from the feature worktree.

| Gate | Command | Result |
| --- | --- | --- |
| Format | `cargo fmt --all -- --check` | clean |
| Rust | `cargo test --workspace` | green (includes `cargo test -p fnscope-server` = 26) |
| UI build | `bash ui/build.sh` | `dist/app.js` ~251 kB, `dist/app.css` ~37 kB, `dist/theme.js` |
| UI source/dist | `node ui/scripts/check-ui.mjs` | `UI_CHECK_OK (53 source files)` |
| UI self-test | `node ui/scripts/check-ui.mjs --self-test` | `UI_CHECK_SELFTEST_OK` |
| UI helpers | `npm --prefix ui test` | `# pass 5`, `# fail 0` — format, entry ranking, search ranking, trail |
| Release + smoke | `bash scripts/test-all.sh` | `MANIFEST_OK: 9 functions, 8 call edges`, `UI_CONTRACT_OK (9 functions)`, `SERVER_SMOKE_OK`, `ALL_TESTS_OK` |
| Culture | `bash scripts/check-culture.sh` | `CULTURE_OK` |
| Culture self-test | `bash scripts/check-culture.sh --self-test` | `CULTURE_SELFTEST_OK` |
| Workspace simulation | `bash docs/features/workspace-ui/simulate.sh` | `WORKSPACE_SIM_OK`, `UI_CONTRACT_OK (2 functions)` |

## Recorded browser audit

`.reports/ui-workspace-audit.md`, screenshots under `.reports/ui-workspace-audit/`.
Scenarios 1–12 PASS or PASS with a recorded server-level check; Scenario 13
(deep links) is explicitly deferred by the PRD's own escape clause. No fake
delay, random duration or sample data is used: the invoke evidence includes a
real server duration and a `simulated: true` payload from the simulate scope.

## Review and drift

- Independent reviews: J4 CLI PASS; J5 explorer PASS; J6 views PASS after one
  cosmetic fix (`f168c97`); J7 invoke PASS.
- Drift D-7 (Preview → Story) and D-9…D-12 (mobile view control, CSP inline
  script, literal `\u00b7`, fixture call edge) are Resolved in `docs/drift.md`.
- The culture gate caught a test-only absolute path in the CLI before commit;
  it is waived with a named test-input comment.

## Deferred / known limits

- Deep links (`#/scope/<id>/fn/<fnId>/<view>`) — deferred by PRD Scenario 13.
- Live Node invoke on the workspace fixture — both scopes are `simulate`; the
  server's Node tests and the earlier single-scope audit cover execution.
- A real zero-function scope was not produced by a fixture; the shared empty
  state is source-reviewed but not screenshotted.
- Bundled IBM Plex fonts are still pending; the product falls back to system
  fonts and loads no CDN asset.
- Cross-scope call resolution and multi-hop graphs are non-goals.

## Public surface

No `FnGraph`/`FnRecord`/`InvokeRequest`/`InvokeResponse` field was renamed or
removed. The workspace routes are additive; the legacy routes keep serving the
default scope. The no-workspace CLI path keeps the single-scope behaviour.
