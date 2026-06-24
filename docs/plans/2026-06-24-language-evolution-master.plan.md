# Language Evolution — Master Implementation Plan

> **For a fresh session:** all design ambiguities are resolved (item-by-item with the developer,
> 2026-06-24). Build straight from this. Specs hold full detail; this file is the authoritative
> sequence + the resolved decisions. Each slice ships green + byte-identical
> (`run ≡ runvm ≡ real PHP 8.5`, oracle: `PHORGE_PHP=/stack/tools/phpbrew/php/php-8.5.7/bin/php
> PHORGE_REQUIRE_PHP=1 cargo test --workspace`) with a guide example, gate per commit.

**Specs:** ergonomics perimeter `docs/specs/2026-06-24-language-ergonomics-perimeter-design.md`;
introspection/process `docs/specs/2026-06-24-introspection-strings-process-design.md`.

## Resolved type design — `void` + `Empty` (the foundation)

- **`void`** (lowercase, keyword-primitive): a function `-> void` returns nothing; **capturing it is a
  compile error** (`var x = noop()` → error). Transpiles to PHP `: void`. The common return type.
- **`Empty`** (PascalCase, built-in type like `List`/`Map`/`Set`/`Html`): a real, inhabited type with
  one value — **holdable**, composes with generics (`(T) -> Empty`, `T = Empty` is fine). Transpiles to
  a plain PHP value (implicit `null`, **not** `: void`), so capturing stays valid → byte-identity safe.
- **`void <: Empty`** (void widens to Empty): so an everyday `void`-returning callback flows into a
  generic `(T) -> Empty` slot — keeps the two-type model ergonomic (the one consequence of having two).
- Replaces the current implicit `Ty::Unit`. Codemod maps every un-annotated fn to `-> void` (common
  case); the rare "must hold a nothing" spot uses `-> Empty`.
- *(Developer chose two types after a 3-round challenge: `void` = "literally nothing", `Empty` = "the
  one you can hold". `unit` keyword rejected. `Empty` PascalCase so it never collides with an `empty`
  variable.)*

## Build sequence

### Phase 0 — Foundation (do first; everything builds on it)
- **S0a — `void` + `Empty` types. ✅ DONE (`4606b1f`).** `Ty::Unit` → `Ty::Void` + new `Ty::Empty`;
  `void <: Empty` in `assignable_with`; both writable builtins; `E-VOID-CAPTURE` when a void value is
  bound (unless annotated `Empty`); `Empty` exempt from the totality check. Transpiler: `void` → PHP
  `: void`, `Empty` → **no return hint** (PHP infers a capturable `null`; `: mixed`/`: null` would
  reject a fall-off or bare `return;`). `examples/guide/void-empty.phg`; byte-identical run≡runvm≡PHP 8.5.
- **S0b — Mandatory return types + repo-wide codemod. ✅ DONE.** Every named function, method
  (incl. `abstract` + interface signatures), and **statement-body** lambda must declare a return type
  (`E-MISSING-RETURN-TYPE`); `main` included. **Expression-body lambdas (`fn(x) => e`) keep inferring**
  — decided after challenge (the `=>` form's whole point is terseness, the soundness rationale doesn't
  apply to a single total expression, and PHP arrow fns can't carry a return type anyway). Constructors
  (no return slot) and property hooks (typed by the property) are exempt. Enforced in `check_function`
  (fns/methods/abstract) + interface-method collection. Codemod `tools/return_type_codemod.py` (a
  balanced-paren scanner — function-typed params contain `->`, so a regex won't do) added `-> void` to
  ~810 sites across all `.phg` + inline Rust test programs; vendored deps already annotated (lock hash
  untouched). `phg explain E-MISSING-RETURN-TYPE`/`E-VOID-CAPTURE` added.

### Phase 1 — Ergonomics perimeter (spec: ergonomics-perimeter; 7 slices)
1. **String — ✅ DONE** (`a0a3c95` + `614b07c`). `+` concat (typed; `string+int` = error; reuses
   `Op::Concat(2)` via new `CTy::Str`; `__phorge_add` PHP helper), `\u{HEX}` escapes (lex→UTF-8),
   literal braces `\{`/`\}` + raw strings `r"…"`/`r#"…"#` (lexer-side interpolation split —
   `TokenKind::Str` → `StrSeg::{Lit,Interp}` segments). `examples/guide/strings-ext.phg`.
2. **Operators/patterns** — **PARTIAL.**
   - **`**` power — ✅ DONE** (`2fd6194`). Type-directed (`int**int→int`, `float**float→float`),
     right-assoc, binds tighter than `*`; no new `Op` (compiler lowers to `Op::CallNative`
     `Core.Math.ipow`/`pow`; single-sourced `value::int_pow`/`float_pow` kernels). `Math.ipow(int,int)
     ->int` native. `examples/guide/operators.phg`.
   - **or-patterns — ✅ DONE** (`d8365ab`). `1 | 2 | 3 => …` / `Red() | Yellow() => …`; parser desugars
     to one arm per alternative (no backend change), binding-free only (`E-OR-PATTERN-BIND`).
     `examples/guide/pattern-matching.phg`.
   - **ternary `? :` — ✅ DEFERRED (developer decision 2026-06-24).** Genuine grammar collision with the
     existing **postfix-`?` propagation** (`f()?`): one-token lookahead can't separate `c ? -1 : 1`
     (ternary) from `f()? - 1` (propagate-then-subtract). Backtracking would resolve it mechanically
     (low impl risk, zero byte-identity risk — lowers to `Expr::If`), but a triple-meaning `?`
     (propagation / `?.` safe-access / ternary) is a permanent reader-legibility cost. The already-
     shipped **expression-`if`** (`int x = if (c) { 1 } else { 2 };`) covers the capability
     unambiguously, so ternary adds no expressive power. **Deferred (not rejected):** mechanism is
     scoped; revisit if real usage shows the missing `? :` chafes. **Slice 2 is COMPLETE** (`**` +
     or-patterns + the existing expr-if).
3. **Types — ✅ COMPLETE.**
   - **Parenthesized return-position function types — ✅ DONE** (`1993847`). `() -> ((int) -> bool)`
     parses (a `(` in type position is now disambiguated: param-list if `->` follows, else a grouped
     type `(T)` ≡ `T`). Parser-only. `examples/guide/lambdas-pipe.phg`.
   - **Fixed-length lists `[T; N]` — ✅ DONE** (`Ty::FixedList`/`Type::FixedList`; compile-time length +
     static literal-index bounds + `[T;N]`→`List<T>` assignability + length-preserving element-set;
     erases to a PHP array, no new `Op`/`Value`). Codes `E-FIXEDLIST-LEN`/`E-FIXEDLIST-BOUNDS`.
     `examples/guide/fixed-lists.phg`. (writable `void`/`Empty` already done in S0a.) **Irrefutable
     fixed-list destructuring payoff lands in slice 5.**
4. **Closures** — `this`-capture (live, by-reference Rc handle; remove `E-LAMBDA-THIS`; PHP arrow-fn
   auto-captures `$this`). Same cycle-leak stance mutation already takes.
5. **Destructuring** — `var Point { x, y } = p` (irrefutable) + `var [a, b] = xs else { … }` (refutable
   list bail-out). After slice 3 so fixed-list destructuring is irrefutable.
6. **UFCS** — `x.f(a)` ≡ `f(x, a)`, **general** (any free function), **method-first** resolution (real
   method on x's type wins; else free-function fallback). Enables `xs.length()`, `xs.filter(p).map(g)`.
7. **stdlib** — `Text.charAt` / `Text.substring` natives (the safe alternative to `s[0]`; → M4).

### Phase 2 — Introspection + process (spec: introspection-strings-process)
- **Core.Reflect** (deterministic, byte-safe): `typeName`/`className`/`implements`/`parents`/`traits`/
  `methodNames`/`fieldNames`. **Mechanism (resolved):** add a `NativeEval::Reflective(fn(&[Value],
  &ClassTables) -> …)` arm — pure-native can't reach the hierarchy, so each backend passes its shared
  `ast::class_implements` + `class_method_origins` + field decls (single-sourced ⇒ byte-identical). No
  new `Op` (still `Op::CallNative`). Read-only name-level only; dynamic dispatch / instantiate-by-string
  / attribute reflection stay rejected.
- **Process I/O** — `Core.Process.args()`, `Core.Env.get/all` on a **quarantine seam** (impure-native
  marker, excluded from `differential.rs`; README walkthrough, not a gated example). M-Batteries
  kickoff. CLI: `phg run f.phg -- arg1 arg2`. `P-build-argv` noted (M2.5 P3).
- **Superglobal map** — documentation/routing: `$_GET`/`$_POST`/`$_FILES`/`$_COOKIE` → M6 `Request`;
  env/args → here; `$_REQUEST`/ambient access → rejected. No new mechanism here.

## Deferred / rejected (do NOT build)
- **Defer:** `s[0]` string index → M-text (codepoint); tuples → classes (revisit as named records);
  generic-fn-as-value → lambda-wrap; `decimal`/`BigInt` → M-NUM/M-NUM-2.
- **Reject:** single-quote strings (raw strings cover it); spaceship `<=>` (typed `Ordering` at sort);
  PHP `.` concat (`.` is member access; concat is `+`); `switch` (match + or-patterns).

## Loose ends (track; not part of the slices)
- **Side-bug:** chained force-unwrap field read `a.next!.next!.v` → "no field v on Node" — likely a real
  `opt!`-then-field-access bug on object optionals. Confirm with a clean repro + fix early (correctness).
- **Playground:** `f66592d` (php-wasm fresh-instance fix) — pending the developer's `git push` + a live
  re-verify of the deployed page (editor + 3-way badge + PHP tab no-redeclare).

## Decisions Log (2026-06-24)
- **Fixed-length lists `[T; N]` — BUILD in slice 3, semantics locked (decided 2026-06-24).**
  `[T; N]` is **assignable to `List<T>`** but not the reverse (a fixed list *is* a list; a list has
  unknown length — like Rust arrays→slices / TS tuples→arrays). **Element-set `pair[i] = e` is allowed**
  (length-preserving; no length-changing op exists in the surface). **Static bounds for literal indices
  only** (`pair[5]` on `[int; 2]` is a compile error); a dynamic index falls back to the existing
  runtime bounds check. **Erases to a PHP array / `Value::List`** — no new `Value`/`Op`; the length is a
  compile-time-only guarantee. The **irrefutable-destructuring payoff is deferred to slice 5**
  (let-destructuring); slice 3 ships the type + static bounds + a guide example.
- **Ternary `? :` — DEFERRED, not rejected (decided 2026-06-24).** The `?` collides with the existing
  postfix-`?` propagation; backtracking would resolve it mechanically (low risk, lowers to `Expr::If`),
  but overloading `?` to a third meaning is a permanent reader-legibility cost, and the already-shipped
  **expression-`if`** covers the capability unambiguously (ternary adds no expressive power). Kept on the
  roadmap (mechanism scoped) to revisit with real usage data. **Slice 2 closed** with `**` power +
  or-patterns + expr-if.
- **Static field initializers — EAGER, no config (decided 2026-06-24).** Feature B-static evaluates
  static expression initializers **once at program start, in declaration order, before `main`** —
  matching the existing Rust static-init model (interpreter/VM already seed statics eagerly at startup;
  PHP via a generated `__phorge_init_statics()` called before `main()`). **Lazy `??=`-on-first-access
  was rejected** (re-architects every `GetStatic`, leans harder on eval-order parity). **Runtime
  env/`.ini` configuration was rejected as architecturally unsound**: the transpiled PHP runs with no
  Phorge runtime, so a runtime knob can't reach it → byte-identity would break in production,
  undetectable by the local gate; it also imports PHP's most-criticized misfeature (server-`.ini`-
  dependent semantics) against Phorge's "remove surprises" thesis. **The legitimate form of
  configurability is COMPILE-TIME** — a `phorge.toml [language]` table / editions, resolved once and
  baked identically into all three backends *and* the emitted PHP (the Rust-editions / `tsconfig`
  model). Deferred to **M13 (editions, post-1.0)**, where static-init mode can become one documented
  edition flag. **General principle for all future feature-flagging:** a language flag may be
  compile-time (baked into all 3 backends) — never a runtime knob each backend reads independently, or
  it breaks the byte-identity spine. Each such flag also doubles the differential test surface, so flags
  are a deliberate gated investment, not a per-feature default.
- **No-value types:** `void` (uncapturable keyword) + `Empty` (PascalCase holdable type), `void <: Empty`.
- **UFCS:** general, method-first.
- **Return-type mandate:** named fns + methods (incl. abstract/interface) + **statement-body** lambdas;
  `main` included. **Expression-body lambdas `fn(x) => e` keep inferring** (decided 2026-06-24 after the
  developer's "Option 2?" instinct was challenged: the `=>` form exists to be terse, an expression body
  can't fall off the end so the soundness mandate is vacuous there, PHP arrow fns take no return type,
  and TS/Rust/Kotlin/Swift all infer — so the rule is "every *block-bodied* function is annotated").
  Constructors + property hooks exempt. Codemod-first (S0b, done).
- **Contested:** string `+` ✓; UFCS ✓; `s[0]`→defer M-text + Text natives; ternary ✓; `switch`→reject,
  or-patterns instead ✓; power→`**`+`Math.ipow` both ✓.
- **Defer set:** `\u{}`→pull forward ✓; tuples→defer; let-destructuring→full+`else` ✓; **fixed-length
  lists `[T; N]`** added ✓; `this`-capture→build ✓; generic-fn-value→defer; decimal/BigInt→M-NUM.
- **Reject confirmed:** single-quotes; `<=>`; `.` concat; `switch`.
- **Literal braces (decided 2026-06-24, after surfacing an implementation wrinkle):** `\{`/`\}`
  backslash escapes (the spec's choice — reads like C/JSON) **and** raw strings `r"…"`/`r#"…"#`. The
  `\{` form needs a lexer-side interpolation split (`TokenKind::Str` → segment list) so the lexer
  distinguishes a literal `\{` from a bare interpolation `{` (the parser-side split on a flat value
  couldn't — `\{` and `\\{` collapse to the same bytes). Raw strings fall out of the same refactor
  (a single literal segment). String-slice part 1 (`+`, `\u{}`) shipped in `a0a3c95`.
- **Introspection depth:** typeName+className+hierarchy+**member enumeration** (read-only).
- **Mandatory `new` — ✅ DONE (`5fb1259`, 2026-06-24).** Shipped front-end-only (parser `Expr::New` +
  checker validate/`unwrap_new` + loader resolve arm + `phg rewrite-new` codemod); 723 lib + 108
  differential + all integration green. `E-NEW-REQUIRED`/`E-NEW-ON-NONCONSTRUCT`. **The
  const→field-init→new feature plan is now fully complete (Features A, B, C).**
- **Mandatory `new` — EVERYWHERE (decided 2026-06-24).** `new ClassName(...)` AND `new Variant(...)`
  for enum-variant construction (`new Some(7)`, `new Circle(2.0)`). The developer chose uniformity ("a
  clean `new` everywhere") over my Option-1 rec (classes only) — accepted trade-off: no surface language
  `new`s a sum-type variant, so it's a deliberate Phorge departure for one-rule simplicity. `new` is
  currently a reserved-but-unhandled token. Breaking codemod (`Name(...)` → `new Name(...)` for every
  class + enum-variant construction; needs the checker's type tables to tell construction from a plain
  call). Lists/maps/sets/closures/primitives are literals/native — unaffected. Own design+plan pass.
- **`const` class constants — ✅ DONE (`c6b1ac2`, 2026-06-24).** Shipped exactly as designed (below):
  shared `ast::class_consts` flatten (own+inherited+trait), checker collect/access/visibility +
  SCREAMING_SNAKE casing, interpreter inline / compiler `Op::Const`+`CTy` / transpiler PHP typed const
  `Class::NAME`; 8 `E-CONST-*` codes; `examples/guide/constants.phg` byte-identical run≡runvm≡PHP 8.5.
- **`const` class constants — ACTIVATE with visibility (decided 2026-06-24).** Currently vestigial
  (reserved `Modifier::Const`, no semantics; parse-errors as a local, rejected as a class field). Make
  it a real PHP-style class constant: `[vis] const TYPE NAME = <literal>;`, class-name-only access
  (`C.MAX`), immutable, member-visibility (public default / `private` / `protected`). Compile-time
  literal, inlined on the Rust backends (no new `Op`/`Value`), → PHP typed class const (`const int MAX
  = 100;`, 8.3+; floor 8.5 ✓) accessed `C::MAX` (no `$`, unlike a static field's `C::$s`). Resolved
  open points (developer accepted all recs): **inherited** (subclass accesses via its own name);
  const-of-const **deferred** (literal-only v1); interface constants **deferred** (classes-only v1).
- **Expression field initializers — instance + static (decided 2026-06-24).** Lifts PHP's
  constant-expression-only restriction on property defaults (verified: PHP forbids call/method/closure/
  static-read/`$this` in a default — "Constant expression contains invalid operations"). Phorge allows
  ARBITRARY expressions + closures in field initializers, lowered to valid PHP: **instance** fields →
  a constructor prelude (per-construction, declaration order); **static** fields → a one-time guarded
  init in PHP (the harder case — developer chose to include statics). An initializer **may read `this`
  and earlier-declared sibling fields** (declaration-order eval; reading a *later* field = error — the
  forward-reference guard tames the half-constructed-object footgun the developer accepted). `const`
  stays compile-time-literal (not part of this). Byte-identity via identical decl-order evaluation on
  all three backends. Own design+plan.
- **Build order: SPECS-FIRST for all three** (`new`, `const`, expression-initializers) before any
  implementation — developer's call. Specs land for review, then plans, then build.
