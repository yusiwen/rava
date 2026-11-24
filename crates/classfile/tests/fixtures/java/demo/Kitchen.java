package demo;

import java.io.IOException;
import java.util.List;

/**
 * The kitchen sink: class, field, method, parameter and type annotations, repeatable
 * annotations, a generic method, exceptions, parameter names, nestmates, an inner class
 * and an anonymous class with an enclosing method.
 */
@Defaults(
        name = "kitchen",
        count = 9,
        color = Color.BLUE,
        type = String.class,
        nested = @Nested("in"),
        primes = {2, 3, 5, 7},
        aliases = {"k", "itchen"},
        flag = false,
        letter = 'k',
        big = 9000000000L,
        ratio = 0.5,
        ratio32 = 0.25f,
        small = 12,
        medium = 1200)
@Hidden("class")
@Tag("a")
@Tag("b")
public class Kitchen extends @TypeUse("super") Object implements Comparable<Kitchen> {

    @Defaults(name = "field")
    private final @TypeUse("field") List<@TypeUse("arg") String> names = List.of("a");

    private @TypeUse("elem") String[] tags = {"t"};

    private List<? extends @TypeUse("wild") String> wild = List.of("w");

    private Kitchen.@TypeUse("nested") Inner nested;

    @Hidden("f")
    private int hidden;

    private @HiddenType String invisibleType;

    static final int LIMIT = 4;

    public Kitchen(@TypeUse("ctor") String name, @Defaults(name = "param") int count) {
        this.nested = null;
    }

    @Defaults(name = "method", color = Color.GREEN)
    public <T extends Comparable<T>> T pick(@TypeUse("param") T left, @Hidden("right") T right) {
        return left;
    }

    @Tags(@Tag("c"))
    public void io() throws @TypeUse("throws") IOException {
    }

    public void varargs(String... rest) {
    }

    @Deprecated
    public void legacy() {
    }

    public Object make() {
        return new Object() {};
    }

    @Override
    public int compareTo(Kitchen other) {
        return 0;
    }

    /** A static nested type: a nestmate of Kitchen. */
    public static class Entry {
        private int secret;

        public int reveal() {
            return new Kitchen("k", 1).names.size() + secret;
        }
    }

    /** A non-static inner type: `InnerClasses` plus an outer instance. */
    class Inner {
        int value() {
            return LIMIT;
        }
    }
}
