# `std/core` Reference

`std/core` is part of Hart's language semantics. Its items are globally accessible via `std::...`, and `use std::*` brings all public `std` types, `default_context`, error handling, printing, logging, heap allocation, vectors, boolean gates, math, and file I/O into scope (along with direct submodule `libc`).

## Core Types

### Primitives & Numeric Unions
- `String`: `Slice<u8>`
- `SignedInteger`: `s16 | s32 | s64 | ssize`
- `UnsignedInteger`: `u8 | u16 | u32 | u64 | usize`
- `Integer`: `SignedInteger | UnsignedInteger`
- `Bool`: `:true | :false`

### Containers & Dynamic Representation
- `Slice<T>`: `{ ptr: *T, len: usize }`
- `Vec<T>`: `{ items: Slice<T>, cap: usize }`
- `Record`: `*{ len: usize, key: *String, val: *Generic }` (target of struct-to-`Record` coercion)
- `UnvoidGeneric`: `Integer | String | Bool | Record | Slice<Generic> | *Generic`
- `Generic`: `UnvoidGeneric | Voidance`

### Errors & Voidance
- `ErrorWithReason<T>`: `(:error, T)`
- `ErrorAndReason`: `(:error, String | Record)`
- `Error`: `ErrorAndReason | :error`
- `Voidance`: `Error | :none`
- `IoError`: Union of `(:error, <reason>)` where `<reason>` is one of `:permission_denied`, `:not_found`, `:interrupted`, `:io_error`, `:bad_fd`, `:would_block`, `:out_of_memory`, `:access_denied`, `:already_exists`, `:not_a_directory`, `:is_a_directory`, `:invalid_argument`, `:no_space`, `:broken_pipe`, `:unknown`.

### Context & I/O Types
- `LogLevel`: `{ info: Bool, warn: Bool, error: Bool }`
- `OpenMode`: `:read | :write | :append | :read_write`
- `SeekWhence`: `:start | :current | :end`
- `File`: `{ fd: s32 }`
- `Context`: Capability struct holding `logger`, `log_level`, `heap_alloc`, `heap_realloc`, `heap_dealloc`, `printer`, `panic`, `open`, `close`, `read`, `write`, and `seek`.
- `default_context`: `const` `Context` wired to `stdLogger`, `stdHeapAlloc`, `stdHeapRealloc`, `stdHeapDealloc`, `stdPrinter`, `stdPanic`, `stdOpen`, `stdClose`, `stdRead`, `stdWrite`, `stdSeek`, with all `log_level` flags set to `:true`.

## Error Handling (`std/core/error`)

- `panic(reason)` (`def`): Calls `@ctx.panic(@ctx.&, reason)` (default `stdPanic` logs the reason unless `:none` and calls `abort()`).
- `must(val)` (`def`): Unwraps `val`; if `:error` calls `panic(:none)`, if `ErrorWithReason<Generic>` calls `panic(err_tuple.1)`, otherwise returns `val`.

## Printing & Pretty-Printing (`std/core/print`, `std/core/pretty`)

- `Printable`: `String | Integer | :nl | :t | (:softTab, Integer) | Bool | Slice<Printable>`
- `print(x: Printable)` (`def` -> `:none | IoError`): Formats and prints `x` to stdout.
- `println(x: Printable)` (`def` -> `:none | IoError`): Prints `x` followed by `:nl`.
- `pprint(item)` (`def`): Calls `@ctx.printer(@ctx.&, item)`.
  - `stdPrinter`: Colorized human-readable printer for `Generic` values (bools, integers, strings, nested arrays, records, pointers, voidance).
  - `debugPrinter`: Hart-syntax printer (e.g. `:true`, `"hello"`, `{ ... }`, `[ ... ]`).

## Logging (`std/core/log`)

- `log(item)` (`def`): Logs `item` at `:info` level with `@caller_line_number` and `@caller_file_name`.
- `logWarn(item)` (`def`): Logs `item` at `:warn` level.
- `logError(item)` (`def`): Logs `item` at `:error` level.
- `logWithLineAndFile(level, line, file, item)` (`def`): Dispatches to `@ctx.logger(@ctx.&, level, @ctx.log_level, line, file, item)`.

## Memory & Collections (`std/core/memory`, `std/core/misc`)

- `heapAlloc(size)` (`def`): Calls `@ctx.heap_alloc(@ctx.&mut, size as usize)` -> `*mut opaque | :error`.
- `heapRealloc(ptr, new_size)` (`def`): Calls `@ctx.heap_realloc(@ctx.&mut, ptr, new_size as usize)` -> `*mut opaque | :error`.
- `heapDealloc(ptr)` (`def`): Calls `@ctx.heap_dealloc(@ctx.&mut, ptr)` -> `:none`.
- `heapNew(x)` (`def`): Heap-allocates `@sizeOf(x)`, copies `x` into it, and returns `*mut T | :error`.
- `vecInit(init)` (`def`): Creates a `Vec<T>`. Pass `:none` for an empty `Vec`, or a slice/array (e.g. `[1, 2, 3]`) to heap-allocate and copy elements into `items` (`Slice<T>`) with `cap: s.len`.
- `fill(slice, val)` (`def`): Overwrites every element of `slice` (`Slice<T>`) with `val`.
## Boolean Gates & Operations (`std/core/misc`)

- `not(b: Bool) -> Bool` (`def`): Logical NOT.
- `and(a: Bool, b: Bool) -> Bool` (`def`): Logical AND.
- `or(a: Bool, b: Bool) -> Bool` (`def`): Logical OR.
- `xor(a: Bool, b: Bool) -> Bool` (`def`): Logical XOR.
- `nand(a: Bool, b: Bool) -> Bool` (`def`): Logical NAND.
- `nor(a: Bool, b: Bool) -> Bool` (`def`): Logical NOR.
- `xnor(a: Bool, b: Bool) -> Bool` (`def`): Logical XNOR.
- `implies(a: Bool, b: Bool) -> Bool` (`def`): Logical implication (`or(not(a), b)`).
- `all(slice)` (`def`): Returns `:true` if every `Bool` element in `slice` is `:true`.
- `any(slice)` (`def`): Returns `:true` if any `Bool` element in `slice` is `:true`.

## Math (`std/core/math`)

- `add(a, b)`, `sub(a, b)`, `mul(a, b)`, `div(a, b)` (`def`): Polymorphic arithmetic helpers.
- `min(a, b)`, `max(a, b)`, `abs(x)`, `clamp(x, lo, hi)` (`def`): Polymorphic comparison/clamping helpers.

## File & Stream I/O (`std/core/io`)

- Constants: `stdin` (`{ fd: 0 }`), `stdout` (`{ fd: 1 }`), `stderr` (`{ fd: 2 }`), `default_buffer_size` (`8192 as usize`).
- Low-level `def` wrappers (dispatching through `@ctx`):
  - `openFile(path, mode)` -> `File | IoError`
  - `closeFile(file)` -> `:none | IoError`
  - `readBytes(file, buf, count)` -> `usize | IoError`
  - `writeBytes(file, buf, count)` -> `usize | IoError`
  - `seekFile(file, offset, whence)` -> `u64 | IoError`
- High-level `def` helpers:
  - `writeAll(file, data)` -> `:none | IoError`
  - `readAll(file)` -> `String | IoError` (seeks to end/start, heap-allocates `size + 1` bytes, null-terminates)
  - `readFile(path)` -> `String | IoError`
  - `writeFile(path, data)` -> `:none | IoError`
  - `appendToFile(path, data)` -> `:none | IoError`
  - `copyStream(src, dst, chunk_size)` -> `:none | IoError`
  - `streamFile(file, chunk_size, callback)` -> `:none | IoError`

## Tests

- [ok_stdcore](tests/ok_stdcore/main.hart) — Rigorous end-to-end test of `default_context`, `print`/`println`, `pprint` (`stdPrinter` & `debugPrinter`), `log`/`logWarn`/`logError`/`logWithLineAndFile`, `math` (`abs`, `min`, `max`, `add`, `sub`, `mul`, `div`), `heapAlloc`/`heapRealloc`/`heapNew`/`heapDealloc`, `vecInit(:none)`/`vecInit([...])`, `fill`, all boolean gates (`not`, `and`, `or`, `xor`, `nand`, `nor`, `xnor`, `implies`, `all`, `any`), `must`, `stdin`/`stdout`/`stderr`, and `default_buffer_size`.
- **Known compiler limitations in `std/core`**: `clamp` (nested `def` inference yields `never`), `openFile` / file-I/O wrappers (specialization `return type mismatch` on `IoError` narrowing), and `(:softTab, Integer)` in `Printable` (tuple-with-union promotion) currently fail compilation when called.
