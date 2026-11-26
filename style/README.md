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
| `func` & `def` | `camelCase` | `assertOk`, `heapAlloc`, `stdPrinter`, `errnoToError` |
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

- **Use UFCS** (`receiver.fn(args)`) for fluent post-processing and conversions:
  ```hart
  print("hello").assertOk()
  raw.@asTypeOf(x.&mut)
  ```
- **Use direct calls** (`fn(args)`) for standalone actions and constructors:
  ```hart
  log("started")
  v := vecInit([1, 2, 3])
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
- **Panic on unrecoverable failure** with `.assertOk()` (or `assertOk(...)`).

## 9. Modules & `use`

- Place `mod <path>` at the top of each file belonging to a named module (e.g. `mod mylib::math`), followed by `pub use` and `use` statements.
- Use `use std::*` at the top of application files to bring the standard library into scope.
- Prefer qualified `mod::item` (or `use mod as alias`) when calling into external modules rather than glob-importing everything, except for `use std::*` and `pub use ...::*` preludes.
- Keep module-internal helpers private (no `pub`), and mark only the intended public API with `pub`.

