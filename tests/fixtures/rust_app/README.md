# fnscope Rust fixture app

A small, coherent Rust crate used as fnscope's end-to-end Rust fixture.
It is the evidence that a real scan of `.rs` files produces a meaningful,
bidirectional function graph from Rust source: free functions, an impl method
with a downstream call edge, a guarded fallible function, and a side effect.

Scan it with:

```text
fnscope scan ./tests/fixtures/rust_app/src
```

or scan the whole crate (the `**/target/**` exclude still applies):

```text
fnscope scan ./tests/fixtures/rust_app
```

The crate has no dependencies and uses edition 2021. It is intentionally small
so an assertion over the full function set stays readable.

## Expected scanner output

A default scan should yield **at least 7 functions**:

| Function | Qualified name | Module | Purity | Key evidence |
| --- | --- | --- | --- | --- |
| `add` | `add` | `lib` | pure | no calls, no I/O |
| `Calculator.new` | `Calculator.new` | `lib` | pure | associated constructor |
| `Calculator.scale` | `Calculator.scale` | `lib` | pure | impl method, downstream call to `add` |
| `normalize_text` | `normalize_text` | `lib` | pure | small helper called by `classify` |
| `classify` | `classify` | `lib` | pure | `EMPTY_TEXT` guard, helper call, `match` |
| `report` | `report` | `lib` | side-effect | `println!` -> `console:out` |
| `passthrough` | `passthrough` | `skip` | pure | ordinary `src/skip.rs` file |

`Calculator.scale` has a **qualified name with a dot**, proving the Rust
discovery pass records impl methods as `Type.method` rather than a bare name.
Its steps include a `call` step whose `callsFnId` resolves to `add`, proving the
1-hop downstream edge out of an impl method.

### `classify` is the fallible hub

- An early guard rejects empty input:
  `if text.is_empty() { return Err(AppError { code: "EMPTY_TEXT" }) }`.
  The resulting `errors` entry carries the code `EMPTY_TEXT`.
- Its body calls the helper `normalize_text` before matching, so its steps
  include a downstream `call` edge to `normalize_text`; equivalently the helper
  has a `calledBy` entry pointing back to `classify`.
- A `match` expression on `normalized.ends_with("er")` produces a `Branch` step
  and selects `Label::Doer` or `Label::Receiver`.
- The `Result<Label, AppError>` return type documents both the success and the
  failure shape.

### `report` is a side effect

- Its `purity` is `side-effect`.
- `permissions` include `console:out` for the `println!` call.

### File filtering

- `src/skip.rs` is **included by default**. Its name is a trap: excludes are
  glob-based, so nothing about "skip" drops it. If `passthrough` is missing,
  the default include set for `.rs` files is broken.
- `target/generated.rs` is **excluded by the default `**/target/**` glob**. It
  defines `generated_should_be_excluded`; that function must **not** appear in
  a default scan. Its presence on disk is what proves the core exclude works.
  A scan with an explicit include for that path should still be droppable via
  `--exclude '**/target/**'`.

## Source-check notes

The crate is valid Rust and can be checked from its own directory with
`cargo check`; the orchestrator runs that gate, so agents must not run cargo
here. `rustfmt --edition 2021 --check src/lib.rs src/skip.rs target/generated.rs`
is safe to run locally and should report no diff.

There is no runtime invoke contract for this fixture (unlike the Node fixture);
it exists to exercise discovery, body analysis, id generation, edge resolution
and default excludes on Rust source.
