# Workflow — Cross-environment runne

## Why this exists

Two iterations of `.reports/goal-build-fnscope.md` record WSL-to-Windows Node
path failures, and a third resolved them.

- Iteration 6: "Server 7/8 pass; real-invoke test returns `data: null`." Exact verification: "`node_invocation_runs_a_real_export` -> `left: None, right: Some(42)`." Ledger failure class: "WSL translates Linux paths in argv for Windows executables, but the generated runner embedded `file:///tmp/...` inside a file, which Windows Node cannot resolve." Pivot: "J06 to pass the module path as argv[3] instead of embedding it".
- Iteration 7 added a diagnostic assertion and captured the real error:
  `node '/mnt/c/Program Files/nodejs/node.exe' exited with exit status: 1 ... Cannot find module 'C:\tmp\fnscope-invoke-...\runner.mjs'`. Ledger failure class: "WSL path translation maps `/tmp/...` to `C:\tmp\...`, where Rust's WSL-created scratch files do not exist." Pivot: create the scratch dir beside the manifest and place the test module under the repo's `target/` dir.
- Iteration 8: native Node 24 installed in WSL -> `CARGO_TEST_STATUS=0`; 66 tests pass including real Node invocation.

A `/mnt/c/...` -> `C:\mnt\c\...` mis-mapping is named in the brief but is not
quoted in the ledger iterations I read; it is flagged in
`docs/ethics/lessons.md`. The checklist below covers it without claiming it as
a quoted incident.

**Rule.** A process launched from one OS into another must use a path both
sides can reach. Pass paths as argv, never embedded in a generated file.
Prefer a native runtime in the lane that owns the files. Diagnose from the
real error text.

## Trigge

Use this checklist whenever any of these is true:

- A WSL process launches a Windows executable, or the reverse.
- A generated script, runner file or manifest is written by one OS and read by
  another.
- A test depends on a runtime that exists in only one lane.

## Checklist

1. **Name both sides.** Write down which OS creates each file and which OS
   reads it. Include the runtime path and version, for example native Node 24
   in WSL versus `/mnt/c/Program Files/nodejs/node.exe`.
2. **Prove the path is reachable from the reader.** Check that the exact path
   exists from the reader's side before launching. A path created unde
   `/tmp/...` in WSL is not the same file as `C:\tmp\...` on Windows. Prefe
   placing scratch files beside the manifest or under `target/` so both sides
   address the same tree.
3. **Use argv for paths.** Pass the module path and arguments as argv entries
   (as J06 did with argv[2] for args and argv[3] for the module). Do not embed
   a `file:///tmp/...` URL inside a generated file.
4. **Prefer a native runtime.** If the files live in WSL, run Node in WSL.
   Iteration 8 ended the path-translation failures this way. If the runtime is
   only on Windows, keep every path on a `/mnt/c/...` tree and confirm the
   mapping.
5. **Bound the wait.** Start the process detached, record its PID, poll with a
   limit, and never wait forever.
6. **Capture stderr and the exit code.** Print the real error text. The
   iteration 7 error named the exact module path that could not be found; that
   is what made the diagnosis possible.
7. **Clean up.** Stop processes by tracked PID, free ports, remove scratch
   directories, and confirm the listener is gone. See `docs/ethics/cleanup.md`.
8. **Record the command and output.** Copy them into the ledger with the lane
   and the runtime version.

## Checkable exit criteria

- Every cross-OS path is listed with both its writer-side and reader-side
  spelling.
- No generated file contains an absolute path from the other OS.
- Paths that must cross are passed as argv.
- The process ran with a bounded wait and a tracked PID.
- stderr and the exit code are in the report, verbatim.
- The scratch directory and process are gone after the check.

## DO NOT

- Do not guess the cause from `data: null`. Add a diagnostic assertion and
  print the response, as iteration 7 did.
- Do not use `pkill -f` to clean up; use the tracked PID.
- Do not hardcode a drive letter, user directory or absolute path in committed
  code or scripts (AGENTS.md).
- Do not call a simulated trace a real invocation. The UI audit distinguishes
  the initial `{"simulated": true}` placeholder from a post-Run response.
