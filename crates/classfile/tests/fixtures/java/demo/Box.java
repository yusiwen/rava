package demo;

/** A generic class with two bounds and a generic method, so `Signature` is not trivial. */
public class Box<T extends Comparable<T> & java.io.Serializable> {

    protected T value;

    public Box(T value) {
        this.value = value;
    }

    public T get() {
        return value;
    }

    public <R> R map(java.util.function.Function<? super T, ? extends R> fn) {
        return fn.apply(value);
    }
}
