# Architecture Decision Records

Canonical records of Phorj's **load-bearing architectural decisions** — the *verdict* and its
*consequences* for each, in [Michael Nygard's ADR format](https://cognitect.com/blog/2011/11/15/documenting-architecture-decisions).

**Authority split** (see [ADR-0001](0001-no-shared-run-vm-ir.md) and `docs/ARCHITECTURE.md`):

- An **ADR here is canonical for the decision + its consequences** — short, self-contained, and
  **immutable once Accepted** (a decision is changed by adding a new ADR that *supersedes* it, never
  by editing the old one).
- The **design specs in `docs/specs/`** remain canonical for the *fuller design exploration* — the
  alternatives weighed, the research, the numbered decision logs. Each ADR links back to its spec.

This division resolves the earlier "specs are the ADRs" policy: a 7–16 KB design spec is a design
*document*, not a discoverable one-page decision *record*. ADRs distill; specs explore.

## Index

| ADR | Decision | Fuller design |
|-----|----------|---------------|
| [0001](0001-no-shared-run-vm-ir.md) | Three backends as free functions — no shared IR, no `Backend` trait | ARCHITECTURE.md; ecosystem E-1 |
| [0002](0002-erasure-not-monomorphization.md) | Generics are erased, not monomorphized (TS→JS model) | m3-language-roadmap-design D-L2/D-L4/D-L9 |
| [0003](0003-rc-not-tracing-gc.md) | Reference counting (`Rc`/`Drop`), not a tracing GC | m2-p5-object-model-design |
| [0004](0004-single-file-brace-namespace-php.md) | PHP emission is a single self-contained brace-namespace file | m5-project-model-design M5-7 |
| [0005](0005-offline-only-vendor.md) | Vendoring is offline-only — determinism over convenience | m5-project-model-design M5-10 |

## Adding a new ADR

Number it sequentially, use the format of the existing records (Status / Context / Decision /
Consequences / Alternatives rejected), set **Status: Accepted (date)**, and link the `docs/specs/`
or `docs/plans/` document that holds the fuller rationale. To reverse a decision, add a new ADR with
`Status: Accepted` that notes "Supersedes ADR-NNNN", and set the old one's status to
`Superseded by ADR-MMMM` (the only edit ever made to an accepted ADR).
