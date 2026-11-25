package demo;

/**
 * M3.3 acceptance fixture: a class with no dependencies that the application loader can
 * define, so {@code ClassLoader.loadClass} and {@code Class.forName} have a target.
 */
public class Existing {

    public static int answer() {
        return 42;
    }
}
