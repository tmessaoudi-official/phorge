# M6 W0 — `bytes` Type Design

> **Status:** IMPLEMENTED 2026-06-18 (design-locked §"Design-lock", then built). Lands `bytes` +
> `b"…"` + `core.bytes` (5 natives); `examples/guide/bytes.phg` byte-identical on run/runvm/PHP.
> Parent: `docs/specs/2026-06-18-m6-web-design.md` §8/§10 (W0 = the first slice, bytes pulled forward
> by developer choice). `bytes` is a standalone language feature; HTTP bodies are its first consumer.

## Why bytes (and why it's cheaper than it looks)

HTTP bodies/headers are octets; Phorge `string` is UTF-8 (`value.rs:17`, enforced — the lexer asserts
`from_utf8`). A `bytes` type makes binary bodies honest. **The transpile is trivial because PHP has no
separate bytes type — PHP strings ARE byte arrays** — so all the design work is Phorge-side: the
literal syntax and the `string`↔`bytes` interop. Verified: literals compile through
`emit_const(Value) -> Op::Const` (`compiler.rs:605`), so a byte literal needs **no new `Op`**.

## 1. Surface syntax — the `b"…"` byte literal

- New token form `b"…"`: the lexer, on seeing `b` immediately followed by `"`, scans a **byte-string
  literal** (a sibling of `scan_string`, `lexer.rs:137`).
- **No interpolation** — unlike `"…"` (which splits into `StrPart::Literal`/`Expr`), `b"…"` is a flat,
  raw byte sequence. `{` and `}` are literal bytes. (Interpolation yields a `string`; bytes are raw.)
- **Escapes:** `\n \t \r \\ \"` (same as strings) **plus `\xHH`** (two hex digits → one arbitrary
  byte). `\xHH` is what lets a literal hold non-UTF-8 octets — the whole point of the type. *(`\xHH` is
  byte-literal-only; allowing it in a UTF-8 `string` could break the `from_utf8` invariant.)*
- AST: a new `Expr::Bytes(Vec<u8>, Span)` (parallel to `Expr::Str`), or reuse a bytes-tagged variant.

## 2. Value + type kernel

- `Value::Bytes(Rc<Vec<u8>>)` — `Rc`-shared like `List`/`Instance`, consistent with the P5a heap.
- `Ty::Bytes` — appended to the `Ty` enum (`types.rs:6`, currently 11 user variants).
- Centralized `Value` methods extended for `Bytes` (the parity-critical single-source points):
  - `type_name` → `"bytes"`; `eq_val` → byte-vector equality; `as_display` → **None** (bytes are not a
    string — forces explicit conversion, mirrors how non-string values aren't auto-stringified).
  - `Display` (used only off the gated path — REPL/errors): a canonical `b"…"` form with `\xHH` for
    non-printable bytes. Shared via `value.rs`, so `run`≡`runvm` is automatic.
- **No implicit coercion** either direction — `bytes` and `string` convert only through explicit
  interop (below). The checker treats them as distinct, non-assignable types.

## 3. `string` ↔ `bytes` interop — `core.bytes` natives

Three natives in a new `core.bytes` module (the established `(module,name)` registry pattern —
`native.rs`), imported via `import core.bytes;`:

| Phorge call | Sig | Semantics | PHP erasure |
|---|---|---|---|
| `bytes.from_string(s)` | `string -> bytes` | UTF-8 encode (identity bytes) | `$s` (identity) |
| `bytes.to_string(b)` | `bytes -> string?` | UTF-8 decode; `null` if invalid | `(mb_check_encoding($b,'UTF-8') ? $b : null)` |
| `bytes.len(b)` | `bytes -> int` | **byte** count | `strlen($b)` |
| `bytes.concat(a, b)` | `bytes, bytes -> bytes` | byte-wise concatenation | `($a . $b)` |
| `bytes.slice(b, start, end)` | `bytes, int, int -> bytes` | half-open `[start, end)` byte slice; **bounds clamped** to `[0, len]` (`start>end` → empty) — total, no fault | `substr($b, max(0,$start), max(0,min($end,strlen($b))-max(0,$start)))` |

- `bytes.to_string` returning `string?` composes with S2 (`?? ""`, `if (var s = bytes.to_string(b))`) —
  invalid UTF-8 is a first-class `null`, never a fault. This is also how the W1 handler will turn a raw
  request body into a usable string.
- **`slice` clamps rather than faults** — deliberately total (unlike list `xs[i]` OOB, which faults via
  `Op::Index`), so it stays deterministic with no new `FaultKind`. The PHP `substr` clamp is matched
  byte-for-byte by the Rust kernel.
- **Note vs `core.text`:** `core.text.len` is `mb_strlen` (characters); `bytes.len` is `strlen`
  (bytes). The distinction is deliberate and documented.
- W0 surface = these **five** (developer choice: minimal conversions **+** `concat`/`slice`, which the
  W1 parser/serializer will consume directly). Byte-indexing is deferred until a concrete need.

## 4. No new `Op` (confirmed)

- Byte literal → `emit_const(Value::Bytes(..)) -> Op::Const(k)` — reuses the constant pool.
- Interop → `Op::CallNative(idx, argc)` — the generic stdlib path.
- `==` on bytes → existing `Op::Eq` via `eq_val`.
- `validate`/`stack_effect`/`exec_op` are untouched (no new variant → no three-match coupling).

## 5. Transpile (Phorge → PHP)

- `emit_type`: `Ty::Bytes -> "string"` (PHP strings are byte arrays) — `transpile.rs:243`.
- `b"…"` literal → a PHP **double-quoted** string with `\xHH` preserved (PHP supports `\xHH` in
  double-quoted strings); `\n\t\r` map directly.
- Interop natives erase per the table above (`strlen` / `mb_check_encoding` / identity).
- **Round-trip caveat:** a byte literal with non-UTF-8 `\xHH` can't be printed (println needs a
  `string`, and `to_string` returns `null`), so byte-identity examples either stay ASCII or deliberately
  exercise the invalid-UTF-8 → `null` path. The `run`≡`runvm` spine is always identical (shared kernel);
  the only sensitivity is the PHP round-trip, handled by keeping printed output ASCII.

## 6. TDD plan (tests first, then implement — each a green commit)

1. **Lexer tests** (`bin`-style unit in `lexer.rs`): `b"abc"` tokenizes; `\xHH` → the right byte;
   `b"a{b}c"` keeps `{}` literal (no interpolation); unterminated/invalid-hex error spans.
2. **Differential examples** (`examples/guide/bytes.phg`, auto byte-identity-gated): `from_string`
   round-trip, `len` (byte vs char distinction with a multi-byte string), `to_string` on valid +
   invalid UTF-8 (the `null` arm), `==` on bytes. Runs identically on `run`/`runvm` + **real PHP**.
3. **Transpile assertion**: `bytes` param → PHP `string` hint; `b"…"` → PHP literal; native erasure.
4. **Checker tests**: `bytes`↔`string` non-assignable; `bytes.to_string` typed `string?`.

## 7. Open micro-decisions (confirm before build)

- **D1 — `\xHH` escape in `b"…"`?** Recommend **yes** (without it, `bytes` can only hold UTF-8 content
  and is pointless). Byte-literal-only (not in `string`).
- **D2 — interop module name `core.bytes`?** Recommend **yes** (consistent with `core.text`/`core.file`).
  Caveat: the type name `bytes` and the import leaf `bytes` overlap — disambiguated by position (type vs
  call), and the existing `E-SHADOW-IMPORT` guard blocks a local named `bytes`. Alternative: built-in
  `string()`/`bytes()` cast functions (rejected — Phorge has no cast syntax; natives are the pattern).
- **D3 — W0 native surface.** RESOLVED: **all five** — `from_string`/`to_string`/`len` **+
  `concat`/`slice`** (developer choice; `slice` clamps, see §3). Byte-indexing deferred.

## Design-lock (2026-06-18)
D1 **yes** (`\xHH`, byte-literal-only) · D2 **`core.bytes`** · D3 **five natives** (`+concat/slice`,
`slice` clamps). Locked — proceed to build under TDD.
