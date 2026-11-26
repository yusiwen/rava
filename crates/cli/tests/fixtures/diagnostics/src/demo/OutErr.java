package demo;

/** Writes one line to each standard stream; the Rust test asserts they stay separate. */
public final class OutErr {
    public static void main(String[] args) {
        System.out.println("OUT-LINE");
        System.err.println("ERR-LINE");
    }
}
