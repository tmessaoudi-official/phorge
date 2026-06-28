# Phorj Playground (WASM) — Implementation Plan

**Goal:** A zero-backend browser playground (run/runvm/transpiled-PHP-executed, 3-way agreement),
auto-deployed to GitHub Pages with the latest `phg` on every `master` push.

**Spec:** `docs/specs/2026-06-24-playground-wasm-design.md`

**Architecture:** Cargo workspace; core `phorj` crate untouched (zero-dep, forbid-unsafe). New
`playground/` cdylib member depends on the lib + wasm-bindgen (wasm-only target dep). Wrapper logic in
plain `*_json(&str) -> String` functions (native-testable) wrapped by thin `#[wasm_bindgen]` exports
gated to `wasm32`. The wrappers **bypass `on_deep_stack`** (uses `std::thread`, unavailable on wasm)
and call the public inner pipeline directly: `parse_program`/`parse_checked_program` → `interpret` /
`compile`+`Vm::run` / `transpile::emit`.

## Decisions Log
- [2026-06-24] AGREED: Build the WASM playground (highest adoption lever; user pivoted from GA spine).
- [2026-06-24] AGREED: Full 3-way from day one — php-wasm (seanmorris, PHP 8.4) executes the transpiled PHP live.
- [2026-06-24] AGREED: Cargo workspace + isolated `playground/` crate; core stays zero-dep.
- [2026-06-24] AGREED: v1 features = examples picker + shareable permalink + diagnostics/explain + backend tabs/diff.
- [2026-06-24] AGREED: CodeMirror 6, GitHub Pages, GitHub Actions deploy on master push; php-wasm via CDN.
- [2026-06-24] AGREED: Finish autonomously, then hand over deploy + live-test steps.

## Tasks
1. **Core seam** — add `pub fn parse_program(&str) -> Result<Program,String>` (exposes `lex_parse`) for
   unchecked-parse → `check_json_program` diagnostics. Additive, no dep, no unsafe. Verify `cargo test`.
2. **Workspace + crate skeleton** — root `[workspace] members=["playground"]`; `playground/Cargo.toml`
   (cdylib+rlib, wasm-only wasm-bindgen), `rust-toolchain.toml` `targets`. Verify core builds.
3. **Wrapper logic + native tests** — `check_json`/`run_json`/`runvm_json`/`transpile_json`/`explain`
   returning JSON via serde_json; `#[cfg(wasm32)]` `#[wasm_bindgen]` exports. Native unit tests assert
   shape + clean/error/fault paths + no panic. Gate: `cargo test`, clippy, fmt.
4. **Web frontend** — `web/index.html`, `web/style.css`, `web/main.js` (CodeMirror 6 via CDN, Web
   Worker running the wasm, debounced check, backend tabs, diagnostics panel, explain-on-click).
5. **php-wasm 3-way** — load php-wasm (PHP 8.4) lazily; execute transpiled PHP; agreement badge +
   diff-on-mismatch banner.
6. **Examples + permalink** — `web/gen_examples.py` scans `examples/guide/*.phg` (filters `Core.File`),
   emits `web/examples.js`; permalink via native `CompressionStream` → base64url URL hash.
7. **CI deploy** — `.github/workflows/playground.yml`: add wasm32, install wasm-pack, build, gen
   examples, assemble `dist/`, deploy-pages. Additive to `ci.yml`.
8. **Docs + evidence** — `playground/README.md`, README/MILESTONES note; capture screenshots of the
   running page where possible; hand over deploy + live-test steps.
