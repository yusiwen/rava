package demo;

/**
 * The interface the M3.2 proxy fixture implements.
 *
 * It carries one abstract method of each interesting shape — a reference, an {@code int}
 * pair, a {@code long}/{@code int} pair — plus a default method, so the proxy handler can
 * exercise boxing, unboxing and {@code InvocationHandler.invokeDefault}.
 */
public interface Service {

    String name();

    int add(int left, int right);

    long widen(long left, int right);

    default String greet() {
        return "hello " + name();
    }
}
