package demo;

/** Prints the suppressed array length, which must be the number added. */
public final class Suppressed {
    public static void main(String[] args) {
        Throwable throwable = new IllegalStateException("outer");
        throwable.addSuppressed(new RuntimeException("inner"));
        System.out.println(String.valueOf(throwable.getSuppressed().length));
    }
}
