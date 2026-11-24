package demo;

import org.springframework.context.annotation.AnnotationConfigApplicationContext;
import org.springframework.context.annotation.Bean;
import org.springframework.context.annotation.Configuration;

/**
 * M2.23 acceptance fixture: start a real Spring {@code AnnotationConfigApplicationContext}
 * and read one bean out of it.
 *
 * <p>{@code proxyBeanMethods=false} keeps the configuration class out of CGLIB, which is
 * M3 work; the context itself, its bean factory, annotation processing and the bean
 * instantiation path are the real Spring code from the pinned jars.
 */
@Configuration(proxyBeanMethods = false)
public class SpringSmoke {

    @Bean
    String hello() {
        return "hello";
    }

    public static void main(String[] args) {
        try (AnnotationConfigApplicationContext ctx =
                new AnnotationConfigApplicationContext(SpringSmoke.class)) {
            System.out.println(ctx.getBean(String.class));
        }
    }
}
