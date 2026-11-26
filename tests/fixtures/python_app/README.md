# fnscope Python fixture app

A small, coherent Python package used as fnscope's Python adapter fixture.
It exercises discovery of free functions and a class method, a downstream call
edge, a guarded `EMPTY_TEXT` raise, a side effect and a module-level type alias.

Scan it with:

```text
fnscope scan ./tests/fixtures/python_app
```

The package has no dependencies and uses Python 3 syntax only. It is
intentionally small so an assertion over the full function set stays readable.

## Expected scanner output

A default scan should yield **at least 6 functions** from `app/*.py`:

| Function | Qualified name | Module | Purity | Key evidence |
| --- | --- | --- | --- | --- |
| `normalize_text` | `normalize_text` | `app/text` | pure | docstring description, no effects |
| `classify` | `classify` | `app/text` | pure | calls `normalize_text`, `EMPTY_TEXT` guard, branch |
| `report` | `report` | `app/text` | side-effect | `print()` → `console:out` |
| `add` | `add` | `app/metrics` | pure | small helper called by `Calculator.scale` |
| `Calculator.__init__` | `Calculator.__init__` | `app/metrics` | pure | constructor with a `self.factor` assignment |
| `Calculator.scale` | `Calculator.scale` | `app/metrics` | pure | class method, downstream call to `add` |

`Calculator.scale` has a **qualified name with a dot**, proving Python
discovery records class methods as `ClassName.method` and sets `class_name`.
Its steps include a call to `add`; equivalently `add` has a `calledBy` entry
pointing back to `Calculator.scale`.

### `classify` is the fallible hub

- A leading `#` comment above its assignment becomes that step's title.
- It calls `normalize_text` before the guard, so its `callee_names` contain
  `normalize_text` and the helper gains a reverse call edge.
- `if not cleaned:` produces a `Branch` step whose output is
  `throws EMPTY_TEXT | continues`.
- `raise ValueError("EMPTY_TEXT")` produces an `ErrorSpec` with code
  `EMPTY_TEXT`, status `400` and `when` = `not cleaned`.

### `report` is a side effect

- Its `purity` is `side-effect`.
- `permissions` include `console:out` for the `print()` call.
- Its timeout budget is `1000 ms` with runtime `node20`.

### Type index

`UserId = int` and `Alias = dict` in `app/text.py` are module-level type
aliases. `collect_types` indexes them under those names. `MAX_RETRIES = 3` is a
literal constant, not an alias, and is not indexed.

### File filtering

- `__pycache__/generated.py` defines `generated_should_be_excluded`. It must
  **not** appear in a default scan because core's default excludes drop
  `__pycache__/` directories. Exclusion is core's job: this adapter only
  documents the expectation and does not implement any glob filtering. Its
  presence on disk is what proves the default exclude works.

## Source-check notes

The files are valid Python 3 and can be syntax-checked with
`python3 -m py_compile app/text.py app/metrics.py app/__init__.py`; agents report
that as syntax-only evidence, not a Rust gate. No cargo command belongs here.
