# Fixture — `go_app`

A small, standalone Go module used to exercise the Go adapter. It has no
third-party imports, so `go vet ./...` and `gofmt -l .` need only the standard
toolchain.

## Files

- `go.mod` — module `example.com/goapp`, Go 1.22.
- `main.go` — six discoverable functions and one struct type.

## Expected discovery facts

Once the registration job wires the adapter into the registry and adds
`**/*.go` to the default scan includes, a scan of this directory should find
six callables, sorted by `(line, qualified_name)`:

| Qualified name | Kind | Parameters | Return type | Class |
|---|---|---|---|---|
| `normalize` | Declaration | `text: string` | `string` | — |
| `analyze` | Declaration | `text: string` | `(string, error)` | — |
| `load` | Declaration | `text: string` | `(string, error)` | — |
| `Service.Apply` | Method | `text: string` | `string` | `Service` |
| `report` | Declaration | `text: string` | `void` | — |
| `main` | Declaration | — | `void` | — |

`Service` is indexed as a struct type with one field, `Prefix: string`.
Leading `//` comments become `docDescription` values; no `@tags` are present,
so `tags` is empty.

## Expected body-analysis facts

- `normalize` has a `Branch` step with condition `text == ""` and one error:
  code `EMPTY_TEXT`, status `400`, `when` `text == ""`, from `panic("EMPTY_TEXT")`.
- `analyze` has steps `Branch`, `Transform`, `Call`, `Return`. Its guard uses
  `errors.New("EMPTY_TEXT")`, so the same code appears once (deduplicated) with
  status `400`. The `fmt.Println` call makes the function `SideEffect` with
  permission `console:out`, timeout `1000` and runtime `node20`.
- `load` calls `analyze` in a `Transform` step and has a `return "", err`
  pattern, reported as code `ERR` with `when` `propagated error`.
- `Service.Apply` calls `normalize` in a `Return` step and is pure (timeout
  `150`, runtime `universal`).
- `report` calls `fmt.Println`, so it is `SideEffect` with `console:out`,
  timeout `1000` and runtime `node20`.
- `main` assigns from `Apply` and calls `report(output)` as a standalone call
  statement. That step is `StepKind::Call` with the raw callee `report`, which
  core resolves into the `main` → `report` edge — the fixture's call edge.
  `main` itself has no effect marker and is pure.

Calls that appear only inside `Transform` or `Return` steps still contribute
raw names to `calleeNames`, but core resolves edges from `Call` steps only.

## Runtime behaviour

`go run .` prints:

```text
>hello
```

The `analyze` and `load` functions are not invoked by `main`; they exist to
supply deterministic guard, error and call-edge facts for the scanner.
