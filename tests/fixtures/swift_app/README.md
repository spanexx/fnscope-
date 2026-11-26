# Fixture — `swift_app`

A small, standalone Swift source used to exercise the Swift adapter. It uses
only `Foundation`, so it needs no package manifest or third-party dependency.

## Files

- `main.swift` — five discoverable callables, one struct, one type index entry.

## Expected discovery facts

Once the registration job wires the adapter into the registry and adds
`**/*.swift` to the default scan includes, a scan of this directory should find
five callables, sorted by `(line, qualified_name)`:

| Qualified name | Kind | Parameters | Return type | Class |
|---|---|---|---|---|
| `Grade.describe` | Method | — | `String` | `Grade` |
| `grade` | Declaration | `text: String`, `minimum: Int` (default `1`) | `Grade` | — |
| `main` | Declaration | — | `void` | — |
| `normalize` | Declaration | `text: String` (external label `_`) | `String` | — |
| `report` | Declaration | `result: Grade` (external label `_`) | `void` | — |

`Grade` is indexed as a struct with two stored fields, `score: Int` and
`role: String`. Its `grade` return type resolves to those two fields. Leading
`///` comments become `docDescription` values; `tags` is empty. Initializers are
discovered as `TypeName.init` methods when present.

## Expected body-analysis facts

- `normalize` has one `Call` step (`trimmingCharacters`) and one `Return` step.
  It is pure (`150` ms, `universal`).
- `grade` has steps `Call`, `Branch`, `Branch`, `Call`, `Return`. Its guard
  throw yields error `EMPTY_TEXT` (`400`, `when` `!clean.isEmpty`) and its `if`
  throw yields `tooShort` (`500`, `when` `clean.count < minimum`). `print`
  makes it `SideEffect` with `console:out`, `1000` ms and runtime `node20`.
- `Grade.describe` is pure and has one `Call` step and one `Return` step.
- `report` calls `describe` and is `SideEffect` with `console:out`.
- `main` calls `grade` inside a `try?` expression and then calls `report` as a
  standalone statement. That `report` step is `StepKind::Call` with raw callee
  `report`, which core resolves into the fixture's `main` → `report` edge.
  `main` itself has no effect marker and is pure.

Calls inside `Transform` or `Return` steps still contribute raw names to
`calleeNames`; core resolves edges from `Call` steps only.

## Runtime behaviour

`swift main.swift` prints:

```text
graded example
example: 8
```

The `Grade.describe` and `report` functions are exercised through `main`; `grade`
exists to supply deterministic guard, error and call-edge facts for the scanner.
