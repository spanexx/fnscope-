# J13 — Real UI browser audit (evidence only)

Date: 2026-11-25
Auditor: J13 verification subagent
Target: `http://127.0.0.1:4312/` served by the **release** binary against `tests/fixtures/sample_app`

## Environment / setup

- Release binary present: `target/release/fnscope` (13,145,472 bytes).
- Native Node used: `/home/spanexx/.local/share/fnscope-node24/bin/node` via `FNSCOPE_NODE`.
- Command: `./target/release/fnscope open tests/fixtures/sample_app --host 127.0.0.1 --port 4312 --no-watch --invoke node`
- Readiness: separate bounded poll, healthy on attempt 1. Body: `{"ok":true,"tool":"fnscope"}`.
- Server log: `scanned 9 functions in 10 files` / `fnscope UI on http://127.0.0.1:4312` — no errors.
- Browser used: `browse` MCP server driving real Chrome (tab id `203395953`, group `fnscope-ui-audit`). The in-app `browser` tool was **disconnected** (`[browser.disconnected]`) for this whole run; the `browser` skill was loaded first as required.
- Cleanup: server killed by tracked PID `191186`, `GET /api/health` confirmed `PORT_DOWN`, `/tmp/fnscope-ui-audit-4312` removed, browser tab closed (`remaining.tabs: []`), scratch scripts removed. No `pkill -f` used. No cargo run, no application code edited.

Real manifest (from `GET /api/fn-graph.json`), 9 functions:
`clamp`, `dispatchWebhookEvent`, `evaluateGrammarRole`, `format`, `constructor`, `gradeSentenceExercise`, `inferSyntacticJob`, `slugify`, `tokenize`. `tool=fnscope schemaVersion=1.0.0`.

## Checks

### 1. Toolbar shows `fnscope` brand — PASS
Exact observed (`document.body.innerText` start):
```
fnscope
clamp()
dispatchWebhookEvent()
evaluateGrammarRole()
format()
constructor()
gradeSentenceExercise()
inferSyntacticJob()
slugify()
tokenize()
```
Page title: `fnscope`. A `<span>` with exact text `fnscope` exists in the toolbar.

### 2. Function buttons come from the REAL manifest, not the built-in sample — PASS
Observed toolbar buttons: `clamp()`, `dispatchWebhookEvent()`, `evaluateGrammarRole()`, `format()`, `constructor()`, `gradeSentenceExercise()`, `inferSyntacticJob()`, `slugify()`, `tokenize()`. This is exactly the 9 served manifest functions. Built-in sample-only names (`gradeSentenceExercise` + `evaluateGrammarRole` + `tokenize` + `inferSyntacticJob` + `dispatchWebhookEvent` with `nlp/roles` modules) are **not** what is shown; real modules are `src/nlp`, `src/study`, `src/legacy`, etc.
Left-pane real source proves it (sample JSX source says `const tokens = tokenize(input.sentence);`, served/observed source says):
```
async function evaluateGrammarRole(input: {
  sentence: string;
  targetWord: string;
  includeTeachingLayer?: boolean;
}): Promise<RoleEvaluationResult> {
  // 1. Tokenize input sentence
  const tokens: Token[] = tokenize(input.sentence);
  // 2. Reject a sentence that produced no word tokens.
  if (tokens.length === 0) {
    throw new FunctionError('EMPTY_SENTENCE', { sentence: input.sentence });
  }
```
Description observed: `Classifies a target word's grammatical job inside a sentence.` (real manifest; not the sample's longer text).

### 3. Four view buttons exist — PASS
Buttons observed: `Raw`, `Split`, `Preview`, `1-Hop Map`. Default view on load: `Split` (both `Function source` and `Explorer preview` panes rendered).

### 4. Split mode: real source left, structured steps right — PASS
Left heading `Function source`, right heading `Explorer preview`. Right pane showed 7 real steps in order:
1. `Tokenize input sentence` / `call` / `inspect tokenize()` / `→ tokens: Token[]` / `const tokens: Token[] = tokenize(input.sentence);`
2. `Reject a sentence that produced no word tokens.` / `branch` / `→ throws EMPTY_SENTENCE | continues`
3. `Normalize the target and locate its token in the stream.` / `call` / `→ normalizedTarget: unknown`
4. `Call find()` / `call` / `→ match: unknown`
5. `Fail fast when the target word is absent from the sentence.` / `branch` / `→ throws TOKEN_NOT_FOUND | continues`
6. `Infer the syntactic job from the surrounding tokens.` / `call` / `inspect inferSyntacticJob()`
7. `Return the role payload with intuition only when requested.` / `return` / `→ Promise<RoleEvaluationResult>`

### 5. `.json` raw toggle shows manifest with `schemaVersion` / `callsFnId` — FAIL (one of two)
Clicked `.json`. Observed code text (3,597 chars) starts:
```
{
  "id": "evaluate-grammar-role",
  "name": "evaluateGrammarRole",
  "module": "src/nlp",
  "version": "1.0.0",
  "purity": "pure",
  "calledBy": [
    {
      "fnId": "grade-sentence-exercise",
      "name": "gradeSentenceExercise",
      "module": "src/study/grading.ts:6",
```
- `callsFnId` — **PRESENT**. Parsed step keys: `["id","kind","title","code","inputs","output","callsFnId"]`.
- `schemaVersion` — **ABSENT** (`has_schemaVersion:false`). Parsed top-level keys: `["id","name","module","version","purity","calledBy","parameters","steps","returns","errors"]`. Graph-level `tool`, `schemaVersion`, `generatedAt`, `rootDir` are all missing. The `.json` view is only a reconstructed per-function object, not the served graph manifest.

### 6. Preview mode sections — PASS
Clicked `Preview`. All required section headers rendered (exact observed):
```
call-path:
gradeSentenceExercise()
→
evaluateGrammarRole()
Back
evaluateGrammarRole
1 callers
7 steps
id
evaluate-grammar-role
universal
purity
pure / deterministic
description
Classifies a target word's grammatical job inside a sentence.
version
1.0.0
calledBy
1
parameters
1
executionSteps
7
returns
Promise<RoleEvaluationResult>
0
throws
2
invokeSandbox
live runner
```
Clicking `throws 2` expanded to real codes: `EMPTY_SENTENCE (tokens.length === 0)` and `TOKEN_NOT_FOUND (!match)`.

### 7. 1-Hop Map — PASS
Clicked `1-Hop Map`. Exact observed text:
```
1-Hop Call Neighborhood: evaluateGrammarRole()
Click any upstream caller or downstream callee to traverse the codebase graph.
1 callers · 2 callees
UPSTREAM CALLERS (1)
gradeSentenceExercise
src/study/grading.ts:6
const expected = await evaluateGrammarRole({ sentence: input.sentence, targetWord: input.targetWord, includeTeachingLayer: true });
FOCUSED NODE
src/nlp
evaluateGrammarRole()
Classifies a target word's grammatical job inside a sentence.
7 execution steps
Inspect steps →
DOWNSTREAM CALLS (2)
tokenize()
src/nlp
Executes 3 ordered steps and returns Token[].
inferSyntacticJob()
src/nlp
Executes 6 ordered steps and returns SyntacticJob.
```
Real upstream/downstream names and real call site. 

### 8. Breadcrumb navigation changes focus and "Back" returns — PARTIAL (one path FAIL)
- `inspect tokenize()` — **PASS**: breadcrumb changed to `call-path:\nevaluateGrammarRole()\n→\ntokenize()`, left pane id became `tokenize`, run button `Run tokenize()`. Clicking `Back` returned to `evaluate-grammar-role` (breadcrumb `call-path:\nevaluateGrammarRole()`, left id `evaluate-grammar-role`, run button `Run evaluateGrammarRole()`).
- `Jump ↑` — **FAIL**: clicking it changed focus to `grade-sentence-exercise` (left manifest id `grade-sentence-exercise`, run button `Run gradeSentenceExercise()`, breadcrumb collapsed to a single `gradeSentenceExercise()`), but the `Back` button **disappeared** (`backButtons: 0`), so there is no way to go back via breadcrumb. Cause is visible in `fnscope_ui.tsx`: `navigateToFunction` truncates the trail with `prev.slice(0, existingIdx + 1)` when the target is already in it, making `navTrail.length === 1`, and `Back` is only rendered when `navTrail.length > 1`.

### 9. Invoke sandbox: Run → JSON console result, not `simulated: true` — PASS (with real bugs)
Network capture of the UI's own `Run` calls:

- **Default focused function `evaluateGrammarRole()`** — `POST /api/invoke` body was:
  `{"fnId":"evaluate-grammar-role","args":{"input":""}}`
  Response body (HTTP 200):
  ```
  {"ok":false,"data":null,"error":"node '/home/spanexx/.local/share/fnscope-node24/bin/node': Cannot read properties of undefined (reading 'replace')","trace":{},"durationMs":696.888583}
  ```
  UI console rendered `422 THROWN` and the exact error text above. **Not** `simulated: true` — Node was genuinely invoked (error names the native Node 24 path). But the default user action fails.
- **`clamp()` with filled args** — UI sent `{"fnId":"clamp","args":{"value":"15","min":"0","max":"10"}}`; response:
  `{"ok":true,"data":10,"error":null,"trace":{},"durationMs":133.87196400000002}`
  UI console rendered `200 OK` and `10`. This is a real Node result, no `simulated` field.
- `tokenize()` with `sentence` filled → `{"ok":false,...,"error":"node '.../node': sentence.replace is not a function"}`.
- `slugify()` with `{value:'Hello World'}` → `error: "node '.../node': value.toLowerCase is not a function"`.
- Direct API with **flat** args for `evaluate-grammar-role` (`{"sentence":"...","targetWord":"fox","includeTeachingLayer":true}`) → `{"ok":true,"data":{"confidence":0.94,"intuition":"\"fox\" anchors the action in this clause.","label":"Doer (subject)","roleId":"doer"},"error":null,"trace":{},"durationMs":707.25}` — proof Node mode is real and works when args match.
- No console JS errors were captured (`entries: []`).

Note: before any Run, the console shows the local fallback `{"simulated": true, "function": "...", "inputs": {...}, "expectedReturnType": "..."}`. That is the initial `defaultRunPreview` state, not an invoke response. With `--invoke node`, no post-Run response contained `simulated: true`.

## Screenshots

Saved through the `browse` tool (its file root is only `C:\Users\shuga\AppData\Local\Temp`; it refused a path inside the repo, so the tool-returned paths are reported as permitted):

- Split mode: `\\?\C:\Users\shuga\AppData\Local\Temp\fnscope-ui-audit\ui-split.png` (120,939 bytes, 1905×911)
- 1-Hop Map: `\\?\C:\Users\shuga\AppData\Local\Temp\fnscope-ui-audit\ui-map.png` (50,579 bytes, 1920×911)

## Bugs found (top 3)

1. **`.json` manifest view omits graph metadata.** It renders a per-function object with no `schemaVersion`, `tool`, `generatedAt`, or `rootDir`, so the explicit "manifest with schemaVersion" expectation fails. Suggested owner: UI (`ui/` app.js build input / `fnscope_ui.tsx` `rawText` branch) — needs graph-level data passed into the component, not reconstructed from `activeFn`.
2. **`Jump ↑` destroys the trail and removes `Back`.** After Jump ↑ the breadcrumb is a single node and `Back` is gone; the user cannot return to the function they came from. The `inspect ...()` path works. Suggested fix: keep the full trail and use Back for both navigation types (or never truncate below 2 when a caller jump occurs).
3. **Invoke sandbox arg contract mismatch.** The UI derives sandbox fields from manifest parameters, so `evaluateGrammarRole`'s single `input` object parameter becomes `{"input":""}` while the real Node export expects flat `{sentence, targetWord, includeTeachingLayer}` — the default Run always fails with `Cannot read properties of undefined (reading 'replace')`. Separately, the Node runner passes the whole `args` object as the single first argument, so scalar-string functions (`tokenize`, `slugify`) always fail with `sentence.replace is not a function` / `value.toLowerCase is not a function` no matter what is typed. `clamp` and `evaluate-grammar-role` succeed with flat args, so the server itself is not stuck in simulate mode.

Additional minor observation: `returns` shows `Promise<RoleEvaluationResult>` with `0` fields for `evaluate-grammar-role` (real manifest has an empty `fields` array) — not a UI bug, just reporting it.

## Verdict

PASS 7, FAIL 1, PARTIAL 1 (of 9 checks). Check 8 is PARTIAL because its `inspect tokenize()` → Back path passes while its `Jump ↑` → Back path fails. No simulate-mode fallback observed after Run; native Node 24 execution confirmed by real results (`clamp → 10`) and by Node-path-bearing errors.

## Re-audit 2026-11-25

Targeted verification of the J08 (UI) and J06 (server) fixes on a **fresh release binary** (`target/release/fnscope`, size 13,174,104, mtime 23:43) on port **4314**. Original findings above are left unchanged.

Setup: `FNSCOPE_NODE=/home/spanexx/.local/share/fnscope-node24/bin/node`, `--invoke node`, `--no-watch`, `tests/fixtures/sample_app`. Server detached with tracked PID `197956`; readiness healthy on attempt 1 with `{"ok":true,"tool":"fnscope"}`; log `scanned 9 functions in 10 files` / `fnscope UI on http://127.0.0.1:4314` with no errors. Browser: connected `browse` MCP real Chrome, tab `203395957`, group `fnscope-reaudit` (in-app `browser` tool not used). Network capture of `POST /api/invoke` was started **before** any Run, with bodies enabled. Cleanup: tab closed (`remaining.tabs: []`), server killed by tracked PID, `PORT_DOWN`, `/tmp/fnscope-reaudit-4314` removed, scratch scripts removed. No `pkill -f`, no cargo, no application code edited.

### R1 — `.json` view includes real graph metadata (`tool`, `schemaVersion`, `generatedAt`, `rootDir`) and still has `callsFnId` — PASS
Clicked `.json`. Exact observed text (first 4072 chars, parsed with `JSON.parse`, no error):
```
{
  "tool": "fnscope",
  "schemaVersion": "1.0.0",
  "generatedAt": "2026-11-25T22:44:24.146179633+00:00",
  "rootDir": "tests/fixtures/sample_app",
  "id": "evaluate-grammar-role",
  "name": "evaluateGrammarRole",
  "module": "src/nlp",
  "version": "1.0.0",
  "purity": "pure",
  "calledBy": [
    {
      "fnId": "grade-sentence-exercise",
      "name": "gradeSentenceExercise",
      "module": "src/study/grading.ts:6",
```
Top-level keys observed: `["tool","schemaVersion","generatedAt","rootDir","id","name","module","version","purity","calledBy","parameters","steps","returns","errors"]`. `has_callsFnId: true`. The original FAIL is fixed; metadata values are from the real served graph.

### R2 — `Jump ↑` appends to breadcrumb and `Back` returns to the previous function — PASS
On `evaluateGrammarRole()` (default breadcrumb `gradeSentenceExercise() → evaluateGrammarRole()`), clicked `Jump ↑`. Exact observed after jump:
```
call-path:
gradeSentenceExercise()
→
evaluateGrammarRole()
→
gradeSentenceExercise()
Back
```
`backCount: 1`, left manifest id `grade-sentence-exercise`, run button `Run gradeSentenceExercise()` — focus changed AND Back was available. Clicked `Back`; exact observed after:
```
call-path:
gradeSentenceExercise()
→
evaluateGrammarRole()
→
gradeSentenceExercise()
→
evaluateGrammarRole()
```
left manifest id `evaluate-grammar-role`, run button `Run evaluateGrammarRole()` — returned to the function jumped from. The original FAIL is fixed.

### R3a — `evaluateGrammarRole()` with DEFAULT args — PASS
Clicked `Run evaluateGrammarRole()` without touching inputs. Captured POST body:
```
{"fnId":"evaluate-grammar-role","args":{"input":{"sentence":"The quick fox jumped over the lazy dog.","targetWord":"fox","includeTeachingLayer":true}}}
```
Response (HTTP 200):
```
{"ok":true,"data":{"confidence":0.94,"intuition":"\"fox\" anchors the action in this clause.","label":"Doer (subject)","roleId":"doer"},"error":null,"trace":{},"durationMs":407.33487}
```
Console observed: `200 OK`, `419 ms`, and the pretty-printed object with `"roleId": "doer"`, `"label": "Doer (subject)"`, `"confidence": 0.94`, `"intuition": "\"fox\" anchors the action in this clause."`. No `simulated: true`; the `{"input":""}` default from the original audit is repaired to a real nested object.

### R3b — `tokenize()` with a sentence filled — PASS
Filled the sandbox textbox with `The quick fox jumped over the lazy dog.` and clicked `Run tokenize()`. Captured POST body:
```
{"fnId":"tokenize","args":{"sentence":"The quick fox jumped over the lazy dog."}}
```
Response (HTTP 200):
```
{"ok":true,"data":[{"index":0,"normalized":"the","raw":"The"},{"index":1,"normalized":"quick","raw":"quick"},{"index":2,"normalized":"fox","raw":"fox"},{"index":3,"normalized":"jumped","raw":"jumped"},{"index":4,"normalized":"over","raw":"over"},{"index":5,"normalized":"the","raw":"the"},{"index":6,"normalized":"lazy","raw":"lazy"},{"index":7,"normalized":"dog","raw":"dog"}],"error":null,"trace":{},"durationMs":389.842867}
```
Console showed `200 OK`, `395.8 ms`, and the 8-token JSON array. The original `sentence.replace is not a function` error is gone — parameter is now passed by name to a named binding.

### R3c — `slugify()` with a value filled — PASS
Filled `Hello World` and clicked `Run slugify()`. Captured POST body:
```
{"fnId":"slugify","args":{"value":"Hello World"}}
```
Response (HTTP 200):
```
{"ok":true,"data":"hello-world","error":null,"trace":{},"durationMs":267.326391}
```
Console showed `200 OK`, `276.6 ms`, and `"hello-world"`. The original `value.toLowerCase is not a function` error is gone.

### R3d — `clamp()` still works — PASS
Filled `value=15`, `min=0`, `max=10` and clicked `Run clamp()`. Button showed `Running...` during the call. Captured POST body:
```
{"fnId":"clamp","args":{"value":"15","min":"0","max":"10"}}
```
Response (HTTP 200):
```
{"ok":true,"data":10,"error":null,"trace":{},"durationMs":121.284895}
```
Console showed `200 OK`, `130 ms`, and `10`.

No browser console errors were captured (`entries: []`). No `simulated: true` appeared in any post-Run response.

### Re-audit verdict
PASS 6, FAIL 0 (R1, R2, R3a, R3b, R3c, R3d). All three reported bugs are fixed.

### Remaining minor observations (not FAILs, no new bug requiring delegation)
- After `Jump ↑` then `Back`, the breadcrumb accumulates duplicate nodes (`grade → evaluate → grade → evaluate`). Navigation still lands on the correct function; it is cosmetic.
- Node-mode responses carry `"trace":{}` (empty). Step trace badges in the executionSteps list therefore do not refresh from a real run. The contract only requires a trace object and the smoke check accepts `{}`, so this is not a contract breach — but it is a UX gap.
- The console shows the local `defaultRunPreview` payload with `"simulated": true` before the first Run. This is initial placeholder state, not an invoke response; no post-Run response contained `simulated: true`.

