# Post-Wave-3 Four-Track Plan

> **Status:** active (started 2026-06-20). Code state at start: master `a6d64bf`, tree clean.
> The developer authorized all four tracks ("all 4 options as agreed"), executed as independent
> green slices with a commit + compaction point between each. Order is the developer's:
> Option 1 → Option 2 → namespace reshape → review pass.

## Decisions Log
- [2026-06-20] AGREED: do all four queued items in order (Opt1 named tags, Opt2 `phg check --json`,
  namespace reshape, review pass) — execute as independent green slices, commit each, compact between.
- [2026-06-20] AGREED: Option 1 = macro-monomorphized per-tag natives (uniform registry, real
  eval+php, byte-identity-testable). Solves the deferred "fn-ptr can't bake a tag" blocker without
  any lexer/parser/checker/backend change — purely additive, like Wave 2. Tag names are single
  lowercase words ⇒ already reshape-safe (no camelCase migration later).
- [2026-06-20] AGREED (Track 3 slice 1): manifest distributable key `name` → `module` (committed
  `ce588e3`); lockfile/`[require]` keys unchanged (dependency coordinates).
- [2026-06-20] AGREED (Track 3 slice 2): **casing is a HARD ERROR for all** — package/folder
  segments PascalCase (`E-PKG-CASE`), types/enums/variants PascalCase, functions/methods/vars/params
  camelCase (single lowercase word counts as camelCase). No `W-CASE` lint fallback. Settles spec §6.
- [2026-06-20] AGREED (Track 3 slice 2): execution is **subagent-driven** — implement the checker
  casing rules + `phg explain` entries + the stdlib public-API rename (`split_once`→`splitOnce`,
  `bool_attr`→`boolAttr`, `void_el`→`voidEl`, `from_string`→`fromString`, `to_string`→`toString`, …)
  in the parent; dispatch a subagent to run the wide mechanical codemod over all `.phg`/fixtures/
  inline test programs and bring the full gate green. Verify master HEAD advanced after the subagent
  commits (worktree git-env gotcha [[agent-worktree-vendor-git-corruption]]).
- [2026-06-20] AGREED (GA blockers): B1–B4 + P1-d done INLINE on master (8a0672e, b2b047c) — full
  gate green incl. PHP oracle. The literal tag gate is cleared.
- [2026-06-20] AGREED (P1-b transpile claim): **DO BOTH** — (a) down-scope the README/docs "transpiles
  the whole language" claim now + list the M11 gaps in KNOWN_ISSUES (honesty), AND (b) complete the
  transpiler arms for literal-pattern `match`, expression-position `match`, and `is` (with PHP-oracle
  byte-identity tests) so the full claim can be restored. Spine-sensitive — verify each arm on
  run≡runvm≡php.
- [2026-06-20] AGREED (P1-a float fidelity): **fix the formatting too** — correct the false
  KNOWN_ISSUES "exactly-representable floats are byte-identical" claim AND add a `__phorge_float`
  helper so transpiled PHP emits positional shortest-round-trip floats (no sci-notation divergence for
  more values). `run≡runvm` is unaffected (spine-safe); this is a transpile-leg fidelity fix.
  `src/transpile.rs:251`, `src/native.rs:963`. Add oracle cases at `1e-5`/`1e15`/`1e20`.
- [2026-06-20] DONE (P1-a) @ `8eae410`: `__phorge_float` helper shipped; finite floats byte-identical
  on run/runvm/PHP; `examples/guide/floats.phg`; KNOWN_ISSUES corrected. Pre-commit gate green.
- [2026-06-20] DONE (P1-b match arms) @ `8579323`: literal-pattern + expression-position `match`
  transpile (IIFE for expr position); fixed a pre-existing Assign-position match unconditional-throw
  bug (now if/elseif/else); `examples/guide/match-expr.phg` + enums-match.phg un-deferred; 394 lib +
  10 oracle green.
- [2026-06-20] AGREED (P1-b `is` operator): user chose **Option 1 — make `is` a real `instanceof`
  type-test** (parser: RHS as a TYPE; checker: validate + smart-cast narrow like S2 if-let;
  interpreter + VM: real type test; transpile: PHP `instanceof`; byte-identity-gated example).
  Discovery: `is` is currently value-equality aliased to `==` (interpreter `Is => l.eq_val(&r)`),
  broken as a type test (`x is T` → E-UNKNOWN-IDENT). **Claude's dissent (recorded, non-binding):**
  recommended RETIRING the alias instead — `is` is largely redundant with Phorge's sealed enums +
  exhaustiveness-checked `match`, and a boolean `is` invites un-checkable if/elseif ladders; PHP needs
  `instanceof` only because it lacks sealed-match. User to confirm Option 1 stands after reading the
  challenge (asked me to challenge). **Post-compact implementation plan for Option 1 (if confirmed):**
  (1) parser — parse `is` RHS as a Type (new grammar) not an Expr; (2) AST — represent type operand
  (e.g. `Expr::Is { value, ty }` or keep BinaryOp::Is with a Type rhs); (3) checker — validate the
  type exists, result `Bool`, smart-cast narrow the value in the `if`-true branch (reuse S2 narrowing);
  (4) interpreter + VM — real type membership test (enum-variant / class instanceof); (5) transpile —
  `$x instanceof T` (de-mangle namespaced type names); (6) example `examples/guide/is-operator.phg`
  byte-identity-gated; (7) update KNOWN_ISSUES (remove the "not a type test" note) + FEATURES.
  GOTCHA: the value-eq `Is` arms in interpreter:515, checker:963/1003, compiler:1192 must be replaced
  (this is a semantics change to a parsed operator — confirm no example relies on `a is b` value-eq).
- [2026-06-20] AGREED (pace): continue GA punch-list autonomously, gate per commit, checkpoint only on
  real forks/failures. After `is`: remaining Gate-2 (P1-c ext-policy CI scan, core.file size cap +
  no-sandbox doc, built-binary exit-status propagation, serve eager respond presence/arity check) →
  reshape slices 2b→3→4 → Gate 3/4/5.

## Track 1 — core.html Option 1 (named per-tag helpers)
**Approach:** two `macro_rules!` (`tag_el!`, `tag_void!`) in `src/native.rs`, each producing a
`NativeFn` whose `eval`+`php` bake the tag literal via `concat!`/`format!`. Append a curated common
HTML5 tag set to `html_natives()`.
- Files: `src/native.rs` (macros + entries + a unit test pinning one el + one void pair),
  `examples/guide/html.phg` (Option-1 demo section), `examples/README.md`, `FEATURES.md`,
  `CHANGELOG.md`, `docs/specs/2026-06-19-core-html-design.md` (named set → shipped),
  `tests/differential.rs` (agree + transpile-shape case).
- Acceptance: `cargo test` green; PHP oracle (`PHORGE_REQUIRE_PHP=1`) byte-identical; clippy+fmt clean.
- Risk: macro Rust-eval vs PHP-php drift → pinned by unit test + example oracle.

## Track 2 — core.html Option 2 (`phg check --json`)
**Approach:** structured diagnostics — serialize the existing `Diagnostic` surface to JSON (std-only,
hand-rolled) behind a new `--json` flag on the `check` command. LSP foothold.
- Files: `src/cli.rs` (flag + JSON path), a diagnostic serializer (likely `src/diagnostic.rs` or
  inline), `tests/cli.rs`, docs.
- Acceptance: `phg check --json good.phg` → `[]`; on error → JSON array of {code,message,severity,span}.
- Risk: JSON escaping correctness → unit-test against a message containing `"`/`\`/newline.

## Track 3 — Namespace reshape (spec `docs/specs/2026-06-20-package-namespace-reshape-design.md`)
Milestone-scale, breaking. Build order (each slice independently green):
1. Manifest `name` → `module`. 2. PascalCase enforce + codemod (`E-PKG-CASE`). 3. `package main` →
`package Main`. 4. Types in libraries (lift `E-PKG-TYPE` + cross-package type mangling).
- Scoped + planned in detail when Tracks 1–2 land (re-read the spec at that point).

## Track 4 — Review pass
Act on / re-run the 2026-06-19 review reports (sleuth/inspect/gaps/forge) against the post-reshape
tree, or run a fresh pass. Hardening, not features.

## Formal Plan
- **Track 1 — DONE** (`9ca5a47`, pre-commit OK): macro-monomorphized per-tag natives, byte-identical
  run/runvm/PHP, docs + memory updated.
- **Track 2 — DONE**: `phg check --json` — std-only diagnostics serializer on `Diagnostic`
  (`diagnostic.rs`), `cli::check_json_program`, `--json` wired in `main.rs` (stdout + exit 0/1),
  unit + 2 CLI tests, FEATURES/CHANGELOG/`--help` updated. Gate green (FMT/CLIPPY 0, tests pass).
- **Track 3 — in progress** (namespace reshape):
  - **Slice 1 — DONE**: manifest distributable `name` → `module` (`src/manifest.rs` struct/parser/
    `namespace_root`; `src/loader.rs` + `tests/project.rs` + `tests/vendor.rs` fixtures; both example
    `phorge.toml`; CHANGELOG + spec §5.1 + example README). Lockfile `name` (dep coordinate) and
    `[require]` keys unchanged. Rename-only, output-preserving; 471 tests green, PHP oracle ran,
    clippy + fmt clean.
  - Slice 2 — SPLIT for safety into 2a + 2b (smaller green commits; the package-segment rule forces
    folder renames, structurally riskier than identifier casing):
    - **2a (in progress)**: identifier + type casing as HARD errors — `E-NAME-CASE` (camelCase for
      functions/methods/params/vars/lambda-params) + `E-TYPE-CASE` (PascalCase for class/enum/
      type-alias/enum-variant names) + `phg explain` entries; rename the 5 snake stdlib natives
      (`split_once`→`splitOnce`, `bool_attr`→`boolAttr`, `void_el`→`voidEl`, `from_string`→
      `fromString`, `to_string`→`toString`); migrate all identifier violations across `.phg`,
      fixtures, inline test programs, docs. Package declarations stay lowercase here.
    - **2b**: `E-PKG-CASE` (PascalCase package/folder segments) — exempt reserved `core` root +
      `main` entry; rename example project folders + test fixtures to match folder=path.
  - Slice 3: entry `package main` → `package Main`.
  - Slice 4: types in libraries (lift `E-PKG-TYPE` + cross-package type mangling + namespaced PHP +
    D5b type-vs-leaf guard).
- **Track 4 — Review pass — DONE** (background Workflow `wjq47kit9`, 26 agents, 14 dims,
  adversarially verified). Report (NOT committed — public repo, live vuln detail):
  `~/.claude/projects/-stack-projects-phorge/reviews/2026-06-20-ga-readiness-review.md`.
  Verdict: **NOT GA-ready but close**; spine held in every test; all weakness in non-spine
  M5/M6 modules. Drives the GA punch-list below.

## GA Readiness — punch-list (target: taggable 1.0; user: "everything to GA-taggable")

Code state: master `012e8cc` (slice 1 `ce588e3`, slice 2a `5d60346`, README `012e8cc`).

### GA blockers B1-B4 + serve tests — DONE (done inline on master, 2026-06-20)

All four hard blockers + the serve test gaps closed inline on the correct base (no worktree, after
the stale-base abort lesson). Two commits, both green through the full pre-commit gate (PHP oracle
included):
- **B1+B2** (`fix(security): close phg vendor git arg-injection + path traversal`): `vendor.rs`
  `--`/`protocol.ext.allow=never` + reject leading-`-`/`ext::`/`file::` (file:// still allowed);
  `manifest.rs` `validate_path_component` for dep.name + source (reject `..`/absolute/bad-char),
  re-checked at vendor + loader join sites. New unit tests; `file://` integration still green.
- **B3+B4+P1-d** (`fix(security): make phg serve DoS-resilient …`): resilient accept loop +
  consecutive-error circuit breaker (B3); per-conn read/write timeout + `--timeout` (B4);
  `read_http_request` → `&mut impl Read` + 10 framing unit tests + un-ignored `tcp_smoke` (P1-d);
  P1-e 500-degradation tests. **Bonus root-cause fix:** the O(n²) whole-buffer terminator re-scan
  (CPU-DoS on a large no-terminator request) → scans only new bytes. SECURITY.md + `--help` updated.
- **LESSON (kept):** do not trust `isolation: worktree` to branch from current master — verify the
  base, or run inline / via a non-worktree subagent on master.

### Foreground (parent): finish the reshape
- **Slice 2b — NEXT**: `E-PKG-CASE` PascalCase package/folder segments — **exempt reserved `core`
  root + `main` entry** (entry rename is slice 3). Touches `src/checker.rs` (casing pass already there
  from 2a — add segment check), `src/loader.rs` (folder=path is case-sensitive), example project
  folders (`examples/project/*/src/acme/...` → `.../Acme/...`) + `package` decls, and the inline
  fixtures in `tests/project.rs`/`tests/loader.rs`/`tests/vendor.rs` (`package acme.util;` +
  `src/acme/util/` → `package Acme.Util;` + `src/Acme/Util/`). Imports of user packages → PascalCase;
  `core.*` imports + native module paths stay lowercase (reserved). Avoid editing the exact
  `loader.rs` dep.name-validation region the blocker subagent (B2) touches — cherry-pick first or
  resolve on merge.
- **Slice 3**: entry `package main` → `package Main` (mechanical once 2b lands; drop the `main` exemption).
- **Slice 4**: types in libraries — lift `E-PKG-TYPE`, cross-package type mangling, namespaced PHP for
  classes/enums, D5b type-vs-leaf guard. (The only real new *capability*; rest is rename.)

### Then: remaining GA P1/P2 punch-list (fan out)
- **P1-a** float honesty: KNOWN_ISSUES claim that exactly-representable floats render byte-identically
  is FALSE (PHP sci-notation). At minimum correct the claim; ideally fix `__phorge_str`/`println`
  float formatting (positional shortest-round-trip). Spine unaffected. `src/transpile.rs:251`,
  `src/native.rs:963`.
- **P1-b** transpiler rejects literal/expression `match` + `is` (M11 gap): complete the arms OR
  down-scope the README "transpile the whole language" claim. `src/transpile.rs:579,638,915`.
- **P1-c** ext-policy denylist scan (no PHP needed): drive every `NativeFn.php` + transpile every
  example, assert no `mb_|ctype_|iconv|...` token; gate in CI. `src/native.rs:533`.
- **P1-f** fuzz/no-panic harness for EV-7 (std-only LCG + grammar-shaped bytes through
  lex/parse/check/run/runvm, assert no panic). `tests/`.
- **P1-g** structure loader diagnostics (`Result<Unit, Vec<Diagnostic>>`, route `check --json` through
  loader) before the `--json` shape freezes at 1.0. `src/loader.rs`, `src/cli.rs`.
- **P2 cluster** (transpiler fidelity): `==`/`!=` → strict `===`/`!==` + `__phorge_eq`; `trim`/`upper`/
  `lower` ASCII-only parity; per-call-site scratch names; `println` via `__phorge_str`; a per-native
  PHP-mapping differential test; `core.file` no-sandbox doc + read size cap; built-binary exit status;
  serve eager `respond` validation; leaf-resolution parity hole (`index_of_by_leaf`).
- **P3**: vendor `&rev[..12]` char-boundary; `overflow-checks` profile; stale "M1"/comment cleanup;
  diagnostic code coverage + explain-coverage enforcement test.

### Benchmarking (user priority — perf story)
- Current `phg bench --vs-php` is vs a **debug** PHP build → not a credible "faster than PHP" claim.
  Build/point at an **optimized** PHP (opcache, NTS release) and run **multiple** workloads before
  any public perf assertion. README already hedged ("early", "sample workload").
