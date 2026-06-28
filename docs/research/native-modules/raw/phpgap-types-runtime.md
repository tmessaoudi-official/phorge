# Stage 3 — PHP runtime / type / reflection / serialization capabilities vs Phorj

**Area:** PHP runtime/type/reflection/serialization stdlib capabilities
(`serialize`, `var_export`, `gettype`/`settype`, `get_object_vars`, closure binding,
generators/iterators, `is_callable`, enum `cases()`/`from`/`tryFrom`, `json_*` niceties).

**Scope discipline:** language-level syntax gaps are tracked in
`docs/specs/2026-06-21-php-parity-and-beyond.md` and are excluded here. This document is the
**stdlib / library CAPABILITY** sweep — natives a program calls at runtime, not parser/checker work.
Where a PHP capability is *only* deliverable as language work (e.g. backed-enum `cases()`, generator
`yield`), I say so and classify it `language` so the consumer doesn't mistake it for a native module.

Confidence is graded per finding (high / medium / low). The big mechanical claims (reuse of
`NativeEval::Reflective`, the closed `Value` enum, `eq_val_rec`'s cyclic visited-set, the injected-type
prelude, the gated-helper transpile path) are all **verified against source** this session:
- `src/native/mod.rs:62` `pub pure: bool`; `NativeEval` enum (`Pure`/`HigherOrder`/`Reflective`).
- `src/native/reflect.rs` — `Core.Reflect` already ships `kind`/`className`/`typeName`/`interfaces`/
  `parents`/`methods`/`fields` over a `ClassTables` (`NativeEval::Reflective`, no new Op).
- `src/value.rs:14` closed `Value` enum (Int/Float/Decimal/Bool/Str/Bytes/Unit/Null/List/Map/Set/
  Instance/Enum/Closure); `ClosureData::{Tree,Named,Byte}`; `Instance` = `Rc<RefCell<fields>>`.
- `src/value.rs:274` `eq_val_rec` with `visited: Vec<(*const Instance,*const Instance)>` — the cyclic
  recursion guard the brief says to reuse for a dumper.
- `src/cli/mod.rs:302` `inject_json_prelude` / `:346` `inject_rounding_mode_prelude` — the injected-type
  pattern for `Core.Json`'s `Json` enum and the `RoundingMode` enum.
- `src/native/convert.rs` — `Core.Convert` already ships `toString`/`toFloat`/`toInt`(`->int?`)/
  `truncate`/`round`/`intToDecimal`/`decimalToFloat`/`decimalToInt`.
- `src/native/json.rs:422` — `Core.Json` ships `parse`/`stringify`/`stringifyPretty` over an injected
  `Json` enum.

---

## The framing question per capability: feasibility (determinism) BEFORE usefulness

Every candidate below is first partitioned Tier A (pure → byte-identity-gateable, ships in
`differential.rs`) vs Tier B (impure → quarantined). Almost everything in *this* area is Tier A,
because runtime/type/reflection/serialization over Phorj's **closed `Value` enum** is a pure function
of the value + the program's `ClassTables`. The classic PHP determinism traps in this area
(`spl_object_id`, `var_dump` `#3` object ids, `serialize`'s address-free-but-format-leaky output, float
`echo` 14-digit vs Rust 17-digit) are avoidable *by construction* because Phorj owns the format and
never prints a pointer.

---

## FINDING 1 — `serialize()` / `unserialize()` → **`Core.Serde` (Phorj-owned binary-stable codec)**

**PHP capability:** `serialize($v)` → a self-describing string; `unserialize($s)` → value. Round-trips
arrays, scalars, and objects (calling `__sleep`/`__wakeup`/`Serializable`/`__serialize`).
**PHP's format is a security and portability disaster** — `unserialize` of untrusted input is a remote
object-injection vector (POP-chain gadgets), the format embeds private property names with NUL bytes,
and it is PHP-version-fragile.

**Does Phorj have an equivalent?** **No.** `Core.Json` covers the *interchange* case for the
JSON-shaped subset, but cannot round-trip a typed `Instance`/`Enum`/`Set`/`Decimal`/`bytes` back to the
same Phorj type (JSON has no class tag, no decimal, no byte type, no set).

**The BETTER port — `Core.Serde`** (Phorj : PHP :: a typed, capability-free codec : `unserialize`):

```phorj
import Core.Serde;

Serde.encode(value) -> bytes               // deterministic, self-describing, version-tagged
Serde.decode(bytes) -> Result<T, SerdeError> // typed, NEVER instantiates an arbitrary class by name
```

Why it is better than PHP's `serialize`:
1. **No code execution on decode.** `unserialize` is RCE-adjacent; `Serde.decode` is a pure data
   reconstruction over the *known* `ClassTables` of the program being run. An unknown class tag is a
   typed `SerdeError`, never a constructed object — closes PHP's #1 deserialization CVE class by
   construction.
2. **Deterministic + byte-stable** — Map/Set are already insertion-ordered `Vec` reps
   (`src/value.rs`), so the encoded bytes are a pure function of the value. Tier A,
   byte-identity-gateable.
3. **Decimal + bytes survive the round-trip** (PHP's serialize stringifies a `decimal` as a float-ish
   BCMath string and loses the type tag).

**Tier:** **A** (encode is pure; decode is pure over `ClassTables`). **No new Op** —
`NativeEval::Reflective` (decode needs the class table to rebuild `Instance`s), `Op::CallNative`.
**Transpile target:** a **gated runtime helper** `__phorj_serde_encode/decode($v)` (the `uses_*` /
`emit_runtime_helpers` mechanism — NOT PHP `serialize`, whose format we are explicitly rejecting).
The PHP helper reproduces Phorj's own format byte-for-byte; cross-version-stable because *we* own it.
**Determinism risk:** float fields inside an instance — must use the same `__phorj_float` (Ryū) the
rest of the spine uses, else Rust-17-digit vs PHP-14-digit diverges (the standing float trap). **Risk:**
`Result<SerdeError,T>` needs generic enums (already shipping per CLAUDE.md) for the typed-error surface.
**Recommendation: adopt-later** — high value but should land after `Core.Debug` (shares the
`Value`-walk skeleton) and after the generic-enum `Result` surface is load-bearing.
**Confidence: medium** (mechanism high; the exact wire format is a design call, and decode's "rebuild an
`Instance` without running its constructor" needs an audited path so invariants/property-hooks aren't
silently bypassed — flagged as the one real correctness subtlety).

---

## FINDING 2 — `var_export()` → folded into **`Core.Debug`** (re-parseable mode)

**PHP capability:** `var_export($v, true)` → a string of *valid PHP source* that re-creates the value.
Used for config-cache generation and golden-file tests.

**Does Phorj have an equivalent?** **No**, but `Core.Debug` is already designed
(`docs/research/native-modules/raw/feasibility-Dump.md`, ADOPT-NOW) as the var_dump/print_r analog.
`var_export` is the **re-parseable sibling** and should be a *mode of the same module*, not a new one:

```phorj
import Core.Debug;
Debug.dump(value) -> string         // human tree (already designed)
Debug.inspect(value, int) -> string // depth-capped (already designed)
Debug.export(value) -> string       // NEW: valid *Phorj* literal source, re-parseable
```

Why it is better than PHP's `var_export`: it emits **Phorj** literal syntax (`[k => v]`, `Some(3)`,
`19.99d`, `b"…"`) — typed and round-trippable through Phorj's own parser, whereas PHP's `var_export`
can't represent a closure (`fatal error`) and renders objects in a non-re-parseable
`\Foo::__set_state(...)` form that needs a magic static. Phorj's closures dump as an opaque
`<closure>` token (deterministic, never source).
**Tier A**, no new Op, same `Value`-walk + `eq_val_rec` cyclic guard as `Debug.dump`. **Transpile:**
gated `__phorj_export` helper. **Recommendation: adopt-now** — it is one extra renderer arm on a
module already greenlit; ship it inside the Debug module rather than as a separate feature.
**Confidence: high.**

---

## FINDING 3 — `gettype()` / `get_debug_type()` → **already covered by `Core.Reflect.kind`/`typeName`**

**PHP capability:** `gettype($v)` (coarse: "integer"/"double"/"string"/"array"/"object"/"boolean"/
"NULL"); `get_debug_type($v)` (8.0+, the *good* one: precise class/`int`/`float`/`string`/`array` etc.).

**Does Phorj have an equivalent?** **YES — fully.** `Core.Reflect.kind(x) -> string` is the
erasure-stable coarse tag (`src/native/reflect.rs:141`); `Core.Reflect.typeName` is the precise static
type (the `get_debug_type` analog), resolved in a checker pass. Phorj's version is strictly **better
than `gettype`**: it never returns the legacy/misleading `"double"` for a float or `"NULL"` string
(it returns honest `"float"`), and `typeName` distinguishes Map/Set/the concrete class — which
`gettype` collapses to `"array"`/`"object"`. **No gap.** **Recommendation: reject** (already shipped).
**Confidence: high.**

---

## FINDING 4 — `settype()` → **deliberately rejected; `Core.Convert` is the better port**

**PHP capability:** `settype(&$v, "integer")` mutates a variable's type in place (`"integer"`/
`"float"`/`"string"`/`"bool"`/`"array"`/`"null"`). It is the antithesis of static typing — a runtime
type-punning footgun.

**Does Phorj have an equivalent?** Yes, the *correct* one: **`Core.Convert`** already ships
`toString`/`toFloat`/`toInt`(`-> int?`, null on non-parseable)/`truncate`/`round`/decimal bridges
(`src/native/convert.rs`). These are **pure, value-returning, null-safe conversions** — the upgrade
over `settype`'s in-place mutation + silent lossy coercion (`settype("12abc","integer")` → `12`
silently; Phorj `Convert.toInt("12abc")` → `null`, forcing the caller to handle it). **No native gap.**
A small breadth follow-up: `Convert.toBool(string) -> bool?` and `Convert.parseInt(string, int radix)
-> int?` round out the parse surface (currently only `toInt` base-10). **Tier A**, no new Op.
**Recommendation: defer** the two breadth natives to M4 (stdlib charter); `settype` itself is **reject**
(its mutate-in-place semantics violate the type discipline — a non-goal, by Phorj's philosophy).
**Confidence: high.**

---

## FINDING 5 — `get_object_vars()` / property enumeration → **`Core.Reflect.fields` exists; add a VALUE-returning `entries`**

**PHP capability:** `get_object_vars($obj)` → an assoc array of *accessible* property name→value pairs
(visibility-scoped). The dynamic-introspection workhorse (ORMs, serializers, `compact`/`extract`).

**Does Phorj have an equivalent?** **Partially.** `Core.Reflect.fields(x) -> List<string>` returns
field *names* (sorted, including inherited/promoted; `src/native/reflect.rs`). It does **not** return
the *values*. The value-side gap is real:

```phorj
import Core.Reflect;
Reflect.fields(x) -> List<string>            // names only — EXISTS
Reflect.entries(x) -> Map<string, T>         // NEW: name -> current value (public fields only)
```

Why a value-returning `entries` is the right port (and better than `get_object_vars`):
- **Visibility-honest** — only `public` fields appear (PHP's `get_object_vars` leaks private props when
  called from inside the class scope, a context-dependent footgun; Phorj fixes the scope to "external"
  for determinism — verified the 6-access-site visibility model exists, memory
  `[[member-visibility-six-access-sites]]`).
- **Typed** — returns a `Map<string, T>` not an untyped assoc array.
- **Insertion/declared order** — uses the same sorted `ClassTables.fields` order, deterministic.

**Tier A**, **no new Op** — `NativeEval::Reflective` already has the field-name list; the value side is a
runtime `Instance.fields` read (a `RefCell::borrow`, the same read path the dumper uses). The result is
a `Value::Map` built via `value::build_map`. **Transpile:** the existing
`__phorj_reflect_*` emitted-table mechanism + a `get_object_vars`-like helper restricted to public
props. **Determinism risk:** none beyond the float-field trap (use `__phorj_float`).
**Recommendation: adopt-later** (rides the same `Value`-walk as `Core.Debug`/`Core.Serde`; build them as
a cluster). **Confidence: high** on mechanism, **medium** on whether `entries` should be on `Core.Reflect`
or a new `Core.Object` leaf (a naming call).

---

## FINDING 6 — Closures: `is_callable` / `Closure::bind` / `fromCallable` / first-class-callable

**PHP capability:** `is_callable($x)` (predicate); `Closure::bind($c,$newThis,$scope)` /
`$c->bindTo(...)` (rebind `$this` and visibility scope); `Closure::fromCallable('strlen')`;
first-class-callable `strlen(...)`.

**Does Phorj have an equivalent?**
- **First-class callables: YES** — `Value::Closure(ClosureData::Named)` and lambda values already ship
  (M3 S3, CLAUDE.md), transpiled to PHP `f(...)`. No gap.
- **`is_callable` predicate: small gap** — there is no native that *tests at runtime* whether a value is
  callable. With static types this is usually unnecessary (the type says so), but for a `T?`/union
  scrutinee a runtime check is useful. Better port: **`Core.Reflect.isCallable(x) -> bool`** —
  byte-identical to `Reflect.kind(x) == "callable"` (the `kind` native already classifies a closure as
  `"callable"`, verified `src/native/reflect.rs`). **Tier A, no new Op, trivial.** Arguably redundant
  with `kind`; **recommendation: defer** (a one-line convenience).
- **`Closure::bind` / `bindTo`: deliberately rejected.** Rebinding `$this` and reaching into private
  scope is a runtime encapsulation-breaker. Phorj's closures capture `this` *at creation*
  (`this_capture` in `ClosureData::Tree`, verified) and the checker **rejects a lambda that touches
  `this` in a free context** (`E-LAMBDA-THIS`, per CLAUDE.md). Re-binding scope at runtime is
  incompatible with the static visibility model and the `Value`-isn't-`Send` single-threaded heap.
  **This is a non-goal — reject.** The legitimate use case (partial application / currying) is served by
  lambdas + the pipe operator, which already ship. **Confidence: high.**

---

## FINDING 7 — Generators / Iterators / `foreach` over user types → **language work, not a native module**

**PHP capability:** `yield`/`yield from` generators; `Iterator`/`IteratorAggregate`/`Traversable`
interfaces so `foreach` works over user objects; `iterator_to_array`.

**Does Phorj have an equivalent?** **No**, and this is **out of scope for this stdlib sweep** — it is
**language work**: `A-iterators` (Iterator protocol, adopt, milestone **M11**) and `A-generators`
(`yield`, **defer to M6**, deep effort) are both tracked in the parity spec. A `Seq<T>` lazy-sequence
variant (`L-lazy-seq`) is **rejected** there (fights the eager byte-identity spine — a lazy generator's
observable evaluation order is hard to keep identical across a tree-walker, a stack VM, and PHP's own
generator implementation).

The only **native** sliver worth noting once the *language* iterator protocol lands at M11: an
`iterator_to_array` analog → **`Core.List.fromIter`** / `Core.Map.fromEntries`. Until the protocol
exists there is nothing to materialize, so this is purely a downstream follow-up.
**Tier:** **language** (the capability), then **A** for the eventual `fromIter` native.
**Recommendation: defer** (blocked on M11). **Confidence: high** (that it is language-gated, not a
native gap).

---

## FINDING 8 — Backed enums: `cases()` / `from()` / `tryFrom()` → **mostly language, one native sliver**

**PHP capability:** backed enums (`enum Status: string`) with the auto-generated `Status::cases()`
(all variants), `Status::from($v)` (throws on miss), `Status::tryFrom($v)` (`?Status`).

**Does Phorj have an equivalent?** `Value::Enum` ships; generic enums ship; but **backed enums +
`cases`/`from`/`tryFrom` are tracked as `A-backed-enums` (adopt, M-RT) — language work** (parser/checker
must recognise the backing type and synthesize the three statics). The runtime support is the *easy*
part. Where this area *does* contribute: the synthesized `from`/`tryFrom` lookup and `cases()` list are
naturally implemented as the same kind of **`ClassTables`-style table emission** that
`Core.Reflect.parents`/`methods` already use — i.e. the reflection plumbing this module owns is the
mechanism the language feature reuses. Worth flagging to the language-side owner: **don't invent a new
table; the enum-cases table is a sibling of the existing `ClassTables` reflective tables.**
**Tier:** **language** (capability) + **A** (the table mechanism is pure). **Recommendation: defer**
(owned by M-RT language work, not this native sweep). **Confidence: high.**

---

## FINDING 9 — JSON niceties (`json_decode` assoc/depth/flags, `JSON_PRETTY_PRINT`, `JsonSerializable`)

**PHP capability:** `json_decode($s, $assoc, $depth, $flags)`; `json_encode($v, $flags)` with
`JSON_PRETTY_PRINT`/`JSON_UNESCAPED_SLASHES`/`JSON_UNESCAPED_UNICODE`/`JSON_THROW_ON_ERROR`;
the `JsonSerializable::jsonSerialize()` hook for custom encoding.

**Does Phorj have an equivalent?** `Core.Json` ships `parse`/`stringify`/`stringifyPretty`
(`src/native/json.rs`) over an injected `Json` enum — covering the encode/decode/pretty cases. Gaps:
- **`JsonSerializable` hook** — there is no way for a user *class* to define its own JSON shape; today
  you must build the `Json` enum by hand. Better port: a **marker interface `Core.Json.Serializable`
  with a `toJson() -> Json` method** the encoder calls if the instance implements it (interfaces +
  `instanceof` dispatch already ship, M-RT S2). Cleaner than PHP's magic-method `jsonSerialize` because
  it is a *typed interface contract* (the checker enforces the return type), not a duck-typed magic
  name. **Tier A**, **no new Op** (`NativeEval::Reflective` to see the implements-table; calls the user
  method via the re-entrant closure/method invoker that already exists for higher-order natives,
  `[[higher-order-natives-reentrant-vm]]`). **Recommendation: adopt-later.** **Confidence: medium**
  (the encoder calling back into user code re-entrantly is proven for higher-order natives, but applying
  it to the JSON walk needs care that a faulting `toJson` is byte-identical across backends).
- **`JSON_UNESCAPED_*` flags** — a `Json.stringifyWith(value, JsonOptions)` breadth follow-up;
  determinism-safe (pure string flag). **adopt-later / M4.** **Confidence: high.**

---

## FINDING 10 — Value equality / comparison / hashing as a *capability* (`spl_object_hash`, `==` vs `===`)

**PHP capability:** `spl_object_hash`/`spl_object_id` (identity); `==` (loose) vs `===` (strict);
`hash()` of a value for cache keys.

**Does Phorj have an equivalent?** Structural equality ships (`value::eq_val` / `eq_val_rec`, the
cyclic-safe one, verified). **`spl_object_id`/`spl_object_hash` are deliberately rejected** — they leak
identity/addresses (the #1 determinism trap the brief names) and have no deterministic byte-identical
analog. There is no *capability* gap here that should be a native: Phorj has no `==`/`===` ambiguity
(one structural `==`, no loose coercion — a deliberate upgrade over PHP's notorious loose `==`). The one
useful sliver — a **stable content hash of a value** for cache keys — belongs to the already-spiked
`Core.Hash` module (`feasibility-Hash.md`, hand-rolled crc32/sha256), applied to `Serde.encode(v)`
bytes: `Hash.sha256(Serde.encode(v))`. So it composes from two already-planned modules; **no new module
needed.** **Recommendation: reject** (as a standalone) — note the `Serde`+`Hash` composition in the
`Core.Serde` design. **Confidence: high.**

---

## Summary table

| PHP capability | Phorj has it? | Better port | Tier | Recommend |
|---|---|---|---|---|
| `serialize`/`unserialize` | no | `Core.Serde` (capability-free typed codec) | A | adopt-later |
| `var_export` | no | `Core.Debug.export` (re-parseable Phorj literals) | A | adopt-now |
| `gettype`/`get_debug_type` | **yes** (`Reflect.kind`/`typeName`) | — | A | reject (shipped) |
| `settype` | yes (`Core.Convert`, better) | `Convert.toBool`/`parseInt` breadth | A | defer (settype itself: reject) |
| `get_object_vars` | partial (names only) | `Reflect.entries -> Map<string,T>` (public, typed) | A | adopt-later |
| `is_callable` | yes (`kind=="callable"`) | `Reflect.isCallable` convenience | A | defer |
| `Closure::bind`/`bindTo` | no (capture-at-creation) | — (encapsulation non-goal) | — | reject |
| `Closure::fromCallable`/first-class | **yes** | — | — | reject (shipped) |
| generators / `yield` | no | language work (M6 defer) | language | defer |
| Iterator/Traversable `foreach` | no | language work (M11) + `Core.List.fromIter` | language→A | defer |
| backed-enum `cases`/`from`/`tryFrom` | no | language (M-RT) reusing reflective tables | language→A | defer |
| `json_decode` flags / pretty | mostly (`Core.Json`) | `Json.stringifyWith` + flags | A | adopt-later |
| `JsonSerializable` | no | `Core.Json.Serializable` typed interface + `toJson()` | A | adopt-later |
| `spl_object_id`/hash | no (rejected) | `Hash.sha256(Serde.encode(v))` composition | A | reject (composes) |

## Cross-cutting observations

1. **The value-walk cluster.** `Core.Debug` (dump/inspect/export), `Core.Serde` (encode/decode), and
   `Reflect.entries` all walk the closed `Value` enum + reuse `eq_val_rec`'s cyclic visited-set + the
   `ClassTables` field order. They should be **designed and built as one cluster** to single-source the
   walk skeleton and the float-rendering (`__phorj_float`) discipline. This is the single biggest
   leverage point in this area.
2. **Determinism is free here by construction** — Phorj's closed `Value`, address-free reps, and
   insertion-ordered Map/Set mean none of these need quarantine. The only recurring trap is **float
   field rendering** (Rust 17-digit vs PHP 14-digit `echo`) — every renderer/codec MUST route floats
   through the existing `__phorj_float` (Ryū) gated helper.
3. **What Phorj deliberately rejects is a feature, not a gap:** `settype` (mutate-type), `Closure::bind`
   (scope-break), `spl_object_id` (identity leak), loose `==`. Each is a documented PHP footgun the
   static/deterministic model closes by construction — the Phorj:PHP :: TS:JS upgrade lens.
4. **Two items are language-gated, not native gaps** (generators, iterator protocol, backed-enum
   statics) — flagged so the consumer routes them to M6/M11/M-RT, not the stdlib charter, but noting the
   reflective-table mechanism this module owns is the implementation substrate for the enum statics.

**Confidence overall: high** on the partition and the reuse mechanisms (all source-verified); **medium**
on the two re-entrant-callback designs (`JsonSerializable.toJson`, `Serde.decode` rebuilding instances
without running constructors) — both are buildable but carry a real "is the fault byte-identical across
three backends?" subtlety that a design slice must pin down.
