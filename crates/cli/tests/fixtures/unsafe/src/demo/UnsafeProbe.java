package demo;

/**
 * M3.5 fixture: a class whose constructor and static initializer are both observable, so
 * the acceptance test can prove that {@code Unsafe.allocateInstance} initialized the class
 * but skipped the constructor.
 */
public class UnsafeProbe {
    public int value;
    public static int marker;

    static {
        marker = 7;
    }

    public UnsafeProbe() {
        value = 42;
    }
}
