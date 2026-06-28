# Introspection, String Ergonomics & Process I/O — Design

**Date:** 2026-06-24
**Status:** Brainstorming → spec for review (not yet planned/implemented)
**Driver:** Real ergonomic gaps surfaced while writing Phorj in the playground — get type/class of a
value, runtime reflection of the class hierarchy, literal braces in strings, terminal args/env, and
the PHP superglobal story.

## Goal & scope

Four independently-shippable slices, sequenced safest-first. Each ships green + byte-identical
(`run ≡ runvm ≡ real PHP 8.5`) **where deterministic**; the one non-deterministic slice (process I/O)
goes on a quarantine seam, outside the byte-identity differential.

1. **Introspection + reflection** (deterministic) — `typeName`, `className`, and name-level hierarchy
   reflection (`implements`/`parents`/`traits`).
2. **Literal braces in strings** — `\{`/`\}` escapes **and** raw strings `r"…"` (the real string gap;
   *not* PHP single-quote duality).
3. **Process I/O** — `Core.Env` (env vars) + `Core.Process` (argv) on a new quarantine seam; the
   **M-Batteries** kickoff.
4. **Superglobal map** — documentation + routing of every PHP superglobal to its Phorj home (mostly
   M6 `Request`, partly Slice 3). No new ambient-global mechanism — by design.

## Constraints & philosophy (the bar every decision is held to)

- **Byte-identity spine.** Deterministic features ship gated on `run ≡ runvm ≡ PHP`. Non-deterministic
  ones (env/args) are *quarantined* off the differential (the `src/serve.rs` precedent).
- **No ambient globals.** Phorj replaces PHP's untyped, mutable, ambient superglobals with typed
  values passed explicitly. This is the core TS-over-PHP value proposition; it is non-negotiable.
- **Legible over powerful.** Reflection is bounded to *read-only, name-level* introspection. Anything
  that defeats static typing (invoke-by-string, instantiate-by-string, attribute-driven magic) is
  rejected — it would reintroduce exactly the dynamism Phorj exists to remove.
- **No new `Op`, no `Value` change** for Slices 1 & 2 (front-end + native registry only).

---

## Slice 1 — Introspection & (bounded) reflection

All deterministic (a value's runtime tag and the *static* class hierarchy are both fixed), so
byte-identity-gated with guide examples. Implemented as **generic natives** (the S7b-1 generic-native
path) inspecting the runtime `Value`; **no new `Op`/`Value`**.

New module `Core.Reflect`:

| Native | Returns | Notes |
|---|---|---|
| `typeName<T>(T x) -> string` | `"int"`/`"float"`/`"bool"`/`"string"`/`"bytes"`; class name for an instance; enum name for a variant; `"List"`/`"Map"`/`"Set"`; `"null"`; `"function"` | the headline answer to "get type / get class" |
| `className<T>(T x) -> string?` | class **or** enum name for an object; `null` for a non-object | nominal name only |
| `implements<T>(T x) -> List<string>` | interface names the value's class implements (transitively) | from `ast::class_implements` (already computed) |
| `parents<T>(T x) -> List<string>` | ancestor class names (nearest-first) | static hierarchy |
| `traits<T>(T x) -> List<string>` | trait names used by the value's class | static |
| `methodNames<T>(T x) -> List<string>` | method names on the value's class (incl. inherited) | read-only, from `class_method_origins` |
| `fieldNames<T>(T x) -> List<string>` | declared field names on the value's class | read-only, static |

These reuse compile-time tables (`class_implements`, `class_method_origins`) that already exist as
checker/transpiler internals — Slice 1 exposes them read-only at the value level. Erases to PHP
`get_class` / `class_implements` / `class_parents` / `class_uses` (tier-1, `php -n`-safe).

**The hard line (challenge to "full reflection"):** Slice 1 is *read-only name-level* reflection only.
Explicitly **rejected** (they defeat the static guarantees that are Phorj's reason to exist):
- invoke-a-method-by-string-name / call-by-reflection,
- instantiate-a-class-by-string-name,
- attribute (`#[Attr]`) reflection and attribute-driven dispatch,
- mutating fields by reflected name.

Member *enumeration* (`methodNames`/`fieldNames -> List<string>`) **is included** (developer-approved
2026-06-24): still read-only + deterministic, drawn from the existing `class_method_origins` table and
the class field declarations. It does *not* enable invocation — names only.

**Use it for:** debugging, logging, serialization. **Not for control flow** — type dispatch stays
`instanceof` + `match` type-patterns (which the checker proves exhaustive).

---

## Slice 2 — Literal braces in strings

**Verified gap:** a literal `{` is currently inexpressible — `"{\"k\":1}"` parse-errors (read as
interpolation), `\{` is `invalid escape`, `{{` is `unterminated interpolation`. JSON, regex (`{n,m}`),
CSS, and code templates are all unwritable today. (PHP single-quote strings stay **rejected** — the
real need is literal braces, not quote-duality.)

Two complementary additions:

1. **`\{` and `\}` escapes** — add to the existing escape set (`\n \t \r \\ \"`) in `scan_string`.
   Minimal, fits the lexer, familiar. `"\{\"k\": 1\}"` → `{"k": 1}`. Interpolation still uses bare
   `{expr}`.
2. **Raw strings `r"…"`** — no escapes, no interpolation; everything literal until the close. For
   JSON/regex/template blocks. Embedded-quote story: Rust-style `r#"…"#` / `r##"…"##` (the lexer counts
   the `#` run to find the matching close), so any content is expressible.

Both are deterministic, front-end-only (lexer + the interpolation splitter), **no new `Op`**; the
transpiler emits ordinary PHP string literals (the brace is just a byte). Byte-identity-gated with a
guide example (a JSON/regex snippet).

---

## Slice 3 — Process I/O (`Core.Env`, `Core.Process`) — the M-Batteries kickoff

The env/args half of the superglobals. **Non-deterministic** (values depend on the environment), so it
**cannot** sit on the byte-identity differential — it requires a **quarantine seam**: natives marked
impure, excluded from `tests/differential.rs`, exercised by their own tests, and showcased by a
**README walkthrough** (not a byte-identity-gated example) — exactly how `serve`/`build` are handled.

| Native | Returns | PHP target |
|---|---|---|
| `Core.Process.args() -> List<string>` | program arguments (excluding the interpreter/script) | `$argv` (sliced) |
| `Core.Env.get(name: string) -> string?` | env var or `null` | `getenv()` |
| `Core.Env.all() -> Map<string, string>` | all env vars | `$_ENV` / `getenv()` |

Supporting work:
- **CLI convention:** `phg run foo.phg -- arg1 arg2` (everything after `--` is the program's argv).
- **Built binaries:** `phg build` output must forward host argv (`P-build-argv`, M2.5 Phase 3) — a
  dependency to note, not necessarily build here.
- **Quarantine seam:** formalize an "impure native" marker so `differential.rs` skips programs that
  touch it (extends the existing impure-feature pattern). This is the reusable mechanism the rest of
  M-Batteries (`Core.Dir`, `Core.Random`, `Core.Csv`, …) will build on.

This is the largest slice — effectively the start of the **M-Batteries** milestone.

---

## Slice 4 — Superglobal map (routing, mostly to M6)

The request-data superglobals belong to the **M6 web model**, where the PHP front-controller reads them
*once* at the edge to build a typed `Request`; Phorj code only ever sees the typed value. This slice
is **documentation + routing**, not new mechanism here — the accessors are M6 waves.

| PHP | Phorj | Home |
|---|---|---|
| `$_GET` | `req.query(name) -> string?` (+ `Core.Url` parse) | M6 (W2+) |
| `$_POST` | `req.form(name)` / parsed body | M6 |
| `$_FILES` | `req.files()` typed uploads | M6 (later) |
| `$_COOKIE` | `req.cookie(name)` | M6 |
| `$_SERVER` (request) | `Request` fields | M6 (partial: `req.header`) |
| `$_SERVER`/`$_ENV`/`getenv()` | `Core.Env` | **Slice 3** |
| `argv`/`argc` | `Core.Process.args()` | **Slice 3** |
| `$_SESSION` | explicit session value | M6+ (deferred — mutable state) |
| `$_REQUEST` | **rejected** | ambiguous GET+POST+COOKIE merge |
| ambient superglobal access | **never exposed** | by design |

## Sequencing & milestone mapping

1. **Slice 1** (introspection) — quick, byte-safe, no new `Op`. Answers the get-type/get-class
   questions immediately.
2. **Slice 2** (literal braces) — quick, byte-safe, closes a real hole.
3. **Slice 3** (process I/O) — larger; kicks off **M-Batteries** with the quarantine seam.
4. **Slice 4** — folded into **M6** waves (referenced here, executed there).

Each slice ships independently with its tests + (where deterministic) a guide example.

## Deferred / rejected (with rationale)

- **Rejected:** PHP single-quote strings (footgun, no need), `$_REQUEST` (ambiguous), ambient
  superglobals (the thing Phorj exists to remove), reflection-driven dynamic dispatch /
  instantiate-by-string / attribute magic (defeats static typing).
- **Deferred:** `#[Attr]` attributes + attribute reflection (`A-attributes`, post-M-RT); `$_SESSION`
  (stateful, M6+); full `ReflectionClass`-style API (`Q-reflection`, v2) — Slice 1's name-level
  introspection covers the legible 80%.

## Decisions Log
- [2026-06-24] AGREED: brainstorm the whole cluster and produce one solid plan first (no ad-hoc natives).
- [2026-06-24] AGREED: include **full (name-level) reflection now**, not deferred — bounded to read-only
  introspection; dynamic-dispatch/attribute reflection stays rejected.
- [2026-06-24] AGREED: literal braces via **both** `\{`/`\}` escapes **and** raw strings `r"…"`.
- [2026-06-24] AGREED: Slice 1 introspection depth = **typeName + className + hierarchy**
  (implements/parents/traits) **+ member enumeration** (methodNames/fieldNames, read-only, names only).
- [2026-06-24] AGREED: PHP superglobals are **on the roadmap, reshaped** — env/args → M-Batteries
  (Slice 3); request data → M6 typed `Request`; `$_REQUEST`/ambient access rejected.
- [2026-06-24] CHALLENGE upheld: no ambient superglobals; reflection is read-only name-level only;
  single-quote strings rejected.
