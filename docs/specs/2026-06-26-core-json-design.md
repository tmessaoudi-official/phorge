# Core.Json — JSON parse / stringify (design)

> Status: **design-locked** (2026-06-26). Backlog item 1
> (`docs/plans/2026-06-26-autonomous-backlog.plan.md`). Spec-first because it introduces a
> compiler-injected stdlib type and a new transpiler capability (reserved-variant mangling).

## Goal

A std-only, deterministic JSON value type + parse/stringify natives, byte-identical across
`run ≡ runvm ≡ real PHP 8.5`. JSON is the lingua franca of the web tier (pairs with M6
`phg serve`/`Request`/`Response`) and is finally expressible without new type-system work — generic
enums + `Map` + `List` all shipped.

## Public surface

```phorj
import Core.Json;

// The value model — a compiler-injected enum (see "Injection" below). Recursive.
enum Json {
    Null(),
    Bool(bool value),
    Int(int value),
    Float(float value),
    Str(string value),
    Arr(List<Json> items),
    Obj(Map<string, Json> entries),
}

Core.Json.parse(string) -> Json?          // None (Value::Null) on malformed input
Core.Json.stringify(Json) -> string        // compact, matches json_encode default
Core.Json.stringifyPretty(Json) -> string  // 4-space indent, matches JSON_PRETTY_PRINT
```

A user constructs (`Json.Int(42)`, `Json.Obj([...])`) and `match`es on the variants like any enum.

### Locked decisions (developer, 2026-06-26 — see plan Decisions Log)

1. **Number model = `Int(int) + Float(float)`** (PHP-faithful). PHP `json_decode("42")` → `int`,
   `"42.0"`/`"1e3"` → `float`; Phorj mirrors that, and it matches Phorj's own int/float split.
   Round-trip is byte-identical to PHP either way (`json_encode(42.0)` → `"42"`).
2. **Ship both `stringify` (compact) and `stringifyPretty` (4-space)** in the first slice.
3. **PHP-reserved enum-variant names are mangled in the transpiler** (append `_`). PHP reserves
   `int`/`float`/`bool`/`null` (and `string`/`true`/`false`/`void`/`iterable`/`object`/`mixed`/
   `never`/`self`/`parent`/`static`) as class names — *even inside a namespace* (verified vs 8.5).
   Enum variants transpile to `final class <Variant> extends <Enum>`, so `Json.Int`/`Bool`/`Null`/
   `Float` would be PHP parse errors. Mangling keeps the clean API and is reusable for any enum.

## Slice A — reserved enum-variant mangling (transpiler-only, prerequisite)

A variant's PHP **class name** is its only PHP-reserved-collision surface. `run`/`runvm` use the
Phorj variant string (`EnumVal.variant`) and never a PHP class name, so this is **transpiler-only**
and stdout byte-identity is untouched by construction.

- New `transpile` helper `php_variant_name(variant: &str) -> String`: lowercase-compare against the
  PHP class-reserved set; on a hit, append `_` (`Int`→`Int_`, `Bool`→`Bool_`, `Null`→`Null_`,
  `Float`→`Float_`). Otherwise identity (`Str`/`Arr`/`Obj` unchanged).
- Route the **two** variant-class-name emission sites through it:
  - `program.rs::emit_enum` — the `final class <V> extends <Base>` declaration (currently `v.name`).
  - `expr.rs::variant_ref` — the construction (`new <V>(…)`) and `instanceof <V>` reference (both
    already funnel here; the namespaced branch mangles the trailing segment).
- Reserved set (verified empirical, PHP 8.5, case-insensitive): `int float bool string true false
  null void iterable object mixed never self parent static`. (`array`/`callable`/`list`/`enum` are
  *not* reserved as class names — do not mangle them.)
- **Edge (KNOWN_ISSUES):** an enum declaring both `Int` and `Int_` would collide after mangling —
  adversarial, deferred. (No first-party or test enum does this.)
- Example: `examples/guide/enum-reserved-variants.phg` — an enum using `Int`/`Str`/`Null` variants,
  matched + printed, byte-identical on run/runvm/real PHP.

## Slice B — the `Json` type + natives

### Injection (where `Json` comes from)

The `Json` enum is **compiler-injected** at the top of `cli::check_and_expand` (the single chokepoint
covering run/runvm/transpile **and** the project loader — `check_resolutions` runs there first, so the
inject must precede it) **iff the program imports `Core.Json`**:

```
fn inject_json_prelude(prog: &Program) -> Program  // prepend the canonical Json EnumDecl if imported
```

The decl is produced by parsing a canonical Phorj snippet once (cached in a `OnceLock`, cloned per
call) — DRY: the snippet *is* the type. Injecting only-on-import keeps the namespace clean (a program
not using JSON has no `Json` enum / no PHP output) and matches Phorj's explicit-import philosophy.
The enum then flows through checker (registers it), interpreter/VM (construct + match), and transpiler
(emits the PHP class hierarchy) as an ordinary enum — **zero new backend machinery**.

Scope: `package Main` programs (single-package). Multi-package projects importing `Core.Json` are a
follow-up (the injected enum has no `package` → emitted flat; the helpers ref bare classes).

### Natives (`src/native/json.rs`)

All `NativeEval::Pure`. The one `eval` body is shared by both Rust backends (the parity guarantee);
the `php` closure delegates to gated helpers.

| native | params | ret | php emission | helper flag |
|---|---|---|---|---|
| `parse` | `[String]` | `Json?` (`Optional(Named "Json")`) | `__phorj_json_decode({0})` | `uses_json_decode` |
| `stringify` | `[Named "Json"]` | `String` | `__phorj_json_encode({0})` | `uses_json_encode` |
| `stringifyPretty` | `[Named "Json"]` | `String` | `__phorj_json_encode_pretty({0})` | `uses_json_pretty` |

Flags set in `transpile/call.rs` for `nat.module == "Core.Json"` (the established Reflect gated-helper
pattern — a `php` closure has no `&mut self`).

### Encoding spec (Rust `eval` and PHP helper must agree byte-for-byte)

- **null** → `null`; **bool** → `true`/`false`; **int** → decimal.
- **float** → **shortest-round-trip positional** (Rust `format!("{}")` ; PHP `__phorj_float`). This is
  Phorj's float convention everywhere — *not* PHP json's scientific notation for extreme magnitudes
  (`1e20`). Documented divergence from native `json_encode` for |exp| extremes (KNOWN_ISSUES); the
  common range is identical (`0.1+0.2`→`0.30000000000000004`, verified).
- **string** → JSON-escaped to match PHP `json_encode` **default**: `"`→`\"`, `\`→`\\`, `/`→`\/`,
  `\b\f\n\r\t`, other control (`<0x20`) → `\u00XX`, non-ASCII (`>0x7F`) → `\uXXXX` (UTF-16 surrogate
  pairs for code points `>0xFFFF`). (Verified: `json_encode("café")`→`"café"`,
  `"a/b"`→`"a\/b"`.) PHP helper uses native `json_encode($string)` for a scalar string (authoritative
  escaping); Rust hand-rolls to match.
- **array** → `[` items `,`-joined `]`; **object** → `{` `"k":v` pairs `,`-joined `}` in Map
  insertion order. Compact = no spaces. Pretty = `JSON_PRETTY_PRINT` layout: 4-space indent per
  level, `": "` after each key, newline after each `{`/`[`/element/pair, closing brace at parent
  indent; an empty `[]`/`{}` stays on one line (matches PHP).

### Decoding spec (`parse` — recursive descent, Rust; PHP delegates to `json_decode`)

- PHP: `__phorj_json_decode($s)` = `$d = json_decode($s)` (objects → `stdClass`, so `{}` ≠ `[]`);
  if `json_last_error() !== JSON_ERROR_NONE` → return `null` (Phorj `None`); else recurse over `$d`:
  `is_null`→`Null_`, `is_bool`→`Bool_`, `is_int`→`Int_`, `is_float`→`Float_`, `is_string`→`Str`,
  `is_array`→`Arr` (list), `is_object`→`Obj` (`get_object_vars`).
- Rust: a std-only recursive-descent parser → `Value::Enum(Json…)`, or `Value::Null` on any syntax
  error (`None`). Number lexing: digits with no `.`/`e`/`E` → `Int`; otherwise `Float`. An integer
  literal that overflows `i64` falls back to `Float` (matches PHP's overflow-to-float). `{}`→empty
  `Obj`, `[]`→empty `Arr`. **Duplicate object keys**: last value wins, first position kept (PHP assoc
  semantics) — `{"a":1,"b":2,"a":3}` → `Obj{a:3, b:2}`. Whitespace ` \t\n\r` skipped between tokens.
  Trailing non-whitespace after the top-level value → error (`None`), matching `json_decode`.
- Returns `Value::Null` for `None`, the `Json` enum value for `Some` (optionals: present = the value
  itself, absent = `Value::Null`).

### Byte-identity risks (and resolutions)

| risk | resolution |
|---|---|
| float format (`1e20` scientific in PHP json) | use `__phorj_float`/Rust positional everywhere; documented divergence from native json, but run≡runvm≡PHP-helper identical |
| string escaping default (`\/`, `\uXXXX`) | PHP helper uses native `json_encode` per-scalar-string; Rust matches default; verified samples |
| int vs float on decode | number lexer: `.`/`e` ⇒ Float else Int; i64 overflow ⇒ Float (matches `json_decode`) |
| `{}` vs `[]` | decode via `json_decode($s)` (stdClass for objects), not assoc mode |
| dup keys | last-value/first-position (PHP assoc) replicated in Rust decoder |
| oracle runs `php -n` | `json_*` + `__phorj_float` are tier-1/core (no ini ext); no `mb_*` |

### Example

`examples/guide/json.phg` — construct a `Json.Obj`, `stringify` + `stringifyPretty` it, `parse` a
literal back, `match` to read a field. Byte-identity-gated by the `examples/**/*.phg` glob.
`examples/README.md` entry added.

## Test plan (TDD)

- **Slice A:** transpiler unit tests (reserved variant → `class Int_` / `new Int_` / `instanceof
  Int_`); the new example runs the oracle.
- **Slice B:** `native/json_tests.rs` — encode each scalar/structure (compact + pretty), round-trip
  parse→stringify, malformed→None, int-vs-float decode, dup keys, `{}`/`[]`, escaping samples; the
  example drives the 3-way oracle. Gate: `PHORJ_PHP=…/php-8.5.7 PHORJ_REQUIRE_PHP=1 cargo test
  --workspace` + clippy + fmt.
```
