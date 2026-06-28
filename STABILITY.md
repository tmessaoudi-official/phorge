# Stability tiers

Phorge's public surface is split into three tiers. The policy attached to each tier — and what changes
are allowed when — is defined in [`SEMVER.md`](SEMVER.md).

- **stable** — intended to last. In `0.x` it may still change, but only with a documented `### Breaking`
  CHANGELOG note; at `1.0` it freezes under strict SemVer. Every stable construct is exercised by the
  [conformance corpus](conformance/), so a regression (or an undocumented removal) fails CI.
- **experimental** — usable today, but the design is still settling. May change or be removed in a minor
  release (with a CHANGELOG note), and is exempt from the 1.0 freeze until it graduates to *stable*.
- **deprecated** — slated for removal. Using a deprecated stdlib symbol emits the **`W-DEPRECATED`**
  lint naming its replacement and removal version (see [`docs/DEPRECATION.md`](docs/DEPRECATION.md)).

> The tier is a statement about the *interface*, not the implementation. The `Op` set, bytecode format,
> AST, and emitted-PHP shape are always internal (see SEMVER "What compatible means").

## Language constructs

### stable
- **Modules & packages** — `package` declaration, `import`, `import type`, import aliasing (`as`),
  folder = package path, the reserved `package Main` entry.
- **Functions** — typed parameters, default parameters, return types, `void`/`never`, recursion,
  lambdas (`fn`), first-class function values, the pipe operator `|>`.
- **Bindings & flow** — `var` local type inference, `type` aliases, immutable-by-default + `mutable`,
  `if`/`else`, expression-`if`, `for … in`, `foreach (… as …)`, `while`/`do`, C-`for`,
  `break`/`continue`, return-on-all-paths totality.
- **Types** — `int`, `float`, `bool`, `string`, `bytes`, `decimal`; `List<T>`, `Map<K,V>`, `Set<T>`,
  fixed-length `[T; N]`; optionals `T?` with `??`, `?.`, `if`-let, `opt!`; union `A | B` and
  intersection `A & B`; erased generics `<T>` on functions, methods, classes, and enums.
- **Classes & objects** — constructor promotion, methods, `this`, fields (incl. `static`), property
  hooks, visibility (`public`/`private`/`protected`), `instanceof` with smart-cast.
- **Inheritance & reuse** — `extends` (single + multiple), `open` (final-by-default), `abstract`,
  `interface` + `implements`, `trait` + `use`, method/function/static **overloading**.
- **Static methods** — `ClassName.method(...)`, inherited and trait-supplied statics, overloaded
  statics.
- **Enums & pattern matching** — variant + literal + type patterns, `match` arm guards (`when`), struct
  destructuring, let-destructuring, exhaustiveness.
- **Operators** — arithmetic (`+ - * / % **`), comparison, logical, bitwise (`& | ^ ~ << >>`), string
  interpolation/concatenation, raw strings, `"""` text blocks, ranges `a..b` / `a..=b`.
- **Errors** — checked exceptions `throws`, `throw`, `try`/`catch`/`finally`, `?`-propagation;
  uncatchable faults/panics.

### experimental
- **HTML templating** — `Core.Html` + the `html"…"` template literal (XSS-safe builders).
- **Reflection** — `Core.Reflect` (runtime kind/type queries).
- **Cast operator** — `value as Type` over the full primitive/union matrix (fallibility-typed).
- **`Secret<T>`** — the opaque, non-printable wrapper (security primitive; surface still settling).
- **HTTP router, middleware & route attributes** — the `Core.Http` `Router` (path params, literal>param
  precedence, `use` middleware, `group` sub-routers) + the `#[Route(...)]` attribute +
  `Http.autoRouter()` desugar (M6 W2 + W2-ext: path params, `use` middleware, `group` sub-routers,
  `{name:regex}` route constraints, `#[Route]` on static methods; plus the W3 concurrent server
  (`phg serve --workers N`, bounded thread pool). The web layer is largely in place; remaining work is
  refinement (optional segments, instance controllers, perf).

## Stdlib modules

### stable
`Core.Console`, `Core.Math`, `Core.Text`, `Core.Bytes`, `Core.Convert`, `Core.Decimal`, `Core.List`,
`Core.Map`, `Core.Set`, `Core.Json`, `Core.Hash`, `Core.Encoding`, `Core.Url`, `Core.Validate`,
`Core.Csv`, `Core.Random`, `Core.File`.

### experimental
`Core.Regex` (depends on the `regex` crate), `Core.Crypto` (Argon2id; depends on `argon2`),
`Core.Reflect`, `Core.Html`, `Core.Env`, `Core.Process`, `Core.Http` (the M6 web layer is in progress).

## CLI commands

### stable
`run`, `runvm`, `check`, `transpile`, `build`, `test`, `fmt`, `explain`, `bench`, `disasm`, `parse`,
`lex`, plus the source forms (`<file>`, `-`/stdin, `-e`/`--eval`, `--`) and `-h`/`-v`.

### experimental
`lift` (PHP → Phorge draft — *review required*, inherently lossy), `serve` (HTTP; M6 in progress),
`vendor` (git dependencies; transitive deps deferred), `lsp` (language server; the query layer is
growing).

## deprecated

*None yet.* When a construct or stdlib symbol is deprecated it will be listed here with its replacement
and the version in which it will be removed, and its use will emit `W-DEPRECATED`.
