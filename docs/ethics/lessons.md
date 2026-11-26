# fnscope — Lessons from real failures

These are not hypotheticals. Each lesson names a real build iteration, the
command and output that exposed the failure, the rule that came out of it, and
the workflow that enforces the rule. Two evidence files are cited throughout:

- `.reports/goal-build-fnscope.md` — the orchestrator ledger, cited as `iteration N`.
- `.reports/ui-browser.md` — the real browser audit and re-audit.

A third section at the bottom lists the details in the brief that I could not
find in those files. They are marked, not quoted as if they were evidence.

## Index

| # | Incident | Rule | Enforced in |
|---|---|---|---|
| L1 | Iteration 3 — 7 Rust compiler errors | Bounded write windows; a compile window follows every batch; agents never run cargo while peers write | `docs/workflows/compile-window.md`, `docs/ethics/boundary-discipline.md` |
| L2 | Iterations 4, 5, 11 — three bad assertions and one missing field | Triage CODE vs TEST against the locked contract before editing | `docs/workflows/test-failure-triage.md` |
| L3 | Iterations 6–8 — WSL/Windows Node path translation | Shared reachable paths, argv not embedded strings, native runtime preferred | `docs/workflows/cross-environment-runner.md` |
| L4 | Iteration 10 — sandbox arg contract mismatch | Audit the payload at every boundary with captured requests | `docs/workflows/boundary-contract-audit.md` |
| L5 | Iteration 10 — breadcrumb and `.json` view | Rendered behavior is not proven by source inspection | `docs/workflows/boundary-contract-audit.md` |
| L6 | Iteration 13 — D-2 status row `Open`, note `Resolved` | Machine-readable status fields are the gate | `docs/ethics/verification-honesty.md`, `docs/drift.md` |
| L7 | Iteration 12 — concern raised after the fix already landed | Verify against current code before escalating; cite the line | `docs/ethics/boundary-discipline.md` |
| L8 | Iterations 13–14 — empty `trace` in Node-mode responses | A response field the UI renders is part of the API contract | `docs/workflows/boundary-contract-audit.md` |
| L9 | Iteration 16 — polyglot expansion | A new language is a plugin, not a branch in the graph builder | `docs/concepts/language-adapter.md` |

## L1 — Seven compiler errors from parallel writes with no compile window

**Incident (iteration 3).** All eleven jobs were integrated and the first
compile was run. Exact ledger verification line:

> `cargo check -p fnscope-model -p fnscope-parser -p fnscope-core -p fnscope-enrich -p fnscope-server` -> `CARGO_CHECK_STATUS=101`; 7 errors, all in `fnscope-parser` (1 E0308 kind-vs-node in discover.rs, cascading E0599/E0382-style inference error, 4 E0597 iterator-temporary lifetimes, 1 E0382 partial move in body.rs).

The ledger names the failure class: "Rust borrow/type errors from delegated
code written without compiler feedback (by design, to avoid parallel cargo
contention)." The pivot was to send the exact diagnostics back to J02 and J03
and keep all gate execution with the orchestrator.

**Rule.** Parallel write windows are bounded. A compile window follows every
batch. Agents may run a read-only `rustfmt --check` on their own files; agents
never run `cargo` while peers are writing the workspace.

**Checkable procedure.** The orchestrator closes the write window, runs
`cargo fmt --all`, then `cargo fmt --all -- --check`, then the smallest compile
scope first and the workspace suite last. It pastes the exact diagnostics into
the owning agent's next brief. It does not fix another agent's file itself.
Full steps: `docs/workflows/compile-window.md`.

## L2 — Three wrong expectations and one missing test field

**Incident (iterations 4, 5, 11).** Three distinct cases where the test, not
the production code, was wrong:

- Iteration 4: `cargo test --workspace` -> "CLI 6/6, core unit 10/10, core integration 5/5, enrich 9/9 pass; model 9/10 — `validate_aggregates_every_problem` expected `steps[0].id` but only faulted indices are reported." Ledger failure class: "test expectation disagrees with correct production behavior." Pivot: "sent exact assertion + real output back to J01; no production change requested." Iteration 4 also records "isolated a wrong test assertion in core, corrected by J04".
- Iteration 5: "parser 16/17. `discover::tests::deterministic_order_and_duplicate_suppression` expected `[\"a\",\"b\"]` but got `[\"b\",\"a\",\"b\"]`." Ledger failure class: "test expectation contradicts the locked dedup key `(qualified_name, line)`."
- Iteration 11: `cargo test --workspace` -> 0, 72 tests, but "`scripts/test-all.sh` -> 101; new test failed with `Directory import ... fnscope-node-scalar-.../ is not supported`". Ledger failure class: "test's FnRecord omitted `file_path`, so the runner targeted a directory." Pivot: "J06 to set `file_path: \"shout.mjs\"` in the new test."

**Rule.** On a failing test, decide whether the CODE or the TEST is wrong by
comparing the assertion to the locked contract before editing anything. Neve
bend production behavior to satisfy a bad assertion.

**Checkable procedure.** Quote the failing line, quote the contract clause,
write down which side is wrong and why, then change only that side. If the
assertion was wrong, fix the test and add the reason to the job report. Re-run
the smallest check, then the full suite once. Full decision tree:
`docs/workflows/test-failure-triage.md`.

## L3 — WSL to Windows Node path translation

**Incident (iterations 6–8).** Iteration 6: "Server 7/8 pass; real-invoke test
returns `data: null`." Exact verification: "`node_invocation_runs_a_real_export`
-> `left: None, right: Some(42)`". Ledger failure class: "cross-environment
path translation. WSL translates Linux paths in argv for Windows executables,
but the generated runner embedded `file:///tmp/...` inside a file, which
Windows Node cannot resolve." Pivot: "J06 to pass the module path as argv[3]
instead of embedding it".

Iteration 7 added a diagnostic assertion and captured the real error:

> `node '/mnt/c/Program Files/nodejs/node.exe' exited with exit status: 1 ... Cannot find module 'C:\tmp\fnscope-invoke-...\runner.mjs'`

Ledger failure class: "WSL path translation maps `/tmp/...` to `C:\tmp\...`,
where Rust's WSL-created scratch files do not exist." Pivot: "J06 to create the
scratch dir beside the manifest (a `/mnt/c/...` path under WSL) and to place
the test module under the repo's `target/` dir." Iteration 8 resolved it by
running Node natively in WSL: `CARGO_TEST_STATUS=0`, 66 tests pass including
real Node invocation.

**Rule.** A process launched from one OS into another must use a path both
sides can reach. Pass paths as argv, never as a string embedded in a generated
file. Prefer a native runtime in the lane where the files live. Diagnose from
the real error text, not from a guessed cause.

**Checkable procedure.** Before launching, name the OS that creates the file,
the OS that reads it, and the exact path each will see. Confirm the path exists
from the reader side. Capture stderr. Full checklist:
`docs/workflows/cross-environment-runner.md`.

## L4 — The sandbox arg contract broke across five boundaries

**Incident (iteration 10).** The browser audit found the default invoke POST
body `{"fnId":"evaluate-grammar-role","args":{"input":""}}` and the response
`{"ok":false,"data":null,"error":"node '.../node': Cannot read properties of undefined (reading 'replace')",...}`. The audit also recorded
`sentence.replace is not a function` for `tokenize` and
`value.toLowerCase is not a function` for `slugify`, and noted that a direct
call with flat args succeeded with `"roleId":"doer"`. Ledger failure class:
"interface contract mismatch between manifest parameters, UI form and Node
runner argv." Critically, the same iteration records
"`cargo test --workspace` still 0 with 71+ tests" — the unit suite was green
while the real UI failed.

**Rule.** Audit the data contract at every boundary in the chain — manifest,
form, POST, runner, export — with captured requests. Unit tests alone are not
proof.

**Checkable procedure.** List the boundaries, capture one real payload at
each, assert the shape on both sides, add an adapter unit test, and add one
real end-to-end check with a browser or HTTP capture. Full checklist:
`docs/workflows/boundary-contract-audit.md`.

## L5 — Breadcrumb and `.json` view passed source review, failed the browse

**Incident (iteration 10).** The browser audit check 5 recorded
`has_schemaVersion:false` and top-level keys
`["id","name","module","version","purity","calledBy","parameters","steps","returns","errors"]` — graph-level `tool`, `schemaVersion`, `generatedAt`, `rootDir` were missing. Check 8 recorded `Jump ↑` collapsing the trail so `backButtons: 0`; the audit traced this to `navigateToFunction` using `prev.slice(0, existingIdx + 1)` and `Back` rendering only when `navTrail.length > 1`. Iteration 13's re-audit reported "6 PASS, 0 FAIL" on all three fixed bugs.

**Rule.** A structural UI feature is not proven by reading the source. Verify
the rendered behavior in a real browser, including the paths that remove o
collapse state.

**Checkable procedure.** Drive the actual control, snapshot the resulting DOM
text and button counts, and repeat the original failing scenario after the
fix. Full checklist: `docs/workflows/boundary-contract-audit.md`.

## L6 — A drift row said `Open` while its note said `Resolved`

**Incident (iteration 13).** The ledger's docs-closure line reads: "Docs: PRD-TRD + IMPL `SHIPPED`, drift D-1 + D-2 `Resolved` (J10 caught and fixed a stale row status)." The stale row was D-2: its machine-readable status column still held `Open` while its prose resolution note already said `Resolved`. The current `docs/drift.md` shows the corrected row and its `Verified by:` note.

**Rule.** Machine-readable status fields are the gate. A prose note does not
count. A row whose fix rides in the same diff must read `Resolved`.

**Checkable procedure.** Read the status cell, not the paragraph. Compare the
status cell to every `Resolved`/`Open` note in the same entry. If they differ,
fix the status cell and re-read the file. This is also stated in `docs/drift.md`
under "Review rule (non-negotiable)".

## L7 — A concern raised after the fix had already landed

**Incident (iteration 12).** The ledger records the integrated fixes:
"Integrated J06 runner arg mapping and J08 UI fixes; formatted and re-ran the
full suite and repo harness" and, after re-formatting,
"`cargo fmt --all -- --check` -> `FMT_STATUS=0`; `scripts/test-all.sh` -> `ALL_TESTS_OK` ... `scripts/enrich-e2e.sh` -> `ENRICH_E2E_OK`. All 72 Rust tests pass, including real Node invocation and scalar-by-name dispatch."

The brief also describes J08 escalating a stale concern about runner code J06
had already fixed. I could not find that escalation in the ledger iterations I
read; see "Evidence limits" below. The rule stands independently of that
attribution.

**Rule.** Verify a concern against the current code before escalating it. Cite
the exact line and the commit or job that changed it.

**Checkable procedure.** Open the file at the current revision, quote the line
that still shows the problem, and name the job that was supposed to fix it. If
the line is already correct, withdraw the concern in writing instead of
escalating it.

## L8 — Empty `trace` made the UI badges inert

**Incident (iterations 13–14).** The browser re-audit's remaining observation:
"Node-mode responses carry `"trace":{}` (empty). Step trace badges in the
executionSteps list therefore do not refresh from a real run." Ledge
iteration 13 pivot: "one polish item — J06 to merge the deterministic trace
into Node-mode responses so step badges refresh." Iteration 14 action: "Final
polish (merge deterministic trace into Node-mode responses), then all gates
and cleanup."

**Rule.** An API response is a UI contract. If the backend omits a field the
UI renders, the feature is incomplete even when the data it does return is
correct.

**Checkable procedure.** Enumerate the response fields the UI reads, capture a
real Node-mode response, and check each field is populated. A schema-valid but
empty `trace` is still a gap.

## L9 — Polyglot expansion must not touch core

**Incident (iteration 16).** The user expanded the goal to "polyglot language
support with a pluggable Tree-sitter/language-backend architecture." The
contract update states: "adding a language requires ZERO changes to core graph
builder, schema, enricher or UI." Acceptance evidence (1)–(5) also requires
"existing TS/JS gates remain green."

**Rule.** A new backend is a plugin, not a branch in the graph builder.

**Checkable procedure.** When adding a language, diff the change and confirm
only the adapter registry plus the new adapter and its fixtures/tests moved.
If `fnscope-core`, the schema, the enricher or the UI changed, stop and treat
it as an architecture drift. Full design: `docs/concepts/language-adapter.md`.

## Evidence limits

Three details named in the brief could not be confirmed in the evidence files
and are therefore not quoted as ledger facts here:

1. The "call_line relative vs absolute" wording for a wrong assertion. Ledge
   iteration 4 records only "a wrong test assertion in core, corrected by J04"
   without the field name.
2. The `/mnt/c/...` -> `C:\mnt\c\...` mis-mapping. The ledger quotes the
   `file:///tmp/...` embedding and the `C:\tmp\...` failure, but not this one.
3. The J08 stale escalation in iteration 12. The ledger records the J06/J08
   fixes and the green gates, but no escalation text.

Each rule above is written so it holds without those details. If a future
agent finds the missing evidence, add the exact quote here rather than
assuming it.
