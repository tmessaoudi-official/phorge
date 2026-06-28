# Governance

This document describes how decisions are made in Phorj. It is intentionally lightweight and will
grow as the project does.

## Current model: single maintainer (BDFL)

Phorj is currently led by its original author, **Takieddine Messaoudi**
([@tmessaoudi-official](https://github.com/tmessaoudi-official)), who has final say on design, scope,
and what gets merged. This is the right model for a pre-1.0, single-developer project: it keeps the
language coherent while the core is still being shaped.

## How decisions are recorded

Phorj does **not** keep a separate `adr/` tree. Architectural and language decisions live in two
durable places, and new decisions extend the relevant one:

- **`docs/specs/`** — frozen design documents, each with a numbered **Decisions Log** (e.g. the VM
  design, the language design, the ecosystem roadmap). These *are* the ADRs.
- **`docs/plans/`** — per-milestone implementation plans, each with its own decisions log capturing
  choices made during execution.

Status across milestones is tracked in [`docs/MILESTONES.md`](docs/MILESTONES.md); the forward plan in
[ROADMAP.md](ROADMAP.md); the long-term direction in [VISION.md](VISION.md).

## Proposing changes

- **Small fixes / docs / tests:** open a PR directly (see [CONTRIBUTING.md](CONTRIBUTING.md)).
- **Language or architecture changes:** open an issue to discuss first. Significant changes should be
  captured as (or fold into) a spec/plan decision before implementation. The
  [correctness invariants](docs/INVARIANTS.md) constrain what is acceptable — a change that would
  break backend parity, the no-panic-on-input guarantee, or the std-only line needs explicit
  justification.

## Quality bar

Every change clears the same gate regardless of author: `cargo test`, `cargo clippy --all-targets`,
and `cargo fmt --check` all green, with tests written test-first. No exceptions for the maintainer.

## Future evolution

As contributors arrive, this document will evolve toward a more shared model — likely a small core
team with documented commit/merge rights and a deprecation/RFC process for language changes. Until
then, the BDFL model keeps the language designable.
