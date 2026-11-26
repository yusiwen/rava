package demo;

/** Prints both streams, then never returns: a killed run must still have the lines. */
public final class Hang {
    public static void main(String[] args) {
        System.out.println("OUT-BEFORE-KILL");
        System.err.println("ERR-BEFORE-KILL");
        for (long spin = 0; ; spin++) {
            // Deliberately never returns; the test kills the process.
        }
    }
}
