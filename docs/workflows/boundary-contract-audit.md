# Workflow — Boundary contract audit

## Why this exists

Three failures came from assuming a contract was correct because the source
looked correct.

**Iteration 10, arg mismatch (L4).** The browser audit captured the UI's own
default invoke POST body `{"fnId":"evaluate-grammar-role","args":{"input":""}}`
and response `{"ok":false,"data":null,"error":"node '.../node': Cannot read properties of undefined (reading 'replace')",...}`. It also captured `sentence.replace is not a function` for `tokenize` and `value.toLowerCase is not a function` for `slugify`. The same iteration records "`cargo test --workspace` still 0 with 71+ tests" — the green unit suite did not catch it. Ledger failure class: "interface contract mismatch between manifest parameters, UI form and Node runner argv." Pivot: "J08 to fix UI metadata/breadcrumb/arg repair; J06 to dispatch Node args by manifest parameter names."

**Iteration 10, rendered UI (L5).** The audit recorded `has_schemaVersion:false`
in the `.json` view and `backButtons: 0` after `Jump ↑`. Iteration 13's re-audit
reported "6 PASS, 0 FAIL" on the fixes. Reading `fnscope_ui.tsx` would not have
proven either behavior.

**Iteration 13/14, empty trace (L8).** The re-audit's remaining observation:
"Node-mode responses carry `"trace":{}` (empty). Step trace badges in the
executionSteps list therefore do not refresh from a real run." The data was
correct; a field the UI renders was missing. Iteration 13 pivot: "J06 to merge
the deterministic trace into Node-mode responses so step badges refresh."

**Rule.** Audit the data contract at every boundary with captured payloads.
Assert the shape on both sides. Prove structural UI features in a real browser.
Do not accept "the source looks right".

## Trigge

Use this workflow when:

- A payload crosses manifest -> form -> POST -> runner -> export.
- A UI field is populated from an API response.
- A change adds or removes a response field.
- A navigation action changes or removes visible state.

## Checklist

1. **List every boundary.** Write the chain explicitly, for example:
   manifest `parameters` -> form field derivation -> `POST /api/invoke` body ->
   runner argv -> exported result. Name the producer and consumer at each hop.
2. **Capture actual payloads at each boundary.** Use a browser network capture
   or `curl.exe`, and start the capture before the first user action. The audit
   started network capture before any Run, which is why the default
   `{"input":""}` body was preserved as evidence.
3. **Assert shape on both sides.** For each hop, state the expected keys and
   types on the producer side and the consumer side. Compare them with the
   captured payload. Flag a scalar wrapped as an object, or an object passed
   where a flat field is expected.
4. **Add a unit test for the adapter.** The fix for iteration 10 was a runne
   that dispatches Node args by manifest parameter names; iteration 11 added a
   scalar-parameter runner test (and had to set `file_path: "shout.mjs"` so the
   runner did not target a directory).
5. **Add one real end-to-end check.** Use a browser or an HTTP capture, not a
   unit test. Click the control, run the default action, and check the result.
6. **Check fields the UI renders.** For every response field the UI reads,
   confirm it is populated in a real response. An empty `trace` is a gap even
   when the schema is valid.
7. **Exercise destructive navigation.** Click `Back` after both `inspect` and
   `Jump ↑`. Count buttons and snapshot breadcrumb text. A path that collapses
   state is a bug even if the other path passes.
8. **Re-run the original failing scenario after the fix.** Iteration 13
   re-audited the exact three bugs: `.json` metadata, `Jump ↑`/`Back`, and
   repaired default invoke args. Result: "6 PASS, 0 FAIL".
9. **Record commands, payloads and verdicts** in `.reports/ui-browser.md` o
   the ledger. Keep PASS, FAIL, PARTIAL and "remaining minor observations" as
   separate statements.

## Checkable exit criteria

- Every boundary in the chain has a named producer and consumer.
- At least one real payload per boundary is quoted in the report.
- An adapter unit test exists for the mapping that was fixed.
- One browser or HTTP end-to-end check ran against the real served manifest.
- Every UI-rendered response field is populated in that check.
- The original failing scenario has a second, passing audit entry.

## DO NOT

- Do not use the built-in sample data as proof; the audit verified the toolba
  buttons came from the real manifest.
- Do not treat a green unit suite as a boundary check.
- Do not promote a minor observation to a full pass, or hide it to keep the
  summary clean.
- Do not leave a server running after the check. Use a tracked PID and confirm
  the port is closed.
