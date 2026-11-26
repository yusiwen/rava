package demo;

/** A caught throwable with a cause and a suppressed exception, printed as a trace. */
public final class Trace {
    public static void main(String[] args) {
        Throwable throwable = new IllegalStateException("outer", new RuntimeException("inner"));
        throwable.addSuppressed(new IllegalArgumentException("sup"));
        throwable.printStackTrace();
    }
}
