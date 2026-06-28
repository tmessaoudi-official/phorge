# Phorj Examples — Full-Coverage Design

> A living set of `examples/*.phg` that demonstrate the **entire runnable language surface**,
> kept correct by the differential harness. Companion to `docs/MILESTONES.md` (M2 complete) and
> the language design (`docs/specs/2026-06-15-phorj-language-design.md`). Implementation plan:
> `docs/plans/2026-06-16-examples-coverage.md` (written after this spec is approved).

## 1. Goal & Non-Goals

**Goal.** Give a newcomer (and the maintainer) a complete, *honest*, runnable picture of what
Phorj can do today: four real-world programs, a focused per-feature guide set, and the
Phorj→PHP transpile bridge — every runnable example guaranteed byte-identical on `phg run`
and `phg runvm`, and grown incrementally as the language grows.

**Non-goals.**
- **No new language features.** Examples use only the *currently runnable* surface. Anything the
  type-checker rejects (below) is documented as future work, never faked.
- **No multi-file import example.** `import a.b.c;` parses but is a **no-op in every backend**
  (`checker.rs`/`compiler.rs`/`interpreter.rs`/`transpile.rs` all skip `Item::Import`; the prelude
  hard-codes `println`). Real module resolution is **M5**. We show the `import std.io;` line as-is
  and state plainly in the README that it is currently decorative.
- **No PHP-consumption example.** The only PHP touchpoint is `phg transpile` *producing* PHP;
  Phorj does not consume composer/PHP packages (FFI was rejected in the ecosystem roadmap). The
  "PHP ecosystem" example is therefore a Phorj→PHP *output* demo.

## 2. The runnable surface (ground truth, verified 2026-06-16)

Empirically confirmed against the built binary on both backends:

**Runnable & byte-identical:**
- Primitive types: `int`, `float`, `bool`, `string`, `void`.
- `List<T>` literals `[…]` (nestable); `for (T x in list)` iteration. (No `Map`/`Set` *values* —
  the types parse but have no literal syntax and indexing is checker-rejected.)
- Immutable typed bindings `T name = expr;`.
- Functions: typed params, `-> ret`, recursion, mutual recursion.
- Control flow: `if`/`else`, `for…in`, `return`.
- Operators: `+ - * / %`, `< > <= >= == !=`, `&& ||`, unary `- !`. Integer arithmetic is
  overflow-checked (faults cleanly, identically on both backends).
- Classes: constructor promotion (`private`/`public`), explicit fields, instance methods, `this`,
  field reads, and class-typed fields / composition (`a.inner.x` — closed in M2 Wave 4).
- Enums: variants with **0+** payload fields; exhaustive `match` with literal / binding / wildcard
  / variant patterns.
- String interpolation `"{expr}"`.
- `println(string)` — the **only** builtin.

**Checker-rejected (excluded from examples; documented as M3+):** `null`, `T?` / `Option`,
`Map`/`Set` values & indexing, `|>` pipe, exceptions (`try`/`catch`/`throw`), traits, function
overloading, sized ints, `decimal`, real `import` resolution.

**Two sharp edges baked into the examples:**
1. **Zero-payload enum variants must use call form `V()` everywhere — construction *and* match
   patterns.** As an expression, bare `Defend` is parsed as an identifier and fails ("unknown
   identifier"); `Defend()` constructs it. As a **match pattern**, bare `Defend =>` is parsed as a
   *catch-all binding* (it silently swallows every scrutinee — a logic bug both backends agree on,
   so the differential test cannot catch it); `Defend() =>` is the variant pattern. Every example
   uses `V()` in both positions.
2. **`is` is omitted.** It is implemented as deep value-equality (`l.eq_val(&r)`) — a confusing
   alias for `==` — so featuring it would mislead. Excluded by choice.

## 3. Layout

```
examples/
  hello.phg  fib.phg  grades.phg     # unchanged — kept where tests/cli.rs + differential.rs reference them
  README.md                          # NEW: index + coverage matrix + honest import/PHP notes
  realworld/                         # four complete, relatable programs
    ledger.phg     # bank accounts + transactions
    library.phg    # catalogue + availability
    shop.phg       # cart + discounts
    rpg.phg        # party + combat actions
  guide/                             # focused, one feature-cluster each
    enums-match.phg classes.phg collections.phg operators.phg control-flow.phg strings.phg
  transpile/                         # the Phorj → PHP bridge (the real "PHP ecosystem" path)
    demo.phg  demo.php  README.md
```

Existing examples are **not moved** (they are referenced by explicit path in `tests/cli.rs` and
`tests/differential.rs`). New ones live in subdirectories for navigability.

## 4. Correctness mechanism — glob the sweep

`tests/differential.rs` currently lists examples by explicit path. Replace the example portion with
a **glob over `examples/**/*.phg`** that runs `agree()` (Ok-path byte-identity) on each file found.

- **Why glob:** directly serves "add examples as we go" — a new `.phg` is byte-identity-gated the
  instant it lands, with **no test edit**. A future example that diverges between backends fails the
  suite loudly.
- **Scope of the glob:** only `*.phg` (so `transpile/demo.php`, a generated artifact, is not picked
  up as a program). Every `.phg` under `examples/` is expected to be an Ok-path, byte-identical
  program — no intentionally-faulting example lives here (those belong to the `agree_err` tables).
- The existing explicit `fib`/`grades`/`hello` reads can be dropped in favor of the glob, or left
  as harmless redundancy; the plan picks one (prefer: drop, glob is the single source).

## 5. Real-world examples (the four)

Each is a small, complete program that reads like real code and exercises most of the surface.

- **`realworld/ledger.phg`** — an `Account` class (methods over an integer-cents balance, exact
  money), an enum `Tx { Deposit(int cents), Withdraw(int cents), Transfer(int cents, string to) }`
  matched to apply/describe, a `List<Tx>` log iterated with `for…in`, a recursive
  compound-interest helper (int arithmetic), `if`/`else` for overdraft handling.
- **`realworld/library.phg`** — a `Book` class, enum `Availability { Available, Borrowed(string by),
  Lost }` (zero-payload `Available()`/`Lost()` + payload `Borrowed`), matched to a status line, a
  `List<Book>` catalogue iterated, float late-fee arithmetic.
- **`realworld/shop.phg`** — `Item` + `Cart` classes (composition), enum `Discount { None,
  Percent(int), Flat(int) }` matched to compute a line price, `List<Item>`, subtotal/total
  arithmetic, a recursive helper (e.g. bundle expansion or running total).
- **`realworld/rpg.phg`** — a `Character` class (HP/attack methods, `this`), enum `Action {
  Attack(int), Heal(int), Defend }` matched to resolve a turn, a `List<Character>` party iterated,
  HP arithmetic with `if`/`else` for clamping/KO.

## 6. Guide (focused) examples

Small and didactic — one feature cluster each, heavily commented:
- `guide/operators.phg` — int/float arithmetic, `%`, comparison, logical, unary, overflow note.
- `guide/control-flow.phg` — `if`/`else`, `for…in`, recursion + mutual recursion.
- `guide/collections.phg` — `List<T>` literals, nesting, iteration, list of instances.
- `guide/classes.phg` — ctor promotion, explicit fields, methods, `this`, composition.
- `guide/enums-match.phg` — payload + zero-payload variants, all four pattern kinds, match as an
  expression.
- `guide/strings.phg` — interpolation with expressions, nested calls, numbers.

## 7. Transpile / PHP example

- `transpile/demo.phg` — a compact program (a class + an enum + a function) that is **also** in the
  byte-identity sweep (it is a normal runnable program).
- `transpile/demo.php` — the committed output of `phg transpile demo.phg`.
- `transpile/README.md` — shows `phg transpile examples/transpile/demo.phg > demo.php` and how to
  run the result under PHP 8.x; states this is the *only* PHP-ecosystem path (output, not input).
- **Snapshot test:** a test (in `tests/cli.rs` or a new `tests/transpile_examples.rs`) regenerates
  the PHP from `demo.phg` and asserts it equals the committed `demo.php`, so transpiler drift fails
  the suite. Actual PHP execution is gated on `php` being available and is otherwise a README step
  (mirrors the existing `transpile_*` CLI tests).

## 8. README index

`examples/README.md`: a one-line index of every example, the §3 coverage matrix, the explicit list
of M3+ not-yet-supported features, and the honest `import`/PHP notes from §1. This is the "what can
Phorj do today" page and is updated whenever an example is added (the "as we go" contract).

## 9. Build order

Three self-contained, green commits (`cargo test` + clippy + fmt clean each):
- **Wave A:** glob the differential sweep + the four `realworld/` examples.
- **Wave B:** the six `guide/` examples.
- **Wave C:** `transpile/` (demo.phg + demo.php + snapshot test + README) + `examples/README.md`
  index + a CHANGELOG entry + a `docs/MILESTONES.md`/`CLAUDE.md` pointer to the example set.

## 10. Success criteria

1. Every `.phg` under `examples/` runs byte-identically on `run` and `runvm` (glob sweep green).
2. The coverage matrix in `examples/README.md` maps every runnable feature to ≥1 example, and lists
   excluded features honestly.
3. `transpile/demo.php` matches freshly-generated output (snapshot test green).
4. `cargo test` green, `cargo clippy --all-targets` clean, `cargo fmt --check` clean.

## 11. Decisions Log

| # | Decision | Choice | Rationale |
|---|---|---|---|
| EX-1 | Import-files examples | **Not written**; `import` documented as decorative (M5) | Real resolution is a no-op today; a working example is impossible — faking it would lie |
| EX-2 | PHP-ecosystem example | A Phorj→PHP **transpile** demo (output), not PHP consumption | The only PHP path is `phg transpile`; FFI/consumption was rejected in the roadmap |
| EX-3 | Real-world domains | **All four** (ledger, library, shop, rpg) | User chose all four; each stresses a different mix of the same surface |
| EX-4 | Sweep mechanism | **Glob `examples/**/*.phg`** into the differential harness | "Add as we go" needs zero test edits; divergence fails loudly |
| EX-5 | Layout | Keep `hello`/`fib`/`grades` flat; new ones under `realworld/`/`guide/`/`transpile/` | Don't break explicit test paths; subdirs aid navigation at this scale |
| EX-6 | Zero-payload variants | Construct with call form `V()` | Bare `V` is an identifier → "unknown identifier"; verified on both backends |
| EX-7 | `is` operator | **Omitted** from examples | Implemented as deep `==`; featuring it would mislead |
