package demo;

import java.beans.PropertyDescriptor;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.HashMap;
import java.util.HashSet;
import java.util.IdentityHashMap;
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

    /** `Map.get` calls the probe key's Java `equals` (collection_equality). */
    public static boolean mapGetUsesEquals() {
        Map<Object, Object> map = new HashMap<>();
        map.put(new Key(3), "x");
        return "x".equals(map.get(new Key(3)));
    }

    /** `Map.containsKey` calls the probe key's Java `equals`. */
    public static boolean mapContainsKeyUsesEquals() {
        Map<Object, Object> map = new HashMap<>();
        map.put(new Key(4), "x");
        return map.containsKey(new Key(4));
    }

    /** A second equal-key `put` replaces the value and keeps one entry. */
    public static boolean mapPutReplacesEqualKey() {
        Map<Object, Object> map = new HashMap<>();
        map.put(new Key(5), "a");
        map.put(new Key(5), "b");
        return map.size() == 1 && "b".equals(map.get(new Key(5)));
    }

    /** `Map.getOrDefault` finds an equal key and returns its value. */
    public static boolean mapGetOrDefaultUsesEquals() {
        Map<Object, Object> map = new HashMap<>();
        map.put(new Key(6), "value");
        return "value".equals(map.getOrDefault(new Key(6), "default"));
    }

    /** `Map.remove(Object)` calls the probe key's Java `equals`. */
    public static boolean mapRemoveUsesEquals() {
        Map<Object, Object> map = new HashMap<>();
        map.put(new Key(7), "value");
        return "value".equals(map.remove(new Key(7))) && map.isEmpty();
    }

    /** `Map.remove(Object, Object)` compares both the key and the value. */
    public static boolean mapRemovePairUsesEquals() {
        Map<Object, Object> map = new HashMap<>();
        map.put(new Key(8), "value");
        return map.remove(new Key(8), "value") && map.isEmpty();
    }

    /** `Map.containsValue` calls the probe value's Java `equals`. */
    public static boolean mapContainsValueUsesEquals() {
        Map<String, Object> map = new HashMap<>();
        map.put("k", new Key(9));
        return map.containsValue(new Key(9));
    }

    /** `Map.putIfAbsent` keeps an existing value for an equal key. */
    public static boolean mapPutIfAbsentUsesEquals() {
        Map<Object, Object> map = new HashMap<>();
        map.put(new Key(10), "a");
        Object previous = map.putIfAbsent(new Key(10), "b");
        return "a".equals(previous) && "a".equals(map.get(new Key(10))) && map.size() == 1;
    }

    /** `Map.computeIfAbsent` computes once for an equal key. */
    public static int mapComputeIfAbsent() {
        Map<String, Integer> map = new HashMap<>();
        int computed = map.computeIfAbsent("k", key -> 42);
        return computed + map.get("k");
    }

    /** `IdentityHashMap` stays reference-based and stores both equal keys. */
    public static int identityMapStoresBoth() {
        IdentityHashMap<Object, Object> map = new IdentityHashMap<>();
        map.put(new Key(11), "a");
        map.put(new Key(11), "b");
        return map.size();
    }

    /** A `Map`-typed call on an `IdentityHashMap` still compares by identity. */
    public static boolean identityMapInterfaceCall() {
        Map<Object, Object> map = new IdentityHashMap<>();
        map.put(new Key(12), "a");
        return map.get(new Key(12)) == null && map.size() == 1;
    }

    /** The `Map` default (`computeIfAbsent`) keeps identity keys apart. */
    public static boolean identityMapComputeIfAbsent() {
        Map<Object, Object> map = new IdentityHashMap<>();
        map.put(new Key(13), "a");
        Object computed = map.computeIfAbsent(new Key(13), key -> "b");
        return "b".equals(computed) && map.size() == 2;
    }

    /** The `Map` default (`remove(k, v)`) does not remove a distinct equal key. */
    public static boolean identityMapRemovePair() {
        Map<Object, Object> map = new IdentityHashMap<>();
        map.put(new Key(14), "a");
        return !map.remove(new Key(14), "a") && map.size() == 1;
    }

    /** `IdentityHashMap.equals` is identity-based and only accepts another identity map. */
    public static boolean identityMapEquals() {
        IdentityHashMap<Object, Object> left = new IdentityHashMap<>();
        IdentityHashMap<Object, Object> right = new IdentityHashMap<>();
        Object key = new Key(15);
        left.put(key, "value");
        right.put(key, "value");
        boolean same = left.equals(right);
        right.clear();
        right.put(new Key(15), "value");
        boolean distinct = left.equals(right);
        Map<Object, Object> hashMap = new HashMap<>();
        hashMap.put(new Key(15), "value");
        boolean cross = left.equals(hashMap);
        return same && !distinct && !cross;
    }

    /** A class with its own `toString`, so the text path must dispatch it. */
    static final class Plain {
        @Override
        public String toString() {
            return "plain!";
        }
    }

    /** `String.valueOf(Object)` calls the receiver's `toString`. */
    public static boolean stringValueOfUsesToString() {
        Object value = new Plain();
        return "plain!".equals(String.valueOf(value));
    }

    /** String concatenation renders an object through its `toString`. */
    public static boolean concatUsesToString() {
        return "1:plain!".equals("1:" + new Plain());
    }

    /** `String.format("%s")` calls the receiver's `toString`. */
    public static boolean formatUsesToString() {
        return "plain!".equals(String.format("%s", new Plain()));
    }

    /** `StringBuilder.append(Object)` calls the receiver's `toString`. */
    public static boolean builderAppendUsesToString() {
        StringBuilder builder = new StringBuilder();
        builder.append((Object) new Plain());
        return "plain!".equals(builder.toString());
    }

    /** `StringBuffer.append(Object)` calls the receiver's `toString`. */
    public static boolean bufferAppendUsesToString() {
        StringBuffer buffer = new StringBuffer();
        buffer.append((Object) new Plain());
        return "plain!".equals(buffer.toString());
    }

    /** `Objects.toString` calls the receiver's `toString`. */
    public static boolean objectsToStringUsesToString() {
        return "plain!".equals(Objects.toString(new Plain()));
    }

    /** `Arrays.toString` renders an object element through its `toString`. */
    public static boolean arraysToStringUsesToString() {
        return "[plain!]".equals(Arrays.toString(new Object[] {new Plain()}));
    }

    /** A class without a `toString` override keeps the structural `ClassName@hex` text. */
    public static boolean defaultToStringStaysStructural() {
        String text = String.valueOf(new Object());
        return text.startsWith("java.lang.Object@");
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
