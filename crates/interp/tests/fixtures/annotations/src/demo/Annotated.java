package demo;

/** A class, a field, a method and a constructor all carry annotations. */
@Mark(
        name = "class",
        count = 7,
        flag = false,
        big = 9000000000L,
        ratio = 2.25,
        type = String.class,
        color = Color.GREEN,
        nested = @Nested("n"),
        aliases = {"k", "it"},
        primes = {5, 7})
@InheritedMark("own")
public class Annotated extends Base {

    @Mark(name = "field")
    public int value;

    public String plain;

    @Mark(name = "method", count = 1)
    public void run() {
    }

    @Mark(name = "ctor")
    public Annotated() {
    }
}
