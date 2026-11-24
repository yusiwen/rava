package dev.rava.reference;

import org.springframework.http.MediaType;
import org.springframework.web.bind.annotation.GetMapping;
import org.springframework.web.bind.annotation.RestController;

/** Serves the reference application's stable plain-text body at {@code /}. */
@RestController
public class HelloController {

    /** The exact response body the compatibility gates assert. */
    public static final String BODY = "rava reference application\n";

    @GetMapping(value = "/", produces = MediaType.TEXT_PLAIN_VALUE)
    public String index() {
        return BODY;
    }
}
