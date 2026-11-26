# Enrichment end-to-end proof

This directory holds the harness that proves `fnscope scan --enrich` really uses a
content-addressed cache and makes **zero API calls** on an unchanged second scan.
Unlike the crate unit tests, it exercises the real CLI, a real HTTP backend and the
on-disk cache together.

## What it proves

`scripts/enrich-e2e.sh` copies the sample fixture into a private `/tmp` workspace,
starts `tests/e2e/mock-llm.mjs` (an OpenAI-compatible HTTP server that counts
requests), and runs four scans against a fresh cache:

1. **First scan** — every function is a cache miss.
   - `fn-graph.json` parses and has functions; `enrich-cache.json` parses and has
     exactly one entry per function.
   - The mock's `GET /count` equals the number of functions in the graph.
   - Every function has a mock-generated description and every step has a
     mock-generated title and detail.
   - CLI stdout reports `enrich: 0 cache hits, N misses, N API calls`.
2. **Second, unchanged scan** — all cache hits.
   - The mock call count is unchanged (zero extra requests).
   - `enrich-cache.json` is byte-for-byte identical (no rewrite on an all-hit run).
   - CLI stdout reports `enrich: N cache hits, 0 misses, 0 API calls`.
3. **Formatting-only edit** (extra blank lines and spaces inside a function body).
   - The mock call count is still unchanged and the cache bytes are still identical,
     proving `astHash` ignores formatting.
4. **Semantic edit** (a string literal changed from `EMPTY_SENTENCE` to
   `EMPTY_SENTENCE_V2`).
   - The mock call count increases by at least one and the cache file changes,
     proving a new hash causes a new API call.

The script prints only `ENRICH_E2E_OK` after all assertions pass and cleanup runs,
or `ENRICH_E2E_FAIL: <reason>` with the mock log tail on failure.

## Files

- `mock-llm.mjs` — Node HTTP server (core modules only). `argv[2]` is the port
  (`0` means OS-assigned), `argv[3]` the counter file, `argv[4]` the request log,
  and `argv[5]` an optional file where the bound port is published. It accepts
  POST on any path, increments the counter, and answers with an OpenAI-style
  envelope whose `choices[0].message.content` is fenced JSON derived from the
  prompt. `GET /count` returns the integer counter.
- `README.md` — this file.

## Running from Windows via WSL

Rust, `curl` and `node` are expected inside WSL Debian. The release binary is
built automatically if `target/release/fnscope` is missing, though the usual gate
pre-builds it.

Replace `<wsl-repo-path>` below with the repository's path as seen from inside
WSL (find it with `wsl.exe -d Debian --exec bash -lc 'pwd'` after `cd`ing to the
repo):

```powershell
wsl.exe -d Debian --exec bash -lc 'cd <wsl-repo-path> && bash scripts/enrich-e2e.sh'
```

No real API key is used: the script sets a fake `OPENAI_API_KEY` and points
`FNSCOPE_LLM_BASE_URL` at `http://127.0.0.1:<port>/v1/chat/completions`. `ureq`
follows `FNSCOPE_LLM_BASE_URL` directly, so a plain HTTP localhost URL is
intentional for this mock; `NO_PROXY` is set so a system proxy cannot intercept
the loopback request. The mock is started detached with a tracked PID and is
stopped by that PID (children first) via an `EXIT` trap — no `pkill -f`, `nohup`
or `setsid`. All temp files live under `/tmp/fnscope-enrich-$$` and are removed
on exit, including any `.fnscope` directory this script would have generated.
