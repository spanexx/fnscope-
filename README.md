# fnscope

## What it does

fnscope reads a TypeScript, JavaScript or Rust source tree and builds a deterministic function graph: signatures, ordered execution steps, upstream callers and downstream calls. It then serves a local web explorer with four views (Raw, Split, Preview and 1-Hop Map) plus a sandbox invoke panel. An optional, hash-cached LLM pass can fill in plain-English descriptions and sample arguments. There is no account, no cloud service and no telemetry: it is a single local binary. The binary is built in Rust and the React UI is bundled into it, so there is no separate frontend to install or run.

## Quick start

From the repository root in WSL Debian:

```bash
cargo build --release -p fnscope
./target/release/fnscope scan ./tests/fixtures/sample_app/src
./target/release/fnscope ./tests/fixtures/sample_app/src
./target/release/fnscope ./tests/fixtures/rust_app/src
```

Then open <http://localhost:4310>. Press Ctrl-C to stop the server. The second and third commands are the bare-target form of `open`: each scans once, serves the UI and watches for file changes. `.rs` files are scanned by default alongside `.ts`, `.tsx`, `.js` and `.jsx`, so the same command also works on a Rust tree such as `crates/`.

From Windows PowerShell, the same steps look like this (the WSL shell inherits your current directory):

```powershell
wsl.exe -d Debian --exec bash -lc 'cargo build --release -p fnscope'
.\target\release\fnscope.exe scan .\tests\fixtures\sample_app\src
.\target\release\fnscope.exe .\tests\fixtures\sample_app\src
.\target\release\fnscope.exe .\tests\fixtures\rust_app\src
```

`cargo` is not on the Windows PATH in this environment, so build through WSL. Running the already-built binary from PowerShell is fine.

## Commands and flags

Exact flags from `--help`.

### `scan`

```text
fnscope scan [OPTIONS] <TARGET_DIR>
```

| Flag | Meaning |
| --- | --- |
| `--out <OUT>` | Output directory for generated artifacts (default `.fnscope`) |
| `--enrich` | Enrich the graph using an LLM plus the local cache |
| `--model <MODEL>` | Model name for enrichment (defaults to the configured low-cost model) |
| `--include <INCLUDE>` | Glob pattern of files to include; repeatable |
| `--exclude <EXCLUDE>` | Glob pattern of files to exclude; repeatable |

Defaults: includes `**/*.ts`, `**/*.tsx`, `**/*.js`, `**/*.jsx`, `**/*.rs`; excludes `node_modules`, `dist`, `**/target/**`, `*.test.ts`, `*.spec.ts` and similar.

### `open`

```text
fnscope open [OPTIONS] [TARGET_DIR]
```

| Flag | Meaning |
| --- | --- |
| `--port <PORT>` | Port to listen on (default `4310`) |
| `--host <HOST>` | Host address to bind (default `127.0.0.1`) |
| `--no-watch` | Disable the live file watcher |
| `--invoke <INVOKE>` | How `/api/invoke` executes functions: `simulate` or `node` (default `simulate`) |
| `--enrich` | Enrich the graph using an LLM plus the local cache |
| `--model <MODEL>` | Model name for enrichment |

`fnscope <TARGET>` is shorthand for `fnscope open <TARGET>`, and the `open` flags also work at the top level, so `fnscope ./src --port 4311` and `fnscope open ./src --port 4311` are the same.

## Artifacts

Everything fnscope writes lands in `.fnscope/` (default; change with `--out`). You can gitignore it or commit it for teammates.

- `fn-graph.json` — the full function graph: functions, parameters, steps, return shapes, errors, call edges and source code. This is what the UI reads.
- `enrich-cache.json` — the LLM enrichment cache, keyed by each function's `astHash`.

`astHash` is a SHA-256 of the function after comments and all whitespace outside string or template literals have been stripped (`sha256:<64 hex>`). It represents the code's shape, not its formatting. Adding blank lines or spaces inside a function leaves the hash unchanged, so the next `--enrich` run is a cache hit and makes zero new API calls. Changing a string literal or the code's meaning changes the hash and triggers a new call.

## Enrichment setup

Enrichment is off by default. Add `--enrich` to `scan` or `open` to turn it on.

Set one of these API keys: `OPENAI_API_KEY`, `GEMINI_API_KEY` or `ANTHROPIC_API_KEY`. fnscope uses the first non-empty one it finds.

- `FNSCOPE_LLM_BASE_URL` — override the chat-completions endpoint. Point this at a local mock or proxy to test without a real API; a plain `http://127.0.0.1:<port>/...` URL is intentional and supported.
- `FNSCOPE_LLM_MODEL` — override the default fast/low-cost model. A `--model NAME` flag takes precedence over this variable.

Offline behavior: with no key, `--enrich` prints a warning and falls back to deterministic descriptions and step text with zero API calls. If you pass `--model NAME` without any key, fnscope exits with a readable error rather than silently proceeding.

## Invoking functions

The explorer's invoke panel calls `POST /api/invoke`. How it runs depends on `--invoke`:

- `simulate` (default, and what the smoke tests use) — returns a deterministic trace built from the function's steps. It never runs project code.
- `node` — writes a temporary runner, imports the target module and calls the real named export (class methods as `Class.method`), then returns JSON with `ok`, `data`, `error` and `trace`.

Real execution of TypeScript needs Node 24, which strips TypeScript types by default and resolves `.ts` specifiers as written. `FNSCOPE_NODE` selects which Node executable to use; if unset, fnscope uses `node` from PATH. Be warned: Node invocation is best effort. If the chosen Node is missing or incompatible (for example, an older Node that cannot import `.ts` files), fnscope falls back to the simulated trace and adds an `error` field explaining why. It does not fail the request.

## Verification

Run these in WSL. Neither script should be run while agents are still writing crate files.

- `bash scripts/test-all.sh` — the single gate. It checks formatting, runs the workspace tests, builds the release binary, scans the sample fixture and smoke-tests the HTTP server as separate steps. Prints `ALL_TESTS_OK` only when all five stages pass.
- `bash scripts/enrich-e2e.sh` — exercises the real CLI, a local mock LLM and the on-disk cache together. It proves that the first scan is all cache misses with one API call per function, an unchanged second scan is all cache hits with zero API calls, a formatting-only edit makes no new call, and a semantic edit does. Prints `ENRICH_E2E_OK` on success.

## Architecture map

- `fnscope-model` — the JSON schema types shared by every crate, including the graph, cache and invoke shapes.
- `fnscope-parser` — tree-sitter discovery of functions and analysis of their bodies for steps, errors and purity, for TS/JS and Rust (via `tree-sitter-rust`).
- `fnscope-core` — the scanner: globs files, builds the graph, computes `astHash`, writes and reads the manifest.
- `fnscope-enrich` — hash-cached LLM enrichment, cache read/write and the no-op offline fallback.
- `fnscope-server` — the axum HTTP routes, embedded UI assets and `/api/invoke` in simulate or node mode.
- `fnscope-cli` — the `fnscope` binary: clap parsing and the `scan` / `open` commands.
- `ui/` — the React explorer (Raw, Split, Preview and 1-Hop Map views); built into `ui/dist` and embedded into the binary at compile time.

## Limits

- Rust source support is new: `.rs` files are extracted with `tree-sitter-rust` into the same `fn-graph.json` schema (`schemaVersion: 1.0.0`) as TS/JS — no separate manifest format or schema version. Runtime labels remain `universal` or `node20` even for Rust functions; they describe the target environment carried in the graph, not a Rust executor.
- Node invocation does **not** execute Rust code. `--invoke node` is a TS/JS runner: it imports a TypeScript/JavaScript module through Node. Rust functions are inspected and navigated like any other function, but invoking one returns the deterministic simulated trace, never a real Rust execution.
- Cross-file type resolution covers type aliases and interfaces within the scanned set; types from outside it fall back to their written names.
- Class constructors are currently listed as functions alongside regular methods.
- Node invocation is best effort: incompatible Node versions fall back to a simulated trace with an `error` field instead of executing the function.
