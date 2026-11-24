package demo;

/** The second permitted subtype of the sealed interface. */
public final class Square implements Shapes {

    @Override
    public double area() {
        return 1.0;
    }
}
