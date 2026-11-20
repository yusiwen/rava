# S5 — Slots, not values

- **Slice:** S5
- **Requirements:** R2
- **Acceptance:** `cargo test -p rava-vm --test instr` exited 0 on 2026-11-20 (6 tests; the
  interpreter's 8 stack tests run under `cargo xtask ci`).

## What surprised me

**The stack instructions are specified in slots; our stack holds values.** `dup2` duplicates
two slots — one `long`, or two `int`s — so the interpreter cannot decide which by counting
entries. Every form now branches on the *category* of the top value (and, for `dup2_x2`, of the
third value down). The six instructions became eight tests: one per instruction, plus the two
extra forms of `dup2_x2`.

**`wide` is not an instruction.** It widens a local-variable index from `u1` to `u2` and, for
`iinc`, the increment from `i8` to `i16`. Widening the enum's slot type to `u16` (and `Iinc`'s
increment to `i16`) was cheaper than a second set of ten variants, and it made `wide` a
three-line prefix handler.

**The verifier had to change unit.** It used to count *values* and admit it was approximate:
`pop2` "removes two slots, or one value when that value is a long/double… the conservative
reading is two values". With `dup2` the approximation starts rejecting valid code — `dup2` on a
`long` needs one value, on two `int`s two — so the depth check now counts *slots*, which is
what `max_stack` is written in. Every instruction's slot effect is fixed by its opcode, so this
is exact and needs no type information; the type-checking half of JVMS §4.10.1 is still out of
scope (ADR-0011). The rewrite also made `long`/`double` arithmetic and `getfield`/`putfield`
depth accounting exact for the first time.

## What the specification says

JVMS §6.5 gives `dup2_x2` four forms and says which applies by the category of the values; §4.7.3
defines `max_stack` as a depth in slots; §2.6.2 says a `long` or `double` occupies two slots.
The decoder's `goto_w` offset is a signed 32-bit branch displacement from the opcode's address,
the same rule as the 16-bit `goto` with a wider field.

## The bug I hit

`dup2_x2` form 3 is `..., value3(cat2), value2, value1` — the category-2 value is the **third**
from the top, not the second. The first version picked forms 3 and 4 with one "is the value
below the top wide?" test, so the form-3 test underflowed asking for four values. Splitting the
decision (top wide? then second; else third) fixed it, and the four-form test now pins it.

## Measurements

| Class | Methods | With code |
| :--- | ---: | ---: |
| `java.lang.String` | 168 | 167 |
| `java.lang.Thread` | 114 | 94 |
| `java.util.ArrayList` | 68 | 68 |
| `java.util.HashMap` | 56 | 56 |
| **total** | **406** | **385** |

All 385 bodies decode. None of the four classes uses `wide` or `goto_w` (checked with
`javap -c`): both are implemented defensively, because a larger method needing them would fail
at load rather than at run time, and the decoder already had the error path.

The measurement that forced this slice still holds: `String.join` needed `dup2`, `Thread`,
`ArrayList` and `HashMap` needed `dup_x1`/`dup_x2`.

## Open threads

- `jsr`, `ret` and `jsr_w` stay unimplemented: they are forbidden from class-file 51 on, and
  version 65 is the target.
- The verifier is exact about depth and blind about types; a wrong-type program passes it and
  fails at run time with a `TypeMismatch` from the interpreter. ADR-0011 still describes the
  trade.
- Loading the four classes through `Shared::load_class` was probed and works (all five core
  classes link, including `String`), which is S6's starting point — it is not part of this
  slice's acceptance.
