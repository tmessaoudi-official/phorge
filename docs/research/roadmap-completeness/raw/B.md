# Track B — Beyond-PHP Game-Changers (Roadmap Gap Audit)

## Track summary

Track B asks the hard question: which TypeScript-over-JavaScript-class capabilities actually
*justify* Phorj as an upgrade of PHP, judged against the philosophy (a pragmatic, legible,
provably-correct upgrade whose apex filter is **craftsmanship**, not raw power)? Phorj already has
the rare-for-PHP foundation that makes most of these feasible: full ADT enums + exhaustive `match`,
erased generics (free fns, methods, classes), unions `A|B`, intersections `A&B`, `instanceof`
smart-cast, optionals `T?` with a compile-time non-null guarantee, first-class functions/closures,
immutable-by-default with `mutable` opt-in, and a checker-pass "expand-before-backends" discipline
that lowers PHP-absent features to ordinary deterministic PHP. The genuinely high-value remaining
gaps cluster tightly: (1) **deeper pattern matching** — guards, or-patterns, payload/structural
destructuring, `@`-bindings, range/literal patterns, `if-let`/`while-let` already partly shipped;
(2) **first-class `Result<T,E>` + `?` propagation** — the principled no-exceptions recoverable-error
story, gated only by generic enums; (3) **derive-style compile-time attributes** (`Eq`/`Show`/`Ord`/
`Default`/`Json`) — a perfect fit for the erase-before-backends model; and (4) **generic enums +
bounds** — the missing piece under `Result`, `Option<T>`, and typed containers. The rest are
**defers** (structured concurrency — rides M6 green threads under the byte-identity quarantine;
design-by-contract; persistent/immutable collections as a richer stdlib; comptime beyond closed
derive) or **rejects** (open proc-macros, typestate as a dedicated system, refinement types as a
solver, reactive signals, units of measure, TCO-as-a-guarantee) — each because it either breaks the
determinism spine, has no idiomatic legible PHP target, or is PL-theory vanity that overruns its
surprise budget for a PHP audience. The single most leveraged unlock is **generic enums**, because
it converts three separate "deferred" rows (`Result`, true `Option<T>`, generic ADT containers) into
shipped features at once.

## Gap table

| id | title | kind | fit | rec | milestone | effort |
|---|---|---|---|---|---|---|
| B-genenums | Generic enums `enum Result<T,E>` / `Option<T>` | port | strong | adopt | M-RT (generics-all follow-up) | M |
| B-result | First-class `Result<T,E>` + `?` propagation | new | strong | adopt | M-RT (after generic enums) | M |
| B-qmark-opt | `?` propagation over optionals (no prereq) | new | strong | adopt | M-RT (now) | S |
| B-guards | Match guards (`Circle c if c.r > 0 =>`) | new | strong | adopt | M-RT (post-S4) | S |
| B-orpat | Or-patterns in match arms (`A \| B =>`) | new | strong | adopt | M-RT (post-S4) | S |
| B-payload-destr | Enum/variant payload destructuring in arms | new | strong | adopt | M-RT (post-S4) | M |
| B-struct-destr | Structural destructuring (nested ADT/class fields) | new | strong | adopt | M-RT (post-S4) | M |
| B-at-bind | `@`-bindings (bind whole value while destructuring) | new | ok | adopt | M-RT (with guards) | S |
| B-range-pat | Range/literal patterns (`1..=5 =>`, `0 =>`) | new | strong | adopt | M-RT (post-S4) | S |
| B-list-destr | List/array destructuring + spread `[a, ...rest]` | port | strong | adopt | M3 (front-end) | M |
| B-derive | Derive-style attributes `#[derive(Eq/Show/Ord/Default)]` | new | strong | adopt | M11 / dedicated derive slice | L |
| B-derive-json | `#[derive(Json)]` (serialize) | new | ok | defer | M11 (after core.json) | M |
| B-bounds | Generic bounds (`<T: Comparable>`) | new | ok | defer | post-M-RT | M |
| B-variance | Declared variance (`in`/`out`) | new | weak | reject | — | M |
| B-concurrency | Structured concurrency: `spawn` + channels (green threads) | port | strong | defer | M6 | L |
| B-async-await | `async`/`await` (colored) | new | weak | reject | — | L |
| B-actors | Actor model / message-passing isolates | new | weak | reject | — | L |
| B-contracts | Design-by-contract (pre/post/invariant) | new | ok | defer | post-GA / contract slice | M |
| B-persistent | Persistent/immutable collection library | new | ok | defer | M11 (stdlib) | M |
| B-newtype | Opaque newtypes / refinement-with-smart-constructor | new | strong | adopt | M-RT or dedicated slice | M |
| B-refinement | Refinement types / liquid types (solver-backed) | new | weak | reject | — | L |
| B-units | Units of measure | new | weak | reject | — | M |
| B-typestate | Typestate (state-machine-in-types) | new | weak | reject | — | L |
| B-comptime | Comptime/`const fn` evaluation | new | ok | defer | post-GA | L |
| B-macros | User-defined/open macros (proc-macro style) | new | weak | reject | — | L |
| B-tco | Guaranteed tail-call optimization | new | weak | reject | — | M |
| B-reactive | Reactive primitives / signals | new | weak | reject | — | L |
| B-active-pat | Active patterns / view patterns (F#-style) | new | weak | reject | — | M |
| B-gadts | GADTs / higher-kinded types | new | weak | reject | — | L |
| B-effects | Algebraic effects / effect handlers | new | weak | reject | — | L |
| B-foreach-coll | `foreach`/iteration over Map/Set/Range (iterator protocol) | port | strong | adopt | M11 (stdlib) | M |
| B-flow-narrow | Negative/flow narrowing in `else` + union exhaustiveness | new | strong | adopt | M-RT (S4 follow-up) | M |

## Rationale for each ADOPT

**B-genenums — generic enums `enum Result<T,E>` / `Option<T>`.** Phorj has erased generics for free
functions, methods, and classes, and full payload ADT enums — but the type-parameter list is
explicitly *not* a feature of enums yet (KNOWN_ISSUES, "Generic *enums* are not supported"). This is
the single highest-leverage unlock in the track: it is the prerequisite under `Result<T,E>`, a true
`Option<T>` (today optionality is the special-cased `T?`/`Value::Null`), and any generic sum-type
container. It rides the existing erasure machinery (`erase_generics` already walks classes/methods;
extend it to the enum arm), introduces no new `Op` (enum construction/match already exist), and erases
to a plain PHP enum/class with `mixed` payloads — byte-identity-safe by construction. Strong fit:
ADTs are the craftsmanship-correct way to model alternatives, and a PHP dev reads `Result<T,E>`
immediately.

**B-result — first-class `Result<T,E>` + `?` propagation.** The principled, no-surprises answer to
PHP's exception culture and to the open exceptions-vs-Result fork. A `Result` is just a 2-variant
generic enum (`Ok(T)`/`Err(E)`) once B-genenums lands; `x?` desugars to *match-and-early-return*
(`match x { Ok(v) => v, Err(e) => return Err(e) }`) — pure front-end lowering, no new `Op`, no
runtime reflection, deterministic PHP target. It coexists with (does not displace) the later
try/catch interop bridge, satisfying the additive-power tenet. The parity spec already grades this
roi=high; Track B confirms it as the recoverable-error spine.

**B-qmark-opt — `?` propagation over optionals (no prerequisite).** The subset of B-result that ships
*today*: `opt?` inside a function returning `U?` lowers to `match opt { x => x, null => return null }`.
No generics needed, no new `Op`, deterministic. Shipping this first delivers the ergonomic win
immediately and de-risks the propagation lowering before the full `Result` arrives. Small effort,
strong fit — it is `?.`/`??`'s natural completion.

**B-guards — match guards.** `match s { Circle c if c.r > 0 => … }` is the top pattern-matching pick
in the parity spec. It lowers to the existing branch ops (`IsInstance` + `JumpIfFalse` + the guard
expression), needs **no new `Op`**, and follows the Rust exhaustiveness rule (a guarded arm is
treated as non-covering). PHP has no equivalent and the in-discussion PHP pattern-matching RFC
confirms this is genuinely PHP-absent — a clear beyond-PHP win that a PHP dev grasps instantly.

**B-orpat — or-patterns.** `match x { A() | B() => … }` collapses duplicated arms; lowers to a
disjunction of the per-pattern tests over existing ops, no new `Op`, exhaustiveness composes
naturally. Cheap, legible, and pairs with guards/destructuring as one coherent "deeper match" slice.

**B-payload-destr — enum/variant payload destructuring in arms.** Today a `match` arm binds the whole
payload value; field-level extraction (`Circle(r) => area(r)`) is the obvious next step after S4 type
patterns. Front-end lowering (bind then field-read), no new `Op`. This is the feature that makes ADTs
*ergonomic* rather than merely *expressible*, and is a defining TypeScript/Rust/Swift capability PHP
lacks.

**B-struct-destr — structural destructuring (nested ADT/class fields).** `Wrap(Point{x, y}) => x + y`
— nested field reads + binds, lowered front-end. The general form of B-payload-destr; sequenced with
it. Strong fit: it is the legible way to take apart deep data, with a deterministic PHP target (a
sequence of property reads).

**B-at-bind — `@`-bindings.** `x @ 1..=5 => …` binds the whole matched value while also testing a
sub-pattern. Small front-end desugar, companion to guards and range patterns. Ok (not strong) fit —
slightly less PHP-familiar syntactically, but high value in deep matches and cheap to ship alongside
the other pattern work.

**B-range-pat — range/literal patterns.** `match n { 0 => …, 1..=5 => …, _ => … }` over the existing
range machinery (`a..=b`) and equality ops; no new `Op`. Natural extension of the literal patterns S4
already relaxed for primitive-union scrutinees. Strong fit, small effort.

**B-list-destr — list/array destructuring + spread.** `[a, b, ...rest] = xs` and call-site
`f(...args)` are pure desugaring to indexed binds / list-concat — binds, not mutation, so it fits the
immutable model — and emit idiomatic PHP `[$a, $b] = …` / `...$args`. This is technically a PHP
*port* (PHP has `[$a,$b]=`), but it is a beyond-PHP-quality win because Phorj can make it
type-checked and exhaustiveness-aware. Pairs with variadics.

**B-derive — derive-style compile-time attributes (`Eq`/`Show`/`Ord`/`Default`).** The native-fit
metaprogramming model for Phorj: an inert `#[...]` passthrough channel plus a **closed** set of
compile-time derives that synthesize ordinary methods (field-wise `==`, a `toString`/display from
fields, lexicographic `<=>`, a default constructor) into the AST *before any backend* — exactly the
`erase_generics`/`resolve_html`/alias-expansion discipline already in the checker. Generated code is
deterministic std-only PHP, no runtime reflection (the rejected reader). This is the clean answer to
PHP's manual `__toString`/`jsonSerialize`/`usort` boilerplate and to the var_dump gap, and it is the
anchor for several other parity rows. Larger effort because it is a small framework (attribute parse +
placement checks + per-derive synthesizers), but very high leverage.

**B-newtype — opaque newtypes / refinement-with-smart-constructor.** A `newtype UserId = int` (or a
single-field wrapper whose constructor is the only way in) gives nominal type safety with zero runtime
cost — it erases like `Core.Html` (the purest existing precedent for the erasure discipline). This is
the *pragmatic* form of refinement (smart constructor enforces the invariant once; the type then
proves it), as opposed to the solver-backed B-refinement which is rejected. Strong fit: it is legible,
deterministic, and directly attacks PHP's "everything is an `int`/`string`" primitive-obsession
anti-pattern (a craftsmanship win).

**B-foreach-coll — iteration over Map/Set/Range.** Maps are already insertion-ordered internally, so
`for ((k, v) in m)` lowers to PHP `foreach ($m as $k => $v)` deterministically; Set and Range
likewise. This closes the "map iteration / Set iteration" deferral that KNOWN_ISSUES flags as
pending. Needs the tuple/pair binding form (small) and a uniform iteration lowering. Strong fit — it
makes the shipped collections usable rather than just constructible.

**B-flow-narrow — negative/flow narrowing + union exhaustiveness.** Today `if (s instanceof Circle)`
narrows the then-branch but the `else` does not narrow `s` to the remaining union members
(KNOWN_ISSUES, S4 deferral). True flow narrowing — the defining TypeScript capability — would let the
checker prove a union is exhausted after an `instanceof`/type-pattern chain and narrow the negative
branch. Front-end only (checker flow analysis), no runtime/`Op` impact, deterministic. Strong fit:
this is the "provably-correct upgrade" promise made concrete, and a PHP dev coming from TS expects it.

### Why the notable DEFERs/REJECTs (one line each)

- **B-concurrency (defer, M6):** already roadmapped as uncolored `spawn` + channels on green threads;
  the byte-identity spine is preserved by quarantining the scheduler outside `differential.rs`
  (the `serve.rs`/`Transport` precedent). Not a missing gap — a sequenced one.
- **B-async-await / B-actors (reject):** colored async breaks the uncolored-`spawn` decision and has
  no clean deterministic PHP target; actors are over-scoped vs the green-thread model.
- **B-contracts (defer):** pre/post/invariant assertions are valuable and lower to deterministic PHP
  guard code, but belong after the error model so a violation has a defined fault path.
- **B-derive-json / B-persistent / B-comptime (defer):** ride later stdlib (`core.json` needs a
  dynamic `Json`/`Any` type) or post-GA work; principled, not missing.
- **B-refinement / B-units / B-typestate / B-tco / B-reactive / B-active-pat / B-gadts / B-effects /
  B-macros / B-variance (reject):** PL-theory maximalism — each either needs a solver/runtime Phorj
  refuses to build, has no legible PHP target, introduces non-determinism, or overruns the
  surprise budget for a PHP audience. Newtypes (adopt) cover the *pragmatic* slice of refinement;
  closed derive (adopt) covers the pragmatic slice of macros; green threads (defer) cover concurrency.

Sources: [PHP RFC: Pattern Matching](https://wiki.php.net/rfc/pattern-matching),
[PHP RFC Watch](https://php-rfc-watch.beberlei.de/)

## Critic pass

Adversarial completeness + mis-listing review against `FEATURES.md`, `KNOWN_ISSUES.md`, the project
`CLAUDE.md` milestone log, and `ROADMAP.md` (all read).

**Mis-listings:** none. All 32 original rows are genuinely unshipped (deferred, rejected, or planned).
Spot-checked the at-risk rows: `B-flow-narrow` is a real `KNOWN_ISSUES` S4 deferral (correctly listed
as a gap, not shipped); `B-foreach-coll` matches the "map iteration / Set iteration / tuples deferred"
KNOWN_ISSUES entry; the destructuring/guard/or-pattern cluster stays valid because the **PHP pattern-
matching RFC is still unvoted** (in discussion through early-2026 — confirmed), so these remain
genuinely PHP-absent. `B-result`/`B-genenums`/`B-qmark-opt` are not shipped (no `Result`, enums have no
type params — `KNOWN_ISSUES` "Generic *enums* are not supported"). Removed: 0.

**Newly-found items (7)** — long-tail beyond-PHP and PHP-port-we-lack gaps the first pass missed:

| id | title | kind | fit | rec | milestone | effort |
|---|---|---|---|---|---|---|
| B-iter-protocol | User-defined iterator/iterable protocol (`for (x in myColl)` over user types) | port | strong | adopt | M11 (with B-foreach-coll) | M |
| B-let-else | `let-else` / bind-or-diverge (`if (var x = opt) {…} else { return }` completed to a binding-or-early-exit form) | new | strong | adopt | M-RT (with null-safety) | S |
| B-tuples | Anonymous tuples `(int, string)` + multi-return + pair binding | new | ok | defer | M-RT (under B-foreach-coll) | M |
| B-labeled-break | `break` / `continue` (optionally labeled) for loop control | port | strong | adopt | M3 (front-end) | S |
| B-sealed | Sealed/closed class hierarchies → exhaustive `match` over subclasses | new | strong | adopt | M-RT (post-S6 extends) | M |
| B-intrinsics | Correctness intrinsics `assert` / `unreachable` / `todo` / `panic` | new | ok | adopt | M3 (front-end) | S |
| B-op-overload-derive | Operator overloading (custom `==`/`<=>`/`+` via a closed derive/trait) | new | weak | reject | — | M |

Rationale for the new rows:

- **B-iter-protocol (adopt, port).** PHP *has* `Iterator`/`IteratorAggregate`; Phorj can only iterate
  built-in `List` (and, once `B-foreach-coll` lands, `Map`/`Set`/`Range`). A user type implementing a
  small `Iterator` interface (`next`/`hasNext`, or yielding a `List`) so `for (x in myColl)` works on
  user collections is a real **port we lack**, not a beyond-PHP luxury. Strong fit: it transpiles to a
  PHP `Iterator`/`foreach` directly — the most PHP-familiar form. Distinct from `B-foreach-coll`, which
  only covers the three shipped built-in collection types. Pairs naturally with it (one "iteration"
  slice). No generators/`yield` (that is a separate, heavier, suspension-based feature — *not* proposed,
  it has no clean deterministic byte-identity story without the M6 green-thread machinery).

- **B-let-else (adopt, new).** `if (var x = opt)` already binds-and-narrows the *then* branch (S2/S1.4);
  the missing companion is the *binding-or-diverge* form where the else branch must exit (`return`/
  `throw`/fault) so `x` is in scope and non-null for the rest of the block — Rust's `let … else`,
  Swift's `guard let`. Pure front-end lowering over the existing if-let machinery, no new `Op`,
  deterministic PHP (`if ($x === null) { return …; } /* $x usable */`). Strong fit: it is the natural
  completion of the shipped null-safety suite and removes the rightward-drift surprise. Small.

- **B-tuples (defer, new).** Anonymous product types `(int, string)` give cheap multi-return and the
  pair-binding form that `B-foreach-coll`'s `for ((k, v) in m)` needs. PHP has no tuple type — it maps
  to a positional array `[a, b]` (idiomatic, deterministic), so *ok* (not strong) fit: a PHP dev reads
  `[$a, $b] = …` but the named-field `class`/record is often the more legible Phorj form. Defer: it is
  the substrate under `B-foreach-coll` pair-binding and `B-list-destr`; ship those with a minimal
  2-tuple form rather than a full general tuple type up front (surprise-budget discipline). Not a miss
  to delay, but a genuine gap the first pass omitted entirely.

- **B-labeled-break (adopt, port).** PHP has `break N;` / `continue N;` (numeric-level loop control);
  Phorj's surface (per `FEATURES.md`) has `for…in` and condition loops but no documented `break`/
  `continue` at all — a real **port we lack**. A bare `break`/`continue` (and optionally a labeled form
  lowering to PHP's `break N;`) is basic, legible control flow a PHP dev expects. Front-end, no runtime
  surprise, idiomatic PHP target. Strong fit, small.

- **B-sealed (adopt, new).** A `sealed`/closed class hierarchy lets the checker prove a `match` over a
  *class* hierarchy exhaustive — the class-side dual of the enum exhaustiveness Phorj already enforces,
  and the missing piece that makes union/`instanceof` matching *provably total* over a fixed subclass
  set. Strong fit with the "provably-correct upgrade" promise; lowers to plain PHP classes (the sealing
  is a compile-time-only check, erased — the `Core.Html`/erase-before-backends precedent). Sequenced
  after S6 `extends` (it needs subclassing to seal). Beyond-PHP (PHP has no sealed classes), but
  immediately legible. Complements `B-flow-narrow`.

- **B-intrinsics (adopt, new).** `assert(cond)`, `unreachable()`, `todo()`, `panic(msg)` are small
  correctness primitives that fit the provably-correct ethos and the clean-fault model (they lower to a
  deterministic fault on the existing `Op::Fault` path — no new `Op`, byte-identical, and `assert`
  transpiles to a PHP `assert()`/guard). Cheap, legible, and they give the developer a principled way to
  mark impossible states (pairs with exhaustiveness). Ok fit (PHP has `assert`; `unreachable`/`todo` are
  beyond-PHP but trivially mapped). Small.

- **B-op-overload-derive (reject, new).** Operator overloading (`==`/`<=>`/`+` on user types) is listed
  as deferred in `KNOWN_ISSUES`; `B-derive` only synthesizes *methods*, never rebinds operators. Reject
  the *operator-rebinding* form: PHP has no user operator overloading, so `a + b` on objects has **no
  idiomatic deterministic PHP target** (it would have to lower to a hidden `$a->__add($b)` method call —
  action-at-a-distance that overruns the surprise budget for a PHP audience). The *pragmatic* slice — a
  derived `equals`/`compareTo` *method* (callable explicitly, usable by `usort`) — is already covered by
  `B-derive`'s `Eq`/`Ord`; that is the legible answer. Reject the operator-syntax form specifically.

Net: the merged list is the 32 original rows (0 removed) + 7 newly-found = **39 items**.
