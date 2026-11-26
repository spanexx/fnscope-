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

    D-3 | 2026-11-26 | .reports/goal-build-fnscope.md iteration 17 records "CR_COUNT=0" after LF normalization of the copied ethics docs | Six files still contain CR bytes: docs/ethics/cleanup.md:1, commenting.md:1, dev-lane.md:1, implementation.md:1, modular.md:1, origin.md:1. The earlier count used `$"\r"` (locale-translation form, a literal backslash-r) instead of `$'\r'`, so it could never match a CR byte and the green was false | Resolved | J20 + orchestrator

D-3 gap: the ledger promised LF-only endings but the six copied files still carry CR bytes. The corrected detector is `grep -rlU $'\r'` with `-U` so CR is not stripped before matching. Closure requires normalizing all six files to LF, re-running the corrected expression to get a real zero, and recording that real command and output in a `Verified by:` note.

**D-3 -> Resolved 2026-11-26.** Verified by: in WSL Debian, `sed -i 's/\r$//' docs/ethics/*.md`, then the corrected detector `grep -rlU $'\r' docs/ethics/*.md | wc -l` -> `0` with `grep_exit_status=1` (no matches). The byte-count check `tr -cd '\r' < "$f" | wc -c` returned `CR=0` for all ten files in `docs/ethics/*.md`. The earlier `CR_COUNT=0` stays recorded as a false green; the rule that came out of it is L10 in `docs/ethics/lessons.md`.

    D-4 | 2026-11-26 | J21 reported `rustfmt --edition 2021 --check -> ALL_OWNED_RS_FMT_OK` and J22 reviewed `PASS` with 0 findings | `cargo test --workspace` after merge gave 12 E0425 errors: identifiers lost their final `r` (`adapter`->`adapte`, `declarator`->`declarato`, `inner`->`inne`, `member`->`membe`) because a CR-normalization command treated `\r$` as a literal trailing `r` | Resolved | J21 + J22 + orchestrator

D-4 closure requires: restore every truncated identifier, verify zero CR bytes with `tr -cd '\r' < file | wc -c`, re-run the real compile gate, and re-review with the semantic-sanity checklist item added by L11. The false PASS stays in `.reports/reviews/J21.md` as evidence.

**D-4 -> Resolved 2026-11-26.** Verified by: re-merged J21's 13 files directly from the worktree with `cp` only (no sed); `tr -cd '\r' < file | wc -c` -> 0 for all 13 (`REMERGE_OK`); `cargo test --workspace` -> `ADAPTER_GATE_STATUS=0` with 101 tests passing (parser 44); `.reports/reviews/J21.md` appended `## Re-review 2026-11-26` ending `Verdict (re-review): PASS` with 0 truncated-identifier hits and byte-identical worktree/main files (SHA256 13/13). J21 demonstrated that `rustfmt --check` passed on the corrupted copy while `cargo test` failed — recorded as L11. The original false PASS was not deleted.

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
