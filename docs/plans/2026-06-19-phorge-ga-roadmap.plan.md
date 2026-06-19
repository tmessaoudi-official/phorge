# Road to Phorge 1.0 (GA) — Milestone Plan

> Consolidates the 2026-06-19 global review (`/sleuth + /inspect + /gaps + /forge + /inspect --vision`,
> ~40 agents, 5 lenses) into a sequenced path to a stable GA release. Source of every finding:
> `~/.claude/projects/-stack-projects-phorge/REVIEW-2026-06-19.md` (master code state: master `687a7bd`,
> docs `0b83e97`, 452 tests green, clippy+fmt clean). Every finding ID (P0-*, S-xx, I-xx, G-xx, forge-xx,
> V*) is mapped to exactly one milestone below or to **Deferred past 1.0** with a reason — nothing dropped.

## Decisions Log

- [2026-06-19] AGREED: build a sequenced **M7 → M12** road to 1.0 GA incorporating all review findings.
- [2026-06-19] AGREED: keep the **3-backend model** (interpreter / VM / transpiler→PHP) + add an **Op descriptor table**; **defer the shared-IR rewrite** (`src/ir.rs`) unless feature velocity demands it.
- [2026-06-19] AGREED: **M7 (correctness closure) is non-negotiably first** — the PHP oracle is the keystone of the correctness story and every later transpiler feature needs it to not regress.
- [2026-06-19] AGREED: **M10 generics (`Ty::Var` + erasure-first) is the keystone unblock** — one type-system primitive gates `core.list`, `core.json`, `Map`/`Set`/tuples, router path-params, and function-type variance.
- [2026-06-19] AGREED: the report-only review findings now become the **GA backlog**; this file is the working tracker, executed milestone-by-milestone.
- [2026-06-19] AGREED: **spec M7 first, then build** (developer chose the spec-driven path for the first milestone) — write a detailed M7 implementation spec/plan (PHP-oracle design, CI-`php` availability strategy, per-P0-fix test list, divergence-class regression matrix) for review BEFORE writing code. Next action after compact = author the M7 spec.
- [2026-06-19] PROPOSED (spec, awaiting approval): fix P0-1/P0-3/P0-4 via **runtime PHP helpers** (`__phorge_div`/`__phorge_rem`/`__phorge_str`) that inspect operand types at PHP-runtime, mirroring Phorge's type-driven value kernels — chosen over a transpiler-side static type resolver because it eliminates a duplicated operand-type resolver (the M9 single-sourcing concern) *and* the inference-completeness regression risk, and ties each helper 1:1 to a `value.rs` kernel / `as_display`. P0-2 stays a syntactic precedence-parens fix. Spec: `docs/specs/2026-06-19-m7-correctness-closure-design.md`.

---

## Phorge 1.0 GA — Definition & Exit Criteria

1.0 / GA means Phorge is a **trustworthy, stable, fully-documented** language: all three backends are
provably byte-identical and enforced in CI, no open silent-correctness or security issues, the type
system supports the committed 1.0 stdlib, and the language surface carries a semver stability promise.

- [ ] **3-backend byte-identity enforced in CI** — `run ≡ runvm ≡ php` over the full `examples/**/*.phg` glob, `PHORGE_REQUIRE_PHP=1` (fails, not skips). *(M7, M9)*
- [ ] **Zero open P0/P1 correctness findings** — all P0-1…4 + P0-ROOT closed, every divergence class has a regression test. *(M7)*
- [ ] **Zero open P0/P1 security findings** — vendor arg-injection, path-traversal, lockfile tamper, serve DoS all closed. *(M8)*
- [ ] **CI green on every push** — GitHub Actions mirrors the local gate (fmt + clippy + test) + a PHP job + a zig cross-build job. *(M9)*
- [ ] **Type system supports the 1.0 stdlib scope** — `Ty::Var` + erasure-first generics ship; `core.list`/`core.json`/`Map`/`Set` are buildable. *(M10, M11)*
- [ ] **Docs accurate + a language reference exists** — MILESTONES.md is the human SSOT, all status/CLI/test-count drift fixed, plus a complete language-reference doc. *(M9, M12)*
- [ ] **Semver / stability commitment** — a documented stability policy for the language surface + a frozen 1.0 grammar. *(M12)*
- [ ] **Security surfaces hardened** — atomic state writes everywhere, symlink-escape closed, git-env isolated, bytecode `validate` exhaustive. *(M8, M9)*
- [ ] **No silent-skip tests** — no test self-skips to PASS on a missing toolchain; the real-socket serve path is gated. *(M7, M9)*
- [ ] **Release automation** — reproducible builds + SHA-256 checksums per artifact, version bumped to 1.0. *(M12)*

---

## M7 — Correctness Closure  *(keystone; first; non-negotiable)*

**Goal:** Make all three backends provably byte-identical in CI by closing the PHP loop and fixing every silent transpiler→PHP divergence.

**Exit criteria**
- [x] `php`-gated 3-way oracle runs over the example glob **+ projects**; `PHORGE_REQUIRE_PHP=1` **fails** when `php` absent (verified both directions); unset skips loudly. Self-skip `cli.rs` PHP tests removed (5, not 2 — undercounted in the spec).
- [x] P0-1…4 fixed: `__phorge_div`/intdiv, precedence-wrap parens, `__phorge_str` bool-coercion, `__phorge_rem`/fmod (runtime helpers, not static inference).
- [x] Divergence classes with dedicated tests: int-div trunc (`m7_int_division_truncates_toward_zero`), `i64::MIN / -1` (`m7_int_min_div_neg_one_faults_identically`), large-range (`m7_large_range_faults_identically`), OOB index (existing), bytes boundaries (oracle over `bytes.phg`), empty/reversed range (oracle + `s1_ranges`). Float-formatting / neg-zero remain governed by the existing exactly-representable-floats KNOWN_ISSUE (oracle-covered via examples; no separate test — low-value edge).
- [x] Large-range fault folded in (clean `run≡runvm` fault `"range too large"`, not exit-101) with `agree_err` + `FaultKind::RangeTooLarge`; `value::build_range` single-sources the size-guarded materialization.
- [x] Zero known *silent* divergences across all three backends — every transpilable example is php-executed; remaining transpiler gaps (literal/expr `match`, `is`) are *loudly deferred* to M11, not silent.

**Status (M7):** ✅ **COMPLETE** — implemented this session on top of `8c6fbb2`; ~453 tests green (differential 43 incl. 2 oracle + 4 M7 regression tests), clippy + fmt clean, oracle green over all examples/projects under `PHORGE_REQUIRE_PHP=1`. Spec: `docs/specs/2026-06-19-m7-correctness-closure-design.md`. Carry-forward: P1-#12/#13 (PHP-builtin-collision / private-field) → M8; CI that *sets* `PHORGE_REQUIRE_PHP=1` → M9; literal/expr `match` + `is` transpile → M11.

**Findings included**

| ID | Finding | file:line | Effort |
|----|---------|-----------|--------|
| P0-1 / S-01 / forge-G3 / I-04 | Integer `/` → PHP float `/` (`7/2`→`3.5`), LIVE in `operators.phg` | `src/transpile.rs:855` | Quick |
| P0-2 / S-01 | Dropped operand grouping parens (`a-(b-c)`, `-(a+b)`, `!(a&&b)`) | `src/transpile.rs:517-536` | Quick |
| P0-3 / S-01 | `bool` interpolation `true`/`false` vs PHP `"1"`/`""`, LIVE | `src/value.rs:110` + `src/transpile.rs` | Quick |
| P0-4 / S-01 | Float `%` → PHP integer `%` (`5.5%2.0`→1.5 vs 1) | `src/transpile.rs:856` | Quick |
| P0-ROOT / P1-#5 / I-03 / forge-H1/C5 / G-05/G-06/G-09/G-10/G-11/G-16/G-31/G-32 | No PHP execution oracle; `cli.rs` PHP tests self-skip-to-PASS | `tests/differential.rs`, `tests/cli.rs:107-135` | Med |
| QW-13 / G-16 / S-01 | Empty/reversed range → wrong descending PHP `range()` | `src/transpile.rs` | Quick |
| P1-#9 / S-09 / I-05 | Large-range materialization panics/OOM (exit 101); breaks EV-7 | `src/vm.rs:252-263`, `src/interpreter.rs:384-389` | Quick |

**Sequencing notes** — This is the foundation: every future transpiler feature (M11's `is`/expr-`match` arms, M10's
erasure emit) must be guarded by the oracle so it cannot silently regress. Land the four discrete emitter fixes
**with** the oracle in the same wave so they're enforced on first commit. The large-range fault is grouped here (not
M8) because it is a `run≡runvm` value-correctness divergence and needs the same `agree_err` parity machinery.

---

## M8 — Trust & Hardening  *(parallelizable with M9)*

**Goal:** Close every supply-chain, server, and state-corruption attack surface so an untrusted manifest, dependency, or request can't cause RCE, traversal, tamper, or DoS.

**Exit criteria**
- [ ] `phg vendor` git invocations are `--`-separated, scheme-allowlisted, and reject `-`-leading refs.
- [ ] Dependency `name` validated against `^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$` at parse + post-join `starts_with(vendor_root)` assertion.
- [ ] Lockfile content-hash **verified on load** (`E-VENDOR-TAMPERED`); re-vendor validates against existing `phorge.lock`.
- [ ] A `write_atomic` (temp-then-rename) helper applied uniformly to `phorge.lock`, `phg build` output, vendor swap, and the cross-stub cache.
- [ ] `phg serve` survives a handler panic (`catch_unwind` → 500), has a read timeout, and handles malformed/absent `Content-Length` as 400.
- [ ] Systemic transpile-safety lint ships: `W-PHP-BUILTIN-SHADOW` + `W-PHP-FIELD-VISIBILITY` on the existing warning channel.
- [ ] Symlink-escape closed across manifest `source` / vendor copy / project walk; git-env fully isolated.

**Findings included**

| ID | Finding | file:line | Effort |
|----|---------|-----------|--------|
| QW-1 / P1-#6 / S-04 / I-01 / VG-§3 | `git clone`/`checkout` lack `--` → option/code-exec injection | `src/vendor.rs:51-52` | Quick |
| QW-2 / P1-#7 / I-02 | Path traversal via dependency `name` → rm/write outside project | `src/vendor.rs:65`, `src/loader.rs:110-123` | Quick |
| P1-#8 / S-05 / I-20 / VG-§2/VJ-§4 | Panicking handler aborts whole server (no `catch_unwind`; worker re-panics) | `src/serve.rs:34-66`, `src/cli.rs:351` | Quick |
| P1-#12 / I-04 / G-10 / forge-G3 | `package main` free-fn names collide with PHP builtins (`Cannot redeclare`) | `src/checker.rs`, KNOWN_ISSUES:108-113 | Med |
| P1-#13 / I-04 / G-11 / forge-G3 | Externally-read promoted fields must be `public` (PHP visibility) | `src/checker.rs`, KNOWN_ISSUES:114-120 | Med |
| P2-#26 / S-10 / VJ-3 | Cross-stub cache served by path-existence + non-atomic `fs::copy` | `src/bundle/cross.rs:176-179,217` | Quick |
| P2-#27 / S-11 / VJ-3 | Vendor swap removes live dep before renaming (crash window) | `src/vendor.rs:63-103` | Quick |
| P2-#28 / S-12 | Malformed/absent `Content-Length` silently framed as 0 → truncated body | `src/serve.rs:163-173` | Quick |
| P2-#29 / I-14 / S-05 | `phg serve` slowloris: no read timeout | `src/serve.rs:133-160` | Quick |
| P2-#36 / I-11 / I-12 | `manifest source`/vendor copy/project walk follow symlinks → escape root | `src/manifest.rs:116-120,193`, `src/vendor.rs:152-185`, `src/loader.rs:642-659` | Quick |
| P2-#39 / S-19 / VG-3 | Re-vendor never validates against existing `phorge.lock` | `src/vendor.rs:34-112` | Quick |
| P2-#40 / VG-3 | Lockfile content hash write-only; loader never verifies (`E-VENDOR-TAMPERED`) | `src/vendor.rs`, `src/loader.rs` | Quick |
| P2-#41 / S-20 | Vendor git-env isolation incomplete (`GIT_CONFIG_*`, `GIT_CEILING_DIRECTORIES`, …) | `src/vendor.rs:121-147` | Quick |
| Theme / S-10/S-11 / VJ-§3 | Non-atomic state writes → `write_atomic` helper (lock/build/vendor/stub-cache) | (cross-cutting) | Quick-Med |
| Theme / I-04 / VA-§3 | `php_compat` lint channel (`W-PHP-BUILTIN-SHADOW` + `W-PHP-FIELD-VISIBILITY`) | `src/checker.rs` warning channel | Med |

**Sequencing notes** — Independent of M7's value-correctness work; can run in parallel with M9. The `write_atomic`
helper is a single shared utility that satisfies P2-#26/#27/#40 and the lock/build paths at once — build it first,
then thread it. The `php_compat` lint (P1-#12/#13) rides the warning channel that shipped in M3 S2, so it's purely
additive. Note P1-#12/#13 are a *transpile divergence* class — they could be argued into M7, but they're placed here
because the fix is a front-end **lint** (a trust/hardening guard), not an emitter change, and they don't produce
wrong numbers, they produce PHP fatals.

---

## M9 — Engineering Hygiene & Evolvability  *(parallelizable with M8)*

**Goal:** Make the project safe to evolve and honest to read — CI enforcement, the descriptor table that dissolves the 3-coupled-match, single-sourced parity surfaces, and a full doc-SSOT sync.

**Exit criteria**
- [x] GitHub Actions CI exists: fmt + clippy + test, a PHP job (`PHORGE_REQUIRE_PHP=1`, fails not skips), a zig cross-build job; pin read from `rust-toolchain.toml`. — `.github/workflows/ci.yml` (`gate` + `cross-build` jobs); the encoded gate verified green locally (453 tests, oracle enforced, fmt+clippy clean) before commit. *(Observed green on a real GitHub run pending first push.)*
- [x] ADRs written for the 5 load-bearing decisions (no shared run↔VM IR; erasure-not-monomorphization; Rc-not-GC; single-file brace-namespace PHP; offline-only vendor). — `docs/adr/0001…0005` (Nygard format, Status: Accepted, immutable + spec back-links) + `docs/adr/README.md` index; `ARCHITECTURE.md` "Decision records" policy updated to make `docs/adr/` canonical for the decision (specs stay canonical for the exploration).
- [ ] Op descriptor table (`Op::meta()` / `OpInfo`) dissolves the 3-coupled-match; `chunk::validate` is **exhaustive** (no `_ => None` fall-through).
- [ ] Single-sourced: runtime fault strings (`faults`/`FaultMsg`), lambda capture-filter (`is_capturable`), native call-head resolution (`resolve_call_head`).
- [ ] Interpreter runtime faults carry a source line (`Cell<Span>` through eval); fault body unchanged so the oracle stays green.
- [ ] Doc-SSOT sync complete: MILESTONES.md = human SSOT; M5/M6 status, CLI table (`serve`/`vendor`), pipe `|>`, INVARIANTS "coming", `phorge`→`phg` slips, test-count 332→452 all fixed.
- [ ] `phg explain` known-codes list derived from `explain_text` (no omissions).

**Findings included**

| ID | Finding | file:line | Effort |
|----|---------|-----------|--------|
| Theme / I-07/I-29 / VB2/VJ-§1 / forge-H1 / G-19 / P1-#20 | ✅ **DONE** — CI added (`.github/workflows/ci.yml`); `PHORGE_REQUIRE_PHP=1` enforces the oracle (fails, not skips); CONTRIBUTING claim now true | `.github/workflows/ci.yml`, `CONTRIBUTING.md` | Quick |
| QW-15 / P1-#16 / forge-A2/C2/C6/D3 / I-36 | `chunk::validate` `_ => None` lets a new index/count Op skip its EV-7 bounds check | `src/chunk.rs:303-332` | Quick |
| Theme / I-36/I-20 / VC-§7/VE-C2 / forge-A1/A2/D7/G1/B1/D3/C6 / S-24 | 3-backend evolution tax → `Op::meta()`/`OpInfo` descriptor table + exhaustive `validate` | `src/{vm,compiler,chunk}.rs` | Low-Med |
| Theme / I-10 / forge-A1/B1 / P1-#22 | Fault strings hand-written twice + capture-filter duplicated 3-way | `src/interpreter.rs` ↔ `src/vm.rs`; `interpreter.rs:415-417` ↔ `transpile.rs:621-623` | Med |
| P2-#24 / forge-B5 | Native call-head resolution policy hand-duplicated compiler ↔ interpreter | `src/compiler.rs:~1299`, `src/interpreter.rs:533` | Quick |
| P1-#18 / I-08 / forge-H5 / G-14 / VC-§2 | Interpreter runtime faults carry no source line; VM faults do | `src/interpreter.rs` (Diagnostic::runtime → line 0) | Med |
| Theme / I-09 / S-08/S-17/S-29 / G-33/G-34/G-35/G-36/G-37 / P1-#21 | Systemic doc SSOT divergence (M5/M6 status, CLI table, ARCHITECTURE modules) | `README.md`, `ROADMAP.md`, `docs/MILESTONES.md`, `docs/ARCHITECTURE.md` | Med |
| QW-7 / S-08 / I-09 / G-33/G-34 | `|>` pipe listed "not yet supported"/"planned" but shipped | `examples/README.md:92-93`, `ROADMAP.md:55`, `VISION.md:46` | Quick |
| QW-14 / S-29 / I-26 | INVARIANTS.md calls shipped P4 ops "coming" | `docs/INVARIANTS.md:53` | Quick |
| QW-5 / I-06 / P1-#19 | Stale committed `phorge.lock` header (`phorge vendor` → `phg vendor`) | `examples/project/withdeps/phorge.lock:1` | Quick |
| QW-6 / S-18 / I-21 | CLAUDE.md test baseline stale (332 → 452) | `CLAUDE.md:31` | Quick |
| QW-9 / I-44 / I-23 | Manifest comments / rustdoc say `phorge serve`/`phorge <cmd>`; binary is `phg` | `examples/web/{handler,router}.phg`, `tests/{examples,cli}.rs` | Quick |
| QW-8 / S-07 / P1-#23 | `phg explain` known-codes omits `E-SHADOW-IMPORT` | `src/cli.rs:293` | Quick |
| P1-#17 / S-02 | `manifest.rs` silently accepts duplicate keys (last-wins) vs "strict TOML" | `src/manifest.rs:106-127,281-306` | Quick |
| P1-#20 / I-07 | CONTRIBUTING claims server-side CI exists (fixed by the CI job above) | `CONTRIBUTING.md:31` | Quick |
| P2-#25 / S-25 / forge-C2 | `index_of_by_leaf` first-match, no uniqueness check → future leaf collision | `src/native.rs:638-642,609-613` | Quick |
| P2-#30 / S-13 | `pascal` emits invalid PHP namespace for digit-leading dir names | `src/manifest.rs:246-252`, `src/loader.rs:246-252` | Quick |
| P2-#31 / S-14 | Comment-stripping no quote-balance handling; round-trip break on `"`-bearing value | `src/manifest.rs:211-243`, `src/lock.rs:138-158` | Med |
| P2-#33 / S-23 | `eq_val` false for two equal `Map`/`Set` (latent until constructible) | `src/value.rs:122-153` | Quick |
| P2-#34 / S-16 | folder=path validation mixes canonicalize/raw semantics | `src/loader.rs:570-622` | Med |
| P2-#35 / S-15 | Lock `PartialEntry::finish` reports wrong line for missing field | `src/lock.rs:68-73,102-104,119-129` | Quick |
| P2-#37 / forge-C2/C6 / S-24 | `CallNative` argc / `Call` arity operands unchecked at `validate` (EV-7 hole) | `src/chunk.rs:319-327` | Quick |
| P2-#38 / S-24 | `CallValue`/`MakeClosure` scratch-slot arithmetic unchecked | `src/vm.rs:423`, `src/compiler.rs:1154,1364,1387,1667` | Quick |
| P2-#43 / G-19 / I-49 | Live real-socket serve smoke test `#[ignore]`d | `tests/serve.rs:172` | Quick |
| P2-#45 / I-38/I-39/I-40 | Interpreter deep-clones whole `FunctionDecl`/`ClassDecl` per call; Rc-share decl tables | `src/interpreter.rs:556,564,533` | Med |
| Vision / ADR set | ✅ **DONE** — 5 ADRs (Nygard, Accepted, spec-linked) + README index; ARCHITECTURE.md policy updated | `docs/adr/0001…0005` + `README.md` | Med |

**Sequencing notes** — Independent of M8's security work. **Write the ADRs before the descriptor-table refactor
touches the decisions they record.** The descriptor table + exhaustive `validate` (QW-15 / P2-#37/#38) close the EV-7
holes structurally, so they supersede point-fixing each unchecked operand. The doc-sync pass (P1-#21 + all the QW
doc items) is one milestone-close sweep — bundle the test-count fix into it so it can't recur. P2-#45 (Rc-share decl
tables) is a perf-hygiene win, optional for GA correctness but cheap; keep it here.

---

## M10 — Keystone: Type System & Generics  *(gates M11)*

**Goal:** Add `Ty::Var` + **erasure-first** generics (no monomorphization) — the single primitive that unblocks the entire 1.0 stdlib and router scope while leaving the byte-identity spine untouched.

**Design approach (locked direction)**
- **Reserve generic constructors now** (`Ty::Param`/`Ty::Any` / `Ty::Var`), build the machinery here.
- **Erasure at transpile** — no monomorphization; a type var is erased to PHP's untyped surface, exactly as TypeScript erases to JavaScript. This is what keeps `run ≡ runvm ≡ php` intact.
- **Per-backend treatment of a type var:** interpreter — runtime values are already dynamic, a var is a no-op at eval; VM — operand `CTy` falls back to the generic/`Other` path (no new `Op`); transpiler — emits no PHP type annotation for the erased parameter. The checker does all the work; the three backends see an already-erased AST (same pattern as M3 S0.3 type-alias expansion and M5 name-mangling — resolve in the front end, hand the backends a lowered tree).

**Exit criteria**
- [ ] `Ty::Var` / `Ty::Any` exist; generic functions and generic type constructors type-check.
- [ ] Erasure pass runs before the backends; `run ≡ runvm ≡ php` holds on a generic example (oracle-gated).
- [ ] The two type/loader-correctness prerequisites are fixed: arm-unification null-typing (S-06) and name-mangle non-injectivity (S-03).
- [ ] A generic guide example ships under `examples/` (developer rule), byte-identity-gated.

**Findings included**

| ID | Finding | file:line | Effort |
|----|---------|-----------|--------|
| Theme / I-16/I-17 / VA-P2 (Top-10 #2) / G-07 (+G-08/G-21/G-22) / forge-G2/A/C4 | `Ty` has no type variable → blocks generics/`core.list`/`core.json`/Map-Set/variance | `src/checker.rs`, `src/ty.rs` (type defs) | Large |
| P1-#10 / S-06 | Checker arm-unification types a possibly-`null` value as non-optional `T` (violates S2 non-null) | `src/checker.rs:918` (also `:857`, `:1381`) | Med |
| P1-#11 / S-03 | Package name-mangle non-injective (`util`/`Util`→`Util`); `E-DUP-DEF` keys on dotted name | `src/loader.rs:233-252`, `src/manifest.rs:246-252` | Med |

**Sequencing notes** — This is **the** keystone. It comes after M7 (so the generic example is oracle-gated from day
one) and is independent of M8/M9, but **must precede M11** — every deferred stdlib/router item roots to it. S-06 and
S-03 are folded in here as type/loader-correctness prerequisites: S-06 is a checker null-typing soundness bug
directly adjacent to the type-system work, and S-03 is a name-injectivity bug whose fix shares the loader-mangling
surface that erasure-emit will touch. Build them as the warm-up before the `Ty::Var` core.

---

## M11 — Stdlib & Language Completion  *(needs M10)*

**Goal:** Use the new type system to ship the 1.0 stdlib (`core.list`, `core.json`, `Map`/`Set`), close the transpiler feature gaps, and land the remaining M3 slices and the M6 router middleware layer.

**Exit criteria**
- [ ] `core.list` (map/filter/reduce + reverse/sum) ships — uses M10 generics + the S3 lambda investment.
- [ ] `core.json` ships with the dynamic `Any`/`Json` type.
- [ ] `Map`/`Set` are constructible (the latent `eq_val` arm from M9 now exercised).
- [ ] Transpiler accepts `is` and expression-position `match` (currently rejected though all backends run them) — oracle-gated.
- [ ] M6 router middleware/closure-route layer ships (zero new `Op`, pure leverage on S3) + typed `Header`; library-package function values.
- [ ] Remaining M3 slices land or are explicitly re-scoped: **S4 = exceptions (try/catch/throw)**, **S5 = traits/mixins** (multiple-inheritance shape, MI rejected per D-L3), **S6/S7 = mutation + the tracing GC** (mutation is the trigger for the mark-sweep collector per ROADMAP §M3).
- [ ] Transitive git deps + `phg build` vendor-merge addressed or re-deferred with reason.

**Findings included**

| ID | Finding | file:line | Effort |
|----|---------|-----------|--------|
| Theme / I-16/I-17 (cont.) / G-07/G-08/G-21/G-22 | `core.list` (map/filter/reduce), `core.json` (dynamic `Any`/`Json`), `Map`/`Set`/tuples | `src/native.rs` (new modules) | Large |
| QW-12 / P1-#14 / G-05 | Transpiler rejects `is` operator that all backends run | `src/transpile.rs:527` | Quick |
| P1-#15 / G-06 | Transpiler rejects expression-position `match` the VM runs | `src/transpile.rs:569,1177` | Med |
| Vision / VA-P4 / J-A2 | M6 router middleware/closure-route layer (zero new Op) | `src/serve.rs`, router | Med |
| Vision / M6 W1 deferral | Typed `Header` (deferred from M6 W1) | router/handler | Med |
| Vision / M3 S3 deferral | Library-package function *values*; block-body return inference; function-type variance | `src/{checker,loader}.rs` | Med |
| P2-#42 / G-18 | `phg build` bypasses the project loader → can't build multi-package/vendored projects | `src/main.rs:111,155` | Med |
| Deferred-list | Transitive git deps (a dep's own `[require]`) + `phg build` vendor-merge | `src/{loader,vendor}.rs` | Med |
| ROADMAP M3 | M3 S4 exceptions, S5 traits/mixins, S6/S7 mutation + tracing GC | (language slices) | Large |

**Sequencing notes** — Gated entirely on M10. The transpiler `is`/expr-`match` arms (P1-#14/#15) are technically
oracle-only fixes (they could ride M7), but they're parked here because they're feature-completion of the transpile
contract rather than silent-divergence repair, and they want the oracle already in place. **Mutation (S6/S7) is the
single feature that introduces heap cycles and therefore the tracing GC** — sequence it last in M11 and treat the GC
as its inseparable companion (ROADMAP §M3: "do not ship mutation without the collector"). If S4–S7 prove too large
for the 1.0 bar, re-scope: exceptions + traits are 1.0 candidates; mutation+GC may slip to v1.1 — decide at M11 entry.

---

## M12 — DX & 1.0 Polish → GA

**Goal:** Ship the developer-experience multipliers, freeze the language surface, automate releases, and clear the cosmetic backlog — then declare GA.

**Exit criteria**
- [ ] LSP server ships on a `--json` diagnostics seam.
- [ ] TextMate / tree-sitter grammar published.
- [ ] REPL ships.
- [ ] Complete language-reference doc published.
- [ ] Fuzzing / property-test harness for lexer + parser in CI.
- [ ] Release automation + SHA-256 checksums per artifact (the unblocked half of M2.5 Phase 3).
- [ ] All cosmetic P3 findings cleared.
- [ ] Version bumped to **1.0**; semver stability commitment documented; **all GA exit criteria above checked**.

**Findings included**

| ID | Finding | file:line | Effort |
|----|---------|-----------|--------|
| Vision / VE-B1 | LSP / editor support on the `--json` diagnostics seam | (new) `--json` seam + LSP | Large |
| Vision / VE-A2.1 | TextMate / tree-sitter grammar | (new) | Med |
| Vision / DX | REPL | (new) | Med |
| Vision / docs | Language-reference doc | `docs/` (new) | Med |
| P2-#44 / I-27/I-28/I-29 | No lexer/parser fuzzing/property testing; native `php` mappings never executed | `tests/` (new fuzz harness) | Med |
| Deferred-half / VJ-§2a | M2.5 Phase 3 unblocked half: SHA-256 checksums + release automation | release pipeline | Med |
| P3-#46 / S-22 | `parse_shorthand` truncates git URL when tag contains `@` | `src/manifest.rs:314-325` | Quick |
| P3-#47 / S-27 | `Project::detect` innermost-`phorge.toml`-wins; no nested-project warning | `src/manifest.rs:180-205` | Quick |
| P3-#48 / S-28 | Lexer column counts bytes not chars → off-by-one column on multi-byte input | `src/lexer.rs:37-47` | Quick |
| P3-#49 / S-26/S-21 | `hash_tree` lossy `to_string_lossy`; `unique_temp_dir` collides on sanitizing names | `src/vendor.rs:190-203,244-250` | Quick |
| P3-#50 / forge-B2 | Compiler `Result<_, String>` for checker-proven-impossible conditions leaks AST via `{e:?}` | `src/compiler.rs:792-860` | Med |

**Sequencing notes** — Pure polish; everything here is independent and can interleave. The `--json` diagnostics seam
(LSP) and the fuzz harness both want CI (M9) in place. The final version bump + stability commitment is the literal
last step — it's the act of declaring GA, gated on every checkbox in the GA definition section.

---

## Deferred past 1.0

| ID / Item | Reason |
|-----------|--------|
| Shared-IR rewrite (`src/ir.rs`, Top-10 #6, forge-A1 vs VA-P1) | The descriptor table (M9) + the PHP oracle (M7) capture most of the safety at a fraction of the cost; the IR is the structural end-state but a Large rewrite — defer unless feature velocity demands it (locked decision). |
| Phase B slot-layout (slot-indexed `Vec` field access) | Bench-gated and unopened; no locality evidence after P5a put the object path within ~15% of scalar. |
| M2.5 Phase 3 signing (`--sign`, Authenticode/codesign/notarize) | Cert/SDK-blocked. The unblocked half (SHA-256 checksums) ships in M12. |
| URL / network (HTTP client) | Breaks both zero-dep and the byte-identical determinism spine; deferred to M6+ / post-1.0. |
| macOS build stub | Mac stub SDK-blocked (M2.5 Phase 3); the macOS *reader* already ships + is fixture-tested. |
| (Tracing GC itself) | Coupled to unbuilt mutation — sequenced *with* mutation inside M11 S6/S7, not standalone; if mutation slips to v1.1 the GC slips with it. |

> **Note:** "Transitive git deps / `phg build` vendor-merge" appear in KNOWN_ISSUES as deferred, but are pulled
> *forward* into **M11** (addressed-or-re-deferred-with-reason) rather than left in this table, so the GA decision is
> explicit about them. The tracing GC is listed here only to record that it is not a standalone milestone — its real
> home is M11 alongside mutation.

---

## Sequencing & Dependencies

```
M7  Correctness Closure ──────────────┐ (FIRST — gates every later transpiler feature)
                                       │
        ┌──────────────────────────────┤
        ▼                              ▼
M8  Trust & Hardening      M9  Engineering Hygiene   (parallel; both independent of each other)
        └──────────────┬───────────────┘
                       ▼
M10 Type System & Generics  (KEYSTONE — gates M11)
                       ▼
M11 Stdlib & Language Completion
                       ▼
M12 DX & 1.0 Polish → GA
```

**Why this order**

1. **M7 is first and non-negotiable.** The review's single strongest finding (corroborated by all five lenses) is
   that the transpiler→PHP backend lives outside the correctness loop, shipping silent wrong output *inside examples
   that claim byte-identity*. Until the PHP oracle exists, **every** future transpiler feature (M10 erasure emit,
   M11 `is`/expr-`match`/router) would land unverified and could regress silently. Correctness is the foundation the
   whole GA story rests on, so it ships before anything builds on top of it.
2. **M8 + M9 are parallelizable hardening.** Security (M8) and evolvability/hygiene (M9) touch disjoint surfaces
   (vendor/serve vs CI/descriptor-table/docs) and neither blocks the other. They sit between M7 and M10 because the
   descriptor table + exhaustive `validate` (M9) and the `write_atomic`/lint guards (M8) make the codebase safe to
   *extend*, which is exactly what M10's type-system work will do.
3. **M10 is the keystone that gates M11.** One missing primitive (`Ty::Var`) blocks the entire 1.0 stdlib and router
   scope. It cannot start before M7 (the generic example must be oracle-gated) and must finish before M11 (which
   consumes generics for `core.list`/`core.json`/`Map`/`Set`/router params).
4. **M11 completes the language** on top of generics, then **M12 is polish** — LSP, grammar, REPL, docs, fuzzing,
   release automation, and the final version bump that *is* the act of declaring GA.

**Interleaving with in-flight milestones (explicit)**

- **M3 language slices** run parallel to the M-series. S0–S3 are done; **S4 (exceptions), S5 (traits/mixins), and
  S6/S7 (mutation + tracing GC) are folded into M11** (Findings table + sequencing notes). Mutation+GC is sequenced
  *last* in M11 because mutation is the only feature that creates heap cycles, and the GC is its inseparable companion.
- **M6 web** shipped W0–W4 (`phg serve` is live). Its open items split: the **serve hardening** (catch_unwind, read
  timeout, Content-Length) lands in **M8**; the **router middleware/closure-route layer + typed `Header`** land in
  **M11** (they need the S3 lambda investment and benefit from M10's path-params). The doc drift that still marks M6
  as `🔲` is fixed in **M9**'s doc-SSOT sweep.
- **M2.5 Phase 3** is split: the SDK-blocked signing half is **deferred past 1.0**; the SHA-256-checksum/release-
  automation half lands in **M12**.

---

## Status

STATUS: Designed — GA roadmap drafted; awaiting developer approval to begin M7.
