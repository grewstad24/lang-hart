# Types, Mutability, Coercion, and Casting

## Primitive Types

| Type | Description |
| :--- | :--- |
| `u8`, `u16`, `u32`, `u64`, `usize` | Unsigned integers (8, 16, 32, 64, and pointer-sized 64-bit) |
| `s16`, `s32`, `s64`, `ssize` | Signed integers (16, 32, 64, and pointer-sized 64-bit) |
| `opaque` | Unsized/untyped pointee type (used as `*opaque` or `*mut opaque`; cannot be dereferenced) |
| `never` | Bottom type for diverging expressions (`return`, `break`, `continue`, `abort()`, `panic()`); coerces to any type |

Untyped integer literals default to `s32` unless contextually typed or explicitly cast.

## Atoms (Symbols)

An atom like `:ok`, `:none`, `:true`, `:false`, or `:error` is both a value and a singleton type:
- `:none` is Hart's unit/void type and value.
- `Bool` in `std/core` is defined as `:true | :false`.
- Atoms implicitly coerce (and can be cast) to `String`, producing the atom's name without the leading `:` (e.g., `:ok as String` is `"ok"`).

## Pointers

- `*T`: Immutable pointer to `T`. Reading `p.*` is allowed; writing `p.* := val` is a compile error (`cannot mutate through immutable pointer`).
- `*mut T`: Mutable pointer to `T`. Allows both reading and writing `p.*`.
- Address-of:
  - `x.&` takes an immutable pointer `*T` to `x`.
  - `x.&mut` takes a mutable pointer `*mut T` to a mutable binding `x`.
- `*opaque` / `*mut opaque`: Untyped pointers (e.g., from `malloc` / `heapAlloc`). Dereferencing `*opaque` is a compile error (`cannot dereference untyped pointer '*opaque'`).

## Arrays and Slices

- **Fixed-size arrays**: `[N]T`, constructed with `[a, b, c]`.
  - `.len` returns `usize` (`N`).
  - `.ptr` returns `*T` (or `*mut T` for mutable array bindings).
  - `arr[i]` indexes element `i`.
- **Slices**: `Slice<T>` is defined in `std/core` as `{ ptr: *T, len: usize }`.
  - `s[i]` indexes element `i` directly on any `Slice<T>`.
  - `String` is `type String := Slice<u8>`.

## Structs, Tuples, and `Record`

- **Structs**: Structural product types `{ x: s32, y: s32 }`. Two aliases with identical field names, order, and types are compatible.
- **Tuples**: Positional product types `(A, B, C)`, indexed with `.0`, `.1`, `.2`. Empty tuple is `()`; single-element tuple requires a trailing comma `(a,)`.
- **Spread (`...`)**:
  - Inside array literals `[...arr, 4]` and tuple literals `(...tup, :end)` and call arguments `foo(...tup)`, `...expr` expands fixed-size arrays or tuples inline.
  - Inside struct literals `{ ...base, x: 10 }`, `...expr` copies all fields from `base`, and later fields override earlier ones.
- **`Record`**: Defined in `std/core` as `*{ len: usize, key: *String, val: *Generic }`. Any struct literal or struct value whose fields can all be cast to `Generic` implicitly coerces (or casts) to `Record`.

## Union Types and Type Aliases

- **Unions**: `A | B | C` represents a tagged sum type. Variants are deduplicated and canonicalized.
- **Type Aliases**:
  - `type Name := TargetType`
  - Parameterized (generic) type aliases: `type Box<T> := { val: T }`, `type Pair<A, B> := (A, B)`.

## Mutability and Definite Initialization

- `x := expr` declares an **immutable** binding (type is inferred from `expr`). Reassigning to it is a compile error (`cannot reassign to immutable constant`). Top-level immutable constants use `const x := expr`.
- `mut x := expr` or `mut x: Type := expr` declares a **mutable** binding, which can be reassigned with `:=` or compound operators `+=`, `-=`, `*=`, `/=`.
- `mut x: Type` declares an uninitialized mutable variable. Reading it before it is definitely initialized on all control-flow paths is a compile error (`mutable variable 'x' may be read before it is initialized`).

## Implicit Coercions vs. Explicit Casts (`as`)

### Implicit Coercions
1. **Lossless integer widening**: Same signedness and strictly smaller bit width (`u8` -> `u16` -> `u32` -> `u64`, `s16` -> `s32` -> `s64`).
2. **Pointer mutability & opaque**: `*mut T` -> `*T`; `*mut opaque` -> `*mut T` or `*T`; `*opaque` -> `*T` (never `*T` -> `*mut T`).
3. **Array to Slice**: `[N]T` -> `Slice<U>` when `T` is compatible with `U`.
4. **Slice covariance**: `Slice<T>` -> `Slice<U>` when `T` is compatible with `U` (e.g., `Slice<String>` -> `Slice<Generic>`).
5. **Atom to `String`**: `:atom` -> `String`.
6. **Struct to `Record`**: `{ k: v, ... }` -> `Record` when all field values can cast to `Generic`.
7. **Union widening**: `T` (or sub-union) -> `U1 | U2 | ...` when every source variant uniquely matches a target variant.
8. **`never`**: Coerces to any expected type.

### Explicit Casts (`expr as TargetType`)
Includes all implicit coercions plus:
- **Any integer to any integer**: Signed/unsigned conversions, narrowing, and widening.
- **Pointer / Function <-> Integer**: `ptr as usize`, `(0 as usize) as *opaque`.
- **Struct field reordering**: Structs with the same field names (castable field types) in a different order.
- **Union-to-union / union-to-target**: Allowed if every variant of the source union can cast to the target. (Narrowing a single union to one variant uses `match`, not `as`.)

## Tests

- [ok_types](tests/ok_types/main.hart) — Primitives, arrays, slices, structs, tuples, generic type aliases, structural typing.
- [ok_atoms_and_coercion](tests/ok_atoms_and_coercion/main.hart) — Atoms, `:atom` -> `String`, `[N]T` -> `Slice<T>`, struct -> `Record`, `Generic`, spread `...`, compound assignments, lossless widening.
- [err_immutable_reassign](tests/err_immutable_reassign/main.hart) — Rejects reassigning an immutable `:=` binding.
- [err_immutable_ptr_write](tests/err_immutable_ptr_write/main.hart) — Rejects writing through `*T`.
- [err_opaque_deref](tests/err_opaque_deref/main.hart) — Rejects dereferencing `*opaque`.
- [err_uninitialized_read](tests/err_uninitialized_read/main.hart) — Rejects reading a `mut` variable before definite initialization.
