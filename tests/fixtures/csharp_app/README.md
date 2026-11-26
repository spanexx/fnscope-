# fnscope C# fixture app

A small, coherent C# file used as the fnscope C# adapter fixture. It exercises
namespace-level class discovery, a constructor, a record primary constructor, a
call edge, two guarded throws, a side effect, a local function and a type index.

Scan it with:

```text
fnscope scan ./tests/fixtures/csharp_app
```

The adapter is not registered globally by J36, so a default built-in scan does
**not** find these functions until registration (a separate, reviewed job) and
a `**/*.cs` include glob are added. The file is valid C# 10+ syntax and has no
external package dependencies. It is intentionally small so an assertion over
the full function set stays readable.

## Expected functions

A registry that has `CSharpAdapter` registered should discover **at least 8
functions** from `App.cs`:

| Function | Qualified name | Kind | Purity | Key evidence |
| --- | --- | --- | --- | --- |
| `Normalize` | `TextTools.Normalize` | Method | pure | `EMPTY_TEXT` guard, `//` doc comment |
| `Add` | `TextTools.Add` | Method | pure | optional `factor = 2.5` default |
| `Analyze` | `TextTools.Analyze` | Method | side-effect | calls `Normalize` and `Add`, `Console.WriteLine`, `TOKEN_NOT_FOUND` guard |
| `ReadConfig` | `TextTools.ReadConfig` | Method | side-effect | `File.ReadAllText` → `fs:read` |
| `Auth` | `TextTools.Auth` | Method | side-effect | `Environment.GetEnvironmentVariable("API_KEY")` → `secrets:read:API_KEY` |
| `Fetch` | `TextTools.Fetch` | Method | side-effect | `HttpClient` → `net:outbound`, 5000 ms budget |
| `Run` | `TextTools.Run` | Method | pure | declares a first-level local function |
| `Localize` | `TextTools.Run.Localize` | Declaration | pure | nested `Inner` is not discovered |
| `RoleResult` constructor | `RoleResult.RoleResult` | Method | pure | assigns `Role` and `Score` |
| `Ping` primary constructor | `Ping.Ping` | Method | pure | record header `string Code, int Retry = 3` |

`TextTools.Analyze` has a **qualified name with a dot**, proving C# discovery
records class methods as `ClassName.method` and sets `class_name`. Its steps
call `Normalize` and `Add`, so both helpers gain a reverse `calledBy` edge.

### `Analyze` is the fallible hub

- `string cleaned = Normalize(text);` is a `Call` step with `calls_fn_id`
  resolving to `Normalize`.
- `double scaled = Add(cleaned.Length);` is a second `Call` step; `Add` has an
  optional default so `Parameter.required` is `false`.
- `if (text.Length == 0)` is a `Branch` step whose output is
  `throws TOKEN_NOT_FOUND | continues`.
- `throw new FunctionError("TOKEN_NOT_FOUND")` becomes an `ErrorSpec` with code
  `TOKEN_NOT_FOUND`, status `422` and `when` containing `text.Length == 0`.
- `Console.WriteLine(...)` gives `purity = side-effect` and `permissions`
  containing `console:out`.

### Side-effect markers

- `ReadConfig` uses `File.ReadAllText`, so it is a side effect with `fs:read`.
- `Auth` reads `API_KEY`, so it carries `secrets:read:API_KEY`.
- `Fetch` constructs an `HttpClient`, so it carries `net:outbound` and the
  network timeout budget of `5000 ms` with runtime `node20`.

Pure helpers keep the contract's base budget of `150 ms` / `universal`.

### Type index

`RoleResult` contributes `Role: string`, `Score: double`, `Count: int` and
`Total: int`. `Ping` contributes its primary-constructor parameters `Code:
string` and `Retry: int`. A method declaring `RoleResult` or `Ping` as its
return type gets those fields filled through `collect_types` plus the shared
`resolve_fields` helper, including when the type lives in another file.

### Local functions

`Localize` is a first-level local function inside `Run` and is discovered as
`TextTools.Run.Localize` with `FnKind::Declaration`. `Inner` is nested inside
`Localize`, so it is deliberately skipped. Body analysis does not emit a step
for either local-function declaration.

### Note on core filtering

Core owns glob includes/excludes. A default built-in registry does not claim
`.cs`, and default includes do not contain `**/*.cs`; registering the adapter and
adding that glob are out of scope for this adapter job and are reported as
findings in `docs/contract/adapters/csharp.md`.
