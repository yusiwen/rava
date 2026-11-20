# S18 — The Unsafe memory model, and how close M4's reflection really was

- **Slice:** S18
- **Requirements:** R12 (the library's VM-facing primitives)
- **Acceptance:** `cargo test -p rava-cli --test unsafe` exited 0 on 2026-11-20 (4 tests).

## What surprised me

**`Unsafe` is an instance API.** Every accessor is an instance method, so the receiver is
argument 0: `getInt(object, offset)` reaches the native as `(this, object, offset)`. The first
registration assumed the static shape, and the direct tests passed because they called the
functions with the arguments the functions expected rather than the ones the library sends. The
`Class.getInterfaces` probe found it immediately (`expected 2 argument(s), got 3`), which is the
argument for probing the real caller, not only the unit.

**`Class$Atomic` is two natives away, not a VarHandle.** The earlier trace made it look like
reflection would drag in `VarHandle`/`MethodHandles`. Its static initializer uses
`Unsafe.getUnsafe()`, `objectFieldOffset(Class, String)` and `compareAndSetReference` directly —
so `objectFieldOffset` returning the field slot and a reference CAS over that slot is enough for
`Class.reflectionData` to install. After this slice the next wall was `getInterfaces0` (a native
S13 left out), not the atomic machinery.

**There are no unaligned natives.** `getIntUnaligned`/`getLongUnaligned` are Java: they compose
aligned `getShort`/`getByte` calls. A byte view over the array elements — `base = 0`,
`scale = element width`, so an offset is a byte offset — is the whole model they need.

## What the specification says

`Unsafe`'s offsets are implementation-private; the library only computes `base + index * scale`
and passes the result back. This VM defines: on an instance, the offset is the loader's field
slot; on a primitive array, it is a byte offset into the element storage; on a reference array, it
is an element index (the scale is 1 because the elements are heap references). `arrayBaseOffset`
is 0 and `arrayIndexScale` is the element width, which is what `ArraysSupport` reads in its
`<clinit>` and shifts by.

## The bug I hit

Two, both about shape rather than logic:

1. The receiver arity above — fixed by giving every accessor `expect_args::<3>`/`<4>` and the CAS
   pair `<5>`, with a `no_op_with_receiver` for the writeback/fence stubs.
2. The byte view's width: `read_bytes` first returned `scale(array.kind)` bytes, which is wrong
   for `getInt` on a `byte[]` (4 bytes, not 1). It now takes the accessor's width.

## Measurements

| Fact | Value |
| :--- | ---: |
| Unsafe natives declared in JDK 21 | 68 |
| Registered by this slice | 36 accessors + 6 CAS + 2 offsets + 2 geometry + allocate + throw + 4 no-ops |
| Unsafe test target | 4 tests (field slots, byte-addressed arrays, reference CAS, allocate/throw) |
| Workspace tests | 103 |

## Open threads

- Volatile accessors and fences are the plain/absent versions: one thread, so there is nothing to
  order against. M3's thread slice must revisit them.
- `getUncompressedObject`, `defineClass0`, `park`/`unpark`, the off-heap `allocateMemory` family
  and `staticFieldOffset`/`Base` are still missing; none is on the G2/#28/#21 paths.
- `Class.getInterfaces` now reaches `getInterfaces0`, which is not implemented yet — the next
  slice.
