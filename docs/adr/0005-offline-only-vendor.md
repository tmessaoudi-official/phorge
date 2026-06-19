# ADR-0005: Vendoring is offline-only — determinism over convenience

- **Status:** Accepted (2026-06-19)
- **Deciders:** project author
- **Fuller design:** `docs/specs/2026-06-18-m5-project-model-design.md` — decision **M5-10**;
  M5 S3 plan `docs/plans/2026-06-18-m5-modules-packages.md`.

## Context

Phorge supports git dependencies (pinned by tag/rev). The project's central correctness contract is
a **byte-identical spine** (`run ≡ runvm ≡ php`) over committed examples, and that demands
deterministic, reproducible builds. Network access during a build is inherently non-deterministic
(a tag can move, a host can be down), which would break both reproducibility and the offline
example suite.

## Decision

`phg vendor` is the **only network-touching command**: clone → checkout the pinned rev → copy the
dependency's source into `vendor/<vendor>/<package>/` → content-hash → write `phorge.lock`.
`run` / `check` / `transpile` / `build` **never fetch**; they consult `vendor/` only, and a project
**auto-goes-offline** when `vendor/` is present. A required dependency that isn't vendored is a hard
error (`E-VENDOR-MISSING`). Examples that need dependencies **ship their `vendor/` committed**.

## Consequences

- **Reproducible, offline builds** — the same source always produces the same output; the example
  spine resolves with zero network (Go's vendoring model + Cargo's pinned lock, fused).
- The single network command is auditable in isolation; everything else is pure.
- **Deferred (documented in KNOWN_ISSUES, not regressions):** transitive dependencies (a dep's own
  `[require]`) and `phg build` merging `vendor/` are out of scope for the initial model.

## Alternatives rejected

- **On-demand fetch during `run`/`build`** — non-deterministic; breaks the byte-identical spine and
  the zero-network example guarantee. Convenience is not worth surrendering reproducibility.
