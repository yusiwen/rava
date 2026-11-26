package demo;

import java.util.concurrent.CountDownLatch;
import java.util.concurrent.Executors;
import java.util.concurrent.LinkedBlockingQueue;

/**
 * The M5 boundary probes: the single-threaded VM, its executor/latch/queue limits and its
 * empty Java stack traces.
 *
 * <p>Each blocking method sets {@link #caught} to false and then attempts the operation
 * inside a {@code catch (Throwable)}. The VM reports a would-block operation as a
 * {@code VmError}, which never becomes a Java throwable, so the catch body does not run and
 * a later {@link #wasCaught()} reads false. That is the property the Rust test asserts: the
 * failure is not catchable from Java.
 *
 * <p>Everything here is ordinary Java compiled with {@code javac --release 17}; the fixture
 * is committed so the test needs no JDK.
 */
public final class M5Probe {

    private static volatile boolean ran;
    private static boolean caught;

    private M5Probe() {
    }

    /** `Thread.start` does not run the task: the flag stays false and `join` returns. */
    public static boolean threadStartRan() throws InterruptedException {
        ran = false;
        Thread thread = new Thread(() -> ran = true);
        thread.start();
        thread.join();
        return ran;
    }

    /** `Thread.isAlive()` is always true on the single-threaded VM. */
    public static boolean threadIsAlive() {
        return new Thread(() -> {
        }).isAlive();
    }

    /** `CountDownLatch.getCount()` is a non-blocking query and must return the count. */
    public static long latchCount() {
        return new CountDownLatch(3).getCount();
    }

    /** `CountDownLatch.await()` with a positive count would block. */
    public static boolean latchAwaitBlocking() {
        caught = false;
        try {
            new CountDownLatch(1).await();
            return true;
        } catch (Throwable throwable) {
            caught = true;
            return false;
        }
    }

    /** `LinkedBlockingQueue.offer` is non-blocking: the first offer fits, the second does not. */
    public static boolean queueOfferSemantics() {
        LinkedBlockingQueue<Object> queue = new LinkedBlockingQueue<>(1);
        return queue.offer(new Object()) && !queue.offer(new Object());
    }

    /** `LinkedBlockingQueue.put` on a full bounded queue would block. */
    public static boolean queuePutFull() {
        caught = false;
        try {
            LinkedBlockingQueue<Object> queue = new LinkedBlockingQueue<>(1);
            queue.offer(new Object());
            queue.put(new Object());
            return true;
        } catch (Throwable throwable) {
            caught = true;
            return false;
        }
    }

    /** `LinkedBlockingQueue.take` on an empty queue would block. */
    public static boolean queueTakeEmpty() {
        caught = false;
        try {
            new LinkedBlockingQueue<>(1).take();
            return true;
        } catch (Throwable throwable) {
            caught = true;
            return false;
        }
    }

    /** An executor factory would need a worker thread. */
    public static boolean executorPool() {
        caught = false;
        try {
            Executors.newSingleThreadExecutor();
            return true;
        } catch (Throwable throwable) {
            caught = true;
            return false;
        }
    }

    /** The `catch (Throwable)` body did not run for the last blocking method. */
    public static boolean wasCaught() {
        return caught;
    }

    /** Java stack traces are empty: the array has no frames. */
    public static int stackTraceLength() {
        return new Throwable().getStackTrace().length;
    }

    /** `StackWalker.walk` sees an empty stream (Boot's main-class deduction relies on it). */
    public static long stackWalkCount() {
        return StackWalker.getInstance(StackWalker.Option.RETAIN_CLASS_REFERENCE)
                .walk(frames -> frames.count());
    }
}
