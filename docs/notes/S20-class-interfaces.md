# S20 — `Class.getInterfaces`, and the clone behind the public method

- **Slice:** S20
- **Requirements:** R4
- **Acceptance:** `cargo test -p rava-cli --test class_interfaces` exited 0 on 2026-11-20 (4 tests).

## What surprised me

**The public method is a wrapper with a clone.** `Class.getInterfaces()` calls
`getInterfaces(boolean cloneArray)`, which calls the private native `getInterfaces0()` and then
`clone()`s the result. The native is what this slice can pin; the public call needs array `clone`
— the stack-trace slice's (S19) — because a `[Ljava/lang/Class;` clone reaches
`invokevirtual [Ljava/lang/Class;.clone()`, and `load_class` has no class file for an array
descriptor.

**The class-file writer had no interfaces.** The fixtures could declare fields, methods and (since
S11) an array class's descriptor, but `ClassBuilder::build` wrote `interfaces_count = 0`. A class
that implements an interface could not be built; the builder now has `interface(name)`.

**The host image is a better oracle than a fixture for some facts.** `java.lang.String` implements
five interfaces in JDK 21 (`Serializable`, `Comparable`, `CharSequence`, `Constable`,
`ConstantDesc`); the test asserts the count and two names, which is a host-data check a fixture
cannot give.

## What the specification says

`getInterfaces` returns the direct superinterfaces in declaration order (JVMS §4.1's
`interfaces` table), as class objects; `null` is not possible. A primitive or array class has no
`interfaces` table. A real JVM reports `Cloneable` and `Serializable` for an array class; this
slice returns none for `ClassTarget::Type`, which is recorded as a limit rather than hidden.

## The bug I hit

Two:

1. The first `getInterfaces0` call through the host method failed with `class not found: [B` from
   `invokevirtual` — the array-`clone` wall above, not a bug in the native.
2. The public-method test then failed with `method not found: HasIface.main` even though the
   fixture declares it: the three earlier tests shared one temp directory (keyed only by the
   process id), so parallel execution overwrote each other's class file. Each test now names its
   own directory.

## Measurements

| Fact | Value |
| :--- | ---: |
| `getInterfaces0` test target | 4 tests (fixture class, host `String`, array class, public `getInterfaces`) |
| `String`'s interfaces in the host image | 5 |
| Builder addition | `ClassBuilder::interface(name)` + the `interfaces` table in `build` |
| Workspace tests | 109 |

## Open threads

- The public `getInterfaces()` now runs end to end (S19's array `clone` landed before this
  merged).
- `ClassTarget::Type` reports no interfaces; a real JVM reports `Cloneable`/`Serializable` for an
  array class. If a program depends on that, it is a small follow-up.
- `getDeclaredFields`/`getDeclaredMethods` now have the reflection data machinery (S18) but no
  native yet; they are the next reflection step, not part of #28's acceptance.
