# Modules, Global Namespace, and `use`

All modules in a Hart program share a **single global namespace**: any `pub` item (function, `def`, type, constant, `extern func`) or submodule exported by a module is directly accessible anywhere via `::` without an import statement. `use` is strictly for bringing names or aliases into the current module's local scope.

## Module Discovery

When compiling a project directory, the compiler recursively discovers all `.hart` files in:
1. `std/` (or `$HART_HOME/std` if `$HART_HOME` exists) — `std/core/` is always included.
2. The target project directory.

Each `.hart` file belongs to either:
- **A named (optionally nested) logical module** via `mod <path>` at the top of the file, e.g. `mod std`, `mod std::libc`, or `mod mylib::math` (multiple files sharing the same `mod <path>` form one logical module and share all private and public symbols).
- **An anonymous file-local module** if no `mod` declaration is present.

## `pub` and Qualified Access (`::`)

Top-level declarations (`type`, `const`, `func`, `def`, `extern func`) are private to their module by default. Prefixing a declaration with `pub` exports it under its module path:

```hart
mod mylib::math

def internalHelper() -> s32 { 0 }

pub def square(n: s32) -> s32 {
    return n * n + internalHelper()
}
```

From any other file or module, `mylib::math::square(6)` and `std::log("hi")` are directly callable without any `use` statement.

## `use` and `pub use`

`use` binds exported symbols or modules into the current module's local scope:
- **`use path::*`** (non-recursive): Brings every `pub` item and every **direct** submodule exported by `path` into scope. For example, `use std::*` brings `log`, `String`, `default_context`, etc. into scope, along with direct submodule `libc` (so `libc::malloc` works, while `malloc` is not brought in unless `use std::libc::*` is used).
- **`use path`**: Brings a symbol or module into scope under its last segment (e.g., `use mylib::math` allows `math::square(6)`, and `use mylib::math::square` allows `square(6)`).
- **`pub use ...`**: Performs the same local binding and also re-exports the brought-in items/submodules as `pub` members of the current module.

## `std/core` Modules

`std/core` defines:
- `std`: Public core API (`Context`, `default_context`, `String`, `Bool`, `Slice`, `Vec`, `Generic`, `Record`, `Error`, `Voidance`, `IoError`, `print`, `println`, `pprint`, `log`, `logWarn`, `logError`, `panic`, `must`, `heapAlloc`, `heapNew`, `vecInit`, `fill`, boolean gates, math helpers, and file/stream I/O).
- `std::libc`: Raw Cosmopolitan Libc `pub extern func` bindings (`malloc`, `free`, `realloc`, `abort`, `printf`, `__errno_location`, `open`, `close`, `read`, `write`, `lseek`).

## Tests

- [ok_modules](tests/ok_modules/main.hart) — Nested modules (`mod mylib::math`, `mod mylib::prelude`), global `::` access, `use std::*` non-recursive submodule import (`libc::malloc`), `use mylib::math` module binding, and `pub use ...::*` re-exporting.
