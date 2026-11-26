package demo;

import java.util.Iterator;
import java.util.ServiceLoader;

/**
 * M4.2 fixture: drives {@code java.util.ServiceLoader} through the real API.
 *
 * The classpath is {@code classes} followed by {@code providers.jar}. The directory
 * supplies {@code META-INF/services/demo.Greeter} for {@link HelloGreeter} (with comments
 * and blank lines) and the jar supplies the same file for {@link WorldGreeter}, so a load
 * that reads only the first resource would miss a provider. The provider constructors and
 * static initializers print, which makes the exact output a proof of when they ran.
 */
public final class ServiceLoaderProbe {
    private ServiceLoaderProbe() {
    }

    public static void main(String[] args) {
        run(args.length == 0 ? "order" : args[0]);
    }

    /** Run one probe mode; the acceptance test calls this directly for the typed errors. */
    public static void run(String mode) {
        if (mode.equals("order")) {
            order();
        } else if (mode.equals("explicit")) {
            explicit();
        } else if (mode.equals("identity")) {
            identity();
        } else if (mode.equals("missing")) {
            missing();
        } else if (mode.equals("bootstrap")) {
            bootstrap();
        } else if (mode.equals("invalid")) {
            invalid();
        } else if (mode.equals("primitive")) {
            ServiceLoader.load(int.class);
        } else if (mode.equals("array")) {
            ServiceLoader.load(Greeter[].class);
        } else {
            System.out.println("unknown:" + mode);
        }
    }

    /** Proves that load and hasNext construct no provider. */
    private static void order() {
        System.out.println("before-iteration");
        ServiceLoader<Greeter> services = ServiceLoader.load(Greeter.class);
        Iterator<Greeter> iterator = services.iterator();
        System.out.println("after-iterator");
        System.out.println("hasNext=" + iterator.hasNext());
        System.out.println("after-hasNext");
        while (iterator.hasNext()) {
            System.out.println("next:" + iterator.next().greet());
        }
    }

    /** Exercises the explicit ClassLoader overload. */
    private static void explicit() {
        ServiceLoader<Greeter> services = ServiceLoader.load(
                Greeter.class, ServiceLoaderProbe.class.getClassLoader());
        Iterator<Greeter> iterator = services.iterator();
        while (iterator.hasNext()) {
            System.out.println("explicit:" + iterator.next().greet());
        }
    }

    /**
     * The provider's class identity is the ordinary internal-name identity, not a second
     * dotted-name class created by loading the configuration.
     */
    private static void identity() {
        Class<?> provider = ServiceLoader.load(Greeter.class).iterator().next().getClass();
        System.out.println("identity.name=" + provider.getName());
        try {
            Class<?> loaded = ServiceLoaderProbe.class.getClassLoader().loadClass("demo.HelloGreeter");
            System.out.println("identity.loadClass.same=" + (loaded == provider));
        } catch (ClassNotFoundException error) {
            System.out.println("identity.loadClass.missing");
        }
        try {
            Class<?> byName = Class.forName("demo.HelloGreeter");
            System.out.println("identity.forName.same=" + (byName == provider));
        } catch (ClassNotFoundException error) {
            System.out.println("identity.forName.missing");
        }
    }

    /** A service with no configuration is an empty iteration, not an error. */
    private static void missing() {
        int count = 0;
        Iterator<MissingService> iterator = ServiceLoader.load(MissingService.class).iterator();
        while (iterator.hasNext()) {
            iterator.next();
            count++;
        }
        System.out.println("missing.count=" + count);
    }

    /** A null ClassLoader selects the bootstrap loader, which carries no provider file. */
    private static void bootstrap() {
        int count = 0;
        Iterator<Greeter> iterator = ServiceLoader.load(Greeter.class, (ClassLoader) null).iterator();
        while (iterator.hasNext()) {
            iterator.next();
            count++;
        }
        System.out.println("bootstrap.count=" + count);
    }

    /** load must reject a configured provider that does not implement the service. */
    private static void invalid() {
        ServiceLoader.load(InvalidService.class);
        System.out.println("invalid.loaded");
    }
}
