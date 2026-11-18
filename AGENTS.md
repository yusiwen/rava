# AGENTS.md

An index, not a manual. Every answer below lives in the linked document: read it there
and do not restate it — not in this file, not in code comments, not in a final report.
If a fact is missing, add it to the document that owns it instead of creating a new one.

## Start here

1. Read `PROJECT.md` — the ledger, and the only authoritative state.
2. Run `bash scripts/workflow-check.sh .` — exit 0 means the ledger matches the disk.
3. Continue at the first slice that is not `done`; its acceptance command is in the
   slice row.

## Where the truth lives

| Document | Owns |
| :--- | :--- |
| `README.md` | What the project is, how to run it, what it deliberately does not do |
| `PROJECT.md` | Phases, milestones, slices, acceptance commands, decision index, resume, ways of working |
| `docs/README.md` | **The index of everything under `docs/`** — six kinds, one owner per fact |
| `docs/requirements.md` | Scope: R1–R12, non-goals, milestone gates, constraints |
| `docs/adr/` | Why each decision was made, including the alternatives that lost |
| `docs/notes/` | What each completed slice taught — one note per slice |

## Decisions to read before touching code

`0001` host the reference class library · `0002` crate boundaries · `0003` load-time
decoding · `0004` heap-index references · `0005` JCL natives in Rust · `0006` class
identity by loader · `0007` licence · `0008` target version and the image · `0009`
collector · `0010` real threads · `0011` verifier scope · `0012` invokedynamic.

## Commands

| Purpose | Command |
| :--- | :--- |
| Enter the pinned toolchain | `nix develop` (or `direnv allow` once) |
| Full gate | `cargo xtask ci` |
| Ledger consistency | `bash scripts/workflow-check.sh .` |
| Architecture invariants | `bash scripts/check-invariants.sh` |
| Dump a class file | `cargo run -p rava-cli -- --dump <file.class>` |
| Disassemble | `cargo run -p rava-cli -- disasm <Class> --home <jdk> --cp <dir>` |
| Verify without running | `cargo run -p rava-cli -- verify <Class> --home <jdk> --cp <dir>` |
| Run a program | `cargo run -p rava-cli -- run <Class> --home <jdk> --cp <dir>` |
| Measure | `cargo run --release -p rava-cli -- bench <Class> --home <jdk> --cp <dir>` |

Git flow, the note-per-slice rule and the merge procedure are in `PROJECT.md` § "Ways of
working"; the definition of done is in § "Definition of Done". They change in one place, so
do not copy them here.
