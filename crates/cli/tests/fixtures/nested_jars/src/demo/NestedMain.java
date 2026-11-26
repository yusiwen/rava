package demo;

import java.net.URL;
import java.util.Enumeration;
import java.util.ServiceLoader;

/**
 * M4.4 Boot nested-jar fixture: the manifest's {@code Start-Class}.
 *
 * The run proves, in order: a class from {@code BOOT-INF/lib/demo-helper.jar} is loaded
 * with the ordinary class identity; resources are found in {@code BOOT-INF/classes} and in
 * the nested jar; {@code getResources} sees every copy; the externally visible URL for
 * both Boot forms opens through {@code URL.openStream} and survives a {@code new URL(...)}
 * round trip; and {@code ServiceLoader} reads {@code META-INF/services} from the nested
 * jar. Only stable class-visible facts are printed, never a host path.
 */
public final class NestedMain {
    public static void main(String[] args) throws Exception {
        System.out.println("nested:" + NestedHelper.greeting());
        // The nested class keeps the ordinary loader identity: one application loader over
        // the whole Boot layout, not a loader per nested jar.
        System.out.println(
                "same-loader:"
                        + (NestedHelper.class.getClassLoader()
                                == NestedMain.class.getClassLoader()));

        System.out.println(
                "outer:"
                        + ResourceReader.read(
                                NestedMain.class.getResourceAsStream("/outer.txt")));
        System.out.println(
                "inner:"
                        + ResourceReader.read(
                                NestedHelper.class.getResourceAsStream("/inner.txt")));
        // The classes root lookup falls through to the nested jar, as Boot's class loader does.
        System.out.println(
                "inner-classroot:"
                        + ResourceReader.read(
                                NestedMain.class.getResourceAsStream("/inner.txt")));

        System.out.println(
                "resources-outer:"
                        + count(
                                NestedMain.class
                                        .getClassLoader()
                                        .getResources("outer.txt")));
        System.out.println(
                "resources-inner:"
                        + count(
                                NestedMain.class
                                        .getClassLoader()
                                        .getResources("inner.txt")));

        URL outer = NestedMain.class.getClassLoader().getResource("outer.txt");
        URL inner = NestedHelper.class.getClassLoader().getResource("inner.txt");
        System.out.println("url-outer-protocol:" + outer.getProtocol());
        System.out.println(
                "url-outer-path:"
                        + (outer.getPath().contains("!/BOOT-INF/classes!/")
                                ? "classes"
                                : "other"));
        System.out.println(
                "url-outer-file:"
                        + (outer.getFile().contains("!/BOOT-INF/classes!/")
                                ? "classes"
                                : "other"));
        System.out.println(
                "url-outer-string:"
                        + (outer.toString().contains("!/BOOT-INF/classes!/")
                                ? "classes"
                                : "other"));
        System.out.println("url-outer-open:" + ResourceReader.read(outer.openStream()));
        URL copied = new URL(outer.toString());
        System.out.println("url-copied-open:" + ResourceReader.read(copied.openStream()));
        System.out.println(
                "url-inner-path:"
                        + (inner.getPath().contains("!/BOOT-INF/lib/demo-helper.jar!/")
                                ? "nested"
                                : "other"));
        System.out.println("url-inner-open:" + ResourceReader.read(inner.openStream()));

        for (Greeter greeter : ServiceLoader.load(Greeter.class)) {
            System.out.println("service:" + greeter.greet());
        }
    }

    private static int count(Enumeration<URL> urls) {
        int count = 0;
        while (urls.hasMoreElements()) {
            urls.nextElement();
            count++;
        }
        return count;
    }
}
