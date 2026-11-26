package demo;

/** M4.3 executable-jar fixture: the helper that must come from the manifest Class-Path. */
public final class Helper {
    private Helper() {
    }

    public static String greeting() {
        return "helper";
    }
}
