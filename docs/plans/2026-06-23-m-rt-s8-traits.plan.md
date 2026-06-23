# M-RT S8 Traits Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended)
> or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Add user-facing traits (`trait` declaration + `use` composition) — horizontal code reuse that is
NOT a type — reusing the S6 MI→PHP trait/use lowering, with the maximal member set (methods, state,
constructors, hooks, const, abstract requirements) and every PHP footgun converted to a clean ahead-of-time
diagnostic.

**Architecture:** Front-end flatten, mirroring `merge_inherited` (checker.rs:1014) and `erase_generics`.
Parse `Item::Trait` + `ClassDecl.uses`; a new `flatten_traits` pass copies trait members into each using
class **before any backend runs**, so the interpreter, VM, and the rest of the checker see plain complete
classes (trait calls become ordinary method calls; a trait ctor folds into the existing `ctor_plan`,
ast.rs:750). Only the **transpiler** keeps the trait distinct and emits native PHP `trait T {…}` + `use T;`,
reusing `emit_class_members(..., as_trait)` (transpile.rs:867) and `build_trait_clauses` (transpile.rs:1212).

**Tech Stack:** Rust 2021 (std-only), the three Phorge backends (interpreter `src/interpreter.rs`, stack VM
`src/vm.rs`+`src/compiler.rs`, transpiler `src/transpile.rs`), the differential harness
`tests/differential.rs` (PHP oracle).

## Global Constraints

- **Byte-identical spine:** `run ≡ runvm ≡ real PHP`, gated by `tests/differential.rs`. Every example under
  `examples/**/*.phg` is auto-globbed and gated — a new example must run byte-identically on all backends.
- **PHP floor = 8.4** (CI pin). The local default `php` is 8.6-dev and **too permissive** — always validate
  with `PHORGE_REQUIRE_PHP=1 PHORGE_PHP=/stack/tools/phpbrew/php/php-8.4.22/bin/php cargo test` before
  declaring a task green. (A separate later milestone may raise this; this slice is version-agnostic.)
- **Expected zero new `Op`** — flatten is front-end; trait members become ordinary class members. If a
  genuine need for an `Op` appears, it extends the three coupled matches (`vm.rs` `exec_op`, `chunk.rs`
  `validate`, `compiler.rs` `stack_effect`) in one commit and this plan is updated.
- **Quality gate per commit:** `cargo test` (PHP-8.4 oracle), `cargo clippy --all-targets -- -D warnings`,
  `cargo fmt --check`. The pre-commit hook runs all three.
- **Toolchain:** `export PATH=/stack/tools/cargo/bin:$PATH`.
- **Naming:** trait names PascalCase (same rule as classes). `package Main`-only this slice.
- **`phg explain`** must document every new diagnostic code (cli.rs `explain_text`, ~line 170).
- **Git autonomy authorized** (add/commit, never push). Commit prefixes `feat:`/`fix:`/`docs:`/`test:`; no
  `Co-Authored-By`.

## Decisions Log

- [2026-06-23] D1–D8 — see the design spec `docs/specs/2026-06-23-m-rt-s8-traits-design.md` Decisions Log
  (reuse-only/not-a-type; maximal member set; footguns→diagnostics; P1/P2 warnings; ctors in-slice).
- [2026-06-23] **D9 (planning-time refinement)** — **`use` is disambiguated by dot-lookahead.** Inside a
  class body the leading `use` ident is already an S6b resolution clause (`use Parent.method`,
  parser.rs:1526-1576). Trait composition reuses the same `use` keyword: after `use <Ident>`, if the next
  token is `.` it is the existing resolution clause; if `,`, `;`, or `{` it is **trait composition**
  (`use Trait[, Trait]* ;`). Trait-vs-trait method collisions reuse the existing `use`/`rename`/`exclude`
  resolution clauses verbatim (a "parent" in a clause may now name a trait). The literal PHP `insteadof`
  keyword is NOT Phorge source syntax — the transpiler emits it.
- [2026-06-23] **D10** — Trait abstract requirements **reuse the existing abstract-method machinery**: a
  trait abstract method is a `FunctionDecl` with the `abstract` modifier + empty body (ast.rs:1032); the
  `flatten_traits` pass registers it in the checker's `abstract_methods[(class,name)]` set (checker.rs:845)
  so the existing unmet-abstract check fires, under a trait-specific code `E-TRAIT-ABSTRACT-UNMET` (clearer
  message than the generic `E-ABSTRACT-UNIMPL`).

## File Structure

| File | Responsibility for this slice |
|---|---|
| `src/ast.rs` | `TraitDecl` struct + `Item::Trait`; `ClassDecl.uses: Vec<UseTrait>`; a `flatten_traits(program)` free function (or trait-member accessors) mirroring the merge pattern; extend `class_method_origins`/field/abstract helpers to count trait-supplied members. |
| `src/parser.rs` | `parse_trait` (clone of `parse_class` body parsing); `Item::Trait` dispatch in `parse_item` (parser.rs:1255); dot-lookahead in the class-body loop (parser.rs:1559) to split `use Trait;` from `use Parent.method`. |
| `src/checker.rs` | Collect traits into a registry; `flatten_traits` (model: `merge_inherited`, checker.rs:1014); `E-USE-UNKNOWN`, `E-TRAIT-ABSTRACT-UNMET`, `E-TRAIT-CTOR-COLLISION`, `W-TRAIT-CTOR-SHADOWED`, `W-TRAIT-CTOR-PARENT-SKIPPED`; reject `instanceof Trait` / typing a var as a trait (`E-INSTANCEOF-TYPE`). |
| `src/interpreter.rs`, `src/vm.rs`, `src/compiler.rs` | **Expected no changes** — they consume the flattened classes. Verified by the T1–T4 checkpoints. |
| `src/transpile.rs` | Emit native `trait <Name> { … }` (reuse `emit_class_members(..., as_trait=true)`); emit `use <Name>;` + resolution clauses in the using class (reuse `build_trait_clauses`). |
| `src/cli.rs` | `phg explain` entries for every new code. |
| `tests/differential.rs` | Trait test cases via `agree` / `agree_out_php` / `agree_err`. |
| `examples/guide/traits.phg` | The shipped example (auto-gated by the glob). |

---

### Task T1 — Parse `trait`/`use` + method flatten + not-a-type

**Files:**
- Modify: `src/ast.rs` (add `TraitDecl`, `Item::Trait`, `ClassDecl.uses`, `flatten_traits`)
- Modify: `src/parser.rs:1255` (`parse_item` dispatch), `:1526` (`parse_class` body loop), add `parse_trait`
- Modify: `src/checker.rs` (trait registry, `flatten_traits` call, `E-USE-UNKNOWN`, `E-TRAIT-ABSTRACT-UNMET`, `instanceof` rejection)
- Modify: `src/transpile.rs` (native `trait`/`use` emission)
- Modify: `src/cli.rs` (`phg explain` for `E-USE-UNKNOWN`, `E-TRAIT-ABSTRACT-UNMET`)
- Test: `tests/differential.rs`, `src/parser.rs` (unit), `src/checker.rs` (unit)

**Interfaces produced (later tasks rely on these exact names):**
- `ast::Item::Trait(TraitDecl)` where `pub struct TraitDecl { pub name: String, pub members: Vec<ClassMember>, pub span: Span }`
- `ast::ClassDecl.uses: Vec<UseTrait>` where `pub struct UseTrait { pub name: String, pub span: Span }` (resolution clauses stay in the existing `ClassDecl.resolutions`)
- `ast::flatten_traits(program: &Program) -> ...` — the trait-member view consumed by the checker (model: `merge_inherited`)

**Key implementation notes:**
- **Parser dot-lookahead (D9):** in the `parse_class` body loop (parser.rs:1559) the current gate is
  `if let TokenKind::Ident(kw) = self.peek() { if matches!(kw.as_str(), "use"|"rename"|"exclude") {...} }`.
  For `use`, peek the token *after* the name: `.` → `parse_resolution` (existing); `,`/`;`/`{` → a new
  `parse_use_trait` that returns `Vec<UseTrait>` appended to `decl.uses`. Keep `rename`/`exclude` routing to
  `parse_resolution` unchanged.
- **`parse_trait`:** clone the class-body member loop (parser.rs:1559-1568) — traits parse the *same*
  `ClassMember` grammar (so visibility, `mutable`, `const`, `abstract`, hooks, ctor all parse for free).
  A trait has no `extends`/`implements`/`type_params` this slice. Dispatch from `parse_item` on
  `TokenKind::Trait`.
- **`flatten_traits` (checker):** mirror `merge_inherited`. For each class, for each `use`d trait, copy the
  trait's methods into the class's `ClassInfo.methods` (skip names the class already declares — class wins);
  a trait abstract method (abstract modifier + empty body) is registered in `abstract_methods[(class,name)]`
  (checker.rs:845) so an unmet requirement raises `E-TRAIT-ABSTRACT-UNMET` (D10). Trait-vs-trait method
  collisions reuse the existing `class_method_origins` conflict path with the `use`/`rename`/`exclude`
  clauses (D9). State + ctors are stubbed/deferred to T2/T3 (a trait with a field or ctor → a clean
  "not yet supported in T1" gate is unnecessary; just don't flatten them yet — T1 tests use method-only
  traits).
- **not-a-type:** where `instanceof`'s RHS is validated and where a type annotation is resolved, a name that
  resolves to a trait → `E-INSTANCEOF-TYPE` (reuse the existing code; extend its `phg explain` text to
  mention traits).
- **Transpiler:** add a `trait` arm to the item-emission match: emit `trait <Name> {` + members via
  `emit_class_members(<synthetic ClassDecl>, …, as_trait=true)` (which already turns promoted params into
  plain fields and omits `__construct`). In a using class, emit `use <Name>;` (+ existing resolution clauses
  via `build_trait_clauses`). User traits need **no** `interface I<name>` (traits aren't types, D2).

- [ ] **Step 1 — failing differential test (method reuse):** add to `tests/differential.rs`:
```rust
#[test]
fn s8_trait_method_reuse_is_byte_identical() {
    agree_out_php(
        r#"import Core.Console;
trait Loud { function shout(string s) -> string { return s; } }
class Crier { use Loud; }
function main() { Console.println(Crier().shout("hi")); }"#,
        "hi\n",
        "s8_trait_method_reuse",
    );
}
```
- [ ] **Step 2 — run, verify it fails** (parser rejects `trait`): `cargo test s8_trait_method_reuse` → FAIL.
- [ ] **Step 3 — implement** the AST additions, `parse_trait`, dot-lookahead, `flatten_traits` (methods),
  transpiler `trait`/`use` emission, per the notes above.
- [ ] **Step 4 — run, verify pass:** `cargo test s8_trait_method_reuse` → PASS.
- [ ] **Step 5 — failing tests for the guards:** add `agree_err`/checker-unit tests for: `use Unknown;` →
  `E-USE-UNKNOWN`; an unmet `abstract function name() -> string;` requirement → `E-TRAIT-ABSTRACT-UNMET`;
  `x instanceof SomeTrait` → `E-INSTANCEOF-TYPE`. Add the parser unit test for dot-lookahead (a class with
  both `use TraitName;` and `use Parent.method` resolves correctly).
- [ ] **Step 6 — implement the guards**, add `phg explain` entries.
- [ ] **Step 7 — run full suite on the 8.4 floor:**
  `PHORGE_REQUIRE_PHP=1 PHORGE_PHP=/stack/tools/phpbrew/php/php-8.4.22/bin/php cargo test` → all green;
  `cargo clippy --all-targets -- -D warnings` + `cargo fmt --check` clean.
- [ ] **Step 8 — checkpoint:** confirm `interpreter.rs`/`vm.rs`/`compiler.rs` were **not** modified (the
  flatten-only proof) — `git diff --name-only` shows none of those three. **Commit:**
  `feat(lang): parse trait/use + flatten trait methods, traits are not types (S8 T1)`.

---

### Task T2 — Trait state (fields, const, visibility, static)

**Files:** `src/ast.rs` (flatten fields/const/static), `src/checker.rs` (`flatten_traits` state arm),
`src/transpile.rs` (already emits fields via `as_trait`), `tests/differential.rs`, `examples` n/a yet.

**Interfaces consumed:** `flatten_traits` from T1. **Produced:** trait state flattened into `ClassInfo`
(`fields`, `mutable_fields`, `statics`, `static_mut`, const) — T3 relies on fields existing before ctors.

**Key implementation notes:**
- Extend `flatten_traits` to copy `ClassMember::Field` (with its `modifiers` — `mutable` carries through to
  `ClassInfo.mutable_fields`, exactly as `merge_inherited` does at checker.rs:1040), `const`, and `static`/
  `static mutable` (per-using-class copy falls out of flatten-into-class — no special handling). Class-declared
  member of the same name wins (skip).
- A trait field collision across two used traits reuses the field-conflict path (`class_field_conflicts`,
  ast.rs:624) → `E-MI-FIELD-CONFLICT` (no `insteadof` for properties, same as MI).
- Transpiler already emits trait fields as plain `public`/typed properties via `as_trait=true`
  (transpile.rs `emit_class_members`) — verify visibility + `mutable`-as-PHP-non-readonly carries through;
  add only if a gap appears.

- [ ] **Step 1 — failing test (immutable + mutable + const state):**
```rust
#[test]
fn s8_trait_state_is_byte_identical() {
    agree_out_php(
        r#"import Core.Console;
trait Counter { const int MAX = 9; mutable int n; function bump() { this.n = this.n + 1; } function read() -> int { return this.n; } }
class C { use Counter; constructor() { this.n = 0; } }
function main() { C c = C(); c.bump(); c.bump(); Console.println("{c.read()} {C.MAX}"); }"#,
        "2 9\n",
        "s8_trait_state",
    );
}
```
  *(If `const` access syntax `C.MAX` differs from the codebase's actual const-read form, match the existing
  const test in `tests/` — check `examples/guide/` for the canonical const read before finalizing.)*
- [ ] **Step 2 — run, verify fail** (trait fields not yet flattened).
- [ ] **Step 3 — implement** the state arm of `flatten_traits`.
- [ ] **Step 4 — run, verify pass.**
- [ ] **Step 5 — failing test:** two used traits declaring the same field → `E-MI-FIELD-CONFLICT` (`agree_err`
  or checker-unit).
- [ ] **Step 6 — implement / confirm** the conflict path covers trait-supplied fields.
- [ ] **Step 7 — full 8.4 suite + clippy + fmt green; confirm backends untouched.**
- [ ] **Step 8 — commit:** `feat(lang): trait state — fields/const/static with visibility + mutability (S8 T2)`.

---

### Task T3 — Trait constructors

**Files:** `src/ast.rs` (`ctor_plan` already at :750 — fold trait ctor in), `src/checker.rs`
(collision/shadow/parent-skip diagnostics + `has_ctor`), `src/interpreter.rs`+`src/compiler.rs` (verify
`ctor_plan` consumption already covers the folded ctor — **expected no change** beyond what `ctor_plan`
returns), `src/transpile.rs` (`emit_synth_construct` already builds from `ctor_plan`), `tests/differential.rs`.

**Interfaces consumed:** flattened state (T2), `ctor_plan` (ast.rs:750). **Produced:** a trait ctor
participates in `ctor_plan` so all three backends construct identically.

**Key implementation notes:**
- A trait ctor is `ClassMember::Constructor` in the trait. Folding rule (matches PHP, verified on 8.4):
  - class has its **own** ctor → class ctor wins; trait ctor dead → `W-TRAIT-CTOR-SHADOWED` (D8).
  - no class ctor, **one** trait ctor → that ctor becomes the class ctor (fold into `ctor_plan`).
  - no class ctor, **≥2** trait ctors unresolved → `E-TRAIT-CTOR-COLLISION` (require a `use`/`rename`
    resolution clause selecting one, reusing the existing resolution mechanism on the `constructor` name).
  - no class ctor, a trait ctor **and** an `extends` parent with a ctor → trait ctor wins, parent ctor not
    auto-run → `W-TRAIT-CTOR-PARENT-SKIPPED` (D6).
- `ctor_plan` already concatenates plan entries; ensure a `use`d trait's ctor is appended at the right
  position (after class-own check). Set `has_ctor` true when a trait supplies the effective ctor so
  inheritance/`merge_inherited` treats it correctly.

- [ ] **Step 1 — failing test (single trait ctor becomes the ctor, PHP-faithful):**
```rust
#[test]
fn s8_trait_constructor_is_byte_identical() {
    agree_out_php(
        r#"import Core.Console;
trait Stamped { public int id; constructor(int id) { this.id = id; } }
class Doc { use Stamped; }
function main() { Doc d = Doc(7); Console.println("{d.id}"); }"#,
        "7\n",
        "s8_trait_ctor",
    );
}
```
- [ ] **Step 2 — run, verify fail.**
- [ ] **Step 3 — implement** trait-ctor folding into `ctor_plan` + the checker bookkeeping.
- [ ] **Step 4 — run, verify pass** (all three backends, incl. real PHP 8.4).
- [ ] **Step 5 — failing tests for the three diagnostics:** two trait ctors → `E-TRAIT-CTOR-COLLISION`;
  class-own ctor + trait ctor → `W-TRAIT-CTOR-SHADOWED` (warning present, build still succeeds);
  `extends Base(ctor)` + trait ctor, no class ctor → `W-TRAIT-CTOR-PARENT-SKIPPED`.
- [ ] **Step 6 — implement** the diagnostics + `phg explain` entries.
- [ ] **Step 7 — full 8.4 suite + clippy + fmt green.**
- [ ] **Step 8 — commit:** `feat(lang): trait constructors fold into ctor_plan + footgun diagnostics (S8 T3)`.

---

### Task T4 — Property hooks in traits

**Files:** `src/checker.rs` (`flatten_traits` hook arm), `src/transpile.rs` (verify hook emission inside a
trait), `tests/differential.rs`.

**Key implementation notes:**
- A trait `ClassMember::Hook` (ast.rs:1219) lowers (M-mut.7b) to synthetic `<Class>::<name>$get`/`$set`
  methods via `Op::CallMethod` (no new Op). The `flatten_traits` pass copies the hook member into the using
  class exactly like a method; the existing hook-lowering then runs over the merged class. Verify the
  compiler's hook detection (the `method_rets` probe + ctype operand trap, M-mut.7b) sees the
  trait-originated hook.
- PHP 8.4 supports hooks in traits (verified) — the transpiler emits the hook inside the native `trait`.

- [ ] **Step 1 — failing test (get hook in a trait):**
```rust
#[test]
fn s8_trait_property_hook_is_byte_identical() {
    agree_out_php(
        r#"import Core.Console;
trait Labeled { string display { get => "<" + this.raw + ">"; } }
class Tag { use Labeled; constructor(public string raw) {} }
function main() { Console.println(Tag("x").display); }"#,
        "<x>\n",
        "s8_trait_hook",
    );
}
```
  *(Confirm the hook + string-concat syntax against `examples/guide` hook examples before finalizing; use the
  canonical concat form the codebase uses.)*
- [ ] **Step 2 — run, verify fail.**
- [ ] **Step 3 — implement** the hook arm of `flatten_traits`.
- [ ] **Step 4 — run, verify pass on all backends + PHP 8.4.**
- [ ] **Step 5 — full 8.4 suite + clippy + fmt green; confirm backends untouched beyond what M-mut.7b needs.**
- [ ] **Step 6 — commit:** `feat(lang): property hooks in traits (S8 T4)`.

---

### Task T5 — Example + docs + housekeeping (closes M-RT)

**Files:**
- Create: `examples/guide/traits.phg`
- Modify: `examples/README.md`, `CHANGELOG.md`, `KNOWN_ISSUES.md`, `docs/MILESTONES.md`,
  `CLAUDE.md` (refresh the stale "NEXT" line), the spec + plan STATUS
- Delete: dead COMPLETE plan files in `docs/plans/` (method-overloading, generic-enums, totality-cluster,
  error-model-slice2, m-mut.*, visibility-modifiers*, etc. — confirm each is COMPLETE first)

**Key implementation notes:**
- `examples/guide/traits.phg` must run byte-identically (auto-gated). Cover: a trait with methods + state +
  a `mutable` field + `const` + an abstract requirement satisfied by the using class + a property hook +
  two-trait composition with a `use Trait.method` resolution clause. Single `Ok` output (no faults — faults
  can't be a runnable example; capture deferrals in KNOWN_ISSUES per the developer rule).
- KNOWN_ISSUES deferrals: traits as types; generic traits; cross-package traits (this slice is
  `package Main`-only); compile-time multi-trait ambiguity beyond ctors.
- `docs/MILESTONES.md`: add "M-RT traits — COMPLETE" and mark **M-RT CLOSED**.

- [ ] **Step 1 — write `examples/guide/traits.phg`**; run `phg run` + `phg runvm` + transpile→PHP 8.4 and
  confirm identical output by hand, then `cargo test` (the glob gates it).
- [ ] **Step 2 — add the `examples/README.md` row + coverage matrix entry.**
- [ ] **Step 3 — CHANGELOG.md + KNOWN_ISSUES.md (deferrals) + docs/MILESTONES.md (M-RT CLOSED).**
- [ ] **Step 4 — refresh stale `CLAUDE.md` "NEXT" line** (method overloading is done; next is S6→done; set
  the new NEXT per the audit, e.g. the PHP-version-targeting milestone / error-model already closed).
- [ ] **Step 5 — prune dead plan files:** for each COMPLETE plan in `docs/plans/`, `git rm` it (verify STATUS
  says COMPLETE first). Include this slice's plan deletion at Phase 8 per Rule 17 (ask first).
- [ ] **Step 6 — full 8.4 suite + clippy + fmt green.**
- [ ] **Step 7 — commit:** `feat(examples): traits guide example + M-RT CLOSED; docs + plan housekeeping (S8 T5)`.

---

## Self-Review

**Spec coverage:** D1 (T1 dispatch) · D2 not-a-type (T1 `E-INSTANCEOF-TYPE`) · D3 visibility+mutability
(T2, free via `ClassMember`) · D4 maximal: ctors (T3), static-mutable (T2), hooks (T4), const (T2), abstract
(T1) · D5 footguns→diagnostics (T1/T3) · D6 `W-TRAIT-CTOR-PARENT-SKIPPED` (T3) · D7 ctors in-slice (T3) · D8
`W-TRAIT-CTOR-SHADOWED` (T3) · D9 dot-lookahead (T1) · D10 abstract reuse (T1). All spec sections map to a
task. Sub-slice order T1→T5 matches the spec.

**Placeholder scan:** the two "*confirm against existing examples*" notes (T2 const-read form, T4 hook/concat
form) are deliberate — the canonical surface syntax must be read from the codebase at execution time, not
guessed; they are bounded ("match the existing X test"), not open TODOs.

**Type consistency:** `TraitDecl`/`UseTrait`/`flatten_traits` names are used identically across tasks;
`ctor_plan` (ast.rs:750), `merge_inherited` (checker.rs:1014), `abstract_methods` (checker.rs:845),
`emit_class_members(..., as_trait)` (transpile.rs:867), `build_trait_clauses` (transpile.rs:1212) are quoted
from the grounded audit.

## Acceptance

Per the spec: byte-identical `run ≡ runvm ≡ real PHP` for `examples/guide/traits.phg` on the 8.4 floor; full
suite + clippy + fmt green; expected zero new `Op`; `phg explain` documents every new code; M-RT CLOSED in
MILESTONES.

## Rollback

Each task is an isolated commit — `git revert` the offending one. T1 is the only broad change (AST + flatten
pass); reverting it removes the trait surface entirely (the `trait` token returns to unused).
