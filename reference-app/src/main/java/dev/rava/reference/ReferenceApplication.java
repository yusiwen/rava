package dev.rava.reference;

import org.springframework.boot.SpringApplication;
import org.springframework.boot.autoconfigure.SpringBootApplication;

/**
 * The rava reference application.
 *
 * <p>It exists to be run on the rava runtime: a Spring Boot servlet application with one
 * stable {@code text/plain} endpoint and no JSON, database, Actuator or security surface.
 */
@SpringBootApplication
public class ReferenceApplication {

    public static void main(String[] args) {
        SpringApplication.run(ReferenceApplication.class, args);
    }
}
