package demo;

import java.lang.reflect.InvocationHandler;
import java.lang.reflect.Method;
import java.lang.reflect.Proxy;

/**
 * Prints the facts the M3.2 proxy test asserts.
 *
 * The handler is a lambda: it returns a fixed name, sums the boxed arguments of
 * {@code add}/{@code widen}, forwards {@code greet} to the interface's default method with
 * {@link InvocationHandler#invokeDefault}, answers {@code toString}, and rejects anything
 * else. Each printed line is one fact, so the acceptance test can compare the whole stdout.
 */
public final class ProxyProbe {

    private ProxyProbe() {
    }

    public static void run() {
        InvocationHandler handler = (proxy, method, args) -> {
            switch (method.getName()) {
                case "name":
                    return "rava";
                case "add":
                    return (Integer) args[0] + (Integer) args[1];
                case "widen":
                    return (Long) args[0] + (Integer) args[1];
                case "greet":
                    try {
                        return InvocationHandler.invokeDefault(proxy, method, args);
                    } catch (Throwable throwable) {
                        throw new RuntimeException(throwable);
                    }
                case "toString":
                    return "proxy!";
                default:
                    throw new UnsupportedOperationException(method.getName());
            }
        };
        Service service = (Service) Proxy.newProxyInstance(
                ProxyProbe.class.getClassLoader(),
                new Class<?>[] {Service.class},
                handler);
        System.out.println(service.name());
        System.out.println("" + service.add(2, 3));
        System.out.println("" + service.widen(4L, 5));
        System.out.println(service.greet());
        System.out.println(service.toString());
        System.out.println("" + Proxy.isProxyClass(service.getClass()));
        System.out.println("" + (Proxy.getProxyClass(
                ProxyProbe.class.getClassLoader(), Service.class) == Proxy.getProxyClass(
                ProxyProbe.class.getClassLoader(), Service.class)));
        System.out.println("" + (service.getClass().getInterfaces()[0] == Service.class));
        System.out.println("" + (Proxy.getInvocationHandler(service) == handler));
    }
}
