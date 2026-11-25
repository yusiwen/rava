# Fields fixture (issue #53)

`demo.FieldsProbe` is the acceptance fixture for `Class.getFields()`: a `javac
--release 17` class whose hierarchy puts class fields, interface constants, hidden
fields and a diamond in one query.

| Type | Declaration |
| :--- | :--- |
| `FieldsProbe.A` | `public int a`, `public static int A_STATIC` |
| `FieldsProbe.I` | `int I_CONST` (implicitly `public static final`) |
| `FieldsProbe.J extends I` | `int J_CONST` |
| `FieldsProbe.B extends A implements I` | `public int b`, `public int a` (hides `A.a`), `public static int B_STATIC` |
| `FieldsProbe.C extends B implements J` | `public int c`, `public int b` (hides `B.b`), `private int secret` |

JDK 17's order is the class's own public fields in class-file order, then its direct
superinterfaces recursively in declaration order, then its superclass recursively.
Deduplication is by declaration, so `C.b` and `B.b` (and `B.a` and `A.a`) are all
returned, while `I.I_CONST` — reachable through both `C -> J -> I` and `C -> B -> I` —
is returned once. The `private int secret` proves the public filter.

`run()` prints one line for the receiver and one `DeclaringSimpleName.fieldName` line per
returned field. `fieldA()` and `inheritedStatic()` print the declaring class of a hidden
and an unhidden inherited field, so the test pins both `C.getField("a") -> B` (the
most-derived declaration, as on the JDK) and `C.getField("A_STATIC") -> A`.

The source and the compiled classes are committed, so the test needs no JDK. Rebuild and
derive the golden output with JDK 17:

```sh
/home/yusiwen/.local/share/mise/installs/java/zulu-17/bin/javac --release 17 \
  -d crates/interp/tests/fixtures/fields/classes \
  crates/interp/tests/fixtures/fields/src/demo/FieldsProbe.java
/home/yusiwen/.local/share/mise/installs/java/zulu-17/bin/java \
  -cp crates/interp/tests/fixtures/fields/classes demo.FieldsProbe
```

That prints:

```text
C.getFields()
C.c
C.b
J.J_CONST
I.I_CONST
B.b
B.a
B.B_STATIC
A.a
A.A_STATIC
fieldA -> B
inheritedStatic -> A
```

`crates/interp/tests/fields.rs` asserts the ten lines `run()` returns, the two lookup
results, the empty `int.class`/`String[].class` arrays and the missing-superclass/interface
tolerance.
