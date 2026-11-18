# S3 — The plan for hosting, and the ledger that keeps it honest

- **Slice:** S3
- **Requirements:** process
- **Acceptance:** `bash scripts/workflow-check.sh .` exited 0 on 2026-11-18.

## What surprised me

**The decisions that carried over were the ones about the machine, not the library.** Decode
once, heap indices, crate direction, the collector: four ADRs survived the reset almost
unchanged. The five that did not — hosting, natives, class identity, the image, threads — are
exactly the ones the old project had answered differently, and each answer had been forced by
the stub library.

**The plan got smaller and sharper.** The previous ledger carried sixty slices because the stub
library needed a slice per class of API. Hosting turns that into a native-surface problem: the
work is enumerable from the classes the library declares, and a slice is "the natives this
program needs", not "one more `java.util` class".

## What the specification says

JVMS does not define a class library; it defines the machine and the class-file format. That is
the seam the whole plan rests on: the VM can be conformant while the library is somebody
else's, and the interface between them is the native method table plus the VM's internal
classes.

## The bug I hit

Nothing broke at the workflow level; the old checker ran unchanged against the new ledger. The
one adjustment was scope: `--strict` (every phase done) is the completion check, so CI runs the
checker without it until the last milestone closes.

## Open threads

- The GitHub repository is recreated by the human; the local history starts fresh and signed.
- M0's first slice is the image reader; until it lands, `--home` has no real meaning.
