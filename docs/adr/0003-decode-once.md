# ADR-0003 — Decode bytecode once, at load

- **Status:** accepted
- **Date:** 2026-11-18
- **Supersedes:** —
- **Superseded by:** —

## Context

A method body is a byte array whose instructions have variable length, whose branch operands are
byte offsets, and whose meaning is fixed by the version of the file. Doing that arithmetic in
the dispatch loop would put address calculation on the hot path and class-file policy inside
the interpreter.

## Decision

Decode every `Code` attribute once, at load, into `Vec<Instr>` with operands already read.
Branch and switch targets are translated to **instruction indices**, not byte offsets. The
dispatch loop matches on `Instr` and never computes an address.

## Alternatives

| Alternative | Why rejected |
| :--- | :--- |
| Decode on every dispatch | A variable-length decode per instruction, and the loop would own version logic. |
| Keep raw bytes and a decoder table outside | The same address arithmetic, one indirection further away. |

## Consequences

**Positive**

- The loop is a `match` over a closed enum; the verifier can walk the same form.
- A malformed instruction is rejected at load, with the method and pc named.

**Negative / accepted costs**

- A decode step and its allocation at load time; `Instr` must grow with the instruction set.
- The decoded form is larger than the bytes; `disasm` exists to show it.

**Follow-up**

- None planned.
