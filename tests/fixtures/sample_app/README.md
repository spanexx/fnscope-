# fnscope sample app

A small, coherent TypeScript/JavaScript application used as fnscope's end-to-end fixture.
It is the evidence that a real scan produces a meaningful, bidirectional function graph and
that the Node invoke runner can call the exported functions directly.

Scan it with:

```text
fnscope scan ./tests/fixtures/sample_app/src
```

The module that holds each function is intentionally the module named in the built-in sample
graph in `fnscope_ui.tsx`, so the fixture doubles as executable documentation for that UI.

## Node ESM / type-stripping strategy

This fixture uses **explicit `.ts` extensions in every relative import** (for example
`import { tokenize } from './tokenizer.ts'`). Node 24 strips TypeScript types by default and
resolves `.ts` specifiers as written, so no build step is needed before invoking a function.

Rules that keep the value-level code importable by Node:

- Type-only imports use `import type { ... }` so the specifier is removed before execution.
- No `enum`, decorators, parameter properties, or runtime-only TypeScript constructs.
- `interface` and `type` declarations live in `src/types.ts` and are erased at load time.
- Only value imports remain at runtime (`FunctionError`, `tokenize`, `inferSyntacticJob`,
  `evaluateGrammarRole`, `dispatchWebhookEvent`, `createHmac`).
- `package.json` declares `"type": "module"`, so the plain-JS fixture in
  `src/legacy/helpers.js` is loadable as ESM by invoke tests too.

In short: **explicit `.ts` imports + `import type` for types**. That is the choice this fixture
documents; do not switch to extensionless imports without updating the runner.

## Expected scanner output

A default scan should yield **at least 8 functions**:

| Function | Expected id | Module | Purity | Key evidence |
| --- | --- | --- | --- | --- |
| `gradeSentenceExercise` | `grade-sentence-exercise` | `study/grading` | side-effect | calls `evaluateGrammarRole` and `dispatchWebhookEvent` |
| `evaluateGrammarRole` | `evaluate-grammar-role` | `nlp/roles` | pure | two guards, two downstream calls, JSDoc `@tags nlp, pure` |
| `tokenize` | `tokenize-sentence` | `nlp/tokenizer` | pure | regex strip, whitespace split, indexed tokens |
| `inferSyntacticJob` | `infer-syntactic-job` | `nlp/classifier` | pure | morphology + position branches |
| `dispatchWebhookEvent` | `dispatch-webhook-event` | `events/delivery` | side-effect | `net:outbound`, `secrets:read:WEBHOOK_SIGNING_SECRET` |
| `slugify` | `slugify` | `utils/strings` | pure | arrow-function discovery |
| `Formatter.format` | `formatter-format` | `utils/strings` | pure | class-method discovery |
| `clamp` | `clamp` | `legacy/helpers` | pure | plain `.js` discovery |

Exact ids come from `make_id(relative_path, qualified_name)`; the table shows the stable
slugs used by the built-in UI sample. What matters for assertions is that ids are unique and
that the edges below resolve to them.

### `evaluate-grammar-role` is the hub

- `calledBy` contains one upstream entry from **`grade-sentence-exercise`**, with a call site
  of `const expected = await evaluateGrammarRole({ ... })`, proving the reverse call-graph
  pass works.
- Its `steps` include two `call` steps whose `callsFnId` resolve to **`tokenize-sentence`**
  and **`infer-syntactic-job`**, proving the 1-hop downstream links.
- Leading `// 1. Tokenize input sentence` style comments become step `title` values.
- Its `errors` include:
  - `TOKEN_NOT_FOUND`, status **422**, thrown when `!match` (target word not in sentence).
  - `EMPTY_SENTENCE`, status **400**, thrown when `tokens.length === 0`.
- Its JSDoc supplies parameter descriptions, a `@returns` description, and the tags
  `nlp` and `pure`.

### `dispatch-webhook-event` is a side effect

- `purity` is `side-effect`.
- `permissions` include `net:outbound` (the `fetch(...)` call) and
  `secrets:read:WEBHOOK_SIGNING_SECRET` (the `process.env.WEBHOOK_SIGNING_SECRET` read).
- It throws `INSECURE_PROTOCOL` when `endpointUrl` does not start with `https://`, and it
  short-circuits on `dryRun` before any network call. Invoke tests should pass `dryRun: true`.

### File filtering

- `src/roles.test.ts` matches the default `**/*.test.ts` exclude and **must not appear** in
  the graph. If its `testEvaluateGrammarRole` function shows up, the default excludes broke.
- `src/ignored.generated.ts` is **included by default** — `*.generated.ts` is not part of the
  default exclude list. It exports only a constant, so it does not add a function to the
  count. A scan with `--exclude '**/*.generated.ts'` must drop it; this makes it a useful
  fixture for include/exclude flag tests.

## Runtime notes for invoke tests

- Import a function by its file URL and call the named export, for example
  `const { tokenize } = await import('<absolute-file-url>/nlp/tokenizer.ts')`.
- `evaluateGrammarRole({ sentence, targetWord })` is fully pure and needs no environment.
- `gradeSentenceExercise(...)` calls a webhook with `dryRun: true`, so it performs no network
  I/O, but it does read `WEBHOOK_SIGNING_SECRET` (missing values default to the empty string).
- `dispatchWebhookEvent({ endpointUrl, eventType, dryRun: true })` is the safe way to invoke
  the side-effect function. Passing an `http://` URL demonstrates `INSECURE_PROTOCOL`.
- `node --check` applies to `src/legacy/helpers.js` only; `.ts` files rely on Node's type
  stripper and should be checked by importing them under Node 24.
