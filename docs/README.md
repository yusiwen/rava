# Documentation

Index for the new project's documents. P0 and P1 were accepted on 2026-11-23; the
status at the top of each document is authoritative.

## Kinds

| Kind | Where | Answers | Rule |
| :--- | :--- | :--- | :--- |
| State | [`requirements.md`](requirements.md), [`open-questions.md`](open-questions.md), [`../PROJECT.md`](../PROJECT.md) | what is true now | the ledger and the requirements win disagreements |
| Decisions | [`adr/`](adr/) | why this and not the alternative | four sections, one status, numbered from 0001 |
| Measurement | `benchmarks.md` (not created yet) | how fast, and what makes measuring hard | the only home of a performance number |
| Explanation | `explanation/` (not created yet) | how a mechanism runs end to end | must quote real output |
| History | `notes/` (not created yet) | what happened, and what it taught | one note per completed slice |

The project starts with state and decisions. Measurement, explanation and history
documents are added when there is something to record.

## Index

| Path | Kind | Owns |
| :--- | :--- | :--- |
| [`requirements.md`](requirements.md) | state | R1–R13, C0–C4, milestones, performance, scope ordering, risks |
| [`open-questions.md`](open-questions.md) | state | all resolved answers Q1–Q18; a decision record |
| [`adr/`](adr/) | decisions | 0001–0012, one decision per record |
| [`adr/README.md`](adr/README.md) | index | the ADR table and the format rules |
| [`../stdlib/README.md`](../stdlib/README.md) | state | the two generated class libraries, and what each one is for |
| [`../stdlib/c1/README.md`](../stdlib/c1/README.md) | state | the M2.2 JDK 17 C1 signature tree, its normalization boundary and its gates |

## Rule

One fact, one owner. If a requirement and an ADR disagree, the requirement is the
contract and the ADR is the reasoning; fix one of them in the same change. An accepted
ADR is immutable and is superseded, never rewritten.
