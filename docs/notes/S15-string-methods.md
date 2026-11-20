# S15 — The rest of `String`, and the local-variable bug underneath it

- **Slice:** S15
- **Requirements:** R4
- **Acceptance:** `cargo test -p rava-cli --test strings --test locals` exited 0 on 2026-11-20
  (2 + 1 tests), workspace 98.

## What surprised me

**`String.concat`'s `NullPointerException` was not about strings.** After S11 (array class
objects) and S14 (`System.arraycopy`), the probe that exercised `hashCode`, `equals`,
`compareTo`, `concat` and `substring` passed the first three and threw a bare NPE in the fourth.
The captured trace pointed at `StringConcatHelper.mix`, whose first instruction reads its
`String` argument with `aload_2` — and `mix(long lengthCoder, String value)` puts that argument
in local slots 2 and 3, because a `long` takes two slots (JVMS §2.6.1). This VM's frames hold
one `Value` per *value*, so `invoke` had bound `value` at slot 1 and `aload_2` read an untouched
NULL slot.

The same bug sat under every method with a `long`/`double` parameter before another one:
`Math.max(long, long)`'s second argument, `(long, ...)` constructors, `(double, ...)` methods.
Nothing in the suite reached one until `String.concat` did.

**The fix is small, the blast radius was not.** `invoke` now advances the target slot by two for
`Value::Long`/`Value::Double` (one otherwise); `take_frame` already sized locals to
`max_locals` slots, so loads and stores needed nothing. The operand stack stays value-based — it
is not addressable by bytecode index.

**A hand-written `lload_0` is easy to get wrong.** The first locals fixture used `0x1f`, which
is `lload_1`, not `lload_0`; the test failed with the same "expected long, found Ref" shape as
the bug it was meant to pin. The opcode table is the authority, not memory.

## What the specification says

JVMS §2.6.1: a `long` or `double` local occupies two local variables; the second may not be
addressed, and the next variable starts after it. The verifier's `max_locals` is already in
slots, which is why the frame was the right size and the wrong address. JVMS §4.4.5: a
`CONSTANT_Long`/`CONSTANT_Double` takes two constant-pool entries — the fixture's builder had to
learn that to emit the constants.

## Measurements

| Fact | Value |
| :--- | ---: |
| Strings target | 2 tests, 7 assertions (`length`, `charAt`, `hashCode`, `equals`, `compareTo`, `concat`, `substring`) |
| `"hello".hashCode()` | 99162322 |
| `"hello".concat(" world")` | `"hello world"` (was an NPE) |
| `"hello".substring(1, 4)` | `"ell"` |
| Locals target | 1 test, four shapes: `(J,J)`, `(J,String)`, `(D,String)`, `(J,int)` |
| Mutation | value-position binding → `Locals failed: expected long, found Ref(ObjRef(0))` |
| Workspace | 98 tests |

## Open threads

- `String.valueOf(...)`, `indexOf`, `getBytes` (charset), `String.format` and the `StringBuilder`
  mutators (`insert`/`reverse`/`delete`) are untested; the first three are expected to work now.
- The `long`/`double` fix is proven on arguments, not on `lstore`-only locals in a method with
  both wide and reference locals; the shapes in `locals.rs` cover the reported failure.
- Java-side stack traces remain empty — #21.
