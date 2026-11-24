package demo;

import java.util.ArrayList;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Set;
import java.util.Spliterator;
import java.util.function.Consumer;
import java.util.stream.Collector;
import java.util.stream.Collectors;
import java.util.stream.Stream;
import java.util.stream.StreamSupport;

/**
 * M2.24 stream fixture: the {@code java.util.stream} shapes Spring's annotation-metadata
 * path uses, compiled with {@code javac --release 17}.
 *
 * <p>The methods return heap objects the Rust test inspects (a {@code List}, a {@code Set},
 * a {@code String}, a primitive), so the assertions stay on VM values and never on a
 * helper the fixture could fake. {@link WordSpliterator} is a real Java
 * {@code Spliterator}: {@code StreamSupport.stream} must drain it through its own
 * {@code tryAdvance} and a VM-native consumer, which is the shape Spring's
 * {@code MergedAnnotationsCollection$AnnotationsSpliterator} has.
 */
public class StreamProbe {

    private static List<String> words() {
        List<String> words = new ArrayList<>();
        words.add("delta");
        words.add("alpha");
        words.add("charlie");
        words.add("bravo");
        return words;
    }

    /** {@code filter}/{@code map}/{@code Collectors.toList}. */
    public static List<String> filterMap(int minLength) {
        return words().stream()
                .filter(w -> w.length() > minLength)
                .map(w -> w.toUpperCase())
                .collect(Collectors.toList());
    }

    /** {@code Collectors.toSet} removes the duplicate and keeps insertion order. */
    public static Set<String> toSet() {
        List<String> values = new ArrayList<>();
        values.add("b");
        values.add("a");
        values.add("b");
        values.add("c");
        return values.stream().collect(Collectors.toSet());
    }

    /** {@code sorted()} and {@code limit(long)} followed by {@code Collectors.joining}. */
    public static String sortedLimited() {
        return words().stream().sorted().limit(2).collect(Collectors.joining("+"));
    }

    /** A capturing lambda: the suffix is an {@code invokedynamic} call-site argument. */
    public static String capturing(String suffix) {
        return words().stream()
                .filter(w -> w.startsWith("a"))
                .map(w -> w + suffix)
                .collect(Collectors.joining());
    }

    /** {@code Collection.spliterator()} drained through {@code StreamSupport.stream}. */
    public static List<String> fromSpliterator() {
        return StreamSupport.stream(words().spliterator(), false)
                .filter(w -> w.length() == 5)
                .collect(Collectors.toList());
    }

    /** A real Java {@code Spliterator}, drained through its own {@code tryAdvance}. */
    public static String fromJavaSpliterator() {
        return StreamSupport.stream(new WordSpliterator(words()), false)
                .sorted()
                .collect(Collectors.joining(","));
    }

    /** The VM {@code Spliterator} surface: size, characteristics and both drain forms. */
    public static String spliteratorSurface() {
        Spliterator<String> spliterator = words().spliterator();
        long estimate = spliterator.estimateSize();
        long exact = spliterator.getExactSizeIfKnown();
        int characteristics = spliterator.characteristics();
        StringBuilder text = new StringBuilder();
        spliterator.tryAdvance(w -> text.append(w).append(";"));
        spliterator.forEachRemaining(w -> text.append(w).append(";"));
        return estimate + "/" + exact + "/" + characteristics + "/" + text.toString();
    }

    /** {@code Collectors.counting()} and {@code anyMatch}. */
    public static String terminalQueries(String probe) {
        long count = words().stream().filter(w -> w.length() == 5).count();
        boolean any = words().stream().anyMatch(w -> w.equals(probe));
        boolean all = words().stream().allMatch(w -> w.length() > 2);
        boolean none = words().stream().noneMatch(w -> w.startsWith("z"));
        return count + "/" + any + "/" + all + "/" + none;
    }

    /** {@code findFirst} over an empty and a non-empty filter. */
    public static String findFirst(String prefix) {
        String found = words().stream().filter(w -> w.startsWith(prefix)).findFirst().orElse("none");
        String missing = words().stream().filter(w -> w.startsWith("zulu")).findFirst().orElse("none");
        return found + "/" + missing;
    }

    /** {@code Stream.toArray()}. */
    public static String toArrayLength() {
        Object[] values = words().stream().toArray();
        return String.valueOf(values.length);
    }

    /** {@code Collectors.toCollection(LinkedHashSet::new)} keeps the supplier. */
    public static Set<String> toCollection() {
        return words().stream()
                .map(w -> w.substring(0, 1))
                .collect(Collectors.toCollection(LinkedHashSet::new));
    }

    /** {@code Collector.of} with a supplied accumulator and an identity finisher. */
    public static String customCollector() {
        Collector<String, ?, StringBuilder> collector = Collector.of(
                StringBuilder::new,
                (builder, word) -> builder.append(word).append("|"),
                (left, right) -> left.append(right));
        return words().stream().sorted().limit(2).collect(collector).toString();
    }

    /** {@code Collector.of} with a real finisher. */
    public static String customFinisher() {
        Collector<String, ?, String> collector = Collector.of(
                ArrayList::new,
                (list, word) -> list.add(word),
                (left, right) -> {
                    left.addAll(right);
                    return left;
                },
                list -> list.size() + "of" + list.get(0));
        return words().stream().sorted().limit(2).collect(collector);
    }

    /** A callback that throws: the fixture catches it inside its own exception table. */
    public static String caughtThrow() {
        try {
            return Stream.of("x", "y")
                    .map(w -> {
                        if (w.equals("y")) {
                            throw new IllegalStateException("boom");
                        }
                        return w;
                    })
                    .collect(Collectors.joining());
        } catch (IllegalStateException exception) {
            return "caught:" + exception.getMessage();
        }
    }

    /** A callback that throws with no handler here: the throwable must cross the native. */
    public static String uncaughtThrow() {
        return Stream.of("x")
                .map(w -> {
                    if (w != null) {
                        throw new IllegalStateException("kaboom");
                    }
                    return w;
                })
                .collect(Collectors.joining());
    }

    /** A Java {@code Spliterator} over a list snapshot, the shape Spring uses. */
    static final class WordSpliterator implements Spliterator<String> {

        private final List<String> words;
        private int index;

        WordSpliterator(List<String> words) {
            this.words = words;
        }

        @Override
        public boolean tryAdvance(Consumer<? super String> action) {
            if (index >= words.size()) {
                return false;
            }
            action.accept(words.get(index++));
            return true;
        }

        @Override
        public Spliterator<String> trySplit() {
            return null;
        }

        @Override
        public long estimateSize() {
            return words.size() - index;
        }

        @Override
        public int characteristics() {
            return ORDERED | SIZED;
        }
    }
}
