# Big Marathon: Cross-pkg lift → Soundness → Stdlib charter → Concurrency Plan

> Started 2026-06-29 from `e9d95a6`. Fully autonomous (`_AUTONOMOUS_3C=1`, full 30/8).
> Byte-identical `run ≡ runvm ≡ real PHP 8.5` spine; examples-ship-with-features; commit green
> self-contained changes as we go (project git autonomy). Gate:
> `PHORJ_PHP=/stack/tools/phpbrew/php/php-8.5.7/bin/php PHORJ_REQUIRE_PHP=1 cargo test --workspace`
> + `cargo clippy --all-targets` + `cargo fmt --check`.

## Decisions Log
- [2026-06-29] AGREED: Marathon = **all four spines**, in the recommended dependency order, **fully autonomous** (full 30/8).
- [2026-06-29] AGREED: Order = (1) Cross-package M-RT lift → (2) Soundness long-tail close → (3) Stdlib charter + breadth → (4) Concurrency + server (M6 W4). Rationale: #1 unifies type system ↔ modules and unblocks core.json multi-package + cross-package stdlib; #2 cleans the now-unified base; #3 writes the charter then breadth (multi-package core.json now possible); #4 capstone capability on a solid foundation.

## Progress
- [2026-06-29] S1.4 cross-package generic library types — DONE `718fa3d` (example-only, already worked).
- [2026-06-29] S1.1 cross-package traits — DONE `cc711b9` (loader symbol-table + resolve `Item::Trait`/`uses` rewrite + transpiler namespace bucketing).
- [2026-06-29] S1.2 lambdas/fn-values in library packages — DONE `5d7beb9` (loader `Expr::Ident` value-resolution arm; Main no-op).
- [2026-06-29] S1.3 core.json multi-package + cross-package map literals — DONE `d63cb9d` (JSON helper `\Main\` prefix + loader `Expr::Map` arm).
- NEXT: S1.5 cross-package parent calls, then Spine 2.

## Formal Plan

### Spine 1 — Cross-package M-RT lift
Lift the `package Main`-only wall. Loader mangle-pass + transpiler namespacing are the heavy machinery.
- S1.1 Cross-package **traits** (`trait` in a library package + cross-package `use`).
- S1.2 Lambdas / first-class fn-values **inside library packages** (loader rewrites lambda bodies + bare fn-value refs to mangled targets).
- S1.3 **core.json multi-package** (injected `Json` enum emitted namespaced, not flat).
- S1.4 Cross-package **generic library types** (`Box<T>` in a library package).
- S1.5 Cross-package **parent calls** (`parent.m()` across package boundary).

### Spine 2 — Soundness long-tail close
- S2.1 **Generic-result VM operand fix** (`id(7)+1` / `box.get()+1` on the VM) — thread reified generic result types into the compiler `CTy`.
- S2.2 **Method return-type overloading** (extend C1 from free-fns to methods).
- S2.3 **must-use Slice B/C** (bidirectional propagation of must-use).
- S2.4 **Pattern-cluster refinements** (while-let guards, same-binding or-patterns where provable).
- S2.5 **Late-static-binding alternative** ergonomics (or document as permanent non-feature with a clean path).

### Spine 3 — Stdlib charter + breadth (M4 / M-Batteries)
- S3.1 Write **`docs/specs/…-m4-stdlib-charter.md`** (naming, subject-first arg order, optional-vs-fault discipline, determinism tiers, native-vs-`.phg` policy).
- S3.2 **core.json encode + safe parse** breadth (now multi-package, post S1.3).
- S3.3 **sprintf / string-format** + more `Core.Text`.
- S3.4 **path / url** breadth on the determinism seam.

### Spine 4 — Concurrency + server (M6 W4)
- S4.1 Server hardening: HTTP **keep-alive**, **graceful shutdown/join**, per-worker metrics.
- S4.2 Uncolored **`spawn`** + **channels** (green threads on the VM's reified call frames), quarantined behind the determinism seam, tested outside `differential.rs`.
- S4.3 `phg serve` CLI + docs + example.

> Each slice: design-check → TDD → implement → full gate green → example + KNOWN_ISSUES/README → commit.
> Scope/deferrals captured in KNOWN_ISSUES as we go. Adjust slice boundaries as discovery dictates.
