# fnscope — Commenting Ethics

Commenting is not decoration. It is the handover between agents. A comment
that says nothing is a comment that loses the work.

## The rule for this Rust repo

- Every public item gets a one-line `///` doc comment that explains **why** it
  exists, not **what** the code already says. This is required by AGENTS.md
  and checked before a job is accepted.
- A module-level `//!` line says which responsibility the file owns.

## What a comment carries

- WHY the item exists.
- WHICH work it belongs to: the job id (J01..J11) or drift entry (D-N).
- WHO changed it and WHEN, so the next agent is not left guessing.

## Shape

    /// WHY: <why it exists>
    /// JOB: J04 | D-N: <drift id>
    /// 2026-11-25 <agent>: <what changed, and why>

## DO

- Plain English, short sentences. Explain a term the reader may not have.
- Write the comment as you write the code, not as an afterthought.
- Keep it current. A stale comment is worse than no comment.

## DO NOT

- Do not restate the code. `i += 1` needs no "increment i".
- Do not leave a public item undocumented, however small the helper.
- Do not comment out code. Delete it, or raise a drift entry.
- Do not edit without logging it in the job's report.
- Do not write jargon where plain English works.
