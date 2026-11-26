package demo;

/**
 * Listed as a provider of {@link InvalidService} but not assignable to it, so
 * {@code ServiceLoader.load} must reject the configuration.
 */
public final class NotAGreeter {
    public NotAGreeter() {
    }

    public String value() {
        return "not-a-greeter";
    }
}
