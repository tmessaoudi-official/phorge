# Feasibility Spike — Core.Dump (deterministic var-dumper)

**Module:** `Core.Dump` (aka `Core.Debug` in the prior-art digest — same thing; recommend the
`Core.Debug` name for cross-language familiarity, see §8).
**Verdict:** **ADOPT-NOW.** Tier A (pure/deterministic), std-only, **no new VM Op**, fits the
existing `NativeEval::Reflective` path with zero new dispatch plumbing. Feasibility **88%**.
Confidence **high** on the mechanism; **medium** only on the exact rendered-format choices (a
design call, not a feasibility risk).

---

## 1. What the module is

A value inspector that turns any `Value` into a deterministic, human-readable string:

- `Debug.dump(value) -> string` — multi-line, indented, type-annotated tree.
- `Debug.inspect(value, int depth) -> string` — same renderer with a depth cap (beyond the cap
  emit a `…` elision marker).

It must render **every** `Value` kind, detect **circular references** (Instances became
shared-mutable handles in M-mut.6 — `a.next = b; b.next = a` is now constructible), and emit
**no** addresses / object-ids / pointers (the #1 var-dump determinism trap; Phorj's closed `Value`
enum makes this avoidable *by construction* — there is no address to leak unless we choose to print
`Rc::as_ptr`, which we never do).

"colors" in the brief: emit plain text only in the byte-identity spine. ANSI color is a Tier-B
concern (a TTY check is environment-dependent → non-deterministic) — **defer color** or make it an
explicit `colorize(s)` post-pass that is *never* in a gated example. Noted in §7.

---

## 2. Determinism partition — **Tier A (pure)**

Every input is an already-materialized `Value`; the output is a pure function of that value plus the
program's `ClassTables` (which all three backends already build identically). No clock, no RNG, no
filesystem, no locale, no iteration over an unordered container *if* we fix field order (see §4).
Therefore it is byte-identity-gateable and ships exactly like `Core.Math`/`Core.Text` —
`pure: true`, included in the differential spine.

This matches the existing pure-native precedent precisely; nothing here resembles the `pure:false`
Process/Env quarantine.

---

## 3. std-only feasibility — **YES, trivially**

The crate has **no `[dependencies]` section at all** (verified in `Cargo.toml` — only the wasm
playground member pulls `wasm-bindgen`, and it is a separate workspace member). The dumper is pure
string assembly over the closed `Value` enum: `String::push_str`, `format!`, integer/float
formatting — all `core`/`alloc`/`std`. No crate needed. `#![forbid(unsafe_code)]` is unaffected.

---

## 4. Byte-identity strategy (the load-bearing section)

Three legs must produce the same bytes: interpreter `dump`, VM `dump`, and the emitted PHP
`__phorj_dump`. The strategy is **one Phorj-owned format**, single-sourced in Rust for the two Rust
backends and mirrored exactly by a gated PHP helper. The risks and their mitigations:

| Trap | Why it bites | Mitigation (verified mechanism) |
|---|---|---|
| **Instance field order** | `Instance.fields` is `RefCell<HashMap<String,Value>>` (`src/value.rs:94`) — iterating it directly is non-deterministic even between two runs of the *same* backend. | **Do NOT iterate `fields` directly.** Take the field-name list from `ClassTables.fields` (`src/native/mod.rs:110`, a `BTreeMap<String,Vec<String>>` → **alphabetically sorted, deterministic, transitive-with-inheritance**), then look each name up in the instance's map. This is exactly what `Reflect.fields` already does and what the transpiler's `__phorj_reflect_of` static table emits — so the order is *already* byte-identical across all three legs. (Declaration order would be prettier, but sorted is sufficient for byte-identity and reuses an existing single source; if declaration order is later wanted, it is a `ClassTables` change shared by all three legs — not a dumper change.) |
| **Float formatting divergence** (KNOWN_ISSUE) | Rust `format!("{x}")` = shortest-round-trip; PHP `echo`/`(string)` = 14 sig-digits. A raw float in a dump would diverge. | Route every `Value::Float` through the **same Ryū path the transpiler already owns**: Rust side uses `as_display`/`format!("{x}")` (Rust's shortest-round-trip), PHP side emits `__phorj_float($v)` (the existing positional-Ryū helper at `src/transpile/program.rs:312`, gated by `uses_float`). The dump helper sets `uses_float = true`. Examples must still use exactly-representable floats per the standing KNOWN_ISSUE. |
| **Map / Set iteration order** | A `HashMap`/`HashSet` walk would differ per-leg. | `Value::Map`/`Set` are already **insertion-ordered `Rc<Vec<…>>`** (`src/value.rs`) — iterate the Vec directly; never re-bucket through a hashmap. PHP arrays are insertion-ordered too, so a `foreach` over the emitted PHP array matches. (This is the same R1 invariant Json/Map/Set natives already rely on.) |
| **Circular references** | M-mut.6 made `Instance` a shared-mutable handle; a self-referential graph would infinite-recurse and overflow the native stack at *different* depths per backend. | **Reuse the `eq_val_rec` visited-set pattern** (`src/value.rs:329`): carry a `Vec<*const Instance>` of instances currently on the path; on re-encounter emit a fixed `<cycle>` (or `*RECURSION*`) token and stop. PHP side: track visited via `spl_object_id` **into a local set only for cycle detection** — the id is never *printed* (it only gates the `*RECURSION*` token), so determinism is preserved. Co-inductive, terminates deterministically — identical to how `==` already handles cycles. |
| **Decimal rendering** | i128 fixed-point. | Reuse `fmt_decimal(unscaled, scale)` (`src/value.rs:847`) on the Rust side; PHP side the BCMath string already `(string)`s to the same form (the M-NUM S1 invariant). |
| **Closure rendering** | No stable identity to print. | Emit a fixed `<closure>` token (no address). PHP: any callable → `<closure>`. |
| **Bytes rendering** | `Value::Bytes(Rc<Vec<u8>>)`. | Emit `b"\xHH…"` lowercase-hex (matches the existing `b"…"` literal form and `Core.Bytes`); PHP side a `bin2hex`-based helper. Pin **lowercase** hex. |
| **mbstring absence under `php -n`** | Multibyte string fns unavailable. | The dumper only needs `strlen`/`str_repeat`/`bin2hex`/string concat — all PHP core, survive `php -n`. No `mb_*`. |

**Net:** every divergence axis has an *already-shipped* single-source mechanism to reuse. This is the
strongest signal for ADOPT-NOW — almost nothing is new infrastructure.

---

## 5. Exact PHP transpile target

**NOT** PHP `var_dump`/`print_r`/`var_export` — those emit object-ids, `#N` refcounts, addresses, and
PHP-flavored formatting that no Rust leg can match. Instead emit a **gated `__phorj_dump($v, $depth)`
helper** in `emit_runtime_helpers` (`src/transpile/program.rs:263`), gated by a new `uses_dump` bool,
following the *exact* precedent of `__phorj_reflect_of` (which already emits a static class table +
a helper reading it):

```php
function __phorj_dump_inner($v, $depth, &$seen, $indent) {
    if (is_int($v))    { return (string)$v; }
    if (is_float($v))  { return __phorj_float($v); }        // reuse existing Ryū helper
    if (is_bool($v))   { return $v ? "true" : "false"; }
    if ($v === null)   { return "null"; }
    if (is_string($v)) { return '"' . <escape> . '"'; }      // pin one escape scheme both legs
    if (is_callable($v)) { return "<closure>"; }
    if (is_object($v)) {
        $id = spl_object_id($v);                              // used ONLY for cycle detection
        if (isset($seen[$id])) { return "*RECURSION*"; }      // never printed as a number
        $seen[$id] = true;
        $cls = get_class($v);
        // field NAMES come from the SAME static class table __phorj_reflect_of uses → sorted, identical
        ...render "ClassName { field: value, ... }" with $indent ...
        unset($seen[$id]);                                    // path-scoped, mirrors Rust Vec pop
        return ...;
    }
    if (is_array($v)) { ... List vs Map disambiguated like __phorj_json_build ... }
}
function __phorj_dump($v) { $seen = []; return __phorj_dump_inner($v, PHP_INT_MAX, $seen, 0); }
```

The list-vs-map array disambiguation already has a precedent in the Json helper
(`array_is_list($arr)` / key-shape inspection). Reuse that exact predicate so Map/List render
distinctly and identically.

The string-escape scheme must be **pinned identically** on both legs (e.g. escape `"`, `\`, `\n`,
`\t`, `\r`; leave other bytes literal — do NOT use PHP `addslashes` which differs from a hand-rolled
Rust escaper). This is the one place to be meticulous; it is straightforward but must match byte-for-byte.

---

## 6. New VM Op? — **NO**

A dumper is an ordinary native call → `Op::CallNative(idx, argc)`, the existing path. It needs the
`ClassTables`, so it is `NativeEval::Reflective(fn(&[Value], &ClassTables) -> Result<Value,String>)`
— a variant that **already exists** and is **already dispatched in both backends**:

- interpreter: `src/interpreter/call.rs:53` (`Reflective(f) => f(&argv, &self.class_tables)`)
- VM: dispatched via the same `class_tables` the chunk carries (`src/chunk.rs:368`,
  `src/vm/exec.rs:260` comment confirms reflection natives read it).

So: **no new Op, no new `NativeEval` variant, no new dispatch code.** Just a new `NativeFn` entry in a
new `src/native/dump.rs` (one file per leaf module, the established convention) plus the gated PHP
helper. This is the cheapest possible integration shape.

---

## 7. Phorj API sketch

```phorj
import Core.Debug;

// scalars + compounds
Console.println(Debug.dump(42));            // 42
Console.println(Debug.dump(3.5));           // 3.5
Console.println(Debug.dump("hi"));          // "hi"
Console.println(Debug.dump([1, 2, 3]));     // [1, 2, 3]   (or multi-line for nesting)
Console.println(Debug.dump(["a" => 1]));    // {"a": 1}

class Point { public int x; public int y; }
Console.println(Debug.dump(Point(1, 2)));   // Point { x: 1, y: 2 }   (fields sorted: x, y)

// depth-bounded
Console.println(Debug.inspect(deepThing, 2)); // elides below depth 2 with …
```

Signatures for the registry:
- `Debug.dump(T value) -> string` — generic param `T` (reuse the S7a `Ty::Param` native path that
  `Core.List.map` etc. already use; erased pre-backend, so no type var reaches a backend).
- `Debug.inspect(T value, int depth) -> string`.

**Color (deferred):** a TTY check is environment-dependent → not Tier A. If wanted later, ship as a
separate `Debug.colorize(string) -> string` that wraps ANSI codes, and **never** put it in a gated
example (or gate it behind an explicit flag the example doesn't set).

---

## 8. Naming

Prior-art digest uses **`Core.Debug`** for PHP/Python/Go; the stage header says `Core.Dump`. Recommend
**`Core.Debug`** (cross-language familiarity, room for `inspect`/future `trace`); if the developer
prefers `Core.Dump`, it is a one-line registry change. Flag this as a developer decision, not a
feasibility blocker.

---

## 9. Determinism risks (named)

1. **Instance field order** — mitigated by reading `ClassTables.fields` (sorted) not the `HashMap`.
   *If a future change iterates `Instance.fields` directly, byte-identity silently breaks.*
2. **Float rendering** — must route through `__phorj_float`/`as_display`; a naked `(string)$float`
   on the PHP leg diverges. Examples restricted to exactly-representable floats (standing KNOWN_ISSUE).
3. **String escape scheme** — must be pinned byte-identically across Rust escaper and PHP helper;
   do not reuse `addslashes`.
4. **Cycle-token determinism** — the `spl_object_id` on the PHP leg must gate the `*RECURSION*` token
   ONLY, never be printed; the Rust leg uses `Rc::as_ptr` in a path-scoped `Vec` the same way.
5. **Map/List disambiguation** — reuse the Json helper's `array_is_list` predicate so the two render
   distinctly and identically; a wrong predicate makes an empty map and empty list collide.
6. **Hex casing for bytes** — pin lowercase (matches `b"…"` literals and `Core.Bytes`).
7. **Depth-elision marker** — the elision token and where exactly it triggers (at-depth vs
   below-depth) must be identical on all three legs.

---

## 10. Effort & recommendation

- **Effort: medium.** New `src/native/dump.rs` (one Reflective native fn pair) + a gated
  `__phorj_dump` PHP helper modeled on `__phorj_reflect_of` + a `uses_dump` flag wired into
  `emit_runtime_helpers` + a guide example `examples/guide/debug.phg` (byte-identity-gated) + unit
  tests for each `Value` kind incl. a cycle. No new Op, no new dispatch, no checker/compiler surgery
  beyond the standard generic-native registration. Comparable in size to the Reflect Tier-2 slice.
- **Recommendation: ADOPT-NOW.** It is high-utility (debugging is a daily need), almost entirely
  reuses shipped mechanisms, has no determinism axis without an existing single-source mitigation, and
  needs no new Op. The only genuinely new code is the format renderer itself + its PHP twin, and the
  format is Phorj-owned (we define what byte-identical means).
- **Feasibility: 88%.** The 12% is the meticulous-but-routine work of pinning the string-escape scheme
  and the exact whitespace/indent format identically across the Rust renderer and the PHP helper —
  caught immediately by the differential harness, so low *risk*, just careful work.

---

## 11. std Rust APIs relied on

- `String`, `format!`, `str::push_str`, `core::fmt` — rendering.
- `std::rc::Rc::as_ptr` — path-scoped cycle detection (same as `eq_val_rec`, `src/value.rs:329`).
- `f64` Display (`format!("{x}")`) — shortest-round-trip float, mirrored by `__phorj_float`.
- Existing in-repo single sources: `Value::as_display` (`value.rs:239`), `fmt_decimal`
  (`value.rs:847`), `ClassTables::from_program` (`native/mod.rs:118`), `NativeEval::Reflective`
  dispatch (`interpreter/call.rs:53`), `__phorj_float`/`__phorj_reflect_of` emission
  (`transpile/program.rs:312`/`812`).

No external crate, no `unsafe`, no clock/RNG/IO. Pure Tier A.
