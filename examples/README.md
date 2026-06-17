# Phorge examples

What Phorge can do **today**. Every `.phg` here runs byte-identically on both backends
(`phorge run` and `phorge runvm`) — enforced by `tests/differential.rs`, which globs this directory,
so a new example is auto-gated the moment it lands. This page is updated as examples are added.

## Index

| Example | What it shows |
|---|---|
| `hello.phg` | the minimal program — `println` |
| `fib.phg` | recursion, `for…in`, `List<int>` |
| `grades.phg` | enums + `match`, a class with a method, `List`, `for…in` |
| `realworld/ledger.phg` | bank accounts: classes + methods + `this`, payload enum + `match`, recursion (compound interest), integer-cents arithmetic, immutability (`apply` returns a fresh `Account`) |
| `realworld/library.phg` | catalogue: zero-payload + payload variants, `match`, a class, `List` + `for`, float arithmetic |
| `realworld/shop.phg` | cart + discounts: enum + `match`, class composition, recursion (bulk pricing), integer arithmetic |
| `realworld/rpg.phg` | turn-based combat: enum + `match`, class + methods + `this`, `List` + `for`, immutable state evolution |
| `guide/operators.phg` | arithmetic, comparison, logical, unary operators; `bool` |
| `guide/control-flow.phg` | `if`/`else`, `for…in`, recursion, mutual recursion |
| `guide/collections.phg` | `List<T>` literals, nested `List<List<int>>`, nested `for`, list of instances |
| `guide/classes.phg` | constructor promotion, methods, `this`, composition, a method call on a field |
| `guide/enums-match.phg` | payload + zero-payload variants; literal, binding, and variant patterns |
| `guide/strings.phg` | string interpolation |
| `bench/workload.phg` | a **profiling** workload (CPU recursion + heap allocation) for `phorge bench`/`disasm` — see `bench/README.md` |
| `transpile/demo.phg` | the **Phorge → PHP** bridge — see `transpile/README.md` |

## Coverage matrix (the runnable surface)

| Feature | Examples |
|---|---|
| `int`/`float` arithmetic, `%`, comparison, logical, unary, overflow-checked | `guide/operators`, all `realworld/*` |
| immutable typed bindings | every example |
| functions, recursion, mutual recursion | `guide/control-flow`, `fib`, `ledger`, `shop` |
| `if`/`else`, `for…in` | `guide/control-flow`, `fib`, all `realworld/*` |
| `List<T>` literals, nesting, iteration | `guide/collections`, all `realworld/*` |
| classes: ctor promotion, fields, methods, `this`, field reads, composition | `guide/classes`, `ledger`, `rpg`, `grades` |
| enums (payload **and** zero-payload via `V()`) + exhaustive `match` | `guide/enums-match`, all `realworld/*`, `grades` |
| string interpolation `"{expr}"` | `guide/strings`, every example |
| `println(string)` (the only builtin) | every example |
| Phorge → PHP transpile | `transpile/demo` |

## Two sharp edges

- **Zero-payload enum variants use call form `V()` everywhere** — to construct (`Defend()`) *and* in
  a `match` arm (`Defend() =>`). A bare `Defend =>` arm is a catch-all *binding*, not a variant
  pattern, so it silently swallows every case.
- **`import` is decorative today.** `import std.io;` parses but resolves nothing — there is no
  multi-file module system yet (planned for **M5**). The `println` builtin is always available.

## Not yet supported (intentionally absent here)

These are designed but not implemented; they will arrive in **M3+** (the language-growth milestone),
and examples will be added as each lands: `null` / `T?` / `Option`, `Map`/`Set` values & indexing,
the pipe operator `|>`, exceptions (`try`/`catch`/`throw`), traits, function overloading, sized ints,
`decimal`, and real multi-file `import` resolution.
