# fnscope — Cleanup Ethics

## The rule

Every job ends with the bench as clean as you found it. Anything you start or
create for a check is your debt: a server, a watcher, a Node runner, a log, a
scratch script, a cache, a temp file, a failed download. Pay it before you
report done.

## Before you start

- Put logs and scratch scripts outside the repo, under the system temp
  folder — never in the repo.
- Write down what you started: name, port, PID, and the files you will remove.
- Prefer a command whose output you can poll; bound every wait.

## Before you finish

### Processes

- Stop every server, Node runner and watcher you started. Stop a WSL server
  through its host-side PID; kill the listener if the launcher is gone.
- Confirm the port is closed (for example `ss -ltnp 'sport = :4310'` in WSL)
  and with a Windows connection check. A closed tab is not a stopped server;
  a port with no listener is the proof.

### Files

- Delete logs, scratch scripts, temp runner scripts, coverage files and
  caches you created for a check. They are rebuildable, not evidence.
- Keep only files cited as evidence by a job that is still open. Once the job
  closes, remove the raw scratch files too; the report keeps the record.
- Do not hide junk behind `.gitignore`. Ignored is not clean.

### What you never delete without permission

- Source, tests, fixtures, docs, the contract, reports, the drift log, or
  decision docs.
- Any credential or `.env` file.
- A worktree or branch that is not yours to remove. A merged feature worktree
  is cleanup debt; leave an unmerged one alone unless asked.

## Verify

- `git status --short` is clean except the intended edits of your job.
- No server, Node runner or watcher you started remains.
- The ports you used are closed.

## If you cannot clean up

Say what is still running, where it lives, and why. An honest leftover is
safer than a silent one.
