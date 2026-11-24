package demo;

/** A sealed interface: `PermittedSubclasses` on this type, `sealed` in its flags. */
public sealed interface Shapes permits Circle, Square {
    double area();
}
