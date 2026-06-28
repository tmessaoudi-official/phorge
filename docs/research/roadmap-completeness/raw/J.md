# Track J — Semantics edge cases

## Track summary

Phorj has already locked the *hard-to-undo* semantic decisions in a way that is faithful to the
philosophy (a legible, provably-correct upgrade of PHP): equality is **structural** `==` only (no loose
`==` juggling — PHP's single largest footgun is gone by construction, since values are statically typed
and never coerced), floats render Rust-shortest-round-trip identically across all three backends, maps
and sets compare order-independently like PHP associative arrays, `eq_val` is cycle-guarded so `==` on a
cyclic instance graph terminates, and `HKey` pins the hashable key set to `int`/`bool`/`string`. What
remains in this domain is a cluster of **small, individually-decidable semantic rules** that a PHP dev
will eventually hit and ask "what does Phorj do here?" — and right now several have no answer because
the construct is simply *rejected* (`compare_ord` only accepts `int`/`int` and `float`/`float`; `for…in`
only walks a materialized `List`; there is no `<=>`, no `===`, no string ordering, no user-type
iteration, no operator overloading). The most valuable gaps here are the ones that **remove a surprise**
(define string/cross-type ordering, define a stable sort, settle the identity question, settle the
numeric-tower coercion rule) rather than the ones that **add power** (operator overloading, custom
iterators) — PL-theory maximalism is mostly *reject* or *defer* under this lens. The single biggest
correctness landmine still open is the **iteration protocol over user types**: M6 concurrency, `core.list`
breadth, and any `Map`/`Set`/`Range` `foreach` all silently depend on it, and it has no design yet.

## Gaps

| id | title | kind | fit | rec | milestone | effort |
|---|---|---|---|---|---|---|
| J-iter-protocol | Iteration protocol over user types (`foreach` over Map/Set/Range/instances) | port | strong | adopt | M11 | L |
| J-ordering-rules | Total ordering rules: string `<`, cross-type compare, enum/bool ordering | port | strong | adopt | M-RT | M |
| J-spaceship | Spaceship `<=>` operator | port | strong | adopt | M-RT | S |
| J-sort-stability | Sort/`usort` semantics + documented stability + comparator contract | port | strong | adopt | M11 | M |
| J-identity-eq | Identity `===` (`Rc::ptr_eq`) decision — adopt or formally reject | omit | ok | defer | M-mut follow-up | S |
| J-float-eq-lint | Float `==` exactness lint (`W-FLOAT-EQ`) | new | strong | adopt | M-RT | S |
| J-numeric-tower | Numeric tower: int↔float coercion rule in arithmetic/compare | port | strong | adopt | M-RT | M |
| J-int-width | Integer width + overflow semantics doc (i64 fixed, `decimal` deferred) | defer | ok | defer | v2 | M |
| J-unicode-model | Unicode / string encoding model (byte vs codepoint vs grapheme) | port | strong | adopt | M11 | M |
| J-string-natives-unicode | `Core.Text` length/index unicode semantics + `mb_*` policy | port | strong | adopt | M11 | M |
| J-op-overload | Operator overloading (`+`/`==`/`<=>` on user types) | new | weak | reject | — | L |
| J-collection-eq-doc | Collection equality semantics — document order-independence as a contract | map | strong | adopt | M-RT | S |
| J-mutation-aliasing-doc | Mutation + aliasing semantics doc (value-COW vs instance-handle) | map | strong | adopt | M-mut follow-up | S |
| J-hash-contract | Hash/key-identity contract for Map/Set keys (and future user keys) | port | ok | defer | M11 | M |
| J-bool-coercion | Truthiness / boolean coercion rule (no implicit `if (x)` on non-bool) | omit | strong | adopt | M-RT | S |
| J-nan-ordering | NaN ordering + sort placement decision (currently `false` everywhere) | port | ok | adopt | M11 | S |
| J-enum-ordinal | Enum ordering / ordinal / comparison decision | omit | ok | defer | M-RT | S |
| J-string-coerce-interp | Interpolation/`toString` coercion contract (`Stringable` analog) | port | ok | defer | M11 | M |

## Rationale per ADOPT item

**J-iter-protocol — Iteration protocol over user types.** Today `for…in` desugars to a counter loop over
an *inline materialized `List`* (compiler.rs `compile_for`); `foreach` over a `Map`, `Set`, `Range`
(without materializing), or a user class is structurally impossible. PHP devs reach for
`foreach ($map as $k => $v)` and `IteratorAggregate`/`Iterator` constantly — this is the single most
load-bearing missing semantic. The philosophy-fit form is **not** PHP's stateful `Iterator` interface
(current/next/valid/rewind is a surprise-laden contract) but a *legible* one: a built-in protocol the
checker recognizes (e.g. an `Iterable<T>` interface emitting an `iter()` that yields, transpiling to PHP
`IteratorAggregate::getIterator` + `yield`). Map/Set iteration and `core.list` breadth both block on it;
adopt at M11 (stdlib breadth) where `Map`/`Set` already live.

**J-ordering-rules — Total ordering rules.** `compare_ord` rejects everything except `int`/`int` and
`float`/`float` — `"a" < "b"` is a *type error today*. A PHP dev expects string ordering (and `sort()`
ordering them). Define: string `<` is byte-lexicographic (matches PHP `strcmp` and Rust `str` `Ord`,
byte-identical), cross-type `int`/`float` mixed comparison follows the numeric-tower rule (J-numeric-tower),
and bool/enum get an explicit decision (J-bool, J-enum-ordinal). This removes a surprise (a PHP dev's
`$a < $b` on strings just works) without adding capability — strong fit, and it is a prerequisite for any
generic `sort`.

**J-spaceship — Spaceship `<=>`.** PHP's three-way operator is idiomatic in every comparator
(`usort($a, fn($x,$y) => $x <=> $y)`). It maps 1:1 to PHP `<=>` and to `compare_ord`'s `Ordering` (which
already exists internally — this is almost pure surface). Returns `int` (`-1`/`0`/`1`). Low effort, high
familiarity, pairs with J-sort-stability. Adopt within M-RT alongside the ordering rules.

**J-sort-stability — Sort semantics + stability + comparator contract.** Once `core.list` lands a `sort`
(M11), the *stability* and the comparator-contract semantics must be locked and documented: PHP `sort` is
**not** stable before 8.0 and **is** stable from 8.0 on — Phorj should commit to **stable** (Rust's
`sort_by` is stable; matches modern PHP) and document the comparator must be a consistent total order.
This is a semantic decision that is painful to change after code depends on it. Strong fit (removes the
"is my sort stable?" surprise).

**J-float-eq-lint — Float `==` exactness lint.** `eq_val` compares floats with raw IEEE `==`
(`#[allow(clippy::float_cmp)]`), which is correct but a classic footgun (`0.1 + 0.2 == 0.3` is `false`).
Phorj already has a warning channel (`W-FORCE-UNWRAP` precedent); a `W-FLOAT-EQ` lint on a literal-float
`==`/`!=` removes the surprise *without* changing semantics (the comparison still works, byte-identical
to PHP). Cheap, strong-fit, ships in the existing warning infra.

**J-numeric-tower — int↔float coercion rule.** `compare_ord` *errors* on `int` vs `float`
(`compare_ord(Int, Float)` → `Err`), and arithmetic kernels are split int-only/float-only. PHP silently
widens `1 + 1.5` to float. Phorj's static-type stance means this must be an *explicit, documented rule*:
either require an explicit cast (most legible, matches "no surprises") or auto-widen `int`→`float` in
mixed arithmetic/comparison like PHP (most familiar). The decision is load-bearing for the whole numeric
surface and is currently *implicit by rejection*. Adopt the rule (recommend: auto-widen to float in mixed
ops, matching PHP, with the result typed `float`) and document it in INVARIANTS.

**J-unicode-model — Unicode / string encoding model.** Phorj has `string` (declared UTF-8) and `bytes`
(octets) — but the *semantic model* of `string` operations is unstated: is `Core.Text.len` codepoints,
bytes, or graphemes? PHP `strlen` is bytes; `mb_strlen` is codepoints. KNOWN_ISSUES already records the
"`php -n` has no mbstring → use tier-1 (PCRE not mbstring)" constraint, which *forces* a byte model for
the transpile leg today. Lock the model explicitly: `string` ops are **byte-oriented to match PHP's
default + the oracle constraint**, with codepoint/grapheme ops as a documented future `Core.Text` addition.
This removes a real surprise (indexing/length on multibyte text) and is required reading before any string
stdlib breadth.

**J-string-natives-unicode — `Core.Text` unicode semantics + `mb_*` policy.** Concrete follow-on to
J-unicode-model: `Core.Text` `len`/`upper`/`lower`/`split` currently transpile to byte-oriented PHP
(`strlen`/`strtoupper`), which is wrong for non-ASCII and silently diverges from any future codepoint
expectation. Document each native's exact semantics (byte vs codepoint) and the policy that multibyte
variants require the tier-3 extension mechanism (already designed in the extension-policy spec). Adopt at
M11 with the stdlib breadth work.

**J-collection-eq-doc — Collection equality as a contract.** `eq_val` already does the right thing
(List = positional, Map/Set = order-independent, matching PHP associative `==`), but it is an *emergent
implementation fact*, not a *documented language guarantee*. Promote it to INVARIANTS/FEATURES as an
explicit contract so a user can rely on `[1=>"a", 2=>"b"] == [2=>"b", 1=>"a"]`. Pure documentation of
shipped behavior — strong fit, near-zero effort.

**J-mutation-aliasing-doc — Mutation + aliasing semantics doc.** Mutation shipped (M-mut closed) with a
subtle value/handle split: `List`/`Map`/`Set`/`Bytes` are copy-on-write *value types* (assignment copies
semantically), `Instance` is a *shared-mutable handle* (assignment aliases). This is the single most
behavior-defining rule a user must understand to predict their program, and it is currently buried in the
mutation spec + CLAUDE.md. Surface it as a first-class FEATURES/guide section (with a runnable
`examples/guide/aliasing.phg`). Shipped behavior — documentation gap only, strong fit.

**J-bool-coercion — Truthiness rule.** PHP's `if ("0")`, `if ([])`, `if (0.0)` truthiness is a famous
footgun. Phorj is statically typed, so `if (cond)` should *require* a `bool` (no implicit coercion of
`int`/`string`/`List` to bool) — this is almost certainly already the de-facto behavior, but it must be a
*documented, enforced* semantic decision (the philosophy is "remove surprises"). Confirm + document +
ensure a clean `E-COND-NOT-BOOL` (or equivalent) rather than silent coercion. Strong fit, cheap.

**J-nan-ordering — NaN ordering + sort placement.** `compare_ord` returns `Ok(None)` for NaN, and the
backend op→bool projection makes every NaN comparison `false` (IEEE-correct, matches PHP `<`). But once a
`sort` exists (J-sort-stability), NaN's *placement* under a `<=>`-style total order is undefined and a
silent divergence risk between Rust `total_cmp` and PHP. Lock it explicitly (recommend: a sort comparator
that treats NaN consistently, documented). Small effort, prevents a sneaky run↔php sort divergence.

## Reject / Defer notes

- **J-op-overload (reject):** Operator overloading on user types (`a + b` calling a user `__add`) is PL
  power, not surprise-removal — it makes `+` non-legible (you can't know what `+` does without finding the
  class) and PHP has no general operator overloading to map to (only the magic-method internals, not
  user-exposed). Against the philosophy; reject.
- **J-identity-eq (defer):** `===` identity (`Rc::ptr_eq`) is already noted as "an optional future
  addition" in KNOWN_ISSUES. With only structural `==` and no surprise from its absence, defer until a
  concrete handle-identity need appears (e.g. cache keys on instance identity).
- **J-int-width (defer):** i64 fixed-width + checked overflow is shipped and documented; sized ints /
  `decimal` are explicitly v2 (native/systems). Defer — document the i64 guarantee, don't expand now.
- **J-hash-contract (defer):** The `HKey` `int`/`bool`/`string` contract is shipped; a *user-defined* hash
  contract (custom types as Map/Set keys) is real PL design but blocks on nothing today and adds surface —
  defer to M11 if user-key demand appears.
- **J-enum-ordinal (defer):** Enum ordering/ordinal is a small decision but low-demand; defer within M-RT
  until a sortable-enum use case appears (right now enums are matched, not ordered).
- **J-string-coerce-interp (defer):** A `Stringable` analog (user type usable in `"{x}"`) is a nice
  ergonomic but not a surprise-remover; interpolation already requires `string`. Defer to M11.

## Critic pass

Verified the shipped semantic surface directly against the source rather than the spec prose:
`src/ast.rs` `enum BinaryOp` = `Add Sub Mul Div Rem Eq NotEq Lt Gt Le Ge And Or Pipe Coalesce`
(`enum UnaryOp` = `Neg Not`); `src/value.rs` `compare_ord` only accepts `int/int` + `float/float`
(everything else `Err`); `src/checker.rs:1932-1957` rejects mixed arithmetic, mixed comparison, and
**cross-type `==`** (`l != r` → `E`-style error); `src/checker.rs:1678/2275/3359` already enforce a
`bool` `if`/expr-`if`/loop condition; `eq_val_rec` (`src/value.rs:259-316`) handles Enum/Instance/Map/
Set/List/scalar structurally and is cycle-guarded. Findings below.

### Mis-listings (already shipped — the *implementation* portion is done)

- **J-bool-coercion — the enforcement already SHIPS.** The recommendation reads as if a decision/
  enforcement is pending ("ensure a clean `E-COND-NOT-BOOL` rather than silent coercion"). It is
  already done: `checker.rs:1678` (`if`), `:2275` (expression-`if`), `:3359`/`:3388` (`while`/`do`)
  all reject a non-`bool` condition with `` `if`/loop condition must be `bool`, found `{c}` `` and have
  tests (`if_condition_must_be_bool`, `expression_if_condition_must_be_bool`, `while_condition_must_be_bool`).
  **Keep the item but downgrade to a pure-doc `map`**: the only real work is surfacing this as a stated
  contract in INVARIANTS/FEATURES (the error message could also be given a stable `E-COND-NOT-BOOL`
  code, but that is a sharpening, not new behavior). Recommendation stays adopt, effort stays S, kind
  flips `omit`→`map`, philosophy_fit strong.

### Newly-found gaps (full rows below; merged into the list)

- **J-string-concat — no string concatenation operator (PHP `.`).** `BinaryOp` has no `.` and no
  string `+`; string building is **interpolation-only** (`"{a}{b}"`). PHP's single most-used string
  operator (`$a . $b`) has no Phorj surface, and the obvious port — overloading `+` — is *rejected*
  by the checker (arithmetic requires int/float). A PHP dev WILL reach for `.` or `+` and hit a wall.
  Philosophy-fit form: adopt PHP's `.` operator on `string` (and `bytes`), `string . string -> string`,
  transpiling 1:1 to PHP `.`. This is surface + one checker arm + one `Op::Concat2` (or reuse the
  existing N-ary `Op::Concat` the interpolation path already has). Strong fit, removes a real surprise.
  ADOPT, M-RT, S.
- **J-pow-operator — no exponentiation operator (PHP `**`).** `Core.Math.pow` exists but the idiomatic
  PHP `**` operator does not. Low value vs. `pow()` already shipping, and `**` brings overflow/float
  questions (`2 ** 63`), so DEFER — note it as a known absence (use `Core.Math.pow`), revisit if demand
  appears. M11, S.
- **J-bitwise-ops — bitwise/shift operators (`& | ^ << >> ~`) are absent AND `&`/`|` now collide with
  type operators.** PHP has the full bitwise set; Phorj has none, and post-S4/S5 a lone `|`/`&` lexes
  to `Bar`/`Amp` (union/intersection *type* operators). Value-level bitwise would need careful
  disambiguation (only in expression position) or named functions (`Core.Bits.and(…)`). Bitwise is
  systems/low-level, weak philosophy fit for an app language, and the token collision is a real cost —
  DEFER to a `Core.Bits` native module (no operator) if a need appears. M11, M.
- **J-eq-asymmetry — `==`/`!=` allow enum/instance/list/map operands but `<`/`>` reject everything
  non-numeric (a documentable asymmetry).** `eq_val` compares Enum/Instance/Map/Set/List structurally,
  so `==` works on rich values; but `compare_ord` rejects all of them. This is *correct* (equality is
  total, ordering is not) but it is an **undocumented asymmetry** a user will trip on (`a == b` works,
  `a < b` is a type error for the same `a,b`). Fold into J-collection-eq-doc / J-ordering-rules as an
  explicit "what is comparable vs equatable" table. Pure doc, S, ADOPT (under M-RT, alongside the
  ordering rules).
- **J-compound-assign-types — compound-assignment operator result-type rules (`+=`, `*=`, `??=`,
  `++`/`--`).** Shipped in M-mut.2, but the *typing rule* of a compound op (does `float x; x += 1`
  widen the `1`? `x++` on what types? `??=` on a non-optional?) is an edge cluster with the same
  numeric-tower interaction as J-numeric-tower. Verify + document the rules (likely: `x op= e` ≡
  `x = x op e` and inherits arithmetic's strict-matching rule; `??=` requires `x` optional). DEFER into
  the J-numeric-tower / mutation-doc work — small, M-RT, S.

| id | title | kind | fit | rec | milestone | effort |
|---|---|---|---|---|---|---|
| J-string-concat | String concatenation operator (PHP `.`) — no string `+`/`.` exists | port | strong | adopt | M-RT | S |
| J-pow-operator | Exponentiation operator `**` (only `Core.Math.pow` exists) | port | ok | defer | M11 | S |
| J-bitwise-ops | Bitwise/shift operators `& \| ^ << >> ~` (absent; `&`/`\|` collide with type ops) | port | weak | defer | M11 | M |
| J-eq-asymmetry | Equatable-vs-comparable asymmetry doc (`==` on rich values, `<` rejects them) | map | strong | adopt | M-RT | S |
| J-compound-assign-types | Compound-assign result-type rules (`+=`/`??=`/`++` typing) | port | ok | defer | M-RT | S |

Sanity check against philosophy: J-op-overload (reject) is correct — operator overloading is
power-not-surprise-removal and has no general PHP target. The new J-string-concat is the *inverse*
case and a clear adopt: it is the single most familiar PHP operator with a 1:1 transpile and no
ambiguity, and its absence is a genuine surprise. The ordering/numeric-tower/sort cluster
(J-ordering-rules, J-numeric-tower, J-spaceship, J-sort-stability, J-nan-ordering) is the highest-value
ADOPT group and correctly framed as surprise-removal. Iteration protocol (J-iter-protocol) remains the
single biggest open landmine and the L-effort assessment is right.
