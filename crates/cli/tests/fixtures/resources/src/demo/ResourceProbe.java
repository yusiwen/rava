package demo;

import java.io.InputStream;
import java.net.URL;
import java.net.URLConnection;
import java.util.Enumeration;

/**
 * M4.1 fixture: exercises the real classpath resource APIs.
 *
 * The classpath is {@code classes} (this class plus {@code demo/data.txt}) followed by
 * {@code resources.jar} (the same name with a different payload), so {@code getResources}
 * must return both in that order.
 */
public final class ResourceProbe {
    private static String text(byte[] bytes) {
        StringBuilder builder = new StringBuilder(bytes.length);
        for (byte value : bytes) {
            builder.append((char) (value & 0xff));
        }
        return builder.toString();
    }

    private static String readAll(InputStream input) throws Exception {
        if (input == null) {
            return "<null>";
        }
        byte[] bytes = input.readAllBytes();
        input.close();
        return text(bytes);
    }

    public static void main(String[] args) throws Exception {
        ClassLoader loader = ResourceProbe.class.getClassLoader();

        URL first = loader.getResource("demo/data.txt");
        System.out.println("getResource.protocol=" + first.getProtocol());
        System.out.println("getResource.path.endsWith=" + first.getPath().endsWith("/demo/data.txt"));
        System.out.println("getResource.payload=" + readAll(loader.getResourceAsStream("demo/data.txt")));

        Enumeration<URL> all = loader.getResources("demo/data.txt");
        int count = 0;
        URL directory = null;
        URL jar = null;
        while (all.hasMoreElements()) {
            URL url = all.nextElement();
            count++;
            if (count == 1) {
                directory = url;
            } else if (count == 2) {
                jar = url;
            }
        }
        System.out.println("getResources.count=" + count);
        System.out.println("getResources.0.protocol=" + directory.getProtocol());
        System.out.println("getResources.0.payload=" + readAll(new URL(directory.toExternalForm()).openStream()));
        System.out.println("getResources.1.protocol=" + jar.getProtocol());
        System.out.println("getResources.1.path.endsWith=" + jar.getPath().endsWith("!/demo/data.txt"));
        System.out.println("getResources.1.payload=" + readAll(new URL(jar.toExternalForm()).openStream()));

        URL relative = ResourceProbe.class.getResource("data.txt");
        URL absolute = ResourceProbe.class.getResource("/demo/data.txt");
        URL constructed = new URL(absolute, "data.txt");
        System.out.println("class.relative.protocol=" + relative.getProtocol());
        System.out.println("class.absolute.protocol=" + absolute.getProtocol());
        System.out.println("class.relative.same=" + relative.toExternalForm().equals(absolute.toExternalForm()));
        System.out.println("class.absolute.payload=" + readAll(absolute.openStream()));
        System.out.println("url.ctor.relative.payload=" + readAll(constructed.openStream()));
        System.out.println("url.forms.equal=" + absolute.toExternalForm().equals(absolute.toString()));
        System.out.println("url.equals.same=" + absolute.equals(constructed));
        System.out.println("url.hash.same=" + (absolute.hashCode() == constructed.hashCode()));
        System.out.println("class.missing=" + ResourceProbe.class.getResource("/demo/missing.txt"));

        InputStream stream = loader.getResourceAsStream("demo/data.txt");
        byte[] body = stream.readAllBytes();
        System.out.println("stream.length=" + body.length);
        System.out.println("stream.eof=" + stream.read());
        System.out.println("stream.eof.read=" + stream.read(new byte[8]));
        System.out.println("stream.available=" + stream.available());
        stream.close();

        URLConnection connection = first.openConnection();
        connection.connect();
        connection.setUseCaches(false);
        System.out.println("connection.protocol=" + connection.getURL().getProtocol());
        System.out.println("connection.lengthPositive=" + (connection.getContentLength() > 0));
        System.out.println("connection.lengthLongPositive=" + (connection.getContentLengthLong() > 0));
        System.out.println("connection.payload=" + readAll(connection.getInputStream()));
    }
}
