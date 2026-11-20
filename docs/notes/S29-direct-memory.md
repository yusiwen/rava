# S29 — Off-heap direct memory for `DirectByteBuffer` (S29a-1)

- **Slice:** S29 (the direct-memory part, S29a-1)
- **Requirements:** R12
- **Acceptance:** `cargo test -p rava-cli --test direct_memory` exited 0 on 2026-11-21 — a
  `javac`-compiled program allocates two direct buffers, writes/reads `int`, `long`, `double` and
  a byte, and its six output lines match `java` byte for byte.

## What surprised me

**`Unsafe.allocateMemory` was rounding every size to zero.** It calls
`alignToHeapWordSize(bytes)`, which reads `Unsafe.ADDRESS_SIZE`; `Unsafe.<clinit>` copies that from
`UnsafeConstants.ADDRESS_SIZE0`, but `UnsafeConstants.<clinit>` only writes **zeros** and returns —
HotSpot patches those fields at boot. Without the patch `allocateMemory` returned 0 for every
request, so `DirectByteBuffer` got address 0 and the first `putInt` failed at "no direct memory at
0". The bootstrap now patches `UnsafeConstants` (`ADDRESS_SIZE0=8`, `PAGE_SIZE=4096`,
`UNALIGNED_ACCESS=true`) and initializes `Unsafe` afterwards so it copies the patched value.

**Direct buffers address memory with a null base.** The JDK calls `Unsafe.putInt(null, address, x)`
and `getInt(null, address)`; the accessors had no null-base path at all. The VM now keeps off-heap
blocks in a map keyed by base address, and the accessors resolve any address **inside** a block
(the JDK passes `address + index`), so the null-base reads/writes find the right bytes.

**The JDK's image reader cannot run here.** The probe for the *resource* plan
(`jdk.internal.jimage.ImageReader.open`) reached `NativeImageBuffer.getNativeMap`, a JNI method
backed by `libjimage`: JDK 21's `BasicImageReader` maps the image through native code. There is no
pure-Java fallback, so the module-resource map cannot be populated by the JDK's reader — S29b must
be a **VM-side** resource lookup. That is the important result of this slice: the planned
FileChannel path was the wrong prerequisite.

## Measurements

| Fact | Value |
| :--- | ---: |
| Direct-memory test | 1 differential test, 6 output lines |
| Workspace tests | 122 |
| Enablers added | `Unsafe.pageSize`, `AtomicLong.VMSupportsCS8`, `System.mapLibraryName`, `NativeLibraries.findBuiltinLib`, `UnixNativeDispatcher.init`/`getcwd` |

## Open threads

- `FileChannel`/`FileDispatcher`/`mmap` (S29a-2) are not implemented; with the JNI finding they are
  `java.io`/`nio` breadth, not the resource path.
- `Unsafe.copyMemory` handles off-heap blocks only.
- The VM never frees a direct block (`Cleaner.create` would need a thread); leaks are bounded by the
  process lifetime.
