# Implementation notes

One note per completed slice, written when the slice's acceptance command first passes.
These are not tutorials: they record what the code does not make obvious.

## Convention

- One file per slice: `S<n>-<kebab-slug>.md`.
- English, and short. The useful questions are "what surprised me", "what does the
  specification actually say", and "what did the bug look like before it worked".
- Writing the note is part of the slice's Definition of Done in `PROJECT.md`.
- **Decisions belong in an ADR, not here.** A note may link to an ADR; it must not introduce
  one. If a note starts to argue for a design change, stop and write the ADR instead.

## Template

    # S<n> — <capability>

    - **Slice:** S<n>
    - **Requirements:** R<x>
    - **Acceptance:** `<command>` exited 0 on <date>

    ## What surprised me

    ## What the specification says

    ## The bug I hit

    ## Open threads
