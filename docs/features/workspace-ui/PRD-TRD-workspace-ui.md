# PRD-TRD: fnscope workspace UI

**Slug:** workspace-ui
**Status:** Approved (delegated)
**Date:** 2026-11-26

## Why This Exists

fnscope already scans code and exposes a function graph, but the current UI is a single dark
explorer that opens on a hardcoded fixture function, silently falls back to sample data, and
shows every function in one flat strip. A reviewer cannot tell which project they are looking at,
cannot isolate two scanned repositories, and cannot move from a caller to a callee without losing
the thread. The validated prototype fixes that with scopes, an entry-point/module explorer, a
story canvas, and a sandbox that follows the selected function.

This pack replaces the old explorer surface with the validated design while keeping the locked
Rust API and CLI compatible. The result should let someone understand a function's inputs,
callers, execution story and outputs without reading the source first, and without accidentally
mixing two codebases.

## Behavioral Spec

### Scenario 1: Single-scope compatibility

**Given** a user runs `fnscope open <dir>` with no workspace configuration
**When** the server starts
**Then** one implicit `default` scope is served, `/api/fn-graph.json` and `/api/invoke` behave as
before, and the UI shows that project without a scope switcher or fake demo data.

### Scenario 2: Workspace scopes

**Given** a `fnscope.workspace.json` names two or more scopes
**When** the user opens the workspace
**Then** `/api/scopes` lists each scope with id, label, relative root, counts, languages,
availability and invoke mode; the UI opens the configured default; a scope card switches the
whole explorer to that scope's own manifest and call edges.

### Scenario 3: Scope switch resets state

**Given** a function is selected with an open drawer and an expanded step list
**When** the user switches scope
**Then** selection becomes the new scope's default function, the trail resets, search clears,
the drawer run/args/error reset, step expansion resets, and any in-flight invoke cannot attach to
the new selection.

### Scenario 4: Entry points and show-all

**Given** a scope is loaded
**When** the explorer is in Entry points mode
**Then** functions with zero resolved internal callers are ranked by step count and name, the
first 24 are shown, and a control reveals the remaining candidates with a visible count. No
hardcoded function-name allowlist is used.

### Scenario 5: Module tree and search

**Given** a scope is loaded
**When** the user expands a group/file or types in search
**Then** the module tree groups by real paths, shows per-file counts, auto-expands the selected
function, and search matches name/id/module/file path case-insensitively with results capped and
an explicit match count. Clearing restores the previous list.

### Scenario 6: Selection and breadcrumb

**Given** a function is selected
**When** the user follows a caller or callee, clicks a breadcrumb, or presses Back
**Then** the trail records the path, the current crumb is distinct, earlier crumbs truncate the
trail, Back is disabled at the root, and an in-scope callee opens the real function record.

### Scenario 7: Story view

**Given** a function record
**When** Story is active
**Then** the header shows module, language, purity, entry-point state, signature, description,
file:line, id, step count and caller count; sections show Triggered by, Inputs, Execution story
(real step ids, six visible, expandable), Returns and Declared errors; empty states are explicit.

### Scenario 8: 1-Hop, Split and Raw

**Given** a function record
**When** the user selects 1-Hop, Split or Raw
**Then** 1-Hop shows callers/current/callees with jumps; Split shows the real source beside the
story; Raw shows the complete `FnRecord` JSON and copies it. None of these views synthesize facts
that are absent from the manifest.

### Scenario 9: Invoke

**Given** a selected function and valid JSON arguments
**When** the user runs it
**Then** the UI posts to the selected scope's invoke route, shows the real `InvokeResponse`,
joins trace entries by step id, displays measured duration, and renders `ok:false`, HTTP errors,
invalid JSON and unknown-function errors as recoverable failures. No fake delay or random
duration is used. Live mode is available only when the server/scope is configured for Node and is
labelled honestly; otherwise the UI explains that the scope is simulate-only.

### Scenario 10: Theme, keyboard and responsive layout

**Given** the app is open
**When** the user toggles Paper/Graphite, uses Cmd/Ctrl+K, `/`, `1`–`4`, Escape, Alt+Left or
Cmd/Ctrl+Enter, or narrows the window
**Then** the theme persists; shortcuts do not fire while typing; the drawer becomes an overlay
with a working backdrop below 1180px; the view switcher remains reachable; the page has no
horizontal overflow; and focus returns after closing overlays.

### Scenario 11: Loading, empty and error states

**Given** the manifest is loading, missing, corrupt or empty
**When** the app boots or the user retries
**Then** the UI shows a named loading/error/empty state with the exact command or retry action,
never sample data presented as real. Scope-level errors do not break other scopes.

### Scenario 12: Local security boundaries

**Given** a request with an unknown scope id, traversal path, foreign Origin/Host, or an oversized
manifest
**When** it reaches the server
**Then** it is rejected without exposing another scope's source or executing a path outside the
scope root; Node invoke is opt-in, bounded and cleaned up.

### Scenario 13: Optional deep links

**Given** the core is green
**When** the user copies a link or refreshes
**Then** `#/scope/<id>/fn/<fnId>/<view>` restores the scope, function and view; browser Back
returns the previous state. If the core is not green, this is deferred and recorded.

## Simulation Contract

The pre-impl simulation is the validated prototype under `wireframe/fnscope-workspace/`; the
post-impl simulation must start the real server with a two-scope fixture and demonstrate:

```bash
# API-level isolation
curl /api/scopes                         # two scopes, correct default, no absolute roots
curl /api/scopes/alpha/fn-graph.json     # alpha-only functions/edges
curl /api/scopes/beta/fn-graph.json      # beta-only functions/edges
curl /api/scopes/alpha/invoke            # alpha fn works; beta fn is unknown
curl /api/fn-graph.json                  # default-scope compatibility route
curl /api/invoke                         # default-scope compatibility route

# UI acceptance (recorded browser audit)
# open the served URL, run the checklist in docs/features/workspace-ui/simulate.sh:
# scope switch, entry points, module tree, search, breadcrumb, Story/1-Hop/Split/Raw,
# invoke simulate/live/error, theme, shortcuts, responsive overlay, loading/error/empty.
```

The recorded audit must include screenshots or captured DOM assertions for every checklist item,
not a prose claim. Use the `browse` MCP (real Chrome) or the in-app `browser` tool; record the
tab/URL, assertions and relative screenshot names in `.reports/ui-workspace-audit.md` (images
under `.reports/ui-workspace-audit/`). The post-impl sim is not the prototype with a different
stylesheet.

## Technical Design

### Server

- New `workspace` module in `crates/fnscope-server`:
  `ScopeConfig { id, label, note, scan_root, display_root, manifest_path, default_function_id,
  invoke_mode }`, `WorkspaceConfig { scopes, default_scope, bind, invoke_mode }`,
  `ScopeDescriptor`, `ScopeList`.
- Additive routes: `GET /api/scopes`, `GET /api/scopes/{id}/fn-graph.json`,
  `POST /api/scopes/{id}/invoke`. Existing `/api/fn-graph.json` and `/api/invoke` remain as
  default-scope wrappers. Unknown scope is 404; a corrupt scope is `available:false` in the list
  and does not fail others.
- Per-scope cache keyed by manifest stamp; serve the locked `FnGraph` schema unchanged. Validate
  the graph before serving. The scope id is a whitelist key; it is never joined into a path.
  Canonicalize `scan_root` and Node module paths.
- Add a contract addendum to `docs/contract/rust-api.md` for the new routes/types. Do not rename
  or remove locked symbols. Record the server's comment-vs-code validation gap in
  `docs/drift.md` before touching it.
- Security: loopback default, no CORS, Host/Origin checks, CSP for embedded assets, request/
  manifest size and count limits, Node timeout and process-tree cleanup, concurrency cap, no
  source in logs.

### CLI and configuration

- `fnscope.workspace.json` with `version`, `defaultScope`, `scopes[]` (`id`, `label`, `note`,
  `path`, optional `include`/`exclude`/`invoke`/`defaultFunctionId`). Relative paths resolve
  against the config file; absolute paths are rejected or warned.
- `open` flags: `--workspace`, repeatable `--scope ID=PATH`, `--default-scope`; env
  `FNSCOPE_WORKSPACE`/`FNSCOPE_SCOPE` optional. No workspace config = exact single-scope
  behavior. Workspace mode writes `.fnscope/scopes/<id>/fn-graph.json`; enrichment cache stays
  shared.
- Watcher: one loop over scopes, rescan only changed roots, log and continue on one scope's
  failure. Do not change locked `ScanOptions` defaults or auto-discover repos.

### UI

- Replace the monolithic `FunctionExplorer` with:
  `ui/src/lib/{types,graph,format}.ts` (pure helpers), `ui/src/state/*` (scope/selection/trail/
  invoke), `ui/src/components/{AppShell,ScopeSwitcher,ThemeProvider,Toast,EmptyState,Badge,
  Button,IconButton,CodeBlock}.tsx`, `ui/src/explorer/*`, `ui/src/views/*`,
  `ui/src/invoke/*`, and `ui/src/api.ts`.
- `main.tsx` only fetches scope data and mounts the shell. Keep the existing TS/JS object-arg
  repair adapter unless the new invoke path proves it unnecessary.
- Theme: CSS variables for Paper/Graphite, bundled IBM Plex (OFL `@fontsource` packages, pending
  opensrc analysis), lucide-react, existing Tailwind build. No CDN. Default Paper; persist.
- State: scope switch remounts by scope id; function selection resets run/args/step expansion;
  in-flight runs use a token or AbortController.
- Views: Story, 1-Hop, Split, Raw; six initial steps; full source from `sourceCode`; no curated
  overrides. Deep links are optional after core green.
- Accessibility/responsive: landmarks, focus trap/restore, aria-live, aria-current, skip link,
  focus-visible, reduced motion, 270px sidebar/320px drawer, overlay below 1180px, 44px targets.

### Tests and evidence

- Rust: server scope isolation, default-scope compatibility, unknown/corrupt scope, cache
  invalidation, concurrent requests, path traversal, origin/host checks; CLI parsing/config/
  watcher tests.
- UI: `ui/scripts/check-ui.mjs` (assets, markers, no absolute/CDN paths, source/dist freshness,
  self-test), pure-helper tests via esbuild + `node:test`, `scripts/check-ui-contract.mjs`
  (manifest/scope/invoke fields, self-test), and a recorded real-browser audit.
- Gates: `bash ui/build.sh`, `npm --prefix ui test`, `node ui/scripts/check-ui.mjs`,
  `node scripts/check-ui-contract.mjs`, `cargo fmt --all -- --check`,
  `cargo test --workspace`, `bash scripts/test-all.sh`, `bash scripts/smoke-server.sh`,
  `bash scripts/check-culture.sh --self-test`, `bash scripts/check-culture.sh`.

## Non-Goals

- No vulnerability scanning in this pack; only stable identity and a future findings seam.
- No cloud, accounts, telemetry, remote sync or directory picker.
- No per-request invoke mode; mode is a server/scope capability.
- No new language adapters or parser changes.
- No manual server-side rescan endpoint in v1; the watcher and client refresh are the behavior.
- No Playwright/Vitest dependency in this pack unless the recorded browser audit proves
  insufficient.
- No change to `FnGraph`/`FnRecord`/`InvokeRequest`/`InvokeResponse`/`schemaVersion`.

## Out of Scope (Future)

- Security findings UI, SARIF export and analyzer plugin crate.
- Source/step lazy-load endpoints for very large manifests.
- Playwright/Vitest harness, visual regression and performance budgets.
- Cross-scope call resolution and multi-hop graphs.
- Additional execution runtimes beyond Node.

## References

- `docs/features/workspace-ui/GRILL-workspace-ui.txt`
- `docs/features/workspace-ui/research/discovery-synthesis.md`
- `wireframe/fnscope-workspace/` — validated pre-impl simulation
- `docs/contract/rust-api.md` — locked API and additive addendum
- `docs/drift.md` — Preview→Story and validation gaps
