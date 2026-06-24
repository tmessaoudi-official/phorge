# Implementation Plan — `const` + Expression Field Initializers + Mandatory `new`

> **For a fresh session:** decisions are locked (see the two specs + the master plan Decisions Log).
> Build straight from this, in order. Specs:
> `docs/specs/2026-06-24-member-initializers-design.md` (const + field-init),
> `docs/specs/2026-06-24-mandatory-new-design.md` (new).
> Each task ships green + byte-identical (`run ≡ runvm ≡ real PHP 8.5`, gate:
> `PHORGE_PHP=/stack/tools/phpbrew/php/php-8.5.7/bin/php PHORGE_REQUIRE_PHP=1 cargo test --workspace`),
> TDD, with a guide example where it adds surface. **Commit pre-hook runs the ~100s build test — use a
> 300s Bash timeout when committing.** Build binary + report path after each feature (standing rule).

**Build order:** A `const` (additive) → B field-initializers (instance, then static) → C `new` (breaking codemod last).

---

## Feature A — `const` class constants ✅ DONE (`c6b1ac2`)

> Landed end-to-end: shared `ast::class_consts` (own + inherited + trait consts flattened), checker
> collection + access + visibility enforcement (the one site Phorge enforces member visibility) +
> SCREAMING_SNAKE casing, interpreter inline, compiler `Op::Const` + `CTy` operand, transpiler PHP
> typed class const + `Class::NAME`. 8 `E-CONST-*` codes, all `phg explain`-documented.
> `examples/guide/constants.phg` byte-identical run≡runvm≡PHP 8.5; 710 lib + 108 differential green.

**Spec:** member-initializers §"Feature 1". No new `Op`/`Value`. `const` is already parsed as
`Modifier::Const` on a field with an initializer (today the checker rejects it as an instance field) —
so the work is checker + backends recognizing a const-modified field as a class constant.

- **A1 — checker collect (`src/checker/collect.rs`):** when a `ClassMember::Field` carries
  `Modifier::Const`, collect it into a new `class_consts: HashMap<(String,String), (Value, Visibility)>`
  table instead of `fields`. Validate: initializer is a literal/const-expr (`value::const_literal`,
  extend for `+`/`*` on consts later — v1 literal is fine) else `E-CONST-NOT-LITERAL`; initializer
  required (`E-CONST-NO-INIT`); `const mutable` → `E-CONST-MUTABLE`. Record visibility (member-level
  `Modifier::{Public(default),Private,Protected}`).
- **A2 — checker access (`src/checker/expr.rs` Member arm + `resolve.rs`):** `C.MAX` where `C` is a
  class name and `MAX` is in `class_consts` (walk parents for inheritance) → resolves to the const's
  type. Instance access `c.MAX` → `E-CONST-INSTANCE-ACCESS` (and sharpen the existing static
  instance-access message similarly). Reassign `C.MAX = …` → `E-CONST-REASSIGN`. Member-visibility
  enforced via the existing lattice.
- **A3 — backends inline the literal:** interpreter (`src/interpreter/`) + compiler
  (`src/compiler/`): a `C.MAX` access resolves to the stored literal `Value` → interpreter returns it;
  compiler emits `Op::Const(idx)`. Mirror the existing static-access resolution path; const branch
  precedes the static-field branch. `ctype` for a const access = the const's `CTy` (operand-trap:
  `C.MAX + 1` must specialize).
- **A4 — transpiler (`src/transpile/`):** emit `[vis] const TYPE NAME = <literal>;` inside the class
  (PHP 8.3+ typed const); emit a `C.MAX` access as `C::MAX` (NO `$` — distinct from a static field's
  `C::$s`, which the static path emits).
- **A5 — example + tests + gate:** `examples/guide/constants.phg` (a public const, a private const used
  internally, an inherited const via subclass name, `C.MAX + 1` as an operand). Checker tests for each
  `E-CONST-*`. `phg explain` for each code. Full gate; commit; build binary.

---

## Feature B — Expression field initializers

**Spec:** member-initializers §"Feature 2". No new `Op`/`Value`. Two steps: instance first (clean ctor
lowering), then static (one-time guarded init — the riskier PHP-timing piece).

### B-instance ✅ DONE (`4873d45`)

> Landed: checker lifts the init rejection + type-checks + forward-ref guard (`E-FIELD-INIT-FORWARD-REF`,
> `E-FIELD-INIT-TYPE`; this-capture reuses `E-LAMBDA-THIS`); shared `ast::field_initializers` (own
> initializers of the constructor PHP invokes — no auto-chain); interpreter `run_field_inits`, compiler
> `SetField` in the synthetic ctor, transpiler ctor-prelude + synthesized `__construct`.
> `examples/guide/field-init.phg` byte-identical run≡runvm≡PHP 8.5; 719 lib + 108 differential green.

- **B1 — checker:** lift the "instance field cannot have an initializer" rejection for a plain
  (non-static, non-const) field. Type-check the initializer against the field type. **Declaration-order
  scope:** an initializer may reference `this` and **earlier-declared** instance fields; a reference to
  a **later** field → `E-FIELD-INIT-FORWARD-REF`. A field-default **closure capturing `this`** →
  `E-FIELD-INIT-THIS-CAPTURE` (v1; defers to the this-capture slice). A non-closure initializer may read
  `this.earlierField`.
- **B2 — interpreter + VM construction:** at instance construction, after promoted-ctor params are
  bound and before/around the ctor body, evaluate each field initializer **in declaration order** and
  set the field. Both backends share the order ⇒ identical field values. (Touches the ctor_plan /
  construction path in `interpreter/` + `compiler/` + `vm/`.)
- **B3 — transpiler:** lower instance-field initializers into the **constructor prelude** — prepend the
  `$this->f = <expr>;` assignments after promotion; synthesize a `__construct` if the class has none.
- **B4 — example + tests + gate:** `examples/guide/field-init.phg` (a computed default via a call, a
  closure default, a `this`/earlier-sibling read in order). Checker tests for the forward-ref +
  this-capture guards. Full gate; commit.

### B-static ✅ DONE (`af3ad03`)

> Landed eager (decided): checker static-init type-check moved to post-collection pass
> (`check_static_inits`, no `this`; `E-STATIC-INIT-CONST` retired); interpreter `eval_static_inits`
> before `main`; compiler `SetStatic` prelude at start of `main` (literals seeded, non-literals
> placeholder); transpiler `__phorge_init_statics()` before `main()`. `examples/guide/static-init.phg`
> byte-identical run≡runvm≡PHP 8.5; 723 lib + 108 differential green. **Feature B COMPLETE.**

- **B5 — static expression initializers:** checker — allow an arbitrary expression (not just a literal)
  for a `static` field. Interpreter/VM — evaluate **once** at program start in declaration order
  (extend the existing `static_inits` path, which today only handles literals). Transpiler — emit a
  **one-time guarded init** (a generated `__phorge_init_statics`-style run-once, or a `??=`-guarded lazy
  set on first access); statics evaluate in declaration order. This is the spec's flagged risky corner —
  keep the guard mechanism single-sourced and differential-gated.
- **B6 — example + tests + gate:** extend `field-init.phg` (or a companion) with a runtime static init;
  byte-identical 3-way. Full gate; commit; build binary.

---

## Feature C — Mandatory `new` (breaking, last)

**Spec:** mandatory-new. Front-end only — no `Op`/`Value`/backend change.

- **C1 — parser (`src/parser/exprs.rs`):** a `new` prefix in expression position parses the following
  construction call and wraps it `Expr::New(Box<Expr>, Span)` (new `ast::Expr` variant). Bare `new` not
  followed by a call → parse error.
- **C2 — checker:** one validation pass over construction sites (uses the class/enum tables): a
  `Call` whose callee is a class/enum-variant and is NOT `Expr::New`-wrapped → `E-NEW-REQUIRED`; an
  `Expr::New` whose inner callee is not a class/variant → `E-NEW-ON-NONCONSTRUCT`. Add an `unwrap_new`
  pass (alongside `expand_aliases`/`erase_generics` in `cli::check_and_expand`) that strips
  `Expr::New` → its inner `Call` before any backend. Backends + transpiler unchanged.
- **C3 — codemod (`tools/new_codemod.py`):** semantic rewrite `Name(args)` → `new Name(args)` for every
  class + enum-variant construction across all `.phg` + inline Rust test programs + fixtures + vendored
  deps. Must consult class/variant names (not syntactic — `Counter()` vs `compute()` look identical):
  drive from the per-file known class/variant set, or a `phg --rewrite-new` mode. Leave free-fn/native
  calls alone. Re-run the gate; the codemod is surface-only so all examples stay byte-identical.
- **C4 — explain + gate:** `phg explain E-NEW-REQUIRED` / `E-NEW-ON-NONCONSTRUCT`. Full gate; commit;
  build binary. (No dedicated example — `new` appears across all existing class/enum examples after the
  codemod, like the return-type mandate.)

---

## Cross-cutting
- After each feature: `cargo build --release --bin phg`, report `/stack/projects/phorge/target/release/phg`.
- Update `CHANGELOG.md` (Unreleased) + `examples/README.md` index/matrix per feature.
- Mark each feature done in the master plan Decisions Log as it lands.
- **Loose end (separate):** the playground `run≠runvm` parity bug needs the developer's repro code — a
  program that type-checks clean but the VM rejects (a CTy-operand-trap-class gap). Not part of these
  features; chase when the repro arrives.
