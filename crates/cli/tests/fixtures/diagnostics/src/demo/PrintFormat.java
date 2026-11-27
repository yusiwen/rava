package demo;

import java.util.Locale;

/**
 * `PrintStream.printf`/`format`/`append` (#105), including the Locale decimal separator and
 * the append range/null forms. The JDK's text is the oracle.
 */
public final class PrintFormat {

    private PrintFormat() {
    }

    public static void main(String[] args) {
        System.out.printf("%d %s%n", 42, "x");
        System.out.format("%.2f%n", 1.5);
        System.out.printf(Locale.GERMANY, "%.2f%n", 1.5);
        System.out.append("ab").append('c').append("def", 0, 2);
        System.out.println();
        System.out.append((CharSequence) null).println("<null>");
        System.out.append("abcdef", 1, 3).println("<range>");
        System.out.println(System.out.printf("x") == System.out);

        System.err.printf("%d %s%n", 7, "err");
        System.err.append("E").append('R').println();
    }
}
