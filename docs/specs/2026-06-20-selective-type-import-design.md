# Selective Type Import — Design (DRAFT / brainstorm)

> Status: **IMPLEMENTED** (M-RT cross-package types) — the terminal `import type Pkg.Path.Type [as A];`
> form ships with the E-PKG-TYPE lift; see `docs/specs/2026-06-20-epkgtype-lift-crosspackage-types-design.md`
> for the implementation design and `examples/project/shapes/` for a worked example. The deferred
> module-qualified form (§4.1 row 1, `Geometry.Point`) and generic types remain future work.
> Original status: **design discussion, not implemented.** Raised by the developer 2026-06-20 (post M-RT S7b-2):
> *"import should be able to import the last item — e.g. `import Core.List.List`, or `import Core.List`
> then `List.List`."* This spec reframes that instinct, challenges it, and proposes a coherent model.
> Gated on library packages exporting **types** (`E-PKG-TYPE` lift) — see §7 sequencing.

## 1. The instinct, restated honestly

The developer wants two import ergonomics:
1. **Qualified** — `import Acme.Geometry;` then `Geometry.Point` (package-qualified type reference).
2. **Terminal/selective** — `import Acme.Geometry.Point;` then bare `Point`.

This is the **Go-vs-Java axis**. Phorj locked **Go** for *functions* ("everything namespaced, nothing
in the wind", leaf-qualified calls, Java object-path rejected — `[[namespace-system-decisions]]`). The
question is whether *types* get the extra **terminal** form.

## 2. The category error in the literal example (the first challenge)

`import Core.List.List` / `List.List` does **not** map onto today's model [Verified: `src/native.rs`
registry + `src/compiler.rs resolve_cty`]:

- `Core.List` is a **module**; its members are the *functions* `reverse`/`sum`. **No member named
  `List` exists in it.**
- `List<T>` is a **built-in type constructor** (like `int`) — globally available, **import-free**,
  resolved by the checker/compiler, not a member of any package.

So you would **never** `import Core.List.List`: `List<T>` needs no import at all, the same way `int`
doesn't. The terminal-type-import feature is for **user/library types** (`Acme.Geometry.Point`), never
for built-ins. This is the sharpest correction to the original framing.

## 3. The decisive constraint: the PHP target has no wildcard import

PHP (the transpile target) resolves a class by **namespace + `use`** or by **FQN**:

```php
namespace Acme\Geometry { class Point { … } }
// consumer:
use Acme\Geometry\Point;        // then bare  Point / new Point()
// or
new \Acme\Geometry\Point();     // FQN, no `use`
```

PHP has **no `use Acme\Geometry\*`** — you cannot bulk-import a namespace's symbols. `use` is terminal,
file-scoped, and symbol-segregated (`use` = class, `use function`, `use const`). **Therefore Phorj's
type-reference model must map onto terminal `use` or FQN — a Phorj `import Acme.Geometry.*;` wildcard
has no honest PHP target** and would have to be desugared into one explicit `use` per exported type
(compiler-enumerated). Reject wildcard for v1.

## 4. Proposed model

### 4.1 Two forms, both type-capable (matches the developer's instinct)

| Form | Binds | Use site | PHP |
|------|-------|----------|-----|
| `import Acme.Geometry;` (module) | qualifier `Geometry` | `Geometry.Point` (type) **and** `Geometry.fn()` (call) | FQN `\Acme\Geometry\Point` |
| `import type Acme.Geometry.Point;` (terminal) | bare type `Point` | `Point` | FQN `\Acme\Geometry\Point` |
| `import type Acme.Geometry.Point as Pt;` | bare type `Pt` | `Pt` | FQN, source-only alias |

The terminal form is **pure sugar** over the qualified form for a single type — exactly the developer's
"or" — except the leaf is a **type**, not a function, and the importable thing is a **user package
type**, not a built-in.

### 4.2 Why an explicit `import type` keyword (second challenge: implicit is a footgun)

The disambiguator "is the last segment a package or a type?" could be **structural** (resolve the full
path against the loaded package set; if it's a package → module import, else strip the leaf → type
import). Rejected:

- **Silent meaning-flip**: adding a package `Acme.Geometry.Point` later would silently turn an existing
  `import Acme.Geometry.Point;` from a *type* import into a *module* import. A rename changes semantics
  at a distance. Unacceptable for a language that made packaging mandatory *for explicitness*.
- **Case can't disambiguate**: package segments, leaf modules (`Core.List`), and types are **all
  PascalCase**, so casing gives no signal.

Explicit `import type …;` mirrors TypeScript `import type` and matches Phorj's established explicit
taste (mandatory `package`, explicit imports even for the stdlib). The rare both-a-package-and-a-type
collision is then a clean `E-…` rather than a silent flip.

### 4.3 It is an erasure-style, checker+loader feature — no runtime, no Op, no Value

Like generics / `type` aliases / `html"…"`, this resolves **before any backend**:

- **Checker**: `resolve_type` consults a per-file *type-import map* (bare name → `(package, Type)`),
  in addition to the local `package Main` classes, so a bare `Point` resolves to the right package's
  type. New diagnostics: `E-TYPE-IMPORT-CONFLICT` (two terminal imports binding the same bare name —
  must alias), `E-TYPE-IMPORT-SHADOW` (bare name collides with a local type or a module qualifier),
  `E-TYPE-IMPORT-UNKNOWN` (no such exported type in that package).
- **Loader**: already mangles cross-package *function* names to FQN bare names; extend the same
  resolution so a cross-package **type** reference resolves to its FQN before the backends. The
  interpreter/VM then see fully-resolved names; only the transpiler de-mangles.
- **Transpiler**: emit the type as a **PHP FQN** (`\Acme\Geometry\Point`) at every use site —
  **uniform with the existing function mangling, no `use` statements** (third challenge below). The
  definition stays in its `namespace Acme\Geometry { class Point {} }` brace-block.

### 4.4 FQN emission vs `use` statements (third challenge)

Emitting `use Acme\Geometry\Point;` per consuming block reads prettier, but requires a `use`-collection
pass and per-block dedup/alias bookkeeping. Emitting the **FQN** `\Acme\Geometry\Point` at each use site
is **uniform with how functions are already de-mangled**, needs no new pass, and the generated PHP is
machine-output (never hand-edited), so ugliness is irrelevant. **Recommend FQN.** Consequence: `import
type` buys **nothing at the PHP level** — it is purely a *Phorj-source* ergonomic (write bare `Point`).
That is exactly analogous to Go's qualifier being erased, and it keeps the transpiler trivial.

## 5. Collision & shadowing rules (sketch)

- Two `import type` binding the same bare name from different packages → `E-TYPE-IMPORT-CONFLICT`
  (alias one). PHP would fatal identically ("cannot use … as … — name already in use").
- Terminal bare name == a `package Main` local type → `E-TYPE-IMPORT-SHADOW`.
- Terminal bare name == a module-import qualifier → `E-TYPE-IMPORT-SHADOW` (keep the two import kinds
  disjoint, like the existing `E-SHADOW-IMPORT` discipline).
- `import type` naming a built-in (`Int`, `List`, …) → `E-TYPE-IMPORT-BUILTIN` (built-ins are
  import-free; §2).

## 6. What this does NOT do (scope discipline)

- No wildcard / on-demand import (§3 — no PHP target).
- No terminal import of **functions** (would reintroduce a free-floating global — violates "nothing in
  the wind"; functions stay Go-qualified). PHP `use function` exists, but the locked principle wins.
- No change to the stdlib call surface (`Console.println`, `List.reverse`) — those are functions.

## 7. Sequencing

Hard dependency: **library packages must be able to export types** (today `E-PKG-TYPE` rejects a
non-`main` type — `src/loader.rs:278`). That lift is part of **generics-all → cross-package types**.
So: design now (this doc); implement *after* E-PKG-TYPE is lifted. Until then there is no cross-package
type to import, and built-ins remain import-free — so there is nothing to build against.

## 7a. Why the PHP output is brace-namespaced (not `namespace Xxx;`)

Developer question (2026-06-20): why `namespace Acme\Geometry { … }` instead of `namespace Acme\Geometry;`
then the rest of the file? Answer — it is **forced**, not stylistic [Verified: `phg transpile
examples/project/tempconv` output + PHP namespace rules]. A transpiled program is **one PHP file** that
contains *multiple* package namespaces **plus a global `namespace { }` block** holding the bootstrap
(`\Main\main();`) and the `__phorj_*` runtime helpers:

```php
namespace Acme\Convert { … }
namespace Acme\Label   { … }
namespace Main { function main(): void { … } }
namespace { \Main\main(); /* __phorj_* helpers */ }   // global, unnamespaced
```

PHP permits the semicolon form `namespace X;` only for a file with a *single* namespace and *no* global
code. PHP's rule: **to combine non-namespaced (global) code with namespaced code, only the bracketed
syntax is supported.** Phorj's output has both multiple namespaces and a global block → braces are
mandatory; `namespace Acme\Convert;` beside a global block is a parse error. The only alternative
(`namespace X;` per file) needs **one file per package + a PSR-4 autoloader**, which was rejected
because PSR-4 autoloads *classes, not free functions*, and Phorj is function-heavy. Type-import is
unaffected: the FQN `\Acme\Geometry\Point` (or a `use`) simply lives inside the consuming brace block.

## 8. Open forks (for the developer)

1. Explicit `import type …` (recommended) vs implicit structural resolution.
2. PHP emission: FQN everywhere (recommended, uniform) vs `use` statements (prettier).
3. Confirm built-ins (`List`/`Map`/`Set`/scalars) stay **import-free** (recommended) — i.e. the literal
   `import Core.List.List` is intentionally *not* a thing; `List<T>` is a primitive.
4. Scope: types-only terminal import (recommended) vs also allow terminal **function** import (rejected
   here as it breaks "nothing in the wind").

## 9. Decisions Log

- **[2026-06-20] ADOPTED** the recommended bundle (developer, "Option 1"): explicit **`import type
  Pkg.Path.TypeName [as Alias];`** marker; **FQN PHP emission** (`\Acme\Geometry\Point`, uniform with
  function de-mangling, no `use` pass); **built-ins (`List`/`Map`/`Set`/scalars) stay import-free** — so
  `import Core.List.List` is intentionally *not* a thing (`List<T>` is a primitive, like `int`);
  **types-only** terminal import (functions stay Go-qualified `Pkg.fn()` — "nothing in the wind").
- **[2026-06-20] CLARIFIED** (developer asked): the brace-namespaced PHP output (`namespace X { … }`) is
  *forced* by single-file multi-package output + a global bootstrap block, not a style choice (§7a). It
  is orthogonal to type-import and unchanged.
- **[2026-06-20] SEQUENCING:** implement after `E-PKG-TYPE` is lifted (library packages export types),
  which is part of generics-all → cross-package types. Not before — nothing to import against until then.
