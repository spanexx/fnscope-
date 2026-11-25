# `fnscope` — Architecture & Agent Build Specification

## 1. Project Overview & Core Philosophy
**`fnscope`** is a local-first developer inspection tool that pairs raw function source code with a structured, interactive **Explorer Preview** and **1-Hop Call Neighborhood Map**.

Unlike documentation generators that only display outer signatures (parameters and return types), `fnscope` zooms inside **what a function actually does internally** as an ordered pipeline of `executionSteps`, links upstream callers (`calledBy`) and downstream callees (`callsFnId`), and allows live interactive sandbox execution.

### Core Architectural Principles
1. **Deterministic AST First, AI Second:** Structural truth (signatures, parameter types, call graph edges, `if` guards, `throw` errors, `return` shapes, and raw statement steps) must be extracted deterministically via AST parsing (`ts-morph` or the TypeScript Compiler API). An LLM is **only** used as an optional, hash-cached enrichment pass to generate human-readable descriptions and sample sandbox defaults.
2. **Zero-Config CLI + Local Web UI:** Developers run one command against their source directory (`npx fnscope scan ./src` or `npx fnscope ./src`), which emits a static JSON manifest (`.fnscope/fn-graph.json`) and serves the React explorer UI on localhost.
3. **1-Hop Focused Graph (No Spaghetti):** Instead of rendering an unreadable whole-repo node graph, navigation is centered around one focused function at a time with bidirectional traversal:
   - **Upstream (`calledBy`):** Who calls this function, at what file/line, and with what call-site snippet.
   - **Downstream (`steps[].callsFnId`):** Which internal functions each execution step invokes, with one-click drill-down and a persistent `call-path` breadcrumb trail.
   - **1-Hop Neighborhood Map:** A 3-column visual view (`Upstream Callers` → `Focused Node` → `Downstream Calls`).

---

## 2. CLI Commands & Developer Workflow

Implement a Node/TypeScript CLI binary named `fnscope`:

### A. `fnscope scan <targetDir> [options]`
Scans `<targetDir>` (e.g., `./src`), extracts the function graph, optionally enriches descriptions via LLM, and writes the manifest to disk.

- **Flags:**
  - `--out <dir>`: Output directory (default: `.fnscope`).
  - `--enrich`: Enable optional LLM enrichment pass for plain-English descriptions, step explanations, and realistic sandbox default values.
  - `--model <name>`: Model identifier for `--enrich` (default: fast/low-cost model configured via `OPENAI_API_KEY` / `GEMINI_API_KEY` / `ANTHROPIC_API_KEY`).
  - `--include <glob>` / `--exclude <glob>`: Filter files (default excludes `node_modules`, `dist`, `*.test.ts`, `*.spec.ts`).

### B. `fnscope [targetDir]` (or `fnscope open`)
- If `<targetDir>` is provided, runs `scan` automatically and starts a file watcher (`chokidar` or `fs.watch`) so code edits regenerate `.fnscope/fn-graph.json` live.
- Serves the React UI (`FunctionExplorer.jsx`) on `http://localhost:4310` and exposes `/api/fn-graph.json` (plus optional `/api/invoke` endpoint to execute real project functions in a sandboxed worker).

---

## 3. Output Artifacts & Incremental Caching

All generated files live inside `.fnscope/` (which projects can add to `.gitignore` or commit for team visibility):

```text
.fnscope/
├── fn-graph.json        # Complete function graph & steps read directly by FunctionExplorer.jsx
└── enrich-cache.json    # SHA-256 astHash -> LLM enrichment cache (prevents redundant AI calls)
```

### Content-Addressable Caching (`astHash`)
1. During `scan`, compute a SHA-256 hash (`astHash`) of each function's normalized AST body + signature (ignoring whitespace/formatting changes).
2. When `--enrich` runs:
   - Look up `astHash` in `.fnscope/enrich-cache.json`.
   - **Cache Hit:** Reuse cached `description`, step `title`/`detail`, and `defaultValue` hints in `0ms` with zero API calls.
   - **Cache Miss:** Send *only* the compact AST skeleton of new/modified functions to the LLM, store the result under `astHash` in `enrich-cache.json`, and merge into `fn-graph.json`.

---

## 4. Deterministic AST Extraction Rules (`scan` Pass 1)

Use `ts-morph` (or `typescript` Compiler API) to parse `.ts`, `.tsx`, `.js`, and `.jsx` files:

### Step 1: Function Discovery
Extract exported and top-level/module-scoped functions:
- `FunctionDeclaration` (`export function foo(...)`)
- `VariableDeclaration` initialized with `ArrowFunction` or `FunctionExpression` (`export const foo = (...) => ...`)
- `MethodDeclaration` inside classes/services (format `id` as `ClassName.methodName`).

Assign each function a stable deterministic `id` (e.g., slugified `modulePath:functionName` or kebab-case name).

### Step 2: Signature, Purity & Capabilities
- **`parameters`:** Extract `name`, resolved TypeScript type string (`type`), `required` (`!param.hasQuestionToken() && !param.hasInitializer()`), and JSDoc `@param` description.
- **`returns`:** Extract resolved return type (`type`) and if it resolves to an object/interface (or `Promise<T>`), unpack top-level properties into `fields: [{ name, type }]`.
- **`purity`:**
  - Mark `'pure'` if the function body performs no I/O, mutates no external variables, accesses no `process.env` / network / DB / filesystem calls, and only calls other `'pure'` functions.
  - Otherwise mark `'side-effect'` and infer `permissions` (e.g., `net:outbound` for `fetch`/`axios`, `fs:read`/`fs:write` for `node:fs`, `secrets:read:<KEY>` for `process.env.<KEY>`).

### Step 3: Internal `executionSteps` Extraction
Walk the top-level statements inside the function's `Block` in execution order and classify each statement into one of four step kinds:

1. **`call`**: Statement invokes another function/method (`CallExpression` or `AwaitExpression` wrapping a `CallExpression`).
   - If the callee resolves to another function inside the scanned codebase, populate `callsFnId` with that target function's `id` so the UI renders a clickable `inspect <fn>()` link and wires the 1-Hop Map.
2. **`branch`**: `IfStatement` or `SwitchStatement` (especially guard clauses containing `ThrowStatement` or early `ReturnStatement`).
3. **`transform`**: `VariableStatement` or expression performing data mapping, filtering, regex, arithmetic, or destructuring without external calls.
4. **`return`**: `ReturnStatement` exiting the function.

For each step, extract deterministically:
- `id`: `step-${index + 1}`
- `kind`: `'call' | 'branch' | 'transform' | 'return'`
- `code`: Single-line normalized source snippet of the statement
- `inputs`: Array of identifier names referenced/read in the statement
- `output`: Assigned variable name + inferred type (for `VariableStatement`), `throws <ERROR_CODE> | continues` (for guard `IfStatement`), or return type (for `ReturnStatement`)
- `title`: Leading single-line comment (`// ...`) immediately above the statement if present; otherwise a deterministic fallback (e.g., `Call tokenize()`, `Guard: check !match`, `Return RoleEvaluationResult`).
- `detail`: Secondary comment line or deterministic summary (enriched by LLM if `--enrich` is enabled).

### Step 4: Error (`throws`) Extraction
Walk all `ThrowStatement` descendants inside the function body:
- Extract error code/class (e.g., `throw new FunctionError('TOKEN_NOT_FOUND')` → `code: 'TOKEN_NOT_FOUND'`, `status: 422`).
- Extract enclosing `if (...)` condition text as `when`.

### Step 5: Bidirectional Call Graph (`calledBy`)
After indexing all functions and their `steps[].callsFnId` references:
- Perform a reverse pass over all edges `callerFn -> calleeFn`.
- Populate `calleeFn.calledBy` with:
  - `fnId`: `callerFn.id` (or `null` if invoked from an HTTP route / event listener / CLI entrypoint)
  - `name`: `callerFn.name`
  - `module`: `${callerFn.relativeFilePath}:${callLineNumber}`
  - `callSite`: Exact trimmed line of code where the call occurs

---

## 5. `fn-graph.json` Schema Contract

The CLI must output `.fnscope/fn-graph.json` matching this exact structure so `FunctionExplorer.jsx` can render it directly:

```json
{
  "tool": "fnscope",
  "schemaVersion": "1.0.0",
  "generatedAt": "2026-11-25T21:30:00.000Z",
  "rootDir": "./src",
  "functions": [
    {
      "id": "evaluate-grammar-role",
      "name": "evaluateGrammarRole",
      "module": "nlp/roles",
      "filePath": "src/nlp/roles.ts",
      "line": 14,
      "astHash": "sha256:9f86d081884c7d659a2feaa0c55ad015...",
      "version": "1.0.0",
      "runtime": "node20 / edge",
      "purity": "pure",
      "timeoutMs": 150,
      "permissions": [],
      "description": "Tokenizes a sentence, finds the target word token (throwing if absent), infers its syntactic job, and returns the role payload.",
      "tags": ["nlp", "pure"],
      "calledBy": [
        {
          "fnId": "grade-sentence-exercise",
          "name": "gradeSentenceExercise",
          "module": "study/grading.ts:8",
          "callSite": "const expected = await evaluateGrammarRole({ sentence, targetWord })"
        }
      ],
      "parameters": [
        {
          "name": "sentence",
          "type": "string",
          "required": true,
          "defaultValue": "The quick fox jumped over the lazy dog.",
          "description": "Full English sentence providing syntactic context."
        }
      ],
      "steps": [
        {
          "id": "step-1",
          "kind": "call",
          "callsFnId": "tokenize-sentence",
          "title": "Tokenize input sentence",
          "code": "const tokens = tokenize(input.sentence)",
          "inputs": ["input.sentence"],
          "output": "tokens: Token[]",
          "detail": "Splits sentence into normalized word tokens with positional indices."
        }
      ],
      "returns": {
        "type": "RoleEvaluationResult",
        "fields": [
          { "name": "roleId", "type": "string" },
          { "name": "label", "type": "string" }
        ]
      },
      "errors": [
        {
          "code": "TOKEN_NOT_FOUND",
          "status": 422,
          "when": "!match"
        }
      ],
      "sourceCode": "export async function evaluateGrammarRole(...) { ... }"
    }
  ]
}
```

---

## 6. UI Component Contract (`FunctionExplorer.jsx`)

The accompanying `FunctionExplorer.jsx` component implements the complete `fnscope` frontend and accepts optional props so the host app can feed live `fn-graph.json` data and wire real sandbox execution:

```jsx
<FunctionExplorer
  graph={loadedFnGraphJson}          // Optional: { functions: [...] } loaded from .fnscope/fn-graph.json (falls back to built-in demo graph)
  initialFunctionId="evaluate-grammar-role"
  onInvoke={async (fn, args) => {   // Optional: server-side runner hook; falls back to fn.execute(args) if provided
    const res = await fetch('/api/invoke', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ fnId: fn.id, args })
    });
    return res.json();
  }}
/>
```

### UI Features Already Built in `FunctionExplorer.jsx`
1. **Four View Modes (`Raw` | `Split` | `Preview` | `1-Hop Map`):**
   - `Raw`: Displays `.ts` source code or `.json` manifest with one-click Copy.
   - `Split`: Side-by-side raw source (left) and structured Explorer Preview (right).
   - `Preview`: Full-width Explorer Preview tree.
   - `1-Hop Map`: 3-column neighborhood graph (`Upstream Callers` → `Focused Node` → `Downstream Calls`).
2. **Call-Path Breadcrumb Navigation:** Tracks traversal across callers (`Jump ↑`) and callees (`inspect fn()`) with instant back navigation.
3. **Ordered Explorer Tree:**
   - Metadata (`id`, `runtime`, `purity`, `description`, `version`, `capabilities`)
   - `calledBy` (upstream callers + call sites)
   - `parameters` (typed inputs arriving first)
   - `executionSteps` (ordered internal logic with `call` / `transform` / `branch` / `return` badges, code lines, variable reads/outputs, clickable downstream links, and step-by-step live execution trace)
   - `returns` (return type & field breakdown)
   - `throws` (error codes & trigger conditions)
   - `invokeSandbox` (auto-generated form from `parameters` + live trace & JSON output console)

---

## 7. Implementation Checklist for the Coding Agent

1. **Initialize Package Structure (`fnscope`):**
   - `package.json`: Configure `"name": "fnscope"` and `"bin": { "fnscope": "./dist/bin/cli.js" }`.
   - `bin/cli.ts`: CLI entry point (`scan` and `open` commands using `commander` or `cac`).
   - `src/scanner/ast-extractor.ts`: `ts-morph` AST walker producing the `functions` array and bidirectional `calledBy` links.
   - `src/scanner/enricher.ts`: Optional LLM enrichment pass gated by `astHash` lookups in `.fnscope/enrich-cache.json`.
   - `src/server/dev-server.ts`: Lightweight HTTP + WebSocket/SSE server serving the Vite/React app, `/api/fn-graph.json`, and optional `/api/invoke` sandbox execution.
   - `src/ui/FunctionExplorer.jsx`: Copy the provided `FunctionExplorer.jsx` directly into the UI bundle and pass the fetched `fn-graph.json` via the `graph` prop.
2. **Verify End-to-End Flow:**
   - Run `fnscope scan ./src` → verify `.fnscope/fn-graph.json` is generated cleanly without an API key.
   - Run `fnscope scan ./src --enrich` → verify `.fnscope/enrich-cache.json` caches hashes and skips unchanged functions on subsequent runs.
   - Launch UI via `fnscope ./src` → verify `Raw`, `Split`, `Preview`, and `1-Hop Map` views, breadcrumb navigation, and `invokeSandbox` work seamlessly.