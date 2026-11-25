package demo;

import java.lang.reflect.Method;
import org.springframework.cglib.proxy.Enhancer;
import org.springframework.cglib.proxy.MethodInterceptor;
import org.springframework.cglib.proxy.MethodProxy;

/**
 * M3.6 fixture: Spring's repackaged CGLIB generates a subclass of {@link Bean} at run time.
 *
 * The enhancer's class is produced by `org.springframework.cglib`/`org.springframework.asm`
 * while the program runs and is defined through the ordinary VM class-definition path, so
 * the acceptance test can inspect the generated class in the class table.
 */
public final class CglibProbe {

    private CglibProbe() {
    }

    /** The superclass CGLIB subclasses. */
    public static class Bean {
        public String hello() {
            return "hello";
        }

        public int add(int a, int b) {
            return a + b;
        }
    }

    public static void run() {
        Enhancer enhancer = new Enhancer();
        enhancer.setSuperclass(Bean.class);
        enhancer.setCallback(new MethodInterceptor() {
            @Override
            public Object intercept(Object obj, Method method, Object[] args, MethodProxy proxy) {
                if (method.getName().equals("hello")) {
                    return "intercepted:" + method.getName();
                }
                return (Integer) args[0] + (Integer) args[1];
            }
        });
        Bean bean = (Bean) enhancer.create();
        System.out.println(bean.hello());
        System.out.println("" + bean.add(2, 3));
        System.out.println("" + bean.getClass().getName().contains("$$EnhancerByCGLIB$$"));
    }

    public static void main(String[] args) {
        run();
    }
}
