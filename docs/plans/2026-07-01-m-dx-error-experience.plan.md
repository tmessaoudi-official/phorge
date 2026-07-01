# M-DX — Error Experience & Build Profiles Plan

> Design spec (locked): `docs/specs/2026-07-01-error-experience-build-profiles-design.md` (`3a85c30`).
> Autonomous full-milestone sweep. Build straight from the spec + running decisions log (no per-slice
> plan files). Slices ship independently green: `cargo test --workspace` + clippy + `fmt --check` +
> PHP-8.5 oracle byte-identity + guide example, then commit.

## Decisions Log
- [2026-07-01] AGREED: build the **full milestone, all 6 slices** (S1→S0→S2→S3→S4→S5) in one
  autonomous sweep; commit each green; stop only on a genuine design fork or an unresolvable red gate.
- [2026-07-01] AGREED: **no per-slice plan files** — build straight from the locked design spec, keep a
  lightweight decisions log here. Be vigilant for surprises; improve existing code in passing or flag
  it if not solid enough.

## Slice tracker
- [x] **S1** Diagnostics quality — DONE. Soundness fixes B/C/D + D' (E-OVERRIDE-SIG return covariance,
      E-DUP-VARIANT, E-DUP-STATIC, E-DUP-CONST); 2 uncoded → coded (E-DUP-TYPE, E-TYPE-ARG-COUNT);
      **24** explain entries added (audit said 14 — the coverage ratchet found 10 more: all four
      E-TYPE-IMPORT-*, the E-DECL-* pair); diagnostic-coverage ratchet
      (`every_emitted_diagnostic_code_has_an_explanation`) + removed the drift-prone hardcoded fallback
      list; golden-diagnostic corpus (`conformance/diagnostics/` + `tests/diagnostics.rs`, bless-able).
      Full workspace green at PHP-8.5 floor, clippy+fmt clean. Corpus-per-all-codes = flagged future
      work (seeded with slice-touched codes only).
- [ ] **S0** Build profiles (Dev/Release), secure-by-construction, fold in `serve --dev`
- [ ] **S2** Secure value renderer (Secret redaction, caps, deterministic, stderr-only)
- [ ] **S3** Value-dump on fault (faulting-frame locals + backtrace headers; Dev + opt-in)
- [ ] **S4** Assertions (`assert`, always-checked, FaultKind::Assert, transpile to `if(!c)`)
- [ ] **S5** Interactive debugger (interpreter-only; REPL + DAP frontends)

## Surprises / improvements flagged
- [S1] The W1 audit's "14 missing explain" undercounted: the coverage ratchet (a source scan) found
  **24** — it caught `E-TYPE-IMPORT-{BUILTIN,CONFLICT,SHADOW,UNKNOWN}` and `E-DECL-{PACKAGE,NONFOREIGN}`
  that the manual audit missed. Lesson: a mechanical ratchet beats a hand audit for completeness.
- [S1] `E-DECL-*` codes live *inside* multi-line `format!` strings in the loader (plain `String`
  errors, not `Diagnostic`) — the ratchet scanner had to go whole-file (not per-line) to see them.
  Loader errors being a separate `String` channel (no `.with_code`, no caret) is an inconsistency worth
  a future slice (migrate loader to `Diagnostic`).
- [S1] The hardcoded "known codes" list in the `explain` fallback had already drifted (missing the 24).
  Removed it entirely; the ratchet is the SSOT guarantee now.
