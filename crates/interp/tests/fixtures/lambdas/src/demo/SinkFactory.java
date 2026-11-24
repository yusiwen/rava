package demo;

/** M2.19 fixture: a constructor method reference's SAM interface. */
@FunctionalInterface
public interface SinkFactory {
    Sink make(String value);
}
