# ADR-0005 — Class loading, dynamic code and `Unsafe` capability/fidelity tiers

- **Status:** accepted (2026-11-23)
- **Date:** 2026-11-22
- **Supersedes:** —
- **Superseded by:** —

## Context

Spring generates classes at run time. CGLIB enhances configuration classes and creates
AOP proxies; Objenesis instantiates proxies without running a constructor; Spring's
class loaders define classes from byte arrays. Without `ClassLoader.defineClass`,
`java.lang.reflect.Proxy`, `MethodHandles.Lookup` and a small amount of
`sun.misc.Unsafe`, C2 cannot pass.

The `Unsafe` part initially looked like an architectural conflict. Our heap represents
references as indices into a VM-owned table, and fields as a vector of `Value` slots;
`Unsafe.objectFieldOffset` exposes a byte offset that a client may add to, pass
across objects, or use for type punning. A token cannot honestly reproduce that.

A source audit of the target stack changes the risk assessment. The audit was
performed against Spring Framework 6.2.0, Spring Boot 3.4.0 and Tomcat 10.1.34; it is
repeated for the pinned 6.2.x/3.5.x patch at M1. Spring repackages Objenesis as
`org.springframework.objenesis`; the Objenesis links point at the upstream 3.3
sources:

| Component | `Unsafe` use | Needs an address? |
| :--- | :--- | :--- |
| Objenesis [`UnsafeUtils`](https://github.com/easymock/objenesis/blob/3.3/main/src/main/java/org/objenesis/instantiator/util/UnsafeUtils.java) | reads the private static `theUnsafe` field | no |
| Objenesis [`UnsafeFactoryInstantiator`](https://github.com/easymock/objenesis/blob/3.3/main/src/main/java/org/objenesis/instantiator/sun/UnsafeFactoryInstantiator.java) | `allocateInstance(Class)` | no |
| CGLIB [`ReflectUtils`](https://github.com/spring-projects/spring-framework/blob/v6.2.0/spring-core/src/main/java/org/springframework/cglib/core/ReflectUtils.java) | `MethodHandles.privateLookupIn` + `Lookup.defineClass`, fallback `ClassLoader.defineClass` | no |
| Tomcat [`ByteBufferUtils`](https://github.com/apache/tomcat/blob/10.1.34/java/org/apache/tomcat/util/buf/ByteBufferUtils.java) | reflective `invokeCleaner(ByteBuffer)`; static init calls `allocateDirect(0)` | no |
| Tomcat [`SocketProperties`](https://github.com/apache/tomcat/blob/10.1.34/java/org/apache/tomcat/util/net/SocketProperties.java) | `directBuffer = false` by default | — |

Across those sources, `sun.misc.Unsafe` appears in three files; `allocateInstance`
has one call site and `invokeCleaner` three. There is **no** `objectFieldOffset`,
`getInt`/`putInt`, `allocateMemory`, `copyMemory`, `getAddress`, `compareAndSwap` or
`addressSize` call in the audited stack. The offset problem is real, but it belongs to
Netty/Hibernate/off-heap libraries, not to Spring's own runtime path.

[`StdInstantiatorStrategy`](https://github.com/easymock/objenesis/blob/3.3/main/src/main/java/org/objenesis/strategy/StdInstantiatorStrategy.java) also selects the Objenesis path from
`java.vm.name`: names starting with `Java HotSpot` or `OpenJDK` use
`ReflectionFactory`, everything else falls back to `Unsafe.allocateInstance`.

## Decision

**Class loading.** Implement a loader hierarchy (bootstrap, platform, application),
parent delegation, `Class.forName`, `loadClass`, `findClass`, protected/public
`defineClass`, and `getResource`/`getResources`. Package access and protection domains
are checked by the VM, not bypassed.

**Dynamic code.** Implement:

- `java.lang.reflect.Proxy` and `InvocationHandler`, including default-method
  dispatch.
- `MethodHandles.Lookup`: `lookup`, `privateLookupIn`, `findVirtual`, `findStatic`,
  `findSpecial`, `findConstructor`, `unreflect`, and `defineClass`.
- `ClassLoader.defineClass` (protected) and the `publicDefineClass` hook CGLIB probes.
- `sun.reflect.ReflectionFactory.newConstructorForSerialization` for libraries that
  call it directly; Objenesis uses it when `java.vm.name` selects that path.

**Unsafe capability tiers.** `sun.misc.Unsafe` is implemented in tiers. A tier opens
only when the manifest or a later target needs it; methods outside the open tiers
throw `UnsupportedOperationException` naming the method — a tripwire, not silent
corruption.

| Tier | Surface | Semantics | Opens when |
| :--- | :--- | :--- | :--- |
| **U0** | `theUnsafe`, `getUnsafe`, `allocateInstance`, `ensureClassInitialized`, `shouldBeInitialized`, `invokeCleaner` | class/instance/cleaner; no addresses | M3; required by the servlet target (Q8) |
| **U1** | `objectFieldOffset`, `staticFieldOffset`/`staticFieldBase`, `arrayBaseOffset`/`arrayIndexScale`, object/array `get*`/`put*` | VM token offsets; no cross-field arithmetic, no type punning | a manifest entry calls it |
| **U2** | CAS, `getAndAdd*`, volatile/acquire/release accessors, `loadFence`/`storeFence`/`fullFence` | the JMM on token access | after M5 concurrency |
| **U3** | `allocateMemory`, `reallocateMemory`, `freeMemory`, `setMemory`, `copyMemory`, `getAddress`, `putAddress`, `addressSize` | S1 fake arena in the first target; S2-raw/S2-objects later (see fidelity levels) | reactive/Netty target, or a manifest entry |
| **U4** | `defineClass`, `park`/`unpark`, `throwException` | class loading and thread primitives | a manifest entry calls it |

**U0 is the first-target subset.** It is the only tier the servlet target promises:

- `theUnsafe` field and `getUnsafe` (with the caller check);
- `allocateInstance(Class)` — allocate with default field values, ensure class
  initialization, skip the constructor;
- `invokeCleaner(ByteBuffer)` — release a VM-managed direct buffer, or no-op when the
  buffer is GC-managed;
- `ensureClassInitialized` / `shouldBeInitialized`.

**U1 is token-based.** Offsets are VM tokens (field slot or array index), not byte
addresses. The token contract excludes cross-field arithmetic, type punning and raw
addresses. U1 opens only when a manifest entry calls it; it is not implemented
speculatively.

**Fidelity levels S0/S1/S2 are per family.** The method set is the same; the levels
describe how much of HotSpot's contract a family honours:

- **S0 — API shape.** The method exists with the right signature and links; an
  unimplemented method throws `UnsupportedOperationException` naming itself.
- **S1 — declared semantics.** The family behaves as this project documents, with no
  promise of real offsets or addresses; tokens and VM-managed arenas are allowed.
  S1 values are opaque and not stable across runs or versions.
- **S2 — faithful semantics for that family.** Real byte offsets or addresses, and the
  arithmetic/type-punning the family defines.

| Family | S1 | S2 | Upgrade rule |
| :--- | :--- | :--- | :--- |
| Field/array offsets + `get`/`put`/CAS | VM tokens | real byte offsets and layout | the whole family upgrades together |
| Raw memory (`allocateMemory`, `copyMemory`, `getAddress`, `addressSize`) | fake arena | real memory/arena | independent of the field family; `copyMemory`/`getAddress` must agree |
| Atomics and memory ordering | declared semantics | the JMM's orderings and fences | independent; no layout change |
| Class definition, `park`/`unpark`, `throwException` | declared semantics | fuller semantics | independent |

**Same method, one level.** A runtime profile declares one level per family; a method
cannot be S1 and S2 at once. Families that share an address space upgrade together, or
a bridge is defined.

**The first target is S1.** U0–U4 as opened are S1; U3 uses a fake address arena. No
S2 family is promised by the first target.

**S2-raw and S2-objects are different decisions.** S2-raw is a real VM-owned memory
arena for direct buffers and raw memory; it can be added without changing object
layout and is the likely first need for JNI/Netty. S2-objects is real byte offsets into
a real object layout; it requires a superseding ADR and a re-examination of ADR-0004's
index heap and the collector. JNI does not by itself require S2-objects: its direct
buffers need S2-raw, while object field access goes through JNI functions.

**S1 → S2 is a major profile, not a patch or a runtime toggle.** API/binary signatures
stay compatible; well-behaved clients keep working; offset/address values change (S1
values were never stable); S2-objects changes layout and GC and is architecturally
breaking. A profile is chosen at build/launch and reported per family.

`jdk.internal.misc.Unsafe` and `VarHandle` are separate APIs, not part of S2; they get
their own decision if a manifest needs them.

**Direct buffers.** `ByteBuffer.allocateDirect` must exist even with Tomcat's
`directBuffer=false`, because `ByteBufferUtils`' class initialization calls
`allocateDirect(0)`. Buffers are VM-managed; `invokeCleaner` is a no-op or a GC hook,
never an address.

**Verification.** Generated classes pass the same verifier as loaded classes. Disabling
verification for "trusted" generated code would make C2 a lie.

## Alternatives

| Alternative | Why rejected |
| :--- | :--- |
| Implement full `sun.misc.Unsafe` first | No manifest evidence; it forces the U3 address-model decision before the application loop works, and raw memory is weeks of work. The tiers deliver U0 first and defer the rest. |
| Freeze at U0 forever | The reactive/Netty target and other libraries need U1–U3; a permanent stop would make the reactive goal unreachable. |
| No `Unsafe` at all; report `java.vm.name=OpenJDK` so Objenesis uses `ReflectionFactory` | Reporting a false VM name to steer one library is fragile, and other libraries call `Unsafe` directly. U0 is honest and small. |
| Implement token offsets now | Not needed by the audited target; adds a compatibility promise (arithmetic, type punning) that the token cannot keep. Defer to U1 when the manifest demands it. |
| Let CGLIB fall back to `ClassLoader.defineClass` reflection only | On JDK 17 this needs `--add-opens` and fails the "unmodified application" criterion; `Lookup.defineClass` is the preferred path. |

## Consequences

**Positive**

- U0 is small and provably address-free for the target.
- The hard C2 work is correctly located in `MethodHandles`/`defineClass`, not in
  object layout.
- Unsupported `Unsafe` methods fail at the call site with a name, which is actionable.

**Negative / accepted costs**

- `MethodHandles.Lookup` is a substantial subsystem; `privateLookupIn` and
  `defineClass` carry access-control and package semantics that must be right.
- A token later cannot honestly reproduce offset arithmetic; any dependency that
  relies on it is a scope decision, not a bug fix.
- S2 is a separate architectural decision: S2-raw (a real arena) and S2-objects (a real
  layout) have different consequences for the collector and for native interop.
- `java.vm.name` becomes a compatibility property; it is `rava` per ADR-0011.
- Direct buffers are VM-managed, so their lifetime is the collector's, not
  `Unsafe.invokeCleaner`'s.

## Acceptance

- The C2 suite (Proxy, CGLIB enhancement, Objenesis, AOP proxy) exits 0.
- A runtime `Unsafe` call log contains only the open tier's methods (U0 at M3); any
  other method fails the test.
- The active fidelity level is reported per family; the first target is S1.
- No U1+ method is implemented without a manifest entry and a test that calls it.
- Generated classes verify; no bypass flag exists.
- The reference application creates an AOP proxy and a `@Configuration` CGLIB subclass
  without a tripwire.
