package demo;

import org.springframework.cglib.proxy.Enhancer;
import org.springframework.cglib.proxy.MethodInterceptor;

/**
 * M3.6 fixture: a CGLIB request the enhancer itself rejects.
 *
 * A final superclass cannot be subclassed. The enhancer throws its own
 * {@link IllegalArgumentException} before any class is defined, so no generated class may
 * appear in the class table and the VM must not substitute a hand-built class.
 */
public final class CglibRejectionProbe {

    private CglibRejectionProbe() {
    }

    /** CGLIB cannot subclass a final class. */
    public static final class FinalBean {
        public final String hello() {
            return "hello";
        }
    }

    public static void run() {
        Enhancer enhancer = new Enhancer();
        enhancer.setSuperclass(FinalBean.class);
        enhancer.setCallback((MethodInterceptor) (obj, method, args, proxy) -> "intercepted");
        try {
            enhancer.create();
            System.out.println("created");
        } catch (IllegalArgumentException rejected) {
            System.out.println(rejected.getMessage());
        }
    }

    public static void main(String[] args) {
        run();
    }
}
