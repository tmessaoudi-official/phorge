# GA Sequence — charter → DX → test → text → breadth-gaps → numerics → lift → release

> Multi-batch autonomous run chosen 2026-06-27. Move GA% / Global% via the highest-leverage
> remaining chunks, in dependency order. Each slice ships byte-identity-gated (run≡runvm≡real
> PHP 8.5) + a guide example, per the standing rules. Commit green; **never push**.

## Decisions Log
- [2026-06-27] AGREED: do **all four candidate batches in sequence** (developer: "do them in
  sequence"), in the **reordered** order below — NOT the M-Test-first framing I led with.
- [2026-06-27] AGREED: **charter-first reorder** (developer chose "Charter-first, as recommended").
  Rationale: M-Test/M-text/breadth all add stdlib surface; minting them before the conventions
  charter risks an API codemod later (the PascalCase-reshape pain). Charter governs all new stdlib.
- [2026-06-27] AGREED: at genuine design forks (Core.Test assertion API, Core.Regex API, Secret<T>
  model) **stop and ask** via AskUserQuestion before committing the public surface (developer choice).
- [2026-06-27] NOTE: roadmap docs were stale — **error model Slice 2 (throws/Result/try-catch) is
  BUILT** (`Op::Throw/PushHandler/PopHandler`, lexer keywords) and **`phg lift` CLI ships**
  (`cmd_lift`, full `src/lift/`). M4 stdlib **breadth is largely built** (sort/map/list/text/set/
  as-cast/parseFloat). So the remaining work is lighter than the milestone titles imply.

## Sequence (dependency order)
1. **M4 charter** — codify the *de-facto* conventions from the ~18 shipped native modules into a
   one-page conventions doc + minimal enforcement. Governs items 3–6. (No API rework: descriptive.)
2. **`phg fmt` + lints** — `fmt` reuses the existing `src/lift/printer.rs`; add unused-import /
   unused-local lints on the warning channel. Near-free; speeds all later authoring.
3. **M-Test** — `phg test` runner + `Core.Test` assertions + `assertFaults` + fixtures/selection/skip
   (+ PHPUnit bridge if cheap). Determinism seam (seedable Random + quarantine) already built. **FORK.**
4. **M-text** — `Core.Regex` (PCRE `/u`), codepoint-aware string ops, `\u{…}` escapes, `number_format`.
   **FORK** (regex API surface).
5. **Breadth gaps** — only what `m4-stdlib-breadth.plan.md` left open (most is ✅); `core.json`
   safe-parse hardening, path/log/sprintf if not present.
6. **Close M-NUM S4** — Math breadth + `number_format` (shared with M-text). Flips M-NUM to ✅.
7. **lift L5** — PHP→Phorge→PHP round-trip oracle gate. Flips lift to ✅.
8. **Release-readiness** — M8 security hardening (injection guards, `Secret<T>` **FORK**, `write_atomic`)
   → GA governance docs (semver/BC/conformance corpus/security model) → M2.5 Phase 3 (CI stub registry
   + `--sign`). Docs last: they describe a stable surface.

## Status
- [ ] 1. M4 charter — IN PROGRESS
- [ ] 2. phg fmt + lints
- [ ] 3. M-Test
- [ ] 4. M-text
- [ ] 5. breadth gaps
- [ ] 6. M-NUM S4
- [ ] 7. lift L5
- [ ] 8. release-readiness
