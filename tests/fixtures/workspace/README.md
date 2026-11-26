# Two-scope workspace fixture

A minimal real workspace used by scope-isolation and invoke tests. It has two independent
projects, each with one entry point and one helper. Both helpers are deliberately named
`shared`, and both entry points call their own local `shared`.

```text
workspace/
  fnscope.workspace.json
  alpha/src/main.ts   # alphaEntry(x) -> shared(x) -> x * 2, then + 1
  beta/src/main.ts    # betaEntry(x)  -> shared(x) -> x * 10, then - 1
  README.md
```

## Why the same name appears twice

`shared` exists in both scopes so isolation is observable rather than assumed:

- Each scope is scanned with its own root, so each manifest contains exactly one `shared`
  and only its own entry point (`alphaEntry` or `betaEntry`).
- Both files live at `src/main.ts`, so a path-plus-name id is identical across scopes while
  the implementation differs. Serving or invoking alpha's `shared` must never return beta's
  source, trace, or result.
- A call to a beta-only function id through the alpha route must fail as unknown instead of
  searching the other scope.

## Configuration

`fnscope.workspace.json` is version 1 with `defaultScope: "alpha"` and two scopes. Every
`path` is relative to the config file (`alpha`, `beta`); absolute paths are not allowed.
Both scopes include `**/*.ts` and run in `simulate` invoke mode.

## Expected behavior

- `alpha` scan: `alphaEntry` (called-by entrypoint) calls `shared`; `shared` returns `x * 2`.
- `beta` scan: `betaEntry` (called-by entrypoint) calls `shared`; `shared` returns `x * 10`.
- Simulate-mode invoke is deterministic because the functions are pure arithmetic; no files,
  clock, network or environment are read.
- The TypeScript avoids enums, decorators, namespaces and parameter properties, so Node's
  type stripping can import these `.ts` files directly. There are no relative imports.

## Checking the fixture

```text
bash scripts/check-workspace-fixtures.sh
```

The script needs bash, `grep`, and the stdlib Node runtime; no server, CLI build or third-party
package. It checks that the sources and config exist, the JSON parses, scope ids are unique and
match `[a-z0-9][a-z0-9-]*`, paths are relative, both scopes declare their own entry plus `shared`,
and the default scope is `alpha`.
On success it prints `WORKSPACE_FIXTURE_OK`.
