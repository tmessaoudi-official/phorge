# M3 Stream 1 — PHP-fidelity syntax reshape (design)

> Status: **DESIGN — approved direction** (PHP-fidelity audit, Stream 1). Spec-first per the
> developer's overnight decision (2026-06-26): write this, then build each sub-feature end-to-end
> behind the byte-identity gate, committing green slices.
>
> Register: `docs/plans/2026-06-25-php-fidelity-and-divergence-audit.plan.md` (findings A-1, A-6,
> A-46, A-62). Philosophy: [[philosophy-of-phorge]] — Phorge : PHP :: TypeScript : JavaScript;
> familiarity-first; *better, not just different*; remove surprises, never capability.

## Why

Four surface divergences from PHP/TypeScript that the audit found have **no solid reason**. Each makes
Phorge look like it doesn't know its parent language. Stream 1 fixes the *syntax* (front-end only) so
both the language and its `lift`/`transpile` bridges read idiomatically. **No backend semantics
change** — the AST after parsing is unchanged for A-1 (pure surface), additive for A-6/A-46/A-62.
The byte-identity spine (`run ≡ runvm ≡ real PHP 8.5`) is the gate for every slice.

## Scope & order

Build order (lowest-risk-highest-confidence first, keystone A-1 last so the codemod lands on a stable
parser):

1. **A-62** — `"""…"""` auto-dedent text blocks (additive, self-contained).
2. **A-46** — expression `++`/`--` (additive; eval-order + `W-SEQUENCE-MUTATION` lint).
3. **A-6** — `foreach (xs as x)` + binding forms + `with int i` counter (additive; desugars to the
   existing `for-in`).
4. **A-1** — `: T` return types + `=>` function types, `->` **retired** (breaking; repo-wide codemod).

Each slice: TDD → parser/checker/backends as needed → guide example (byte-identity-gated) → docs →
green commit. A-1 ships last because its codemod rewrites every `.phg`; doing it after the additive
slices means the codemod runs once against final syntax.

---

## A-62 — `"""…"""` auto-dedent text blocks

**Decision (locked):** Java/Swift/Kotlin text-block consensus. `"…"` strings are **unchanged**
(single-line, `\n` escapes, interpolation). A new `"""` … `"""` form is a *multi-line* string with
**incidental-indentation stripping** and interpolation. Transpiles to a PHP **double-quoted string**
(NOT heredoc — heredoc's indent rules differ and PHP <7.3 lacks closing-marker indent; a plain
double-quoted literal with the dedented content is exact and floor-safe).

### Grammar / lexer
- Open: `"""` followed by optional trailing whitespace then a **mandatory newline** (the opening line
  carries no content — Java rule; `"""x` is an error `E-TEXTBLOCK-OPEN`).
- Close: a line whose only non-whitespace content is `"""`. The closing `"""`'s indentation column
  sets the **dedent baseline**.
- **Dedent algorithm (Java JEP 378, deterministic):** the common leading-whitespace prefix is the
  minimum indentation over (a) every non-blank content line and (b) the closing delimiter line. Strip
  exactly that prefix from every line. Trailing whitespace on each line is stripped. The result joins
  with `\n`; there is no leading or trailing newline unless written explicitly.
- **Interpolation:** identical to `"…"` — `{expr}` holes, `\{` escapes a brace. `\` escapes work.
- A backslash-newline at end of a content line suppresses that newline (Java `\<eol>`), optional —
  defer if it complicates; not required for parity.

### AST / backends
- **No new AST node, no new `Op`.** The lexer produces the same `StrSeg` sequence a `"…"` would; the
  text block is purely a *lexer-level* sugar that yields the dedented literal + interpolation holes.
  The parser/checker/interpreter/VM/transpiler never know it was a text block.
- Transpiler emits a normal PHP `"…"` (escaping `\n` etc.) — already handled by the Stream-2
  interpolation path.

### Tests / example
- Lexer unit tests for the dedent algorithm (mixed indent, blank lines, closing-delimiter column,
  interpolation hole spanning indentation).
- `examples/guide/text-blocks.phg`: a multi-line SQL-ish / HTML-ish block with an interpolated hole,
  byte-identical run/runvm/real-PHP.

### `lift` (↑)
- PHP heredoc/nowdoc (`<<<EOT`) → Phorge `"""…"""` is a **later** lift addition (not this slice;
  Tier-2 today). Note in KNOWN_ISSUES.

---

## A-46 — expression `++` / `--`

**Decision (locked, developer overruled my statement-only KEEP):** allow `++`/`--` as **expressions**
(prefix and postfix), PHP/C semantics. Obligations from the audit:
- **Pre vs post:** `++x` increments then yields the new value; `x++` yields the old value then
  increments. Both lower to existing ops (read, `+ 1`, store) — **no new `Op`**.
- **Eval order pinned to PHP L→R:** in `a[i++] = x` / `f(i++, i++)` the side effects sequence
  left-to-right, matching PHP. The differential (run ≡ runvm) is the proof; a divergence here is the
  classic null-op-scratch-slot trap ([[null-op-scratch-slot]]) — add a **two-in-one-expression** case.
- **`W-SEQUENCE-MUTATION` lint:** warn when a single expression both reads and mutates the same lvalue
  in a way whose result is order-dependent (`a[i] = i++`), rides the warning channel (stderr, never
  gates). Conservative: flag only the clear footguns.
- **Lvalue restriction:** target must be an lvalue (`Ident`, field, index) — `5++` / `(a+b)++` are
  `E-INCDEC-LVALUE`.

### AST / backends
- New `Expr::IncDec { target, inc: bool, prefix: bool, span }` (mirrors the existing PHP-side
  `PhpExpr::IncDec` already in the lifter). Lowers in **all three backends** to read-modify-write of
  the existing arithmetic + store ops; the value yielded depends on `prefix`. No new `Op`, no `Value`.
- Transpiler emits PHP `++$x`/`$x++` 1:1.
- **`lift` already parses `$i++`** (C-style for loop) but currently only as a `for`-step statement;
  this slice lets the lifter map `PhpExpr::IncDec` in *expression* position too.

### Tests / example
- Pre/post value semantics; in a loop; two-in-one-expression eval order; lvalue rejection; the lint.
- `examples/guide/incdec.phg`.

---

## A-6 — `foreach (xs as x)` + binding forms + `with` counter

**Decision (locked):** keep Phorge's `for (T x in xs)` working **and** add PHP's `foreach (xs as x)`
as a first-class equal (developer: "better, not just different — keep AND add on top"). Plus the four
binding forms and an optional **`with int i`** auto-increment position counter (developer's idea: "I
have to set a var to count things").

### Surface forms (all desugar to the existing `for-in` / indexed iteration — no new `Op`)
```
foreach (xs as x) { … }                         // value
foreach (xs as k => v) { … }                     // key/value (Map) or index/value (List)
foreach (xs as Point { x, y }) { … }             // destructure each element (reuses let-destructure)
foreach (0..n as i) { … }                        // range source
foreach (xs as x with int i) { … }               // value + a 0-based counter `i` (auto-increments)
foreach (xs as k => v with int i) { … }          // counter alongside key/value
```
- `as` is **contextual** (not a reserved word elsewhere) — like `when`/`as`-import already are.
- **Element type:** Phorge is typed; `foreach (xs as x)` infers `x`'s type from `xs`'s element type
  (the checker already knows `List<T>`/`Map<K,V>`). This is the inference the `lift` side calls
  Tier-2 — here we own both sides so the checker resolves it. A non-inferable source →
  `E-FOREACH-ELEM-TYPE` (suggest the explicit `for (T x in xs)`).
- **`with int i`:** introduces `i` (the declared type must be `int`; `E-FOREACH-COUNTER-TYPE`),
  initialized 0, `+1` each iteration after the body — lowered as an extra induction local. Scoped to
  the loop. Works with every binding form.

### AST / backends
- Extend the existing for-in `Stmt` with optional `key` binding, an optional destructure pattern
  element, and an optional `counter: Option<(String, /*int*/)>`. OR add `Stmt::Foreach` that the
  checker **lowers** to the existing for-in + counter induction before backends (preferred: lower in
  the checker/parser so interpreter/VM/transpiler are untouched — same discipline as `|>`→Call,
  alias expansion). Decision: **lower in the parser/checker**, no backend changes.
- Transpiler: a `foreach` lowered form still emits idiomatic PHP `foreach ($xs as $k => $v)` — so the
  transpiler keeps the foreach shape for output fidelity (it reads the lowered AST; if lowering
  erases the foreach shape, the PHP would be a C-style for — acceptable but less idiomatic). **Open
  sub-decision:** keep a `Stmt::Foreach` through to the transpiler for idiomatic PHP, lower only for
  interpreter/VM. Resolve during build (favor idiomatic PHP — likely keep the node, lower in the two
  Rust backends via a shared helper). If this balloons, park to the blockers file.

### Tests / example
- Each binding form; counter; range source; nested; element-type inference + the two error codes.
- `examples/guide/foreach.phg`; round-trip a PHP `foreach` via `lift` (un-defers the lifter's
  Tier-2 foreach rejection — the lift side now has the same element-type inference to lean on).

---

## A-1 — `: T` return types, `=>` function types, `->` retired

**Decision (locked):** PHP/TypeScript return-type syntax `function f(): T`. Function **types** use
`=>` (`(int) => string`). The `->` token (`TokenKind::Arrow`) is **fully retired**. Typed lambda:
`fn(int x): string => …` (expr body) / `fn(int x): string { … }` (block body). The `=>` lambda-body
separator and map/match `=>` are unchanged (distinct contexts — type vs expression).

### Grammar (verified safe)
- `:` is free in signature position. Its only current uses are **brace-internal** field renames
  (struct pattern `{ field: bind }`, let-destructure `Type { field: bind }`) — a return-type `:`
  always follows the `)` of a param list, no collision. [Verified: `src/parser/patterns.rs:84`,
  `src/parser/stmts.rs:142`.]
- Three return-type sites move `Arrow → Colon`: fn decl (`items.rs:170`), method (`items.rs:449`),
  lambda (`exprs.rs:423`). One function-type site moves `Arrow → FatArrow` (`types.rs:95`).
- Function type `=>` (type context) vs lambda body `=>` (expr context) vs map/match `=>` never
  collide — the parser is in distinct states. [Inferred: type vs expr parse paths are separate.]

### Migration (the codemod — the risky part, gate-protected)
**Two-phase, gate-verified retirement** (never break the suite mid-flight):
1. **Dual-accept parser:** return-type position accepts `:` **and** (deprecated) `->`; function-type
   position accepts `=>` **and** (deprecated) `->`. Commit — non-breaking, all 89 examples still
   parse. Add new-syntax tests.
2. **Codemod** every `.phg` (examples + projects + fixtures) and the ~190 inline test programs and
   the docs to the new syntax. Tool: a Python pass that distinguishes the two `->` roles. Heuristic —
   a **return-type** `->` is followed by a type then one of `{` `;` `=>` or end-of-signature; a
   **function-type** `->` is followed by a type then `)` `,` or a param name (still inside a type/param
   list). Where the heuristic is unsure, leave `->` (dual-accept still parses it) and the differential
   flags nothing; a final `grep -rn '\->' examples` lists residuals for hand-fixing. **The
   byte-identity gate is the safety net:** any mis-rewrite either fails to parse or diverges in output.
3. **Update printer + lifter** to EMIT `:` / `=>`. Regenerate `examples/lift/sample.phg`.
4. **Retire `->`:** once `grep -rn '\->' **/*.phg` is empty and green, remove the deprecated `Arrow`
   branches; `Arrow` becomes an `E-ARROW-RETIRED` parse error pointing to `:`/`=>`. Keep the token in
   the lexer so the error is friendly (not "unexpected character"). Commit.

### Backends
- **Zero backend change.** Return-type/function-type syntax is consumed entirely by the parser; the
  AST (`FunctionDecl.ret`, `Type::Function`, `LambdaBody`) is unchanged. Interpreter/VM/transpiler and
  the PHP output are byte-identical before/after. The differential proves it.

### Tests / example
- New-syntax parse tests at all four sites; `E-ARROW-RETIRED` after retirement; the whole example
  suite re-greened post-codemod (that *is* the coverage); `examples/guide/` programs already exercise
  return types throughout.
- **`phg explain E-ARROW-RETIRED`** entry.

### `lift` (↑)
- The lifter already emits Phorge; after the printer update it emits `: T` / `=>` automatically. PHP
  `: T` return types already lift (the PHP parser reads `:`); no lift change beyond the printer.

---

## Cross-cutting

- **Docs:** `README.md`, `docs/INVARIANTS.md` (if it pins syntax), `FEATURES.md`, `examples/README.md`,
  any tutorial — sweep for `->` and `function … ->`. The `phg explain` registry gains
  `E-ARROW-RETIRED`, `E-TEXTBLOCK-OPEN`, `E-INCDEC-LVALUE`, `E-FOREACH-ELEM-TYPE`,
  `E-FOREACH-COUNTER-TYPE`, `W-SEQUENCE-MUTATION`.
- **Playground:** the default example + any bundled snippets use `->`; codemod them too (the wasm
  build embeds examples).
- **Blockers:** any sub-decision that turns out to need the developer (e.g. A-6 transpiler foreach
  shape, or a codemod ambiguity that the gate can't disambiguate) → `overnight-blockers-2026-06-26.md`
  with a recommendation, and move on.

## Success criteria (per slice)
`run ≡ runvm ≡ real PHP 8.5` byte-identical on every example incl. the new guide program; full
workspace + PHP oracle green; clippy + fmt clean; a guide example + README entry shipped in the same
commit; `phg explain` covers every new code.
