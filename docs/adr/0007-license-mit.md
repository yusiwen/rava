# ADR-0007 — Licence the project under MIT

- **Status:** accepted
- **Date:** 2026-11-18
- **Supersedes:** —
- **Superseded by:** —

## Context

The project is published as a teaching artefact. Without a licence the default is "all rights
reserved", which defeats publication; a copyleft licence would impose obligations on readers
who want to reuse fragments in their own work, and relicensing later needs every contributor's
consent.

## Decision

MIT, with `LICENSE` at the repository root and no per-file headers.

## Alternatives

| Alternative | Why rejected |
| :--- | :--- |
| Apache-2.0 | Adds patent and notice machinery that a teaching repository does not need. |
| GPL-2.0/3.0 | Copyleft blocks the reuse this artefact exists for. |
| Unlicense / CC0 | Waiving attribution is unusual for source code and is rejected by several licence scanners. |
| No licence | Nobody may legally use the work. |

## Consequences

**Positive**

- The whole repository is reusable under one permissive licence.

**Negative / accepted costs**

- Dependencies must remain licence-compatible; a copyleft dependency needs a new ADR.
- The hosted class library is GPLv2+CE, but it is loaded from the user's JDK and never
  redistributed here, so it does not change this repository's licence.

**Follow-up**

- None planned.
