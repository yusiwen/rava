# S14 — `System.arraycopy`, and the exception a native cannot raise itself

- **Slice:** S14
- **Requirements:** R4 (the library's own array paths, not a private array representation)
- **Acceptance:** `cargo test -p rava-cli --test arraycopy` exited 0 on 2026-11-20 (10 tests).

## What surprised me

**The copy is small; deciding which layer raises is not.** `HeapObject::Array` already carries a
`kind` and its `elements`, so matching kinds is enum equality and the move is `copy_within` (same
array) or a temporary `Vec` (two arrays). The native is 106 lines including its helpers. The
interesting part is the three refusals: a `JavaException` returned by a native is turned into a
real throwable by the interpreter
(`new_exception` → `Thrown` → handler search), so `NullPointerException` and
`ArrayStoreException` are catchable Java exceptions. A native's `IndexOutOfBounds`, however, is
*not* run through the interpreter's `fault` mapping — `fault` is only applied at the `ArrayStore`
instruction in `crates/interp/src/lib.rs`. So `arraycopy`'s bounds fault reaches `run_main` as a
`VmError` and the CLI prints a VM error, not an `ArrayIndexOutOfBoundsException` trace. The slice
reports the fault as instructed (`VmError::IndexOutOfBounds`, the same fault interpreter array
access produces) and records the boundary here instead of papering over it.

**Reference arrays have no element type on the heap.** `anewarray` stores only
`ArrayKind::Reference`, so an `Object[]` holding an `Integer` can be copied into a `String[]`
without the per-element `ArrayStoreException` the Javadoc promises. That check needs the element
descriptor S11 is adding; `aastore` has the same gap today.

**`boolean[]` and `byte[]` are the same storage but different types.** `ArrayKind::accepts` (what
`baload`/`bastore` use) deliberately lets a byte instruction work on a boolean array, as JVMS §2.2
does. `System.arraycopy` must not be that lenient — the JDK throws `ArrayStoreException` for a
`byte[]` into a `boolean[]` — so the check is equality, not `accepts`.

**The borrow constraint pushes toward the specification.** Two arrays cannot be borrowed mutably
through the same `&mut Shared`, so the two-array path has to go through a temporary anyway — which
is exactly the Javadoc's "as if the components … were first copied to a temporary array". The
same-array path uses `Vec::copy_within` (`memmove`), which is overlap-safe in both directions.

## What the specification says

The JDK 21 `System.arraycopy` contract, in the order the refusals are listed: `NullPointerException`
if either array is null; `ArrayStoreException` if the two arrays have different primitive component
types (or either argument is not an array); `IndexOutOfBoundsException` unless
`0 <= srcPos`, `0 <= destPos`, `0 <= length`, `srcPos + length <= src.length` and
`destPos + length <= dest.length`. An overlapping self-copy is specified to behave "as if the
components at positions `srcPos` through `srcPos + length - 1` were first copied to a temporary
array" — so `copy_within`/`to_vec` is the required semantics, not an optimization. JVMS §2.2 gives
`boolean` byte storage, which is why the storage kinds are easy to confuse and why the Java-level
type rule above is the one that must win.

## The bug I hit

Two, both found by running first:

1. The first test version compared `shared.string_text(...)` with `Some("b")`. It returns
   `Result<String, VmError>`, so the target did not compile (`E0369` in `crates/cli/tests/arraycopy.rs`).
   Fixed with `.expect(...)`: a test bug, not a VM one.
2. The one worth keeping: a native's `VmError::IndexOutOfBounds` never passes through the
   interpreter's `fault`, so the negative-position fixture run under `rava run` printed
   `error: array index -1 out of bounds for length 2` and exited 1 — a VM abort, where the Javadoc
   promises a `java.lang.ArrayIndexOutOfBoundsException`. The acceptance test therefore asserts
   the raw fault, and the follow-up belongs to the slice that owns `interp`: a native's
   `IndexOutOfBounds` needs the same `fault` mapping the `ArrayStore` instruction already gets.

The fixture bytecode itself was right first try (unlike S10's hand-counted `if_acmpne` offset):
`ClassBuilder` owns the constant-pool indices and the test owns only the opcodes, so the earlier
failure mode is gone.

## Measurements

| Fact | Value |
| :--- | ---: |
| `arraycopy` test target | 10 tests (4 success paths, 6 refusal paths) |
| Workspace tests (`cargo test --workspace`) | 92 over 23 suites |
| Native implementation in `crates/native/src/system.rs` | 106 lines (registration + copy + argument/bounds helpers) |
| Mutation: bounds check removed | 9 passed, 1 failed (`a_length_past_the_end_is_rejected`), exit 101 |
| Mutation: kind check removed | 9 passed, 1 failed (`mismatched_array_kinds_are_rejected`), exit 101 |
| Mutation: forward element loop instead of `copy_within` | 9 passed, 1 failed (`an_overlapping_self_copy_moves_as_if_buffered`), left `[0, 1, 0, 1, 0, 1, 0, 7]`, right `[0, 1, 2, 3, 4, 3, 4, 7]`, exit 101 |
| `rava run` on `arraycopy(arr, -1, arr, 0, 1)` | `error: array index -1 out of bounds for length 2`, exit 1 |

The overlap numbers are the point: a forward byte-by-byte copy of `[0..5]` to `[2..7]` overwrites
the source it has not read yet, so the test pins `[0, 1, 2, 3, 4, 3, 4, 7]` and the mutation
produces `[0, 1, 0, 1, 0, 1, 0, 7]`.

## Open threads

- **Route native faults through `fault`.** `System.arraycopy`'s bounds refusal becomes a catchable
  `ArrayIndexOutOfBoundsException` only when the interpreter applies its `fault` mapping to native
  outcomes, as it already does for the `ArrayStore` instruction. Interp-owned; this slice could not
  touch it.
- **Per-element reference checks wait for S11.** `Object[] → String[]` with a non-`String` element
  must throw `ArrayStoreException` at the first offending element, with the partial copy the
  Javadoc describes. That needs the array class model's element descriptor, and the test grows a
  case then (no test was deleted to hide the gap — it was never written).
- **`boolean[]` ↔ `byte[]`** stays an equality check; a test should pin it once S11 gives `Boolean`
  arrays their own identity.
- **The string methods unblocked here** — `concat`, `substring`, `Arrays.copyOf` — still need S11's
  `checkcast [B` and `Unsafe`'s unaligned accessors before their tests can run; `arraycopy` was one
  of their three walls, not the last.
