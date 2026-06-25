# M-Lift — PHP → Phorge (`phg lift`) Plan

> The reverse of `transpile`: read PHP, emit a Phorge **draft**. A new front-end subsystem. Scoped as
> a **best-effort, review-required** tool — NOT a verified-equivalent transform (see the verdict below).

## Verdict / Decisions Log
- [2026-06-25] AGREED: **Pursue it** — but framed correctly. Build a bounded best-effort tool, not a
  100%-confidence transpiler.
- [2026-06-25] AGREED: **Name = `lift`** (`phg lift foo.php` → `foo.phg`). NOT "transpile" — that name
  carries the byte-identity guarantee the reverse direction cannot have (false promise, like the
  rejected `composer.json`). Asymmetry of names mirrors asymmetry of guarantees: transpile *down*
  (total, verified) vs lift *up* (partial, review-required). Alternatives considered: `port`/`import`.
- [2026-06-25] AGREED: **100% confidence is impossible in general** (fundamental, not an engineering
  gap): the languages aren't bijective (Phorge = strict/typed/smaller; PHP = dynamic/larger), type
  inference from untyped PHP is undecidable + lossy (`array` ⇒ List|Map|Set), and no spine runs
  backward. Same reason no 100% JS→TS converter exists. So: best-effort + human-in-the-loop, honest
  boundaries.
- [2026-06-25] AGREED: **Tier-1 first, demo-angle first.** Highest value-per-effort + the "show what
  PHP becomes in Phorge" use case (playground "paste PHP → see Phorge").
- [2026-06-25] OPEN (ask before build): demo angle (smaller, playground-first) vs migration angle
  (bigger, needs the round-trip gate) as the primary driver — they share the parser but differ in depth.

## Feasibility tiers (what `lift` handles)
| Tier | PHP shape | Confidence |
|---|---|---|
| **1** | Already Phorge-shaped: typed signatures (PHP 7/8 hints), typed class props, `enum` (8.1), `match`, plain control flow, arrays | High (near 1:1 backward) |
| **2** | Untyped-but-inferrable; `array` whose List/Map role is clear from use | Medium (heuristic + checker validation) |
| **3** | Dynamic PHP (`$$x`, `eval`, magic methods, reflection, true `mixed`) | **Refuse + flag** `// CANNOT LIFT: <reason>`, never guess |

## Phases (slices)
| Phase | Work | Size |
|---|---|---|
| **L1** | PHP lexer (std-only) for the Tier-1 token set | S–M |
| **L2** | **PHP parser, Tier-1 subset** (typed fn sigs, classes + typed props + promotion, `enum`, `match`, `if`/`for`/`foreach`/`while`, exprs, array literals) | **L — dominant cost; rivals Phorge's own parser** |
| **L3** | Phorge AST → `.phg` **pretty-printer** (does not exist yet; the transpiler prints PHP, not Phorge) | M |
| **L4** | **Lifter**: PHP-AST → Phorge-AST. Map typed PHP 1:1; infer `List`/`Map`/`Set` from `array` usage; map `?T`→`T?`, `??`/`?->`; flag dynamic features as `// CANNOT LIFT`. | M–L |
| **L5** | **Round-trip differential gate** + confidence annotations: `lift` PHP→Phorge, `transpile` back→PHP, run BOTH PHPs on sample inputs, compare stdout. Match = evidence the lift preserved behavior. Annotate output `// lifted (verify)`. | M |
| **L6** | `phg lift` CLI + **playground "paste PHP → see Phorge" demo** | S–M |

## Contract (lock before build)
- **Review-required**: output is a draft/scaffold, never a verified equivalent.
- **Annotates confidence**: `// lifted (verify)` on lifted code; `// CANNOT LIFT: <reason>` on Tier-3.
- **Refuses Tier-3 loudly** rather than guessing.
- **Round-trip-gated** (L5) as the quality signal — confidence is *earned and visible*, like the rest
  of Phorge, not claimed.
- The Phorge type-checker validates the lifted draft: if it type-checks, it's structurally sound
  (behavior still needs review).

## Effort
~15–25 gated slices ≈ a major milestone. The PHP parser (L2) dominates. Roughly **3–4× Track 1**.
Start at L1–L3 + a thin Tier-1 lifter behind the playground demo; grow the parser incrementally.

## Dependencies / sequencing
- **After Track 1** (transpile modernization): a clean native-PHP printer makes the L5 round-trip
  comparison far easier to validate.
- L3 (Phorge printer) is independently useful (e.g. `phg fmt` could reuse it later).

## Decisions Log (build)
- [2026-06-25] AGREED: **demo angle first** (playground "paste PHP → see Phorge"). Tier-1 PHP
  subset, thin lifter, `// lifted (verify)` annotations; L5 round-trip optional this phase. Build
  L1 (PHP lexer) → L2 (Tier-1 parser) → L3 (Phorge pretty-printer) → L4 (thin lifter) → L6 (CLI +
  playground demo). Module lives at `src/lift/`.
- [2026-06-25] AGREED (reach EXPANDED, developer): **Tier-1 + Tier-2 AND attempt Tier-3** — no longer
  demo-only. Tier-2 (`array`→`List`/`Map`/`Set` inference, `?T`→`T?`, `??`/`?->`) is in scope and
  **round-trip-gated (L5)**. Tier-3 is **best-effort + loud `// LIFTED TIER-3 (unsafe — verify)`**,
  with L5 as the confidence proof; the **hard-untranslatable** core (`eval`, `$$x`, runtime magic,
  dynamic class names) still emits `// CANNOT LIFT` and never guesses. This supersedes the tier table's
  "Tier-3 → refuse" for the *attemptable* subset; the refusal stands only for the untranslatable core.
  Coordinated by [`2026-06-25-full-bidirectional-php-support.plan.md`](2026-06-25-full-bidirectional-php-support.plan.md).

## Progress
- [2026-06-25] **L1 COMPLETE** (`2f4ee27`): `src/lift/` module + std-only Tier-1 PHP lexer
  (`src/lift/lexer.rs` — `PTok` enum, `lex_php`, `PTokenSpanned` with line tracking), 7 tests green.
  Out-of-tier input (backtick, unterminated string/comment, bare `$`) → loud `lift lex error`,
  never a guess. No backend touched.
- [2026-06-26] **L2 COMPLETE** (`f5e9c73` L2a + `fb3cb06` L2b): the dominant M-Lift slice — a Tier-1
  PHP parser (`src/lift/parser.rs`) + a dedicated PHP AST (`src/lift/ast.rs`).
  - **L2a** — parser spine: typed top-level functions; full expression grammar with the **PHP-8**
    precedence table (concat `.` below `+`/`-` but above comparison — pinned by tests); postfix
    `() [] -> ?-> ::` (method vs member; static call/const/prop); primary incl. array literals, `new`,
    `match`, `true`/`false`/`null`; statements `return`/`if`-`elseif`-`else`(+`else if`)/`while`/`for`/
    `foreach`/`echo`/`break`/`continue`/block. A `depth` guard (`MAX_NEST_DEPTH`) bounds recursion on
    untrusted input. **L1 amendment:** `PTok::InterpStr` (raw) for double-quoted strings with an
    unescaped `$` (escaped `\$` excluded) → parser rejects interpolation as Tier-2 instead of silently
    lifting `"hi $name"` as literal; plus `++ -- += -= *= /= %= .= ??=` tokens (realistic for-loops).
  - **L2b** — classes (typed props + visibility + static/readonly/const + methods + abstract/final +
    `extends`/`implements` + **constructor promotion**) and PHP-8.1 enums (pure + backed cases + methods).
  - **Tier boundary = loud rejection, never a guess:** interpolated strings, casts, closures/arrow-fns,
    dynamic `new $x`/`$obj::`, array-append `[]`, `interface`/`trait`/`try`/`switch`/`namespace`/…
  - Wholly isolated — no `Op`/`Value`/checker/interpreter/VM/transpiler change; nothing outside
    `src/lift/` consumes it. 840 lib tests green (43 in the lift module), clippy + fmt clean.
- [2026-06-26] **L3 COMPLETE** (`d1a074b`): `src/lift/printer.rs` — Phorge AST → `.phg` pretty-printer
  (inverse of the PHP transpiler). Scoped to the lifter-output subset (out-of-subset node → clear
  `Err`); strings escaped (incl. `{`/`}`→`\{`/`\}`), binaries fully-parenthesized — both re-parse-safe.
  Verified by exact-output + round-trip-idempotency tests. Reusable later as `phg fmt`. 11 tests.
- [2026-06-26] **L4 COMPLETE** (`bf08b1d`): `src/lift/lifter.rs` — PHP-AST → Phorge-AST + `lift_source`
  (lex→parse→lift→print). **The ↑ PHP→Phorge direction is now end-to-end for the Tier-1 core.** Idiomatic
  mapping (top-level code → `main()`; `$x=e`→`mutable var`; `.`→`+`; `===`→`==`; `echo`→`Console.print`
  +auto-import; `__construct`→`constructor`; PHP fields→`mutable`, non-final class→`open`; array→List/Map;
  ternary→expr-`if`; match→`Expr::Match`). Loud lift-errors for the Tier-2/no-equivalent frontier
  (`array` type, instance-field default, backed enums + enum methods, **foreach** [Phorge for-in needs a
  concrete element type — `var` is VarDecl-only], default params, untyped params, elvis, assign-as-subexpr,
  non-literal match arms, main/top-level collision). End-to-end test asserts the lifted `.phg` re-parses
  as valid Phorge. 13 tests. 864 lib green, isolated.
- **NEXT = L5** — round-trip differential gate (lift PHP→Phorge, transpile back→PHP, run both under real
  PHP, compare stdout) — the behavior-preservation proof. Then L6 (`phg lift` CLI + playground "paste PHP
  → see Phorge"), then the Tier-2 build-out (`array`/foreach inference, default params, backed enums).
