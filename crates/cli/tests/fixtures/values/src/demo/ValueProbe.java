package demo;

import java.lang.management.ManagementFactory;
import java.math.BigInteger;
import java.time.Duration;
import java.time.Instant;
import java.util.Random;

/**
 * Runtime values that must match the JDK (#71): uptime, `Instant`/`Duration` text, the
 * `Random` sequence for a fixed seed and the `BigInteger` constants/hash.
 *
 * <p>Each method returns the JDK's answer; the Rust test compares it. The fixture is
 * ordinary Java compiled with {@code javac --release 17} and committed, so the test needs
 * no JDK.
 */
public final class ValueProbe {

    private ValueProbe() {
    }

    public static boolean uptimePositive() {
        return ManagementFactory.getRuntimeMXBean().getUptime() > 0;
    }

    public static boolean startTimePositive() {
        return ManagementFactory.getRuntimeMXBean().getStartTime() > 0;
    }

    public static String instantZero() {
        return Instant.ofEpochMilli(0).toString();
    }

    public static String instantMillis() {
        return Instant.ofEpochMilli(1735689600123L).toString();
    }

    public static String durationText() {
        return Duration.ofSeconds(90061).toString();
    }

    public static String durationFraction() {
        return Duration.ofMillis(1500).toString();
    }

    public static String randomSequence() {
        Random random = new Random(0);
        return random.nextInt() + "," + random.nextInt() + "," + random.nextInt(100);
    }

    public static String bigZero() {
        return BigInteger.ZERO.toString();
    }

    public static boolean bigConstants() {
        return "1".equals(BigInteger.ONE.toString()) && "10".equals(BigInteger.TEN.toString());
    }

    public static int bigHash() {
        return BigInteger.valueOf(4294967296L).hashCode();
    }

    public static String bigAdd() {
        return BigInteger.valueOf(2).add(BigInteger.valueOf(3)).toString();
    }
}
