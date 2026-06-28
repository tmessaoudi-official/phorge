# Stage 2b — Adversarial Byte-Identity Refutation: Core.Dump

**Verdict: determinism does NOT hold across run/runvm/real-PHP-8.5 as designed.** The spike's
88%/Tier-A claim is over-optimistic. The Rust↔Rust spine (interpreter == VM) is sound — both read
the same closed `Value` — but the **third leg (transpiled PHP) loses structural information at the
transpile boundary** that no formatting discipline can recover. The headline mitigations the spike
relies on (ClassTables field order, Ryū float, insertion-ordered Map/Set, cycle visited-set) are all
real and correctly cited — but they cover the wrong risks. The actual breaks are in axes the spike
either never analyzed (Enum) or mis-diagnosed as "careful formatting work" (Map keys, empty
collections).

Revised feasibility: **~55%** for a full all-Value-kinds dumper on the spine. A *restricted* dumper
(no maps with bool/numeric-string keys, no empty-container kind distinction, enum format pinned)
could reach Tier A, but that is a different, smaller module than the one claimed.

---

## REFUTATION 1 (P0, blocks the claim) — Map key-type is destroyed on the PHP leg

**The break.** A Phorj `Map` value is `Rc<Vec<(HKey, Value)>>` where `HKey ∈ {Int(i64), Bool(bool),
Str(String)}` (`src/value.rs:107`). The checker explicitly admits `int`/`bool`/`string` keys
(`src/checker/expr.rs:695-725`, code `E-MAP-KEY`). A map literal transpiles to a plain PHP array
`[k => v]` (`src/transpile/expr.rs:171-177`). **PHP coerces array keys**: I verified under
`php -n` (PHP 8.5.7):

```
$m = [true => "t", false => "f", "5" => "numstr", "x" => "s"];
foreach ($m as $k => $v) var_dump($k);
=> int(1)        // bool true  -> integer 1
=> int(0)        // bool false -> integer 0
=> int(5)        // "5"        -> integer 5  (numeric string canonicalized)
=> string(1) "x"
gettype(true-key) == "integer"
```

So on the **Rust legs** a dumper iterating the `Vec<(HKey,Value)>` holds `HKey::Bool(true)` and would
render e.g. `true => …` or `bool(true) => …`. On the **PHP leg** the original key type is *already
gone* before `__phorj_dump` runs — the array only has `int(1)`. A PHP dumper reading
`foreach ($arr as $k => $v)` sees `1`, an integer, and cannot reconstruct that it was a bool, nor
that `"5"` was a string. **One byte (at minimum) differs; usually the whole key token differs.**

This is not the "Map/Set iteration order" risk the spike's §4 table addresses (that is about *order*,
and is correctly mitigated by the insertion-ordered `Rc<Vec>`). Key *type fidelity* is a separate
axis the spike never names. The closed-`Value` "no address to leak by construction" argument does not
save it: the information loss happens in the PHP value representation, not in what the dumper chooses
to print.

**Severity.** `Map<int, string>` happens to survive (PHP does not coerce already-integer keys), and
that is the one shipped example (`examples/guide/maps.phg:20`). That is exactly why a naive
guide example would pass and mask the bug — the dangerous cases (`Map<bool, V>`, a `Map<string, V>`
whose keys are numeric strings like `"5"`/`"007"`/`"0"`/`"+1"`) are constructible, type-checked, and
would silently diverge. A differential example using `["5" => 1]` or `[true => 1]` fails immediately.

**No clean fix on the spine.** To preserve key type the transpiler would have to stop emitting Phorj
maps as bare PHP arrays (e.g. wrap them in a tagged object carrying the original key kind) — a
language-wide representation change far outside a "new native + PHP helper" dumper, and one that would
ripple into every existing Map native and the byte-identity of `maps.phg` itself.

## REFUTATION 2 (P0) — Empty Map vs empty List are indistinguishable on the PHP leg

**The break.** `Value::List([])` and an emptied `Value::Map` both transpile to PHP `[]`. Verified:
`array_is_list([]) === true` under `php -n`. The spike's §5 proposes disambiguating List vs Map with
"the Json helper's `array_is_list` predicate" and even flags in §9.5 that "a wrong predicate makes an
empty map and empty list collide" — but then offers `array_is_list` *as the mitigation*, which is the
very predicate that yields `true` for an empty map. There is **no runtime PHP signal** that separates
`[]`-from-a-List and `[]`-from-a-Map.

The Rust legs know the static kind (`Value::List` vs `Value::Map` are distinct enum arms), so a Rust
dumper renders `[]` vs `{}` (or `[]` vs `[:]`) deterministically; the PHP leg cannot. Result: an empty
map dumps as a list (or vice versa) on exactly one leg → divergence. A `Debug.dump([:])` example
(empty map) breaks the harness.

(Non-empty maps with at least one key are disambiguable by key shape — but see Refutation 1, which
breaks the non-empty case for bool/numeric-string keys anyway. The two refutations together leave only
"non-empty map with at least one genuinely-non-numeric string key, or at least one int key" as the
safe Map subset.)

## REFUTATION 3 (P1) — Enum rendering is entirely unanalyzed (third structural axis)

`Value::Enum(EnumVal{ ty, variant, payload: Vec<Value> })` (`src/value.rs:98-101`) is a first-class
Value kind. The spike's §4 trap table enumerates Instance / Float / Map / Set / Cycle / Decimal /
Closure / Bytes — and **never mentions enums**. `grep -in enum` on the spike finds only prose about
the *Rust* `Value` enum, never the Phorj enum value kind. Yet enums are pervasive (Option/Result,
RoundingMode, the injected Json enum, every user `enum`). The PHP transpile target for an enum (its
runtime object/array shape) must be rendered byte-identically to the Rust `ty::variant(payload…)`
form, and the payload recursion must agree — an unbudgeted, non-trivial format-pinning task with its
own potential coercion traps (e.g. an enum carrying a Map payload inherits Refutations 1 & 2). At
minimum this invalidates the "every divergence axis has an already-shipped single-source mitigation"
claim (§4 "Net").

## REFUTATION 4 (P2, but real) — Float edge cases beyond the KNOWN_ISSUE caveat

The spike routes floats through `__phorj_float` (correct, and the PHP helper at
`transpile/program.rs:300+` does handle `NaN`→`"NaN"`, `±inf`, and signed-zero `-0`). So ordinary and
even special floats are actually OK *if and only if* the Rust renderer uses the identical tokens
(`"NaN"`, `"inf"`/`"-inf"`, `"-0"`/`"0"`). But the spike says the Rust side uses
`as_display`/`format!("{x}")` — and `format!("{x}")` for `f64::NAN` yields `"NaN"`, for infinity
`"inf"`/`"-inf"`, and for `-0.0` yields `"-0"` — these happen to match, but **only by luck and only if
the dumper calls `as_display`, not a different `format!`**. The spike's §11 lists `format!("{x}")`
directly as a relied-on API; a dumper author following §11 literally (not §4's `as_display`) on
`-0.0` gets `"-0"` from Rust Display too, so this one is consistent — downgrade to P2. The residual
risk is the standing KNOWN_ISSUE (irrational/14-digit-divergent floats) which the spike does
acknowledge. Not a fresh break, but it narrows the example surface further.

## REFUTATION 5 (P2) — String/Bytes escape scheme is asserted, not pinned

The spike (§5, §9.3) correctly identifies that the escape scheme must be byte-identical and warns
against `addslashes`. But it leaves the actual scheme unspecified ("pin one escape scheme both legs").
Concrete trap: a string containing a raw `\xHH` non-printable, an embedded NUL, or a multi-byte UTF-8
sequence. The Rust side operates on a Rust `String` (UTF-8, char-oriented); the PHP side operates on a
PHP byte-string with **no mbstring under `php -n`**. A char-vs-byte escape decision (e.g. escaping
"non-printable" by Unicode category vs by byte value) diverges. Resolvable, but it is genuinely
"meticulous work" the spike's 12% is supposed to cover — except the spike spends that budget on the
escape scheme while the Map/Enum P0s sit outside its accounting entirely.

---

## What the spike got RIGHT (verified, not disputed)

- `NativeEval::Reflective(fn(&[Value], &ClassTables))` exists and is dispatched in both backends
  (`src/native/mod.rs:93`, interpreter call path). No new Op needed — correct.
- `ClassTables.fields` is a `BTreeMap<String, Vec<String>>`, sorted/transitive
  (`src/native/mod.rs:110`). Reading field NAMES from it (not iterating the `RefCell<HashMap>` at
  `value.rs:94`) is the right call and is byte-identical across legs. Instance *field order* holds.
- The `eq_val_rec` cycle pattern is real (`src/value.rs:274-345`, `Vec<(*const Instance, *const
  Instance)>`); a single-instance visited `Vec<*const Instance>` for cycle detection is sound and
  terminates deterministically. Cycles hold (the token is fixed, the id never printed).
- `fmt_decimal` (`value.rs:847`) is the correct single source for decimals; BCMath string `(string)`
  agreement is the M-NUM S1 invariant. Decimals hold.
- Closures → fixed `<closure>` token, no address. Holds.
- `php -n` missing-extension trap: the dumper needs only `strlen`/`str_repeat`/`bin2hex`/concat —
  all core, survive `php -n`. Correct (no `mb_*`).

The Rust-vs-Rust spine (run == runvm) is **not** in doubt — both read the same `Value`. Every break
is at the PHP leg, where the transpile representation has already discarded information the Rust
`Value` retains.

---

## Bottom line

`determinism_holds = false`. The 88% claim treats the problem as "pin a format carefully"; the real
problem is "the PHP value representation is lossy relative to the Rust `Value`, on the spine, for two
of the closed enum's arms (Map keys, empty containers) and one unanalyzed arm (Enum)." A full
all-Value-kinds `Core.Dump` is **not** Tier A as designed. A *restricted* dumper is feasible at Tier A
if scoped to: no `Map<bool, V>`; no map whose string keys are numeric; no empty-container kind
distinction (render both `[]`); enum format pinned and added to the trap table. Recommend revising to
~55% and tier=A-only-with-restrictions (effectively mixed), with the Map-key and empty-collection
losses documented as KNOWN_ISSUES that constrain the example surface — exactly the way the float
KNOWN_ISSUE already constrains it, but for *structure*, not just float precision.
