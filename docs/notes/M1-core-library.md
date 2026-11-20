# M1 — The core library, and the wall every "simple" feature reaches

- **Milestone:** M1
- **Requirements:** R4 (the library's own classes, not substitutes), R5, R6
- **Slices:** S9–S16
- **Closed:** 2026-11-20, by the acceptance commands below (all exit 0, workspace 100 tests)

## What M1 delivers

| Area | Slice | What runs |
| :--- | :--- | :--- |
| Strings | S10, S15 | strings are `java.lang.String` instances (`value`/`coder`); `length`, `charAt`, `hashCode`, `equals`, `compareTo`, `concat`, `substring`; literals intern |
| Arrays | S11, S14 | arrays know their class descriptor (`getClass`, `instanceof`/`checkcast`, `getComponentType`, covariance, `isInstance`/`isAssignableFrom`); `System.arraycopy` for primitives and references, overlap-safe |
| Exceptions | S12, and S15's strings | construct, throw, catch; `getMessage`; `toString`; uncaught report with the captured trace |
| `Class` basics | S9, S13, S11 | `getName`, `desiredAssertionStatus`, `getPrimitiveClass`, `isInstance`, `isAssignableFrom`, `getSuperclass`, `getModifiers`, `isArray`, `isPrimitive`, `isInterface`, `getComponentType` |
| StringBuilder | S16 | the host `AbstractStringBuilder` runs as written; the VM-side shortcut is gone |

Acceptance, all green in `cargo xtask ci`:

```
cargo test -p rava-cli --test strings --test locals     # strings and the slot fix
cargo test -p rava-cli --test arrays --test arraycopy   # arrays
cargo test -p rava-cli --test exceptions                # construct/throw/catch/message/toString
cargo test -p rava-cli --test class_basics --test class_object
cargo test -p rava-cli --test stringbuilder
```

## The measured wall

Every feature that looked like "just run the library's method" reached the same place — the
VM-facing primitives R12 puts in M4:

- `String.hashCode` → `ArraysSupport.<clinit>` → `Unsafe.<clinit>` (array base offsets/scales);
  after S11 removed its `checkcast [B`, the rest is plain array access.
- `String.equals`/`concat`/`substring` → `ArraysSupport.mismatch` (`Unsafe.getIntUnaligned`) and
  `System.arraycopy`.
- `String.concat`'s bare `NullPointerException` was not Unsafe at all: locals were value-indexed,
  so a `long` parameter hid the next one (S15 fixed the slot binding).
- `Class.getInterfaces` → `Class.reflectionData` → `Class$Atomic.<clinit>` →
  `Unsafe.objectFieldOffset` — the object-field half of the model, not just a native.
- `Throwable.getStackTrace` → `StackTraceElement.of(backtrace, depth)`; the trace lives in Rust
  (`Shared::traces`), not in the `Throwable.backtrace` field the JDK expects.

The first, second and third are now solved (S11, S14, S15); the last two are tracked, not
forgotten: #28 (reflection data / `getInterfaces` / `getDeclared*`) and #21 (Java-side stack
traces). Both are M4-shaped: they need the `Unsafe` model, or a decision to synthesise the JDK's
private objects.

## What is deliberately not in M1

- **Printing** is G2 (ADR-0014): `println` reaches `HeapByteBuffer.<clinit>` → `Unsafe`, so it
  follows M4.
- **`Class.getInterfaces`, `getDeclaredFields/Methods`, annotations** — reflection data, #28.
- **`Class.forName`, loaders, `(loader, name)` identity** — M2.
- **`String.format`/Formatter, regex, charset `getBytes`/`new String(byte[])`** — M5 breadth.
- **Java-side throwable stack traces** — #21.

## Lessons for M2 onwards

1. **The host library is not a leaf.** Its own methods class-initialize `Unsafe` and friends
   before they touch a field; a feature estimate that stops at "the class is loaded" is wrong by
   one `Unsafe` model.
2. **The VM's representation choices leak through bytecode.** Value-indexed locals looked
   harmless for two milestones; `long` parameters found it. The verifier's slot count was the
   tell.
3. **A dangling native is found by running the report, not by reading the diff.** S11's
   `getComponentType` registration was invisible until `rava natives` ran; S16 removed it.
