# Type Casting and Coercion in Hart

This document is the complete reference for type casting, implicit coercion, and representation conversions in the Hart programming language.

---

## 1. Core Principles

Hart's type system is **structural** rather than nominal. Types are compared by their shape, size, and layout, not their declared names.

Type transitions happen in two ways:
1. **Implicit Coercion**: Performed automatically by the compiler at specific boundary locations (function arguments, assignments, returns, binary arithmetic, and unambiguous union promotion).
2. **Explicit Casting**: Written by the programmer via `value as TargetType` (or the `@asTypeOf(val, target)` intrinsic).

---

## 2. The 8 Casting Categories

### 1. Integers
| Source | Target | Cast Type | Behavior |
| :--- | :--- | :--- | :--- |
| `u8`, `u16`, `u32` | `u16`, `u32`, `u64`, `usize` | **Implicit / Explicit** | Lossless zero-extension (`zext`). |
| `s16`, `s32` | `s32`, `s64`, `ssize` | **Implicit / Explicit** | Lossless sign-extension (`sext`). |
| Larger integer | Smaller integer | **Explicit** | Truncation (`trunc`). |
| Signed | Unsigned (same width) | **Explicit** | Bitcast reinterpretation. |
| Integer literal | Sized integer | **Implicit** | Automatically fits if value is within bounds. |

### 2. Pointers & Mutability
| Source | Target | Cast Type | Behavior |
| :--- | :--- | :--- | :--- |
| `*mut T` | `*T` | **Implicit / Explicit** | Drop mutability (safe widening). |
| `*T` | `*mut T` | **Forbidden** | Cannot gain mutability implicitly. |
| `*T` / `*mut T` | `*opaque` | **Implicit / Explicit** | Erase type to raw untyped pointer (`bitcast`). |
| `*opaque` | `*T` | **Explicit** | Recover typed pointer from raw pointer. |
| `*A` | `*B` | **Explicit** | Valid if `A` and `B` are equivalent structural types. |

> **Safety Rule**: `*opaque` cannot be dereferenced (`ptr.*` is a compile error). It exists solely for type erasure behind pointers.

### 3. Pointers and Integers (Addresses)
| Source | Target | Cast Type | Behavior |
| :--- | :--- | :--- | :--- |
| `*T` / `*mut T` / `*opaque` | `usize`, `u64` | **Explicit** | `ptrtoint` (pointer address as integer). |
| `usize`, `u64` | `*T` / `*mut T` / `*opaque` | **Explicit** | `inttoptr` (integer as memory address). |

### 4. Arrays, Tuples, and Slices
| Source | Target | Cast Type | Behavior |
| :--- | :--- | :--- | :--- |
| `[T; N]` (Array) | `Slice<T>` | **Implicit / Explicit** | Synthesizes `{ ptr: &arr[0], len: N }`. |
| `(T, T, ...)` (Homogeneous Tuple) | `Slice<T>` | **Implicit / Explicit** | Stores tuple on stack and returns `{ ptr, len }`. |
| `(T1, T2, ...)` (Heterogeneous Tuple) | `Slice<(T1 \| T2 \| ...)>` / `Slice<Generic>` | **Implicit / Explicit** | Unifies element types to a sum type (or uses target slice element type), casts each element into a contiguous stack buffer, and returns `{ ptr, len }`. |
| `()` (Empty Tuple) | `Slice<T>` / `Slice<Generic>` | **Implicit / Explicit** | Returns empty slice with `{ ptr: null, len: 0 }`. |
| `Slice<A>` | `Slice<B>` | **Explicit** | If `A as B` is valid, converts elements element-by-element into a new slice. |
| `Slice<T>` | `Slice<opaque>` | **Implicit / Explicit** | Bitcasts the slice pointer `{ ptr: *opaque, len }`. |

> **Union Disambiguation**: Tuples do **not** cast to `String` (`Slice<u8>`). This ensures that `()` or `(T, ...)` coerced into `Generic` resolves unambiguously to `Slice<Generic>`, never colliding with `String`.

### 5. Structs to Records / Maps
A struct literal `{ key1: val1, key2: val2 }` has compile-time string field names.

```hart
type Map <K, V> := *{
      len: usize
      keys: *K
      values: *V
}

type Record <V> := Map <String, V>
```

| Source | Target | Cast Type | Behavior |
| :--- | :--- | :--- | :--- |
| `struct { field: T, ... }` | `Record<V>` / `Map<String, V>` | **Implicit / Explicit** | Synthesizes string keys array `["field"]`, boxes values into `V` array, and returns a pointer to the record header `{ len, keys, values }`. |

> **Key Matching Requirement**: A struct literal can only cast to `Map<K, V>` if `K` is `String` (or `Slice<u8>`). It will **not** cast to `Map<Generic, Generic>` or `Map<i32, i32>`.

### 6. Symbols and Atoms
- **Symbol**: A compile-time interned identifier (e.g. `:foo`, `:func`, `:true`, `:none`). Zero runtime size (`{}`).
- **Atom**: A runtime string wrapper (`type Atom := { str: String }`).

| Source | Target | Cast Type | Behavior |
| :--- | :--- | :--- | :--- |
| `:symbol` (e.g. `:user`) | `Atom` | **Implicit / Explicit** | Synthesizes an `Atom` struct `{ str: "user" }`. |

### 7. Functions and Function Pointers
| Source | Target | Cast Type | Behavior |
| :--- | :--- | :--- | :--- |
| `func(...) -> R` | Function pointer type | **Implicit / Explicit** | Reinterprets function symbol as callable pointer. |
| Function pointer | `:func` (Symbol) | **Implicit / Explicit** | Evaluates to the unit symbol `:func` (used for `Generic` boxing). |
| Function pointer | `usize`, `u64` | **Explicit** | Function entrypoint address (`ptrtoint`). |
| Function pointer | `*opaque` | **Forbidden** | Function pointers do not cast to `*opaque`; they cast to `:func`. |

### 8. Sum Types / Unions (Including `Generic`)
Hart unions are represented in LLVM as `{ i8 tag, [max_size x i8] payload }`.

| Operation | Syntax | Condition | Behavior |
| :--- | :--- | :--- | :--- |
| **Upcast / Promotion** | `T as (A \| B \| C)` | `T` is supported by variant `A` | Stores variant tag index and payload into the union struct. |
| **Union-to-Union** | `(A \| B) as (A \| B \| C)` | Every variant in source maps to target | Switches on source tag and maps to corresponding target tag. |
| **Narrowing / Downcast** | `(A \| B) as A` | Target variant is part of the union | Unboxes variant or branches via `match`. |

---

## 3. Implicit Coercion Boundaries

Implicit coercion is triggered automatically at the following semantic boundaries:

1. **Call Arguments**: When an argument type does not match the function parameter type.
2. **Variable Assignment**: When assigning `target := value` and `target` has an explicit type annotation.
3. **Function Return**: When the returned expression does not match the declared return type.
4. **Binary Arithmetic**: Widening the smaller integer to match the larger integer (e.g. `u8 + u32` &rarr; both promoted to `u32`).
5. **Pattern Match Arm Unification**: Unifying arm return values to the overall match expression type.

### The Union Ambiguity Rule
When coercing a value into a union `U := V1 | V2 | ...`:
- If **exactly one** variant $V_k$ can accept the value, the compiler automatically inserts an implicit cast:
  ```hart
  logInfo("hello")  // String matches variant String in Generic -> Coerced automatically
  ```
- If **zero or multiple** variants match, implicit coercion is **rejected** with a compile-time ambiguity error. An explicit cast `as Target` is required to disambiguate.

---

## 4. Memory Semantics of Casts

| Cast Operation | Memory Overhead | Allocation Location |
| :--- | :--- | :--- |
| Integer extension / truncation | Zero-cost register | Register |
| `*T` &rarr; `*opaque` | Zero-cost pointer cast | Register |
| Value &rarr; Union (Boxing) | Tag + payload copy | Stack (`alloca`) or Register |
| `[T; N]` &rarr; `Slice<T>` | Pointer + length struct | Stack (`alloca`) |
| `{ name: val }` &rarr; `Record<V>` | Keys array + values array + header | Stack (`alloca`) |

> **Lifetime Warning**: Because Array-to-Slice and Struct-to-Record casts synthesize storage on the **current stack frame** via `alloca`, returning a synthesized `Slice` or `Record` from a function returns a pointer to dead stack memory. For data that must outlive the function, allocate memory explicitly via `memalloc` or heap allocators.

