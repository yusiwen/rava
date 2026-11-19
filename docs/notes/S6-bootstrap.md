# S6 — Five classes, sixteen, and 455 bodies

- **Slice:** S6
- **Requirements:** R4
- **Acceptance:** `cargo test -p rava-vm --test bootstrap` exited 0 on 2026-11-20 (4 tests).

## What surprised me

**Five classes is a closure of sixteen.** `String` implements `Comparable`, `CharSequence`,
`Serializable`, `Constable` and `ConstantDesc`; `Class` implements `Serializable`,
`GenericDeclaration`, `Type`, `AnnotatedElement` and two nested `TypeDescriptor` interfaces;
`Thread` implements `Runnable`. The loader resolves each superclass and interface on the way,
so "bootstrap the five" is 16 classes, 543 methods and 455 decoded bodies — and every one of
them passed the verifier, which is what S5's rewrite to slots bought.

**The native surface arrives before any program runs.** Those 16 classes declare 71 native
methods: `Object` 6, `System` 9, `Thread` 20, `Class` 35. The 35 in `Class` are reflection
(`getName0`, `getDeclaredFields0`, …) and nothing on the boot path calls them, but their count
is the reason #2 (the report of declared-versus-registered natives) is worth building before
S7 rather than after.

**Initialization is not loading.** S6 covers loading and linking — verification and prepared
statics — and deliberately stops there. `<clinit>` is where the natives are needed: `System`'s
static initializer calls `initPhase1`, and that is exactly what S7 supplies.

## What the specification says

JVMS §5.3 splits a class's life into loading, linking and initialization. This slice ends after
linking; §2.4's default values for statics are what the loader prepares, and R4's "without a
substitute" is the property the test pins — every loaded class has a class file from the image,
and there is exactly one `java/lang/Object`.

## The bug I hit

No loader bug: the classes loaded on the first run, which was the surprise. What the slice did
find was a documentation lie. `Class.file` was documented as "`None` only for the synthesized
`java/lang/Object`", a class S2 deleted; nothing in the tree sets it to `None` any more. The
comment now says the truth, and the vestigial `Option` is recorded as an open thread below
rather than changed in a bootstrap slice.

## Measurements

| Class | Methods | With code | Natives |
| :--- | ---: | ---: | ---: |
| `java.lang.Object` | 13 | 7 | 6 |
| `java.lang.String` | 168 | 167 | 1 |
| `java.lang.Class` | 171 | 136 | 35 |
| `java.lang.System` | 48 | 39 | 9 |
| `java.lang.Thread` | 114 | 94 | 20 |
| closure of the five | 543 | 455 | 71 |

The closure is 16 classes; the test asserts every one came from the image and that no class is
synthesized.

## Open threads

- **Initialization** (`<clinit>`, JVMS §5.5) is S7/S8 work: `System.initPhase1` and
  `String`'s static comparator are the first users of the natives.
- **`Class.file` is an `Option` that is always `Some`.** Making it a plain `ClassFile` would
  delete six error paths in resolution; it is a refactor, not a fix, so it waits for an issue.
- **The 71 natives are declared, none is registered.** The registry is still empty by design
  (`register_jcl_natives`); S7 starts filling it, and #2's report will measure the gap.
