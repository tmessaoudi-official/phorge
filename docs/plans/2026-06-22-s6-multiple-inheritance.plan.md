# S6 Multiple Inheritance Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add explicit-resolution multiple inheritance (`class C extends A, B`) to Phorge, lowered to PHP via interface+trait decomposition, byte-identical across interpreter, VM, and real PHP 8.4.

**Architecture:** Front-end-only. All composition, collision detection, resolution, and flattening happen in the checker/loader **before any backend runs**, so the backends consume a single resolved target per `(class, member)` — **no new `Op`, no `Value` change**. The subtyping oracle (`ast::class_implements`, today interfaces-only) generalizes to a transitive `class_supertypes` closure threaded through `Ty::assignable_with`. The transpiler emits plain `extends` for one parent and interface+trait decomposition for multiple parents. Decomposed into three independently-green sub-slices S6a → S6b → S6c.

**Tech Stack:** Rust (edition 2021, std-only, no external crates). Test harnesses: `cargo test` (lib unit tests + `tests/differential.rs` byte-identity oracle + `tests/integration.rs`). PHP oracle: php-8.4.22.

**Spec:** `docs/specs/2026-06-22-s6-multiple-inheritance-design.md` (read it — it carries the full Decisions Log and the research basis in `docs/research/s6-mi/raw/`).

## Global Constraints

- **PHP transpile floor = 8.4.** Run the gate with `PHORGE_REQUIRE_PHP=1 PHORGE_PHP=/stack/tools/phpbrew/php/php-8.4.22/bin/php` before any commit — the local hook's php-master is too permissive. — [[php-transpile-floor-84]]
- **No new `Op` variant, no `Value` change** (front-end-only; structural byte-identity).
- **`run ≡ runvm ≡ real PHP 8.4` byte-identical** for every example; `tests/differential.rs` globs `examples/**/*.phg` and (for project dirs) every `phorge.toml` root.
- **Examples ship with the feature:** each sub-slice lands a runnable `examples/guide/inheritance*.phg` + an `examples/README.md` row, in the same commit. — [[examples-ship-with-features]]
- **Quality gate:** `cargo clippy --all-targets -- -D warnings` (the pre-commit hook is stricter than `--all-targets` alone) + `cargo fmt --check` clean before every commit. — [[mutation-milestone]] gotcha.
- **Git autonomy:** commit each green slice (`feat(lang):`/`docs:` prefix, no `Co-Authored-By`); never `git push`.
- **Op/match coupling reminder:** even though no new `Op` is planned, adding an AST field (e.g. `ClassDecl.extends`, `ClassDecl.open`) or a `Modifier` variant breaks ~10–12 exhaustive matches (loader, checker collect/check/rewrite passes/casing-walk/this-walk/erase_generics/alias, all four backends) — Rust will keep the build red until every arm is added in the same commit. — [[op-variant-match-coupling]], [[mutation-milestone]].

---

## Progress

- **S6a COMPLETE** (`3b31ecd`→`c5e1a4c`): open modifier + final-keyword retired (S6a.1); `ClassDecl.extends`+`open` parse (S6a.2); `class_supertypes` oracle + member inheritance + `E-EXTEND-FINAL`/`-UNKNOWN`/`-MI-CYCLE` (S6a.3); method override + `E-OVERRIDE-FINAL` + parent-chain dispatch (S6a.4); transpile `extends`/`final class` + `examples/guide/inheritance.phg` (S6a.5). 779 tests green on the PHP-8.4 floor, byte-identical run≡runvm≡real PHP. **Method-level PHP `final` deferred** (not needed for byte-identity; checker enforces override-finality). **Override signature-variance check deferred** (KNOWN_ISSUES).
- **S6b COMPLETE** (`8aa9181`→`4313f7b`): multi-parent compose via the shared `ast::class_method_origins`
  dispatch table — the single source both backends consume, closing the latent interp-first-parent-only
  vs compiler-BFS-all-parents divergence (S6b.1); resolution clauses `use`/`rename`/`exclude` + diamond
  auto-merge + `E-MI-CONFLICT` (S6b.2); `abstract` classes & methods + `E-ABSTRACT-INSTANTIATE`/`-UNIMPL`/
  `E-OPEN-STATIC` (S6b.3); transpiler interface+trait decomposition with `insteadof`/`as` +
  `examples/guide/inheritance-multi.phg` (S6b.4). 649 lib + PHP-8.4-oracle differential + integration
  green, byte-identical run≡runvm≡real PHP 8.4. **`E-MI-SUPER-AMBIGUOUS` is N/A** — the language has no
  `super`/`parent` construct (inherited methods dispatch via `this.m()`); the reservation lands with that
  feature. **Verified gap fixed mid-slice:** a `rename P.m as n` alias needed (a) checker exposure of the
  new name on the child's `ClassInfo.methods` and (b) a compiler name-pool entry — without both, `child.n()`
  type-checked-then-VM-`unknown field` (a real break the vacuous `agree()` had masked; caught by the
  rename PHP-oracle test). **Deferred to S6c:** type/`instanceof` references to a *decomposed ancestor*
  rewriting to its interface form (full subtyping across the lattice); MI field/constructor composition.
- **S6c — IN PROGRESS** (field/ctor composition + diamond + full subtyping/`instanceof` across the lattice).
  - **S6c.1 COMPLETE** (`5a78e0c`): `ast::class_field_conflicts` (field analog of `class_method_origins`,
    minus resolution clauses — PHP has no `insteadof` for properties) → `E-MI-FIELD-CONFLICT` on a
    same-named instance field inherited from ≥2 distinct origins; diamond-shared field auto-merges (dedup
    by declaring origin). Self-documented via `phg explain`. 800 tests green on the PHP-8.4 floor.

### S6c.2 Decisions Log

- [2026-06-23] AGREED (Claude recommended, developer delegated "what do you recommend + continue"):
  **constructor model = Option A (implicit synthesized orchestrating constructor)**, scoped to the
  *no-own-constructor* case. A class with parents and **no own ctor** gets a synthesized constructor whose
  params are the parents' ctor params concatenated in `extends`-list (linearization) order, and whose body
  promotes each parent's promoted params + runs each parent's ctor body, in order. Built as a **front-end
  synthesis pass** (synthesize a real `ClassMember::Constructor` before any backend, like
  `erase_generics`/`expand_aliases`) ⇒ **no new `Op`, no `Value` change**, byte-identical by construction.
  - **Rejected — Option B (explicit per-parent init call, e.g. `Named(n)`):** the natural spelling
    *collides with construction* (`Named(n)` already builds a `Named`), and `super`/`parent` is not
    tokenized (S6b: "the language has no super/parent construct"). B is the right *future ergonomic layer*
    but needs its own syntax-design slice; it becomes the additive follow-up for the deferred case below.
  - **Rejected — Option C (promotion-only strict):** strictly dominated by A (same deferral, but C can't
    run parent ctor bodies and needs a restriction diagnostic A doesn't).
  - **Deferred → KNOWN_ISSUES:** a child declaring **its own** constructor *under inheritance* (single or
    multi) — there is no `super`/parent-forwarding mechanism yet, so it can't initialize inherited state.
    This is exactly the case B solves; punted to the B follow-up. S6c.2 ships the no-own-ctor orchestrator.
  - **De-risked by S6c.1:** cross-parent same-named promoted fields are already `E-MI-FIELD-CONFLICT`, so
    the concatenated synthesized params carry distinct field names (bounds the "implicit positional" critique).

- **S6c.2a COMPLETE** (`4e97018`): single-parent constructor inheritance. `ast::effective_ctor`
  (own-else-nearest-single-parent) is the single decision, consumed by the compiler (instance descriptor +
  synthetic ctor + arity) and mirrored by the interpreter's `effective_ctor_parts`; the checker inherits
  the ctor signature into a no-own-ctor class (`ClassInfo.has_ctor` distinguishes a zero-arg ctor from
  none). Transpiler unchanged — PHP inherits the parent ctor natively. Non-vacuous `agree_out_php` helper
  added (asserts exact output across run/runvm/PHP, not a shared-failure `agree`). 802 tests green, 8.4 floor.

- **S6c.2b — NOT STARTED — design fork surfaced (needs decision before build):** the multi-parent
  orchestrating constructor. Checker (concat parent ctor sigs) + interpreter (run each parent's effective
  ctor with a sliced arg range) are tractable. The fork is the **compiler + transpiler** lowering of
  *multiple parent ctor bodies into one construction*:
  - **Inline-bodies (in C's synthetic ctor):** concat all parents' params, run each parent body in
    sequence. Cheaper, but must isolate each body in its own scope AND scope each body's `return` to the
    end of *that* body (not the ctor epilogue) — else an early `return` in parent A's body skips parent
    B's init (the interpreter runs them as separate calls, so it would NOT skip — a byte-identity break).
    Fiddly but localized to `compile_constructor`; transpiler inlines bodies into one PHP `__construct`,
    decomposed parent traits emit fields-only (no `__construct`, stripping promotion), concrete parent
    classes keep their own `__construct` so direct `new Parent(...)` still works.
  - **Init-functions (spec's "uniquely-named init method"):** each parent ctor → a callable that inits an
    *existing* instance; C's ctor calls each. Matches the interpreter's separate-call semantics for free
    (no return/scope-leak trap), but adds new synthetic functions + dispatch in the compiler and
    `__phorge_init_<Parent>` methods in the transpiler. More machinery, more byte-safe by construction.
  - **Diamond-with-ctor:** a shared base reached through two arms has its ctor run **twice** (once per
    arm) under either approach — but identically on all three backends, so byte-identity holds; the
    double-run is harmless for the immutable field-set. (Documented, not special-cased.)
  - Claude's lean (revised): **init-functions** — byte-safe by construction (each parent body gets its
    own frame, so the early-return/scope-leak trap of the inline approach cannot occur); the project's
    spine is byte-identity, so the extra machinery (synthetic init fns + `__phorge_init_*` methods) is
    worth eliminating the whole trap class rather than defending against it. Awaiting go.

- **S6c.3 COMPLETE** (`f682531`) — done *before* S6c.2b (correctness-before-feature; self-contained;
  prerequisite the eventual 2b "with state + instanceof" guide needs): `instanceof`/subtyping across the
  full lattice. **Root bug found:** the runtime oracle (`class_implements`) held only interfaces, so
  `d instanceof Animal` against a *parent class* was wrongly `false` on both Rust backends — single AND
  multi inheritance, latent because no example tested it; the transpiler also emitted `instanceof Swimmer`
  (concrete) where a multi-parent subtype only `implements ISwimmer`. Fix: `ast::instanceof_table` =
  `class_supertypes` ∪ `class_implements`, the single oracle for `instanceof` + match type-patterns +
  overload subtyping on both backends (a `Dog` now correctly matches an `Animal` param); transpiler
  `type_pos_ref` emits the interface form for a decomposed ancestor in type positions only
  (construction/`extends` stay concrete). `examples/guide/inheritance-lattice.phg` (single + multi +
  diamond) byte-identical run≡runvm≡real PHP 8.4. 803 tests green; no new `Op`.

- **S6c.2b COMPLETE** (`ffc9ff4`) — multi-parent orchestrating constructor; **S6c is now done and M-RT
  S6 (multiple inheritance) is CLOSED.** A class with ≥2 parents and no own ctor gets a synthesized
  orchestrating ctor: params = parents' ctor params concatenated in `extends` order; constructing it runs
  each parent's ctor (its arg slice) on the one instance, initializing every inherited field (parent ctor
  may have a body). `ast::ctor_plan` (replaced `effective_ctor`) is the shared ordered plan — own, else
  single-parent's, else every parent's concatenated. Compiler: descriptor + synth ctor load all promoted
  params → `MakeInstance`, then each entry's body in its own return-jump set (early `return` ends only
  that body, matching the interpreter's separate per-parent calls). Interpreter: promote all, then run
  each body with its arg slice. Checker: concatenated signature. Transpiler: a decomposed parent's trait
  emits promoted params as PLAIN public fields + NO `__construct` (two trait `__construct`s = PHP fatal);
  the concrete class and the multi-parent subclass each get an explicit-assignment `__construct`
  (`emit_class_members` gained `as_trait`; new `emit_synth_construct`). `examples/guide/inheritance-state.phg`
  byte-identical run≡runvm≡real PHP 8.4. No new `Op`, no `Value`. 804 tests green. Deferred → KNOWN_ISSUES:
  a child with its own ctor under inheritance (the `super`-replacement follow-up); cross-parent
  non-promoted param name collision.

**S6 MULTIPLE INHERITANCE COMPLETE** — S6a (single `extends`/override/`open`-final) + S6b (multi-parent
compose/resolution/`abstract`) + S6c (field conflict, ctor composition single+multi, instanceof across
the lattice). Next M-RT: S8 traits; error-model Slice 2 in parallel.

## Sub-slice S6a — single `extends` + override + the `open`/`final` model

**Deliverable:** `open class A {…}  class B extends A {…}` with method override; `open` opt-in; `final` keyword retired; single-parent `super(...)`/`parent` works; subtyping + `instanceof` against the parent chain. One parent only (multi-parent is `E-…` deferred to S6b). Byte-identical run≡runvm≡PHP.

### Task S6a.1: `open` token + retire `final` keyword

**Files:**
- Modify: `src/token.rs:30` (`Final` variant), `src/lexer.rs:367` (`"final" => Final`)
- Modify: `src/ast.rs:575` (`Modifier::Final`), `src/parser.rs:1709` (`TokenKind::Final => Modifier::Final`)
- Test: `src/parser.rs` (inline `#[cfg(test)]`)

**Interfaces:**
- Produces: `TokenKind::Open`; `Modifier::Open`; lexer maps `"open" => Open`.

- [ ] **Step 1: Write the failing test** — in `src/parser.rs` tests, assert `open` lexes/parses as a modifier and `final` is no longer a keyword (it lexes as a bare identifier).

```rust
#[test]
fn open_is_a_modifier_and_final_is_retired() {
    // `open` parses as a member modifier
    let p = parse_ok("open class A { open function f() -> int => 1 }");
    // (assert the method carries Modifier::Open — exact accessor per current test helpers)
    // `final` is now an ordinary identifier, NOT a keyword:
    let toks = lex_ok("final");
    assert!(matches!(toks[0].kind, TokenKind::Ident(_)));
}
```

- [ ] **Step 2: Run to verify it fails** — `cargo test -p phorge open_is_a_modifier`. Expected: FAIL (no `TokenKind::Open`).
- [ ] **Step 3: Implement** — add `Open` to `TokenKind`; lexer `"open" => Open`; **remove** `Final` from `TokenKind`, the `"final" => Final` lexer arm, `Modifier::Final`, and the `TokenKind::Final => Modifier::Final` parser arm. Add `TokenKind::Open => Modifier::Open` to `parse_modifiers` (`src/parser.rs:1701`). (`Final` is parsed-but-never-enforced today — no checker/backend reads `Modifier::Final`, verified by grep — so removal is clean.)
- [ ] **Step 4: Run** — `cargo build` (exhaustive `Modifier` matches now must drop the `Final` arm / add `Open`) then `cargo test -p phorge open_is_a_modifier`. Expected: PASS.
- [ ] **Step 5: Commit** — `git add -A && git commit -m "feat(lang): add 'open' modifier, retire the 'final' keyword (S6a.1)"`

### Task S6a.2: `ClassDecl.extends` + `open` flag + parser

**Files:**
- Modify: `src/ast.rs:798-815` (add `extends: Vec<String>` and `open: bool` to `ClassDecl`)
- Modify: `src/parser.rs:1496-1521` (`parse_class`) + the top-level item parse site that builds a class (to consume an `open` prefix before `class`)
- Test: `src/parser.rs` tests

**Interfaces:**
- Produces: `ClassDecl { vis, name, type_params, extends: Vec<String>, open: bool, implements, members, span }`.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn parses_open_class_with_single_extends() {
    let prog = parse_ok("open class Animal {}  class Dog extends Animal {}");
    // Animal.open == true, Animal.extends == []
    // Dog.open == false, Dog.extends == ["Animal"]
    // (assert via the program's class items — exact path per current helpers)
}
```

- [ ] **Step 2: Run to verify it fails** — Expected: FAIL (no `extends` field).
- [ ] **Step 3: Implement** — add the two fields to `ClassDecl`. In `parse_class`, after `type_params` and before `implements`, parse `let extends = if self.eat(&TokenKind::Extends) { self.parse_name_list("a class name after 'extends'")? } else { Vec::new() };` (reuse `parse_name_list`, parser.rs:1584). Thread an `open: bool` from the item-level prefix (parse `open` before the `class` keyword at the item dispatch site; default `false`). Update **every** `ClassDecl { … }` literal (parser + any test fixtures) to set the new fields. Fix the ~10 exhaustive matches that destructure `ClassDecl` (loader, checker collect/casing/this-walk/erase/alias, transpiler) — most just need to ignore the new fields with `..` or add a passthrough.
- [ ] **Step 4: Run** — `cargo build` then the test. Expected: PASS.
- [ ] **Step 5: Commit** — `git add -A && git commit -m "feat(lang): ClassDecl.extends + open flag, parse single extends (S6a.2)"`

### Task S6a.3: `class_supertypes` oracle + subtyping

**Files:**
- Modify: `src/ast.rs` (near `class_implements`, ~293) — add `pub fn class_supertypes(program) -> BTreeMap<String, Vec<String>>` (transitive, cycle-checked); extend `class_implements` so a class inherits its parents' interfaces transitively.
- Modify: `src/types.rs` (`assignable_with`, ~162) — the subtype oracle consults class supertypes.
- Modify: `src/checker.rs` — store the supertype closure on `Checker`; emit `E-EXTEND-FINAL` (parent not `open`), `E-MI-CYCLE` (cycle), `E-EXTEND-UNKNOWN` (parent isn't a class).
- Test: `src/checker.rs` tests + `tests/integration.rs`

**Interfaces:**
- Consumes: `ClassDecl.extends`, `ClassDecl.open`.
- Produces: `ast::class_supertypes`; `Checker.class_supertypes: BTreeMap<String, Vec<String>>`; subtyping edge `Dog <: Animal`.

- [ ] **Step 1: Write the failing test** — a `Dog extends Animal` instance flows into an `Animal`-typed local; extending a non-`open` class errors `E-EXTEND-FINAL`; a 2-cycle errors `E-MI-CYCLE`.

```rust
#[test]
fn subclass_is_assignable_to_superclass() {
    check_ok("open class Animal { function name() -> string => \"a\" } \
              class Dog extends Animal {} \
              function f() -> string { Animal a = Dog(); return a.name(); }");
}
#[test]
fn extending_a_non_open_class_errors() {
    let d = check_err("class Animal {} class Dog extends Animal {}");
    assert_eq!(d.code, Some("E-EXTEND-FINAL"));
}
```

- [ ] **Step 2: Run to verify it fails** — Expected: FAIL.
- [ ] **Step 3: Implement** — `class_supertypes` walks `extends` transitively with a visited-set cycle guard (mirror the `class_implements` closure at ast.rs:304-316). Thread the result into `assignable_with`'s `Ty::Named` subtype oracle (today checks interfaces only). In `collect_class`/`check`, validate each `extends` name is a known **class** that is `open`. Inherit the parent's fields/methods into the child's `ClassInfo` (so `a.name()` resolves). Add `phg explain` entries for the new codes (`src/cli.rs`).
- [ ] **Step 4: Run** — `cargo test -p phorge subclass_is_assignable` + `extending_a_non_open`. Expected: PASS.
- [ ] **Step 5: Commit** — `git add -A && git commit -m "feat(lang): class supertype oracle + E-EXTEND-FINAL/-CYCLE (S6a.3)"`

### Task S6a.4: method override + `E-OVERRIDE-FINAL` + interpreter/VM parent-chain dispatch

**Files:**
- Modify: `src/checker.rs` — when a child method name matches a parent's, require the parent method `open` (`E-OVERRIDE-FINAL`); validate signature (exact params, covariant-or-equal return).
- Modify: `src/interpreter.rs` (`call_method`, ~1387) — on miss, walk the `extends` chain.
- Modify: `src/compiler.rs` — pre-flatten inherited methods into `BytecodeProgram.methods`/`method_overloads`.
- Test: `src/checker.rs` tests + `tests/differential.rs` (a run≡runvm case).

**Interfaces:**
- Consumes: supertype oracle (S6a.3).
- Produces: override semantics; flat method table including inherited methods.

- [ ] **Step 1: Write the failing test** — child overrides an `open` parent method; overriding a non-`open` method errors; inherited (non-overridden) method dispatches.

```rust
#[test]
fn override_open_method_dispatches_to_child() {
    // run≡runvm: Dog.speak() overrides open Animal.speak()
}
#[test]
fn overriding_a_final_method_errors() {
    let d = check_err("open class A { function f() -> int => 1 } \
                       class B extends A { function f() -> int => 2 }");
    assert_eq!(d.code, Some("E-OVERRIDE-FINAL")); // A.f is final-by-default
}
```

- [ ] **Step 2: Run to verify it fails** — Expected: FAIL.
- [ ] **Step 3: Implement** — checker override validation; interpreter parent-chain method lookup; compiler pre-flatten (inherited method → `methods[(Child, name)] = parent_fn_idx`). `super`/`parent` in a single-parent class resolves to the one parent (normal PHP `parent::`).
- [ ] **Step 4: Run** — the new tests + `cargo test`. Expected: PASS.
- [ ] **Step 5: Commit** — `git add -A && git commit -m "feat(lang): method override + E-OVERRIDE-FINAL, parent-chain dispatch (S6a.4)"`

### Task S6a.5: transpiler `extends` + `final class` + guide example

**Files:**
- Modify: `src/transpile.rs` (`emit_class`, ~715-760) — emit `class C extends Parent`; non-`open` class → `final class`; non-`open` method → PHP `final` method.
- Create: `examples/guide/inheritance.phg` (single inheritance + override + `open`).
- Modify: `examples/README.md` (index + coverage row).
- Test: `tests/differential.rs` picks up the example automatically (glob).

- [ ] **Step 1: Write the failing test** — add `examples/guide/inheritance.phg`; the differential glob now asserts run≡runvm≡PHP. Run `PHORGE_REQUIRE_PHP=1 PHORGE_PHP=…/php-8.4.22 cargo test --test differential`. Expected: FAIL (transpiler still ignores `extends`).
- [ ] **Step 2: Implement** — `emit_class` emits the `extends` clause + `final`/non-`final` per the `open` flag. Single parent only this slice.
- [ ] **Step 3: Run the floor oracle** — `PHORGE_REQUIRE_PHP=1 PHORGE_PHP=/stack/tools/phpbrew/php/php-8.4.22/bin/php cargo test`. Expected: PASS (all backends byte-identical).
- [ ] **Step 4: clippy + fmt** — `cargo clippy --all-targets -- -D warnings && cargo fmt --check`.
- [ ] **Step 5: Commit** — `git add -A && git commit -m "feat(lang): transpile single extends + final class + inheritance example (S6a.5)"`

---

## Sub-slice S6b — multi-parent compose + resolution clauses + `abstract`

**Deliverable:** `class C extends A, B`; cross-parent method collisions are `E-MI-CONFLICT` until resolved via `use P.m` / `rename P.m as n` / `exclude P.m` / override; `abstract` classes & methods; `E-MI-SUPER-AMBIGUOUS` reserves `super`/`parent` under multi-parent. Transpiler interface+trait decomposition with `insteadof`/`as`.

### Tasks (each a green, byte-identical commit — expanded to bite-sized steps at execution)

- **S6b.1 — multi-parent parse + compose.** Allow ≥2 names in `extends`; checker merges all parents' members; a diamond shared base auto-merges only on byte-identical members. Test: `class Duck extends Swimmer, Flyer` composes both `move`s into a conflict set (no resolution yet → next task errors). Files: `src/parser.rs` (already multi via `parse_name_list`), `src/checker.rs` (merge loop), `src/ast.rs` (no change). Acceptance: parses + composes; run≡runvm on a no-collision multi-parent program.
- **S6b.2 — `E-MI-CONFLICT` + resolution clauses.** Parse `use P.m` / `rename P.m as n` / `exclude P.m` in the class body (new `ClassMember`-adjacent resolution list, or a `ClassDecl.resolutions: Vec<Resolution>` field). Unresolved collision → `E-MI-CONFLICT`. Resolved → the checker rewrites to a single concrete target per name. Files: `src/parser.rs`, `src/ast.rs`, `src/checker.rs`. Tests: each clause; the error. Acceptance: the diamond example resolves and runs run≡runvm.
- **S6b.3 — `abstract` classes & methods.** `abstract` modifier (new `Modifier::Abstract` + token); abstract class can't be instantiated (`E-ABSTRACT-INSTANTIATE`); a concrete subclass must implement every abstract method (`E-ABSTRACT-UNIMPL`); abstract method is implicitly `open`; `open` on `static` → error. Files: `src/token.rs`, `src/lexer.rs`, `src/ast.rs`, `src/parser.rs`, `src/checker.rs`. Tests: both errors + a concrete impl. Acceptance: run≡runvm.
- **S6b.4 — `E-MI-SUPER-AMBIGUOUS` + transpiler decomposition.** `super`/`parent` under ≥2 parents → error. Transpiler: each parent → interface `I<Name>` + trait `T<Name>`; `class C extends A,B` → `class C implements IA,IB { use TA,TB { …insteadof/as… } }`; resolution clauses → `insteadof`/`as`. Files: `src/checker.rs`, `src/transpile.rs`. Create `examples/guide/inheritance-multi.phg` (the diamond, explicitly resolved). Acceptance: `PHORGE_REQUIRE_PHP=1` floor oracle byte-identical run≡runvm≡PHP; clippy+fmt clean.

---

## Sub-slice S6c — field/ctor composition + diamond + full subtyping

**Deliverable:** field-collision detection; synthesized orchestrating constructor; diamond auto-merge of byte-identical members; full `instanceof`/assignability against every ancestor with smart-cast.

### Tasks (expanded at execution)

- **S6c.1 — `E-MI-FIELD-CONFLICT`.** Same-named field from ≥2 parents → error (PHP has no `insteadof` for properties). Resolve by parent rename or child redeclare. Files: `src/checker.rs`. Test: the error + a resolved case.
- **S6c.2 — synthesized orchestrating constructor.** Each parent ctor → a uniquely-named init method; `C`'s synthesized ctor calls each in `extends`-list order, then `C`'s own ctor body. Files: `src/checker.rs` (compose ctor params/order), `src/interpreter.rs`, `src/compiler.rs`, `src/transpile.rs` (emit the init-method pattern). Test: a multi-parent class with state initializes all parent fields; run≡runvm≡PHP.
- **S6c.3 — diamond + `instanceof` across the lattice + guide example.** Diamond shared base auto-merge confirmed; `instanceof` + smart-cast against any ancestor/interface. Create `examples/guide/inheritance-state.phg` (multi-parent with fields + `instanceof`). Acceptance: floor oracle byte-identical; clippy+fmt clean.

---

## Open sub-questions to resolve at S6a/S6b start (flag to developer, don't assume)

- **`open`/`final`-keyword retirement blast radius:** confirm no shipped example or test program uses `final` as a *modifier* (grep before S6a.1). If any do, they migrate in the same commit.
- **Where `open` attaches on a class:** as an item-level prefix (`open class`) vs a `Modifier` on the decl — S6a.2 chooses item-level prefix + `ClassDecl.open: bool`. Confirm fits the existing item-parse dispatch.
- **Override return variance:** S6 requires exact-or-covariant return, exact params (contravariant params deferred — KNOWN_ISSUES).

## Self-Review (against the spec)

- **Spec coverage:** §Syntax→S6a.2/S6b.1; §Composition→S6b.1; §Collision (method)→S6b.2; §Field collision→S6c.1; §Constructors→S6c.2; §Subtyping/instanceof→S6a.3/S6c.3; §super reservation→S6b.4; §open/final→S6a.1/S6a.3/S6a.4; §Lowering→S6a.5/S6b.4/S6c.2; §Diagnostics→each task adds the code + `phg explain`; §Deferrals→KNOWN_ISSUES at S6b.4/S6c.3 commits. No spec section is unmapped.
- **Placeholder scan:** S6a is bite-sized with concrete tests; S6b/S6c are task-level by deliberate design (their exact code depends on S6a's resulting shapes) — each is expanded to bite-sized steps when reached, per the repo's established plan convention.
- **Type consistency:** `class_supertypes`, `ClassDecl.extends`/`open`, `Modifier::Open`, the `E-*` codes are used consistently across tasks.

## Acceptance (whole slice)

Each sub-slice: byte-identical `run ≡ runvm ≡ real PHP 8.4` for its guide example; full lib + PHP-oracle differential + integration suite green on the PHP-8.4 floor; clippy `-D warnings` + fmt clean; **no new `Op`**; every new diagnostic documented by `phg explain`. On S6 completion: update `CHANGELOG.md`, `KNOWN_ISSUES.md`, `docs/MILESTONES.md`, the `CLAUDE.md` Active-plan block (developer applies — classifier-blocked for the agent), and the M-RT progress memory.

## Rollback

Each sub-slice (and each task within) is an isolated commit; `git revert` the offending commit. S6a.3 (subtype oracle generalization) + S6a.1 (`final` retirement) are the broad changes; reverting restores the interface-only oracle and the `final` keyword.
