# m5 — the M5 boundary fixture

`demo/M5Probe` is ordinary Java compiled with `javac --release 17`; it exercises the
single-threaded VM's declared M5 boundary (`docs/explanation/class-loading-and-libraries.md`
§5e, `docs/slices/M4.md` M4.5):

- `Thread.start` does not run the task, `join` returns, `isAlive()` is true;
- `CountDownLatch.getCount()` answers, `await()` with a positive count would block;
- `LinkedBlockingQueue.offer` is non-blocking, `put`/`take` would block;
- an `Executors` factory would need a worker;
- `Throwable.getStackTrace()` is empty and `StackWalker.walk` sees an empty stream.

Every blocking method catches `Throwable`; the VM reports the would-block case as a
`VmError`, so the catch body never runs. The Rust test asserts that too.

Committed files:

- `src/demo/M5Probe.java` — the source.
- `classes/demo/M5Probe.class` — the compiled class, so CI needs no JDK.

Rebuild after editing the source (needs a JDK 17 `javac`):

```sh
JAVAC=/path/to/jdk-17/bin/javac
"$JAVAC" --release 17 -d crates/cli/tests/fixtures/m5/classes \
  crates/cli/tests/fixtures/m5/src/demo/M5Probe.java
```
