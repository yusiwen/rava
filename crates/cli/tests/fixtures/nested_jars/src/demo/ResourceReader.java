package demo;

import java.io.InputStream;

/** M4.4 nested-jar fixture: read a resource stream into text without a host path. */
public final class ResourceReader {
    private ResourceReader() {
    }

    public static String read(InputStream input) throws Exception {
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
