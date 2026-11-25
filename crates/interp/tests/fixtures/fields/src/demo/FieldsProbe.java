package demo;

import java.lang.reflect.Field;

/**
 * The `Class.getFields()` acceptance fixture.
 *
 * <p>The hierarchy is intentionally two levels deep with both class and interface fields:
 *
 * <pre>
 * Object
 *   A                 (a, A_STATIC)
 *     B extends A implements I   (b, a hides A.a, B_STATIC)
 *       C extends B implements J (c, b hides B.b, private secret)
 *   I                 (I_CONST)
 *     J extends I     (J_CONST)
 * </pre>
 *
 * <p>JDK 17's {@code Class.getFields()} order is: the class's own public fields in
 * class-file order, then its superinterfaces recursively in declaration order, then its
 * superclass recursively. Deduplication is by declaration, so {@code B.a} and
 * {@code A.a} are both returned while {@code I.I_CONST}, reachable through both
 * {@code C -> J -> I} and {@code C -> B -> I}, is returned once.
 */
public final class FieldsProbe {
    private FieldsProbe() {
    }

    public static class A {
        public int a = 1;
        public static int A_STATIC = 2;
    }

    public interface I {
        int I_CONST = 10;
    }

    public interface J extends I {
        int J_CONST = 20;
    }

    public static class B extends A implements I {
        public int b = 3;
        public int a = 4;
        public static int B_STATIC = 5;
    }

    public static class C extends B implements J {
        public int c = 6;
        public int b = 7;
        private int secret = 8;
    }

    /**
     * One line for the receiver, then one line per field as
     * {@code DeclaringSimpleName.fieldName}.
     */
    public static String run() {
        StringBuilder text = new StringBuilder();
        text.append(C.class.getSimpleName()).append(".getFields()\n");
        for (Field field : C.class.getFields()) {
            text.append(field.getDeclaringClass().getSimpleName())
                    .append('.')
                    .append(field.getName())
                    .append('\n');
        }
        return text.toString();
    }

    /** The declaring class of the hidden {@code a} that {@code getField} selects. */
    public static String fieldA() throws NoSuchFieldException {
        return C.class.getField("a").getDeclaringClass().getSimpleName();
    }

    /** The declaring class of an inherited field no subclass hides. */
    public static String inheritedStatic() throws NoSuchFieldException {
        return C.class.getField("A_STATIC").getDeclaringClass().getSimpleName();
    }

    /** The number of public fields of a type; used for the empty primitive/array cases. */
    public static int fieldCount(Class<?> type) {
        return type.getFields().length;
    }

    /** The name of the first public field of a type; used by the tolerance probe. */
    public static String firstName(Class<?> type) {
        return type.getFields()[0].getName();
    }

    /** `int.class.getFields()` is empty: a primitive has no class file. */
    public static int primitiveFieldCount() {
        return int.class.getFields().length;
    }

    /** `String[].class.getFields()` is empty: an array class has no class file. */
    public static int arrayFieldCount() {
        return String[].class.getFields().length;
    }

    /** Prints the golden lines the test asserts, for a stock JDK 17 comparison. */
    public static void main(String[] args) throws Exception {
        System.out.print(run());
        System.out.println("fieldA -> " + fieldA());
        System.out.println("inheritedStatic -> " + inheritedStatic());
    }
}
