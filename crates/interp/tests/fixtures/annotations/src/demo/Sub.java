package demo;

/** A subclass without its own inherited annotation: it inherits `@InheritedMark("own")`. */
@Mark(name = "sub")
public class Sub extends Annotated {
}
