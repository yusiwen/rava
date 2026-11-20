# S26 — `printStackTrace` compared with the reference launcher

- **Slice:** S26
- **Requirements:** R6 (exceptions), G2's differential method
- **Acceptance:** `cargo test -p rava-cli --test print_differential` exited 0 on 2026-11-20
  (2 tests: the stdout printing sample and the stderr stack-trace sample).

## What surprised me

**HotSpot hides the internal frames, and S19 had pinned our own six.** With the caught sample,
`java` prints one frame:

```
java.lang.IllegalStateException: boom
	at TraceSample.main(TraceSample.java:5)
```

while the VM printed six: `Throwable.fillInStackTrace`, `Throwable.<init>`, `Exception.<init>`,
`RuntimeException.<init>`, `IllegalStateException.<init>`, `TraceSample.main`. The JDK drops
leading frames for `fillInStackTrace` and for the constructor chain of the throwable being built;
S19's acceptance asserted the VM's first implementation (`LEN == 6`, `[0] == fillInStackTrace`),
which was wrong against the reference. The differential is what made that visible.

**`printStackTrace()` writes to `System.err`.** S21's test already knew it; the differential
compares stderr, not stdout, which is also why the sample needs no `System.setErr`.

## The fix

`visible_frames` in `crates/native/src/exceptions.rs` filters the leading
`Throwable.fillInStackTrace` frames and the `<init>` frames whose class is assignable to
`java/lang/Throwable`. The filtered list is what fills `stackTrace` **and** what
`Shared::traces` keeps, so the CLI's uncaught report and the Java view agree. `depth` follows the
filtered length.

## Measurements

| Fact | Value |
| :--- | ---: |
| Differential test | 2 tests (stdout sample, stderr sample) |
| Frames before / after | 6 → 1 for `new IllegalStateException("boom")` |
| Line numbers | matched: the host class's `LineNumberTable` gives `TraceSample.java:5` |
| Workspace tests | 117 |

## Open threads

- Only the caught case is compared; an **uncaught** throwable's CLI report is compared by
  content in `exceptions.rs`, not byte for byte. The filter above makes the two reports agree on
  the frame set, so that comparison is now feasible.
- `printStackTrace(PrintStream)` and `System.err` formatting are not exercised.
- `-XX:+ShowHiddenFrames`-style behaviour is fixed (hidden); the JDK flag has no equivalent here.
