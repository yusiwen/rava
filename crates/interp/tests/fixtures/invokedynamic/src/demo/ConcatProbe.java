package demo;

/** M2.18 fixture: compiler-generated StringConcatFactory call sites. */
public class ConcatProbe {
    public static String concat(String s, int n) {
        return "v=" + s + "/" + n;
    }

    public static String constants(String s) {
        return "[" + s + "]";
    }

    public static String escaping(String s) {
        return s + "\u0001" + s;
    }

    public static String emptyAndNull(String s) {
        return "" + s + "";
    }

    public static String primitives(int i, boolean z, char c, Object o) {
        return "p=" + i + "," + z + "," + c + "," + o;
    }
}
