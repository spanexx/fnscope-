# fnscope Ruby fixture app

A small, coherent Ruby file used as fnscope's Ruby adapter fixture. It exercises
discovery of module-level methods, a singleton method and a class instance
method, a downstream call edge, a guarded `EMPTY_TEXT` raise, a `puts` side
effect and a `Struct` type-index entry.

Scan it with:

```text
fnscope scan ./tests/fixtures/ruby_app
```

The file uses only Ruby 3 core methods and has no dependencies. It is kept
small so an assertion over the full function set stays readable.

## Expected scanner output

A default scan should yield **at least 5 functions** from `main.rb`:

| Function | Qualified name | Kind | Purity | Key evidence |
| --- | --- | --- | --- | --- |
| `normalize_text` | `normalize_text` | declaration | pure | calls `strip_controls`, `EMPTY_TEXT` guard, branch |
| `strip_controls` | `strip_controls` | declaration | pure | single call expression, `@param`/`@return` YARD types |
| `report` | `report` | declaration | side-effect | `puts` → `console:out` |
| `Formatter#format` | `Formatter.format` | method | pure | class instance method, `class_name = "Formatter"` |
| `default_formatter` | `default_formatter` | declaration | pure | module-level `def self.` singleton method |

`Formatter.format` has a **qualified name with a dot**, proving Ruby discovery
records class instance methods as `ClassName.method` and sets `class_name`.

## Step and error evidence

- `normalize_text` has three steps in source order:
  1. `Call` — `cleaned = strip_controls(text)`, with `calls_fn_id` resolving to
     `strip_controls` and `callee_names` containing `strip_controls`.
  2. `Branch` — `if cleaned.empty?`, output `throws EMPTY_TEXT | continues`.
  3. `Return` — `cleaned`, output `String` from the `@return [String]` tag.
- `raise ArgumentError, "EMPTY_TEXT"` produces one `ErrorSpec` with code
  `EMPTY_TEXT`, status `400` and `when` = `cleaned.empty?`.
- `attr_reader :prefix` is **not** a discovered function.
- `report` has `purity = side-effect`, permission `console:out`, timeout
  `1000 ms` and runtime `node20`.

## Type index

`Record = Struct.new(:id, :name)` is indexed under `Record` with `type_ =
"Struct"` and two fields, `id` and `name`. The `class Formatter` declaration is
indexed under `Formatter`; its `attr_reader :prefix` contributes the `prefix`
field.

## Source-check notes

The Ruby file is syntax-valid Ruby 3 and can be checked with
`ruby -c main.rb` where Ruby is installed. That is syntax-only evidence, not a
Rust gate. No cargo command belongs here; the orchestrator runs all Rust gates.
