# Agent L — Linter-Catalog Enforcement Sweep, Batch 3

> Swept: rust-clippy (correctness + suspicious), PHPStan (L5–10), ESLint (recommended + unicorn
> correctness), detekt (potential-bugs); honorary: Go vet, SwiftLint analyzer, Roslyn CAxxxx.
> Filter: always-wrong-code · checker-only (no Op/Value/type-system change) · zero plausible false
> positives · not among Phorj's 198 existing codes (grepped from `src/checker/` + `src/cli/explain*`)
> nor the ten adopted batch-2 rules · applicable to Phorj's real surface.
>
> Calibration: syntax verified against `examples/guide/*.phg` (`function f(): T`, `new V()`,
> `mutable`-by-opt-in locals, `throws`/try/catch/finally, property hooks, `#[Route]`, `r"…"` raw
> strings, `discard`, modules `Core.Output`/`Core.Math`/`Core.List`/`Core.String`/`Core.Regex`).
> Live-run verifications were made against `target/release/phg` (noted [Verified] inline).
> All snippets below assume the `package Main; import Core.Output;` preamble unless shown.

**Kept: 26** — dead code 6 · always-wrong logic 7 · suspicious equality/numerics 7 · error
handling 2 · API misuse 4 · concurrency 0 (see note). **Rejected: 16** famous rules (with the
false-positive story each).

---

## 1. Dead code

| Proposed code | Source | Minimal trigger | What it catches | Why zero-FP |
|---|---|---|---|---|
| `W-UNUSED-IMPORT` | Go compiler ("imported and not used" — a hard error there) / SwiftLint `unused_import` | `import Core.Math;` in a file that never writes `Math.` | An import whose qualifier has zero use sites | Imports are load-bearing qualifiers (Wave 1 — resolution is import-driven); Core.Reflect exposes only kind/className/typeName, so no dynamic access path can consume an import. Unused = provably dead. |
| `W-UNUSED-LOCAL` | ESLint `no-unused-vars` (recommended) / clippy `unused_variables` | `function main(): void { var x = 7; Output.printLine("hi"); }` | A local binding declared (initializer still runs) but never read | Phorj already has `discard <expr>;` for "effect only, no name" — so a named-but-never-read binding is always leftover code, never an idiom. No `_`-prefix convention needed. |
| `W-UNUSED-PRIVATE-MEMBER` | ESLint `no-unused-private-class-members` (recommended) / detekt `UnusedPrivateMember` / PHPStan | `class A { constructor(private int v) {} function f(): int { return 1; } }` — `v` never read | A `private` field or method with zero references inside its own class | Privates are enforced at all six access surfaces (member-visibility milestone) and reflection cannot reach them — inside-the-class reference count is the complete truth. |
| `W-UNUSED-DECLARATION` | SwiftLint analyzer `unused_declaration` | `function helper(): int { return 1; }` never referenced; `main` doesn't call it | A `package Main` function/class/enum unreachable from `main` (excluding `main` itself, `#[Route]`-attributed handlers, and `test"…" { }` blocks) | A Main-package program is a closed world with one entry point; after excluding attribute-registered and test items, unreferenced = unreachable, mechanically. Library packages (public API) are out of scope by construction. |
| `W-LOOP-NEVER-LOOPS` | clippy `never_loop` (correctness) / detekt `UnconditionalJumpStatementInLoop` | `while (i < 5) { Output.printLine("once"); break; }` | A loop whose body terminates the loop on **every** path in the first iteration — it is an `if` wearing a loop costume | Reuses the shipped totality/terminator engine (`block_terminates`) with `break` counted as a terminator; "cannot reach a second iteration" is a structural fact independent of input. |
| `W-RANGE-EMPTY-CONST` | detekt `InvalidRange` | `for (int i in 5..1) { Output.printLine("{i}"); }` | A range with both bounds literal and lo > hi — materializes an empty `List<int>`, the body never runs | Ranges are ascending-only by definition; with two integer literals the emptiness is decidable at check time with no analysis at all. |

## 2. Always-wrong logic

| Proposed code | Source | Minimal trigger | What it catches | Why zero-FP |
|---|---|---|---|---|
| `E-ALMOST-SWAPPED` | clippy `almost_swapped` (correctness, deny-by-default) | `mutable int a = 1; mutable int b = 2; a = b; b = a;` | The failed-swap pair: after `a = b`, the second statement copies the already-overwritten value back — the pair is exactly equivalent to `a = b;` alone | The two statements are adjacent, operate on the same two names, and the second is provably a self-assign-in-effect; no reading of the pair does anything a single assignment doesn't. The author meant a swap. |
| `E-LOOP-IMMUTABLE-CONDITION` | clippy `while_immutable_condition` / ESLint `no-unmodified-loop-condition` | `int flag = 1; while (flag == 1) { Output.printLine("tick"); }` | A `while` whose condition reads only **immutable** locals (no calls, no fields) and whose body contains no `break`/`return`/`throw` — the loop runs zero times or forever | Phorj's immutable-by-default makes loop-invariance a *checked fact*, not a heuristic: an immutable local has no alias, no concurrent writer (closures capture by value), and cannot be reassigned. Nothing can change the condition between iterations. |
| `E-UNCONDITIONAL-RECURSION` | clippy `unconditional_recursion` | `function f(int n): int { return f(n - 1); }` | A function where **every** execution path reaches an unguarded self-call before returning | No conditional path avoids the self-call ⇒ there is no base case by construction; the only possible runtime outcome is the recursion-depth fault. Same conservative path engine as `E-MISSING-RETURN`. |
| `W-INSTANCEOF-REDUNDANT` | PHPStan ("instanceof … will always evaluate to true") | `function f(Dog d): bool { return d instanceof Dog; }` | `x instanceof T` where x's static type is already T or a nominal subtype — the test is constant `true` | A non-optional `T` can never hold `null` (S2 guarantee) and subtyping is nominal + fully known at check time; there exists no runtime value in that slot for which the test fails. |
| `W-CONTRADICTORY-CONDITION` | Go vet `bools` | `if (x == 1 && x == 2) { … }` · dual: `if (x != 1 || x != 2) { … }` | The same immutable operand compared against two *distinct* constants joined the wrong way — always-false (`==`/`&&`) or always-true (`!=`/`\|\|`); the author meant the other connective | Operand syntactically identical + immutable (no reassignment between evaluation), constants provably distinct — no evaluation order or side effect can make both/neither hold. |
| `W-UNION-SUBSUMED` | PHPStan (redundant union/instanceof type) | `interface Shape {}` `class Circle implements Shape {}` … `function f(Circle \| Shape s): void {}` — also `catch (NegativeInput \| Error e)` | A union member (in a type position or a `catch (A \| B e)` clause) that is a subtype of another member — it contributes nothing | Nominal subtyping is closed and known at check time; `A \| B` with A ⊆ B is semantically identical to `B` in every assignability, narrowing, and dispatch context. Pure redundancy, harmless but always a misunderstanding. |
| `E-CAST-IMPOSSIBLE` | detekt `CastNeverSucceeds` / PHPStan (impossible type) | `class Dog {}` `class Cat {}` … `var c = new Dog() as Cat;` | An `as` cast between provably disjoint types — the checked cast can only ever produce `null` | Final-by-default (S6) closes the hierarchy: two class types with no subtype path and no shared open descendant cannot classify one instance. The batch-2 precedent `E-INSTANCEOF-IMPOSSIBLE` is this rule's boolean twin; this is the `T?`-producing twin. |

## 3. Suspicious equality & numerics

| Proposed code | Source | Minimal trigger | What it catches | Why zero-FP |
|---|---|---|---|---|
| `E-MAP-DUP-KEY` | ESLint `no-dupe-keys` (recommended) | `Map<string,int> m = ["a" => 1, "a" => 2];` | A duplicate **constant** key in a map literal — one value is silently discarded at construction. **[Verified: ran it — compiles clean, prints `2`; the `1` is silently dropped (PHP last-value-wins semantics in `value::build_map`)]** | Two values written for one literal key can never both be intended; both keys are compile-time constants (`int`/`bool`/`string` — the `HKey` subset), so equality is decidable with zero analysis. Non-literal keys are simply not checked. |
| `E-SHIFT-RANGE-CONST` | clippy (`shift` overflow lints, correctness) | `Output.printLine("{x << 65}");` | A constant shift count outside `0..=63`: ≥ 64 erases the operand **[Verified: `x << 65` prints `0` for `x = 5`]**; a negative count is a runtime fault [Inferred: `value::int_shl` returns `Result` and handles the ≥ 64 case explicitly per its doc comment] | `int` is exactly 64-bit; for every possible operand a count outside the bit width produces either a constant or a fault — there is no input for which the expression is meaningful. |
| `W-ERASING-OP` | clippy `erasing_op` (correctness) | `var y = x * 0;` · `var y = x & 0;` | An arithmetic/bitwise expression whose result is a constant regardless of the non-constant operand | `* 0` and `& 0` yield `0` for **all** x by algebra; naming the operand is pure noise — the author either meant a different constant or a different operator. |
| `W-MODULO-ONE` | clippy `modulo_one` | `var r = n % 1;` | Modulo by constant `1` (or `-1`) — always `0` for every n | Mathematically constant; in practice a mangled parity check (`% 2`). No integer input produces a non-zero result. |
| `E-BIT-MASK-IMPOSSIBLE` | clippy `bad_bit_mask` (correctness) / Go vet | `if ((flags & 4) == 3) { … }` | A mask-then-compare where the compared constant has bits **outside** the mask — `(x & C1) == C2` with `C2 & ~C1 != 0` is always false | Pure bit arithmetic over two literals; no type, alias, or ordering caveat exists. Same always-false family as the adopted `E-INSTANCEOF-IMPOSSIBLE`. |
| `W-LITERAL-PRECISION-LOSS` | ESLint `no-loss-of-precision` (recommended) | `float f = 9007199254740993.0;` | A float literal whose written digits do not survive the f64 round-trip — the program contains a number the author did not write (stored: `…992`) | The round-trip check (parse → print → compare digits) is exact and mechanical; when it fails, the stored value *provably* differs from the source text. Natural fix-it: point at `decimal`, Phorj's exact type. |
| `W-INVERTED-CLAMP` | clippy `min_max` (correctness) | `var c = Math.min(Math.max(x, 100), 0);` | A min/max clamp whose constant bounds are inverted (lo > hi) — the result is a constant, x is ignored | With both bounds literal and hi < lo the composition degenerates by algebra (here: always `0`). `Math.min`/`Math.max` verified present (`examples/guide/math.phg`). |

## 4. Error handling

| Proposed code | Source | Minimal trigger | What it catches | Why zero-FP |
|---|---|---|---|---|
| `E-FINALLY-CONTROL-FLOW` | ESLint `no-unsafe-finally` (recommended) / Roslyn CA2219 | `function f(): int { try { return 1; } finally { return 2; } }` | A `return`/`throw`/`break`/`continue` **inside `finally`** — it silently overrides the in-flight return value or swallows the in-flight exception. **[Verified: ran it — accepted today, prints `2`; the try's `return 1` is silently lost]** | `finally` exists to run cleanup on *every* exit path; a control-flow statement there always cancels an outcome established elsewhere. There is no program where that is the clear expression of intent — every linter family carries this rule, none reports FPs. |
| `W-USELESS-COALESCE-NULL` | detekt `RedundantElvisExpression` | `int? y = opt ?? null;` | `?? null` — coalescing absence into the same absence; the expression equals `opt` for every value | The RHS is the literal `null`; substitution is identity by the semantics of `??`. Complements the adopted `W-USELESS-COALESCE`, which fires on a never-null *left* side — this is the degenerate *right* side. |

## 5. API misuse

| Proposed code | Source | Minimal trigger | What it catches | Why zero-FP |
|---|---|---|---|---|
| `E-REGEX-INVALID-CONST` | clippy `invalid_regex` (correctness) | `import Core.Regex;` … `var re = Regex.compile(r"(unclosed");` | A **literal** pattern that `Regex.compile` is guaranteed to reject — syntax error, backreference, or lookaround (the engine refuses those by design) | The checker can run the *same* engine validation on the literal at check time — same code path, same verdict, divergence impossible by construction. Non-literal patterns are not checked. [Inferred: `Regex.compile` rejects backrefs/lookaround per the `examples/guide/regex.phg` header] |
| `E-ROUTE-DUPLICATE` | ASP.NET Core route analyzer (Roslyn family); no PHP/JS twin because their routers are runtime | Two static handlers with `#[Route("GET", "/x")]` | Two `#[Route]` attributes with identical method + path — exactly one handler can ever be dispatched; the other is silently dead | Route specs are string literals (already validated by `E-ROUTE-SPEC`), so spec equality is a compile-time fact; the router matches exactly one handler. **[Verified: today's checker validates arity/spec/handler shape but has no duplicate check — grep of `checker/desugar_router.rs` + `checker/program.rs`]** |
| `E-HOOK-SELF-RECURSION` | Roslyn CA2011 ("Avoid infinite recursion" in property accessors) | `class A { constructor(public mutable int raw) {} int balance { get => this.balance; } }` | A property hook whose body reads (in `get`) or assigns (in `set`) the hooked property **itself** | Hooks are *virtual* — they have no backing storage (`property-hooks.phg`: "no storage of its own"), so self-access can only re-enter the same hook: unconditional infinite recursion, no configuration where it terminates. |
| `E-INDEX-OOB-CONST` | clippy `out_of_bounds_indexing` (correctness) | `var x = [1, 2, 3][5];` | A constant index into a list **literal** of known length — a guaranteed `list index out of range` fault | Both length and index are compile-time facts of the same expression; no dataflow needed. Complement of the existing `E-FIXEDLIST-BOUNDS`, which covers `FixedList` types only, not plain list literals. |

## 6. Concurrency

**Zero rules kept — deliberately.** The famous concurrency lints all fail the filter for Phorj:
Go vet `loopclosure` is *structurally impossible* here (lambdas capture enclosing locals **by
value**, so the loop-variable-capture bug cannot be written); channel-deadlock detection
("receive on a channel never sent to") needs whole-program escape analysis and false-positives
the moment a channel value is passed to a function; `join`-twice / send-after-close rules are
runtime-state-dependent, not syntactic. The existing `E-SPAWN-*`/`E-CHANNEL-*`/`E-CONCURRENCY-*`
codes already cover the statically decidable surface.

---

## REJECT list — famous rules that FAIL the filter

| Rule | Source | Why rejected (the false-positive story) |
|---|---|---|
| Float equality `==` ban | clippy `float_cmp` | `x == 0.0` sentinel checks and exactly-representable comparisons are legitimate — Phorj's own guide examples deliberately compare exactly-representable floats (KNOWN_ISSUES policy). Not always-wrong. |
| Variable shadowing | Go vet `-shadow` | Shadowing is *idiomatic* in Phorj: if-let (`if (var x = opt)`) and smart-cast rebinding shadow by design. Vet itself ships this off-by-default because of FP volume. |
| Unused **parameter** | ESLint `no-unused-vars` (args) / PHPStan | Interface implementations, overrides (`E-OVERRIDE-SIG` forces exact match), and lambdas passed to `List.map` have contract-fixed signatures — an unused param there is *required*, not wrong. (Unused *local* kept; unused *param* rejected.) |
| Empty block | ESLint `no-empty` | An empty `else` or placeholder block can be deliberate documentation of a considered-and-ignored case. Dead-but-intentional ⇒ fails always-wrong. |
| `require-atomic-updates` | ESLint (recommended!) | A famous FP factory even in its home ecosystem (whole GitHub threads of apologies). Phorj green threads are cooperative, so the interleaving model that motivates it barely applies. |
| Redundant `else`/catch-all in exhaustive `match` | detekt `ElseCaseInsteadOfExhaustiveWhen` | A catch-all arm over an exhaustive variant set can be deliberate forward-compat (a new variant added later should hit the default, not a compile error). Opinion, not defect. |
| Leading-zero integer literal (`010`) | ESLint `no-octal` (analogue) | Phorj parses `010` as decimal 10 (octal is `0o10`), so a PHP dev *may* be surprised — but zero-padding for visual column alignment is a plausible legitimate use. Fails zero-FP; a fmt concern at most. |
| Loop-variable capture | Go vet `loopclosure` | Structurally impossible: Phorj lambdas capture **by value**. Nothing to lint. |
| Possibly-undefined variable | PHPStan level 1-ish | Phorj already enforces definite assignment (declaration-with-initializer) and immutability; the checker rejects the underlying shape natively. |
| Ban all indexing | clippy `indexing_slicing` | Restriction-tier lint: flags every `xs[i]` including provably-safe ones. Drowning FP rate by design. |
| Rethrow losing stack trace | Roslyn CA2200 (`throw e;` vs `throw;`) | Phorj has no bare-rethrow form and traces are attributed by the runtime (Slice 1); the C# footgun doesn't exist here. |
| Formatting-shape heuristics | clippy `suspicious_else_formatting`, `possible_missing_comma` | Pure layout heuristics with documented FPs; in Phorj layout is `phg fmt`'s jurisdiction, not the checker's. |
| Assignment in condition | ESLint `no-cond-assign` | Assignment is not an expression in Phorj, and the one condition-binding form is the dedicated if-let syntax. Already unwritable. |
| Force-unwrap bans | SwiftLint `force_unwrapping` / detekt `MapGetWithNotNullAssertionOperator` | Already covered wholesale: `W-FORCE-UNWRAP` lints **every** `!` use. |
| Truthiness / strict-bool rules | PHPStan strict-rules, ESLint `eqeqeq` family | Phorj conditions are already `bool`-typed only and `==` is type-checked; the loose-comparison surface these guard doesn't exist. |
| NaN comparison | ESLint `use-isnan` / clippy `cmp_nan` | No NaN literal or constant surfaced in Phorj's guide examples; without a way to *write* the comparison the rule has no trigger. Revisit only if `Math.nan` ships. |

## Dedupe notes (rules that map onto existing/adopted codes — not re-proposed)

- Go vet `unusedresult` → existing `E-UNUSED-VALUE` (must-use + `discard`).
- Go vet `unreachable` / detekt `UnreachableCode` → existing `W-UNREACHABLE`.
- Go vet `assign` / clippy `self_assignment` → adopted `W-SELF-ASSIGN`; clippy `eq_op` → adopted `W-SELF-COMPARE`.
- clippy `if_same_then_else` → adopted `W-IDENTICAL-BRANCHES`; `ifs_same_cond` / ESLint `no-dupe-else-if` → adopted `W-DUPLICATE-CONDITION`.
- PHPStan "instanceof always false" → adopted `E-INSTANCEOF-IMPOSSIBLE` (the always-**true** side is newly proposed above as `W-INSTANCEOF-REDUNDANT`).
- PHPStan dead-catch → existing `W-CATCH-NEVER-THROWN`; subsumed-catch-order → existing `W-CATCH-UNREACHABLE`.
- ESLint `no-duplicate-case` / clippy `match_same_arms` (dup side) → existing `W-MATCH-UNREACHABLE`.
- clippy `zero_divided_by_zero` / PHPStan div-by-zero-const → adopted `E-DIV-ZERO-CONST`; overflowing literals → adopted `E-CONST-OVERFLOW`.
- ESLint `no-constant-condition` → adopted `W-CONST-CONDITION`; `no-constant-binary-expression` (the `??`-never-null half) → adopted `W-USELESS-COALESCE` / `W-USELESS-SAFE`.
- detekt `EmptyCatchBlock` → adopted `W-EMPTY-CATCH`; PHPStan "throws never thrown" → adopted `W-THROWS-NEVER`.

## Implementation notes

- Every kept rule is front-end-only: literal/const facts (`E-MAP-DUP-KEY`, `E-SHIFT-RANGE-CONST`, `E-BIT-MASK-IMPOSSIBLE`, `E-INDEX-OOB-CONST`, `W-RANGE-EMPTY-CONST`, `W-LITERAL-PRECISION-LOSS`, `W-INVERTED-CLAMP`, `E-REGEX-INVALID-CONST`, `E-ROUTE-DUPLICATE`), the existing totality/terminator engine (`W-LOOP-NEVER-LOOPS`, `E-UNCONDITIONAL-RECURSION`, `E-FINALLY-CONTROL-FLOW`), the existing nominal-subtype oracle (`W-INSTANCEOF-REDUNDANT`, `W-UNION-SUBSUMED`, `E-CAST-IMPOSSIBLE`), reference counting over resolved names (`W-UNUSED-*`), or single-statement syntax (`E-ALMOST-SWAPPED`, `W-ERASING-OP`, `W-MODULO-ONE`, `W-USELESS-COALESCE-NULL`, `E-HOOK-SELF-RECURSION`, `W-CONTRADICTORY-CONDITION`, `E-LOOP-IMMUTABLE-CONDITION`). No new `Op`, no `Value` change, no `Ty` extension.
- Two rows carry live verified repros against today's binary: `E-FINALLY-CONTROL-FLOW` (accepted, prints `2`, try's return silently lost) and `E-MAP-DUP-KEY` (accepted, prints `2`, first value silently dropped). These are the highest-urgency items — both are *silent data loss* shapes shipping today.
- E/W split follows the commissioned convention: E-* where the flagged code misleads at runtime (wrong value, fault, hang, dead handler), W-* where it is dead-but-harmless redundancy. Borderline calls flagged for the developer: `W-LITERAL-PRECISION-LOSS` and `W-INVERTED-CLAMP` produce *wrong values* and could be argued up to E.

