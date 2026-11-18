# ADR-0011 — Verify structure first; types when hosting requires them

- **Status:** accepted
- **Date:** 2026-11-18
- **Supersedes:** —
- **Superseded by:** —

## Context

A faithful verifier is a dataflow analysis with a type lattice, and the decoded form does not
carry operand types. The hosted library's classes are valid, so type checking is not needed to
run them; malformed files, however, must not corrupt the VM.

## Decision

Verify the structural properties now: stack depth against `max_stack`, control-flow targets,
local indices, and handler entry depth. Parse past `StackMapTable` without validating it. Add
type checking only if hosting a program needs it, or if a malformed file demonstrates the need.

## Alternatives

| Alternative | Why rejected |
| :--- | :--- |
| The full type-checking verifier now | It is the largest single piece of a JVM and delays the boot milestone. |
| Trust everything | One malformed class would panic or corrupt the frame; a typed refusal is cheap. |
| Bounds checks in the interpreter instead | A per-instruction cost to defend against files the VM did not produce. |

## Consequences

**Positive**

- The VM cannot be corrupted by a structurally malformed file, and the check runs once per
  method.

**Negative / accepted costs**

- A type-confused but structurally valid file is accepted; operands are not validated.

**Follow-up**

- Type checking, if a hosted program or a fuzzing result asks for it.
