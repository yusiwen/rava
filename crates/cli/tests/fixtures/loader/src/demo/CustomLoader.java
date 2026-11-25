package demo;

/**
 * M3.3 acceptance fixture: a user {@code ClassLoader} that defines a class from bytes the
 * test generates in process.
 *
 * <p>{@code defineClass} is protected in {@code java.lang.ClassLoader}; the subclass
 * exposes it so the acceptance test can drive the real native through compiled Java code.
 */
public class CustomLoader extends ClassLoader {

    public CustomLoader() {
        super();
    }

    public Class<?> define(String name, byte[] bytes) {
        return defineClass(name, bytes, 0, bytes.length);
    }
}
