# E-PKG-TYPE Lift — Cross-Package Types — Implementation Design

> Status: **design (implementation-grade), not yet implemented.** Author pass 2026-06-20, after the
> generic-methods sub-slice (`bd8782c`). This is the first half of the developer's "both 1 and 2"
> directive: design the E-PKG-TYPE lift first, then implement generic types/classes on top.
> Anchored by the adopted **selective type import** spec (`2026-06-20-selective-type-import-design.md`,
> §9 ADOPTED) and the verified cross-package *function* machinery map (loader/transpiler).

## 1. Goal & non-goals

**Goal:** a **library** package (`package acme.geometry;`) may declare a `class`/`enum`/`interface`,
and another package may **use that type** — retiring the `E-PKG-TYPE` rejection (`loader.rs:277`). The
consumer references it through the **adopted `import type`** mechanism:

```phorge
// src/acme/geometry/point.phg
package acme.geometry;
class Point { constructor(public int x, public int y) {} function sumXY() -> int { return this.x + this.y; } }

// src/main.phg
package main;
import type acme.geometry.Point;          // binds bare `Point` → Acme\Geometry\Point
import Core.Console;
function main() {
  var p = Point(3, 4);                     // instantiation
  Console.println("{p.sumXY()}");          // 7
  Console.println("{p instanceof Point}"); // true
}
```

**Non-goals (this slice):**
- **No module-qualified type reference** (`import acme.geometry;` then `Geometry.Point`). The adopted
  bundle is the *terminal* `import type` form; the module-qualified type form is a deferred follow-on
  (the spec lists both in §4.1 but §9 adopts the terminal form). Functions keep the Go-qualified
  `pkg.fn()` form — unchanged.
- **No generics interaction yet** — generic *types* (`Box<T>`) are the **next** sub-slice; this one ships
  monomorphic cross-package types. (They compose cleanly: a generic library type erases its `<T>` via the
  existing `erase_generics` before the loader's type-resolution sees it.)
- **No wildcard import** (PHP has no `use X\*` — adopted spec §3).
- **No terminal *function* import** ("nothing in the wind" — functions stay Go-qualified).

## 2. The model — extend the function mangle/resolve pass to types

The cross-package *function* machinery (verified map) is exactly the template:

| Concern | Functions (exists) | Types (this slice) |
|---|---|---|
| Mangle a library def's name | `loader::mangle(pkg, fn)` → `Acme\Util\compute`; `main`/empty stay bare | **same `mangle`**, applied to class/enum/interface **names** → `Acme\Geometry\Point` |
| Symbol table (Pass 1) | `defined: HashMap<(pkg,fn), mangled>` | **new** `types: HashMap<(pkg,type), mangled>` (parallel) |
| Consumer binding | `user_import_map`: leaf qualifier → dotted path (for `pkg.fn()`) | **new** `type_import_map`: bare name (or alias) → `(dotted pkg, type)` from `import type` |
| Resolve a reference (Pass 2) | `resolve_call`: bare→same-pkg mangle; `q.fn()`→cross-pkg mangle | **extend resolution** to rewrite **type names** (annotations, instantiation, `instanceof`, enum access) to the mangled FQN |
| Transpiler de-mangle | functions bucketed by `namespace_of`; bare→`Main` | **same bucketing** — a mangled class/enum/interface lands in its `namespace Acme\Geometry { … }`; references emit the FQN `\Acme\Geometry\Point` |
| Runtime | n/a (names are just strings) | `Value::Instance{class}` / `Value::Enum{name}` carry the **mangled** string on both backends — already string-keyed, so interpreter/VM "just work" if names are consistently mangled |

**Key invariant (byte-identity):** a single-`package main` program produces **no `\` names** (mangle is
identity for `main`), so the transpiler's flat path runs and output is byte-for-byte the pre-lift result.
Cross-package types only change output for genuinely multi-package programs.

## 3. AST + parser changes

1. **`import type` statement.** Reuse `Item::Import` with a discriminator rather than a new item, to keep
   the four backends' item matches stable: add `kind: ImportKind` to `Item::Import` where
   `enum ImportKind { Module, Type }` (or a `bool type_only`). `import type a.b.C;` parses the same dotted
   path; the **leaf is the type name**, the prefix is the package. `as Alias` already parsed for module
   imports — reuse it.
   - Parser: in `parse_import`, after `import`, peek for the contextual `type` keyword (like `as` is
     contextual). `import type` → `ImportKind::Type`.
2. **No new type syntax.** The consumer writes a **bare** type name (`Point`), so `Type::Named{name}`,
   instantiation `Call{Ident("Point")}`, and `Expr::InstanceOf{type_name:"Point"}` are unchanged at the
   parse level — only *resolution* maps the bare name to its FQN.

## 4. Loader changes (`src/loader.rs`)

1. **Retire `reject_library_types`** (lines 277–299) — delete the `E-PKG-TYPE` gate. (Keep a narrower
   guard: a `package main` type is still entry-local; a library `package main` is already `E-VENDOR-MAIN`.)
2. **Pass 1 — type symbol table.** Alongside `defined`, build
   `types: HashMap<(String,String), String>` mapping `(pkg_dotted, type_name) → mangle(pkg, type_name)`
   for every `Item::{Class,Enum,Interface}` in a **non-main** package (a `main` type stays bare, so it
   needn't be indexed — bare lookup still finds it). Enforce `E-DUP-DEF` for a duplicate
   `(pkg, type_name)` (parallel to functions; also catches a type colliding with… no, types and funcs are
   separate namespaces in PHP, so key the maps separately).
3. **Pass 1 — `type_import_map`** per consuming file: walk `Item::Import { kind: Type, path, alias }`;
   bind `alias.unwrap_or(path.last())` → `(path[..-1] joined, path.last())`. Diagnostics here:
   - `E-TYPE-IMPORT-UNKNOWN` — the `(pkg, type)` is not in the `types` table (no such exported type).
   - `E-TYPE-IMPORT-CONFLICT` — two `import type` bind the same bare name (must `as`-alias one).
   - `E-TYPE-IMPORT-BUILTIN` — leaf is a built-in (`Int`/`List`/`Map`/`Set`/…); built-ins are import-free.
   - `E-TYPE-IMPORT-SHADOW` — bare name collides with a `package main` local type **or** a module-import
     qualifier (keep import kinds disjoint, the `E-SHADOW-IMPORT` discipline).
4. **Pass 2 — type-reference rewrite.** Extend the resolve pass (currently `resolve_item`/`resolve_call`,
   which rewrite **call exprs only**) to also rewrite **type names**. A new `resolve_type_name(name, ctx)`:
   - If `name` is in the file's `type_import_map` → its mangled FQN.
   - Else if `(ctx.package, name)` is in `types` (a **same-library-package** sibling type) → its mangled
     FQN. (So a library type referencing its own package's sibling type resolves too.)
   - Else unchanged (a `package main` local type, or a built-in, stays bare).
   Apply it everywhere a type name lives — this is the **bulk of the work** and the main risk surface:
   - **Type annotations** in `Type::Named{name}`: params, returns, fields, ctor params, `var`/`for` decls,
     `(T)->T` function types, lambda param/return types. (Recurse through `Type` like `erase_generics`'s
     `rty` does — a near-mirror helper.)
   - **Instantiation**: a `Call{ callee: Ident(TypeName) }` whose `TypeName` resolves as a type → rewrite
     the callee to the mangled name. (Disambiguation: the resolve pass already special-cases bare-ident
     calls for functions; a type-import name takes precedence as a constructor — a name is either an
     imported type or a function, never both in one file, guarded by `E-TYPE-IMPORT-SHADOW`.)
   - **`instanceof`**: `Expr::InstanceOf{ type_name }` → rewrite to the mangled name.
   - **Enum access/construction**: `Color.Red` / `Color.Circle(r)` — the *enum name* head resolves.
   - **`match` patterns** over an imported enum: the variant pattern's enum-name head resolves.
5. **Same-package type self-reference inside a library package** (e.g. `acme.geometry`'s `class Segment`
   has a `Point start` field) — covered by the `(ctx.package, name)` branch in `resolve_type_name`.

## 5. Checker changes (`src/checker.rs`)

The checker runs **after** the loader has merged + mangled, so it sees fully-mangled names. The class/
enum/interface tables (`self.classes` etc.) are keyed by the **mangled** name; `resolve_type`'s `other`
arm looks up the (already-mangled) name and finds it. **So the checker needs little change** — the same
"resolve before backends" discipline as function mangling. Confirm:
- `class_implements` / `iface_flat_methods` operate on the mangled names uniformly (they already key by
  the stored name).
- `instanceof` type-name check (`E-INSTANCEOF-TYPE`) accepts the mangled name (it is in the class/iface
  table post-merge).
- **`E-PKG-TYPE` in the checker?** The map shows the rejection is loader-only; verify no checker-side
  copy. (The checker's `E-PKG-TYPE` mention, if any, is the per-file `check()` path used by `-e`/stdin —
  those are single-`package main`, so unaffected.)

## 6. Transpiler changes (`src/transpile.rs`)

The function de-mangling already buckets by `namespace_of`. Extend the **type** path:
1. **Bucketing** (`emit_program_namespaced`, lines 183–193): currently *all* enums/classes/interfaces go
   to `Main`. Change to `namespace_of(&type_name)` — a mangled `Acme\Geometry\Point` → bucket
   `Acme\Geometry`; a bare `Point` (package main) → `Main`. The class/enum/interface is emitted with its
   **last segment** name inside its namespace block (reuse `last_segment`).
2. **Type references at use sites** emit the **FQN** `\Acme\Geometry\Point` (adopted §4.4 — uniform with
   functions, no `use` pass). Touch the emit points: `new \Acme\Geometry\Point(...)`, a typed param/return
   annotation `\Acme\Geometry\Point $p`, `instanceof \Acme\Geometry\Point`, enum access
   `\Acme\Geometry\Color::Red`. A single helper `emit_type_name(mangled) -> "\\" + mangled` (and bare →
   bare) centralizes it. (A mangled name already contains `\`; prefix a leading `\` for an absolute FQN.)
3. **Single-package byte-identity** preserved — no `\` names ⇒ flat path ⇒ identical output.

## 7. Backends (interpreter/VM) — no structural change

`Value::Instance{class}` and `Value::Enum{name}` are string-keyed; method/variant dispatch looks up the
(mangled) name in the merged tables. Because the loader rewrote every definition AND reference to the same
mangled string *before* either backend runs, `run ≡ runvm` holds by construction — the same guarantee the
function mangling already relies on. **No new `Op`, no `Value` change.**

## 8. Diagnostics (new)

`E-TYPE-IMPORT-UNKNOWN`, `E-TYPE-IMPORT-CONFLICT`, `E-TYPE-IMPORT-BUILTIN`, `E-TYPE-IMPORT-SHADOW`
(all loader-side, with `phg explain` entries). `E-PKG-TYPE` is **retired** (its `explain` entry becomes a
"lifted in <commit>" note rather than being deleted, to keep old links resolvable).

## 9. Test & example plan

- **Example** (byte-identity-gated, project-aware harness): a new `examples/project/<name>/` two-package
  project — a library package exporting a `class` (and an `enum`) consumed via `import type` from
  `package main`; runs identically on `run`/`runvm`/**real PHP 8.6**. Mirrors `examples/project/tempconv`.
- **Differential**: project-aware harness already globs `examples/project/*/`; the new project is gated
  automatically. Add focused loader tests for each new diagnostic (UNKNOWN/CONFLICT/BUILTIN/SHADOW) and a
  cross-package `instanceof` + method-call agreement case.
- **Transpile**: assert the consuming site emits `new \Acme\Geometry\Point(` and `instanceof
  \Acme\Geometry\Point`, and the def lands in `namespace Acme\Geometry {`.

## 10. Risk register

| # | Risk | Mitigation |
|---|------|-----------|
| R1 | **Type-reference rewrite misses a position** (a `Type::Named` site not walked) → a bare name reaches a backend and a cross-package type silently fails to resolve | Mirror `erase_generics`'s exhaustive `rty`/`rstmt`/`rexpr` walk — it already enumerates every `Type` and `Expr` position; reuse that enumeration as the checklist. Add a debug assert post-merge: no `Type::Named` in a non-main item resolves to an unknown name. |
| R2 | **Instantiation vs function-call ambiguity** (`Foo()` — type ctor or fn?) | A name is an imported type XOR a function in one file (`E-TYPE-IMPORT-SHADOW` enforces disjointness); the type table is consulted first for a bare-ident call. |
| R3 | **Enum/match name resolution** more positions than class refs | Enumerate enum-access + match-pattern heads explicitly in the rewrite; add a cross-package enum example. |
| R4 | **PHP FQN in type position** (`function f(\Acme\Geometry\Point $p)`) — PHP accepts absolute FQN in param types | Verified against PHP namespace rules; the real-PHP oracle gates it. |
| R5 | **Scope creep** — enums + interfaces + classes + generics all at once | **Ship classes first** if the rewrite surface proves large; enums/interfaces are the same mechanism and can be a fast follow. Generic types are explicitly the *next* sub-slice. |

## 11. Sequencing within "both 1 and 2"

1. **This slice — E-PKG-TYPE lift (cross-package monomorphic types via `import type`).** One green
   byte-identical commit (or two: classes, then enums/interfaces, if the rewrite surface is large — R5).
2. **Next slice — generic types/classes `Box<T>`** (package-main first; then it composes with cross-package
   types for free, since a generic library type erases `<T>` before the loader's type resolution).
3. **Follow-on — `import type` polish** already lands here; the deferred module-qualified type form
   (`Geometry.Point`) is a later optional ergonomic.
