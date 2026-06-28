# Design — `Core.Dump` deterministic value-dumper format

**Stage 4 (DESIGN).** Module: `Core.Dump`. Tier A (pure/deterministic), std-only, **no new VM Op**,
fits the existing `NativeEval::Reflective` dispatch with zero new plumbing. This document specifies the
EXACT textual format for every `Value` kind, circular-ref handling, depth limiting, string truncation,
and indentation — then the IDENTICAL `__phorj_dump()` PHP helper so transpiled output is byte-for-byte
equal to the two Rust backends.

Confidence: **high** on mechanism + determinism argument (every divergence axis traces to a verified
single-source mechanism already in the repo); **medium** on a small number of cosmetic format choices
(they are design calls, caught instantly by `differential.rs`, not feasibility risks).

This design *supersedes* two claims in `feasibility-Dump.md` that I verified to be wrong against the
actual repo (see §0). Read §0 before anything else.

---

## 0. Corrections to the feasibility spike (verified against the repo)

The feasibility spike (`feasibility-Dump.md`) is sound on the mechanism, but two of its specifics are
wrong and one trap is missing. All three are load-bearing for byte-identity:

1. **String escape scheme (spike §5/§9.3 is WRONG).** The spike says "escape `"`, `\`, `\n`, `\t`,
   `\r`". But the repo's actual `php_escape` (`src/transpile/mod.rs:729`) escapes only `\` → `\\`,
   `"` → `\"`, `$` → `\$` and leaves `\n`/`\t`/`\r` **literal**. I am NOT reusing `php_escape` for the
   dumper (it escapes `$`, which the Rust side has no reason to). Instead I pin a **dump-specific
   escape scheme** (§5.4) identical on both legs: escape `\` `"` `\n` `\t` `\r`, everything else
   literal. The escaper is a new shared helper, written once per leg, byte-matched.

2. **CRITICAL missing trap — `Set` and `Bytes` lose their Phorj type on the PHP leg.** I verified:
   - A Phorj **`Set`** transpiles to a PHP **plain list-array** (`array_is_list($x) === true`) — it is
     *runtime-indistinguishable from a `List`* on the PHP side (`src/native/set.rs`: a Set is just a
     deduped `array`).
   - A Phorj **`Bytes`** transpiles to a PHP **string** (`src/transpile/expr.rs:185`,
     `php_escape_bytes`) — *runtime-indistinguishable from a `Str`* on the PHP side.

   So a PHP `__phorj_dump($v)` that sniffs `is_array`/`is_string` **cannot** tell `Set` from `List` or
   `Bytes` from `Str`. The two Rust backends *can* (the `Value` enum is closed and tagged). A runtime
   dispatch on the PHP leg therefore CANNOT be byte-identical for these two kinds.

   **Resolution (the design's central decision):** the dump native's `php` mapping is **static-type
   driven at the call site**, exactly like `Convert.toInt`/`Reflect.kind` already are
   (`src/transpile/call.rs:100-170`). The transpiler knows the *static* Phorj type of the argument
   expression. It emits a **type-tagged** call so the PHP helper renders the correct kind:
   `__phorj_dump($x, 'set')` / `__phorj_dump($x, 'bytes')` / `__phorj_dump($x)` (untagged → runtime
   sniff for everything else). See §5.2. This keeps `Set`/`Bytes` byte-identical without any runtime tag
   on the PHP value.

3. **Naming.** The spike floats `Core.Debug`. The stage header pins **`Core.Dump`** — I follow the
   header (module = `Core.Dump`); the registry key is a one-token change if the developer later prefers
   `Core.Debug`. Function names: `Dump.dump(value)` and `Dump.inspect(value, depth)`.

---

## 1. Why this is deterministic (the partition argument)

`Core.Dump` is **Tier A (pure)**: the output is a pure function of (the already-materialized `Value`)
+ (the program's `ClassTables`, which all three backends build identically via
`ClassTables::from_program`, `src/native/mod.rs:118`). There is:

- **no clock / RNG / filesystem / locale / env** → no Tier-B impurity (`pure: true`);
- **no address or object-id ever printed.** The closed `Value` enum has no address to leak unless we
  *choose* to print `Rc::as_ptr` — which we never do. Cycle detection uses `Rc::as_ptr` only as a
  *visited-set key* (never rendered); the PHP leg uses `spl_object_id` the same gated way (§4);
- **no unordered iteration.** Every container the dumper walks has a *fixed* order:
  - `Value::List` / `Value::Map` / `Value::Set` are insertion-ordered `Rc<Vec<…>>`
    (`src/value.rs:37/43/49`) — iterate the `Vec` directly, never re-bucket;
  - `Instance.fields` is a `HashMap` (`src/value.rs:94`) whose iteration order is **non-deterministic** —
    so the dumper **never iterates it directly**. It takes the field-name list from
    `ClassTables.fields` (a `BTreeMap<String, Vec<String>>`, `src/native/mod.rs:110` → **sorted,
    transitive-with-inheritance, deterministic**) and looks each name up in the instance map. This is
    exactly what `Reflect.fields` and the transpiler's `__phorj_reflect_of` static table already do, so
    field order is *already* byte-identical across all three legs;
- **float rendering single-sourced** through Rust shortest-round-trip (`format!("{x}")` via
  `as_display`, `src/value.rs:242`) on the Rust legs and `__phorj_float` (positional Ryū,
  `src/transpile/program.rs:312`) on the PHP leg — the same pairing every existing float-touching native
  uses;
- **decimal rendering single-sourced** through `fmt_decimal(unscaled, scale)` (`src/value.rs:847`); the
  emitted PHP BCMath string `(string)`s to the same form (M-NUM S1 invariant).

Because every axis of potential divergence resolves to an **already-shipped single-source mechanism**,
the format is Phorj-owned: *we define what byte-identical means*, and the differential harness enforces
it.

---

## 2. Output shape overview

`Dump.dump(v)` produces a multi-line, indented, type-annotated tree. Scalars are single-line; compounds
nest with **2-space indentation per level**. The renderer is the same on all three legs.

Top-level grammar (informal):

```
dump        := scalar | compound
scalar      := int | float | bool | "null" | "unit" | string | bytes | decimal | closure
compound    := list | map | set | instance | enum
```

A nested value is rendered at `indent = parentIndent + 1`. The opening token of a compound stays on the
current line; children go on their own indented lines; the closing bracket aligns with the opening
line's indent. Empty compounds render on one line (`[]`, `{}`, `Set {}`, `Foo {}`, `None`).

### 2.1 Worked example

```phorj
import Core.Dump;
import Core.Console;

class Point { public int x; public int y; }

Console.println(Dump.dump(42));                 // 42
Console.println(Dump.dump(3.5));                // 3.5
Console.println(Dump.dump(true));               // true
Console.println(Dump.dump("hi\n"));             // "hi\n"        (escaped, one line)
Console.println(Dump.dump([1, 2, 3]));          // see below
Console.println(Dump.dump(["a" => 1, "b" => 2])); // see below
Console.println(Dump.dump(Point(1, 2)));        // see below
```

`Dump.dump([1, 2, 3])`:

```
[
  0 => 1,
  1 => 2,
  2 => 3,
]
```

`Dump.dump(["a" => 1, "b" => 2])`:

```
{
  "a" => 1,
  "b" => 2,
}
```

`Dump.dump(Point(1, 2))` (fields sorted by `ClassTables` → `x`, `y`):

```
Point {
  x: 1,
  y: 2,
}
```

A short scalar list still expands one-per-line (above). **Rationale:** a single layout rule (always
expand non-empty compounds, one child per line, trailing comma) is *far* easier to keep byte-identical
across three legs than a width-sensitive "inline if it fits" rule, which would need an identical
line-width heuristic in PHP. Inline-if-short is explicitly **rejected** (medium-confidence design call;
it would add a divergence axis for zero correctness value). Empty compounds are the only one-line
compound form.

---

## 3. EXACT per-kind format

Indentation unit `IND = "  "` (two spaces). `pad(n) = IND repeated n times`. `d` = current depth
(top-level call is `d = 0`). All tokens below are the *rendered text*, byte-exact.

### 3.1 `Int(n)`
Render `n.to_string()` (Rust) / `(string)$v` (PHP, an int is exact). No type annotation, no quotes.
```
42
-7
0
```

### 3.2 `Float(x)`
Render via the **float single-source**: Rust `format!("{x}")` (== `as_display`); PHP `__phorj_float($v)`
(gate `uses_float = true` when the dump helper is emitted). Shortest round-trip, positional, never
scientific; integer-valued floats print without a trailing `.0` (the existing `__phorj_float`
behavior — verified `src/transpile/program.rs:312`). Examples must use exactly-representable floats
(standing KNOWN_ISSUE; irrational floats diverge at PHP's 14-digit `echo`, unrelated to the dumper).
```
3.5
-0.25
```

### 3.3 `Bool(b)`
`"true"` / `"false"` (lowercase). Rust: `b.to_string()`. PHP: `$v ? "true" : "false"`.
```
true
false
```

### 3.4 `Null`
The literal `null`. Rust: `"null"`. PHP: `"null"` (guard `$v === null` **before** any other test, since
PHP `null` is falsy/ambiguous).
```
null
```

### 3.5 `Unit`
The literal `unit` (matches `as_display`'s `Value::Unit => "unit"`, `src/value.rs:250`). Rust: `"unit"`.
PHP: there is no PHP value for `Unit` in normal dumps (it is the empty/void result); if it ever reaches
the dumper it is `null`-shaped — but to stay faithful, `Unit` is **statically known at the call site**
when the argument type is `void`/`Empty`. In practice `Unit` is not a first-class dumpable value in
Phorj programs, so this arm is defensive. Rust renders `unit`; the PHP leg is reached only via a
statically-`void` argument, which is degenerate — **deferred / not in any gated example** (a `void`
expression cannot be a value argument in Phorj anyway). State explicitly: `Unit` is unreachable as a
real argument; the Rust arm exists for totality only.

### 3.6 `Str(s)`
A double-quoted, escaped, **single-line** string. Escape scheme (pinned, §5.4): `\` → `\\`, `"` → `\"`,
newline → `\n`, tab → `\t`, carriage-return → `\r`; every other byte literal (UTF-8 passes through).
Then apply **truncation** (§6): if the *unescaped* char length > `MAX_STR` (default 100), keep the first
`MAX_STR` chars, append `…` (U+2026) *inside* the quotes, then `"` — and append a length suffix
` (len N)` *outside* the quotes. Truncation operates on Unicode scalar values (Rust `chars()`), and on
the PHP leg on `mb`-free byte-safe char iteration of the *already-UTF-8* string (we count code points
with a tier-1-only scan — `preg_match_all('/./us', …)`, PCRE is core under `php -n`; see §5.5).
```
"hi"
"line1\nline2"
"aaaaaaaa… (len 240)"
```
*(Truncation default `MAX_STR = 100`; tunable only as a future `inspect` parameter — not in v1.)*

### 3.7 `Bytes(b)`
`b"…"` with each octet: printable ASCII `0x20..=0x7E` verbatim (with `\` `"` escaped), every other octet
`\xHH` **lowercase** (matches the `b"…"` literal lexer `src/lexer/mod.rs:672` and `php_escape_bytes`).
**Two-digit always** (so PHP's greedy `\x` can't merge with a following hex char). Truncation (§6):
if `b.len() > MAX_BYTES` (default 64) keep the first `MAX_BYTES` octets, then `…`, then close `"`, then
` (len N)`.
```
b"hello"
b"\x00\x01\xff"
```
**PHP leg:** the static type is `bytes`, so the transpiler emits `__phorj_dump($x, 'bytes')`; the helper
runs a `bin2hex`-free per-octet loop (`ord()`, `sprintf("\\x%02x", …)`) — tier-1 only, survives `php -n`.

### 3.8 `Decimal { unscaled, scale }`
Render `fmt_decimal(unscaled, scale)` (`src/value.rs:847`) with a trailing `d` to disambiguate from a
float/int. Rust: `format!("{}d", fmt_decimal(u, s))`. PHP: the BCMath string already `(string)`s to the
`fmt_decimal` form (M-NUM S1); append the literal `d`. The static type is `decimal`, so the call emits
`__phorj_dump($x, 'decimal')` (PHP can't distinguish a BCMath decimal-string from a plain numeric
string at runtime).
```
19.99d
-0.50d
```

### 3.9 `Closure(_)`
A fixed token `<closure>` — **no address, no arity, no identity** (a closure has no stable printable
identity). Rust: `"<closure>"`. PHP: `is_callable($v)` (or static type `function`) → `"<closure>"`.
```
<closure>
```

### 3.10 `List(xs)` — ordered, integer-keyed
A list renders its elements with **explicit integer index keys** (`i => value`), one per line, trailing
comma, surrounded by `[` … `]`. Index keys make a `List` visibly distinct from a `Map` and make element
position legible. Empty → `[]` (one line).
```
[
  0 => 1,
  1 => "two",
  2 => [
    0 => 3,
  ],
]
```
*(Design call, medium confidence: integer keys vs bare values. Chosen so List-vs-Map is unambiguous on
the page AND because the PHP leg already keys list arrays `0,1,2,…` — `foreach ($v as $k => $x)` yields
the same keys, byte-identical for free.)*

### 3.11 `Map(pairs)` — ordered key → value
Surrounded by `{` … `}`. Each entry `key => value`, one per line, trailing comma, **in the `Vec`'s
insertion order** (never re-bucketed). The key is rendered by the **same scalar renderer** restricted to
the hashable subset (`HKey::{Int, Bool, Str}`): an `Int` key prints bare (`3 =>`), a `Bool` key prints
`true`/`false`, a `Str` key prints quoted+escaped (`"a" =>`). Empty → `{}` (one line).
```
{
  "a" => 1,
  "b" => {
    "nested" => true,
  },
}
```
**PHP leg:** a Map transpiles to a PHP keyed-array. Disambiguate List vs Map with **`array_is_list($v)`**
(the same predicate the Json helper relies on): `array_is_list` true → render as List (§3.10); false (or
the array is empty *and* statically a Map — see below) → render as Map. **Empty-collision caveat:** an
empty PHP array is `array_is_list([]) === true`, so an empty `Map` and an empty `List` collide on the PHP
leg. Resolve it the same way as Set/Bytes: when the static type is a `Map`, emit `__phorj_dump($x,
'map')`; the helper then renders `{}` for an empty array instead of `[]`. A non-empty array is
unambiguous via `array_is_list`, so the tag matters only for the empty case (but emit it unconditionally
for any `Map`-typed argument — it costs nothing and removes the edge).

### 3.12 `Set(elems)` — ordered, deduped, `Set { … }`
Surrounded by `Set {` … `}` (the `Set` prefix is the *only* thing distinguishing it textually from a
List on the page, and — critically — the only thing the PHP leg has, since a Set is a plain list-array at
runtime). Elements one per line (rendered by the scalar renderer over `HKey`), trailing comma, insertion
order. Empty → `Set {}`.
```
Set {
  1,
  2,
  3,
}
```
**PHP leg:** a Set is runtime-indistinguishable from a List → the transpiler emits `__phorj_dump($x,
'set')` (static type drives it). The helper, seeing the `'set'` tag, renders the `Set { … }` wrapper and
walks the array as values (no keys).

### 3.13 `Instance(inst)` — `ClassName { field: value, … }`
`ClassName {` then each instance field **in `ClassTables.fields[ClassName]` (sorted, inherited) order**,
`fieldName: value`, one per line, trailing comma, closing `}` at the opening indent. A field absent from
the instance map (shouldn't happen — EV-1 fully constructs) renders `fieldName: <unset>` (defensive,
unreachable in valid programs). Empty class → `ClassName {}`.
```
Point {
  x: 1,
  y: 2,
}
```
**Field order single-source:** `ClassTables.fields` (`BTreeMap`, sorted; `__phorj_reflect_of`'s static
table emits the identical sorted list on the PHP leg). The PHP helper reads the field names from the
**same static class table** `__phorj_reflect_of` builds (§5.3), then `$obj->$name` for each — so the
order is byte-identical by construction. **Never** `get_object_vars($obj)` (insertion/declaration order,
divergent).

### 3.14 `Enum(ev)` — `TypeName.Variant(payload…)`
`TypeName.Variant` for a zero-payload variant; `TypeName.Variant(p0, p1, …)` for a payload, with each
payload element rendered by the full dumper (nested compounds indent). Payload is positional (no field
names — enum payloads are positional in Phorj). A multi-line payload nests:
```
Color.Red
Shape.Circle(2.5)
Tree.Node(
  Tree.Leaf(1),
  Tree.Leaf(2),
)
```
*(Single-line when every payload element is a scalar; multi-line only when a payload element is itself a
compound. This is the one place a compound renders inline — for a scalar payload — because the
`Variant(...)` call syntax is the natural Phorj form. The same rule applies on all three legs: a payload
element that is a compound forces the multi-line form. Medium confidence; trivially byte-matched.)*
**PHP leg:** a Phorj enum transpiles to a PHP class hierarchy with a discriminant. The dumper reads the
type/variant from the emitted enum representation (the same `__phorj_reflect_of`-style static info, or
the enum object's known shape — to be pinned in implementation against the actual enum PHP emission;
`src/transpile` enum lowering). This is the **one kind whose PHP rendering needs an implementation-time
check** against the enum lowering; flagged as the highest-effort sub-part (still front-end-only).

---

## 4. Circular-reference handling

Instances became shared-mutable handles in M-mut.6 — `a.next = b; b.next = a` is constructible. An
unguarded recursive dumper would overflow the native stack at *different* depths per backend (breaking
`agree_err`).

**Mechanism — reuse the `eq_val_rec` visited-set pattern verbatim** (`src/value.rs:274-341`):

- The dumper carries a **path-scoped** `Vec<*const Instance>` of the instances currently on the recursion
  path (a `Rc::as_ptr` per `Value::Instance` — `src/value.rs:328` does exactly this for `==`).
- Before descending into an instance's fields: if `Rc::as_ptr(inst)` is already in the path-set, emit the
  fixed token **`<circular>`** in place of the instance and **do not recurse**. Otherwise push the
  pointer, render the fields, pop the pointer (path-scoped — a diamond that is not a cycle still renders
  fully on each path; only a true back-edge is cut).
- `Rc::as_ptr` is used **only as a set key, never printed** — so no address leaks. This is co-inductive
  and terminates deterministically, identical to how `==` already handles cycles.

Only `Instance` can form a cycle (List/Map/Set are immutable+acyclic value containers — their `Rc`
contents can't point back to a parent instance through a mutable edge in M1+M-mut except *via* an
`Instance`, so tracking instances alone is sufficient — the same scope `eq_val_rec` uses).

**PHP leg:** track visited instances in a `&$seen` set keyed by `spl_object_id($v)` — **used ONLY to gate
the `<circular>` token, never printed**. Push on entry, `unset` on exit (path-scoped, mirrors the Rust
`Vec` pop). `spl_object_id` is tier-1, survives `php -n`.

```
Node {
  next: Node {
    next: <circular>,
  },
}
```

---

## 5. EXACT PHP transpile target

### 5.1 Integration shape (cheapest possible)

`Dump.dump` / `Dump.inspect` are ordinary native calls → `Op::CallNative(idx, argc)` (the existing path,
**no new Op**). They need `ClassTables`, so they are
`NativeEval::Reflective(fn(&[Value], &ClassTables) -> Result<Value, String>)` — a variant that **already
exists and is already dispatched in both backends** (interpreter `src/interpreter/call.rs`, VM via the
`class_tables` the chunk carries). New code: one `src/native/dump.rs` leaf file (the established
one-file-per-module convention) + the gated PHP helper + a `uses_dump` flag + a guide example. No
checker/compiler surgery beyond standard generic-native registration (`T value` reuses the S7a
`Ty::Param` native path; erased pre-backend).

### 5.2 Static-type-tagged emission (the Set/Bytes/Map/Decimal fix)

Because `Set→list-array`, `Bytes→string`, `Decimal→numeric-string`, and `empty Map→empty list-array` are
runtime-indistinguishable on the PHP leg (§0.2), the transpiler emits a **type tag** derived from the
**static** Phorj type of the argument, mirroring the existing `Reflect.kind`/`Convert.toInt` call-site
dispatch (`src/transpile/call.rs:100-170`). In the `Core.Dump` arm of `emit_call`:

```rust
if nat.module == "Core.Dump" && nat.name == "dump" {
    self.uses_dump = true;                       // gate the helper
    let tag = match static_ty_of(&args[0]) {     // the transpiler's existing static-type resolver
        Ty::Set(_)            => Some("'set'"),
        Ty::Bytes             => Some("'bytes'"),
        Ty::Decimal          => Some("'decimal'"),
        Ty::Map(..)           => Some("'map'"),   // disambiguates the empty-map edge
        Ty::Function(..)      => Some("'closure'"),
        _                     => None,            // int/float/bool/str/null/list/instance/enum: runtime sniff
    };
    // emit __phorj_dump($arg) or __phorj_dump($arg, <tag>)
}
```

`inspect(value, depth)` emits `__phorj_dump_depth($arg, <depth>, <tag?>)` (same tag logic).
**No type tag reaches a backend value** — it is a *literal string argument the transpiler bakes into the
call*. The two Rust backends ignore the concept entirely (they read the closed enum tag directly).

### 5.3 The gated `__phorj_dump` helper (emitted in `emit_runtime_helpers`)

Modeled exactly on `__phorj_reflect_of` (`src/transpile/program.rs:816`) for the field table, and
`__phorj_float`/`__phorj_json_encode` for the gated-recursive-helper pattern. Gated by `uses_dump`.
Reuses `__phorj_float` (sets `uses_float`) and the `__phorj_reflect_of` static field table (sets
`uses_reflect_tables`). Pseudocode (the literal emission, indentation elided):

```php
function __phorj_dump($v, $tag = null) { $seen = []; return __phorj_dump_inner($v, $seen, 0, PHP_INT_MAX, $tag); }
function __phorj_dump_depth($v, $depth, $tag = null) { $seen = []; return __phorj_dump_inner($v, $seen, 0, $depth, $tag); }

function __phorj_dump_inner($v, &$seen, $ind, $maxDepth, $tag) {
    // depth elision FIRST (so an over-deep compound is elided, scalars below cap still print)
    // see §6 for the exact trigger condition

    // --- type-tag branches (Set/Bytes/Decimal/Map/closure forced by the static tag) ---
    if ($tag === 'closure') { return "<closure>"; }
    if ($tag === 'bytes')   { return __phorj_dump_bytes($v); }            // b"\xHH…" lowercase, §3.7
    if ($tag === 'decimal') { return (string)$v . "d"; }                   // §3.8 (BCMath string already fmt_decimal form)
    if ($tag === 'set')     { return __phorj_dump_set($v, $seen, $ind, $maxDepth); }   // "Set { … }", §3.12
    if ($tag === 'map')     { return __phorj_dump_map($v, $seen, $ind, $maxDepth); }   // "{ … }" even when empty, §3.11

    // --- untagged: runtime sniff (int/float/bool/null/str/list/instance/enum) ---
    if ($v === null)        { return "null"; }                             // BEFORE is_* (null is falsy)
    if (is_bool($v))        { return $v ? "true" : "false"; }
    if (is_int($v))         { return (string)$v; }
    if (is_float($v))       { return __phorj_float($v); }                 // float single-source
    if (is_string($v))      { return __phorj_dump_str($v); }              // "…" escaped+truncated, §3.6
    if (is_object($v)) {
        // enum object vs class instance — distinguished by the emitted enum representation (impl-time, §3.14)
        if (__phorj_is_enum($v)) { return __phorj_dump_enum($v, $seen, $ind, $maxDepth); }
        $id = spl_object_id($v);
        if (isset($seen[$id])) { return "<circular>"; }                    // §4 — id gates token, never printed
        $seen[$id] = true;
        $cls    = get_class($v);
        $fields = __phorj_reflect_of($v, 'fields');                       // SAME sorted table as Reflect, §3.13
        if (count($fields) === 0) { unset($seen[$id]); return $cls . " {}"; }
        $pad = str_repeat("  ", $ind + 1);
        $out = $cls . " {\n";
        foreach ($fields as $name) {
            $out .= $pad . $name . ": " . __phorj_dump_inner($v->$name, $seen, $ind + 1, $maxDepth, null) . ",\n";
        }
        unset($seen[$id]);
        return $out . str_repeat("  ", $ind) . "}";
    }
    if (is_array($v)) {                                                    // untagged array ⇒ List or non-empty Map
        if (array_is_list($v)) { return __phorj_dump_list($v, $seen, $ind, $maxDepth); }   // [ i => … ], §3.10
        return __phorj_dump_map($v, $seen, $ind, $maxDepth);              // { k => … }, §3.11
    }
    return "<unknown>";                                                    // unreachable (totality)
}
```

`__phorj_dump_list/_map/_set/_enum/_str/_bytes` are small sibling helpers, each gated under `uses_dump`
(emitted as a block). All use only tier-1 PHP (`str_repeat`, `count`, `foreach`, `ord`, `sprintf`,
`spl_object_id`, `array_is_list`, `preg_*` core, `get_class`) — **all survive `php -n`** (no `mb_*`, no
`json_encode` for the structural rendering — only `__phorj_float` for floats).

### 5.4 Pinned string-escape scheme (byte-matched, both legs)

The dumper's string escaper is a NEW helper (NOT `php_escape`, which escapes `$` and not `\n`). Both legs
implement the identical map; only these five sequences are escaped, everything else literal:

| input byte/char | output |
|---|---|
| `\` | `\\` |
| `"` | `\"` |
| `\n` (0x0A) | `\n` |
| `\t` (0x09) | `\t` |
| `\r` (0x0D) | `\r` |
| anything else | literal (UTF-8 passes through) |

Rust: a `String::with_capacity` loop over `s.chars()`. PHP: a loop over the bytes (a Phorj `Str` is
valid UTF-8 → byte-level escaping of these five ASCII bytes is safe, multibyte sequences pass through
untouched). **Do NOT use `addslashes`** (it escapes `'` `"` `\` `NUL` only — different set). This is the
one meticulous-but-routine spot; the differential harness catches any drift immediately.

### 5.5 String truncation on the PHP leg (char-count without mbstring)

`MAX_STR` counts **Unicode scalar values** (to match Rust `chars()`). Under `php -n`, `mb_strlen` is
absent. Use PCRE (core): `preg_match_all('/./us', $s, $m)` → `count($m[0])` code points; slice the first
`MAX_STR` with `preg_split` / array slice + `implode`. The `…` marker is U+2026 (`"\u{2026}"` in Rust,
`"\xe2\x80\xa6"` literal UTF-8 in the emitted PHP — pin the bytes, do not rely on a PHP escape). The
` (len N)` suffix uses the code-point count `N` (same on both legs). *(For `Bytes`, truncation counts
**octets**, `strlen`/Rust `.len()` — no PCRE needed.)*

---

## 6. Depth limiting & elision

`Dump.dump(v)` uses an **unbounded** depth (`PHP_INT_MAX` / `usize::MAX`) — cycles are cut by §4, so an
acyclic graph always terminates. `Dump.inspect(v, depth)` caps recursion at `depth` levels.

**Exact trigger (pinned identically on all three legs):** at the top of the recursive worker, if the
current `depth` (number of compound levels already descended; top-level call is `d = 0`) **equals or
exceeds `maxDepth`** AND the current value is a **compound** (List/Map/Set/Instance/Enum-with-payload),
emit the elision token **`…`** (U+2026, same bytes as §5.5) *in place of the whole compound* and do not
recurse. A scalar at any depth always renders fully (it has no children to elide). A zero-payload enum
variant is a scalar for this purpose (renders fully).

```phorj
Dump.inspect([[1, 2], [3, 4]], 1)
```
```
[
  0 => …,
  1 => …,
]
```
*(At `d = 0` the outer list renders; its elements are compounds at `d = 1 == maxDepth` → each elided.)*

`maxDepth` is the second arg to `inspect`, threaded as a plain `int` through the native and baked into
the PHP `__phorj_dump_depth($v, $depth, $tag)` call. **The "at-or-below" boundary (`d >= maxDepth`) and
the "compound only" predicate must be byte-identical across legs** (named risk — §8.7). I pin
`d >= maxDepth && is_compound(v)` as the single rule.

---

## 7. Phorj API & registry

```phorj
import Core.Dump;

Dump.dump(value)          // T value -> string   (unbounded depth, cycle-safe)
Dump.inspect(value, depth) // (T value, int depth) -> string   (depth-capped)
```

Registry entries in `src/native/dump.rs` (keyed `(module, name)` per the existing convention):

```rust
NativeFn {
    module: "Core.Dump", name: "dump",
    params: vec![ParamSig::generic("T")],          // T value  (S7a Ty::Param native path; erased pre-backend)
    ret: Ty::Str,
    pure: true,
    eval: NativeEval::Reflective(dump_value),       // reads &ClassTables for field order
    php: /* gated: emit __phorj_dump($arg [, tag]); set uses_dump */,
},
NativeFn {
    module: "Core.Dump", name: "inspect",
    params: vec![ParamSig::generic("T"), ParamSig::int("depth")],
    ret: Ty::Str,
    pure: true,
    eval: NativeEval::Reflective(inspect_value),
    php: /* gated: emit __phorj_dump_depth($arg, $depth [, tag]); set uses_dump */,
},
```

`pure: true` → included in the differential spine (unlike Process/Env `pure:false`). The `php` mapping
sets `uses_dump` at the call site (a native's `php` closure has no `&mut self`, so the flag is set in the
`Core.Dump` arm of `emit_call`, exactly as `Reflect`/`Json`/`Convert` do today — §5.2).

**Color is explicitly deferred** (a TTY check is environment-dependent → not Tier A). If ever wanted:
a separate `Dump.colorize(string) -> string` post-pass, never in a gated example.

---

## 8. Determinism risks (named, each with a single-source mitigation)

1. **Instance field order** — read `ClassTables.fields` (sorted `BTreeMap`), never iterate
   `Instance.fields` (`HashMap`). The PHP leg reads the identical `__phorj_reflect_of` static table.
   *If a future change iterates the HashMap directly, byte-identity silently breaks.*
2. **Float rendering** — route through `as_display`/`format!("{x}")` (Rust) and `__phorj_float` (PHP);
   a naked `(string)$float` diverges. Examples restricted to exactly-representable floats.
3. **String escape scheme** — pin the §5.4 five-sequence map identically; do NOT reuse `php_escape`
   (escapes `$`, not `\n`) or `addslashes` (different set).
4. **Set/Bytes/Decimal/empty-Map type erasure on the PHP leg** — the §0.2/§5.2 **static-type tag** is the
   only correct fix; a runtime `is_array`/`is_string` sniff CANNOT recover the Phorj kind. *This is the
   design's central, non-obvious decision.*
5. **List vs Map (non-empty)** — `array_is_list($v)` predicate (same as the Json helper). Empty arrays
   collide → covered by the `'map'` tag (#4).
6. **Cycle-token determinism** — `Rc::as_ptr` (Rust) / `spl_object_id` (PHP) gate `<circular>` ONLY,
   never printed; path-scoped push/pop.
7. **Depth-elision boundary** — pin `d >= maxDepth && is_compound(v)` and the `…` (U+2026) token bytes
   identically across legs.
8. **Hex casing for bytes** — pin **lowercase** `\xHH` (matches `b"…"` literals + `php_escape_bytes`).
9. **Truncation char-count** — count Unicode scalar values (Rust `chars()` / PHP PCRE `/./us`), not
   bytes, for `Str`; count octets for `Bytes`. The `…` and ` (len N)` markers identical.
10. **Enum PHP rendering** — the one kind needing an implementation-time check against the actual enum
    lowering (§3.14). Highest-effort sub-part; still front-end-only, caught by the differential harness.

---

## 9. std Rust APIs relied on (citations)

- `String`, `format!`, `str::push_str`, `core::fmt` — rendering (alloc/core).
- `std::rc::Rc::as_ptr` — path-scoped cycle detection, identical to `eq_val_rec` (`src/value.rs:328`).
- `f64` Display (`format!("{x}")`) via `Value::as_display` (`src/value.rs:242`) — float single-source,
  mirrored by `__phorj_float` (`src/transpile/program.rs:312`).
- `fmt_decimal(unscaled, scale)` (`src/value.rs:847`) — decimal single-source.
- `ClassTables::from_program` (`src/native/mod.rs:118`) + the sorted `fields` `BTreeMap`
  (`src/native/mod.rs:110`) — deterministic field order, shared with `__phorj_reflect_of`
  (`src/transpile/program.rs:816`).
- `NativeEval::Reflective` dispatch (`src/native/mod.rs:93`) — already wired in both backends.
- `chars()` for code-point truncation.

No external crate, no `unsafe` (`#![forbid(unsafe_code)]` unaffected), no clock/RNG/IO. Pure Tier A.

---

## 10. Effort & open questions

- **Effort: medium.** `src/native/dump.rs` (two Reflective natives + the Rust renderer over the closed
  enum, single-sourced for both backends) + the gated `__phorj_dump*` PHP helper block in
  `emit_runtime_helpers` + the `uses_dump` flag + a `Core.Dump` arm in `emit_call` for the static-type
  tag + `examples/guide/dump.phg` (byte-identity-gated) + unit tests per `Value` kind incl. a cycle.
  No new Op, no new `NativeEval` variant, no checker/compiler surgery beyond generic-native registration.
- **Highest-effort sub-part:** the enum PHP rendering (§3.14) — needs an implementation-time check
  against the actual enum lowering shape; everything else reuses a shipped single source.
- **Open questions for the developer (cosmetic, medium-confidence — all caught by `differential.rs`):**
  (a) List integer-key form `i => v` vs bare `v` (§3.10 chose keys); (b) `Set { … }` wrapper spelling;
  (c) enum inline-scalar-payload vs always-multiline (§3.14 chose inline-for-scalars);
  (d) truncation defaults `MAX_STR = 100` / `MAX_BYTES = 64`; (e) `Core.Dump` vs `Core.Debug` module
  name (header pins `Core.Dump`). None affect feasibility — each is a one-line both-legs change.
