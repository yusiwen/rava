package demo;

import java.lang.annotation.ElementType;
import java.lang.annotation.Retention;
import java.lang.annotation.RetentionPolicy;
import java.lang.annotation.Target;

/**
 * Every element-value tag this slice covers, each with a default:
 * string, int, class literal, enum constant, nested annotation and arrays.
 */
@Retention(RetentionPolicy.RUNTIME)
@Target({
    ElementType.TYPE,
    ElementType.FIELD,
    ElementType.METHOD,
    ElementType.CONSTRUCTOR
})
public @interface Mark {
    String name() default "mark";

    int count() default 3;

    boolean flag() default true;

    long big() default 9L;

    double ratio() default 1.5;

    Class<?> type() default String.class;

    Color color() default Color.RED;

    Nested nested() default @Nested("inner");

    String[] aliases() default {"a"};

    int[] primes() default {2, 3};
}
