package demo;

/** A member class with an owning parameterized type. */
public class Nest<T> {
    public class Inner<U> {
    }

    public Nest<String>.Inner<Integer> inner;

    public Nest() {
    }
}
