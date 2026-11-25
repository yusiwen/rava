package demo;

import org.springframework.objenesis.ObjenesisStd;

/**
 * M3.6 fixture: Objenesis allocates an instance without running its constructor.
 *
 * {@code value} stays at the field's default because the constructor that would set it to
 * 42 never runs, while {@code marker} is 7 because allocating the instance still
 * initializes the class.
 */
public final class ObjenesisProbe {

    private ObjenesisProbe() {
    }

    /** A class whose constructor and static initializer are both observable. */
    public static class Constructed {
        public int value = 42;

        public static int marker;

        static {
            marker = 7;
        }

        public Constructed() {
            value = 42;
        }
    }

    public static void run() {
        Constructed instance = new ObjenesisStd().newInstance(Constructed.class);
        System.out.println("value=" + instance.value);
        System.out.println("marker=" + Constructed.marker);
    }

    public static void main(String[] args) {
        run();
    }
}
