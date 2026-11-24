# Method-flags fixture (M2.28)

`demo.FlagsProbe` is the acceptance fixture for slice M2.28: a Java 17 class whose
compiler-generated members carry the class-file predicate flags
`crates/interp/tests/method_flags.rs` asserts.

| Member | Class-file flag |
| :--- | :--- |
| `Object get()` | `ACC_PUBLIC \| ACC_BRIDGE \| ACC_SYNTHETIC` (`0x1041`), the bridge javac adds for `Getter<String>.get()` |
| `int sum(int...)` | `ACC_PUBLIC \| ACC_STATIC \| ACC_VARARGS` (`0x0089`) |
| `FlagsProbe(int...)` | `ACC_PUBLIC \| ACC_VARARGS` (`0x0081`); `Constructor.getModifiers()` masks it to `0x0001` |
| `void lambda$counter$0()` | `ACC_PRIVATE \| ACC_SYNTHETIC` (`0x1002`), the lambda body of `counter()` |
| `Kind.$VALUES` | `ACC_PRIVATE \| ACC_STATIC \| ACC_FINAL \| ACC_SYNTHETIC` (`0x101A`) |
| `Kind.$values()` | `ACC_PRIVATE \| ACC_STATIC \| ACC_SYNTHETIC` (`0x100A`) |

`Getter` is the generic interface the bridge implements. The source and the compiled
classes are committed so the test needs no JDK.

The fixture also carries the Spring blockers M2.28 fixed: a `LinkedHashSet(Collection)`
that deduplicates (including `null`), an `ArrayList(Collection)` that keeps order and
duplicates, `removeAll`/`retainAll`/`containsAll` calls, and a `Comparator` subclass whose
`compare` is declared on its superclass (the shape of Spring's
`AnnotationAwareOrderComparator`/`OrderComparator`) so `List.sort` must resolve the
callback up the superclass chain.

Rebuild with:

```sh
javac --release 17 \
  -d crates/interp/tests/fixtures/method_flags/classes \
  crates/interp/tests/fixtures/method_flags/src/demo/Getter.java \
  crates/interp/tests/fixtures/method_flags/src/demo/FlagsProbe.java
```
