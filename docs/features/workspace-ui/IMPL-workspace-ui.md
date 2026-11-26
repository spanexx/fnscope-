# IMPL: fnscope workspace UI

**Slug:** workspace-ui
**Status:** Approved (delegated)
**Date:** 2026-11-26

## Phase Plan

### Phase 0: Guardrails, drift and pack lock

**Build:**
- Add the Preview→Story drift entry and record the server `fn_graph` comment-vs-code validation
  gap in `docs/drift.md` before touching either area.
- Add a path-specific origin mapping so the new UI/scope modules require
  `docs/features/workspace-ui/`; keep the base mapping for untouched base files.
- Add `ui/scripts/check-ui.mjs` and `scripts/check-ui-contract.mjs` with `--self-test` fixtures
  that prove both a good and a bad input (no false-green check without a negative probe).
- Add `ui/package.json` scripts `check`, `check:source`, and `test`; add the pure-helper test.
- Add the UI build and checks to `scripts/test-all.sh` before the release build (the chosen
  freshness mechanism), and wire `check-ui-contract.mjs` into `scan-fixture.sh`.
- Add `.gitattributes` (`* text=auto eol=lf`) and normalize the tracked files that still carry
  CR bytes in the committed fixtures; use `tr -d '\r'`, never `sed 's/\r$//'`.
- Lock the pack documents; freeze the additive contract addendum shape.

**Verify:**
- [ ] `bash scripts/check-culture.sh --self-test` -> `CULTURE_SELFTEST_OK`
- [ ] `bash scripts/check-culture.sh` -> `CULTURE_OK` (pack present, GRILL locked, no CRLF/paths)
- [ ] `node ui/scripts/check-ui.mjs --self-test` -> `UI_CHECK_SELFTEST_OK`
- [ ] `node scripts/check-ui-contract.mjs --self-test` -> `UI_CONTRACT_SELFTEST_OK`
- [ ] `git diff --check` clean

**Blocked by:** nothing.

### Phase 1: Server scope registry and additive API

**Build:**
- New `crates/fnscope-server/src/workspace.rs` (or `scope.rs`) with `ScopeConfig`,
  `WorkspaceConfig`, `ScopeDescriptor`, `ScopeList`, per-scope cache and error handling.
- Additive routes: `/api/scopes`, `/api/scopes/{id}/fn-graph.json`, `/api/scopes/{id}/invoke`;
  default-scope wrappers for the existing routes.
- Contract addendum in `docs/contract/rust-api.md`; the **orchestrator writes it**, no locked
  symbol renamed/removed.
- Path canonicalization, scope-id whitelist, Host/Origin checks, CSP header for embedded assets,
  manifest/request size and count limits, loopback default/no CORS, Node timeout/process cleanup,
  concurrency cap, no source in logs.
- Tests: two-scope isolation, same `fnId` collision, unknown/corrupt scope, default compatibility,
  cache invalidation, traversal rejection, origin rejection.

**Verify:**
- [ ] `cargo fmt --all -- --check`
- [ ] `cargo test -p fnscope-server`
- [ ] `cargo test --workspace`
- [ ] `bash scripts/smoke-server.sh` still prints `SERVER_SMOKE_OK`

**Blocked by:** Phase 0.

### Phase 2: CLI workspace configuration and multi-scope watching

**Build:**
- `fnscope.workspace.json` parsing and validation; relative-path resolution; duplicate/empty id
  rejection; absolute-path warning.
- `open` flags `--workspace`, `--scope ID=PATH`, `--default-scope`; env equivalents.
- Workspace scan into `.fnscope/scopes/<id>/fn-graph.json`; single-scope behavior unchanged.
- Multi-scope watcher with per-scope failure isolation; keep the existing single-scope watcher.
- Tests for parsing, precedence, single-scope compatibility, watcher fingerprints.

**Verify:**
- [ ] `cargo fmt --all -- --check`
- [ ] `cargo test -p fnscope-cli`
- [ ] A two-scope fixture scan produces both manifests
- [ ] `bash scripts/test-all.sh` -> `ALL_TESTS_OK`

**Blocked by:** Phase 1 public route/type shape.

### Phase 3: UI foundation, theme and shell

**Before this phase:** run the `opensrc` skill for the OFL `@fontsource` IBM Plex packages and
record the dependency analysis below.

**Build:**
- `ui/src/lib/{types,graph,format}.ts`, `ui/src/api.ts`, `ui/src/state/*`.
- `ui/src/components/{AppShell,ScopeSwitcher,ThemeProvider,Toast,EmptyState,Badge,Button,
  IconButton,CodeBlock}.tsx`.
- `ui/src/main.tsx` fetch/mount only; scope list fetch with compatibility fallback.
- Paper/Graphite CSS variables, bundled IBM Plex, Tailwind build, no CDN; one asset bundle.
- Loading/empty/error states; provenance strip; platform-aware shortcut hints; global shortcut
  registry; focus/inert primitives.

**Verify:**
- [ ] `node ui/scripts/check-ui.mjs --source` -> `UI_CHECK_OK`
- [ ] `npm test` pure-helper tests green
- [ ] Reviewer PASS; the orchestrator runs `bash ui/build.sh` after integration and records
      emitted sizes
- [ ] The build works from PowerShell/WSL and emits one bundle

**Blocked by:** Phase 1 JSON shape; Phase 0 guardrails.

### Phase 4: Explorer

**Build:**
- `ui/src/explorer/{Explorer,Search,FunctionList,ModuleTree,entryPoints}.tsx|ts`.
- Graph-derived entry points with 24-item cap and show-all; module tree; search; selection and
  reveal; breadcrumb prepend/append/truncate/back.
- Scope switch resets selection/trail/search/run/step state; remount by scope id.
- Disjoint from views/invoke; imports only Phase 3 types/primitives.

**Verify:**
- [ ] `node ui/scripts/check-ui.mjs --source`
- [ ] Reviewer PASS on the changed files
- [ ] Orchestrator browser audit after integration: scope switch, entry/module/search,
      breadcrumb/back, no stale selection

**Blocked by:** Phase 3 shell/types.

### Phase 5: Story, 1-Hop, Split and Raw

**Build:**
- `ui/src/views/{StoryView,MapView,SplitView,RawView,HeaderCard,StepList,Panels}.tsx`.
- Real `FnRecord` data: caller/inputs/steps/returns/errors; six-step display with expansion;
  full source; complete raw JSON; copy actions.
- 1-Hop caller/current/callee jumps; outside-scope handling; empty states.
- No curated overrides or invented step ids.

**Verify:**
- [ ] `node ui/scripts/check-ui.mjs --source`
- [ ] Reviewer PASS
- [ ] Orchestrator browser audit after integration: all four views, step expansion,
      caller/callee jumps, raw copy

**Blocked by:** Phase 3 shell/types; Phase 4 selection/trail contract.

### Phase 6: Invoke drawer

**Build:**
- `ui/src/invoke/{InvokeDrawer,ArgsEditor,TraceOutput,useInvoke}.tsx|ts`.
- Per-scope invoke route; real `InvokeResponse`; trace join by step id; measured duration.
- JSON validation/focus recovery; reset; run token/AbortController; late-response guard.
- Simulate/live capability from scope descriptor; honest labels; no fake delay.
- Error/fallback rendering.

**Verify:**
- [ ] `node ui/scripts/check-ui.mjs --source`
- [ ] Reviewer PASS
- [ ] Orchestrator browser audit after integration: valid run, invalid JSON, `ok:false`, HTTP
      error, scope switch during run
- [ ] `node scripts/check-ui-contract.mjs` -> served fields/scope/invoke checks green

**Blocked by:** Phase 3 shell/types; Phase 4 selection; Phase 5 step ids.

### Phase 7: Accessibility, responsive and public polish

**Build:**
- Focus trap/restore for drawer/menu; skip link; `aria-live` toasts; `aria-current`;
  `aria-pressed`/`aria-expanded`; visible focus; reduced motion.
- Sidebar/drawer overlays below 1180px; backdrop; no page overflow; 44px targets; 200% zoom.
- Remove fake actions; add copy-path/refresh behavior; real empty/error copy; offline assets.
- Optional hash deep links if the core is green; otherwise record the deferral.

**Verify:**
- [ ] Keyboard-only path: search → select → views 1–4 → drawer → Run → Escape
- [ ] Viewports 375×667, 768×1024, 1440×900: no horizontal scroll, overlays and focus return
- [ ] Reviewer PASS
- [ ] Orchestrator runs the build and offline check after integration: no external requests, no
      CDN strings in `ui/dist`

**Blocked by:** Phases 3–6.

### Phase 8: Acceptance harness, post-impl sim, drift and full gate

**Build:**
- `docs/features/workspace-ui/simulate.sh` (or `.html`) starting the real two-scope fixture;
  API assertions and the browser checklist.
- `.reports/ui-workspace-audit.md` with recorded PASS/FAIL/PARTIAL per scenario. Drive the real
  browser with the `browse` MCP (real Chrome) or the in-app `browser` tool; record the tab/URL,
  assertions and relative screenshot names under `.reports/ui-workspace-audit/`.
- `docs/features/workspace-ui/VERIFY.md` with exact markers.
- Drift check sub-agent; resolve or log gaps; reconcile simulations.
- Update `docs/STATUS.md`, `README.md`, `CONTEXT.md` if terms emerged.

**Verify:**
- [ ] `bash ui/build.sh`
- [ ] `node ui/scripts/check-ui.mjs`
- [ ] `node scripts/check-ui-contract.mjs`
- [ ] `cargo fmt --all -- --check`
- [ ] `cargo test --workspace`
- [ ] `bash scripts/test-all.sh` -> `ALL_TESTS_OK`
- [ ] `bash scripts/smoke-server.sh` -> `SERVER_SMOKE_OK`
- [ ] `bash scripts/check-culture.sh --self-test` then `CULTURE_OK`
- [ ] Recorded browser audit covers every PRD scenario
- [ ] Drift report settled; PR-ready evidence

**Blocked by:** Phases 0–7.

## Phase Dependencies

```
0 -> 1 -> 2
0 -> 3 -> 4 -> 5 -> 6 -> 7 -> 8
1 ---------^    ^
2 --------------^ (CLI scope fixture for acceptance)
```

Phases 1 and 2 can overlap only after Phase 1 freezes the public type shape. Phases 4, 5 and 6
can run in parallel after Phase 3, with disjoint directories. Phase 7 waits for all three. Phase 8
is serial integration.

## Test Strategy

- Rust: unit/integration tests beside the server/CLI modules; scope isolation and security cases
  are mandatory.
- UI pure helpers: bundle with the existing esbuild into a temp `.mjs` and run with `node:test`.
- Asset/contract: `ui/scripts/check-ui.mjs` and `scripts/check-ui-contract.mjs`.
- Rendered behavior: recorded real-browser audit; every PRD scenario gets PASS/FAIL/PARTIAL, with
  screenshots or DOM assertions.
- No new test framework in this pack unless the audit proves insufficient; that would be a new
  pack/dependency decision.
- **Build ownership:** workers never run `bash ui/build.sh` or write `ui/dist`. They run the
  source check and their pure-helper tests. The orchestrator builds after each integration and
  owns the embedded assets.

## Dependency Analysis (opensrc)

**Before Phase 3:** `@fontsource/ibm-plex-sans`, `@fontsource/ibm-plex-mono`,
`@fontsource/ibm-plex-serif`.

- **Name + version:** `@fontsource/ibm-plex-{sans,mono,serif}@5.3.0`.
- **License:** OFL-1.1.
- **Maintenance:** active; npm registry `time.modified` is 2026-07-19 for all three.
- **Why:** self-hosted IBM Plex woff2 files keep the validated typography and make the app
  offline/self-contained. The CDN version is rejected (external request, privacy, flake).
- **Alternatives considered:** system sans/mono stack (loses the approved identity), Google Fonts
  CDN (rejected for offline/privacy), copying Windows fonts (not portable/licensed).
- **Call pattern:** import the family CSS from `ui/src/input.css`/`ui/src/main.tsx`; esbuild and
  the Tailwind build bundle the woff2 assets into the single embedded asset set.
- **opensrc note:** the `opensrc` CLI is not installed in this environment; npm registry metadata
  was checked instead. The packages are small OFL assets with no runtime code. If the source is
  needed later, run `opensrc path @fontsource/ibm-plex-sans --cwd ui` after installing the CLI.

No other new runtime dependency is planned. If a test framework is added, it needs its own
opensrc entry and IMPL update.

## Rollout

- Feature branch `feature/workspace-ui`; the orchestrator commits the integrated tree. Workers
  may commit locally in their own worktrees only under the documented fast-mode exception; they
  never merge, push or touch the integration branch.
- Single-scope users keep the existing routes and behavior; workspace users opt in with config.
- Old UI fallback: a new UI against an old server gets 404 from `/api/scopes` and falls back to
  the default `/api/fn-graph.json`; an old UI against the new server uses the default wrappers.
- No `main` merge or release without the human PR gate.

## Risk Notes

- The additive scope API is the largest contract risk; freeze its shape in Phase 1 and review it.
- Per-scope caller semantics change entry-point counts; test with a shared function name.
- Full source/steps can be large; optimize rendering before adding an endpoint.
- Node invoke is RCE; security checks are must-ship, not polish.
- The UI is a near-total rewrite; keep the object-arg repair and single asset bundle to avoid
  breaking TS/JS invocation.
