# Stack Pointer Escape Analysis

Hart tracks whether pointer-producing expressions may point to a local stack variable (`may_point_to_stack`) and rejects any code that would allow a stack pointer to outlive its stack frame.

## Tracked Sources

An expression is marked `may_point_to_stack` when it originates from:
- Taking the address of a local variable (`x.&` or `x.&mut`) or `@ctx` (`@ctx.&`, `@ctx.&mut`).
- Indexing or member-accessing a local aggregate (`arr[i].&`, `s.field.&`).
- Dereferencing-and-re-addressing a stack pointer (`p.*.&`).
- Propagating through local bindings (`alias := local.&`), reassignments (`mut p := heap_ptr; p := local.&`), casts (`local.& as *opaque`), `@asTypeOf`, blocks, `if`/`match` arms, tuples, structs, or unions.

## Forbidden Escapes

1. **Returning a stack pointer**: Returning a pointer (or a struct/tuple/union/cast containing a pointer) that may point to a local stack variable is a compile error:
   `pointer to local stack variable cannot escape function via return`
2. **Storing a stack pointer through an out-parameter**: Writing a stack pointer into a dereferenced pointer (`out.* := local.&`) when `out` itself does not point to the current stack frame is a compile error:
   `cannot store pointer to local stack variable into dereferenced pointer`

## Safe Patterns

- Returning **values** (integers, structs, tuples, slices of static/heap data).
- Returning pointers obtained from **heap allocation** (`heapAlloc`, `heapNew`) or **parameters** passed in by the caller.
- Storing a stack pointer into another local stack variable (`local_ptr.* := local.&`).

## Tests

- [ok_escape_safe](tests/ok_escape_safe/main.hart) — Safe value returns, parameter pointer pass-through, and heap-allocated pointer returns.
- [err_escape_return](tests/err_escape_return/main.hart) — Direct return of `x.&`.
- [err_escape_alias](tests/err_escape_alias/main.hart) — Returning an alias to `x.&`.
- [err_escape_reassign](tests/err_escape_reassign/main.hart) — Reassigning a pointer to `x.&` before returning.
- [err_escape_struct](tests/err_escape_struct/main.hart) — Returning a struct containing `x.&`.
- [err_escape_tuple](tests/err_escape_tuple/main.hart) — Returning a tuple containing `x.&`.
- [err_escape_union](tests/err_escape_union/main.hart) — Returning a union containing `x.&`.
- [err_escape_cast](tests/err_escape_cast/main.hart) — Returning `x.& as *opaque`.
- [err_escape_out_param](tests/err_escape_out_param/main.hart) — Storing `x.&` into `out.*`.

