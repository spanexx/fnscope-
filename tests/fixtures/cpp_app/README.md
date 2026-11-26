# Fixture — `cpp_app`

A small standalone C++ file used to exercise the C++ adapter. It uses only the
standard library (`<cstdio>`, `<iostream>`, `<stdexcept>`), so it needs no
third-party headers. Once the registration job adds `tree-sitter-cpp`, the
`pub mod cpp;` line, the registry entry and `**/*.cpp`/`**/*.cc` scan includes,
a scan of this directory produces the facts below.

## Files

- `main.cpp` — seven discoverable callables, one class type and no external deps.

## Expected discovery facts

Seven callables, sorted by `(line, qualified_name)`:

| Qualified name | Kind | Parameters | Return type | Class |
|---|---|---|---|---|
| `add` | Declaration | `a: int`, `b: int` | `int` | — |
| `normalize` | Declaration | `n: int` | `int` | — |
| `Calculator.Calculator` | Method | `factor: int` | `void` | `Calculator` |
| `Calculator.scale` | Method | `n: int` | `int` | `Calculator` |
| `Calculator.Calculator` | Method | `factor: int` | `void` | `Calculator` |
| `guard` | Declaration | `n: int` | `int` | — |
| `report` | Declaration | `n: int` | `void` | — |

The two `Calculator.Calculator` rows are the in-class declaration (line 20) and
the out-of-class definition (line 27). `Calculator` is indexed as a type with
one field, `factor_: int`. Leading `//` comments become `docDescription`
values; no `@tags` are present, so `tags` is empty.

## Expected body-analysis facts

- `add` has a `Transform` step (`int total = a + b;`) then a `Return` step
  (`int`). It is pure: timeout `150`, runtime `universal`.
- `normalize` has a `Call` step whose code starts with
  `int shifted = add(n, 1);`, then a `Return` step. The raw callee `add` is the
  fixture's call edge: core resolves it into `normalize` → `add`.
- `Calculator.scale` has one `Return` step (`int`) and is pure.
- The `Calculator.Calculator` definition has an empty body, so it has no steps
  and stays pure.
- `guard` has a `Branch` step with output `throws NEGATIVE_INPUT | continues`
  and a `Return` step. Its one error is `code: NEGATIVE_INPUT`,
  `status: 400`, `when: n < 0`, from `throw std::runtime_error("NEGATIVE_INPUT")`.
- `report` has two `Call` steps: `std::cout << n` (callee `cout`) and
  `printf("done\n")` (callee `printf`). Both markers make it `SideEffect` with
  permission `console:out`, timeout `1000` and runtime `node20`.

Calls inside `Return` steps still contribute raw names to `calleeNames`, but core
resolves graph edges from `Call` steps only — hence `normalize` is the call edge.

## Runtime behaviour

This file is a scanner fixture, not an executable program: it has no `main`.
Its purpose is to supply deterministic discovery, guard/error and side-effect
facts to the adapter tests.
