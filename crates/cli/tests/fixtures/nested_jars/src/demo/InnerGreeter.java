package demo;

/** M4.4 nested-jar fixture: the provider that exists only in {@code BOOT-INF/lib/demo-helper.jar}. */
public final class InnerGreeter implements Greeter {
    @Override
    public String greet() {
        return "inner-greeter";
    }
}
