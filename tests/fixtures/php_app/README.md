# fnscope PHP fixture app

A small, coherent PHP package used as the PHP adapter fixture. It exercises
free functions, a class method, a downstream call edge, a guarded `EMPTY_TEXT`
throw, an `echo`/`printf` side effect and a class type-index entry.

Scan it with:

```text
fnscope scan ./tests/fixtures/php_app
```

The package has no dependencies and uses PHP 8 syntax only. It is intentionally
small so an assertion over the full function set stays readable. The adapter
itself does not filter paths; file discovery and excludes belong to core.

## Expected scanner output

A scan with the PHP adapter registered should yield **at least 5 functions**
from `src/*.php`:

| Function | Qualified name | Module | Purity | Key evidence |
| --- | --- | --- | --- | --- |
| `normalize_text` | `normalize_text` | `src/Text.php` | pure | docblock description, `trim()` / `strtolower()` calls, no effects |
| `classify` | `classify` | `src/Text.php` | pure | calls `normalize_text`, `EMPTY_TEXT` guard, branch |
| `report` | `report` | `src/Text.php` | side-effect | `echo` and `printf` → `console:out` |
| `add` | `add` | `src/Metrics.php` | pure | optional `$b = 0`, helper called by `Calculator.scale` |
| `Calculator.scale` | `Calculator.scale` | `src/Metrics.php` | pure | class method, call to `add` |

`Calculator.scale` has a **qualified name with a dot**, proving PHP discovery
records class methods as `ClassName.method` and sets `class_name`. Its
`callee_names` contain `add`; equivalently `add` gains a `calledBy` entry
pointing back to `Calculator.scale` when core builds the graph.

### `classify` is the fallible hub

- The `// Normalize before the guard.` line above its assignment becomes that
  step's title.
- It calls `normalize_text` before the guard, so the helper gains a reverse
  call edge.
- `if ($cleaned === '')` produces a `Branch` step whose output is
  `throws EMPTY_TEXT | continues`.
- `throw new FunctionError("EMPTY_TEXT")` produces an `ErrorSpec` with code
  `EMPTY_TEXT`, status `400` and `when` = `$cleaned === ''`.

### `report` is a side effect

- Its `purity` is `side-effect`.
- `permissions` include `console:out`.
- Its timeout budget is `1000 ms` with runtime `node20`.

### Type index

`Calculator` is indexed with fields `factor: int` (a `property_declaration`)
and `SCALE: 3` (a `const_declaration` / `const_element`, the grammar's form of
a class constant). A function declaring `: Calculator` resolves those fields
through the shared `crate::resolve::resolve_fields`.

## Source-check notes

The files are valid PHP 8 and can be syntax-checked with
`php -l src/Text.php` and `php -l src/Metrics.php` where PHP is available; that
is syntax-only evidence, not a Rust gate. No cargo command belongs here.
