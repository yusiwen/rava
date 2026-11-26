package demo;

import java.io.InputStream;

/**
 * M4.3 executable-jar fixture: the `Main-Class` inside {@code app.jar}.
 *
 * It proves three things in one run: the launcher found this class in the executable jar,
 * it resolved {@link Helper} from the manifest's {@code Class-Path} entry, and it read
 * {@code data.txt} from {@code app.jar} through the M4.1 class-loader resource path. The
 * program arguments follow, in order, so the caller can distinguish "passed through"
 * from "consumed as a launcher option".
 */
public final class JarMain {
    public static void main(String[] args) throws Exception {
        System.out.println("jar:hello");
        System.out.println("helper:" + Helper.greeting());
        System.out.println("resource:" + read(JarMain.class.getResourceAsStream("/data.txt")));
        for (String argument : args) {
            System.out.println("arg:" + argument);
        }
    }

    private static String read(InputStream input) throws Exception {
        if (input == null) {
            return "<null>";
        }
        byte[] bytes = input.readAllBytes();
        input.close();
        StringBuilder text = new StringBuilder(bytes.length);
        for (byte value : bytes) {
            text.append((char) (value & 0xff));
        }
        return text.toString();
    }
}
