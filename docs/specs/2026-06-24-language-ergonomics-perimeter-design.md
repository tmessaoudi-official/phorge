# Language Ergonomics & Legibility Perimeter — Design

**Date:** 2026-06-24
**Status:** Brainstorming complete → spec for review (decisions locked item-by-item with the developer).
**Origin:** A systematic empirical probe of the language surface (not just ad-hoc reports) for the
class of "ergonomic surprises a PHP/TS dev would hit." Every example below is verified against `phg`.

This is a sibling to the **introspection/strings/process** spec
(`docs/specs/2026-06-24-introspection-strings-process-design.md`, `c905198`) and the
**mandatory-return-type** milestone (separate; the writable `unit` here is its prerequisite). Where the
two overlap (string literals), this spec is authoritative.

All items are **front-end / stdlib only** unless noted — no new `Op`, no `Value` change, byte-identical
`run ≡ runvm ≡ real PHP 8.5` — except where a runtime kernel is explicitly called out.

## ✅ IMPLEMENT (this perimeter)

| # | Feature | Example | Notes / decision |
|---|---|---|---|
| 1 | **String concat `+`** | `"a" + "b"` → `"ab"` | type-directed; `string + int` stays a **type error** (no coercion — the typed system kills JS's `"1"+1` footgun) |
| 2 | **UFCS method sugar** | `xs.length()` ≡ `List.length(xs)`; `xs.filter(p).map(g)` | `x.f(a)` desugars to `f(x, a)`; reuses free functions, enables chaining; no separate method system |
| 3 | **Ternary `? :`** | `int x = c ? 1 : 2;` | parser disambiguates optional-type `x?` (type pos) from ternary (expr pos) |
| 4 | **Or-patterns in `match`** | `match n { 1 \| 2 \| 3 => "low", _ => … }` | grouped cases without `switch`; still exhaustive, no fall-through |
| 5 | **Power `**` + `Math.ipow`** | `2 ** 3`; `Math.ipow(2, 3)` | type-directed `**` (int→int, float→float); `Math.ipow(int,int)->int` native (int-power as a value); keep `Math.pow` (float) |
| 6 | **Unicode escapes `\u{…}`** | `"\u{1F600}"` | lex-time codepoint→UTF-8 bytes; independent of the hard i18n work |
| 7 | **Literal braces / raw strings** | `"\{\"k\": 1\}"`; `r"…"`, `r#"…"#` | `\{`/`\}` escapes **and** raw strings (Rust `#`-run delimiter for embedded quotes); fixes JSON/regex |
| 8 | **Paren'd return-position fn types** | `() -> ((int) -> bool)` | parser fix: a parenthesized function type must parse as a return type (today only the parens-free right-assoc form works) |
| 9 | **Writable `unit` type** | `function main() -> unit { … }` | make `unit` a writable builtin; prerequisite for the mandatory-return-type milestone |
| 10 | **let-destructuring (objects + lists)** | `var Point { x, y } = p;` · `var [a, b] = xs else { return; }` | object form is irrefutable; **refutable** list form requires an `else { … }` bail-out (Swift-style) |
| 11 | **Fixed-length lists `[T; N]`** | `[int; 2] pair = [1,2]; var [a,b] = pair;` | alongside `List<T>` (open); compile-time length + static bounds; **length-immutable** (no push/pop); erases to a PHP array (length is a compile-time guarantee); makes fixed-list destructuring irrefutable |
| 12 | **`this`-capture in closures** | `function handler() -> () -> unit { return fn() => log(this.label); }` | remove `E-LAMBDA-THIS`; capture `this` by reference (Rc handle ⇒ **live**); PHP arrow-fn auto-captures `$this`. Same cycle-leak stance mutation already takes (cycles already buildable today) |
| 13 | **`Text.charAt` / `Text.substring` natives** | `Text.charAt(s, 0)`, `Text.substring(s, 1, 3)` | the **safe** alternative to `s[0]` (explicit semantics) pending the codepoint operator; M4 stdlib |

## 🔵 DEFER (with the agreed reason)

| Feature | Why / where | Workaround today |
|---|---|---|
| **String index `s[0]`** | UTF-8 byte-vs-codepoint is an i18n decision → **M-text** (codepoint-correct) | `Text.charAt`/`substring` (#13) |
| **Tuples `(int,string)`** | positional `.0`/`.1` fights legibility; revisit as **named records** | a small class |
| **Generic fn as a value** `var f = id` | needs rank-N polymorphic fn types (Rust forbids too) | lambda-wrap `fn(int x) => id(x)` |
| **`decimal` / `BigInt`** | numeric-correctness milestone → **M-NUM / M-NUM-2** | (names reserved) |

## 🔴 REJECT (confirmed)

| Feature | Why |
|---|---|
| **Single-quote strings `'x'`** | re-imports PHP's quote-duality footgun; `r"…"` (#7) covers the no-interpolation need |
| **Spaceship `<=>`** | magic-int −1/0/1 is un-legible; use a typed `Ordering` enum at sort time (M4) |
| **PHP `.` concat** | `.` is member access (and UFCS); concat is `+` (#1) |
| **`switch`** | `match` is a safer superset (no fall-through); or-patterns (#4) give grouped cases |

## Cross-references / separately tracked

- **Introspection cluster** (`c905198`): `Core.Reflect.typeName`/`className`/`implements`/`parents`/
  `traits`/`methodNames`/`fieldNames`; process I/O (`Core.Args`/`Core.Env`, quarantine seam);
  superglobal map. *Open wrinkle:* hierarchy-reflection natives need the interpreter's class tables,
  which the pure-native signature can't reach — a mechanism decision for that plan.
- **Mandatory return types** (own milestone): every function/method/lambda must declare a return type;
  no exemptions where syntactically applicable (ctors have no return slot). **Open:** the no-value
  spelling (`unit` vs `void`) is *unresolved* — #9 (writable `unit`) is staged regardless as the
  enabling primitive. Breaking codemod across all `.phg`/tests.
- **Side-bug to investigate:** a chained force-unwrap field read (`a.next!.next!.v`) errors
  "no field `v` on Node" — likely a real `opt!`-then-field-access bug on object optionals.
- **Playground** `f66592d` (php-wasm fresh-instance fix): pending the developer's push + live re-verify.

## Proposed build sequencing (slices, each independently green + byte-identical)

1. **String slice** — `+` concat (#1), `\u{…}` (#6), literal braces + raw strings (#7).
2. **Operators/patterns slice** — ternary (#3), or-patterns (#4), `**`+`Math.ipow` (#5).
3. **Types slice** — writable `unit` (#9), paren'd return-position fn types (#8), fixed-length lists (#11).
4. **Closures slice** — `this`-capture (#12).
5. **Destructuring slice** — object + list let-destructuring with `else` (#10) [after #11, so fixed-list destructuring is irrefutable].
6. **UFCS slice** — method-call sugar (#2) [touches call resolution; do after the simpler wins].
7. **stdlib** — `Text.charAt`/`substring` (#13) [folds into M4].

(The mandatory-return-type codemod should ideally run **before** large new-code slices so new code is born annotated; gated on the `unit`/`void` call.)

## Decisions Log (2026-06-24, item-by-item with the developer)
- Contested: string `+` ✅ implement · UFCS `xs.length()` ✅ · `s[0]` → defer M-text + `Text.charAt`/`substring` natives · ternary ✅ add · `switch` ❌ reject, **add or-patterns** instead · power → **both** `**` operator + `Math.ipow`.
- Defer: `\u{…}` → **pull forward** · tuples → **defer** (classes now) · let-destructuring → **full** (objects + lists, `else` bail-out) · **+ fixed-length lists `[T; N]`** added (Option A: alongside `List<T>`) · `this`-capture → **build** (live) · generic-fn-value → **defer** · decimal/BigInt → **defer** (M-NUM).
- Reject confirmed: single-quotes · spaceship `<=>` · PHP `.` concat.
