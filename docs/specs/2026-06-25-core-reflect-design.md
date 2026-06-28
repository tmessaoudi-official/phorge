# Core.Reflect — Design (resolving the reflection-vs-erasure tension)

> ## ✅ DECISIONS LOCKED (developer, 2026-06-25)
> - **Q1 `typeName` — REDESIGNED, no compromise (the developer pushed for precision; challenge upheld
>   the spine).** `typeName(x)` is resolved by `x`'s **static type** at compile time: a **compile-time
>   string literal** for value types (`int`/`float`/`bool`/`string`/**`bytes`**/`List`/`Map`/`Set`/enum
>   — baked identically into all 3 backends, so PHP's erasure is never consulted) and a **runtime
>   `get_class`** for objects (class/interface/union/intersection — handles polymorphism, and
>   `get_class` ≡ the instance's class byte-identically). An optional `T?` lowers to a runtime null-branch
>   (`null` → `"null"`, else the inner's rule). An **erased-generic** (`mixed`) static type degrades to
>   the coarse `kind` (inherent — the type isn't known there). **This sidesteps Q3 (enum erasure) entirely**
>   — enums emit a baked literal, never inspected in PHP. Mechanism: a checker pass recording a span-keyed
>   substitution (reuses the UFCS/`rewrite_*` infra + the absolute-span fix). **No runtime tagging of
>   collections** (rejected — would bloat every array + break interop).
> - **Plus `Reflect.kind(x) -> string`** (the developer's "parent type" idea, as its own native): the
>   **coarse** PHP-reproducible kind (`"array"` for List/Map/Set, `"string"` for bytes, `"object"`,
>   `"int"`, …) via a `__phorj_kind` helper — byte-identical for *all* inputs.
> - **Q5 — full set** (`className` + `typeName` + `kind` + the sorted enumeration natives).
> - **Q2/Q3/Q4 — I resolve during build:** Q2 verify `get_class` matches the transpiled (possibly FQN)
>   class name; Q3 sidestepped (literals); Q4 `fieldNames` via a transpiler-emitted per-class static list.
>
> Status: **APPROVED — implementing.** Original open-questions section retained below for context.
>
> ### Build notes (verified during implementation prep, 2026-06-25)
> - **Q3 enum erasure — VERIFIED:** an enum transpiles to a PHP **class hierarchy** —
>   `abstract class <Enum> {}` + `final class <Variant> extends <Enum> {}`. So a variant value is a PHP
>   object whose `get_class` is the **variant** name (`"Red"`), not the enum name. Consequences, all
>   byte-identical: (a) `kind(enumVal)` = `"object"` (Rust `Value::Enum` → `"object"`, PHP `is_object`
>   → `"object"`); (b) `typeName`/`className` on an enum emit a **compile-time literal of the *enum*
>   name** (the static type is the enum — variants aren't types — and we never call `get_class`, so the
>   variant-vs-enum mismatch is moot). **No runtime enum inspection anywhere.**
> - **`kind` PHP emission needs a gated helper:** native `php` closures emit inline PHP and can't set a
>   transpiler `uses_*` flag, so `Reflect.kind` is special-cased in `emit_member_call` (set
>   `uses_reflect_kind`, emit `__phorj_kind($x)`), with `__phorj_kind` defined once in
>   `emit_runtime_helpers` — the established gated-helper pattern (`__phorj_str`/`_div`/…). `__phorj_kind`
>   checks `is_callable` **before** `is_object` (a PHP closure is both; Phorj closures must map to
>   `"callable"`, instances/enums to `"object"`).
> - **`typeName`/`className` resolution** is a checker pass (type-directed, span-keyed substitution like
>   UFCS): value type → string literal; object type → a `Reflect.__classOf` native (`get_class`,
>   byte-identical for classes); enum → enum-name literal; optional → null-branch; erased → `kind`.
> Origin: Phase 2 Slice 1 of `docs/specs/2026-06-24-introspection-strings-process-design.md`, parked
> autonomously on 2026-06-25 as fork **F-006** (`docs/plans/2026-06-25-overnight-design-forks-review.plan.md`)
> because the original spec's "byte-identical with PHP" claim is unachievable as written.

## The problem (why this needs a design, not just a slice)

Phorj's correctness spine is `run ≡ runvm ≡ real PHP` (byte-identical stdout) for **every** program.
But the Phorj→PHP transpiler **erases** type distinctions that the Rust backends still see at runtime:

| Phorj runtime value | PHP runtime value | Can PHP recover the Phorj name? |
|---|---|---|
| `int` / `float` / `bool` / `string` | `int` / `float` / `bool` / `string` | ✅ yes (`is_int`/…) |
| `bytes` | **`string`** | ❌ indistinguishable from a real `string` |
| `List<T>` / `Map<K,V>` / `Set<T>` | **`array`** (all three) | ❌ all collapse to `array` |
| class instance | object of that class | ✅ `get_class` (for `package Main`) |
| enum variant | (see Q3 — current erasure unverified) | ❓ depends on the enum PHP shape |
| `null` | `null` | ✅ |
| closure | `\Closure` | ✅ `is_callable` |

So a native like `typeName(x)` that returns `"Map"` / `"Set"` / `"bytes"` / an enum name would diverge
on the PHP backend **in ordinary user code**, not just examples — a silent spine break. Reflection wants
to expose runtime type identity; erasure has thrown some of that identity away by the time PHP runs.

## Design principles

1. **The spine is inviolable.** No reflect native may return a value the PHP backend cannot reproduce
   byte-for-byte. A native that *can't* be byte-identical is either restricted, redesigned, or rejected —
   never shipped with a "diverges sometimes" caveat (that caveat IS a spine break).
2. **Reflection is read-only, name-level** (unchanged from the original spec): no invoke-by-name, no
   instantiate-by-name, no attribute reflection, no field mutation by name.
3. **Static type info is the checker's job, not reflection's.** Use `instanceof` / `match` for control
   flow (the checker proves them exhaustive). Reflection is for debugging / logging / serialization.

## Proposed native set (revised for byte-identity)

### Tier 1 — byte-identical, ship-able (recommended for v1)

| Native | Returns | PHP erasure | Byte-identical? |
|---|---|---|---|
| `className<T>(T x) -> string?` | class name for a class instance; `null` for a non-object | `is_object($x) ? get_class($x) : null` | ✅ for `package Main` classes (Q3 covers enums) |
| `isInstance<T>(T x, ...) ` | *(omit — that's just `instanceof`)* | — | — |

`className` on a **class instance** is byte-identical: `get_class` returns the PHP class name, which
equals the Phorj class name for a `package Main` program (and the de-mangled FQN for a packaged class —
needs a check that the transpiler's class name matches what `className` would report; see Q2).

### Tier 2 — needs the `ClassTables` plumbing + a sortable contract

| Native | Returns | PHP erasure | Byte-identical strategy |
|---|---|---|---|
| `interfaces<T>(T x) -> List<string>` | interface names the class implements (transitive) | `class_implements` | **return a SORTED list**; PHP side wraps `sort(array_values(class_implements($x)))` so order matches the Rust `BTreeMap`-sorted output |
| `parents<T>(T x) -> List<string>` | ancestor class names | `class_parents` | sorted, same wrapper |
| `traits<T>(T x) -> List<string>` | trait names used | `class_uses` | sorted, same wrapper |
| `methodNames<T>(T x) -> List<string>` | method names (incl. inherited) | `get_class_methods` | sorted, same wrapper |
| `fieldNames<T>(T x) -> List<string>` | declared field names | (object-vars / reflection) | sorted, same wrapper |

**Key contract decision:** these return **sorted** lists (lexicographic) rather than declaration- or
PHP-native order. Sorting is the only order both the Rust `BTreeMap`/`Vec` side and PHP's various
`class_*` builtins can agree on byte-for-byte. (Cost: callers lose declaration order — acceptable for
debugging/serialization; documented.) Each erases to its PHP builtin wrapped in `sort()`.

**Plumbing (the `NativeEval::Reflective` arm):** these need the static class hierarchy, which a value
doesn't carry. Add a third `NativeEval` variant:

```rust
enum NativeEval {
    Pure(fn(&[Value], &mut String) -> Result<Value, String>),
    HigherOrder(fn(&[Value], &mut ClosureInvoker) -> Result<Value, String>),
    Reflective(fn(&[Value], &ClassTables) -> Result<Value, String>),   // NEW
}
```

`ClassTables` is one bundle built **once** from the `Program` (reusing the existing
`ast::class_implements` / `class_supertypes` / `class_method_origins` + field decls), keyed by class
name, each list pre-sorted:

```rust
struct ClassTables {
    interfaces: BTreeMap<String, Vec<String>>,   // class -> sorted iface names
    parents:    BTreeMap<String, Vec<String>>,   // class -> sorted ancestor names
    traits:     BTreeMap<String, Vec<String>>,   // class -> sorted trait names
    methods:    BTreeMap<String, Vec<String>>,   // class -> sorted method names (incl inherited)
    fields:     BTreeMap<String, Vec<String>>,   // class -> sorted field names
}
```

- **Interpreter** already holds `classes` / `class_implements` / `method_origins` → builds `ClassTables`
  at `interpret()` start, hands `&ClassTables` to a `Reflective` native.
- **VM**: `BytecodeProgram` gains a `ClassTables` field (computed at `compile()` time, mirroring the
  existing `class_implements` field). The VM hands `&self.program.class_tables` to a `Reflective` native.
- **Coupled match sites** (the discipline): the 3 `NativeEval` dispatch sites
  (`vm/exec.rs`, `interpreter/call.rs`, `native/html_tests.rs`) each gain a `Reflective` arm.
- No new `Op`/`Value` (still `Op::CallNative`).

### Tier 3 — `typeName` (the genuinely hard one — DECISION REQUIRED, Q1)

`typeName(x) -> string` cannot be both **precise** (distinguish List/Map/Set/bytes/enum) and
**byte-identical** (PHP can't). Three honest options:

- **Q1-A — Coarse + byte-identical:** `typeName` returns what PHP *can* see: `"int"`/`"float"`/`"bool"`/
  `"string"` (bytes also → `"string"`), `"array"` (List/Map/Set all → `"array"`), the class name for an
  object, `"null"`, `"function"`. A `__phorj_type_name($x)` helper reproduces exactly this. **Byte-
  identical for all inputs**, but loses Phorj's finer distinctions (a Map reports `"array"`).
- **Q1-B — Drop `typeName`; keep only `className`.** Most of the "what is this" need is "what class is
  this" (`className`) + "is it a T" (`instanceof`). Skip `typeName` entirely.
- **Q1-C — Precise but run/runvm-only.** Keep precise names (`"Map"`/`"bytes"`/enum), but **exclude any
  program calling `typeName` from the PHP oracle** (quarantine, like Process I/O). A real departure from
  "everything is byte-identical with PHP" — needs explicit blessing, and it fragments the guarantee.

**My recommendation: Q1-A** (coarse + byte-identical) — it keeps the spine intact and is still useful
("is this a scalar / array / object / which class"); the finer List-vs-Map-vs-Set distinction is
statically known anyway (you wrote the type), so runtime precision there has low value.

## Open questions (these gate the build)

- **Q1 — `typeName` contract:** A (coarse, byte-identical), B (drop it), or C (precise, run/runvm-only)?
  *Rec: A.*
- **Q2 — packaged-class names:** for a non-`Main` package class, does `className` report the bare name
  or the namespaced FQN, and does it match `get_class` on the transpiler's emitted class? (Needs a quick
  empirical check; likely the FQN `Acme\Geometry\Point` on both — confirm.)
- **Q3 — enum erasure:** verify how an enum variant transpiles to PHP today (object? array? tagged?), then
  define `className`/reflection on enum values to match — or restrict reflection to class instances in v1.
- **Q4 — `fieldNames` on PHP:** `get_object_vars` only sees *initialized public* props at runtime;
  declared-field enumeration may need the static `ClassTables.fields` mirrored by a PHP **static list**
  the transpiler emits per class (not a runtime PHP reflection call). Confirm the approach.
- **Q5 — scope:** Tier 1 (`className`) + Tier 2 (sorted enumeration natives) only for v1, with `typeName`
  per Q1? Or a smaller first cut (just `className`) to land something immediately?

## What I did NOT decide (parked for you)

I did not ship any of this autonomously — the byte-identity contract for `typeName` (Q1) and the enum
erasure question (Q3) are genuine design forks. On your answers I implement the agreed tier set, each
byte-identity-gated with a guide example (using only the reproducible subset).
