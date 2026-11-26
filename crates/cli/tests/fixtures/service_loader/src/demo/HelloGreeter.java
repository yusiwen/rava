package demo;

/**
 * Directory-classpath provider for the M4.2 fixture.
 *
 * The static initializer and the constructor both print, so the golden output shows that
 * neither ran before {@code Iterator.next()}.
 */
public final class HelloGreeter implements Greeter {
    static {
        System.out.println("clinit:demo.HelloGreeter");
    }

    public HelloGreeter() {
        System.out.println("init:demo.HelloGreeter");
    }

    @Override
    public String greet() {
        return "hello";
    }
}
