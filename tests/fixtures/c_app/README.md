# Fixture — `c_app`

A small, standalone C program used to exercise the C adapter. It uses only the
C standard library, so `cc main.c -o c_app` needs no third-party headers. The
fixture exists to supply deterministic discovery, body and effect facts; it does
not have to be compiled for the scanner to read it.

## Files

- `main.c` — four discoverable functions and one struct type.

## Expected discovery facts

Once the registration job wires the adapter into the registry and adds
`**/*.c` to the default scan includes, a scan of this directory should find
four callables, all `FnKind::Declaration`, sorted by `(line, qualified_name)`:

| Qualified name | Parameters | Return type | Notes |
|---|---|---|---|
| `add` | `a: int`, `b: int` | `int` | pure arithmetic |
| `normalize` | `text: const char *` | `const char *` | guard error, console side effect |
| `report` | `label: const char *` | `void` | console side effect |
| `main` | — | `int` | call edge to `report` |

`Point` is indexed as a struct type with fields `x: int` and `y: int`. Leading
`/* */`, `/** */` and `//` comments become `docDescription` values; no `@tags`
are present, so `tags` is empty. Header declarations are deliberately not
discovered: the adapter claims only `.c`.

## Expected body-analysis facts

- `add` has a single `Return` step and is `pure` with timeout `150` and runtime
  `universal`.
- `normalize` has a `Branch` step then a `Return` step. The guard condition is
  `text == NULL`; it reports two errors, `EMPTY_TEXT` (from
  `fprintf(stderr, "EMPTY_TEXT")`) and `NULL` (from `return NULL` inside the
  guard), both with status `400` and `when` referring to `text == NULL`. The
  branch step's output is `throws EMPTY_TEXT | continues`. The `fprintf` call
  makes the function `side-effect` with permission `console:out`, timeout `1000`
  and runtime `node20`.
- `report` calls `printf`, so it is `side-effect` with `console:out`, timeout
  `1000` and runtime `node20`.
- `main` has steps `Call`, `Call`, `Call`, `Return`: a declaration with the
  `normalize` initializer is a `Call` step, `assert(value != NULL)` is a `Call`
  step carrying an `ASSERT` error with status `400` and `when` `assertion
  failed`, and `report(value)` is the standalone `Call` step that gives core the
  `main` → `report` edge. The final `return 0` is the `Return` step. `main` has
  no effect marker of its own and is therefore `pure`.

## Runtime behaviour

`cc main.c -o c_app && ./c_app` prints:

```text
hello
```

The `add` and `normalize` functions are not invoked by `main` with effectful
arguments; they exist to supply deterministic arithmetic, guard and error facts
for the scanner.
