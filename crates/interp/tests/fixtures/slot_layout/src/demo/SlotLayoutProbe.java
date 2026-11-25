package demo;

/**
 * A compiled probe for JVM local-slot layout.
 *
 * Every method puts a wide value before a narrower one, so the narrower parameter must be
 * loaded from its JVM slot rather than from a compact argument index:
 *
 * <ul>
 *   <li>{@code shift(long, int)} is {@code (JI)J}: the {@code int} is local 2;</li>
 *   <li>{@code scale(long, double)} is {@code (JD)I}: the {@code double} spans 2..=3;</li>
 *   <li>{@code blend(long, float, int)} is {@code (JFI)J}: the {@code float} and
 *       {@code int} are locals 2 and 3;</li>
 *   <li>{@code instanceShift(long, int)} has the receiver at local 0, so its parameters
 *       start at 1 and 3.</li>
 * </ul>
 */
public final class SlotLayoutProbe {
    private SlotLayoutProbe() {
    }

    public static long shift(long value, int distance) {
        return value << distance;
    }

    public static int scale(long value, double factor) {
        return (int) (value * factor);
    }

    public static long blend(long value, float scale, int extra) {
        return value + (long) (scale * extra);
    }

    public long instanceShift(long value, int distance) {
        return value << distance;
    }

    public static void main(String[] args) {
        System.out.println("shift(1, 40) = " + shift(1L, 40));
        System.out.println("shift(3, 2) = " + shift(3L, 2));
        System.out.println("scale(4, 2.5) = " + scale(4L, 2.5));
        System.out.println("scale(3, -1.5) = " + scale(3L, -1.5));
        System.out.println("blend(10, 2.5, 4) = " + blend(10L, 2.5f, 4));
        System.out.println("blend(1, 0.5, 3) = " + blend(1L, 0.5f, 3));
        SlotLayoutProbe probe = new SlotLayoutProbe();
        System.out.println("instanceShift(1, 40) = " + probe.instanceShift(1L, 40));
        System.out.println("instanceShift(5, 3) = " + probe.instanceShift(5L, 3));
    }
}
