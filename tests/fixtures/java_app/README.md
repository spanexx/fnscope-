# Fixture — `java_app`

A small, standalone Java file used to exercise the Java adapter. It has no
third-party imports, so `javac Main.java` needs only the standard toolchain.

## Files

- `Main.java` — seven discoverable callables, one nested result class and one
  top-level error class.

## Expected discovery facts

Once the registration job wires the adapter into the registry and adds
`**/*.java` to the default scan includes, a scan of this directory should find
seven callables, sorted by `(line, qualified_name)`:

| Qualified name | Kind | Parameters | Return type | Class |
|---|---|---|---|---|
| `Main.normalize` | Method | `text: String` | `String` | `Main` |
| `Main.analyze` | Method | `text: String` | `RoleResult` | `Main` |
| `Main.readConfig` | Method | — | `int` | `Main` |
| `Main.auth` | Method | — | `String` | `Main` |
| `Main.run` | Method | `args: String` | `void` | `Main` |
| `Main.main` | Method | `args: String[]` | `void` | `Main` |
| `FunctionError.FunctionError` | Method | `code: String` | `void` | `FunctionError` |

`RoleResult` is indexed with fields `role: String` and `score: double`, so
`Main.analyze` resolves its return fields. The nested `RoleResult` class has no
methods, and the local class pattern is not used here; discovery deliberately
walks only the direct members of top-level class bodies. Leading `//` comments
become `docDescription` values; no `@tags` are present, so `tags` is empty.

## Expected body-analysis facts

- `Main.normalize` has steps `Branch`, `Return`. Its guard is `text.isEmpty()`
  and the throw yields error code `EMPTY_TEXT`, status `400`, `when`
  `(text.isEmpty())`.
- `Main.analyze` has steps `Branch`, `Call`, `Call`, `Call`, `Transform`,
  `Return`. The guard throws `TOKEN_NOT_FOUND` (status `422`, `when`
  `(text.isBlank())`). The `normalize(text)` initializer is a `Call` step with
  raw callee `normalize`; `System.out.println(...)` is a `Call` step with
  callee `println`; `new RoleResult()` is a constructor `Call` step with
  callee `RoleResult`. `result.role = cleaned;` is the `Transform` step. The
  print makes the function `SideEffect` with permission `console:out`,
  timeout `1000` and runtime `node20`.
- `Main.readConfig` reads through `Files.readString`, so it is `SideEffect`
  with `fs:read`, timeout `1000` and runtime `node20`.
- `Main.auth` reads `System.getenv("API_KEY")`, so it has permission
  `secrets:read:API_KEY`.
- `Main.run` has `Call`, `Call` steps. `String output = analyze("hello").role;`
  is the fixture's call edge: the raw callee `analyze` is resolved by core into
  the `Main.run` → `Main.analyze` edge. The second `System.out.println` call is
  also a `Call` step with callee `println`.
- `Main.main` calls `run(args)` in a `Call` step with raw callee `run` — a
  second call edge into `Main.run`.
- `FunctionError.FunctionError` is pure with no permissions (timeout `150`,
  runtime `universal`); its `super(code)` statement is a `Transform` step.

Calls that appear only inside `Transform` or `Return` steps still contribute
raw names to `calleeNames`, but core resolves edges from `Call` steps only.

## Runtime behaviour

From this directory, `javac Main.java && java Main` prints:

```text
cleaned: hello
hello
```

The `readConfig` and `auth` functions are not invoked by `main`; they exist to
supply deterministic filesystem and secret-permission facts for the scanner.
`readConfig` needs a `config.txt` file only if it is called, not to compile.
