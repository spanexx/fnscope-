# fnscope — Origin Ethics

Every piece of code must account for why it is there. There are exactly two
ways in. There is no third.

## The two ways in

- A delegated job J01..J11 in `.reports/goal-build-fnscope.md` — new work. It
  runs GRILL, PRD-TRD, IMPL, implement, gate; its files are named in
  `docs/features/fnscope-base/IMPL.md`.
- A drift entry D-N in `docs/drift.md` — closing a gap between the locked
  contract or spec and the code. The entry must exist **before** the fix
  begins.

## Pick one before writing a line

- New capability? It is a job.
- Closing a gap against something already decided? It is a drift-driven fix.
- Cannot tell? It is a job. A fix needs a locked decision behind it.

## The trail

- The owner job or D-N goes in the code comment (`commenting.md`) and in the
  agent's report. Code with no trail is code nobody can justify: raise it as
  a finding.

## DO NOT

- Do not just make a change. There is no in-between and no informal pass.
- Do not add code with no decision behind it: no drive-by improvements, no
  opportunistic refactors, no "while I was in there".
- Do not call it a fix when no decision exists.
- Do not skip the process because the change is small. Small means surgical,
  not nothing.
- Do not edit a file your job does not own: `AGENTS.md`, `docs/contract/`,
  `.reports/`, another crate, `ui/`, `scripts/`, or tests. Report the need to
  the orchestrator instead.
