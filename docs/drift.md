# fnscope — drift log

The drift log records the gap between a locked decision (the contract or the
spec) and the code. It is a gate, not a diary. A gap nobody closes is a
decision nobody kept.

## Template

    D-N | date | doc says | code does | status | owner

- `D-N` — unique id, next unused number.
- `date` — `YYYY-MM-DD` the gap was found.
- `doc says` — the exact words from the doc, with `path:line`. Quote the doc;
  never cite from memory.
- `code does` — what the code actually does, with `path:line`.
- `status` — one of:
  - `Open` — a real gap. It describes the gap, never the fix.
  - `Awaiting decision` — an extension or contradiction that needs the
    orchestrator's or the user's call. It carries the exact question.
  - `Resolved` — fixed, with a `Verified by:` note saying how.
- `owner` — the job id or agent responsible for closing it.

## Live entries

    D-N | date | doc says | code does | status | owner

    D-1 | 2026-11-25 | tests/fixtures/sample_app/README.md:19 says "Node 24 strips TypeScript types by default and resolves .ts specifiers as written" | WSL Debian's `node` on PATH is v20.20.2, which cannot import .ts specifiers; the Windows Node v24.20.0 is available as `/mnt/c/Program Files/nodejs/node.exe` | Resolved | J06 + orchestrator

D-1 resolution path: `fnscope-server` Node mode reads `FNSCOPE_NODE` before falling back to `node` on PATH, and its Node-mode test skips with a printed reason when Node < 22 and no override is set. Real invocation has now been verified with native Node 24 installed in WSL and selected with `FNSCOPE_NODE`; see the Resolved note below.

**D-1 -> Resolved 2026-11-25.** Verified by: native Node 24 installed in WSL and selected through `FNSCOPE_NODE`; `cargo test --workspace` includes `node_invocation_runs_a_real_export` and `node_invocation_passes_a_scalar_parameter_by_name`, both passing; and the real browser audit in `.reports/ui-browser.md` showed genuine Node execution (`clamp -> 10`) with no post-Run `simulated: true`. The Node runner passes the module path and arguments as argv instead of embedding a Linux `file:///` URL, and creates its scratch directory beside the manifest so WSL path translation cannot point Windows Node at a nonexistent file.

    D-2 | 2026-11-25 | fnscope_architecture_agent_build_spec.md section 4 step 2 says "if it resolves to an object/interface (or Promise<T>), unpack top-level properties into fields" | Real scan of `tests/fixtures/sample_app` gives `evaluate-grammar-role.returns.fields == []` because `RoleEvaluationResult` is declared in another module and discovery resolves aliases only within the same file | Resolved | J02 + J04

D-2 resolution path: add `collect_types` / `resolve_return_fields` to `fnscope-parser` (J02) and build a project-wide type index in `fnscope-core` (J04). Keep D-2 Open until a real scan shows the fields populated.

**D-2 -> Resolved 2026-11-25.** Verified by: `crates/fnscope-core/tests/scan.rs::scan_resolves_return_fields_from_project_wide_types` passes, and a real `fnscope scan tests/fixtures/sample_app` now reports `evaluate-grammar-role.returns.fields` with `roleId`, `label`, `confidence`, `intuition` from `src/types.ts`. J04 correctly rejected my suggested assertion that `roleId.type_ == "string"` — the fixture declares a string-literal union, and the test asserts the declared text instead.

## Review rule (non-negotiable)

When a review finds a gap between a locked doc and the code, append the entry
to this file **before the next edit**. The next edit must not silently fix or
ignore the gap: it must name it first. An Open entry must be resolved in the
same commit that touches the named file. An entry whose fix rides in the same
diff is `Resolved`, not `Open`.

Never delete an entry to silence a check. Fix it, or record why it cannot be
fixed. Fix a stale reference instead of deleting the entry. New design is a
decision, not drift; file it as `Awaiting decision` with the exact question.
Do not author an entry that permits the change you are making in the same
diff — a surgical change cites a D-N that existed before it began.

## Why this exists

Drift is a gate in this repo (AGENTS.md). A build split across subagents only
stays coherent if a discovered mismatch is written down before anyone is
allowed to act on it. The log is the record of what was promised versus what
was shipped, and it is the proof that a later agent did not quietly rewrite
the promise.
