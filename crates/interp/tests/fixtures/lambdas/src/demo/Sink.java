package demo;

/** M2.19 fixture: a mutable receiver for a bound method reference and a void lambda. */
public class Sink {
    public String value;

    public Sink(String value) {
        this.value = value;
    }

    public void set(String value) {
        this.value = value;
    }

    public int addLength(String s) {
        return value.length() + s.length();
    }
}
