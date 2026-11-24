package demo;

import java.lang.annotation.ElementType;
import java.lang.annotation.Repeatable;
import java.lang.annotation.Retention;
import java.lang.annotation.RetentionPolicy;
import java.lang.annotation.Target;

/** An enum used as an annotation element value (`e` tag). */
enum Color {
    RED,
    GREEN,
    BLUE
}

/** Every element-value tag except arrays-of-arrays, all with defaults. */
@Retention(RetentionPolicy.RUNTIME)
@Target({
    ElementType.TYPE,
    ElementType.FIELD,
    ElementType.METHOD,
    ElementType.PARAMETER,
    ElementType.RECORD_COMPONENT,
    ElementType.ANNOTATION_TYPE
})
@interface Defaults {
    boolean flag() default true;

    byte small() default 7;

    char letter() default 'r';

    short medium() default 300;

    int count() default 3;

    long big() default 42L;

    float ratio32() default 1.5f;

    double ratio() default 2.25;

    String name() default "anon";

    Class<?> type() default String.class;

    Color color() default Color.RED;

    Nested nested() default @Nested("d");

    int[] primes() default {2, 3, 5};

    String[] aliases() default {"x"};
}

/** The nested annotation type the `@` element-value tag needs. */
@Retention(RetentionPolicy.RUNTIME)
@Target(ElementType.ANNOTATION_TYPE)
@interface Nested {
    String value();
}

/** Repeatable on its own; javac encodes repeats inside {@link Tags}. */
@Retention(RetentionPolicy.RUNTIME)
@Target({ElementType.TYPE, ElementType.FIELD, ElementType.METHOD})
@Repeatable(Tags.class)
@interface Tag {
    String value();
}

/** The repeatable container, kept exactly as javac encodes it. */
@Retention(RetentionPolicy.RUNTIME)
@Target({ElementType.TYPE, ElementType.FIELD, ElementType.METHOD})
@interface Tags {
    Tag[] value();
}

/** A type-use annotation with a default, for parameter and type annotations. */
@Retention(RetentionPolicy.RUNTIME)
@Target({ElementType.TYPE_USE, ElementType.PARAMETER, ElementType.FIELD, ElementType.METHOD})
@interface TypeUse {
    String value() default "tu";
}

/** Class retention: lands in `RuntimeInvisibleAnnotations`. */
@Retention(RetentionPolicy.CLASS)
@Target({ElementType.TYPE, ElementType.FIELD, ElementType.METHOD, ElementType.PARAMETER})
@interface Hidden {
    String value() default "h";
}

/** Class retention and type-use only: lands in `RuntimeInvisibleTypeAnnotations`. */
@Retention(RetentionPolicy.CLASS)
@Target(ElementType.TYPE_USE)
@interface HiddenType {
}
