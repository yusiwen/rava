package demo;

/** M2.19 fixture: an ordinary instance that implements the fixture's SAM interface. */
public class IntFnImpl implements IntFn {
    @Override
    public int apply(String s) {
        return s.length() + 1;
    }
}
