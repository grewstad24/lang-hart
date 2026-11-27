# Hart Style Guide (Default Conventions)

This guide defines the default conventions for writing idiomatic Hart code where the language allows multiple equivalent forms.

---

## 1. Formatting & Layout

- **Indentation**: 6 spaces, no tabs (matching `.editorconfig`).
- **Separators**:
  - Multi-line blocks, structs, and parameter lists use newlines without trailing commas.
  - Single-line structs, tuples, arrays, and argument lists use `, ` between items:
    ```hart
    point := { x: 1, y: 2 }
    nums := [1, 2, 3]
    ```

## 2. Naming

| Construct | Convention | Examples |
| :--- | :--- | :--- |
| Types & Type Aliases | `PascalCase` | `Context`, `Slice<T>`, `IoError`, `LogLevel` |
| `func` & `def` | `camelCase` | `must`, `heapAlloc`, `stdPrinter`, `errnoToError` |
| Constants, Locals, Params, Fields | `snake_case` | `default_context`, `log_level`, `curr_ptr` |
| Atoms (Symbols) | `:snake_case` | `:none`, `:true`, `:false`, `:permission_denied` |
| Modules & Namespaces | `snake_case` | `std`, `std_impl`, `std_libc`, `memory` |

## 3. Functions: `def` vs. `func`

- **Use `def`** for all top-level directly called functions (optionally adding `: Type` / `-> Type` annotations on public APIs when helpful).
- **Use `func` only** for:
  - `main` (required by the compiler), and
  - Callbacks / function-pointer values with a fixed ABI signature (e.g., `Context` handlers like `stdPrinter`, `stdLogger`).

## 4. Context (`@ctx`) & Struct Updates

- **Initialize `@ctx` once** at the start of `main` (or via `@ctx := ctx.*` inside `func` callbacks).
- **Prefer struct spread** over line-by-line field mutation when overriding `@ctx` or deriving a new struct:
  ```hart
  @ctx := { ...default_context, printer: debugPrinter }
  @ctx := { ...@ctx, log_level: { ...@ctx.log_level, info: :false } }
  ```

## 5. Blocks, Implicit `return`, and `yield`

- **Single-line `def`s / helpers**: Omit `return` when the body is a single expression with no trailing newline before `}`:
  ```hart
  def add(a, b) { a + b }
  ```
- **Multi-line functions**: Always write explicit `return expr`.
- **Multi-line value blocks**: Always use explicit `yield expr`.

## 6. Calls & UFCS

- **Use UFCS** (`receiver.fn(args)`) **only** for:
  1. **Ceremonial / post-processing functions** like `must()`:
     ```hart
     print("hello").must()
     ```
  2. **Chaining a function's return value** as the first argument to another function:
     ```hart
     v := vecInit([1, 2, 3]).must()
     readFile(path).must().println().must()
     ```
- **Pass arguments directly to the first function in a pipeline** rather than chaining the initial input in with UFCS (e.g., `vecInit([1, 2, 3]).must()` and `readFile(path).must()`, not `[1, 2, 3].vecInit().must()` or `path.readFile().must()`).
- **Use direct calls** (`fn(args)`) everywhere else—do not use UFCS on plain variables, literals, or standalone actions:
  ```hart
  log("started").must()
  pprint(user).must()
  v := vecInit([1, 2, 3]).must()
  ```

## 7. Control Flow

- **`match`**: Use for union/sum-type discrimination (enforces exhaustiveness).
- **`switch`**: Use when comparing one scrutinee against multiple constant values (`0`, `:read`, etc.); include an explicit `else => ...` arm when a value is expected.
- **`if`**: Use for general `Bool` conditions.
- **Loops**:
  - Use `repeat for iterable [using item]` to iterate over arrays and slices.
  - Use `repeat count [using i]` for counted loops.
  - Use `repeat while cond` for positive conditions, and `repeat until cond` when the natural condition is negated (preferring `repeat until cond` over `repeat while not(cond)`).

## 8. Error Handling

- **Recover or propagate** fallible results with `match`:
  ```hart
  file := match openFile(path, :read) using res {
        IoError => return res
        else => res
  }
  ```
- **Panic on unrecoverable failure** by calling `must()` in postfix UFCS form (`expr.must()`).

## 9. Modules & `use`

- Place `mod <path>` at the top of each file belonging to a named module (e.g. `mod mylib::math`), followed by `pub use` and `use` statements.
- Use `use std::*` at the top of application files to bring the standard library into scope.
- Prefer qualified `mod::item` (or `use mod::sub` to bind `sub::item`) when calling into external modules rather than glob-importing everything, except for `use std::*` and `pub use ...::*` preludes.
- Keep module-internal helpers private (no `pub`), and mark only the intended public API with `pub`.

## 10. Standard Library (`stdcore`)

- **Context setup**: Initialize `@ctx := default_context` at the start of `main`, and restore `@ctx := ctx.*` at the top of any `func` callback that calls `def`s.
- **Imports**: Use `use std::*` in application files; keep `use std::libc::*` confined to low-level FFI modules.
- **Error propagation**: Reusable `stdcore` and library functions must propagate errors (`:error` / `IoError`) upwards to their callers rather than calling `must()` internally.
- **`must()` convention**: Ideally call `must()` with postfix UFCS (`print("ok").must()`, `heapAlloc(n).must()`) rather than prefix `must(print("ok"))`.
- **Memory cleanup**: Pair `heapAlloc`, `heapNew`, and `vecInit` allocations with `defer heapDealloc(...)` (or explicit `heapDealloc`) when ownership does not outlive the scope.

