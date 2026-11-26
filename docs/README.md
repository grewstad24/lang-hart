# Hart Language & `std/core` Semantics

Hart is a statically typed, compiled systems language targeting native executables via LLVM and Cosmopolitan Libc (`cosmocc`). The `std/core` library is part of the language semantics: core types (`Bool`, `String`, `Integer`, `Slice<T>`, `Error`, `Voidance`, `Generic`, `Record`, `Context`, etc.), the implicit `@ctx` context, and core operations are defined in `std/core/` and automatically discovered by the compiler.

## Documentation Index

| Topic | Description | Tests |
| :--- | :--- | :--- |
| [Types & Casting](types_and_casting/README.md) | Primitives, atoms, pointers, arrays, slices, structs, tuples, unions, mutability, definite initialization, coercions, and `as` casts | [tests](types_and_casting/tests/) |
| [Control Flow](control_flow/README.md) | Blocks & `yield`, `if`, `switch`, `match`, loops (`repeat`, `for`), `break`, `continue`, `return`, `defer`, and error-discarding rules | [tests](control_flow/tests/) |
| [Functions & Intrinsics](functions_and_intrinsics/README.md) | `func`, `extern func`, `main` entry point, `def` polymorphic specialization, UFCS, `@ctx`, and `@` intrinsics | [tests](functions_and_intrinsics/tests/) |
| [Modules & Namespaces](modules_and_namespaces/README.md) | Directory discovery, `mod`, `decl mod`, `export`, `import`, and `std/core` namespaces | [tests](modules_and_namespaces/tests/) |
| [Escape Analysis](escape_analysis/README.md) | Compile-time stack pointer escape rules across returns, aliases, aggregates, casts, and out-parameters | [tests](escape_analysis/tests/) |
| [`std/core` Reference](stdcore/README.md) | `Context`, `default_context`, error handling, printing, logging, heap memory, vectors, math, and file I/O | [tests](stdcore/tests/) |

## Lexical Structure & Operators

- **Comments**: Line comments begin with `#` and run to the end of the line.
- **Integer Literals**: Decimal integer literals, including negative literals (e.g. `42`, `0`, `-6`). There is no unary prefix `-` operator on arbitrary expressions; negate expressions with `0 - x`.
- **String Literals**: Double-quoted UTF-8 byte sequences (`"hello\n"`) supporting escapes `\0`, `\n`, `\r`, `\t`, `\\`, `\"`, and `\u{...}` (1–6 hex digits). String literals have type `String` (`Slice<u8>`) and are null-terminated in memory.
- **Atoms (Symbols)**: Prefixed with `:` followed by `[a-zA-Z_][a-zA-Z0-9_]*` (e.g. `:none`, `:true`, `:false`, `:error`).
- **Precedence** (highest to lowest):
  1. Postfix: `expr(args)`, `expr[i]`, `expr.*`, `expr.&`, `expr.&mut`, `expr.field`, `expr.0`, `expr.@tag`, `expr as Type`
  2. Multiplicative: `*`, `/`, `%`
  3. Additive: `+`, `-`
  4. Comparison: `=`, `!=`, `<`, `<=`, `>`, `>=`
  5. Assignment / Binding: `:=`, `+=`, `-=`, `*=`, `/=`

## Running the Integration Tests

All integration tests live beside their topic documentation under `docs/<topic>/tests/` and are executed by `tests/testing/main.rs`:

```sh
cargo test
```

