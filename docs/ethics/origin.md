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

## The origin gate

Code arrives by one of two delivery paths: the feature pipeline (GRILL,
PRD-TRD, IMPL, implement, gates) or `surgical-change`, where the design is
already locked and the change closes a gap in the smallest surface. Either way
the origin pack is written before the code, not reconstructed afterwards.

Under the goal skill's delegated authority the agent may make decisions the
user would otherwise make. Delegation is not the absence of a record: each such
decision is written **at decision time** with its WHY and SOURCE, and it must
align with the goal contract and with earlier decisions already locked in the
pack. A decision written later from evidence is labelled a reconstruction; it
is never presented as a confirmation from the user.

`scripts/check-culture.sh` enforces this mechanically, not by trust.
`ORIGIN_GATE` scans feature code under `crates/` and `ui/src/` (not `scripts/`,
`docs/` or `tests/`) and resolves each changed path relative to `main` to a
feature pack holding the three origin documents. The six historical base
crates and `ui/src` map to `fnscope-base` through an explicit allow-list; the
base pack keeps its legacy `GRILL.txt`/`PRD-TRD.md`/`IMPL.md` names, while new
packs use `GRILL-<slug>.txt`/`PRD-TRD-<slug>.md`/`IMPL-<slug>.md`. Both are
required by their real filenames, never by an existence-only waiver.
`DECISION_RECORD` checks every `GRILL*.txt` pack file, legacy name included,
for `Status: Locked` and a WHY/SOURCE-backed decision. A missing pack or record
fails the culture gate, so an unexplained commit cannot become a green build.

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
