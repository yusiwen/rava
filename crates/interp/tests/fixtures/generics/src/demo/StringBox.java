package demo;

/** A non-generic subclass, so its generic superclass is a parameterized type. */
public class StringBox extends Box<Integer> {
    public StringBox(Integer value) {
        super(value);
    }

    @Override
    public int compareTo(Box<Integer> other) {
        return 0;
    }
}
