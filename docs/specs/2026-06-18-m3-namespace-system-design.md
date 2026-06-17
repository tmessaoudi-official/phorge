# Phorge — Namespace / Package System — Design

> Brainstorm + design, 2026-06-18. Trigger: developer wants **everything in a namespace** — "nothing
> in the wind" — as the **default** behavior, with **meaningful, not bloated** names. Supersedes the
> flat-global model where `println` is the lone free builtin. This doc reshapes Track B
> (`2026-06-18-trackB-stdlib-io-imports.md`): namespacing becomes the **spine** of the stdlib + import
> work, and pulls a (deliberately stricter-than-PHP) user-package model forward. **Draft — locked
> decisions + open items below.**

## 1. The principle

**No free-floating globals.** Every callable is reachable only through a namespace path — built-ins
*and* user code. This is a discipline choice in the same family as the project's `strictNullChecks`
stance: **stricter than the PHP runtime, idiomatic PHP on emission.** It scales the stdlib safely —
the moment `core` grows past a handful of functions (`read`, `sqrt`, `split`, `map`, `sum`…), a flat
global namespace *would* collide with user code. Namespacing is therefore **necessary, not gold-
plating**, and the cheapest moment to adopt it is **before** the `NativeModule` registry is committed.

## 2. The model — Go-style module-qualified functions (NOT Java object-path)

Rejected: `System.out.println` (Java). It models stdout as an **object** (`System` class → `out`
field → `println` method); **PHP has no object analog** (idiomatic PHP is `echo`), so it breaks the
transpile contract **D-L9 (Phorge : PHP :: TypeScript : JavaScript)**. It also collides with Phorge's
existing `Expr::Member` (instance field/method) semantics.

Adopted: **module-qualified functions** (Go `fmt.Println`, Python `os.path.join`, Rust `std::io`).
You `import` a module; a call site names the module then the function. The namespace is a **compile-
time organizing layer that erases** to PHP's flat builtins on emission (like generics erase) — so it
costs the PHP target **nothing**.

## 3. Locked decisions (2026-06-18, developer-confirmed)

- **N-1 — everything namespaced, default, "nothing in the wind".** No bare global callables. The lone
  legacy global `println` is retired in favor of its namespaced form.
- **N-2 — Go-style module-qualified calls** (reject the Java `System.out.println` object-path shape).
- **N-3 — reserved `core.` root for the standard library.** `core.` means "ships with the language",
  and (given N-6) cleanly separates built-ins from user `app.*` packages. Reserved like built-in type
  names (cf. the existing `is_builtin_type_name` guard, `checker.rs`) — user code cannot define a
  package rooted at `core`.
- **N-4 — jargon-free leaf module names.** `console` (was `io`), `file` (was `fs`), plus `math`,
  `text` (string ops — `text` over `string` to avoid shadowing the `string` *type*), `list`, `json`,
  `time`. Stdlib module paths: `core.console`, `core.file`, `core.math`, `core.text`, `core.list`,
  `core.json`, `core.time`.
- **N-5 — `println` becomes `core.console.println`.** Default. The bare global form is removed; every
  example and test migrates (mechanical; "examples ship with features" already expects churn).
- **N-6 — user code is mandatorily packaged.** Deliberately stricter than PHP and TypeScript (neither
  *mandates* packaging), but emits idiomatic PHP namespaces. Developer lean: **explicit `package
  app.util;` declaration at file top** (Java/PHP-style) **+ a strict folder structure** — each segment
  of the dotted path must be a real directory (file `app/util/parse.phg` ⇒ `package app.util;`), so the
  package path and the on-disk path are forced to agree (Java's package rule). **Final syntax + the
  strict-folder enforcement are deferred** (developer: "decide later") — see open item O-B.

## 4. PHP emission (transpile contract)

Dotted Phorge path → PHP `\`-namespace, each segment PascalCased:

```
Phorge                         PHP
import core.console;           (front-end only — no emission)
core.console.println(x)        echo $x . "\n";          // console.* erase to PHP I/O builtins
core.math.sqrt(n)              sqrt($n)                  // core.* erase to PHP's flat global builtins
core.file.read(p)              file_get_contents($p)
---
package app.util;              namespace App\Util;
function parse(s) {...}         function parse($s) {...}
app.util.parse(x)              \App\Util\parse($x)       // user packages emit real PHP namespaces
```

So `core.*` is **erased** to PHP's native flat stdlib (zero namespace cost), while **user** packages
emit genuine PHP `namespace`s. Consistent with "TS-discipline over PHP-runtime."

## 5. Native registry (reshapes Track B Task 1)

`NativeFn` is keyed by **(module, name)**, not a bare name:

```
struct NativeFn {
    module: &'static str,   // "core.console", "core.math", …
    name:   &'static str,   // "println", "sqrt", …
    params: Vec<Ty>,
    ret:    Ty,
    eval:   fn(&[Value], &mut String) -> Result<Value, String>,  // shared by interpreter + VM (parity)
    php:    fn(&[String]) -> String,                              // PHP-emission mapping
}
fn registry() -> &'static [NativeFn];                 // built once via OnceLock (Vec<Ty> isn't const)
fn index_of(module: &str, name: &str) -> Option<usize>;
const CONSOLE_PRINTLN: usize = 0;                      // pinned; registry self-checks its own slot
```

`Op::Print` → **`Op::CallNative(idx, argc)`** (the registry index + arg count). Touches the three
coupled `Op` matches (`vm::exec_op`, `compiler::stack_effect`, `chunk::validate`) in one commit
(invariant). `CallNative` pushes the native's return value, so the old `Print` + `Const(Unit)` pair
collapses into one op (net stack-effect `1 - argc`, unchanged). The shared `eval` (threading
`&mut out`) is the structural parity guarantee — one impl, two callers, like the value kernels.

## 6. Import + call-site resolution

`import core.console;` (already parsed into `Item::Import { path: Vec<String> }` — decorative today)
becomes **load-bearing**: it enables a module's natives and binds a call-site qualifier. A call
`core.console.println(x)` parses as nested `Expr::Member`; resolution must recognize the **head as a
module path** (not field access on a value) and dispatch to the registry by `(module, name)`.

## 7. Open items (decide in this spec before/while implementing)

- **O-A — call-site form (decide before Wave 1; it shapes the parser + every line):**
  - *Full-path:* `core.console.println(x)` everywhere (3 segments). Most explicit; most verbose.
  - *Leaf-qualified (RECOMMENDED):* `import core.console;` then `console.println(x)` — the **root lives
    in the import** (identity), the **leaf qualifies the call** (Go's exact model: `import "fmt"` →
    `fmt.Println`). Reconciles "reserved `core` root" with "2-segment, non-bloated call sites".
    Collision between leaf names (`core.text` vs a user `…​.text`) handled by import aliasing (O-D).
- **O-B — user-package syntax + strict-folder enforcement** (N-6): explicit `package a.b;` decl (lean)
  vs file-path-derived; how strictly the compiler checks path↔folder agreement; where the source root
  is. Deferred by the developer.
- **O-C — `main()` entrypoint package:** an implicit root package, or `main()`'s file is the entry
  regardless of its package.
- **O-D — import aliasing** for leaf-name collisions (`import core.text as ctext;`?).
- **O-E — explicit import of stdlib required, or `core.*` implicitly available?** Lean: **required**
  (true "nothing in the wind" — you import even `core.console`, as Go requires `import "fmt"` for
  `Println`). Flagged because `println` is so common.

## 8. Implementation waves (post-decision)

1. **Wave 1 — namespaced native foundation** (reshaped Track B Task 1): registry keyed by
   `(module, name)`; `core.console.println`; `Op::Print`→`Op::CallNative`; real `import core.console`
   resolution; migrate all `println` call sites. Byte-identical, one green commit.
2. **Wave 2 — stdlib breadth:** `core.file` (fixture-gated file reads), `core.math`, `core.text`,
   `core.list`, `core.json`. Each a registry entry with its PHP erasure.
3. **Wave 3 — user packages** (N-6): `package` decl + strict folder structure + PHP `namespace`
   emission + entrypoint handling. The larger, separable piece.

## 9. ROI summary

High, and ideally timed. Namespacing the stdlib is **necessary** (not optional) once the stdlib grows,
prevents global/user collisions, costs the PHP target ~nothing (erasure), and is far cheaper to adopt
*before* the registry ships than to retrofit after. The rejected Java object-path would have breached
the transpile contract; the adopted Go model maps cleanly. User-package mandatory packaging is a
deliberate stricter-than-PHP discipline that still emits idiomatic PHP namespaces.
