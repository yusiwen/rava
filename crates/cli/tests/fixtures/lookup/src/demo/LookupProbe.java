package demo;

import java.lang.invoke.MethodHandle;
import java.lang.invoke.MethodHandles;
import java.lang.invoke.MethodHandles.Lookup;
import java.lang.invoke.MethodType;

/**
 * Prints the facts the M3.4 method-handles test asserts.
 *
 * The fixture follows the shape CGLIB's {@code org.springframework.cglib.core.ReflectUtils}
 * uses to define a generated class: ask {@link MethodHandles#privateLookupIn} for a lookup
 * on the class that owns the generated code, then resolve members through that lookup.
 * Every printed line is one fact, so the acceptance test can compare the whole stdout.
 *
 * Values that are not strings are concatenated with {@code ""} before printing, because
 * this VM's {@code PrintStream} stubs only implement the {@code String} overloads; that
 * keeps every line a single deterministic fact.
 */
public final class LookupProbe {

    private LookupProbe() {
    }

    public static int add(int left, int right) {
        return left + right;
    }

    public String echo() {
        return "lookup";
    }

    public static void run() throws Throwable {
        Lookup lookup = MethodHandles.privateLookupIn(LookupProbe.class, MethodHandles.lookup());
        MethodType type = MethodType.methodType(int.class, int.class, int.class);
        MethodHandle add = lookup.findStatic(LookupProbe.class, "add", type);
        System.out.println("" + add.invokeWithArguments(2, 3));
        System.out.println(lookup.lookupClass().getName());
        System.out.println("" + lookup.hasPrivateAccess());
        System.out.println("" + lookup.findVirtual(
                LookupProbe.class, "echo", MethodType.methodType(String.class))
                .invokeWithArguments(new LookupProbe()));
    }
}
