package demo;

/**
 * Jar-classpath provider for the M4.2 fixture.
 *
 * Its provider-configuration file lives in {@code providers.jar}; the class itself is in
 * the directory classpath, so {@code load} must combine both resources.
 */
public final class WorldGreeter implements Greeter {
    static {
        System.out.println("clinit:demo.WorldGreeter");
    }

    public WorldGreeter() {
        System.out.println("init:demo.WorldGreeter");
    }

    @Override
    public String greet() {
        return "world";
    }
}
