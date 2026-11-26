package demo;

import java.net.URI;
import java.net.URISyntaxException;

/**
 * `java.net.URI` accessors against the JDK (#71): raw and decoded components, opaque
 * forms, resolve, and the two constructor errors.
 *
 * <p>Each method returns the JDK's answer; the Rust test compares it.
 */
public final class UriProbe {

    private static final String FORM = "http://example.com/a%20b?q=1%202#f%203";

    private UriProbe() {
    }

    private static URI form() {
        return URI.create(FORM);
    }

    public static String rawPath() {
        return form().getRawPath();
    }

    public static String path() {
        return form().getPath();
    }

    public static String rawQuery() {
        return form().getRawQuery();
    }

    public static String query() {
        return form().getQuery();
    }

    public static String rawFragment() {
        return form().getRawFragment();
    }

    public static String fragment() {
        return form().getFragment();
    }

    public static String schemeSpecificPart() {
        return form().getSchemeSpecificPart();
    }

    public static String rawSchemeSpecificPart() {
        return form().getRawSchemeSpecificPart();
    }

    public static String opaqueSchemeSpecificPart() {
        return URI.create("mailto:a%20b@example.com").getSchemeSpecificPart();
    }

    public static String resolve() {
        return URI.create("http://example.com/a/b/c").resolve("../d").toString();
    }

    public static String malformedAuthority() {
        try {
            new URI("http://exa mple.com/");
            return "no-throw";
        } catch (URISyntaxException exception) {
            return "URISyntaxException";
        }
    }

    public static String malformedEscape() {
        try {
            new URI("http://example.com/%zz");
            return "no-throw";
        } catch (URISyntaxException exception) {
            return "URISyntaxException";
        }
    }
}
