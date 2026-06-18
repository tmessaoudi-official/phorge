# Phorge examples

What Phorge can do **today**. Every `.phg` here runs byte-identically on both backends
(`phorge run` and `phorge runvm`) — enforced by `tests/differential.rs`, which globs this directory,
so a new example is auto-gated the moment it lands. This page is updated as examples are added.

## Index

| Example | What it shows |
|---|---|
| `hello.phg` | the minimal program — `import core.console;` + `console.println` |
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
| `guide/math.phg` | the `core.math` stdlib module — `sqrt`/`pow`/`floor`/`ceil`/`abs`/`min`/`max` (M3 Track B Wave 2) |
| `guide/text.phg` | the `core.text` stdlib module — `len`/`upper`/`lower`/`trim`/`contains`/`split`/`join`/`replace` (M3 Track B Wave 2) |
| `guide/file.phg` | the `core.file` stdlib module — `read` (→ `string?`), `exists`; reads a committed fixture, composes with S2 `??` / if-let (M3 Track B Wave 2) |
| `bench/workload.phg` | a **profiling** workload (CPU recursion + heap allocation) for `phorge bench`/`disasm` — see `bench/README.md` |
| `transpile/demo.phg` | the **Phorge → PHP** bridge — see `transpile/README.md` |
| `build/app.phg` | **standalone executables** — `phorge build` — see `build/README.md` |
| `cli/demo.phg` | the **`phorge` CLI** — source forms, `check`/`parse`/`lex`, diagnostics, `explain` — see `cli/README.md` |

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
| `var` local type inference, `type` aliases | `guide/inference` |
| `console.println(string)` (after `import core.console;`) | every example |
| `core.math` stdlib: `sqrt`/`pow`/`floor`/`ceil`/`abs`/`min`/`max` | `guide/math` |
| `core.text` stdlib: `len`/`upper`/`lower`/`trim`/`contains`/`split`/`join`/`replace` | `guide/text` |
| `core.file` stdlib: `read` (→ `string?`), `exists` (fixture-gated) | `guide/file` |
| Phorge → PHP transpile | `transpile/demo` |
| standalone executable (`phorge build`) | `build/app` |
| CLI: source forms, inspection (`check`/`parse`/`lex`), diagnostics, `explain` | `cli/demo` |

## Two sharp edges

- **Zero-payload enum variants use call form `V()` everywhere** — to construct (`Defend()`) *and* in
  a `match` arm (`Defend() =>`). A bare `Defend =>` arm is a catch-all *binding*, not a variant
  pattern, so it silently swallows every case.
- **`import core.console;` is load-bearing (M3 Wave 1).** Everything is namespaced — "nothing in the
  wind" — so there is no free global `println`: a program must `import core.console;` and call
  `console.println(...)`. Stdlib modules are reserved under `core.*`; the root lives in the import and
  the leaf qualifies the call (Go's `import "fmt"` → `fmt.Println`). *File-based* `import` of user
  `.phg` modules is still a later milestone.

## Not yet supported (intentionally absent here)

These are designed but not implemented; they will arrive in **M3+** (the language-growth milestone),
and examples will be added as each lands: `Map`/`Set` values & indexing, the pipe operator `|>`,
exceptions (`try`/`catch`/`throw`), traits, function overloading, sized ints, `decimal`, and real
multi-file `import` resolution.
