# Fixture — `kotlin_app`

A minimal Kotlin application used to exercise the J35 adapter with real
`tree-sitter-kotlin` 0.3.8 parse trees. The fixture is data only: registering
the adapter and scanning this directory are integration work owned by the
orchestrator.

## Input shape

`src/Main.kt` declares:

- a `data class Result` with a primary constructor (`Result.<init>`),
- a `class Greeter` with a primary constructor, a companion-object factory
  (`Greeter.of`) and a member function (`Greeter.greet`),
- a top-level `fun echo(text: String = "world")` with a doc comment and a default,
- a top-level `fun evaluate(input: Int)` with a call edge, a guard, a throw and a
  `println` side effect.

## Expected adapter output

Discovery returns six callables, sorted by `(line, qualified_name)`:

| qualified name | kind | class | parameters |
|---|---|---|---|
| `Result.<init>` | method | `Result` | `code: Int`, `message: String` |
| `Greeter.<init>` | method | `Greeter` | `prefix: String` (default `"Hello"`) |
| `Greeter.of` | method | `Greeter` | `prefix: String` (default `"Hi"`) |
| `Greeter.greet` | method | `Greeter` | `name: String` |
| `echo` | declaration | — | `text: String` (default `"world"`) |
| `evaluate` | declaration | — | `input: Int` |

`collect_types` indexes `Result` with fields `code: Int`, `message: String` and
`Greeter` with `prefix: String`. Because the return type is declared in the same
file, `evaluate.returns.fields` is already populated at discovery time and
`resolve_return_fields` is a no-op; a cross-file test in
`crates/fnscope-parser/src/kotlin/mod.rs` covers the project-wide index path.

`analyze(evaluate)` produces, in source order:

1. step-1 `branch` — `require(input >= 0) { "INVALID_INPUT" }`, output
   `throws INVALID_INPUT | continues`;
2. step-2 `branch` — `if (input == 0) { throw ... }`;
3. step-3 `call` — `val greeter = Greeter.of("Hello")`, callee `of`;
4. step-4 `call` — `val message = greeter.greet(echo("Kotlin"))`,
   callees `greet` and `echo`;
5. step-5 `call` — `println(message)`;
6. step-6 `return` — `return Result(input, message)`, output `Result`.

`errors` contains two de-duplicated specs: `INVALID_INPUT` (status 422, `when`
from the guard) and `EMPTY_SENTENCE` (status 400, `when` = `input == 0`).

`purity` is `side-effect`, `permissions` is `["console:out"]`, `runtime` is
`node20` and `timeout_ms` is `1000`. `echo` and `greet` remain `pure` with
`runtime` `universal` and `timeout_ms` `150`.
