# S10 — Strings are instances, and a String method is a tour of what is missing

- **Slice:** S10
- **Requirements:** R4 (the library's `String`, not our storage)
- **Acceptance:** `cargo test -p rava-cli --test strings` exited 0 on 2026-11-20 (2 tests).

## What surprised me

**`String.<clinit>` is trivial; `StringLatin1.hashCode` is not.** The initializer sets
`COMPACT_STRINGS`, one empty array and a comparator. But the first interesting method walks a
chain of VM gaps: `StringLatin1.hashCode` calls `ArraysSupport.vectorizedHashCode`, which forces
`jdk.internal.util.ArraysSupport.<clinit>`, which calls `Unsafe.getUnsafe()` and reads
`Unsafe.ARRAY_*_INDEX_SCALE` — so `Unsafe.<clinit>` runs (`registerNatives`,
`arrayBaseOffset0`, `arrayIndexScale0`). With those natives in place, `vectorizedHashCode`
then does `checkcast [B`, and this VM has no array class objects, so `load_class("[B")` fails.
`equals` goes through `ArraysSupport.mismatch`, which uses `Unsafe.getIntUnaligned`; `concat`
and `substring` go through `Arrays.copyOf`, i.e. `System.arraycopy`. Five slices' worth of work
sit behind `"abc".hashCode()`.

**The representation change touches sixteen sites.** `intern`, `ldc` of a `CONSTANT_String`,
`describe_throwable`, the concat recipe, record `equals`/`hashCode`, `Class.getPrimitiveClass`,
the GC and the class lookup. `new_string` now needs the class path, because it loads
`java/lang/String` to get the layout, so string creation is fallible — the two GC unit tests
that used strings moved to arrays so they stay image-free.

## What the specification says

A string is an ordinary object of `java.lang.String` with a `byte[] value` and a `coder` byte
(compact strings), and `ldc` of the same string yields the same object (JVMS §5.3). Nothing
about that needs a VM-private representation; the private Rust string was the deviation.

## The bug I hit

The first version of the test called `length`, `charAt`, `equals`, `hashCode`, `concat` and
`substring`; it failed at `hashCode` with the `Unsafe` wall. Trimming it to what runs today
(`length`, `charAt`, interning) exposed a second, smaller one: the hand-assembled `if_acmpne`
used offset 6 where the layout needed 7, and the decoder reported `truncated code at pc 24` —
the offset landed inside an instruction.

## Measurements

| Fact | Value |
| :--- | ---: |
| Strings test target | 2 tests (length/charAt; literal instance + interning) |
| Natives test target | 23 (one for the `Unsafe` array geometry) |
| Workspace tests | 77 |
| `"hello"` as a `String` | `Instance` of `java/lang/String`, coder 0, five bytes |
| `ldc` of the same literal | one object (interning) |

The wall, measured: `StringLatin1.hashCode` → `ArraysSupport.<clinit>` → `Unsafe.<clinit>` →
`vectorizedHashCode` → `checkcast [B`. `equals` adds `getIntUnaligned`; `concat`/`substring`
add `System.arraycopy`.

## Open threads

- **S11 (arrays) is now the prerequisite for the rest of strings**: array class objects make
  `checkcast [B` resolvable and give `class_of` an answer for arrays.
- After that, `Unsafe`'s unaligned accessors and `System.arraycopy` finish `equals`, `concat`
  and `substring`; the strings test grows those cases then (no test was deleted to hide a
  failure — they were never written).
- `HeapObject::Builder` (a `StringBuilder` held as a Rust string) has the same shape of problem
  and waits for the same array/`Unsafe` work.
