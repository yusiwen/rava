package demo;

import java.util.Locale;

/**
 * `String.format` conversions against the JDK (#71): scientific notation, boxed-width
 * integral output, `%c`, positional arguments and the locale decimal separator.
 *
 * <p>Each method returns the JDK's answer; the Rust test compares it.
 */
public final class FormatProbe {

    private FormatProbe() {
    }

    public static String scientific() {
        return String.format("%e", 1234.5);
    }

    public static String scientificZero() {
        return String.format("%e", 0.0);
    }

    public static String hexInt() {
        return String.format("%x", -1);
    }

    public static String hexLong() {
        return String.format("%x", -1L);
    }

    public static String hexByte() {
        return String.format("%x", (byte) -1);
    }

    public static String hexShort() {
        return String.format("%x", (short) -1);
    }

    public static String octalByte() {
        return String.format("%o", (byte) -1);
    }

    public static String character() {
        return String.format("%c", 'A');
    }

    public static String positional() {
        return String.format("%1$s-%1$s", "z");
    }

    public static String germanDecimal() {
        return String.format(Locale.GERMANY, "%.2f", 1.5);
    }

    public static String usDecimal() {
        return String.format(Locale.US, "%.2f", 1.5);
    }

    public static String defaultDecimal() {
        return String.format("%.2f", 1.5);
    }

    public static String padded() {
        return String.format("%08.2f", 1.5);
    }

    public static String decimalInt() {
        return String.format("%d", 42);
    }

    public static String twoStrings() {
        return String.format("%s %s", "a", "b");
    }
}
