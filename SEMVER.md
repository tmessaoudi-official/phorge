# Versioning & compatibility policy

Phorj follows [Semantic Versioning](https://semver.org/) **with an explicit pre-1.0 contract**. The
current series is `0.x` (the binary reports `phg --version`).

## Pre-1.0 (the `0.x` series) — *may break, always documented*

While the major version is `0`, the public surface is **not yet frozen**. A minor release (`0.y`)
**may** include a breaking change when it materially improves the language — but every break is:

1. **Documented** in [`CHANGELOG.md`](CHANGELOG.md) under a clearly marked **`### Breaking`** heading
   for that version, stating *what* changed, *why*, and the *migration*.
2. **Mechanically caught** where possible — a removed or renamed construct fails the
   [conformance corpus](conformance/) (so a break can't ship silently), and a *deprecation* of a
   stdlib symbol emits the **`W-DEPRECATED`** lint for at least one minor release before removal (see
   [`docs/DEPRECATION.md`](docs/DEPRECATION.md)).

Patch releases (`0.y.z`) are bug fixes and additive only — never breaking.

This is the honest contract for a language still finding its shape: we keep the freedom to fix design
mistakes, and we pay for it with documentation and tooling rather than silent churn. The
[stability tiers](STABILITY.md) say which parts of the surface are *intended to last* (and so are the
most reluctantly broken) versus those still in flux.

## 1.0 and beyond — *strict SemVer, frozen surface*

At `1.0.0` the **stable** tier of [`STABILITY.md`](STABILITY.md) freezes:

- **MAJOR** (`x.0.0`) — a breaking change to the stable surface (a removed/renamed construct, a changed
  evaluation result, a narrowed type rule). Reserved for genuine necessity.
- **MINOR** (`1.y.0`) — backward-compatible additions: new language constructs, new stdlib modules or
  functions, new CLI commands/flags, relaxed (more-accepting) type rules.
- **PATCH** (`1.y.z`) — backward-compatible bug fixes.

A construct in the **experimental** tier is exempt from this freeze even after 1.0 — it may change or
be removed in a minor release (with a CHANGELOG note), and graduates to **stable** only when its design
has settled. The **deprecated** tier may be removed in the next **MAJOR**.

## What "compatible" means for Phorj

Phorj's defining invariant is that the three backends — the tree-walking interpreter (`run`), the
bytecode VM (`runvm`), and the transpiled PHP — produce **byte-identical** output for every program.
Compatibility is therefore about *observable program behavior*, not internal representation: the `Op`
set, the bytecode format, the AST, and the emitted-PHP *shape* are all implementation details that may
change at any time, as long as a stable-tier program's **output** is unchanged.

The standalone-executable container format (`phg build`) is **versioned and CRC-guarded** internally; a
binary built by one `phg` is not guaranteed to be readable by a different `phg` (rebuild from source).
