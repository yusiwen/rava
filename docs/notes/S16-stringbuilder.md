# S16 — StringBuilder was already working; the VM's shortcut was not

- **Slice:** S16
- **Requirements:** R4
- **Acceptance:** `cargo test -p rava-cli --test stringbuilder` exited 0 on 2026-11-20 (2 tests).

## What surprised me

**No implementation was needed.** With strings as instances (S10) and `System.arraycopy` (S14),
`AbstractStringBuilder`'s Java code works as written: the constructor allocates its `byte[]
value`, `append` grows through `Arrays.copyOf`, and `toString` calls the host `String`
constructor. The first probe — `new StringBuilder().append("hello").append(' ').append(42)
.toString()` — returned `"hello 42"` before any change to the VM.

**The old shortcut was dead.** `HeapObject::Builder` (a `StringBuilder` held as a Rust string)
was left from the stub-library era; nothing constructed it, and `Shared::object_factory` — the
hook that would have — was always `None`. The slice became a removal: the variant, the factory
field and type, the `Instr::new` hook and the two `builder`/`builder_mut` helpers are gone, so
`new` has one path again.

**A dangling registration came out of the previous slice.** S11 registered a
`Class.getComponentType` native, but JDK 21 declares that method in Java (it reads the final
`Class.componentType` field); the registration could never run and `rava natives` reported it as
"registered, but nothing declares it". It is removed here, and the model fix S11 made (filling
`componentType` when the type class object is created) is what actually answers the call.

## What the specification says

`StringBuilder` is an ordinary class extending `AbstractStringBuilder` with a `byte[] value`, a
`coder` and a `count`; nothing about it is VM-specific. The VM's only job is to run its
constructors and methods, which it does once objects have real fields and `System.arraycopy`
exists.

## The bug I hit

The dangling registration above was found by running `rava natives java.lang.Class` and reading
the summary (`1 dangling`); no test covered the reverse check on the real image. The same run
would have caught it earlier had it been in CI — noted as an open thread.

## Measurements

| Fact | Value |
| :--- | ---: |
| Builder test target | 2 tests (append+toString; growth past the first capacity) |
| Growth case | 25 appends of ten digits → 250 chars, every byte survives |
| Workspace tests | 95 (before the cleanup) |
| Removed | `HeapObject::Builder`, `ObjectFactory`, `object_factory`, the `new` hook, `builder`/`builder_mut`, one dangling native |

## Open threads

- The `rava natives` reverse check is not run in CI; a registered native the library does not
  declare stays invisible until someone runs the tool. A check in `xtask` would need the image
  and the full walk (about half a second), so it is feasible; no issue is filed for it yet.
- `StringBuilder`'s `insert`/`reverse`/`delete` are untested; they use the same fields and
  `arraycopy`, so they are expected to work, but the fixture only pins `append` and `toString`.
