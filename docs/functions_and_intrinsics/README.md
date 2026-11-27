# Functions, `def`, `@ctx`, and Intrinsics

## `func` and `extern func`

- **Functions (`func`)**: Monomorphic functions with explicitly typed parameters, an explicit return type, and a concrete ABI signature (can be called directly or passed as function-pointer values):
  ```hart
  func addPair(a: s32, b: s32) -> s32 {
      return a + b
  }
  ```
- **External C functions (`extern func`)**: Declare external ABI symbols (e.g. from Cosmopolitan Libc). Variadic parameters (`...`) are supported on `extern func`:
  ```hart
  extern func printf(ptr: *u8, ...) -> s32
  ```

## Entry Point (`main`)

- `main` must be a `func`, **never** a `def` (`entry point 'main' must be a function ('func'), never a 'def'`).
- `main` accepts either no parameters `func main()` or a single parameter `func main(args: Slice<String>)`.
- `main` must return either `s32` or `:none` (returning `:none` exits with status `0`).

## Polymorphic `def` Functions

`def` defines a compile-time specialized, polymorphic function whose parameter types and return type are inferred/specialized per call site:

```hart
def addOne(x) {
    return x + 1
}
```
- Each distinct argument-type combination generates a monomorphized specialization.
- Parameters may optionally specify a type (`def f(x: s32, y)`), and the return type `-> Type` is optional.
- `def` bodies can return early with `return`, or use single-line implicit return (`def id(x) { x }`). If both `return` and a trailing expression exist, their types are unified into a sum type.
- Calling a `def` function requires `@ctx` to be initialized in the caller's scope (`'@ctx' must be initialized before calling 'def' function`).

## Uniform Function Call Syntax (UFCS)

Any function or `def` (or intrinsic) in scope can be called using method syntax `receiver.name(args...)`, which desugars to `name(receiver, args...)` when `receiver` does not have a struct field named `name`:
- `print(:nl).must()` -> `must(print(:nl))`
- `raw.@asTypeOf(x.&mut)` -> `@asTypeOf(raw, x.&mut)`

By convention, UFCS should only be used for:
1. Ceremonial functions such as `must()` (ideally always called postfix: `expr.must()`), or
2. Passing a function's return value directly as the first argument to another function (e.g. `vecInit([1, 2, 3]).must()`).

For the first function in a pipeline, always pass arguments directly rather than chaining the initial value in (e.g. `readFile(path).must()`, not `path.readFile().must()`).

## Implicit Context (`@ctx`)

`@ctx` is a language-level implicit variable of type `Context` (defined in `std/core`).
- `@ctx` must be initialized before it is read or before any `def` is called (typically `@ctx := default_context` at the start of `main`).
- Individual capabilities can be overridden using struct spread, e.g. `@ctx := { ...default_context, printer: debugPrinter }` or `@ctx := { ...@ctx, printer: debugPrinter }` (or field assignment `@ctx.printer := debugPrinter`).
- Inside `func`, `@ctx` is not automatically inherited from callers; functions that need `Context` take `ctx: *Context` and set `@ctx := ctx.*` (as `stdPrinter`, `debugPrinter`, `stdLogger`, and `stdPanic` do).

## Compiler Intrinsics

| Intrinsic | Availability | Description |
| :--- | :--- | :--- |
| `@sizeOf(expr)` | Anywhere | Returns the size in bytes of `expr`'s type as `usize`. |
| `@asTypeOf(expr, target)` | Anywhere | Reinterprets/casts `expr` to the type of `target` (commonly used to type `*mut opaque` from `heapAlloc`). |
| `@caller_line_number` | Inside `def` only | Expands to the call-site line number (`u64`). Using it in `func` is a compile error. |
| `@caller_file_name` | Inside `def` only | Expands to the call-site file path (`String`). |
| `@this_line_number` | Anywhere | Expands to the current source line number (`u64`). |
| `@this_file_name` | Anywhere | Expands to the current source file path (`String`). |

## Tests

- [ok_def_intrinsics](tests/ok_def_intrinsics/main.hart) — `def` polymorphism, `@ctx`, `@sizeOf`, `@asTypeOf`, `@caller_*`, `@this_*`.
- [err_caller_outside_def](tests/err_caller_outside_def/main.hart) — Rejects `@caller_line_number` outside a `def`.
- [err_main_is_def](tests/err_main_is_def/main.hart) — Rejects `def main`.
- [err_ctx_uninitialized](tests/err_ctx_uninitialized/main.hart) — Rejects calling a `def` before `@ctx` is initialized.

