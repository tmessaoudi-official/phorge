# Agent P — Plan-File Verdict Table (Stage 4 prep)

> Produced 2026-07-02. Scope: all **66** files in `docs/plans/` (specs are frozen records, out of scope).
> Evidence base: per-plan full reads (4 extraction passes), `docs/research/full-audit/raw/C-decisions.md`
> (register: 141 DEC rows, 10 conflicts, 33 supersessions), `CHANGELOG.md`, `docs/MILESTONES.md`,
> `ROADMAP.md`, project `CLAUDE.md`, memory index — plus **first-hand code anchors run 2026-07-02**
> (every `rg`/`ls` cited below was executed this session against the working tree at `ccb2403`).

## Operating rules used (state them so the verdicts are auditable)

1. **A deferral does not block deletion when it is recorded on a still-live surface** — KNOWN_ISSUES,
   ROADMAP.md, the decision register, or a surviving MERGE plan. Otherwise nothing would ever be
   deletable (every plan defers something). An open item recorded **only** in the plan itself forces
   MERGE.
2. **Stale checkboxes/status lines are not evidence of open work** — early plans never tick boxes
   (verified convention: all M1/M2 plans, `2026-06-21-stack-traces-impl`, `2026-06-25-overnight` etc.
   carry `- [ ]` for shipped work). Code anchors decide.
3. **Historical path citations do not block deletion.** Precedent verified: **7 plan files dated
   06-22/06-23 were already deleted** (`totality-cluster`, `generic-enums`, `method-overloading`,
   `error-model-slice2`, `pattern-cluster`, `decomposition-milestone`, `m-rt-s8-traits`) and are still
   cited by CLAUDE.md / MILESTONES.md / CHANGELOG — [Verified: `ls docs/plans/ | grep '2026-06-2[23]'`
   returns nothing; `grep -r "docs/plans/2026-06-2[23]" --include='*.md'` returns 7 dangling paths].
   Dangling-citation cleanup is already a queued doc-reconciliation task (register C-7 / adjudication Q4).
   Only a reference **as active** (MILESTONES "forward SSOT", CLAUDE.md "Active plan") blocks deletion.
4. Register rows are cited as `C:DEC-xxx`; conflicts as `C:C-n`.

## Verdict summary

| Verdict | Count |
|---|---|
| **DELETE-VERIFIED** | **48** |
| **MERGE** (live content → master plan) | **15** |
| **KEEP-AS-RECORD** (adjudication-pending decision logs) | **2** |
| **KEEP-ACTIVE** | **1** |
| Total | 66 |

---

# 1. DELETE-VERIFIED (48 files)

Shared macro-anchors verified once, reused below — [Verified 2026-07-02]:
- Pipeline dirs all exist: `src/{lexer,parser,checker,interpreter,compiler,vm,transpile}/` (ls).
- `src/chunk.rs` holds the full Op set (112 `Op::` refs) incl. `MakeMap(usize)`, `MakeRange(bool)`,
  `MakeClosure(usize)`, `CallValue(usize)`, `IsInstance(String)`, `CallParent(usize,usize)`,
  `SetIndexLocal(usize)` (grep, line-numbered hits).
- `tests/differential.rs` exists with `agree_err` fault-parity (grep hit line 6).
- `examples/guide/` holds 100+ byte-identity-gated programs (ls, full listing captured).
- MILESTONES.md: M1 ✅ L8, M2 ✅ L19, M5 ✅ L248, M7 ✅ L284, M-Decomp ✅ L132, M-mut ✅ L194,
  M-TIME ✅ L175, visibility ✅ L213.

### M1 plans (7 files) — milestone ✅ COMPLETE (MILESTONES.md L8, tag `9da6e56`)

| File | Evidence |
|---|---|
| `2026-06-15-m1-plan1-scaffold-lexer.md` | (a) no in-file marker (convention, rule 2); (b) `src/lexer/` exists; every example lexes (differential glob green per CLAUDE.md baseline ~453 tests); (c) C:DEC-001, MILESTONES M1. |
| `2026-06-15-m1-plan2-parser-expressions.md` | (b) `src/parser/` (5-cluster split, M-Decomp); interpolation shipped (`examples/guide/strings.phg`); (c) MILESTONES M1, M-Decomp L145. |
| `2026-06-15-m1-plan3-statements-declarations.md` | (b) `src/parser/` + `parse_program` public API in use by loader; deliberate M1 limits (reassignment, while) all later shipped (M-mut ✅ MILESTONES L194); (c) MILESTONES M1. |
| `2026-06-15-m1-plan4-typechecker.md` | (b) `src/checker/` (11-cluster split); its "not yet supported in M1" gates (optionals, indexing, `\|>`, overloading) all shipped in S1/S2/S3/M-RT; (c) MILESTONES M1+M3. |
| `2026-06-15-m1-plan5-evaluator.md` | (b) `src/interpreter/` + `src/value.rs` (`Instance(Rc<Instance>)` grep hit); (c) MILESTONES M1. |
| `2026-06-15-m1-plan6-cli.md` | (b) `src/cli/` with cmd_* family (`cmd_bench`/`cmd_explain`/`cmd_lift` grep hits show the CLI grew far past plan 6); (c) MILESTONES M1 "CLI" bullet. |
| `2026-06-15-m1-polish.md` | (a) in-file `Status: Done — M1 polish complete.`; (b) `examples/fib.phg`, `examples/hello.phg` exist (ls); (c) MILESTONES M1. |

### M2 / transpile / examples (7 files) — milestone ✅ COMPLETE (MILESTONES L19)

| File | Evidence |
|---|---|
| `2026-06-15-m2-plan1-vm-core.md` | (b) `src/chunk.rs` (112 Op refs) + `src/vm/`; (c) MILESTONES M2 P1 ✅. |
| `2026-06-15-m2-plan2-compiler-runvm.md` | (b) `src/compiler/` (`enum CTy` grep hit) + `tests/differential.rs`; (c) MILESTONES M2 P2 ✅ (cites this plan's path — historical citation, rule 3). |
| `2026-06-15-m2-plan3-functions-callframes.md` | (b) call frames + recursion live (`examples/fib.phg` gated); (c) MILESTONES M2 P3 ✅. |
| `2026-06-15-transpile-php.md` | (b) `src/transpile/` (M-Decomp split); its "not yet supported" gates (`is`, `\|>`, null, indexing, expr-match) all later shipped incl. native expr-match (transpile-modernization ✅); (c) M7 PHP oracle enforces the leg, C:DEC-003. |
| `2026-06-16-examples-coverage.md` | (b) `examples/realworld/{ledger,library,rpg,shop}.phg` exist (ls) + differential glob; (c) CLAUDE.md "full-coverage example set landed", C:DEC-011. |
| `2026-06-16-m2-p3.5-hardening-roadmap.md` | (a) Waves 0–3 ✅ in-file; the single unchecked box (Task 1.4 Part 2, `num_ty`→Member/Index) closed by Wave-4 plan + M3 S1 indexing; (b) `agree_err` in differential.rs (grep), `BytecodeProgram::validate`; (c) MILESTONES M2 criteria, Wave-4 plan below. |
| `2026-06-16-m2-wave4-compiler-types.md` | (a) all steps `[x]`; (b) `enum CTy` in `src/compiler/mod.rs` (grep hit); (c) MILESTONES M2 Wave 4 ✅, C:DEC-122. |

### M2 P4/P5a, M2.5 (4 files)

| File | Evidence |
|---|---|
| `2026-06-16-m2-p4-classes-enums-match.md` | (a) P4a/b/c ✅ DONE in-file with `[x]`; (b) `Op::CallMethod`/enum ops in chunk.rs (112-op set), `examples/grades.phg` (ls); (c) MILESTONES M2 P4 ✅, C:DEC-120. |
| `2026-06-16-m2-p5a-rc-shared-heap.md` | (a) A0–A5 all `[x]`, Phase-B non-opening was the *decision*; (b) `Instance(Rc<Instance>)` in src/value.rs (grep); Phase B itself later shipped with evidence (slot-indexed S1a/S1b + inline cache, 06-28 marathon); (c) C:DEC-121, MILESTONES M2 P5a ✅. |
| `2026-06-16-m2.5-phase1-build-linux-gnu.md` | (b) `src/bundle/` module (ls: container/elf/pe/macho/section/cross/sha256) + `tests/build.rs` (16.4K); (c) MILESTONES M2.5 Phase 1 ✅, C:DEC-160. |
| `2026-06-16-m2.5-phase2-cross-os.md` | (b) `pe.rs`/`macho.rs`/`cross.rs` with `fnv1a_64` (grep hit L331); (c) MILESTONES M2.5 Phase 2 ✅. Phase-3 deferral is tracked in ROADMAP M2.5 🔲 + ga-sequence Item 8 (MERGE carrier) → rule 1. |

### M3 S0–S3, v0.4.0, backfill (6 files)

| File | Evidence |
|---|---|
| `2026-06-17-examples-backfill.md` | (a) 13 boxes unchecked (rule 2); (b) all deliverables exist: `examples/guide/functions.phg`, `examples/build/app.phg`+README, `examples/cli/demo.phg`+README (ls, first-hand); (c) the standing rule it created = C:DEC-011 + CLAUDE.md "Examples ship with features". |
| `2026-06-17-m3-s0-dx.md` | (b) `Type::Infer` (grep src/ast) + `cmd_explain` (grep src/cli/explain.rs:1297); (c) MILESTONES M3 "S0" ✅, C:DEC-080. |
| `2026-06-17-m3-s1-ergonomics.md` | (b) `MakeRange(bool)` chunk.rs:187 (grep) + `examples/guide/ergonomics.phg` (ls); S1.4 deferral landed in S2 as planned; (c) MILESTONES M3 S1 ✅. |
| `2026-06-17-m3-s2-null-safety.md` | (a) in-file "S2 COMPLETE" + tasks `[x]`; (b) `examples/guide/null-safety.phg` (ls), `Ty::Optional` machinery; (c) MILESTONES M3 S2 ✅, C:DEC-081. |
| `2026-06-17-v0.4.0-profiling-disasm.md` | (b) `cmd_bench` (src/cli/bench.rs:21 grep) + `examples/bench/workload.phg` (ls); (c) MILESTONES "Tooling (v0.4.0)" section, C:DEC-161. CLI verbs since renamed (C:C-7 doc task, not plan work). |
| `2026-06-18-m3-s3-lambdas-pipe.md` | (b) `MakeClosure(usize)`+`CallValue(usize)` in chunk.rs (grep) + `examples/guide/lambdas-pipe.phg` (ls); deferrals all rehomed: `this`-capture → superseded by `E-BARE-FIELD` rule (C:DEC-066, grep hit src/checker/expr.rs:95), `core.list` map/filter/reduce → shipped (S7b-3, src/native/list.rs 33.7K); rest in KNOWN_ISSUES; (c) C:DEC-082. |

### M5 / M6-research / M8-import / M7 (4 files)

| File | Evidence |
|---|---|
| `2026-06-18-m5-modules-packages.md` | (a) S1–S3 `[x]`, "M5 COMPLETE" in-file; (b) `src/{loader/,manifest.rs,vendor.rs,lock.rs}` (ls) + `examples/project/{tempconv,withdeps,…}` (ls, 9 projects); (c) MILESTONES M5 ✅, C:DEC-025/028/029/030/031/033. Deferrals (transitive deps, `phg build` vendor-merge) live in KNOWN_ISSUES + GA-roadmap M11 (MERGE carrier) → rule 1. |
| `2026-06-18-m6-web-capabilities-research.md` | Stale "RESEARCH (no code yet)" header; everything it gated **shipped**: (b) `src/serve.rs` (ls) + `examples/web/` (ls: 11 files incl. handler/router/middleware/server.php); its open questions were resolved in `docs/specs/2026-06-18-m6-web-design.md` (kept) — (c) C:DEC-140/141/142, MILESTONES M6 "CORE COMPLETE". |
| `2026-06-18-m8-php-import-design.md` | "DESIGN PHASE (build deferred)", Formal Plan empty — **explicitly superseded**: the ↑ direction shipped as M-Lift ((b) `src/lift/` ls: lexer/parser/printer/lifter + `tests/lift_roundtrip.rs`; `cmd_lift` grep src/cli/mod.rs:1341); the quality bar it minted = C:DEC-015; Stage-C dynamic-PHP rejection = C:DEC-166 tier table. Its unshipped co-features (named args, variadics) are carried by `full-bidirectional` (MERGE) → rule 1. |
| `2026-06-19-m7-correctness-closure.plan.md` | (a) in-file "✅ Implemented — W1–W4 complete" + its own Phase-8 note proposes delete-plan-keep-spec; (b) PHP oracle live (`PHORJ_REQUIRE_PHP` contract in CLAUDE.md gate; `__phorj_div/_rem/_str/_range` helpers per MILESTONES M7 ✅ L284); (c) C:DEC-003/162. |

### 06-21 cluster (5 files)

| File | Evidence |
|---|---|
| `2026-06-21-php-parity-review.plan.md` | Never executed **by design** — explicitly superseded: roadmap-completeness plan L10-11 "supersedes the narrower php-parity-review, which becomes Track A". (c) C-decisions SUPERSEDED table row "php-parity-review → 20-track review". Zero unique live content. |
| `2026-06-21-roadmap-completeness-review.plan.md` | (a) in-file "STATUS: COMPLETE — audit delivered, decisions locked, written back"; (b) deliverable SSOT exists `docs/specs/2026-06-21-php-parity-and-beyond.md` + write-backs verified in ROADMAP.md §audit + MILESTONES.md §audit; (c) C:DEC-060/068 + register §10 summarizes the 555-row triage. Downstream backlog lives in ROADMAP/spec, not here. |
| `2026-06-21-error-handling-and-traces.plan.md` | (a) "SLICE 1 COMPLETE — 690 tests green" in-file; Slice 2 (its only open pointer) later CLOSED (memory error-model-slice2; `throws` parser live — grep src/parser/types.rs:22; `examples/guide/{errors,result,cause-chain}.phg` ls); Task-7 color deferral → KNOWN_ISSUES; (c) C:DEC-068/155, MILESTONES L225. |
| `2026-06-21-stack-traces-impl.plan.md` | All boxes unchecked (rule 2) but companion plan records SLICE 1 COMPLETE; (b) `trace_stack` (grep src/interpreter/coop.rs:52) + `Diagnostic.frames` (grep src/diagnostic.rs:70); (c) MILESTONES "Error handling & stack traces Slice 1 ✅", spec kept. |
| `2026-06-21-ga-direction-and-autonomy.plan.md` | "PAUSED" header is stale — every paused thread resolved downstream, each verifiable: modifier model CONFIRMED in-file L132-138; M-mut.2–.7 all shipped (MILESTONES M-mut ✅ L194, itemized .1–.7b); matrix re-grade done (register §10 "verdicts re-graded under craftsmanship-apex"); Wave-A backlog superseded by the 20-track SSOT (the plan itself designates php-parity-and-beyond as build-order holder); MI revisit → S6 shipped (C:DEC-062); autonomy contract → memory ga-direction-and-autonomy + C:DEC-010; mutation decisions → C:DEC-065 + mutation spec. |

### 06-24 → 06-26 execution plans (9 files)

| File | Evidence |
|---|---|
| `2026-06-24-new-const-fieldinit.plan.md` | (a) "PLAN COMPLETE" (A `c6b1ac2`, B `4873d45`/`af3ad03`, C `5fb1259`); (b) `examples/guide/{constants,field-init,static-init}.phg` (ls); the loose-end playground repro was fixed 06-29 (`25e34e6`, memory session-playground-fix); (c) C:DEC-083/084/085. |
| `2026-06-24-playground-wasm.plan.md` | No in-file completion marks (rule 2) but shipped: (b) `playground/` + `playground/web/` exist (ls); memory playground-wasm = milestone closed; (c) C:DEC-164. Residual "deploy + live re-verify after push" folded into the global unpushed item (LI-H1). |
| `2026-06-25-transpile-modernization.plan.md` | (a) "Status — COMPLETE" + T6d COMPLETE in-file (7 commits listed); irreducible helpers documented as by-design; (c) C:DEC-165, memory transpile-modernization. |
| `2026-06-25-m-lift-php-to-phorj.plan.md` | (a) L1–L6 all COMPLETE in-file with commits; (b) `src/lift/` (ls: 44K parser, 31K lifter, 29K printer) + `tests/lift_roundtrip.rs` (ls); (c) C:DEC-166/167. The "NEXT: Tier-2 build-out" + playground lift button are carried verbatim by the `full-bidirectional` umbrella (MERGE) → rule 1. |
| `2026-06-25-overnight-autonomous-session.plan.md` | Every open box resolved downstream: UFCS built (`0dc071c`, `examples/guide/ufcs.phg` ls); F-006 Core.Reflect shipped ((b) `src/native/reflect.rs` 11.7K ls, `examples/guide/reflect.phg`); F-007 Process I/O shipped ((b) `src/native/process.rs` ls + commit `fc32707` "Core.Process/Core.Env natives + impure-native marker"); Slice-7 Text natives → M4/M-text carrier (ga-sequence MERGE); superglobal map → doctrine C:DEC-092. (c) C:DEC-012/087. |
| `2026-06-26-autonomous-backlog.plan.md` | (a) items 1–4 `[x]`, "All backlog items resolved"; item-5 `sort` deferral shipped in M4 ((b) sort/sortWith in `src/native/list.rs` + `examples/guide/sort.phg` ls); (c) C:DEC-111/145. |
| `2026-06-26-default-parameters.plan.md` | (a) `[x]` COMPLETE; (b) `examples/guide/default-params.phg` (ls); first-class-value fill deferral → KNOWN_ISSUES; (c) C:DEC-101. |
| `2026-06-26-m-num-decimal.plan.md` | (a) S1–S4 ✅, "M-NUM — CLOSED"; (b) `Value::Decimal` (grep src/value.rs:118) + `src/native/decimal.rs` + `examples/guide/{decimals,decimal-div}.phg` (ls); (c) C:DEC-147/148/149. M-NUM-2 deferrals (BigInt, arb-precision, Money) already in ROADMAP → rule 1 + LI-D10. |
| `2026-06-26-m4-stdlib-breadth.plan.md` | (a) pinned backlog 1–6 all ✅ DONE, slices 1/2/2a/2b DONE; (b) `src/native/convert.rs` (17.7K ls) + `examples/guide/{map-ops,list-ops,text-ops,set-ops,as-cast}.phg` (ls); (c) C:DEC-104/146. |
| `2026-06-26-native-modules-research.plan.md` | Research executed: (b) `docs/research/native-modules/SSOT.md` (30.2K) + `docs/research/extended-modules/SSOT.md` (43.1K) exist (ls); its Tier-A build list shipped (src/native/: encoding/hash/url/validate/csv/random all present, ls) and Regex shipped via the vetted dep (Cargo.toml `regex` feature, grep); (c) C:DEC-007/150. Its stale "zero-dep LOCKED FRAMING" = C:C-3 (register-captured). Unbuilt residue (Sql/DB/HTTP-client/Core.Dump/Caching) ⊂ `native-modules-extended-scope` backlog (MERGE) → rule 1. |
| `2026-06-26-retire-var-declaration-reshape.plan.md` | Self-superseding plan (its own log records the reversal → keep-`var`-contextual); shipped: (b) `E-RESERVED-NAME` (grep src/checker/tests/casing.rs) + `examples/guide/contextual-var.phg` (ls); (c) C:DEC-100 (cites both logs), memory contextual-var-and-reserved-names. F-m reserved-name guard → done in autonomous-backlog item 4. |

### 06-27 → 07-01 (3 files)

| File | Evidence |
|---|---|
| `2026-06-27-as-primitives-matrix.plan.md` | (a) S1–S4 `[x]`, "MATRIX COMPLETE"; (b) `examples/guide/{as-primitives,numeric-convert}.phg` (ls) + `src/native/convert.rs`; deferrals (union-as-decimal, erased-generic sources, float→decimal overflow bound) → KNOWN_ISSUES; (c) C:DEC-104, memory as-primitives-and-crypto-session. |
| `2026-06-28-ga-marathon-super-overloading.plan.md` | (a) "MARATHON COMPLETE" in-file (all 6 steps DONE + committed); (b) `CallParent(usize,usize)` chunk.rs:300 + `examples/guide/{parent-dispatch,parent-dispatch-mi,return-overloading,must-use}.phg` (ls) + `src/lsp/` (ls: scope/symbols); (c) C:DEC-059/069/127. Deferrals (C2 remaining sinks, MI lowering corners, cross-file *references*, overloaded parent methods) → KNOWN_ISSUES + LI-E13. |
| `2026-06-29-big-marathon-crosspkg-soundness-stdlib-concurrency.plan.md` | Spines 1–3 DONE in-file; the open crux (S4.3 cooperative cutover) **completed in the next marathon** (mega-marathon records "A1 ✅ COMPLETE", memory marathon-a1, litmus passed): (b) `src/green/` (ls: coro/exec/sched/spike) + Cargo.toml `corosensei` feature (grep) + `examples/guide/concurrency.phg` (ls); (c) C:DEC-132/133/134. The one open design fork (sprintf) is carried by mega-marathon C3 (MERGE) → rule 1. DEC-133 Round-3 adjudication reads from the register, not this plan. |
| `2026-07-01-m-dx-error-experience.plan.md` | (a) S0–S5 all `[x]`, "M-DX COMPLETE — all 6 slices shipped"; (b) `src/{profile.rs,inspect.rs,debug.rs,dap.rs,json.rs}` all exist (ls) + `examples/guide/assertions.phg` (ls); (c) C:DEC-129, memory m-dx-error-experience. Its deviations/deferrals (VM named-locals, VM stepping, conditional breakpoints) are exactly Lane 3 of the four-lane plan (MERGE) → rule 1. |

---

# 2. MERGE (15 files) — live content itemized

> This section is the raw material for the master plan. Items are cross-referenced into the
> deduplicated inventory (LI-x) in §5.

## M-1 `2026-06-19-phorj-ga-roadmap.plan.md` — the GA tracker (BIG)
Referenced **as active** by MILESTONES.md M8–M12 section ("the forward SSOT lives in
docs/plans/2026-06-19-phorj-ga-roadmap.plan.md") → cannot delete regardless. M7 ✅; M9 partially ✅.
**Caution: many checkboxes are stale** (M10 generics shipped as S7a/generics-all; M11 core.list/json/Map/Set
shipped; transpiler `is`/expr-match rejects fixed) — the master plan must re-verify per item. Live:
- All 11 GA exit criteria (L27–36) → LI-G1.
- **M8 security findings** (L77–104): git `--` separation, dep-name path traversal, serve `catch_unwind`,
  free-fn/PHP-builtin collision, promoted-field visibility, stub-cache atomicity, vendor swap window,
  malformed Content-Length, slowloris/read timeout, symlink escape, lock re-validation
  (`E-VENDOR-TAMPERED`), lockfile hash verify, git-env isolation, `write_atomic` theme, php_compat lints → LI-G2.
- **M9 leftovers** (L122/125/134–155): single-sourced fault strings + capture-filter + call-head;
  interpreter fault source line (may be fixed by traces Slice 1 — verify); `phg explain` codes derived
  from `explain_text` (M-DX S1 ratchet may cover — verify); manifest dup keys; `index_of_by_leaf`
  uniqueness; digit-leading-dir PHP namespace; comment-strip quote balance; `eq_val` Map/Set (S7b-2
  realign may cover — verify); folder=path canonicalize mismatch; lock `PartialEntry::finish` line;
  `validate` argc/arity EV-7 holes; scratch-slot arithmetic checks; `#[ignore]`d socket smoke test;
  Rc-share decls per call → LI-G3.
- **M10 residue**: `id(7)+1` generic-result-operand VM gap (still open, KNOWN_ISSUES); arm-unification
  null-typing; mangle non-injectivity → LI-G4.
- **M11 residue**: typed Header; library-package fn values; block-body return inference; fn-type
  variance; `phg build` bypasses project loader / vendor-merge; transitive git deps → LI-F5/LI-G4.
- **M12** (all open): language reference, tour, migration guide, transpile-contract doc, fuzzing,
  TextMate/tree-sitter grammar, release automation + SHA-256 + 1.0 bump, P3 #46–#50 → LI-G5.

## M-2 `2026-06-20-post-wave3-four-tracks.plan.md`
Track 3 slices later shipped (E-PKG-CASE — grep hit src/cli/explain.rs:281; `package Main` reshape
`15a5745`). Live (recorded only here):
- GA punch-list P1-c (ext-policy denylist CI scan), P1-f (fuzz/no-panic EV-7 harness), P1-g (route
  `check --json` through the loader) → LI-G6.
- P2 transpiler-fidelity cluster (`==`→strict `__phorj_eq`, trim/upper/lower ASCII parity, per-call-site
  scratch names, per-native PHP-mapping differential, core.file read cap/doc, built-binary exit status,
  serve eager respond validation, `index_of_by_leaf` parity) → LI-G6.
- P3 batch (vendor `&rev[..12]` char boundary, overflow-checks profile, stale-comment cleanup,
  diagnostic-code/explain coverage enforcement test) → LI-G6.
- **Benchmark vs optimized PHP** (opcache, release NTS) before public perf claims → LI-F7.

## M-3 `2026-06-24-language-evolution-master.plan.md`
Nearly all shipped (S0a/S0b, string `+`, `**`, or-patterns, types, closures, UFCS, let-destructuring,
mandatory `new`, const, field-init). Live:
- **Ternary `? :` deferred-not-rejected** — revisit trigger recorded (C:DEC-090; C:C-5 stale perimeter
  record) → LI-E9.
- Slice 7 stdlib (`Text.charAt`/`substring` byte-vs-codepoint) → M-text → LI-D9.
- Phase-2 superglobal **documentation map** ($_GET→Request routing table) — doc item → LI-H4.
- Playground deploy live re-verify (post-push) → LI-H1.
- Heavy Decisions Log (~15 entries) = C:DEC-083–092 — content register-captured; keep file until
  master plan lands, then delete with the batch.

## M-4 `2026-06-25-full-bidirectional-php-support.plan.md` — umbrella
Wave 1 ✅; sub-plans complete. Live:
- **W2.2 variadic params — [Verified ABSENT: `grep -ri variadic src/parser src/checker` → no hits]** → LI-E10.
- **W2.3 named arguments — [Verified ABSENT: grep → no hits]** → LI-E10.
- W2.4 attributes as general inert metadata (beyond the shipped `#[Route]`) + open inert-vs-behavior
  decision → LI-E10.
- Lift Tier-2/Tier-3 inference depth (array→List/Map/Set, foreach element types, defaults, backed
  enums, key-foreach, elvis, assign-as-subexpr…) → LI-F6.
- Playground "paste PHP → Phorj" input mode → LI-F6.
- W1.1 narrow corners: `private` *static* field visibility via `ClassName.field`; intersection-typed-
  receiver member visibility → LI-E14.

## M-5 `2026-06-25-php-fidelity-and-divergence-audit.plan.md`
15/16 shipped; A-46 later shipped too (M-mut.2, C:DEC-096). Live:
- **`->` return-syntax full removal** — parked for a string-literal-scoped tool; [Verified still parsed:
  src/parser/types.rs:109 eats `TokenKind::Arrow` as alias]. Same item as dogfood DF-1 normalization → LI-E11.
- `W-SEQUENCE-MUTATION` lint — register flags status **unverified**; confirm shipped-or-schedule → LI-E11.
- A-6 follow-up binding forms (key/value foreach destructure variants rejected "follow-up") + C-2
  foreach-vs-for-in adjudication outcome execution → LI-E12.

## M-6 `2026-06-26-developer-idea-backlog.plan.md`
Batches A–G ✅; Batch-1 B (`main(args)`) and C (`handle` entry) shipped downstream ([Verified:
`ast::entry_point` + `E-MULTIPLE-MAIN` greps; commit `b710c6e`]). Live narrow deferrals (recorded only
here + KNOWN_ISSUES):
- Static-init constructing a parent's `protected` ctor (init-expr scan missing) → LI-E14.
- Interface-method `throws` not discharged through interface-typed receiver; method-`?` propagation
  (`x.m()?`) → LI-E14.
- Nested un-inferred generic placeholder conservatively rejected → LI-E14.
- Static-call ergonomics: cross-class `Class.method()`, static-via-instance rejection, transpiler
  `static function` emission → LI-E14.

## M-7 `2026-06-26-native-modules-extended-scope.plan.md`
The module-backlog holder. Shipped since: Core.Test/Mock/Faker (M-Test), Core.Random pure-PRNG,
green-thread async core. Live:
- Unbuilt modules: **Core.Serde, Core.Event, Core.Cli, Core.Template, Core.Uuid (v5/v3 pure; v4 policy),
  Core.Log (record pure / emit Tier-B), Caching/memoize, Core.Dump** ([Verified absent from
  src/native/ ls]), rich HTTP response types (JsonResponse/RedirectResponse/HtmlResponse/StreamResponse
  — partial vs shipped `Response.text`), **Sql builder + DB execution (Tier B), HTTP client** (→ Q3) → LI-D8.
- Per-slice open decisions D-Stream / D-Test-Q1 / D-Mock / D-Http / D-Cache / D-Db ("resolved as each
  is built") → LI-D8.
- D-Async-1 residue: pure data-parallel `Core.Parallel` map/forkJoin + reactive subset (partially
  superseded by green threads — re-scope against DEC-132/135) → LI-A3.

## M-8 `2026-06-27-ga-sequence.plan.md` (BIG, heavily stale)
Items 2/3 (fmt, M-Test), decision-fixes 9/9, Rock 3, LSP v2, M8.5, slot-indexed S1a/S1b+S2, cross-file
LSP defs, public-surface file rule, Phorj crate rename — all DONE (here or downstream). Live:
- **Item 1 M4 charter** — charter spec adopted ([Verified: docs/specs/2026-06-29-m4-stdlib-charter.md
  exists]); the "minimal enforcement" half unverified → LI-D11.
- **Item 4 M-text** (codepoint ops, `s[0]`, locale-free extras) → LI-D9.
- **Item 5 breadth gaps** (json safe-parse hardening, path/log/sprintf leftovers) → LI-D1/D3/D8.
- Item 7 residue: **fmt F5 lift-comment fidelity** deferred → LI-F6.
- **Item 8 release-readiness**: M8 hardening chain (→ M-1), GA governance doc-bundle remainder,
  **M2.5 Phase 3b** `--sign` + macOS stub (cert/SDK-blocked) + CI stub registry productionization → LI-F8/G5.
- LSP v2 deferrals: member completion (resolved-type index), lambda/match-pattern binders; cross-file
  **references**; natively-compiled **JetBrains plugin** finish → LI-F4.
- `Math.rem`/`mod` (+`fmod`?) follow-up — never closed → LI-D12.
- GitHub repo rename + dir `mv` (manual, from the Phorj rename) → LI-H2.

## M-9 `2026-06-30-mega-marathon-23-workstreams.plan.md` (BIG — the standing feature backlog)
A1, B1, naming ✅. Live workstreams (each individually open):
- **A2 generators/`yield` + lazy sequences** (flagged NEXT; dogfood says "resume after") → LI-A1.
- **A3 async/await + structured concurrency** (`Task.all`, select/race, timeouts) → LI-A2.
- **B2 comprehensions**; **B3 tuples + multiple returns + deferred `zip`**; **B4 stdlib blitz** → LI-E1/E2/D2.
- **C1 enum methods + associated fns**; **C2 match extras (range patterns, `@` bindings)**;
  **C3 `Core.Fmt` + interpolation format specs** (absorbs the sprintf fork); **C4 ergonomics pack**;
  **C5 core.json dynamic `Any`/`Json` type** → LI-E3/E4/D3/E5/D4.
- **D1 protocols** (Comparable/Equatable/Iterable/Display + operator dispatch) → LI-E6.
- **E1 M-perf VM pass**; **E2 incremental/cached compilation** → LI-B1/B3.
- **F1 `phg repl`**; **F2 `phg doc`**; **F3 DAP step-through** (interpreter leg shipped in M-DX; VM leg
  = Lane 3) → LI-F1/F2/C1.
- **G1 real-app showcase + tutorial**; **G2 docs site + playground polish** → LI-F3.
- **H1 multicore actor parallelism** (= M-Parallel); **H2 compile-time macros**; **H3 FFI/native
  extensions**; **H4 editions (M13)** → LI-A3/E7/E8/G7.
- A1 KNOWN_ISSUES follow-ups: method/overloaded/closure-spawn deferral, coop fault-trace frames,
  per-task statics; wasm stays eager pending frame-swap executor → LI-A4.

## M-10 `2026-07-01-m-dogfood-benchmark-marathon.plan.md`
W0–W12 mostly ✅. Live:
- W6 large-data memory stress only `[~]` partial → LI-F7.
- **6 of 8 benchforge benchmarks intentionally unported** (need in-place cross-call mutation);
  Sorting stays blocked on by-ref params → feeds post-dogfood W3 → LI-F7/E15.
- `W-UNKNOWN-IMPORT` lint deferred (needs single-sourced known-module set) → LI-E11.
- DF-1: `phg fmt` normalization `->`→`:` + optional deprecation lint (same as LI-E11 `->` item).
- A2 generators paused "resume after" (dup of LI-A1).

## M-11 `2026-07-01-post-dogfood-workstreams.plan.md` — ALL FIVE OPEN
- **W1 enforcement audit** — enumerate every language rule + should-error tests; findings→fixes;
  conformance suite → LI-E16.
- **W2 field-base index-assign** `this.f[i]=e`/`obj.f[i]=e` (extends `84622c2`; today `E-ASSIGN-TARGET`) → LI-E15.
- **W3 port remaining benchforge benchmarks** (Search, StringProcessing, ObjectGraph; in
  /stack/projects/phorj-app, no auto-commit) → LI-F7.
- **W4 import-roots PSR-4** `[packages]` map + `vendor:` prefix + loader/checker/transpiler +
  migration codemod (spec committed `8fc85f2`, C:DEC-048 📐) → LI-E17.
- **W5 clarity**: ARCHITECTURE.md narrated rewrite, module `//!` docs, **blanket `clippy::pedantic`
  fix-ALL** (dev overrode selective, C:DEC-176) → LI-F9.

## M-12 `2026-07-01-post-m-dx-four-lane-backlog.plan.md` — the locked lane SSOT (`b85fcd8`)
Lane 1 ✅, Lane 2 W1 ✅ ([Verified: scripts/perf-gate.sh exists]). Live:
- **Lane 2 M-perf W2–W7**: W2 Rc-share `Value::Str` (scoped, deferred, "NEXT concrete perf step");
  W3 intern IsInstance; W4 dispatch; W5 const-fold; W6 peephole; W7 lazy for-range → LI-B1/B2.
- **Lane 3 VM debug symbols W1–W5** (scope IP ranges → named locals at fault → VM pause hook → wire
  into debug engine so REPL+DAP run over runvm → examples/docs) → LI-C1.
- **Lane 4 stdlib breadth W1–W8** (json encode/safe-parse, regex breadth, sprintf, hash/encoding,
  path/url, log, iterators, collections audit; one charter decision per module) → LI-D1–D7.
- **Q1 method-references-as-values** + typed-registry guide (C:DEC-107 📐) → LI-E13.
- **Q2 filesystem remainder** — [Verified partial: src/native/file.rs has append/delete/rename/copy/size
  (commit `a23ca00`); **mkdir/listDir/isDir/metadata + Core.Directory absent**] → LI-D5.
- **Q3 M-HTTP-Client** (Guzzle-style incl. HTTPS/rustls fork; design-spec first) → LI-D6.
- **No-wind closure implementation** (C:DEC-047 📐): intrinsics behind `import Core;`
  ([Verified: `E-UNIMPORTED` absent from src/]), deep imports, stdlib aliasing, de-reserve
  Attr/Error/Channel/Task → `Core.Async` → LI-E18.
- **M-Parallel deep plan** (ON HOLD, Fable to audit; C:DEC-135 📐) → LI-A3.
- Folded ADD candidates: `phg repl`, `phg doc`, parser multi-error recovery, A2 generators,
  opportunistic wins → LI-F1/F2/E19/A1.

## M-13 `2026-06-19-m3-global-review-pass.plan.md` (tiny)
Review executed (deliverable exists: ~/.claude/projects/-stack-projects-phorj/REVIEW-2026-06-19.md, 30K,
ls-verified); findings absorbed into the GA roadmap. Live, recorded only here:
- **Delete dangling branches** — [Verified STILL PRESENT: `git branch` shows
  `worktree-agent-a2764d080140ece46` AND `worktree-agent-af24cab61b7b26f18`] → LI-H3.
After LI-H3 executes, this file becomes DELETE-VERIFIED.

## M-14 `2026-06-18-trackB-stdlib-io-imports.md` (tiny)
Waves 1–2 ✅; the core.list/core.json deferral box is stale (both shipped — src/native/{list,json}.rs ls).
Live, recorded only here:
- **Task 6 / Track C: a file-reading realworld program + per-module coverage audit** — [Verified absent:
  `grep -rl "Core.File" examples/realworld/` → no hits] → LI-H5.
After LI-H5 is carried, deletable.

## M-15 `2026-06-27-big-chunk-entry-native-lift.plan.md` (small)
Stages 1–2 ✅ (6/6 natives), Slice A/B0/B1 ✅; Stage-3 lift done in m-lift plan. Live:
- **Slice C — class-static `handle` web entry** — [Verified OPEN: the serve bridge's `has_fn` matches
  only top-level `Item::Function` (src/cli/mod.rs:654-658, 674) — a `static handle` method is not
  resolved] → LI-E13.
- Slice B0 scope limits: own-class-only + non-overloaded static calls (dup of LI-E14 static ergonomics).

---

# 3. KEEP-AS-RECORD (2 files)

| File | Why kept |
|---|---|
| `2026-06-20-m-rt-rich-types.plan.md` | All build work shipped (M-RT CLOSED; the in-file S6/S8 "pending" rows are stale — both shipped via the already-deleted 06-22/06-23 plans). Kept because (1) project CLAUDE.md cites it **as the live record**: "Locked decisions + slice order live in the plan's Decisions Log"; (2) its ~28-entry log is the primary source for C:DEC-050–061 and adjudication **Round 2 (R2-A: generics explicit type args / no-turbofish)** reads from exactly this territory. Deletable after adjudication closes + CLAUDE.md rewrite lands. |
| `2026-06-25-overnight-design-forks-review.plan.md` | All 7 forks resolved (top table); per-entry `⏳ AWAITING CONFIRMATION` footers are stale. Kept because **R2-B (UFCS stability policy) is queued in the adjudication cursor** and this file is the primary rationale record for F-001/F-003 (C:DEC-087 RATIFIED). Deletable after R2-B closes. |

# 4. KEEP-ACTIVE (1 file)

| File | Why |
|---|---|
| `2026-07-01-full-audit-and-master-plan.plan.md` | The current session plan; holds the live adjudication cursor (Round 2 ASKED-PENDING, Round 3 queue). |

---

# 5. CONSOLIDATED LIVE-ITEM INVENTORY (deduplicated)

> Every unshipped item found across the 15 MERGE plans, deduplicated, with source pointers.
> **56 numbered items** (some hold sub-lists). This is the master-plan seed.

## A. Concurrency & runtime
- **LI-A1** Generators/`yield` + lazy sequences (mega A2 — flagged NEXT; dogfood "resume after"; four-lane folded candidate).
- **LI-A2** async/await + structured concurrency (`Task.all`, select/race, timeouts) (mega A3).
- **LI-A3** M-Parallel: multicore actor-model plan (four-lane ON-HOLD, C:DEC-135 📐; mega H1) + re-scope the D-Async-1 pure data-parallel/reactive subset (extended-scope) against green threads.
- **LI-A4** Green-thread follow-ups: method/overloaded/closure spawn deferral; cooperative fault-trace frames; per-task statics; wasm frame-swap executor (mega A1 deferrals; big-marathon).

## B. Performance (M-perf, Lane 2)
- **LI-B1** W2 Rc-share `Value::Str` — scoped+deferred, declared "the NEXT concrete perf step" (four-lane; C:DEC-128).
- **LI-B2** W3 intern `IsInstance` · W4 faster dispatch · W5 const-fold · W6 peephole · W7 lazy for-range (four-lane Lane 2; mega E1).
- **LI-B3** Incremental/cached compilation keyed on content hash (mega E2).

## C. VM debug symbols (Lane 3, all open)
- **LI-C1** W1 per-local scope IP ranges → W2 named locals at VM fault (`runvm --dump-on-fault`) → W3 VM per-line pause hook → W4 wire VM into `src/debug.rs` (REPL + DAP over runvm) → W5 `examples/debug/` (four-lane Lane 3; closes the M-DX S3/S5 deviation; mega F3 VM leg).

## D. Stdlib breadth (Lane 4 + module backlog)
- **LI-D1** `core.json` encode + safe-parse hardening audit (Lane 4 W1; ga-sequence Item 5).
- **LI-D2** B4 stdlib blitz — List/Map/Set/Text/Math easy wins (mega B4; Lane 4 W8 collections audit vs charter).
- **LI-D3** `Core.Fmt` / sprintf + interpolation format specs `{pi:0.2f}` — open design fork (variadic-vs-list, %-vs-{}) (mega C3; big-marathon fork; Lane 4 W3).
- **LI-D4** Dynamic `Json`/`Any` type via injected-type pattern (mega C5).
- **LI-D5** Q2 filesystem remainder: mkdir/listDir/isDir/metadata + `Core.Directory` (four-lane Q2; append/delete/rename/copy/size shipped `a23ca00`).
- **LI-D6** Q3 **M-HTTP-Client** — Guzzle-style typed client incl. HTTPS (rustls fork), middleware closures, pooling on green threads, Transport quarantine; **design spec first** (four-lane Q3; extended-scope HTTP-client Tier B).
- **LI-D7** Regex breadth vs PCRE `/u` (Lane 4 W2 — `Core.Regex` shipped; audit remaining surface), hash/encoding breadth (W4), path/url breadth (W5), log facility (W6, + extended-scope Core.Log), iterators (W7).
- **LI-D8** Extended-scope module backlog: Core.Serde, Core.Event, Core.Cli, Core.Template, Core.Uuid, Caching/memoize, Core.Dump ([Verified absent]), rich HTTP response types, Sql builder + DB execution (Tier B); per-module decisions D-Stream/D-Test-Q1/D-Mock/D-Http/D-Cache/D-Db.
- **LI-D9** **M-text**: codepoint-aware ops, `s[0]` string indexing, `Text.charAt`/`substring` semantics, locale-free extras (ga-sequence Item 4; language-evolution Slice 7).
- **LI-D10** **M-NUM-2**: BigInt, arbitrary-precision decimal, `Money`+currency (m-num deferrals; ROADMAP).
- **LI-D11** M4 charter *enforcement* half (charter spec adopted; minimal checks unverified) (ga-sequence Item 1).
- **LI-D12** `Math.rem`/`mod` (+`fmod`?) follow-up (ga-sequence L107, never closed).

## E. Language surface & soundness
- **LI-E1** B2 comprehensions (list + map) (mega).
- **LI-E2** B3 tuples + multiple return values + destructuring + `zip` (mega; language-evolution "revisit as named records").
- **LI-E3** C1 enum methods + associated functions (closes generic-enum-methods gap) (mega).
- **LI-E4** C2 match extras: range patterns, `@` bindings (mega).
- **LI-E5** C4 ergonomics pack (scope at start) (mega).
- **LI-E6** D1 protocols: Comparable/Equatable/Iterable/Display + operator dispatch (mega, design-first).
- **LI-E7** H2 compile-time metaprogramming/macros (mega, milestone).
- **LI-E8** H3 FFI / native extension interface (mega, milestone).
- **LI-E9** Ternary `? :` — deferred-not-rejected; revisit on demand (language-evolution; C:DEC-090, C:C-5 record fix).
- **LI-E10** PHP-parity call features: **variadic params** [Verified absent], **named arguments** [Verified absent], general inert attributes beyond `#[Route]` (+ inert-vs-behavior decision) (full-bidirectional W2).
- **LI-E11** Lint/normalization batch: `->` full retirement / `phg fmt` normalization to `:` (+ optional deprecation warning) [Verified `->` still parsed]; `W-SEQUENCE-MUTATION` status verify; `W-UNKNOWN-IMPORT` (needs single-sourced known-module set) (php-fidelity; dogfood W12/DF-1).
- **LI-E12** foreach follow-up binding forms + execute the C-2 foreach-vs-for-in adjudication outcome (php-fidelity A-6; register C-2 ratified Round 1).
- **LI-E13** Entry/value ergonomics: **Q1 method-references-as-values** (C:DEC-107 📐) + typed-registry guide; **class-static `handle` serve bridge** [Verified open: has_fn top-level-only, src/cli/mod.rs:654/674]; overloading C2 remaining sinks; cross-file LSP *references* (ga-marathon deferrals; big-chunk Slice C; four-lane Q1).
- **LI-E14** Narrow soundness holes (KNOWN_ISSUES-tracked, master plan should batch them): static-init protected-ctor scope; interface-method `throws` discharge; method-`?` propagation; nested un-inferred generic placeholder; static-call ergonomics (cross-class `Class.method()`, static-via-instance, transpiler `static function`); private *static* field + intersection-receiver visibility corners; MI lowering corners (transitive-jump, multi-of-multi, bare `parent.constructor()`, overloaded parent methods) (developer-idea-backlog; full-bidirectional; ga-marathon).
- **LI-E15** W2 field-base index-assign `this.f[i]=e` / `obj.f[i]=e` (post-dogfood W2) — also the key unblock for the in-place benchmark ports; by-ref params question behind Sorting stays open.
- **LI-E16** W1 enforcement audit — every language rule gets a should-error test; conformance suite (post-dogfood W1).
- **LI-E17** W4 import-roots PSR-4 `[packages]` + `vendor:` prefix + migration codemod (spec `8fc85f2`, C:DEC-048 📐) (post-dogfood W4).
- **LI-E18** No-wind closure implementation (C:DEC-047 📐): `import Core;` intrinsics (`E-UNIMPORTED` [Verified absent]), deep imports any depth, stdlib aliasing, de-reserve Attr/Error/Channel/Task → `Core.Async` (four-lane).
- **LI-E19** Parser multi-error recovery (four-lane folded candidate).
- **LI-E20** `id(7)+1` erased-generic result not a VM arithmetic operand — the standing run↔runvm surface gap (GA-roadmap M10; KNOWN_ISSUES).

## F. Tooling / DX / distribution
- **LI-F1** `phg repl` (mega F1; four-lane candidate).
- **LI-F2** `phg doc` generator (mega F2; feeds G2).
- **LI-F3** G1 real-app showcase + tutorial chapters; G2 public docs site + playground polish (mega).
- **LI-F4** LSP finish: member completion (resolved-type index), lambda/match-pattern binders, cross-file references, JetBrains natively-compiled plugin completion (ga-sequence).
- **LI-F5** `phg build`: merge `vendor/` (multi-file projects), bytecode-payload flip, project-loader routing (M5 deferral; GA-roadmap P2-#42; M-DX follow-up).
- **LI-F6** Lift Tier-2/3 depth + `phg fmt` F5 lift-comment fidelity + playground PHP-input button (m-lift/full-bidirectional/ga-sequence).
- **LI-F7** Benchmarks: vs optimized PHP (opcache/NTS) before public claims (post-wave3); W3 port Search/StringProcessing/ObjectGraph in /stack/projects/phorj-app (post-dogfood); W6 large-data stress completion (dogfood).
- **LI-F8** M2.5 Phase 3b: `--sign` (Authenticode + rcodesign notarize), macOS stub, CI stub-registry productionization (cert/SDK-blocked) (ga-sequence Item 8; ROADMAP Phase 3).
- **LI-F9** W5 clarity: ARCHITECTURE.md narrated rewrite, module `//!` docs, blanket `clippy::pedantic` fix-ALL (C:DEC-176) (post-dogfood W5).

## G. GA hardening / release (statuses stale — re-verify each)
- **LI-G1** The 11 GA exit criteria (GA-roadmap L27–36).
- **LI-G2** M8 security findings batch (15 items, listed under M-1).
- **LI-G3** M9 hygiene leftovers (15 items, listed under M-1; several possibly fixed since — verify).
- **LI-G4** M10/M11 residue: arm-unification null-typing, mangle injectivity, typed Header, library-package fn values, block-body return inference, fn-type variance, transitive git deps.
- **LI-G5** M12: language reference, tour, migration guide, transpile-contract doc, fuzzing (P2-#44 + P1-f), grammar files, release automation + SHA-256 + 1.0, P3 #46–#50, GA governance bundle remainder.
- **LI-G6** post-wave3 punch-list: P1-c denylist CI, P1-g loader-routed `check --json`, P2 transpiler-fidelity cluster, P3 batch.
- **LI-G7** H4 editions mechanism, M13 post-1.0 (mega; C:DEC-006).

## H. Housekeeping / process
- **LI-H1** **PUSH**: origin/master = `0d952a8`; everything since (M-DX, marathons, Lane 1, perf gate…) unpushed. Playground deploy + live re-verify rides the push.
- **LI-H2** GitHub repo rename + directory `mv` phorge→phorj (manual; C:DEC-013).
- **LI-H3** Delete 2 dangling branches [Verified present]: `worktree-agent-a2764d080140ece46`, `worktree-agent-af24cab61b7b26f18` (m3-global-review TODO#2).
- **LI-H4** Doc-reconciliation batch (adjudication Q4): C-1 D-L3 text, C-3 zero-dep framing, C-5 ternary perimeter record, C-7 CLI-verb drift (`bench`/`disasm`/`fmt`/`lex` in CLAUDE.md/docs), stale MILESTONES headers (M2.5/M3 "IN PROGRESS"; M6 W3 "superseded green-threads" note now itself superseded by DEC-132), superglobal→Request doc map, **plus the dangling plan citations created by this deletion batch** (18 in CLAUDE.md, 11 in MILESTONES.md, 13 in CHANGELOG.md — counts verified).
- **LI-H5** trackB Task 6: realworld file-reading example + per-module example-coverage audit [Verified: no realworld program reads Core.File].
- **LI-H6** Confirm C-4 (Core.Text→Core.String shadowing rationale consciously dismissed) during adjudication.

**Inventory size: 56 deduplicated items (≈120 with sub-items).**

---

# 6. Deletion command (for developer approval — NOT executed)

48 files. Note: git-tracked (`git rm`), and LI-H4 must follow (dangling citations in
CLAUDE.md/MILESTONES/CHANGELOG — precedent already exists for 7 previously-deleted plans).

```bash
cd /stack/projects/phorj && git rm \
  docs/plans/2026-06-15-m1-plan1-scaffold-lexer.md \
  docs/plans/2026-06-15-m1-plan2-parser-expressions.md \
  docs/plans/2026-06-15-m1-plan3-statements-declarations.md \
  docs/plans/2026-06-15-m1-plan4-typechecker.md \
  docs/plans/2026-06-15-m1-plan5-evaluator.md \
  docs/plans/2026-06-15-m1-plan6-cli.md \
  docs/plans/2026-06-15-m1-polish.md \
  docs/plans/2026-06-15-m2-plan1-vm-core.md \
  docs/plans/2026-06-15-m2-plan2-compiler-runvm.md \
  docs/plans/2026-06-15-m2-plan3-functions-callframes.md \
  docs/plans/2026-06-15-transpile-php.md \
  docs/plans/2026-06-16-examples-coverage.md \
  docs/plans/2026-06-16-m2-p3.5-hardening-roadmap.md \
  docs/plans/2026-06-16-m2-p4-classes-enums-match.md \
  docs/plans/2026-06-16-m2-p5a-rc-shared-heap.md \
  docs/plans/2026-06-16-m2-wave4-compiler-types.md \
  docs/plans/2026-06-16-m2.5-phase1-build-linux-gnu.md \
  docs/plans/2026-06-16-m2.5-phase2-cross-os.md \
  docs/plans/2026-06-17-examples-backfill.md \
  docs/plans/2026-06-17-m3-s0-dx.md \
  docs/plans/2026-06-17-m3-s1-ergonomics.md \
  docs/plans/2026-06-17-m3-s2-null-safety.md \
  docs/plans/2026-06-17-v0.4.0-profiling-disasm.md \
  docs/plans/2026-06-18-m3-s3-lambdas-pipe.md \
  docs/plans/2026-06-18-m5-modules-packages.md \
  docs/plans/2026-06-18-m6-web-capabilities-research.md \
  docs/plans/2026-06-18-m8-php-import-design.md \
  docs/plans/2026-06-19-m7-correctness-closure.plan.md \
  docs/plans/2026-06-21-error-handling-and-traces.plan.md \
  docs/plans/2026-06-21-ga-direction-and-autonomy.plan.md \
  docs/plans/2026-06-21-php-parity-review.plan.md \
  docs/plans/2026-06-21-roadmap-completeness-review.plan.md \
  docs/plans/2026-06-21-stack-traces-impl.plan.md \
  docs/plans/2026-06-24-new-const-fieldinit.plan.md \
  docs/plans/2026-06-24-playground-wasm.plan.md \
  docs/plans/2026-06-25-m-lift-php-to-phorj.plan.md \
  docs/plans/2026-06-25-overnight-autonomous-session.plan.md \
  docs/plans/2026-06-25-transpile-modernization.plan.md \
  docs/plans/2026-06-26-autonomous-backlog.plan.md \
  docs/plans/2026-06-26-default-parameters.plan.md \
  docs/plans/2026-06-26-m-num-decimal.plan.md \
  docs/plans/2026-06-26-m4-stdlib-breadth.plan.md \
  docs/plans/2026-06-26-native-modules-research.plan.md \
  docs/plans/2026-06-26-retire-var-declaration-reshape.plan.md \
  docs/plans/2026-06-27-as-primitives-matrix.plan.md \
  docs/plans/2026-06-28-ga-marathon-super-overloading.plan.md \
  docs/plans/2026-06-29-big-marathon-crosspkg-soundness-stdlib-concurrency.plan.md \
  docs/plans/2026-07-01-m-dx-error-experience.plan.md
```

> After the master plan lands (all §5 items absorbed), a second smaller batch becomes deletable:
> the MERGE plans whose only live content was carried (notably M-3 language-evolution, M-5
> php-fidelity, M-13 review-pass, M-14 trackB, M-15 big-chunk) and the two KEEP-AS-RECORD files once
> adjudication R2-A/R2-B closes.

---

## Final counts

- **DELETE-VERIFIED: 48** (the command above — safe now).
- **MERGE: 15** plans → 56 deduplicated live items (≈120 with sub-items) inventoried in §5.
- **KEEP-AS-RECORD: 2** (m-rt-rich-types, overnight-design-forks) — deletable after adjudication R2-A/R2-B + CLAUDE.md rewrite.
- **KEEP-ACTIVE: 1** (full-audit).
- Post-deletion follow-up: LI-H4 dangling-citation cleanup (42 plan-path citations across CLAUDE.md/MILESTONES/CHANGELOG counted).
