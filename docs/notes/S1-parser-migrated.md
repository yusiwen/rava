# S1 — The class-file parser, migrated unchanged

- **Slice:** S1
- **Requirements:** R2
- **Acceptance:** `cargo test -p rava-classfile` exited 0 on 2026-11-18 (13 tests: 7 unit, 6 in
  `tests/parse.rs`).

## What surprised me

**The parser did not depend on the class library at all.** It parses bytes and knows JVMS
chapter 4; nothing in it referred to `java.lang`, the corpus, or the previous project's stub
library. It moved with no source change, which is the clearest evidence that the layer was
drawn in the right place (ADR-0002).

**The only thing that did not move was a test, not code.** The previous integration tests lived
in `crates/cli/tests/` and every one of them started by registering the stub library's natives.
They were dropped with the corpus; the parser's own tests, including the fixture embedded in
`tests/parse.rs`, came across untouched.

## What the specification says

JVMS chapter 4 defines the format; version 65 adds `Record`, `PermittedSubclasses` and the
module attributes, all of which the parser already accepted as raw attribute bytes. The parser
is version-agnostic by design (ADR-0008): it accepts a newer file and preserves what it does
not understand, and the loader owns any policy about what it will execute.

## The bug I hit

None. The migration compiled on the first `cargo build`, which is itself a finding: if a layer
that had grown over sixty slices moves without an edit, its boundary was enforced by more than
convention.

## Open threads

- The corpus loop that used to live in `tests/parse.rs` was removed rather than left to skip:
  it asserted version 52 and a count of stub-library classes, and the new model has neither.
  A fixture directory for the hosted library is the replacement to write when S4 lands.
