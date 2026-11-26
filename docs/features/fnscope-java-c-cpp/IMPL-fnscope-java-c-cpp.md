# fnscope-java-c-cpp — IMPL (implementation plan, reconstructed)

RECONSTRUCTED 2026-11-26. This plan was written after the work shipped; it is
NOT a pre-impl artifact. It maps J30-J34 to the files each job owned and to the
real gates recorded in `.reports/goal-build-fnscope.md`. No date is backdated.
Status: SHIPPED (commit `5408cadf2fc4a8ae05e8d93f67d8b82d612ec717`).

## Scope and ownership

Java, C and C++ are the second polyglot batch. Each adapter job ran in its own
isolated worktree and owned only its module, contract doc and fixture. J33 was
an independent source review with no implementation authority. J34 alone
owned the shared registration files. The orchestrator ran every gate, routed
failures back to the owning job and staged explicit paths for the one commit.
No agent ran cargo and no agent committed or merged its own work.

## Delivered jobs

| Job | File area owned | Delivered | Recorded result |
| --- | --- | --- | --- |
| J30 | `crates/fnscope-parser/src/java/`, `docs/contract/adapters/java.md`, `tests/fixtures/java_app/` | Java adapter with class/interface/record discovery, varargs, comments, type index, four step kinds, throw errors, effects; 18 unit tests | Iteration 31 `:303`; review `.reports/reviews/J30-J32.md:23-31` PASS |
| J31 | `crates/fnscope-parser/src/c_lang/`, `docs/contract/adapters/c.md`, `tests/fixtures/c_app/` | C adapter with top-level `function_definition` only, struct/union/typedef index, `fprintf`/guard/`assert` errors; 11 unit tests | Iteration 31 `:303`; review `:37-45` PASS |
| J32 | `crates/fnscope-parser/src/cpp/`, `docs/contract/adapters/cpp.md`, `tests/fixtures/cpp_app/` | C++ adapter with free functions and class methods, defaults/trailing returns, throw errors, stream effects; 14 unit tests | Iteration 31 `:303`; review `:51-58` PASS |
| J33 | `.reports/reviews/J30-J32.md` | One independent review across all three worktrees: PASS, 0 findings; registration notes preserved as external edits | Review `:2`, `:65-90`, `:92-98` |
| J34 | Root `Cargo.toml`, `crates/fnscope-parser/Cargo.toml`, `Cargo.lock`, `crates/fnscope-parser/src/lib.rs`, `crates/fnscope-parser/src/adapter.rs`, `crates/fnscope-core/src/scan.rs` | Registered the three adapters, their grammar deps and default globs; the only job touching shared files | Iterations 31-32 `:307`, `:311`, `:314` |

All four jobs are SHIPPED in `5408cad`. The commit contains 26 files: nine
adapter `.rs` files, three contract docs, three fixture READMEs and three
fixture sources, plus the shared registration changes and the ledger/review.

## Job detail

### J30 — Java adapter

Own `java/mod.rs`, `java/discover.rs`, `java/body.rs`, the contract doc and the
fixture. Implement all six `LanguageAdapter` methods, delegate
`resolve_return_fields` to `crate::resolve::resolve_fields`, and keep grammar
knowledge local. Only root `class_declaration`, `interface_declaration` and
`record_declaration` become callables; each is `ClassName.method`. Handle the
wrapper table in `docs/contract/adapters/java.md:35-49`, keep `when` verbatim
per spec step 4 (`java/body.rs:783-795`, test `:1016-1018`), and report the
external registration edits without making them (`java.md:154-174`).
**Gate:** 18 unit tests, CR=0 by `tr`, format check labelled format-only.

### J31 — C adapter

Own `c_lang/mod.rs`, `c_lang/discover.rs`, `c_lang/body.rs`, the contract doc
and the fixture. Module name `c_lang`, public id `"c"`, suffix `"c"`; `.h` is
not claimed (`docs/contract/adapters/c.md:23-27`). Only top-level
`function_definition` nodes become callables; prototypes and nested bodies are
excluded. Map declaration-with-call to `Call`, and C's three error idioms to
`ErrorSpec`. **Gate:** 11 unit tests, CR=0 by `tr`, registration edits reported
but not made (`c.md:130-155`).

### J32 — C++ adapter

Own `cpp/mod.rs`, `cpp/discover.rs`, `cpp/body.rs`, the contract doc and the
fixture. `id() == "cpp"`, `extensions() == &["cpp", "cc"]`; `.cxx`/`.hpp` are
left to a future decision (`docs/contract/adapters/cpp.md:11-14`). Handle
`field_declaration_list`, `condition_clause`, `init_declarator`, the absent
`method_definition`/`throw_expression` nodes and a defensive `statement_list`
wrapper. **Gate:** 14 unit tests, CR=0 by `tr`, no compile claim in the doc
(`cpp.md:109`).

### J33 — independent review

Read every owned file in the three worktrees plus the shared targets of the
registration edits. Checklist: exact trait surface, owned paths only, shared
resolver delegation, discovery, body, tests, fixture coherence, LF/temp-file
hygiene and L11 semantic sanity. Prove the CR detector with a positive and a
negative control before trusting a zero. **Gate:** PASS with 0 findings
(review `:2`), with the explicit limit "no defect found by source audit", not
a compile verdict (`:94`).

### J34 — registration

Apply only the external edits preserved in the review's registration notes:

1. Workspace deps: `tree-sitter-java = "0.23.5"`, `tree-sitter-c = "0.24.2"`,
   `tree-sitter-cpp = "0.23.4"`; parser deps with `.workspace = true`;
   `Cargo.lock` updated (review `:69`, `:72-73`, `:79-80`, `:86-87`).
2. `pub mod java;`, `pub mod c_lang;`, `pub mod cpp;` in `lib.rs`
   (review `:74`, `:81`, `:88`).
3. Imports and `register(Box::new(...))` calls in
   `Registry::with_builtin_adapters`, plus id-table rows `("java", "java")`,
   `("c", "c")`, `("cpp", "cpp")` and `("cc", "cpp")` (review `:75`, `:82`,
   `:89`).
4. Default globs in `ScanOptions::default()` and the matching expectation in
   `default_options_match_the_contract`: `**/*.java`, `**/*.c`, `**/*.cpp`,
   `**/*.cc` (review `:76`, `:83`, `:90`).

**Gate:** compile + tests + mixed scan, run by the orchestrator only.

## Gate trail (real commands and verdicts)

| Iteration | Command / evidence | Verdict | Recorded pivot |
| --- | --- | --- | --- |
| 31 (`:303-308`) | Java 18 / C 11 / C++ 14 tests reported by owners; CR=0 via `tr`; format checks labelled format-only | Pending review and gate | One registration job after review |
| 32 (`:310-315`) | `J33 reviewed Java/C/C++ with PASS (0 findings)` | In progress | Separate worktrees; one registration owner |
| 33 (`:317-324`) | `cargo test --workspace` -> `ADAPTER_GATE_STATUS=101`; seven E0597 errors at `c_lang/body.rs 207,398,881`, `cpp/body.rs 190,201`, `java/body.rs 211,445` | Fail (compile), L1 class | Exact locations sent to J30/J31/J32; no orchestrator edits |
| 34 (`:325-331`) | `ADAPTER_GATE_STATUS=101`; parser 111 passed / 2 failed: C doc description kept `*/` and Java `when` had parentheses with a stale test expectation | Fail (2 assertions) | Exact outputs to J31/J30 with an explicit code-vs-test decision request for Java |
| 35 (`:333-338`) | Gate green: 130 tests; mixed 8-language scan: 46 functions across ts/js/rs/py/go/java/c/cpp, 105 steps, 19 resolved calls, 0 dangling; `5408cad` | Pass | Proceed to the final five adapters |

These are the only compile/test verdicts for the batch. No format marker and
no source review were presented as a compile result (L11,
`docs/ethics/lessons.md:226-252`). Failed gates were recorded, not hidden, and
each fix was returned to the file's owner (L1, `docs/ethics/lessons.md:29-49`).

## Housekeeping recorded with the batch

- All 18 owned files had `tr -cd '\r' < file | wc -c` equal to 0, with the
  detector proven against a CRLF positive (`2`) and an LF negative (`0`)
  (review `:10-11`, per-language item 8 in each table). No `sed 's/\r$//'` was
  used (L11).
- No temp logs, scratch files, `*.orig` or `nohup.out` in the three worktrees
  (review evidence line 13).
- No hardcoded absolute paths in owned files; the `~/.cargo/...` mention in
  `cpp.md:27` is home-relative (review `:57`).
- No agent commit in any adapter worktree; HEAD stayed at `01062af` until the
  integration commit (review evidence line 9).

## Verification artifacts in this pack

- `simulate.sh` — canonical post-impl source-level simulation: scans the three
  fixtures, asserts suffix and registration facts, asserts call edges and
  checks that no edge endpoint is dangling. Pure shell; no cargo, no server.
- `archive/simulate-pre.reconstructed.sh` — labelled design-time placeholder;
  it asserts nothing and is not a verdict.
- `GRILL-fnscope-java-c-cpp.txt` — locked decisions with decision/WHY/SOURCE/
  ALIGNMENT for the nine reconstructed choices.
- `PRD-TRD-fnscope-java-c-cpp.md` — behavioral and technical requirement set.

## Sources

`.reports/goal-build-fnscope.md` iterations 30-35; `.reports/reviews/J30-J32.md`;
`docs/ethics/lessons.md` L1/L2/L10/L11; `docs/drift.md` D-5;
`docs/contract/adapters/{java,c,cpp}.md`;
`crates/fnscope-parser/src/{java,c_lang,cpp}/`;
`tests/fixtures/{java_app,c_app,cpp_app}/README.md`; commit `5408cad`.
