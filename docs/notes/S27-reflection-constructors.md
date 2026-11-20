# S27 — Reflection constructors, and the class-loader tail behind `printf`

- **Slice:** S27
- **Requirements:** R4 (the library's own classes over the VM's class model)
- **Acceptance:** `cargo test -p rava-cli --test reflection` exited 0 on 2026-11-20 — a
  `javac`-compiled program reads `getDeclaredConstructors()`, `getDeclaringClass()`,
  `getParameterCount()` and runs `newInstance()` (no-arg and a boxed `int`), compared with
  `java` byte for byte.

## What surprised me

**A `Constructor` is mostly data.** The host's Java methods read `clazz`, `parameterTypes`,
`exceptionTypes`, `modifiers`, `slot`, `signature`, `annotations` and `parameterAnnotations`
from fields; building real instances from the method descriptor makes `getDeclaredConstructors`,
`getParameterCount`, `getDeclaringClass` and `getModifiers` work with no native per getter. The
VM had a `ConstructorRef(ClassId, usize)` heap object, but it is not a `java.lang.reflect`
instance — the library cannot read it. Real instances replaced it for the reflection path.

**`Constructor.newInstance` is an accessor, not a native.** The JDK generates a
`ConstructorAccessor` (a `MethodHandle`-shaped path this VM does not have). The VM binds the
public `newInstance(Object...)` as an intrinsic: read `clazz`/`slot`, allocate, unbox the
arguments per the descriptor, and invoke `<init>` through the interpreter's call seam.

**`is_type_descriptor` misread a class named `C`.** It treated the single letters `I J F D Z B
C S V` as primitive descriptors, so `ldc class C` resolved to `char` — a user class named `C`
was unloadable. Per JVMS §4.4.1 a `CONSTANT_Class` name is a class/interface name or an array
descriptor; javac writes primitive class literals as `getstatic Integer.TYPE`. The function now
accepts only array descriptors, and the two fixtures that used `ldc "I"` use `Integer.TYPE` (what
`javac` emits).

**The JDK's boot initializes `AccessibleObject`.** Its `<clinit>` registers the
`JavaLangReflectAccess` that `ReflectionFactory` caches in its instance; our VM reached
`ReflectionFactory` first and NPE'd on a null accessor. The bootstrap now initializes
`AccessibleObject` before the streams, like HotSpot's startup.

## The class-loader tail

Getting `Formatter` → `DecimalFormatSymbols` → `LocaleProviderAdapter` as far as resources
needed, in order: the reflection constructors above, then

`Reference.refersTo0`, `Reflection.getClassAccessFlags`, `Unsafe.ensureClassInitialized0`,
`Class.getEnclosingMethod0`/`getDeclaringClass0`/`getSimpleBinaryName0` (null: the VM does not
parse those attributes), `UnixFileSystem.initIDs`/`canonicalize0`/`getBooleanAttributes0`,
`BootLoader.setBootLoaderUnnamedModule0` (no-op), `PreviewFeatures.isPreviewEnabled`,
`Object.clone` for instances (shallow field copy), the `file.separator`/`path.separator`
properties, and GC-root pinning of the constructor graph while it is assembled.

## Where `printf` stands

It now reaches `sun.util.resources.Bundles` and stops at `MissingResourceException`: the locale
provider needs **resource bundles** (`ClassLoader.getResourceAsStream` / `Module.getResourceAsStream`
over the host image), which is M2's resources work. Tracked as #45; #37 stays open, and #43 is
down to fields/methods/annotations.

## Measurements

| Fact | Value |
| :--- | ---: |
| Reflection test | 1 differential test, 5 output lines |
| Workspace tests | 119 |
| Before / after | `getDeclaredConstructors()` NPE → real objects; `newInstance()` works with unboxing |

## Open threads

- Reflection fields/methods/annotations (`getDeclaredFields0`, `getDeclaredMethods0`) and
  `EnclosingMethod`/`InnerClasses` parsing are not implemented (#43).
- `Constructor.newInstance` skips access checks (the constructors the library reflects on are
  public); it also does not wrap exceptions in `InvocationTargetException` yet.
