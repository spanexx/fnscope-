# Triage — Python/Go language batch

Job: J27 (read-only triage). Gate evidence: `ADAPTER_GATE_STATUS=101`, `cargo test --workspace`, parser 61 passed / 9 failed, exact assertions from `/tmp/fnscope-adapter-gate.log` (ledger iteration 28, `.reports/goal-build-fnscope.md:278-284`). No code was changed, no cargo was run, no test was re-run by this triage.

## Summary

| Test | Decision | Owner |
| --- | --- | --- |
| `golang::tests::go_adapter_resolves_return_fields_from_a_type_index` | CODE | J24 |
| `golang::discover::tests::go_collect_types_indexes_struct_interface_and_alias` | CODE | J24 |
| `golang::body::tests::go_extracts_panic_and_error_constructor_codes` | CODE | J24 |
| `golang::body::tests::go_analysis_is_deterministic_on_repeat` | CODE | J24 |
| `golang::tests::go_discovery_resolves_same_file_return_fields` | CODE | J24 |
| `golang::body::tests::go_analyzes_call_branch_transform_return_steps` | BOTH | J24 (code + test in the same file) |
| `golang::body::tests::go_classifies_go_and_defer_as_calls` | CODE | J24 |
| `python::tests::collects_module_type_aliases` | CODE | J23 |
| `python::tests::analyzes_call_branch_transform_and_return_steps` | TEST | J23 (test only — production diff must be empty) |

Verdict counts: **CODE 7, TEST 1, BOTH 1** (9 failures).

Root causes: two Go AST-wrapper misses (`statement_list`, `field_declaration_list`), one Python AST-wrapper miss (`expression_statement`), one contract-breaking test expectation (Python), one Go test that both exposes a wrapper bug and encodes a contract-breaking `Transform` expectation.

Top 3 required fixes (details below):
1. `crates/fnscope-parser/src/golang/body.rs:88-97`, `analyze()` — descend through the `block` → `statement_list` wrapper so top-level statements become steps (fixes 4 Go body failures: the 3 pure-CODE ones plus the code half of the BOTH case).
2. `crates/fnscope-parser/src/golang/discover.rs:287-313`, `TypeIndex::fields_from_struct()` — descend through `struct_type` → `field_declaration_list` so struct fields are indexed (fixes all 3 Go return-field failures).
3. `crates/fnscope-parser/src/python/discover.rs:275-311`, `type_index()` — unwrap `expression_statement` to reach the `assignment` before applying the left/right/type-expression checks (fixes the Python alias failure).

## Details

### golang::tests::go_adapter_resolves_return_fields_from_a_type_index

- Location: `crates/fnscope-parser/src/golang/mod.rs:97` (panic at `:97:9`), test body `:84-100`.
- Assertion: `assert_eq!(resolved.returns.fields.len(), 1)` after `GoAdapter.resolve_return_fields(&mut resolved, &types)`; actual `left: 0`, expected `right: 1`.
- Contract clause: `docs/contract/adapter-api.md:13-20` fixes the trait: `fn collect_types(&self, source: &str) -> BTreeMap<String, ReturnShape>;` and `fn resolve_return_fields(&self, found: &mut DiscoveredFn, types: &BTreeMap<String, ReturnShape>);`. `docs/contract/adapter-api.md:73-77`: the shared resolver is grammar-free and "only manipulates declared type *names*". `fnscope_architecture_agent_build_spec.md:68-70`: "**`returns`:** Extract resolved return type (`type`) and if it resolves to an object/interface (or `Promise<T>`), unpack top-level properties into `fields: [{ name, type }]`." Adapter doc `docs/contract/adapters/go.md:75-82` promises that "Structs contribute their named fields" and "a simple alias copies the fields of its target".
- Decision: **CODE**. The test supplies the struct in a different file (`types_source = "package p\ntype Item struct { id int }"`), so the only way fields can appear is through `collect_types` → shared resolver; the test mirrors the locked trait and the documented cross-file contract exactly. The failure is upstream: `collect_types` returns an empty map because `fields_from_struct` never reaches the fields (see the `go_collect_types_indexes_struct_interface_and_alias` entry for the AST evidence). The resolver and the test are behaving as designed; production is not.
- Required fix: in `crates/fnscope-parser/src/golang/discover.rs`, change `TypeIndex::fields_from_struct` (`:287-313`) so it descends into the `field_declaration_list` child of `struct_type` before filtering for `field_declaration` nodes. Do not change `golang/mod.rs` or `resolve.rs`.

### golang::discover::tests::go_collect_types_indexes_struct_interface_and_alias

- Location: `crates/fnscope-parser/src/golang/discover.rs:636` (panic at `:636:38`), test body `:628-645`.
- Assertion: `types.get("Item").expect("struct must be indexed")`; actual `"struct must be indexed"` (no `Item` key at all).
- Contract clause: `fnscope_architecture_agent_build_spec.md:68-70` requires return types that resolve to an object to unpack their properties into `fields`; `docs/contract/adapters/go.md:41-42` lists "`type_spec`, `struct_type`, `field_declaration` | Struct fields in the type index" and "`interface_type`, `method_elem` | Interface methods in the type index"; `:75-79` requires structs, interfaces and aliases in one `BTreeMap`.
- Decision: **CODE**. The test is consistent with the adapter's own documented type-index contract. Runtime AST evidence: in the compiled `tree-sitter-go 0.25.0` grammar, `block`→`statement_list` is not the only wrapper — `struct_type`'s named child is `field_declaration_list`, and the `field_declaration` nodes live *inside* that list (`.../tree-sitter-go-0.25.0/src/node-types.json:962-974`, and the `struct_type` child list at `:2110-2120`). `fields_from_struct` at `discover.rs:289-291` iterates `node.named_children()` of `struct_type` and accepts only kind `field_declaration`, so it sees only `field_declaration_list` and returns an empty `Vec`. `named_map()` (`:346-357`) then drops the empty entry, and the alias copy at `:341-344` has no target to copy. Interfaces are unaffected (their `method_elem` nodes are direct children, `node-types.json:1420-1434`), so this is a struct-only defect, exactly as reported.
- Required fix: `crates/fnscope-parser/src/golang/discover.rs:287-313`, `TypeIndex::fields_from_struct`: find the `field_declaration_list` named child of `struct_type` and iterate *its* named children for `field_declaration` (a small recursive walk that skips non-`field_declaration` wrappers is also acceptable). Keep grouped `a, b int` handling and the embedded-field skip unchanged. No test edit: the assertions (`Item`, `Reader`, `Alias`, the `func() (int, error)` signature) all match the contract.

### golang::body::tests::go_extracts_panic_and_error_constructor_codes

- Location: `crates/fnscope-parser/src/golang/body.rs:980` (panic at `:980:9`), test body `:960-985`.
- Assertion: `analysis.steps.iter().any(|step| step.kind == StepKind::Branch && step.output == "throws EMPTY_TEXT | continues")`; actual false (no such step). The preceding `errors` assertions at `:976-979` passed.
- Contract clause: `fnscope_architecture_agent_build_spec.md:75-76`: "Walk the top-level statements inside the function's `Block` in execution order and classify each statement into one of four step kinds"; `:80`: "**`branch`**: `IfStatement` or `SwitchStatement` (especially guard clauses containing `ThrowStatement` or early `ReturnStatement`)"; `:89`: "`output`: ... `throws <ERROR_CODE> | continues` (for guard `IfStatement`)"; `:93-96` (step 4) requires walking all throw descendants and recording the enclosing `if` as `when`. `docs/contract/adapters/go.md:86-91,98-102` says the same for `panic`/`errors.New` and guard branches.
- Decision: **CODE**. The test source has two top-level `if` guards, the first containing `panic("EMPTY_TEXT")`, so a `Branch` step with that guard output is exactly what the contract demands; the error facts were already extracted correctly, proving only the step pass is broken. AST evidence: `tree-sitter-go-0.25.0/src/node-types.json:518-529` shows `block` has a single named child, `statement_list`. `analyze()` at `golang/body.rs:88-97` filters `named_children` of the `block` but never unwraps `statement_list`, so `statements` contains one synthetic node. `step_kind()` (`:306-329`) falls through to the `_` arm for that wrapper and, because the wrapper contains `panic`/`errors.New` calls, labels it `Call`; no `Branch` step is ever built.
- Required fix: `crates/fnscope-parser/src/golang/body.rs:88-97`, in `analyze()`: when collecting `statements`, if the `block`'s named child is `statement_list`, use that child's named statements instead (filter comments as today); keep the `Some(node) => vec![node]` fallback for non-block bodies. No test edit.

### golang::body::tests::go_analysis_is_deterministic_on_repeat

- Location: `crates/fnscope-parser/src/golang/body.rs:1066` (panic at `:1066:9`), test body `:1053-1069`.
- Assertion: `assert_eq!(first.steps.len(), 3)`; actual `left: 1`, expected `right: 3`.
- Contract clause: `fnscope_architecture_agent_build_spec.md:75-76` (top-level statements in execution order) and `:84-85`: "`id`: `step-${index + 1}`"; `docs/contract/adapters/go.md:84-91`: "Top-level statements of the function body are visited in source order and get ids `step-1`, `step-2`, ...". `docs/contract/rust-api.md:52-58` fixes `Step.id` as `"step-1", "step-2", ...`.
- Decision: **CODE**. The fixture `func ping(n int) int { if n < 1 { return 0 }; out := n + 1; return out }` has exactly three top-level statements, and the test's `step-1`/`step-3` assertions at `:1067-1068` are contract-correct. The `first == second` assertion at `:1065` passed, so determinism is fine; the single step is the same `statement_list` wrapper defect described above (the wrapper has no calls, so `step_kind()` labels it `Transform`, but it is still only one step).
- Required fix: same as the previous entry — `golang/body.rs:88-97`, `analyze()`, unwrap `statement_list`. No test edit.

### golang::tests::go_discovery_resolves_same_file_return_fields

- Location: `crates/fnscope-parser/src/golang/mod.rs:108` (panic at `:108:9`), test body `:103-110`.
- Assertion: `assert_eq!(found[0].returns.fields.len(), 1)`; actual `left: 0`, expected `right: 1`.
- Contract clause: `fnscope_architecture_agent_build_spec.md:68-70` (unpack object return properties into `fields`); `docs/contract/adapter-api.md:55-61`: `collect_types` delegates to the built-in adapter and `resolve_return_fields` to the shared resolver; `docs/contract/adapters/go.md:63-66,75-82` promises same-file struct field resolution through the type index.
- Decision: **CODE**. `discover()` builds its `TypeIndex` from the same source (`:35`) and `extract_go_returns` looks up `Item` through `resolve_fields_from_map` (`:218`), so this test is the same-file mirror of the cross-file failure and is contract-correct. It fails for the identical reason: `Item` is never indexed because `fields_from_struct` skips the `field_declaration_list` wrapper. No resolver or test defect.
- Required fix: `crates/fnscope-parser/src/golang/discover.rs:287-313`, `TypeIndex::fields_from_struct` — descend into `field_declaration_list`. No test edit.

### golang::body::tests::go_analyzes_call_branch_transform_return_steps

- Location: `crates/fnscope-parser/src/golang/body.rs:942` (panic at `:942:9`), test body `:916-957`.
- Assertion: `assert_eq!(kinds, [Branch, Transform, Call, Return])`; actual `left: [Call]`, expected `right: [Branch, Transform, Call, Return]`.
- Contract clause: `fnscope_architecture_agent_build_spec.md:75-82` — "Walk the top-level statements inside the function's `Block` in execution order and classify each statement into one of four step kinds: 1. **`call`**: Statement invokes another function/method ... 3. **`transform`**: `VariableStatement` or expression performing data mapping, filtering, regex, arithmetic, or destructuring **without external calls**." The locked schema example at `fnscope_architecture_agent_build_spec.md:151-161` shows `"kind": "call"` for `"code": "const tokens = tokenize(input.sentence)"` — a variable assignment whose initializer calls a function. `docs/contract/adapters/go.md:86-91` requires one step per top-level statement.
- Decision: **BOTH**. (a) CODE: the observed `[Call]` is the `statement_list` wrapper defect already proven above — the whole body is misclassified as one Call step. Fixing only that wrapper would make the old expectation pass, but (b) TEST: the expected `Transform` for `cleaned := normalize(text)` contradicts the locked step classification and the schema's own `const tokens = tokenize(...)` example: that statement invokes a function, so it must be a `Call` step, not a `Transform`. The adapter's local doc line `docs/contract/adapters/go.md:46` ("`short_var_declaration` ... | Transform steps") is itself in drift with the locked spec on call-initializer declarations and must be reconciled separately by the doc owner. The correct final sequence is `[Branch, Call, Call, Return]`.
- Required fixes (both sides, owned by J24, same file set):
  - Code: `golang/body.rs:88-97`, `analyze()` — unwrap `statement_list` as in the two preceding Go body failures. Then `golang/body.rs:306-329`, `step_kind()` — change the arm at `:320` (`"short_var_declaration" | "var_declaration" | "assignment_statement" => StepKind::Transform`) so it returns `StepKind::Call` when `has_call(statement)` is true (the existing helper at `:375-384` already excludes `func_literal` calls) and `StepKind::Transform` otherwise.
  - Test: `golang/body.rs:942-950` — change the expected array to `[Branch, Call, Call, Return]` with a comment explaining that a call-initializer declaration is a `call` per spec §4 step 3. Leave `:951-956` unchanged (`steps[0].title`, `steps[1].code`, `steps[2].kind == Call`, `callee_names == ["normalize", "Println"]` all remain valid after the fix).

### golang::body::tests::go_classifies_go_and_defer_as_calls

- Location: `crates/fnscope-parser/src/golang/body.rs:1002` (panic at `:1002:9`), test body `:996-1007`.
- Assertion: `assert_eq!(analysis.steps.len(), 2)`; actual `left: 1`, expected `right: 2`.
- Contract clause: `fnscope_architecture_agent_build_spec.md:75-82`: "**`call`**: Statement invokes another function/method"; `docs/contract/adapters/go.md:44`: "`call_expression`, `go_statement`, `defer_statement` | Call steps and `callee_names`"; `:86-91` and `:97`: "`go fn()` and `defer fn()` are Call steps."
- Decision: **CODE**. `func work() { go run(); defer stop() }` contains two top-level Go statements, each invoking a known function, so two `Call` steps with `calls_fn_id` `run` and `stop` are contract-correct and the test's later assertions at `:1003-1006` match the contract. The count of 1 is the `statement_list` wrapper again: the single wrapper node has calls, so it is emitted as one Call step instead of two.
- Required fix: `golang/body.rs:88-97`, `analyze()` — unwrap `statement_list`. The existing `step_kind()` arms for `go_statement`/`defer_statement` at `:313-319` already classify them correctly; no other code or test change.

### python::tests::collects_module_type_aliases

- Location: `crates/fnscope-parser/src/python/mod.rs:153` (panic at `:153:9`), test body `:148-157`.
- Assertion: `assert_eq!(types.len(), 2)`; actual `left: 0`, expected `right: 2`.
- Contract clause: `docs/contract/adapter-api.md:13-20` fixes `collect_types(&self, source: &str) -> BTreeMap<String, ReturnShape>`; `docs/contract/adapters/python.md:63-66`: "`collect_types` indexes simple module-level aliases (`UserId = int`, `Alias = Dict[str, int]`) keyed by alias name, with `ReturnShape.type_` set to the aliased expression and empty `fields`. Literal right-hand sides such as `MAX = 10` are not aliases and are skipped."
- Decision: **CODE**. The test asserts exactly the documented behavior, including the `MAX = 10` exclusion, and `is_type_expression()` at `python/discover.rs:313-319` already implements that distinction, so the test is not relaxing or inventing anything. AST evidence: in the compiled `tree-sitter-python 0.25.0` grammar, a module-level `UserId = int` is an `expression_statement` whose named child is the `assignment` (`.../tree-sitter-python-0.25.0/src/node-types.json:1584-1607`). `type_index()` at `python/discover.rs:279-281` iterates `root.named_children()` and `continue`s unless `child.kind() == "assignment"`, so it never reaches the nested assignment and returns an empty map.
- Required fix: `crates/fnscope-parser/src/python/discover.rs:275-311`, `type_index()`: for each named child of `root`, unwrap `expression_statement` to its inner `assignment` (e.g. extract a small helper equivalent to `body.rs:186-198`'s `statement_expression`, or inspect `child.named_child(0)`) before the existing `left` / `right` / `is_type_expression` checks. Keep the `identifier`/`subscript` allow-list so `MAX = 10` stays excluded. No test edit.

### python::tests::analyzes_call_branch_transform_and_return_steps

- Location: `crates/fnscope-parser/src/python/mod.rs:173` (panic at `:173:9`), test body `:159-193`.
- Assertion: `assert_eq!(kinds, vec![Call, Branch, Transform, Return])`; actual `left: [Call, Branch, Call, Return]`, expected `right: [Call, Branch, Transform, Return]`.
- Contract clause: `fnscope_architecture_agent_build_spec.md:75-82` — "**`call`**: Statement invokes another function/method"; "**`transform`**: `VariableStatement` or expression performing ... **without external calls**"; the locked schema example at `fnscope_architecture_agent_build_spec.md:151-161` marks `"const tokens = tokenize(input.sentence)"` as `"kind": "call"`. Adapter doc `docs/contract/adapters/python.md:74-80` maps "`expression_statement` containing a `call` node" to `Call`.
- Decision: **TEST**. The source statement is `result = text.upper()`. In tree-sitter-python this is an `expression_statement` wrapping an `assignment` containing a `call`, so the code's `_` arm in `classify_statement()` (`python/body.rs:208-215`) correctly returns `Call` via `has_call()`. That is the contract behavior: a variable assignment whose initializer calls a function is a `call` step, not a `transform` — the spec's schema example is explicit. The observed `[Call, Branch, Call, Return]` is therefore correct and the test's `Transform` expectation is the only wrong side; the rest of the test (single-line `code`, `throws EMPTY_TEXT | continues` guard at `:183-187`, `callee_names` and `calls_fn_id` for `helper` at `:188-192`) is contract-correct and passed up to this assertion. Per `docs/workflows/test-failure-triage.md:37-39`, only the test changes for this failure: the production diff must be empty.
- Required fix: `crates/fnscope-parser/src/python/mod.rs:173-181` — change the expected vector to `vec![StepKind::Call, StepKind::Branch, StepKind::Call, StepKind::Return]` and add a one-line comment citing spec §4 step 3 / the `const tokens = tokenize(...)` schema example. No production change for this verdict.

## Notes for the orchestrator (not part of the nine verdicts)

- Latent (not this failure): `python/body.rs:205` still maps a bare `assignment`/`augmented_assignment` node unconditionally to `Transform`. It is currently unreachable for top-level Python statements (they are `expression_statement`-wrapped, as the passing/failing output proves), so changing it is **not** part of the `analyzes_call_branch_transform_and_return_steps` fix. If aligned later, do it in a separate, tested change together with `docs/contract/adapters/python.md:78`.
- Doc drift to record, not silently edit: `docs/contract/adapters/go.md:46` says a `short_var_declaration` is a Transform step, which conflicts with locked spec §4 step 3 and its schema example once a call initializer is involved. Since `docs/contract/rust-api.md` and the architecture spec are locked, the adapter doc (and any fixture docs repeating the rule) should be corrected after the code/test fix; consider a `docs/drift.md` entry per `AGENTS.md`.
- These are AST-wrapper mistakes of the same class as the earlier Python/Go E0597 and E0308 defects: grammar node kinds were checked against cached grammar sources, but runtime shapes were never exercised until the gate ran. The owning agents should verify the resulting step arrays with the smallest crate-level test before the full workspace gate.
