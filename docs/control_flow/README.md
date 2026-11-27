# Control Flow & Error Handling

All control-flow constructs in Hart are expressions.

## Blocks, Implicit `return`, and `yield`

A block `{ stmt1; stmt2; ... }` creates a new lexical scope and a `defer` scope:
- **No trailing newline before `}`** (e.g. `func f() -> s32 { 42 }` or `def id(x) { x }`): The final expression (unless it is already `return`, `yield`, `break`, `continue`, or `defer`) is **implicitly wrapped in `return`**, returning from the enclosing function.
- **Trailing newline before `}`** (multi-line blocks): Every expression statement is evaluated and discarded (unless it is `return`, `yield`, `break`, `continue`, `defer`, or contains `yield`), and the block evaluates to `:none` unless it uses `yield`.
- **Explicit `yield`**: `yield expr` exits the innermost enclosing block with `expr` as its value. `yield` with no expression (followed by a newline or `}`) defaults to `yield :none`. All `yield` arms in a block are unified into a sum type if their types differ.

## `if` Expressions

```hart
result := if {
    a > b => a
    else => b
}
```
- Each arm condition must evaluate to `Bool` (`:true | :false` from `std/core`). Non-`Bool` conditions are a compile error (`condition must evaluate to Bool`).
- If no `else => ...` arm is provided, an implicit `else => :none` arm is appended.

## `switch` Expressions

`switch` desugars to `if` and matches a value against constant/equality patterns (`=`). Like `if`, if no `else => ...` arm is provided, an implicit `else => :none` arm is appended:

```hart
label := switch code {
    0 => "zero"
    1 => "one"
    else => "other"
}
```

## `match` Expressions

`match` performs **exhaustive** type-based discrimination on union values:

```hart
val := match result using res {
    IoError => return res
    else => res
}
```
- `using binder` is optional; when present, `binder` is bound in each arm with the narrowed type for that arm.
- Every variant of the target union must be covered by an explicit type arm or an `else => ...` arm, otherwise compilation fails (`non-exhaustive match expression`).

## Loops: `repeat`

Hart provides four `repeat` loop forms, all evaluating to `:none`:
- **Counted loop**: `repeat count { ... }` or `repeat count using i { ... }` (where `i` is `usize` from `0` to `count - 1`).
- **Collection loop**: `repeat for iterable { ... }` or `repeat for iterable using item { ... }` iterates over a fixed-size array `[N]T` or a `Slice<T>`.
- **Pre-condition loop**: `repeat while cond { ... }` (`cond` must be `Bool`).
- **Inverted-condition loop**: `repeat until cond { ... }` (`cond` must be `Bool`; continues while `cond` is `:false`).

Inside any loop:
- `break` exits the innermost loop.
- `continue` jumps to the next iteration of the innermost loop.
- Using `break` or `continue` outside a loop is a compile error.

## `return` and `defer`

- `return expr` exits the enclosing `func` (or returns from a `def` specialization).
- `defer expr` schedules `expr` to execute when the enclosing block exits, in **LIFO (last-in, first-out)** order.
- `defer` runs on normal block exit, `yield`, `break`, `continue`, and `return` (unwinding all active `defer` scopes up to the target boundary).

## Error-Discarding Prevention

Hart forbids silently discarding any expression whose type may contain an error:
- A type "may contain an error" if it is `:error`, a tuple whose first element is `:error` (such as `(:error, :not_found)` or `ErrorWithReason<T>`), or a union containing any such variant (such as `IoError`, `Error`, `Voidance`, or `s32 | :error`).
- Calling a fallible function as a standalone statement without handling it (via `match`, `must(...)`, or binding/returning it) is a compile error (`may be an error and cannot be discarded`).

## Tests

- [ok_control_flow](tests/ok_control_flow/main.hart) — `if`, `match`, `switch`, `repeat`, `repeat while`, `repeat until`, `break`, `continue`, `yield`, `defer`.
- [ok_for_and_defer](tests/ok_for_and_defer/main.hart) — `repeat` over arrays and slices, LIFO `defer` ordering across `yield`, `break`, `continue`, and `return`.
- [err_break_outside_loop](tests/err_break_outside_loop/main.hart) — Rejects `break` outside a loop.
- [err_continue_outside_loop](tests/err_continue_outside_loop/main.hart) — Rejects `continue` outside a loop.
- [err_discard_error](tests/err_discard_error/main.hart) — Rejects discarding a value whose type may contain an error.
- [err_if_non_bool](tests/err_if_non_bool/main.hart) — Rejects non-`Bool` conditions in `if`.
- [err_match_non_exhaustive](tests/err_match_non_exhaustive/main.hart) — Rejects non-exhaustive `match` expressions.
- [err_yield_type_mismatch](tests/err_yield_type_mismatch/main.hart) — Rejects mismatched `yield` / return types.
