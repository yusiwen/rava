package demo;

/** M2.19 fixture: a custom SAM interface whose method takes a String and returns an int. */
@FunctionalInterface
public interface IntFn {
    int apply(String s);
}
