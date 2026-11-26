package demo;

import java.beans.PropertyDescriptor;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.HashSet;
import java.util.List;
import java.util.Map;
import java.util.Objects;
import java.util.Set;
import java.util.Spliterator;
import java.util.function.BiPredicate;
import java.util.function.Predicate;

/**
 * The M4.5 interpreter-side surface probes: the modules with no direct test before #76.
 *
 * <p>Every method returns the JDK's answer; the Rust test compares it. The fixture is
 * ordinary Java compiled with {@code javac --release 17} and committed, so the test needs
 * no JDK.
 */
public final class SurfaceProbe {

    private SurfaceProbe() {
    }

    /** A value key: equality and hashCode are overridden so a reference comparison fails. */
    static final class Key {
        final int value;

        Key(int value) {
            this.value = value;
        }

        @Override
        public boolean equals(Object other) {
            return other instanceof Key && ((Key) other).value == value;
        }

        @Override
        public int hashCode() {
            return 7;
        }
    }

    /** A {@link CharSequence} whose only length is the interface default's input. */
    static final class Empty implements CharSequence {
        @Override
        public int length() {
            return 0;
        }

        @Override
        public char charAt(int index) {
            throw new IndexOutOfBoundsException();
        }

        @Override
        public CharSequence subSequence(int start, int end) {
            return this;
        }

        @Override
        public String toString() {
            return "";
        }
    }

    public static final class Bean {
        private String name = "n";

        public String getName() {
            return name;
        }

        public void setName(String name) {
            this.name = name;
        }
    }

    /** `List.contains` calls the element's Java `equals` (collection_equality). */
    public static boolean listContainsUsesEquals() {
        List<Object> list = new ArrayList<>();
        list.add(new Key(1));
        return list.contains(new Key(1));
    }

    /** `Set.contains` calls the element's Java `equals` (collection_equality). */
    public static boolean setContainsUsesEquals() {
        Set<Object> set = new HashSet<>();
        set.add(new Key(2));
        return set.contains(new Key(2));
    }

    /** `Objects.hash` runs each element's Java `hashCode` (objects). */
    public static int objectsHash() {
        return Objects.hash(1, 2);
    }

    /** `Objects.hashCode` runs the argument's Java `hashCode` (objects). */
    public static int objectsHashCode() {
        return Objects.hashCode(new Key(3));
    }

    /** `CharSequence.isEmpty()` dispatches the receiver's `length()` (interface_defaults). */
    public static boolean charSequenceEmpty() {
        CharSequence sequence = new Empty();
        return sequence.isEmpty();
    }

    /** `Map.getOrDefault` calls the receiver's own `get`/`containsKey` (interface_defaults). */
    public static String mapGetOrDefault() {
        Map<String, String> map = new HashMap<>();
        return map.getOrDefault("missing", "default");
    }

    /** `BiPredicate.or` composes two Java lambdas (functional). */
    public static boolean biPredicateOr() {
        BiPredicate<Integer, Integer> positive = (left, right) -> left > 0;
        return positive.or((left, right) -> right > 0).test(0, 1);
    }

    /** `Predicate.and` composes two Java lambdas (functional). */
    public static boolean predicateAnd() {
        Predicate<Integer> positive = value -> value > 0;
        return positive.and(value -> value < 10).test(5);
    }

    /** `Iterable.spliterator()` drains the receiver's iterator (iterables). */
    public static long iterableSpliteratorCount() {
        List<Object> list = new ArrayList<>();
        list.add("a");
        list.add("b");
        Iterable<Object> iterable = list;
        long[] count = {0};
        Spliterator<Object> spliterator = iterable.spliterator();
        while (spliterator.tryAdvance(value -> count[0]++)) {
            // The native snapshot has two elements; the consumer counts them.
        }
        return count[0];
    }

    /** The three-argument `PropertyDescriptor` constructor (bean_support). */
    public static boolean propertyDescriptor() throws Exception {
        PropertyDescriptor descriptor = new PropertyDescriptor(
                "name",
                Bean.class.getMethod("getName"),
                Bean.class.getMethod("setName", String.class));
        return descriptor.getReadMethod() != null
                && descriptor.getWriteMethod() != null
                && descriptor.getPropertyType() == String.class;
    }
}
