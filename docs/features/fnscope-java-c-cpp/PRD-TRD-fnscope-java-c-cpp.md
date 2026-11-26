# fnscope-java-c-cpp — PRD-TRD (reconstructed)

RECONSTRUCTED 2026-11-26. This document was written after J30-J34 shipped the
Java, C and C++ adapters in commit `5408cad`. It is a reconstruction from the
locked spec, the ledger, the independent review and the shipped files — not a
pre-impl artifact. No date is backdated and no quote is invented. Status:
SHIPPED.

## Why

The base product scans TS/JS into a schema-valid function graph manifest.
Iteration 15 added Rust; iteration 16 expanded the goal to:

> "fnscope scans Rust, TS/JS, Python, Go and the standard language set with
> accurate function graphs and execution steps; adding a language requires
> ZERO changes to core graph builder, schema, enricher or UI"
>
> — `.reports/goal-build-fnscope.md:181`, also quoted in L9 at
> `docs/ethics/lessons.md:187-199`

Java, C and C++ are the second polyglot batch, after Python/Go (`01062af`,
iteration 30). The adapter registry from J21 supplies the seam, so they are
plugins and the core graph builder, schema, enricher and UI do not change. They
ship together because all three need the same shared registration edits and
one gate (iterations 30-35, `.reports/goal-build-fnscope.md:294-338`).

The batch is recorded at iteration 35: "Java/C/C++ gate green (130 tests),
mixed 8-language scan passed (46 functions across ts/js/rs/py/go/java/c/cpp,
105 steps, 19 resolved calls, 0 dangling), batch committed as `5408cad`".

## Behavioral spec

### Adapter identity

| Fact | Java (J30) | C (J31) | C++ (J32) |
| --- | --- | --- | --- |
| Module | `crates/fnscope-parser/src/java/` | `crates/fnscope-parser/src/c_lang/` | `crates/fnscope-parser/src/cpp/` |
| Public type | `JavaAdapter` | `CAdapter` | `CppAdapter` |
| `id()` | `"java"` | `"c"` | `"cpp"` |
| `extensions()` | `&["java"]` | `&["c"]` | `&["cpp", "cc"]` |
| Grammar | `tree_sitter_java::LANGUAGE` 0.23.5 | `tree_sitter_c::LANGUAGE` 0.24.2 | `tree_sitter_cpp::LANGUAGE` 0.23.4 |
| Unit tests reported | 18 | 11 | 14 |

All three implement the six `LanguageAdapter` methods with the signatures in
`docs/contract/adapter-api.md:13-20`, hold no global state, expose `new()` and
`Default`, and delegate `resolve_return_fields` to the shared grammar-free
helper `crate::resolve::resolve_fields` (`java/mod.rs:15,58-63`,
`c_lang/mod.rs:20,64-69`, `cpp/mod.rs:16,60-65`). Suffixes are bare lowercase
strings; `Registry::for_extension` trims a leading dot and ignores case. C's
`.h` is excluded because headers repeat `.c` declarations
(`docs/contract/adapters/c.md:23-27`); C++ does not claim `.cxx`/`.hpp`
because this job owns only two suffixes (`docs/contract/adapters/cpp.md:11-14`).
The test counts are from iteration 31 (`:303`) and were matched by the
independent reviewer (`.reports/reviews/J30-J32.md:28`, `:42`, `:56`).

### Discovery

- **Java:** only root `class_declaration`, `interface_declaration` and
  `record_declaration` are walked. Every callable is `ClassName.method` with
  `class_name = Some("ClassName")` and `FnKind::Method`; a constructor is
  `ClassName.ClassName`. Varargs are parameters, record components enter the
  type index, nested local classes are not descended. Results sort and dedupe
  by `(line, qualified_name)` (`docs/contract/adapters/java.md:35-49`, `:68-85`;
  `java/discover.rs:46,101,124,75-76`).
- **C:** only top-level `function_definition` nodes are callables; prototypes
  and nested definitions are skipped. The bare identifier is both `name` and
  `qualified_name`, `class_name` is `None`, `FnKind::Declaration`. Struct,
  union and typedef fields form the type index. Declarators are unwrapped
  through pointer/parenthesized/attributed/array nodes
  (`docs/contract/adapters/c.md:23-27`, `:38-44`, `:57-76`;
  review `:40`).
- **C++:** direct `function_definition` children of `translation_unit` are
  free functions; scoped `Calc::method` definitions become methods. Members
  come from `field_declaration_list`; no walk descends into a
  `compound_statement`, so nested helpers are excluded. `parameter_declaration`
  is required, `optional_parameter_declaration` carries a default. There is no
  `method_definition` node and no `throw_expression` node in this grammar
  (`docs/contract/adapters/cpp.md:19-46`, `:49-73`; review `:54`).
- No new model field is introduced: `DiscoveredFn`, `BodyAnalysis`, `Step`,
  `ErrorSpec` and `ReturnShape` keep the shapes in `docs/contract/rust-api.md`.

### Body analysis

- Top-level statements in source order become `step-1`, `step-2`, ... with one
  of four kinds: `call`, `branch`, `transform`, `return` (spec step 3,
  `fnscope_architecture_agent_build_spec.md:75-91`).
- A declaration whose initializer calls a function is a `call`, not a
  `transform`, per the spec step 3 rule and its `const tokens = tokenize(...)`
  schema example (`:81`, `:151-161`). This is the same decision that closed
  D-5 for Python/Go (`docs/drift.md` D-5); Java asserts it at
  `java/body.rs:969-978`, C at `c_lang/body.rs:952-961`, C++ at
  `cpp/body.rs:892-900`.
- `callee_names` keeps raw names; a `Call` step carries `calls_fn_id` when the
  callee is known. Core alone resolves the bidirectional `calledBy` edge.
- Errors: Java `throw new X("CODE")`, C `fprintf(stderr, "CODE")` and guarded
  `return -1`/`return NULL` and `assert(expr)`, C++ `throw X("CODE")` and
  `throw std::runtime_error("CODE")`. Sites dedupe by code, first wins.
  Status mapping is documented per adapter; the spec's worked example maps
  `TOKEN_NOT_FOUND` to `422` (`docs/contract/adapters/{java,c,cpp}.md`).
- **`when` is verbatim.** Spec step 4 says "Extract enclosing `if (...)`
  condition text as `when`" (`:93-96`). `enclosing_condition` in
  `java/body.rs:783-795` returns the condition node text unchanged, so a guard
  `if (text.isEmpty())` yields `when == "(text.isEmpty())"`, asserted at
  `:1016-1018` and documented in `tests/fixtures/java_app/README.md:35-40`.
  Iteration 34 recorded the mismatch with a test expecting no parentheses
  (`.reports/goal-build-fnscope.md:326-331`); per L2 the contract, not the
  assertion, was authoritative. A throw without a guard uses `explicit throw`
  (`java/body.rs:1037`).
- Purity and permissions use the existing labels: pure `150 ms`/`universal`,
  side effect `1000 ms`/`node20`, network `5000 ms`/`node20`. These are
  execution labels, not a claim that Java/C/C++ run on Node.
- Deterministic repeat: each body walk is source-order stable and repeat tests
  assert equality (`java/body.rs:1069-1087`, `c_lang/body.rs:1039-1057`,
  `cpp/body.rs:986-1002`).

### Grammar wrappers

The three grammars insert wrapper nodes between a block or declaration and the
fact the adapter expects. The adapters look through them and keep a fallback
to the original node:

| Wrapper | Language | Handling |
| --- | --- | --- |
| `class_body` / `interface_body` / `formal_parameters` / `block` / switch groups | Java | `docs/contract/adapters/java.md:35-49` |
| `field_declaration_list`, pointer/parenthesized declarators; defensive `statement_list` | C | `docs/contract/adapters/c.md:38-44` |
| `field_declaration_list`, `condition_clause`, `argument_list`, `init_declarator`, `trailing_return_type`; defensive `statement_list` | C++ | `docs/contract/adapters/cpp.md:49-73` |

The review recorded the same evidence at `.reports/reviews/J30-J32.md:26`,
`:40`, `:54`, and noted at `:96` that no real tree-sitter parse was run by the
reviewer. Iteration 31 (`:304`) required explicit wrapper handling; the Python/Go
batch had already failed nine tests on the same class of defect (iterations
28-29, `:278-292`).

### Fixtures

- `tests/fixtures/java_app/` — `Main.java` has seven expected callables
  (`Main.normalize`, `Main.analyze`, `Main.readConfig`, `Main.auth`,
  `Main.run`, `Main.main`, `FunctionError.FunctionError`), a `RoleResult` type
  with `role`/`score` fields, errors `EMPTY_TEXT` (`400`, `when`
  `(text.isEmpty())`) and `TOKEN_NOT_FOUND` (`422`, `when`
  `(text.isBlank())`), and two call edges: `Main.run` → `Main.analyze` and
  `Main.main` → `Main.run` (README `:17-60`).
- `tests/fixtures/c_app/` — `main.c` has four callables (`add`, `normalize`,
  `report`, `main`), a `Point` struct, errors `EMPTY_TEXT`/`NULL`/`ASSERT`
  with status `400`, and the `main` → `report` edge (README `:17-48`).
- `tests/fixtures/cpp_app/` — `main.cpp` has seven callables (including two
  `Calculator.Calculator` rows), one `Calculator` type, error `NEGATIVE_INPUT`
  with status `400` and `when` `n < 0`, and the `normalize` → `add` edge
  (README `:15-50`). It is a scanner fixture with no `main`, not an executable.

## Simulation contract

- `simulate.sh` is the canonical **post-impl** simulation. It is pure shell:
  no `cargo`, no server and no Java/C/C++ toolchain. It resolves the repo root
  from its own path (no hardcoded drive or user directory), "scans" each of the
  three fixtures by reading its source and README facts, asserts the claimed
  suffix set of each adapter and the registered/glob facts in shared files,
  asserts each documented call edge and asserts every edge endpoint is a known
  function (no dangling). It prints one `[PASS]`/`[FAIL]` line per assertion
  and a final `PASS` or `FAIL`, exiting 0 only on PASS. It is a deterministic
  source-level simulation, not a compile or runtime gate.
- `archive/simulate-pre.reconstructed.sh` is a **labelled design-time
  placeholder**. No pre-impl simulation artifact exists in the evidence, so it
  asserts nothing and prints a placeholder marker. It is not a verdict and
  must not be quoted as one.
- All five deliverables are LF-only: verified with
  `tr -cd '\r' < file | wc -c` equal to 0, using the single-quoted form from a
  script file — never `sed` (L11, `docs/ethics/lessons.md:226-252`).

## Technical design

- One adapter area per language (`java/`, `c_lang/`, `cpp/`) with `mod.rs`,
  `discover.rs`, `body.rs`; grammar knowledge stays inside it. The shared
  resolver remains grammar-free, so no core, schema, enricher or UI file
  changes for a new language (iteration 16 invariant, L9).
- Cargo: workspace entries `tree-sitter-java = "0.23.5"`,
  `tree-sitter-c = "0.24.2"`, `tree-sitter-cpp = "0.23.4"` with matching
  `.workspace = true` parser deps and a `Cargo.lock` update. The review warns
  at `.reports/reviews/J30-J32.md:69` that all three were absent from the
  lock before integration.
- Registration is one separate job (J34) after review: three module lines in
  `lib.rs`, three `register(...)` calls plus id-table rows in `adapter.rs`, and
  `**/*.java`, `**/*.c`, `**/*.cpp`, `**/*.cc` defaults in
  `crates/fnscope-core/src/scan.rs` (`.reports/reviews/J30-J32.md:65-90`,
  ledger iteration 32 `:314`).
- Review and gate loop: J33 gave PASS with 0 findings but explicitly ran no
  cargo (review `:2`, `:94`); iteration 33's real gate found seven E0597
  iterator-temporary errors and returned them to the owners (`:317-324`);
  iteration 34 found two assertion failures and returned them too (`:325-331`);
  iteration 35 recorded the green gate (`:333-338`). This is L1 and L11 in
  practice (`docs/ethics/lessons.md:29-49`, `:226-252`).
- Tests live beside the code: 18 + 11 + 14 reported in iteration 31 and
  matched by the reviewer. The real gates are the orchestrator's, recorded in
  iterations 33-35. Delivery: `5408cad` (26 files).

## Non-goals

- **No Java/C/C++ execution via `invoke`.** The runtime labels are schema
  labels only; none of these languages is run by the Node runner.
- No new schema field, no rename of locked contract names, no change to the
  core graph builder, enricher or UI for any of the three languages.
- No `.h`, `.cxx`, `.hpp`, `.hh` or `.c++` suffix claim; no header indexing.
- No registration edit inside an adapter worktree; J34 owns the shared files.
- No claim that `javac`, `cc` or a C++ build was run by this reconstruction.
  The reviewer explicitly read those fixture runtime claims but did not
  execute them (review `:97`).
- No claim that `rustfmt --check` or a source review proves compilation (L11).
- No backdating: this reconstruction is dated 2026-11-26 and cites the ledger
  iterations as historical evidence.

## Sources

`.reports/goal-build-fnscope.md` iterations 30-35; `.reports/reviews/J30-J32.md`;
`docs/ethics/lessons.md` L1/L2/L9/L11; `docs/drift.md` D-5;
`fnscope_architecture_agent_build_spec.md` sections 3-4, step 3 and step 4;
`docs/contract/{rust-api,adapter-api}.md`;
`docs/contract/adapters/{java,c,cpp}.md`;
`crates/fnscope-parser/src/{java,c_lang,cpp}/`;
`tests/fixtures/{java_app,c_app,cpp_app}/README.md`;
commit `5408cadf2fc4a8ae05e8d93f67d8b82d612ec717`.
