package demo;

/**
 * A record whose component carries a runtime annotation and a type-use annotation. Both
 * are encoded inside the `Record` attribute, which is why the record's components need
 * their own annotation decoders.
 */
public record Circle(@Defaults(name = "component") @TypeUse("r") double radius) implements Shapes {

    @Override
    public double area() {
        return 3.0 * radius * radius;
    }
}
