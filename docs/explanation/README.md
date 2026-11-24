# Explanation

Explanation documents answer **how a rava mechanism runs end to end**. They are not
the ledger, not a decision record, and not a replacement for the code: they name the
entry point, the data flow, the artifacts and the commands that prove the description.

`PROJECT.md` owns state (what is done, which slice, which gate). The ADRs own the
reasoning (why this design and not another). The class-library READMEs own the state of
their trees. This directory owns the mechanism narrative and the interpretation of
evidence that spans those files.

## What belongs here

- Inputs, outputs and transform steps of a pipeline.
- The runtime path from a command or a Java class to a mechanism's result.
- The exact commands and quoted output that support the explanation.
- The boundary: what the mechanism does **not** prove, and why.
- Links to the files that own the facts; do not copy a state table here.

A note in `docs/notes/` is history: what happened and what it taught. An explanation
is timeless: how the current mechanism works, even after the slice that introduced it
has been superseded.

## Document contract

Every explanation document should contain:

1. **Question** — the one question the document owns.
2. **Entry point** — command, class, or API that starts the mechanism.
3. **Flow** — ordered steps with the relevant source paths.
4. **Evidence** — commands and real output (or a link to the gate that records it).
5. **Boundaries** — what is not implemented, normalized away, skipped or synthetic.
6. **Owners** — links to the state/decision documents instead of repeating them.

When code changes a described mechanism, update the explanation in the same change.
When the fact is only a status, update `PROJECT.md` instead.

## Index

| Document | Question it owns | Status |
| :--- | :--- | :--- |
| [`c1-metadata.md`](c1-metadata.md) | What exactly does the M2 C1 metadata diff compare, and what does “diff 0” prove? | current |
| [`spring-context.md`](spring-context.md) | How does a real Spring `AnnotationConfigApplicationContext` reach a bean on rava? | current |

## Planned implementation documents

These are the next explanation documents, in priority order. Each is a separate
PR-sized writing task; a document is done only when its commands have been re-run and
its boundaries re-checked against `PROJECT.md`.

| Priority | Document | Question it must answer |
| :--- | :--- | :--- |
| P0 | `compatibility-gates.md` | How are C0–C4 measured, which artifact/command owns each gate, and what is the promotion rule? |
| P0 | `class-loading-and-libraries.md` | How do `ClassSource`, loader identity, parent-first delegation, the two `stdlib` trees and the native registry fit together? |
| P1 | `interpreter-execution.md` | How do decoded IR, frames, static/instance calls, the native ABI, exceptions, `invokedynamic`, lambdas and streams execute a method? |
| P1 | `reflection-and-metadata.md` | How does a C1 metadata record become a `Class`/member/annotation/generic `Type` at runtime? |
| P2 | `testing-and-evidence.md` | How are fixtures, golden dumps, opt-in Spring acceptance and reproducible stub trees generated and checked? |
| P2 | `notes/M2.md` (history) | What did M2 teach, which blockers appeared in order, and what remains fragile? |

## Current boundaries

The C1 explanation deliberately records a **qualified** metadata pass: 1,011 classes
have a release-17 `ct.sym` reference, and 36 manifest owners are documented placeholders
excluded from the diff. The Spring explanation records the exact single-bean smoke
context and its opt-in local acceptance. Neither is a claim that every C1/C3 surface is
complete.

The machine-readable state remains in [`PROJECT.md`](../../PROJECT.md); the pinned
versions remain in [`compat/targets.toml`](../../compat/targets.toml).
