package demo;

import java.lang.reflect.Constructor;
import java.lang.reflect.Executable;
import java.lang.reflect.Field;
import java.lang.reflect.Member;
import java.lang.reflect.Method;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Set;

/**
 * M2.28 acceptance fixture: a Java 17 class whose compiler-generated members carry each
 * class-file predicate flag.
 *
 * <ul>
 *   <li>{@code get()} is the {@code String} implementation of {@code Getter<String>};
 *       javac adds a bridge {@code Object get()} marked {@code ACC_BRIDGE |
 *       ACC_SYNTHETIC}.</li>
 *   <li>{@code sum(int...)} is a varargs method ({@code ACC_VARARGS}), and
 *       {@code FlagsProbe(int...)} a varargs constructor.</li>
 *   <li>{@code counter()} returns a lambda, whose body javac emits as a synthetic
 *       {@code lambda$counter$0} method, and the nested {@code Kind} enum owns the
 *       synthetic {@code $VALUES} field.</li>
 * </ul>
 *
 * The discovery helpers below use the JDK-shaped loops Spring's startup writes (a
 * {@code getDeclaredMethods()} walk filtered by the predicate), while the typed helpers
 * let the Rust test pin the exact flags of one member object.
 */
public class FlagsProbe implements Getter<String> {
    private int hits;

    public FlagsProbe(int... seeds) {
        this.hits = seeds.length;
    }

    @Override
    public String get() {
        return "value";
    }

    public static int sum(int... values) {
        int total = 0;
        for (int value : values) {
            total += value;
        }
        return total;
    }

    public Runnable counter() {
        return () -> hits++;
    }

    public enum Kind {
        FIRST,
        SECOND
    }

    public static Method bridgeMethod() {
        for (Method method : FlagsProbe.class.getDeclaredMethods()) {
            if (method.isBridge()) {
                return method;
            }
        }
        return null;
    }

    public static Method varargsMethod() {
        for (Method method : FlagsProbe.class.getDeclaredMethods()) {
            if (method.isVarArgs()) {
                return method;
            }
        }
        return null;
    }

    public static Method syntheticMethod() {
        for (Method method : FlagsProbe.class.getDeclaredMethods()) {
            if (method.isSynthetic() && !method.isBridge()) {
                return method;
            }
        }
        return null;
    }

    public static Constructor<?> varargsConstructor() {
        for (Constructor<?> constructor : FlagsProbe.class.getDeclaredConstructors()) {
            if (constructor.isVarArgs()) {
                return constructor;
            }
        }
        return null;
    }

    public static Field syntheticField() {
        for (Field field : Kind.class.getDeclaredFields()) {
            if (field.isSynthetic()) {
                return field;
            }
        }
        return null;
    }

    public static int methodModifiers(Method method) {
        return method.getModifiers();
    }

    public static boolean methodIsBridge(Method method) {
        return method.isBridge();
    }

    public static boolean methodIsVarArgs(Method method) {
        return method.isVarArgs();
    }

    public static boolean methodIsSynthetic(Method method) {
        return method.isSynthetic();
    }

    public static int constructorModifiers(Constructor<?> constructor) {
        return constructor.getModifiers();
    }

    public static boolean constructorIsVarArgs(Constructor<?> constructor) {
        return constructor.isVarArgs();
    }

    public static boolean constructorIsSynthetic(Constructor<?> constructor) {
        return constructor.isSynthetic();
    }

    public static int fieldModifiers(Field field) {
        return field.getModifiers();
    }

    public static boolean fieldIsSynthetic(Field field) {
        return field.isSynthetic();
    }

    public static boolean executableIsVarArgs(Executable executable) {
        return executable.isVarArgs();
    }

    public static boolean executableIsSynthetic(Executable executable) {
        return executable.isSynthetic();
    }

    public static boolean memberIsSynthetic(Member member) {
        return member.isSynthetic();
    }

    public static Method missingMethod() {
        try {
            return FlagsProbe.class.getDeclaredMethod("absent");
        } catch (NoSuchMethodException ex) {
            throw new IllegalStateException(ex);
        }
    }

    public static int inheritedCompareSort() {
        List<Object> values = new ArrayList<>();
        values.add(Integer.valueOf(2));
        values.add(Integer.valueOf(1));
        values.sort(new SubComparator());
        return ((Integer) values.get(0)).intValue();
    }

    static class BaseComparator implements java.util.Comparator<Object> {
        @Override
        public int compare(Object left, Object right) {
            return ((Integer) left).intValue() - ((Integer) right).intValue();
        }
    }

    static final class SubComparator extends BaseComparator {
    }

    public static int setConstructorSize() {
        Set<Object> values = new LinkedHashSet<>(Arrays.asList("a", null, "a", "b"));
        return values.size();
    }

    public static boolean setConstructorContainsNull() {
        Set<Object> values = new LinkedHashSet<>(Arrays.asList("a", null, "a", "b"));
        return values.contains(null);
    }

    public static int listConstructorSize() {
        List<Object> values = new ArrayList<>(Arrays.asList("a", "a", null));
        return values.size();
    }

    public static boolean bulkRemoveAll() {
        List<Object> values = new ArrayList<>(Arrays.asList("a", "b", "c", "b"));
        boolean changed = values.removeAll(Arrays.asList("b"));
        return changed && values.size() == 2;
    }

    public static boolean bulkRetainAll() {
        List<Object> values = new ArrayList<>(Arrays.asList("a", "b", "c"));
        boolean changed = values.retainAll(Arrays.asList("a", "c"));
        return changed && values.size() == 2;
    }

    public static boolean bulkContainsAll() {
        List<Object> values = new ArrayList<>(Arrays.asList("a", "b", "c"));
        return values.containsAll(Arrays.asList("a", "c")) && !values.containsAll(Arrays.asList("a", "z"));
    }
}
