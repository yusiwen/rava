package demo;

import org.springframework.boot.ApplicationArguments;
import org.springframework.boot.ApplicationRunner;
import org.springframework.boot.SpringApplication;
import org.springframework.boot.WebApplicationType;
import org.springframework.boot.SpringBootConfiguration;
import org.springframework.context.annotation.Bean;

/**
 * A real non-web Spring Boot application: the M4.5 gate.
 *
 * <p>{@link SpringApplication} is started with {@link WebApplicationType#NONE}, so no
 * servlet container is created, and the context registers one {@link ApplicationRunner}
 * that prints the stable marker {@code boot:started}. The application is a real Boot
 * context ({@link SpringBootConfiguration}), but it deliberately carries no
 * {@code @EnableAutoConfiguration}: the full auto-configuration path is the C3 target
 * that follows the M4 gate, and pulling it in here would make this gate about Jackson,
 * SSL and task-executor auto-configuration rather than about Boot startup itself.
 *
 * <p>See the fixture README for the build command.
 */
@SpringBootConfiguration
public class BootNonWeb {

    public static void main(String[] args) {
        SpringApplication application = new SpringApplication(BootNonWeb.class);
        application.setWebApplicationType(WebApplicationType.NONE);
        application.run(args);
    }

    @Bean
    public ApplicationRunner bootMarker() {
        return (ApplicationArguments args) -> System.out.println("boot:started");
    }
}
