package demo;

/** M2.19 fixture: javac-emitted LambdaMetafactory call sites. */
public class LambdaProbe {
    /** A capturing lambda whose target is static: captures a String and an int. */
    public static IntFn capture(String prefix, int base) {
        return s -> prefix.length() + base + s.length();
    }

    /** An unbound method reference: the SAM argument is the target receiver. */
    public static IntFn unbound() {
        return String::length;
    }

    /** A static method reference: no capture, the SAM argument is the target argument. */
    public static IntFn staticRef() {
        return LambdaProbe::staticLength;
    }

    public static int staticLength(String s) {
        return s.length();
    }

    public static Sink sink(String value) {
        return new Sink(value);
    }

    /** A no-argument void lambda that captures a receiver and a String. */
    public static Runnable runnable(Sink sink, String prefix) {
        return () -> sink.set(prefix);
    }

    public static void runIt(Runnable r) {
        r.run();
    }

    public static String runAndRead(Runnable r, Sink sink) {
        r.run();
        return sink.value;
    }

    public static int applyIt(IntFn fn, String s) {
        return fn.apply(s);
    }

    public static String className(Object o) {
        return o.getClass().getName();
    }

    /** An ordinary instance target, dispatched through invokeinterface on the class. */
    public static IntFn impl() {
        return new IntFnImpl();
    }

    /** A String receiver where the interface method is a registered String native. */
    public static int charSequenceLength(CharSequence cs) {
        return cs.length();
    }

    /** An ArrayList receiver where the interface method is a registered collection native. */
    public static int listSize(java.util.List<String> list) {
        return list.size();
    }

    public static java.util.List<String> oneElementList() {
        java.util.ArrayList<String> list = new java.util.ArrayList<>();
        list.add("a");
        return list;
    }

    /** A constructor method reference (`REF_newInvokeSpecial`). */
    public static SinkFactory factory() {
        return Sink::new;
    }

    public static Sink makeSink(SinkFactory factory, String value) {
        return factory.make(value);
    }

    public static String sinkValue(Sink sink) {
        return sink.value;
    }
}
