package demo;

/**
 * M4.4 nested-jar fixture: a class that exists only in the nested helper jar.
 *
 * The main class refers to it, so a successful run proves nested class loading, not just
 * that the nested jar was indexed.
 */
public final class NestedHelper {
    private NestedHelper() {
    }

    public static String greeting() {
        return "helper";
    }
}
