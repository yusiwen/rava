package demo;

import java.lang.reflect.InvocationHandler;
import java.lang.reflect.Method;
import java.lang.reflect.Proxy;

import org.aopalliance.intercept.MethodInterceptor;
import org.springframework.aop.framework.ProxyFactory;
import org.springframework.cglib.proxy.Enhancer;
import org.springframework.context.annotation.AnnotationConfigApplicationContext;
import org.springframework.context.annotation.Configuration;
import org.springframework.objenesis.ObjenesisStd;

/**
 * M3.7 acceptance fixture: one real Spring AOP proxy of each kind, plus the CGLIB,
 * Objenesis and JDK {@link Proxy} paths the earlier slices proved, in a single run.
 *
 * <p>Every line of {@code run()}'s output is a fact the acceptance test asserts. The
 * Spring classes are the unmodified ones from the pinned {@code compat/jars} set; no
 * proxy is pre-generated, and both the {@code @Configuration} subclass and the
 * {@code setProxyTargetClass(true)} proxy are produced by Spring's repackaged CGLIB while
 * the program runs.
 */
public final class C2Gate {

    private C2Gate() {
    }

    /**
     * A full (proxyBeanMethods = true) configuration class, so Spring subclasses it with
     * CGLIB and routes the {@code @Bean} method calls through the bean factory.
     *
     * <p>Spring 6.2's repackaged CGLIB names both this subclass and the AOP proxy below
     * with its own {@code SpringNamingPolicy}, so the marker is {@code $$SpringCGLIB$$},
     * not the plain-CGLIB {@code $$EnhancerByCGLIB$$}.
     */
    @Configuration(proxyBeanMethods = true)
    public static class Config {

        // Fully qualified because this fixture's nested `Bean` class shadows the
        // `org.springframework.context.annotation.Bean` annotation.
        @org.springframework.context.annotation.Bean
        String hello() {
            return "hello";
        }

        @org.springframework.context.annotation.Bean
        String greeting() {
            return hello() + "!";
        }
    }

    /** The interface the JDK proxy implements and the AOP proxies expose. */
    public interface Greeter {
        String greet();
    }

    /** The target class for both AOP proxies. */
    public static class GreeterImpl implements Greeter {
        @Override
        public String greet() {
            return "hello";
        }
    }

    /** The superclass the bare CGLIB enhancer subclasses. */
    public static class Bean {
        public String hello() {
            return "hello";
        }
    }

    /**
     * Objenesis' target: the constructor sets {@code value = 42} and the static
     * initializer sets {@code marker = 7}, so a fresh instance must show {@code 0} and
     * {@code 7}.
     */
    public static class Constructed {
        public int value = 42;

        public static int marker;

        static {
            marker = 7;
        }

        public Constructed() {
            value = 42;
        }
    }

    public static void run() {
        try (AnnotationConfigApplicationContext context =
                new AnnotationConfigApplicationContext(Config.class)) {
            boolean enhanced = context.getBean(Config.class).getClass().getName()
                    .contains("$$SpringCGLIB$$");
            System.out.println("config-enhanced:" + enhanced);
            System.out.println("greeting:" + context.getBean("greeting", String.class));
        }

        Enhancer enhancer = new Enhancer();
        enhancer.setSuperclass(Bean.class);
        enhancer.setCallback(new org.springframework.cglib.proxy.MethodInterceptor() {
            @Override
            public Object intercept(Object obj, Method method, Object[] args,
                    org.springframework.cglib.proxy.MethodProxy proxy) {
                return "cglib:" + method.getName();
            }
        });
        Bean bean = (Bean) enhancer.create();
        System.out.println("cglib:" + bean.hello());

        Constructed constructed = new ObjenesisStd().newInstance(Constructed.class);
        System.out.println("objenesis-value:" + constructed.value);
        System.out.println("objenesis-marker:" + Constructed.marker);

        Greeter handlerProxy = (Greeter) Proxy.newProxyInstance(
                Greeter.class.getClassLoader(),
                new Class<?>[] {Greeter.class},
                new InvocationHandler() {
                    @Override
                    public Object invoke(Object proxy, Method method, Object[] args) {
                        return "proxy:" + method.getName();
                    }
                });
        System.out.println("jdk-proxy:" + handlerProxy.greet());
        System.out.println("jdk-proxy-class:" + Proxy.isProxyClass(handlerProxy.getClass()));

        ProxyFactory jdkFactory = new ProxyFactory(new GreeterImpl());
        jdkFactory.addAdvice(new MethodInterceptor() {
            @Override
            public Object invoke(org.aopalliance.intercept.MethodInvocation invocation)
                    throws Throwable {
                return "aop:" + invocation.proceed();
            }
        });
        Greeter jdkProxy = (Greeter) jdkFactory.getProxy();
        System.out.println("aop-jdk:" + jdkProxy.greet());
        System.out.println("aop-jdk-class:" + Proxy.isProxyClass(jdkProxy.getClass()));

        ProxyFactory cglibFactory = new ProxyFactory(new GreeterImpl());
        cglibFactory.setProxyTargetClass(true);
        cglibFactory.addAdvice(new MethodInterceptor() {
            @Override
            public Object invoke(org.aopalliance.intercept.MethodInvocation invocation)
                    throws Throwable {
                return "aop:" + invocation.proceed();
            }
        });
        Greeter cglibProxy = (Greeter) cglibFactory.getProxy();
        System.out.println("aop-cglib:" + cglibProxy.greet());
        System.out.println("aop-cglib-class:"
                + cglibProxy.getClass().getName().contains("$$SpringCGLIB$$"));
    }

    public static void main(String[] args) {
        run();
    }
}
