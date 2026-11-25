# Reflection and metadata: from a class file to a reflected object

## Question

How does a C1 class-file record become a runtime `Class`, member, annotation or generic
`Type`, and which reflection calls are actually implemented?

## 1. The source of truth: the C1 metadata model

The runtime does not parse reflection signatures in the VM. It starts from the class
file parsed by `rava-classfile`; `ClassFile::metadata()` in
[`crates/classfile/src/metadata/`](../../crates/classfile/src/metadata/) decodes the C1
attributes into a typed model:

- class: access flags, binary name, superclass, interfaces, `Signature`;
- visible/invisible annotations, parameter annotations and type annotations, with every
  element-value kind;
- fields, methods and constructors with descriptors, signatures, annotations,
  `AnnotationDefault`, `Exceptions` and `MethodParameters`;
- records, permitted subclasses, nest host/members, inner/enclosing classes and
  `SourceFile` when present.

Every original attribute payload is also retained as a raw attribute, so an attribute the
typed model does not decode is not lost. `ClassEntry::metadata()` is the VM's on-demand
view; decoding is not cached on the entry.

The C1 stubs under `stdlib/c1/classes/` are class files with no `Code`. They therefore
provide metadata and class shape, not executable bodies. The C1 diff and its 1,011/36
boundary are explained in [`c1-metadata.md`](c1-metadata.md).

## 2. Class objects

A loaded class is a `ClassId` in the `(loader, name)` table. Three heap shapes represent
a class object:

- `HeapObject::Class(ClassId)` — a named class;
- `HeapObject::PrimitiveClass(PrimitiveKind)` — one of the nine primitives;
- `HeapObject::ArrayClass(ArrayClass)` — an array class, keyed by its component
  `ClassRef` (named, primitive or nested array).

`Shared::class_object(ClassId)` returns one cached object per `ClassId`;
`Shared::class_object_for_ref(&ClassRef)` dispatches to the named, primitive or array
cache. The caches are permanent roots, so `Class.forName(...) == Class.forName(...)` is
reference-identical for the same identity. `Shared::get_class(reference)` maps an
ordinary instance to its defining class, a boxed value to its wrapper class, an array to
its array class, a class object to `java.lang.Class`, and a lambda to its functional
interface.

`Class` natives resolve their receiver through these caches:
`getName`, `getModifiers`, `isInterface`, `isPrimitive`, `isArray`,
`getSuperclass`, `getComponentType`, `getClassLoader`, `forName` and `getClasses` are
registered in [`crates/vm/src/reflection.rs`](../../crates/vm/src/reflection.rs).
`getSuperclass` is null for `Object`, interfaces and primitives; an array's superclass is
`Object`.

## 3. Member objects

A reflected member is `HeapObject::Reflect(ReflectMember)`:

```
ReflectMember {
    kind: Field | Method | Constructor,
    class: ClassId,
    name: String,
    descriptor: String,
}
```

`Shared::reflect_member` caches one object per
`(class, kind, name, descriptor)`, and `Shared::roots` reports the cache. The member's
class is its declaring class, so the object is stable across repeated queries.

`Class.getDeclaredFields()`, `getDeclaredMethods()` and
`getDeclaredConstructors()` filter the class's own fields/methods (constructors are
`<init>`) and return real heap reference arrays of member objects. They do not include
superclass members. The inherited arrays are `getFields()` (issue #53), `getMethods()`
and `getConstructors()`: `getFields` returns the class's public fields in class-file
order, then its superinterfaces recursively in declaration order, then its superclass
recursively, deduplicated by `(declaring class, name, descriptor)`; a hidden field is
kept, a diamond declaration is returned once, and a primitive or array class returns an
empty array. `getClasses()` returns public member classes from the `InnerClasses`
metadata, with the superclass walk; `getDeclaredClasses()` reports the declared subset.

The named lookups build the exact descriptor from their `Class[]` argument:

- `getDeclaredField`/`getField` match a field by name;
- `getDeclaredMethod`/`getMethod` match name plus parameter descriptors;
- `getDeclaredConstructor`/`getConstructor` match `<init>` plus parameter descriptors.

`getField`/`getMethod` walk the superclass then interface hierarchy; constructors are
not inherited. A miss throws a real `java.lang.NoSuchFieldException` or
`java.lang.NoSuchMethodException` (M2.30), not `null`. The message names the receiver,
member and parameter descriptors.

Member natives read the cached metadata rather than a second copy:

- `Field.getName`/`getModifiers`/`getDeclaringClass`/`getType`;
- `Method.getName`/`getModifiers`/`getDeclaringClass`/`getReturnType`/
  `getParameterTypes`/`getParameterCount`/`isBridge`/`isVarArgs`/`isSynthetic`;
- `Constructor.getName` (binary class name)/`getModifiers`/`getDeclaringClass`/
  `getParameterTypes`/`getParameterCount`/`isVarArgs`/`isSynthetic`.

Primitive and array descriptors resolve to the existing primitive/array `Class` objects;
a missing or malformed descriptor is a typed error. `getParameterTypes` returns a
`Class[]`; **`getParameters` and `java.lang.reflect.Parameter` are not implemented**.

## 4. Annotations

The annotation metadata from the class file becomes `HeapObject::Annotation`:

```
AnnotationObject {
    class: ClassId,                 // the annotation type
    values: decoded element values  // string, primitive, Class, enum,
                                    // nested annotation or array
}
```

`Class`, `Field`, `Method` and `Constructor` answer `getDeclaredAnnotations`,
`getAnnotations` and `getAnnotation(Class)`; `AccessibleObject` registers the inherited
member-declaration forms. The declared query preserves class-file order and returns an
empty array for none; the single query returns null for a missing annotation. Element
defaults come from the annotation type's `AnnotationDefault` when the use site omits a
value. `Class.getAnnotations` merges superclass annotations only for annotation types
carrying `@Inherited`; the nearest declaration wins.

A receiver that is an annotation object is intercepted before ordinary method lookup:
`annotationType`, the element methods, `equals`, `hashCode` and `toString` are answered
by [`crates/vm/src/annotation.rs`](../../crates/vm/src/annotation.rs). Enum constants
used as values are cached `HeapObject::Enum` objects; `Enum.name`/`toString` are natives.
`getAnnotationsByType` (repeatable containers), runtime-invisible annotations,
parameter/type annotations as reflection objects and annotation proxy identity caching
are not implemented.

## 5. Generic types

`Class.getGenericSuperclass`, `getGenericInterfaces`, `getTypeParameters`,
`Field.getGenericType`, `Method.getGenericReturnType`/`getGenericParameterTypes`/
`getTypeParameters`, and the constructor variants are registered in
[`crates/vm/src/generic.rs`](../../crates/vm/src/generic.rs).

The flow is:

1. The class/method/field `Signature` string is parsed by
   [`crates/classfile/src/metadata/signature.rs`](../../crates/classfile/src/metadata/signature.rs)
   into a JVMS type model (class/parameterized/type-variable/wildcard/array, bounds,
   nested owners, throws).
2. A raw type resolves to an existing `Class`/primitive/array class object.
3. Parameterized, type-variable, wildcard and generic-array types become
   `HeapObject::Type(TypeObject)`, structurally cached by `Shared::type_object`, with
   the cache and its nested references enumerated as roots.
4. `Type.getTypeName`, `ParameterizedType.getActualTypeArguments`/`getRawType`/
   `getOwnerType`, `TypeVariable.getName`/`getBounds`/`getGenericDeclaration`,
   `WildcardType.getUpperBounds`/`getLowerBounds` and
   `GenericArrayType.getGenericComponentType` answer from that graph.

`getClass()` on a type object reports its public `java.lang.reflect` interface, not a
private JDK implementation class. Record/sealed metadata is retained by the class-file
model, but `getRecordComponents`/`getPermittedSubclasses` and a record/sealed reflective
view are not implemented; `Class.isRecord`/`isSealed` currently answer false.

## 6. Reflective invocation and field access

`Field.get`/`set` and the `Method.invoke`/`Constructor.newInstance` natives live partly
outside the VM:

- [`crates/vm/src/reflection.rs`](../../crates/vm/src/reflection.rs) implements
  `Field.get`/`set` for instance fields, boxing/unboxing primitive values and preserving
  reference identity. Static fields and access checks are not handled here.
- [`crates/interp/src/reflect_invoke.rs`](../../crates/interp/src/reflect_invoke.rs)
  implements `Method.invoke` and `Constructor.newInstance`: it resolves the reflected
  name and descriptor exactly (no overload search), unboxes/adapts arguments with JLS
  widening, calls the existing static/instance execution path, boxes a primitive result
  and passes a reference result through.
- A Java `Thrown` from the target is returned as `InterpError::Thrown`; there is no
  `InvocationTargetException` wrapping yet.
- `Method.invoke` does not perform virtual override dispatch or varargs adaptation;
  `Constructor.newInstance` does not enforce abstract/interface/access rules.

## 7. Evidence

```console
$ cargo test -p rava-classfile --test metadata_model
$ cargo test -p rava-vm --test c1_runtime
$ cargo test -p rava-interp --test class_objects
$ cargo test -p rava-interp --test reflection_members
$ cargo test -p rava-interp --test fields
$ cargo test -p rava-interp --test reflection_invoke
$ cargo test -p rava-interp --test annotations
$ cargo test -p rava-interp --test generic_types
$ cargo test -p rava-interp --test primitive_classes
$ cargo test -p rava-interp --test array_classes
$ cargo test -p rava-interp --test member_lookup
$ cargo test -p rava-interp --test method_flags
$ cargo test -p rava-interp --test lookup_exceptions
$ RAVA_SPRING_ACCEPTANCE=1 cargo test -p rava-cli --test spring_context
```

The Spring smoke is the integration proof that these layers compose for a real
`AnnotationConfigApplicationContext`; see [`spring-context.md`](spring-context.md).

## 8. Boundaries

- No live JDK 17 reflection differential; the C1 evidence is a static class-file diff
  (see [`c1-metadata.md`](c1-metadata.md)).
- No `Parameter`/`getParameters`, `AnnotatedType`, `getGenericExceptionTypes`,
  `getRecordComponents`/`getPermittedSubclasses` or record/sealed reflective views.
- The inherited arrays `getFields`/`getMethods`/`getConstructors` exist (issue #53 added
  `getFields`), but they are queries over the **observed** C1 tree, not the closed JDK
  hierarchy: a superclass or interface the tree omits ends that branch instead of failing
  the query. The 69/35/44 closure boundary is in
  [`c1-metadata.md`](c1-metadata.md#c1-closure).
- Runtime-visible annotations only; no `getAnnotationsByType`, runtime-invisible
  annotations, parameter/type-annotation reflection objects, access checks or
  `setAccessible` enforcement (the flag is stored but not enforced).
- Annotation `equals`/`hashCode`/`toString` are value/deterministic implementations, not
  the JDK's exact formulas/identity cache.
- Type `getClass` reports the public interface, not the JDK's private implementation.
- `Field.get/set` is instance-field only; `Method.invoke` has no varargs/overload/virtual
  override; `InvocationTargetException` wrapping is absent.
- Class/primitive/array/type/member caches are permanent roots in this no-collector
  milestone; annotation instances are ordinary heap objects, not identity-cached.

## Owners

- [`crates/classfile/src/metadata/`](../../crates/classfile/src/metadata/) — typed C1 metadata and signatures.
- [`crates/vm/src/reflection.rs`](../../crates/vm/src/reflection.rs) — class/member natives and lookup.
- [`crates/vm/src/annotation.rs`](../../crates/vm/src/annotation.rs) — annotation objects and element dispatch.
- [`crates/vm/src/generic.rs`](../../crates/vm/src/generic.rs) — generic `Type` graph.
- [`crates/vm/src/shared.rs`](../../crates/vm/src/shared.rs) — class/member/type caches and roots.
- [`crates/interp/src/reflect_invoke.rs`](../../crates/interp/src/reflect_invoke.rs) — reflective invocation.
- [ADR-0006](../adr/0006-reflection-and-metadata.md) — C1 shape decision.
- [`PROJECT.md`](../../PROJECT.md) — slice status and evidence counts.
