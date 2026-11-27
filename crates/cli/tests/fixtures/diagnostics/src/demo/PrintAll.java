package demo;

/**
 * Every `PrintStream` overload the runtime installs (#90): primitives, `char[]`, a
 * Java-`toString` object and the error stream. The JDK's text is the oracle.
 */
public final class PrintAll {

    /** A deterministic `toString` override; the identity fallback is tested separately. */
    static final class Bean {
        @Override
        public String toString() {
            return "BEAN";
        }
    }

    private PrintAll() {
    }

    public static void main(String[] args) {
        System.out.println(42);
        System.out.println(9007199254740993L);
        System.out.println(true);
        System.out.println('x');
        System.out.println(1.5f);
        System.out.println(1.5d);
        System.out.println(1.0d);
        System.out.println(Float.NaN);
        System.out.println(Float.POSITIVE_INFINITY);
        System.out.println(new char[]{'a', 'b'});
        System.out.println(new Bean());
        System.out.print("A");
        System.out.print(7);
        System.out.println('Z');

        System.err.println(42);
        System.err.println(true);
        System.err.println(1.5);
        System.err.println('x');
        System.err.println(new char[]{'a', 'b'});
        System.err.println(new Bean());
    }
}
