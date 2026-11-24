package demo;

import java.util.List;
import java.util.Map;

/**
 * A generic class with the shapes generic reflection has to preserve: a bounded type
 * parameter, a parameterized superinterface, a parameterized field, a generic array, two
 * wildcard arguments, a member-class owner and generic constructors and methods.
 */
public class Box<T extends Number> implements Comparable<Box<T>> {
    public List<String> labels;
    public T[] values;
    public List<? extends Number> upper;
    public List<? super Integer> lower;
    public Map.Entry<String, Integer> entry;

    public Box(T value) {
    }

    public <C extends Number> Box(List<C> seeds) {
    }

    @Override
    public int compareTo(Box<T> other) {
        return 0;
    }

    public int size() {
        return 0;
    }

    public T value() {
        return null;
    }

    public <E extends CharSequence> List<E> copy(List<E> in) {
        return in;
    }
}
