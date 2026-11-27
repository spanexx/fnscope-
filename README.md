# fnscope

## What it does

fnscope reads 13 named languages — TypeScript, TSX, JavaScript, JSX, Rust, Python, Go, Java, C, C++, C#, Kotlin, Ruby, PHP and Swift — across 16 file suffixes, and builds a deterministic function graph: signatures, ordered execution steps, upstream callers and downstream calls. It then serves a local web explorer with four views (Story, 1-Hop, Split and Raw), isolated project scopes and a sandbox invoke panel. An optional, hash-cached LLM pass can fill in plain-English descriptions and sample arguments. There is no account, no cloud service and no telemetry: it is a single local binary. The binary is built in Rust and the React UI is bundled into it, so there is no separate frontend to install or run.

## Quick start

From the repository root in WSL Debian:

```bash
cargo build --release -p fnscope
./target/release/fnscope scan ./tests/fixtures/sample_app/src
./target/release/fnscope ./tests/fixtures/sample_app/src
./target/release/fnscope ./tests/fixtures/rust_app/src
```

Then open <http://localhost:4310>. Press Ctrl-C to stop the server. The second and third commands are the bare-target form of `open`: each scans once, serves the UI and watches for file changes. All 16 suffixes are scanned by default — `.ts`, `.tsx`, `.js`, `.jsx`, `.rs`, `.py`, `.go`, `.java`, `.c`, `.cpp`, `.cc`, `.cs`, `.kt`, `.php`, `.rb` and `.swift` — so the same command works on a Rust tree such as `crates/` or a mixed-language project.

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

Defaults: includes `**/*.ts`, `**/*.tsx`, `**/*.js`, `**/*.jsx`, `**/*.rs`, `**/*.py`, `**/*.go`, `**/*.java`, `**/*.c`, `**/*.cpp`, `**/*.cc`, `**/*.cs`, `**/*.kt`, `**/*.php`, `**/*.rb`, `**/*.swift`; excludes `node_modules`, `dist`, `**/target/**`, `**/.fnscope/**`, `**/__pycache__/**`, test/spec patterns and dot-directories.

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
| `--workspace <FILE>` | Serve the scopes listed in a `fnscope.workspace.json` |
| `--scope <ID=PATH>` | Add or override one scope; repeatable |
| `--default-scope <ID>` | Pick the scope used by the compatibility routes |
| `--enrich` | Enrich the graph using an LLM plus the local cache |
| `--model <MODEL>` | Model name for enrichment |

`fnscope <TARGET>` is shorthand for `fnscope open <TARGET>`, and the `open` flags also work at the top level, so `fnscope ./src --port 4311` and `fnscope open ./src --port 4311` are the same.

### Workspace scopes

One server can serve several independent project graphs without mixing their call edges or invoke
results:

```bash
./target/release/fnscope open --workspace tests/fixtures/workspace/fnscope.workspace.json
```

The config is explicit and relative to its own directory:

```json
{
  "version": 1,
  "defaultScope": "alpha",
  "scopes": [
    { "id": "alpha", "label": "Alpha", "path": "alpha", "include": ["**/*.ts"], "invoke": "simulate" },
    { "id": "beta", "label": "Beta", "path": "beta", "include": ["**/*.ts"], "invoke": "simulate" }
  ]
}
```

Absolute paths and `..` traversal are rejected. With no workspace config, `open` keeps the original
single-scope behaviour and serves the implicit default scope on the compatibility routes. The
workspace routes are additive: `GET /api/scopes`, `GET /api/scopes/<id>/fn-graph.json` and
`POST /api/scopes/<id>/invoke`; `/api/fn-graph.json` and `/api/invoke` keep serving the default
scope. See the additive section in `docs/contract/rust-api.md`.

## Languages

Every suffix is claimed by one adapter in the built-in registry. Adapter modules live under `crates/fnscope-parser/src/`; adding a language means implementing `LanguageAdapter` and registering it, with no change to core's graph builder.

| Suffixes | Named language | Adapter module |
| --- | --- | --- |
| `.ts`, `.tsx` | TypeScript / TSX | `typescript` (one `TypeScriptAdapter` instance per grammar) |
| `.js`, `.jsx` | JavaScript / JSX | `typescript` (one `TypeScriptAdapter` instance per grammar) |
| `.rs` | Rust | `rust` |
| `.py` | Python | `python` |
| `.go` | Go | `golang` |
| `.java` | Java | `java` |
| `.c` | C | `c_lang` |
| `.cpp`, `.cc` | C++ | `cpp` |
| `.cs` | C# | `csharp` |
| `.kt` | Kotlin | `kotlin` |
| `.php` | PHP | `php` |
| `.rb` | Ruby | `ruby` |
| `.swift` | Swift | `swift` |

Kotlin is parsed with `tree-sitter-kotlin-ng`; the older `tree-sitter-kotlin` pins a tree-sitter version that conflicts with the workspace runtime. `scan_with_registry` lets a caller inject a private registry instead of using the built-in one.

## Artifacts

Everything fnscope writes lands in `.fnscope/` (default; change with `--out`). You can gitignore it or commit it for teammates.

- `fn-graph.json` — the full function graph: functions, parameters, steps, return shapes, errors, call edges and source code. This is what the UI reads.
- `scopes/<id>/fn-graph.json` — one manifest per configured workspace scope, written only in workspace mode.
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
- `node` — writes a temporary runner, imports the target module and calls the real named export (class methods as `Class.method`), then returns JSON with `ok`, `data`, `error` and `trace`. This runner executes TypeScript/JavaScript exports only; it is not an executor for the other languages.

Real execution of TypeScript needs Node 24, which strips TypeScript types by default and resolves `.ts` specifiers as written. `FNSCOPE_NODE` selects which Node executable to use; if unset, fnscope uses `node` from PATH. Be warned: Node invocation is best effort. If the chosen Node is missing or incompatible (for example, an older Node that cannot import `.ts` files), fnscope falls back to the simulated trace and adds an `error` field explaining why. It does not fail the request.

## Verification

Run these in WSL. Neither script should be run while agents are still writing crate files.

- `bash scripts/test-all.sh` — the single gate. It checks formatting, runs the workspace tests, builds the release binary, scans the sample fixture and smoke-tests the HTTP server as separate steps. Prints `ALL_TESTS_OK` only when all five stages pass.
- `bash scripts/enrich-e2e.sh` — exercises the real CLI, a local mock LLM and the on-disk cache together. It proves that the first scan is all cache misses with one API call per function, an unchanged second scan is all cache hits with zero API calls, a formatting-only edit makes no new call, and a semantic edit does. Prints `ENRICH_E2E_OK` on success.
- `bash docs/features/workspace-ui/simulate.sh` — starts the real two-scope fixture, asserts scope isolation, call edges, the compatibility routes and scoped invoke, then runs the served-manifest contract check. Prints `WORKSPACE_SIM_OK`.

## Architecture map

- `fnscope-model` — the JSON schema types shared by every crate, including the graph, cache and invoke shapes.
- `fnscope-parser` — tree-sitter adapters for all 13 languages, each owning its grammar and the extraction of functions, steps, errors, types and purity behind `LanguageAdapter`.
- `fnscope-core` — the scanner: globs files, looks up one adapter per suffix, builds the graph, computes `astHash`, writes and reads the manifest.
- `fnscope-enrich` — hash-cached LLM enrichment, cache read/write and the no-op offline fallback.
- `fnscope-server` — the axum HTTP routes, the workspace scope registry, embedded UI assets and `/api/invoke` in simulate or node mode.
- `fnscope-cli` — the `fnscope` binary: clap parsing, the `scan` / `open` commands and workspace configuration.
- `ui/` — the React workspace UI (Story, 1-Hop, Split and Raw views, scope switcher, explorer and invoke drawer); built into `ui/dist` and embedded into the binary at compile time.

## Limits

- **Only TS/JS are executed.** `--invoke node` is a TypeScript/JavaScript runner: it imports a `.ts`/`.tsx`/`.js`/`.jsx` module through Node. Rust, Python, Go, Java, C, C++, C#, Kotlin, Ruby, PHP and Swift functions are inspected and navigated like any other function, but invoking one returns the deterministic simulated trace — there is no executor for those languages.
- **Kotlin uses `tree-sitter-kotlin-ng`.** The older `tree-sitter-kotlin` pins an incompatible tree-sitter version; `-ng` is the grammar actually in use.
- **Runtime labels are carried data, not executors.** `universal` and `node20` describe the target environment recorded in the graph for every language; they do not promise that Node can run that language.
- **Cross-file type resolution is best effort.** Types declared inside the scanned set are unpacked into return fields; imported or external types fall back to their written names.
- **Class constructors are listed as functions** alongside regular methods.
- **Node invocation is best effort.** An incompatible Node version falls back to a simulated trace with an `error` field instead of executing the function.
