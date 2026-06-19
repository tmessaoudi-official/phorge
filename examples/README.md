# Phorge examples

What Phorge can do **today**. Every `.phg` here runs byte-identically on both backends
(`phg run` and `phg runvm`) — enforced by `tests/differential.rs`, which globs this directory,
so a new example is auto-gated the moment it lands. This page is updated as examples are added.

## Index

| Example | What it shows |
|---|---|
| `hello.phg` | the minimal program — `package main;` + `import core.console;` + `console.println` |
| `fib.phg` | recursion, `for…in`, `List<int>` |
| `grades.phg` | enums + `match`, a class with a method, `List`, `for…in` |
| `realworld/ledger.phg` | bank accounts: classes + methods + `this`, payload enum + `match`, recursion (compound interest), integer-cents arithmetic, immutability (`apply` returns a fresh `Account`) |
| `realworld/library.phg` | catalogue: zero-payload + payload variants, `match`, a class, `List` + `for`, float arithmetic |
| `realworld/shop.phg` | cart + discounts: enum + `match`, class composition, recursion (bulk pricing), integer arithmetic |
| `realworld/rpg.phg` | turn-based combat: enum + `match`, class + methods + `this`, `List` + `for`, immutable state evolution |
| `guide/operators.phg` | arithmetic, comparison, logical, unary operators; `bool` |
| `guide/control-flow.phg` | `if`/`else`, `for…in`, recursion, mutual recursion |
| `guide/functions.phg` | functions: typed params, return types, a no-return function, composition, a `List<int>`-returning function |
| `guide/collections.phg` | `List<T>` literals, nested `List<List<int>>`, nested `for`, list of instances |
| `guide/classes.phg` | constructor promotion, methods, `this`, composition, a method call on a field |
| `guide/enums-match.phg` | payload + zero-payload variants; literal, binding, and variant patterns |
| `guide/strings.phg` | string interpolation |
| `guide/inference.phg` | `var` local type inference + `type` aliases (M3 S0) |
| `guide/ergonomics.phg` | indexing `xs[i]`, integer ranges `0..n` / `0..=n`, expression `if` (M3 S1) |
| `guide/null-safety.phg` | optionals `T?`, `??`, `?.`, `if (var x = opt)`, `opt!`, `match` over `T?` (M3 S2) |
| `guide/lambdas-pipe.phg` | lambdas (expression + statement body), higher-order functions, first-class named-fn references, the pipe operator `\|>` (M3 S3 Track A) |
| `guide/math.phg` | the `core.math` stdlib module — `sqrt`/`pow`/`floor`/`ceil`/`abs`/`min`/`max` (M3 Track B Wave 2) |
| `guide/text.phg` | the `core.text` stdlib module — `len`/`upper`/`lower`/`trim`/`contains`/`split`/`join`/`replace` (M3 Track B Wave 2) |
| `guide/file.phg` | the `core.file` stdlib module — `read` (→ `string?`), `exists`; reads a committed fixture, composes with S2 `??` / if-let (M3 Track B Wave 2) |
| `guide/bytes.phg` | the `bytes` type + `b"…"` literals (`\xHH`) + `core.bytes` interop — `from_string`/`to_string` (→ `string?`)/`len`/`concat`/`slice` (M6 W0) |
| `guide/html.phg` | `core.html` — the escape **kernel** (`text`/`raw`/`render`) plus the typed element **builders** (`el`/`void_el`/`attr`/`bool_attr`/`concat`); `Html`/`Attr` are distinct from `string`, XSS-safe by construction (core.html Waves 1–2) |
| `bench/workload.phg` | a **profiling** workload (CPU recursion + heap allocation) for `phg bench`/`disasm` — see `bench/README.md` |
| `transpile/demo.phg` | the **Phorge → PHP** bridge — see `transpile/README.md` |
| `build/app.phg` | **standalone executables** — `phg build` — see `build/README.md` |
| `cli/demo.phg` | the **`phg` CLI** — source forms, `check`/`parse`/`lex`, diagnostics, `explain` — see `cli/README.md` |
| `web/handler.phg` | the **M6 W1 HTTP handler model** — `Request`/`Response` classes, `parse_request`/`serialize_response` in pure Phorge, `handle(Request) -> Response`; `bytes` bodies, `req.header(name)` lookup, `bytes.find` + `text.split_once`. No socket yet (that's W3's `phg serve`) |
| `web/router.phg` | the **M6 W2 static router** — a data-driven `List<Route>` table, linear exact-match `(method, path)` scan → a `Handler` enum tag, dispatched by exhaustive `match` to named handler functions; method-sensitive 404 fallback. Pure Phorge (no new feature); path params + middleware deferred (Track A / generics) |
| `web/server.phg` | the **M6 W4 served app** — W1 parse/serialize + W2 routing + the single entry `respond(bytes) -> bytes` that **`phg serve`** runs over a real socket. `web/server.php` is the **`php -S`** front-controller bridge (both call the same `handle(Request) -> Response`) — see `web/README.md` |
| `project/tempconv/` | a **multi-file project** (M5) — mandatory packages, folder = path, cross-package qualified calls + import aliasing, namespaced PHP — see `project/README.md` |
| `project/withdeps/` | a project with a **vendored git dependency** (M5 S3) — `[require]`, `phg vendor`, `phorge.lock`, offline `vendor/` — see `project/withdeps/README.md` |

## Coverage matrix (the runnable surface)

| Feature | Examples |
|---|---|
| `int`/`float` arithmetic, `%`, comparison, logical, unary, overflow-checked | `guide/operators`, all `realworld/*` |
| immutable typed bindings | every example |
| functions, recursion, mutual recursion | `guide/functions`, `guide/control-flow`, `fib`, `ledger`, `shop` |
| `if`/`else`, `for…in` | `guide/control-flow`, `fib`, all `realworld/*` |
| `List<T>` literals, nesting, iteration | `guide/collections`, all `realworld/*` |
| classes: ctor promotion, fields, methods, `this`, field reads, composition | `guide/classes`, `ledger`, `rpg`, `grades` |
| enums (payload **and** zero-payload via `V()`) + exhaustive `match` | `guide/enums-match`, all `realworld/*`, `grades` |
| string interpolation `"{expr}"` | `guide/strings`, every example |
| indexing `xs[i]`, ranges `0..n` / `0..=n`, expression `if` | `guide/ergonomics` |
| null safety: `T?`, `??`, `?.`, `if (var x = opt)`, `opt!`, `match` over `T?` | `guide/null-safety` |
| lambdas (expr + stmt body), higher-order fns, first-class named-fn refs, pipe `\|>` | `guide/lambdas-pipe` |
| `var` local type inference, `type` aliases | `guide/inference` |
| `console.println(string)` (after `import core.console;`) | every example |
| `core.math` stdlib: `sqrt`/`pow`/`floor`/`ceil`/`abs`/`min`/`max` | `guide/math` |
| `core.text` stdlib: `len`/`upper`/`lower`/`trim`/`contains`/`split`/`join`/`replace` | `guide/text` |
| `core.file` stdlib: `read` (→ `string?`), `exists` (fixture-gated) | `guide/file` |
| `core.html` kernel (`text`/`raw`/`render`) + builders (`el`/`void_el`/`attr`/`bool_attr`/`concat`); `Html`/`Attr` ≠ `string` (XSS-safe by construction) | `guide/html` |
| `core.bytes`: `find` (→ `int?`); `core.text`: `split_once` (→ `List<string>`) | `web/handler` |
| HTTP handler model: `Request`/`Response`, `parse_request`/`serialize_response`, `handle()` | `web/handler` |
| static HTTP router: `List<Route>` table, exact `(method, path)` match → `Handler` enum + exhaustive dispatch | `web/router` |
| HTTP serve runtime: `phg serve` (native socket) + `php -S` front-controller, one `respond(bytes) -> bytes` entry | `web/server` |
| Phorge → PHP transpile | `transpile/demo` |
| standalone executable (`phg build`) | `build/app` |
| CLI: source forms, inspection (`check`/`parse`/`lex`), diagnostics, `explain` | `cli/demo` |
| multi-file projects: packages, folder = path, cross-package imports + aliasing, namespaced PHP | `project/tempconv` |
| git dependencies: `[require]`, `phg vendor`, `phorge.lock`, offline `vendor/` | `project/withdeps` |

## Three sharp edges

- **Every file declares a package (M5 S1) — `package main;` is the runnable entry.** Nothing lives
  "in the wind": each file's first line is a `package` declaration, never inferred. A runnable program
  uses the reserved `package main;` (every example here starts with it); `core` is reserved for the
  stdlib. Dotted library packages (`package acme.convert;`) + strict folder=path + cross-package
  imports are now **shipped** — see `project/tempconv/` and `project/README.md`.
- **Zero-payload enum variants use call form `V()` everywhere** — to construct (`Defend()`) *and* in
  a `match` arm (`Defend() =>`). A bare `Defend =>` arm is a catch-all *binding*, not a variant
  pattern, so it silently swallows every case.
- **`import core.console;` is load-bearing (M3 Wave 1).** Everything is namespaced — "nothing in the
  wind" — so there is no free global `println`: a program must `import core.console;` and call
  `console.println(...)`. Stdlib modules are reserved under `core.*`; the root lives in the import and
  the leaf qualifies the call (Go's `import "fmt"` → `fmt.Println`). The same leaf-qualified `import`
  resolves user `.phg` packages in a project (M5) — see `project/tempconv/`.

## Not yet supported (intentionally absent here)

These are designed but not implemented; they will arrive in **M3+** (the language-growth milestone),
and examples will be added as each lands: `Map`/`Set` values & indexing, the pipe operator `|>`,
exceptions (`try`/`catch`/`throw`), traits, function overloading, sized ints, `decimal`, and
**transitive** git dependencies (a dependency's own `[require]`; direct git deps are shipped — see
`project/withdeps/`).
