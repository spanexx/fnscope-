# Workspace UI — recorded browser audit

**Date:** 2026-11-27
**Pack:** `docs/features/workspace-ui/`
**Fixture:** `tests/fixtures/workspace/fnscope.workspace.json` (scopes `alpha` and `beta`, both `invoke: simulate`)
**Server:** `cargo run -p fnscope -- open --workspace tests/fixtures/workspace/fnscope.workspace.json --port 4320 --host 127.0.0.1 --no-watch`
**Browser:** in-app browser tab `tab_cafed5e2-9135-40fa-8396-1633be68b2e6`, plus headless Chrome `158.0.8127.37` over CDP for viewport measurements.

Every assertion below was read from the running product. No sample data, curated override or
fabricated timing was used.

## Fixture facts asserted through the API

- `/api/scopes` returned `defaultScope: alpha` and exactly two available scopes.
- `rootDir` was `alpha` / `beta`; no absolute paths.
- Each scoped graph had exactly two functions (`alphaEntry`/`shared`, `betaEntry`/`shared`),
  real `sourceCode` and real steps.
- Each entry recorded a resolved call edge to its own `shared`, and `shared.calledBy` named the
  entry as a resolved caller. The two `shared` bodies differ (`* 2` vs `* 10`).
- Compatibility `/api/fn-graph.json` served the default (alpha) graph.
- `docs/features/workspace-ui/simulate.sh` repeats these assertions plus scoped invoke and prints
  `WORKSPACE_SIM_OK`; `node scripts/check-ui-contract.mjs` then prints `UI_CONTRACT_OK (2 functions)`.

## Scenario checklist

| Scenario | Verdict | Evidence |
| --- | --- | --- |
| 1. Single-scope compatibility | PASS (API) | `bash scripts/smoke-server.sh` prints `SERVER_SMOKE_OK`; `/api/scopes` exists in single-scope mode. The workspace browser run covers the multi-scope UI. |
| 2. Workspace scopes | PASS | `Alpha` first paint; scope card menu lists both scopes with real counts, language counts, scan timestamps and relative roots. Switching to Beta swapped the explorer, story, trail and drawer route to `/api/scopes/beta/*`. |
| 3. Scope switch resets state | PASS | Beta switch with the drawer open: selection became `betaEntry`, trail `[betaEntry()]`, drawer title/args/template/route switched to Beta, old trace absent, Back disabled. |
| 4. Entry points / show-all | PASS | Header count came from zero-resolved-caller candidates; after the call-edge fix only `alphaEntry` remained an entry point. The 24 cap/show-all branch is covered by `ui/scripts/test-lib.mjs` (`entryPointPage` hidden/total). |
| 5. Module tree and search | PASS | Tree showed `src (2) → main.ts (2)` with `aria-expanded`; selecting a function auto-expanded it. Search `ALPHA` matched one record case-insensitively; clearing restored the entry list. |
| 6. Selection and breadcrumb | PASS | `Inspect shared()` from the story appended `shared()`; the 1-Hop callee card did the same. Caller `Jump` prepended the caller. Back removed the last crumb; at one crumb Back was disabled. Crumbs truncate through the same helper used by the prototype. |
| 7. Story view | PASS | Header badges (module, TypeScript, pure, entry point), signature, description, `src/main.ts:5`, id, step/caller counts, Invoke/Copy. Triggered by, Inputs, Execution story (2/2 steps, real `step-1`/`step-2`), Returns and Declared errors all rendered from the record. |
| 8. 1-Hop, Split, Raw | PASS | 1-Hop: real caller `entrypoint` (external) and resolved callee `shared` with Jump; Split: full real source beside the compact story; Raw: complete JSON with `id`, `astHash` and `sourceCode`, plus Copy JSON. |
| 9. Invoke | PASS | Simulate run posted to `/api/scopes/alpha/invoke`, returned `ok`, `simulated: true`, `durationMs: 3.88` and a trace keyed by `step-1`; the scope label said Simulate and Live Node was disabled with an explanatory title. Invalid JSON produced `Invalid JSON: Unexpected end of JSON input`, focused the textarea and kept the previous trace recoverable. Beta re-ran against its own route. |
| 10. Theme, keyboard, responsive | PASS | Graphite toggle persisted across reload with no inline-script CSP error. CDP key events: Ctrl+K focused search, Escape blurred it, `3`/`1` switched Split/Story, Alt+Left went back, Ctrl+Enter opened then ran the drawer (trace present), Escape closed it. |
| 11. Loading / empty / error | PASS (error), PARTIAL (empty) | Server stopped mid-session, scope switch showed `Scope could not load` / `Could not load Alpha` / `Failed to fetch` with Retry in both sidebar and canvas; restarting and pressing Retry restored `alphaEntry`. An empty scope is covered by the shared empty-state component but was not produced by a real zero-function manifest. |
| 12. Local security boundaries | PASS (server suite) | `cargo test -p fnscope-server` includes unknown-scope, traversal, Origin/Host, body-limit and corrupt-manifest tests; `simulate.sh` proves a beta entry id is rejected by alpha's invoke route. |
| 13. Optional deep links | DEFERRED | Not implemented; recorded as deferred in the pack, per Scenario 13's own escape clause. |

## Viewport measurements (headless Chrome, CDP)

| Viewport | Document width / overflow | Header controls | Overlay behaviour |
| --- | --- | --- | --- |
| 375×667 | 375 / none | Explorer, scope, compact view menu, theme, drawer all inside 0–375 | Drawer 320 px and sidebar 270 px open as overlays with a backdrop; no overflow |
| 768×1024 | 768 / none | Breadcrumb, four view buttons, theme, drawer visible | Same overlay behaviour |
| 1440×900 | 1440 / none | Sidebar docked (270 px), drawer docked (320 px), breadcrumb and view tabs visible | Not modal |

The 375 px layout needed a compact view menu; before that fix the four view buttons were
`display:none` below 640 px and the drawer button sat 1 px outside the viewport. Both are fixed
and re-measured (drawer button right edge 367 px).

## Screenshots

- `.reports/ui-workspace-audit/cdp-375x667.png` — mobile Story, open drawer disabled state, compact view control.
- `.reports/ui-workspace-audit/cdp-768x1024.png` — tablet with breadcrumb and view tabs.
- `.reports/ui-workspace-audit/cdp-1440x900.png` — desktop, Paper, docked sidebar and drawer.
- `.reports/ui-workspace-audit/graphite-beta-1440.jpeg` — Graphite palette after switching to Beta.

## Fixes made because of this audit

1. The inline palette bootstrap was blocked by `script-src 'self'`. It is now `ui/src/theme.js`,
   copied to `/theme.js`, embedded and served by the server; the inline hash exception is not needed.
2. Two JSX text nodes rendered `\u00b7` literally (`ProvenanceStrip`, `ScopeSwitcher`); both now use
   the character in an expression.
3. The workspace fixture's `return shared(x)` never produced a call edge (the TypeScript adapter
   classifies return statements as `Return`). Both fixtures now call the helper in a declaration,
   and `shared_call_edge_stays_inside_its_scope` proves the resolved edge stays inside its scope.
4. Mobile had no way to change views. A compact icon menu now exposes all four views below 640 px.
5. The 375 px header overflowed by one pixel; the mobile gap/padding was tightened.

## Known limitations (recorded, not hidden)

- Live Node invoke was not exercised here: both fixture scopes are `simulate`. The server's Node
  tests and the earlier `.reports/ui-browser.md` Node run cover execution; the drawer correctly
  disables Live Node for simulate scopes.
- A zero-function scope was not produced. The empty state is the same component used by the
  sidebar/canvas and is covered by source review, but no screenshot exists.
- `Cmd/Ctrl+Enter` opens the drawer on the first press when it is closed and runs on the next press
  (or immediately when focus is inside the drawer). This is a superset of the prototype, which only
  ran when the drawer was already open.
- After a caller jump the prototype prepends the caller while the canvas shows it; the last
  breadcrumb stays the previously selected function. This port keeps the validated prototype
  behaviour rather than silently changing the call-path semantics.
