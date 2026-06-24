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
- **S0a — `void` + `Empty` types.** Add `Ty::Void` (uncapturable) + `Ty::Empty` (holdable), `void <:
  Empty` in `assignable_with`; make both writable builtins (`is_builtin_type_name`); migrate internal
  `Ty::Unit` semantics. Capture-of-void → new error `E-VOID-CAPTURE`. Transpiler: `void` → `: void`,
  `Empty` → plain value.
- **S0b — Mandatory return types + repo-wide codemod.** Every function/method/**lambda** must declare a
  return type; **no exemptions** where syntactically applicable (constructors have no return slot →
  inherently N/A; property hooks are typed by the property). New code is then born annotated.
  Breaking codemod across all `.phg` + inline test programs + fixtures + vendored deps (mirror the
  namespace-reshape tooling). New error `E-MISSING-RETURN-TYPE`. Run **before** Phase 1/2.

### Phase 1 — Ergonomics perimeter (spec: ergonomics-perimeter; 7 slices)
1. **String** — `+` concat (typed; `string+int` = error), `\u{HEX}` escapes (lex→UTF-8), literal braces
   (`\{`/`\}` + raw strings `r"…"`/`r#"…"#`).
2. **Operators/patterns** — ternary `? :` (disambiguate optional `x?` in type pos), or-patterns in
   `match` (`1 | 2 | 3 =>`), `**` operator (type-directed) + `Math.ipow(int,int)->int`.
3. **Types** — parenthesized return-position function types (`() -> ((int) -> bool)`); fixed-length
   lists `[T; N]` (alongside `List<T>`; compile-time length + static bounds; length-immutable; erases
   to PHP array). *(writable `void`/`Empty` already done in S0a.)*
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
- **No-value types:** `void` (uncapturable keyword) + `Empty` (PascalCase holdable type), `void <: Empty`.
- **UFCS:** general, method-first.
- **Return-type mandate:** all fns/methods/lambdas; no exemptions; folded in, codemod first (S0b).
- **Contested:** string `+` ✓; UFCS ✓; `s[0]`→defer M-text + Text natives; ternary ✓; `switch`→reject,
  or-patterns instead ✓; power→`**`+`Math.ipow` both ✓.
- **Defer set:** `\u{}`→pull forward ✓; tuples→defer; let-destructuring→full+`else` ✓; **fixed-length
  lists `[T; N]`** added ✓; `this`-capture→build ✓; generic-fn-value→defer; decimal/BigInt→M-NUM.
- **Reject confirmed:** single-quotes; `<=>`; `.` concat; `switch`.
- **Introspection depth:** typeName+className+hierarchy+**member enumeration** (read-only).
