package demo;

/** Overriding a generic return type with a concrete one makes javac emit a bridge method. */
public class IntBox extends Box<Integer> {

    public IntBox(Integer value) {
        super(value);
    }

    @Override
    public Integer get() {
        return value;
    }
}
