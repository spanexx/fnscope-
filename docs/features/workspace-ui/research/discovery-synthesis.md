# Workspace UI — discovery synthesis

Targeted exploration for the `workspace-ui` feature pack. Sources: the validated prototype
under `wireframe/fnscope-workspace/`, the locked contract, the current UI/server/CLI, and three
deep-scout briefs (server/scopes, wireframe acceptance, product/security). The UI architecture
brief is pending a Hy4 fallback after the DeepSeek free-tier ceiling; the two briefs agree on the
module split below.

## What exists today

- The UI is one React component, `ui/src/FunctionExplorer.tsx`, plus `ui/src/main.tsx`. It fetches
  one `/api/fn-graph.json`, passes `{ graph, initialFunctionId, onInvoke }`, and falls back to a
  built-in sample graph when the fetch fails. It has Raw / Split / Preview / 1-Hop views and a
  sandbox.
- The server has one `ServerConfig { manifest_path, bind, invoke_mode }` and six routes:
  `/`, `/app.js`, `/app.css`, `/api/health`, `/api/fn-graph.json`, `/api/invoke`
  (`crates/fnscope-server/src/lib.rs`; `docs/contract/rust-api.md:180-201`).
- `FnGraph` has no scope field. `rootDir` is one value per graph. `calledBy`/`callsFnId` are
  manifest-local. Node invoke resolves `rootDir + filePath`.
- `ui/package.json` has only `build: bash build.sh`; there is no UI test or lint command.
- The prototype is a self-contained static app with three demo scopes sliced from one self-scan.
  It is a validated design reference, not a real scope implementation.

## Scope model — the gating decision

The prototype's client-side slices are not real isolation: they retain cross-scope edges, one
`rootDir`, and one invoke target. The real product needs **one manifest per project, scanned
independently, plus a server-side scope registry**.

Additive design (no breaking change to existing names/fields):

- `ScopeConfig { id, label, note, scan_root, display_root, manifest_path, default_function_id,
  invoke_mode }` and `WorkspaceConfig { scopes, default_scope, bind, invoke_mode }`.
- `GET /api/scopes` → descriptors (id, label, note, relative root, generatedAt, counts, languages,
  availability/error, default function, invoke mode).
- `GET /api/scopes/{id}/fn-graph.json` → the same locked `FnGraph` schema.
- `POST /api/scopes/{id}/invoke` → the same locked `InvokeRequest`/`InvokeResponse`.
- Keep `/api/fn-graph.json` and `/api/invoke` as default-scope compatibility routes.
- Per-scope cache; one corrupt scope must not break the list or other scopes.
- Scope id is a whitelist key; never join it into a filesystem path. Canonicalize scope roots and
  module paths; reject traversal.
- Workspace config file (local) with CLI flags; single-scope behavior stays byte-compatible when
  no workspace is configured. Demo scopes for this repo are configured, not hardcoded.
- Do not change `FnGraph`, `FnRecord`, `InvokeRequest`, `InvokeResponse`, or `schemaVersion` for
  scopes. The contract addendum is additive and orchestrator-owned.

This is a public-contract addendum: adding routes/types, not removing or renaming the locked
ones. It is recorded as an agent decision in the GRILL and will be reviewed at the PR gate.

## Prototype acceptance inventory (must work in the real app)

- **Shell/defaults:** active scope, default function, trail, mode, view, theme, sidebar/drawer
  state. Paper is default; theme persists.
- **Scope switcher:** list of real scopes with counts/languages; switching resets selection,
  trail, search, run, args and step expansion; no stale function or edge.
- **Explorer:** entry-point list, module tree, search/clear, selection and reveal, breadcrumb
  prepend/append/truncate/back.
- **Story:** module/language/purity/entry badges; signature; description; file:line; id; step
  count; callers; inputs; ordered steps with kind, code, inputs→output, detail, callee inspect;
  expand/collapse; returns; declared errors; empty states.
- **1-Hop:** callers left, current center, callees right; jump prepends/appends; outside-scope
  names; empty states.
- **Split:** full real source on the left, story on the right, file:line.
- **Raw:** the real `FnRecord` JSON (not a trimmed record) and copy.
- **Invoke:** simulate/live per server capability; args template; reset; invalid JSON; real
  `InvokeResponse` (`ok/data/error/trace/durationMs`); trace joined by step id; no fake delay;
  disabled/explained live mode; per-scope invoke target.
- **Theme:** Paper/Graphite tokens, IBM Plex, bundled assets, persisted toggle.
- **Responsive:** 270px sidebar, 320px drawer, overlay below 1180px, backdrop close, no page
  overflow, reachable view switcher.
- **Keyboard:** Cmd/Ctrl+K, `/`, 1–4, Escape, Alt+Left, Cmd/Ctrl+Enter; platform-aware hint.
- **States:** loading, missing/corrupt manifest, empty scope, no entry points, no search matches,
  no callers/callees/steps/source/errors, invoke running/success/error/fallback, stale manifest.

## Simulated → real

| Prototype behavior | Real behavior |
|---|---|
| Three slices of one scan | Per-scope scanned manifests + registry |
| Curated stories / trimmed steps | Real `FnRecord.steps` and `sourceCode`; no curated overrides |
| Fake 650 ms invoke | Real `POST /api/invoke`, measured duration, real errors/trace |
| `Open source` toast | Copy path + full source in Split/Raw; no fake action |
| `Rescan` toast | Refresh client data / watcher status; no fake rescan endpoint in v1 |
| Live tab always available | Live only when the scope/server is in Node mode; explain otherwise |
| Sample graph fallback | Explicit loading/error/empty states; never show sample data as real |
| Client-side entry allowlist | Graph-derived zero-caller ranking + show-all toggle |
| CDN Tailwind/Lucide/fonts | Bundled CSS/icons/fonts; offline and no external requests |

## Public-product and security must-ship

- First-run/loading/error/empty states with exact command and retry.
- Provenance strip: root, generatedAt, counts, languages, local-only/no-telemetry, simulate/live.
- Remove fake affordances; keep real actions honest.
- Deep links (`#/scope/:id/fn/:fnId/:view`) and browser back/forward if core is green.
- Focus trap/restore for drawer/menu, `aria-live` toasts, `aria-current`, skip link, visible focus,
  reduced motion, 44px targets, 200% zoom.
- Offline bundle; no CDN; CSP; no source in logs.
- Local server hardening: loopback default, Host/Origin checks, path canonicalization for Node
  invoke, timeout/process cleanup, concurrency cap, no CORS.
- Future security extension points only: stable `scopeId + fnId + stepId + astHash`, a separate
  findings model/plugin crate, and a findings route. Do not implement vulnerability scanning now.

## Contract, process and guardrails

- `docs/contract/rust-api.md` is orchestrator-owned. Add only the scope routes/types; preserve all
  locked names, camelCase wire names and `schemaVersion`.
- Add a drift entry for the Preview → Story rename and for any doc-vs-code gap found.
- `scripts/check-culture.sh` requires a locked GRILL with `[agent decision]` + WHY/SOURCE, a
  GRILL/PRD-TRD/IMPL for changed code, LF endings, no unwaived absolute paths, and the origin
  pack. `ui/src` is currently mapped to `fnscope-base`; keep that mapping working and add a
  path-specific mapping/check for the new UI/scope modules if feasible.
- Real commands: `cargo fmt --all -- --check`, `cargo test --workspace`,
  `bash scripts/test-all.sh`, `bash scripts/smoke-server.sh`, `bash scripts/check-culture.sh`
  (`--self-test` first), `bash ui/build.sh`. UI needs a new test/audit command; exact harness is
  decided in the IMPL after the process fallback.
- WSL Debian command lane; detached servers; bounded waits; no cargo while agents write.

## Proposed job split (disjoint ownership)

| Wave | Job | Owns |
|---|---|---|
| 1 | Server scope registry + additive routes + tests | `crates/fnscope-server/**`, new scope module |
| 1 | CLI workspace config + multi-scope scan/watch + tests | `crates/fnscope-cli/**` |
| 2 | UI foundation: theme, API, state, shell, primitives | `ui/src/main.tsx`, `ui/src/App.tsx`, `ui/src/lib/**`, `ui/src/components/**`, `ui/tailwind.config.js`, `ui/index.html`, `ui/src/input.css` |
| 3 | Explorer | `ui/src/explorer/**` |
| 3 | Canvas views | `ui/src/views/**` |
| 3 | Invoke drawer | `ui/src/invoke/**` |
| 4 | A11y, responsive, polish, deep links (sequential after Wave 2; single owner) | `ui/src/components/**` only after the shell is integrated |
| 4 | Acceptance harness, post-impl sim, docs, drift, reconcile | `ui/tests/**`, `scripts/**`, `docs/features/workspace-ui/**` |

`ui/src/types.ts`/`api.ts` and the shell are serialized before the parallel waves. The old
`FunctionExplorer.tsx` is retired only after the new views pass acceptance.

## Risks / open items

- The scope API is an additive contract change; review it at the PR gate.
- Per-scope caller semantics change entry-point counts and 1-Hop content.
- Full source/steps may make the manifest large; client lazy rendering first, source endpoint only
  if evidence demands it.
- Curated prototype narratives must be dropped or clearly marked; real traces use manifest ids.
- Live mode is server-config, not per-request, unless a later additive API is approved.
- UI test harness (Vitest/Playwright vs a lighter check) is pending the process fallback.
