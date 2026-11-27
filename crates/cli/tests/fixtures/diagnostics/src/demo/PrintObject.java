package demo;

/** `println(Object)` on a class with no `toString` override: the identity fallback. */
public final class PrintObject {

    private PrintObject() {
    }

    public static void main(String[] args) {
        System.out.println(new Object());
    }
}
