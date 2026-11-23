# ADR-0006 — Reflection must reproduce JDK 17 metadata, not just dispatch

- **Status:** accepted (2026-11-23)
- **Date:** 2026-11-22
- **Supersedes:** —
- **Superseded by:** —

## Context

Spring's component scan and bean factory do not just call methods. They read class
files and reflective metadata: annotations on classes, methods, fields and parameters;
generic signatures; record components; sealed permits; nestmates; parameter names.
Spring can use ASM to avoid loading a class, but the same metadata must be available
through `Class`, `Field`, `Method`, `Constructor` and `Parameter` once the class is
loaded. If our reflection returns less than JDK 17, injection, validation, AOP pointcut
matching and configuration binding fail in ways that look like application bugs.

## Decision

Reflection reproduces the JDK 17 **shape** for every class in the manifest:

- Class-file attributes retained and exposed: `RuntimeVisibleAnnotations`,
  `RuntimeVisibleParameterAnnotations`, `AnnotationDefault`,
  `RuntimeVisibleTypeAnnotations`, `Signature` (generics), `InnerClasses`,
  `EnclosingMethod`, `NestHost`/`NestMembers`, `Record`, `PermittedSubclasses`,
  `MethodParameters`, `Exceptions`, `SourceFile`.
- `AnnotatedElement` semantics, including annotation proxies, `getAnnotations`,
  `getDeclaredAnnotations`, `getAnnotation`, default values, `equals`/`hashCode` of
  annotation instances and repeatable containers.
- Generic reflection: `Type`, `ParameterizedType`, `TypeVariable`, `WildcardType`,
  `GenericArrayType`, `getGenericSuperclass`, `getGenericInterfaces`,
  `getGenericParameterTypes`, `getGenericReturnType`, `getGenericExceptionTypes`.
- Records and sealed types: `Class.isRecord`, `getRecordComponents`, `isSealed`,
  `getPermittedSubclasses`, record accessor invocation.
- Parameter names: expose `MethodParameters` names when compiled with `-parameters`;
  report `arg0`-style names otherwise, exactly as JDK 17 does.
- Reflection invocation: `Method.invoke`, `Constructor.newInstance`, `Field.get/set`
  for primitives and references, access checks, varargs and boxing rules.

The acceptance artifact is a normalized dump of the reflective view for every
manifest class, compared with JDK 17. Normalization removes ordering, synthetic
members and identity hashes, not annotations or signatures.

## Alternatives

| Alternative | Why rejected |
| :--- | :--- |
| Metadata through ASM only, no reflective API | Spring falls back to reflection for many paths, and third-party code calls `getAnnotation` directly. |
| Implement `Class` but not generic signatures | Spring's `ResolvableType`, dependency injection and message converters use generic types. |
| Copy the JDK's reflection implementation | It is tied to HotSpot internals and licensing; we need the observable contract, not the implementation. |

## Consequences

**Positive**

- Annotation-driven frameworks behave like they do on the reference JDK.
- The metadata diff gives a precise, finite gate for a large surface.
- Record/sealed support reuses the class-file parser rather than special cases.

**Negative / accepted costs**

- The parser must retain raw attributes that the interpreter never reads.
- Annotation proxy identity, default values and repeatable containers carry subtle
  semantics and their own tests.
- Generic signatures are a parser and a type model, not a formatting exercise.

## Acceptance

- `rava compat metadata` produces a normalized diff of 0 for every manifest class.
- Annotation proxy tests cover defaults, repeatable annotations and `equals`/`hashCode`.
- A Spring bean using `@Value`, `@Autowired`, `@Qualifier`, generic injection and a
  record component starts correctly.
