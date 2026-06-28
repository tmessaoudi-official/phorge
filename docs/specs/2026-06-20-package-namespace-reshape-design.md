# Package / Namespace Reshape — Design Spec

> **Status:** ✅ **COMPLETE** (2026-06-23). All four slices landed: slice 1 (manifest `name` →
> `module`, §5.1); slice 2a (identifier casing `E-NAME-CASE`/`E-TYPE-CASE`); **slice 2b (package +
> import + alias segment casing `E-PKG-CASE`, `0aa5a90`)**; **slice 3 (entry `package main` → `Main`,
> `15a5745`)**; slice 4 (library types / `E-PKG-TYPE` lifted, shipped earlier with generics-all). All
> examples + projects + fixtures migrated to PascalCase (distributable coordinates — manifest `module`,
> `[require]` keys, vendor dirs, lockfile `name` — stay lowercase, concept C); the reshape is
> output-preserving (`pascal()` already mapped segments) so `run≡runvm≡real PHP 8.4` stayed byte-
> identical throughout. 796 tests green.
> **Date:** 2026-06-20 · **Code state at spec time:** master `8676d1d` (core.html Wave 3), tree clean.
> **Decider:** the developer (each decision below was an explicit, adversarially-challenged choice).
> **Supersedes / extends:** `docs/specs/2026-06-18-m3-namespace-system-design.md` (the original
> "everything namespaced, Go-shaped" decisions) and the M5 project model
> (`docs/specs/2026-06-18-m5-project-model-design.md`). M5's interim scope cuts (library
> functions-only; lowercase packages) are revised here.

---

## 1. Motivation

The developer reopened the `package` keyword question — *"we're not packaging anything like Java;
it's really a namespace like PHP."* Interrogating that surfaced that the single word `package` is
overloading **four distinct concepts**:

| | Concept | Today |
|---|---|---|
| **A** | lexical **namespace qualifier** (prefixes names → PHP `namespace`) | the keyword |
| **B** | **source-grouping unit** (folder = path, dir-mapped) | the keyword |
| **C** | **distributable / dependency unit** (git dep, `vendor/`, manifest) | `phorj.toml` `name` |
| **D** | **runnable-entry marker** | `package Main` |

The mature-language lesson is that **nobody uses one word for A+B+C**: Go splits `package` (A+B) +
`module` (C); Rust splits `mod` (A) + `crate` (B+C). And the languages that *enforce folder=path*
(Go, Java, Python) call the unit a **package/module**, never a **namespace** — `namespace` (PHP, C#)
is reserved for the *path-independent, multi-per-file* construct, which is exactly what Phorj
forbids. So Phorj's construct **is** a Go-style package, not a PHP namespace.

Key clarification on the cross-OS angle: **PHP-the-language is case-insensitive** for
namespaces/classes; PHP's famous cross-OS case bug lives in **Composer's PSR-4 autoloader**
(case-sensitive file lookup on Linux). **Phorj emits a single brace-namespaced file with no
autoloader**, so the *output* is immune; the only case concern is Phorj's own `.phg` loader, which
we control — hence we can pick and enforce any source convention for free.

---

## 2. Decisions (all ratified)

### D1 — Keep the keyword `package`; rename the manifest's distributable to `module`
The construct is Go-shaped (folder=path, dir-mapped, `Main` entry), so `package` is the accurate
term; `namespace` would imply PHP's looseness Phorj forbids and orphan the entry marker. The TS:JS
contract (Phorj's own model) does **not** require source-keyword == target-keyword — TS keeps
`interface`/`type`/`enum` and lowers them — so emitting PHP `namespace A\B {}` from a `package`
keyword is correct, not dishonest. The real wart (the word `package` colliding with `phorj.toml`'s
`name = "vendor/package"`) is fixed at the **manifest** layer: the distributable becomes a
**`module`** (Go's `go.mod` split — `package` = code unit, `module` = distributable).

### D2 — Runnable entry is `package Main;` (PascalCase-consistent)
Diverges from Go's iconic lowercase `package Main`, but obeys the one casing rule (D5) uniformly —
no lowercase exception.

### D3 — Library packages MAY declare types (lift `E-PKG-TYPE`)
A library that cannot export a `class`/`enum` is not a library. Requires: cross-package **type**
name-mangling (today only functions are mangled — `loader.rs`), namespaced PHP emission for classes
(`namespace Acme\Shapes { class Circle … }`), and qualified type references at use sites
(`Shapes.Circle`). The M5 `E-PKG-TYPE` guard is removed.

### D4 — Many items per file + free functions (Go/PHP shape, unchanged)
Any number of functions/classes/enums per file; free functions need no class wrapper (locked
identity — the reason for single-file brace-namespace PHP, since PSR-4 can't autoload free
functions). Filename is **free** (only parent directories form the package; many files in one dir
merge into one package).

### D5 — Casing conventions (enforced)
- **Types / enums / variants:** `PascalCase`.
- **Functions / methods / variables / parameters:** `camelCase`.
- **Package / folder segments:** `PascalCase` → a **1:1 mapping to PHP** (`package Acme.StringUtil;`
  ⇒ `namespace Acme\StringUtil`, no casing transform; the loader's `pascal_seg` becomes identity).
- Enforced by a new checker diagnostic (proposed `E-PKG-CASE` for segments; identifier-casing may be
  a lint `W-CASE` rather than a hard error — TBD at build time).

**Accepted tradeoff (D5a):** PascalCase packages visually overlap PascalCase types
(`StringUtil` can be both a package leaf and a type — `StringUtil.StringUtil`). This is a
*readability* cost, **not** a correctness one: Phorj has **no static methods**, so `X.member` is
always a package/value member, `X(...)` is a constructor, `X x` is a type position — grammar position
disambiguates. Go avoids the overlap by lowercasing packages; Phorj accepts it for the clean 1:1
PHP mapping, plus a guard (D5b).

**D5b — guard:** a type name may not equal an in-scope **import leaf** (extends the
`E-SHADOW-IMPORT` family). Prevents the genuine same-scope `StringUtil` (type) vs `StringUtil`
(imported package leaf) ambiguity.

---

## 3. Recommended defaults for the not-yet-explicitly-decided cases

These follow the Go-shaped model; override at build time if desired.

- **Exports / visibility:** all top-level items in a package are exported (no package-private yet).
  Revisit if access control is wanted (Go uses capitalization; Phorj's identifier casing is already
  spoken for by D5, so a future `private`/`pub` keyword would be the route).
- **Imports:** leaf-qualified (`import Acme.StringUtil;` → call `StringUtil.fn()`), with `as`
  aliasing (already shipped). No glob/wildcard imports; no importing a single member
  (`import Acme.Shapes.Circle`) — import the package, qualify the member.
- **Sub-packages:** `Acme.StringUtil` and `Acme.StringUtil.Inner` are **separate** packages with no
  implicit access between them (Go model).
- **`core.` root** stays reserved for the stdlib.

---

## 4. Migration impact (why this is milestone-scale)

Breaking, wide-blast: rename keyword usages + entry (`package Main` → `package Main`) + PascalCase
**every** package segment and its folder, across all examples and multi-file projects; PascalCase
all type names and camelCase all functions/vars in every `.phg`, fixture, and inline test program;
**migrate the shipped stdlib API** to camelCase (`split_once`→`splitOnce`, `bool_attr`→`boolAttr`,
`void_el`→`voidEl`, `from_string`→`fromString`, `split_once`, etc.) — a public-surface break;
rename `phorj.toml` `name` → `module`; update loader type-mangling for cross-package types; add
`E-PKG-CASE` + the D5b type-vs-leaf guard; update FEATURES/CHANGELOG/KNOWN_ISSUES/README + every
spec that shows a package decl. The byte-identity spine (`run≡runvm≡php`) must stay green throughout
(rename-only changes are output-preserving, so the differential harness is the safety net).

A tooling-assisted migration (a codemod over `.phg` files, like `tools/wave1_migrate.py`) will be
needed; this is **not** a single-sitting change.

---

## 5. Suggested build order (each slice independently green)

1. **Manifest `name` → `module`** (C-word fix) — smallest, isolated. ✅ **DONE** — `Manifest.module`
   (struct + parser key + error messages + `namespace_root`), `phorj.toml` `module = …`, all manifest/
   loader/project/vendor fixtures + example projects migrated; lockfile `name` (dependency coordinate)
   and the `[require]` keys unchanged; 471 tests green, PHP oracle ran, clippy + fmt clean.
2. **Casing enforcement** (`E-PKG-CASE` for segments + identifier-casing lint) + migrate all `.phg` /
   stdlib / fixtures / docs to the conventions (codemod). Keyword stays `package`; entry stays
   `main` *temporarily* to isolate churn.
3. **Entry `main` → `Main`** (reserved-name change) — mechanical once casing lands.
4. **Types in libraries** (lift `E-PKG-TYPE` + cross-package type mangling + namespaced PHP for
   classes/enums + the D5b type-vs-leaf guard) — the only real *new capability*; the rest is rename.

---

## 6. Open questions for build time
- `E-PKG-CASE` hard error vs `W-CASE` lint for identifier casing (segments almost certainly hard;
  identifiers maybe lint to avoid over-gating).
- Exact PHP emission for a library `enum`/`class` under a dotted namespace (extend the existing
  function de-mangling in `transpile.rs`).
- Whether `module` (manifest) also wants a lockfile-key rename for consistency.
