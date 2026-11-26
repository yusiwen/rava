package demo;

/**
 * Record {@code equals}/{@code toString}/{@code hashCode} probes for #72.
 *
 * <p>Each static method returns the JDK's answer; the Rust test compares it. The fixture
 * is ordinary Java compiled with {@code javac --release 17} and committed, so the test
 * needs no JDK.
 */
public final class RecordProbe {

    private RecordProbe() {
    }

    record FloatRecord(float value) {
    }

    record DoubleRecord(double value) {
    }

    record MixedRecord(char letter, boolean flag) {
    }

    /** A component whose equals throws: the identity shortcut must skip it. */
    static final class Exploding {
        @Override
        public boolean equals(Object other) {
            throw new IllegalStateException("component equals must not run");
        }

        @Override
        public int hashCode() {
            return 0;
        }
    }

    record BombRecord(Object value) {
    }

    /** `record.equals(record)` answers true without reading the component. */
    public static boolean identityShortcutSkipsComponents() {
        BombRecord record = new BombRecord(new Exploding());
        return record.equals(record);
    }

    /** `this == o` is true before any component is compared. */
    public static boolean selfEquals() {
        FloatRecord record = new FloatRecord(Float.NaN);
        return record.equals(record);
    }

    /** `Float.compare(NaN, NaN) == 0`: NaN equals NaN. */
    public static boolean nanPairEquals() {
        return new FloatRecord(Float.NaN).equals(new FloatRecord(Float.NaN));
    }

    /** `Float.compare(0.0f, -0.0f) != 0`: the zeros are not equal. */
    public static boolean zeroEquals() {
        return new FloatRecord(0.0f).equals(new FloatRecord(-0.0f));
    }

    /** `Double.compare(NaN, NaN) == 0`. */
    public static boolean doubleNaNEquals() {
        return new DoubleRecord(Double.NaN).equals(new DoubleRecord(Double.NaN));
    }

    /** `Double.compare(0.0, -0.0) != 0`. */
    public static boolean doubleZeroEquals() {
        return new DoubleRecord(0.0).equals(new DoubleRecord(-0.0));
    }

    /** A boolean component renders `true`, a char component renders the character. */
    public static String text() {
        return new MixedRecord('x', true).toString();
    }

    /** `Float.hashCode(NaN)` is the canonical NaN bits. */
    public static int floatNaNHash() {
        return new FloatRecord(Float.NaN).hashCode();
    }

    /** `Double.hashCode(NaN)` over the canonical bits. */
    public static int doubleNaNHash() {
        return new DoubleRecord(Double.NaN).hashCode();
    }

    /** `31 * Character.hashCode + Boolean.hashCode`. */
    public static int mixedHash() {
        return new MixedRecord('x', true).hashCode();
    }
}
