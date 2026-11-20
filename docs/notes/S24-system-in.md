# S24 — `System.in`, and the properties `StaticProperty` demanded

- **Slice:** S24
- **Requirements:** R12 (the library's VM-facing primitives)
- **Acceptance:** `cargo test -p rava-cli --test stdin` exited 0 on 2026-11-20 (3 tests: `read()`
  to EOF, `read(byte[])`, and a `BufferedReader(new InputStreamReader(System.in)).readLine()`
  compared with `java` through the CLI).

## What surprised me

**`System.in` needed almost no new machinery.** The bootstrap constructs
`new BufferedInputStream(new FileInputStream(FileDescriptor.in))` exactly like `out`/`err`, and
the read natives serve a stdin buffer. The read path is `read0` (one byte), `readBytes` (a chunk),
`available0`, `skip0`, `position0`, `length0`; the buffer is filled from the process's stdin on
first use, so a program that never reads does not block.

**The charset stack initialized `StaticProperty`, which throws for any missing property.**
Reading a line goes `BufferedReader` → `InputStreamReader` → `StreamDecoder` → `StaticProperty`
→ `System.getProperties()`, and `StaticProperty.getProperty` throws `InternalError` when the key
is absent. The minimal set S21 installed was enough for `println`; it was not enough for the
decoder. The fix is not a hack: the bootstrap now publishes the properties the JDK's own
`initPhase1` would (`java.home`, `user.*`, `os.*`, `native.encoding`, `java.io.tmpdir`,
`java.library.path`, the locale entries...).

## The bugs I hit

Both were mine, in the fixtures:

1. A hardcoded `getstatic System.in` constant-pool index (`7`) instead of the builder's
   `field_ref` — the fixture read the wrong field.
2. `iconst_4` written as `0x04` (that is `iconst_1`), so `new byte[4]` was `new byte[1]` and
   `BufferedInputStream.implRead` threw `IndexOutOfBoundsException` on its `off+len <= b.length`
   check. The second time in this project; the opcode table is one `javap` away.

## Measurements

| Fact | Value |
| :--- | ---: |
| Stdin test target | 3 tests |
| Properties installed | 40 keys (the JDK's `StaticProperty` set plus the stream keys) |
| Native additions | `FileInputStream.initIDs/read0/readBytes/available0/skip0/position0/length0`, `FileDescriptor.close0` |
| Workspace tests | 116 |

## Open threads

- The stdin buffer is filled with `read_to_end` on first use: `rava run` is batch, not
  interactive (a terminal waits for EOF).
- `FileInputStream.open0(String)` is still missing, so `new FileInputStream("path")` does not
  work; only the standard descriptor does.
- `printf`/`Formatter` remain #37; the stack-trace differential remains #39.
