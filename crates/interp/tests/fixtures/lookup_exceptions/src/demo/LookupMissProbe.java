package demo;

import java.lang.reflect.Constructor;
import java.lang.reflect.Field;
import java.lang.reflect.Method;

/**
 * M2.30 acceptance fixture: the JDK's typed lookup misses, observed from compiled Java.
 *
 * <p>The six named `Class` lookups normally return a member. On a miss the JDK throws
 * {@link NoSuchFieldException} (field queries) or {@link NoSuchMethodException} (method
 * and constructor queries) instead of returning null; a compiled `catch` clause must see
 * that exception, and its message must name the receiver class, member and parameters.
 */
public final class LookupMissProbe {
    public int value;
    public String label;

    public LookupMissProbe() {
    }

    public LookupMissProbe(int value) {
        this.value = value;
    }

    public LookupMissProbe(String label) {
        this.label = label;
    }

    public int getValue() {
        return value;
    }

    public void setLabel(String label) {
        this.label = label;
    }

    public static int sum(int left, int right) {
        return left + right;
    }

    /** A field miss caught by a compiled `NoSuchFieldException` handler. */
    public static String fieldMiss() {
        try {
            Field field = LookupMissProbe.class.getDeclaredField("missing");
            return "no exception:" + field;
        } catch (NoSuchFieldException exception) {
            return describe(exception);
        }
    }

    /** An inherited field miss through `getField`. */
    public static String inheritedFieldMiss() {
        try {
            Field field = LookupMissProbe.class.getField("missing");
            return "no exception:" + field;
        } catch (NoSuchFieldException exception) {
            return describe(exception);
        }
    }

    /** A method miss with parameter types, caught by `NoSuchMethodException`. */
    public static String methodMiss() {
        try {
            Method method = LookupMissProbe.class.getDeclaredMethod(
                    "missing", int.class, String.class);
            return "no exception:" + method;
        } catch (NoSuchMethodException exception) {
            return describe(exception);
        }
    }

    /** An inherited method miss through `getMethod`. */
    public static String inheritedMethodMiss() {
        try {
            Method method = LookupMissProbe.class.getMethod("missing");
            return "no exception:" + method;
        } catch (NoSuchMethodException exception) {
            return describe(exception);
        }
    }

    /** A constructor miss with two parameter types. */
    public static String constructorMiss() {
        try {
            Constructor<?> constructor = LookupMissProbe.class.getDeclaredConstructor(
                    String.class, String.class);
            return "no exception:" + constructor;
        } catch (NoSuchMethodException exception) {
            return describe(exception);
        }
    }

    /** A constructor miss through `getConstructor` (constructors are not inherited). */
    public static String inheritedConstructorMiss() {
        try {
            Constructor<?> constructor = LookupMissProbe.class.getConstructor(long.class);
            return "no exception:" + constructor;
        } catch (NoSuchMethodException exception) {
            return describe(exception);
        }
    }

    /**
     * The exception kinds are distinct: a field miss is a `NoSuchFieldException` and not a
     * `NoSuchMethodException`, as the JDK defines them.
     */
    public static String fieldMissKind() {
        try {
            LookupMissProbe.class.getDeclaredField("missing");
            return "no exception";
        } catch (Exception exception) {
            if (exception instanceof NoSuchFieldException) {
                return "NoSuchFieldException|" + exception.getMessage();
            }
            if (exception instanceof NoSuchMethodException) {
                return "NoSuchMethodException|" + exception.getMessage();
            }
            return "other:" + exception.getClass().getName();
        }
    }

    /** The same misses, uncaught: the exception crosses the native ABI as a throwable. */
    public static Field uncaughtFieldMiss() throws NoSuchFieldException {
        return LookupMissProbe.class.getDeclaredField("missing");
    }

    public static Method uncaughtMethodMiss() throws NoSuchMethodException {
        return LookupMissProbe.class.getDeclaredMethod("missing", int.class);
    }

    public static Constructor<?> uncaughtConstructorMiss() throws NoSuchMethodException {
        return LookupMissProbe.class.getDeclaredConstructor(String.class, int.class);
    }

    /** The success paths still return the same members. */
    public static String success() throws Exception {
        Field field = LookupMissProbe.class.getDeclaredField("value");
        Method method = LookupMissProbe.class.getDeclaredMethod("getValue");
        Constructor<?> constructor = LookupMissProbe.class.getDeclaredConstructor();
        return field.getName() + "|" + method.getName() + "|" + constructor.getParameterCount();
    }

    /** An inherited success through the public lookups. */
    public static String inheritedSuccess() throws Exception {
        Field field = LookupMissProbe.class.getField("label");
        Method method = LookupMissProbe.class.getMethod("sum", int.class, int.class);
        Constructor<?> constructor = LookupMissProbe.class.getConstructor(int.class);
        return field.getName() + "|" + method.getName() + "|" + constructor.getParameterCount();
    }

    private static String describe(Throwable throwable) {
        return throwable.getClass().getName() + "|" + throwable.getMessage();
    }
}
