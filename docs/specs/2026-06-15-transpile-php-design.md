# Phorge → PHP Transpiler — Design

> Milestone: **Converter (stage 1 of bidirectional)**. Built after M1 (interpreter).
> Direction: **Phorge → PHP only**. PHP → Phorge import is a separate, larger future
> milestone (needs a full PHP front-end + dynamic→static type inference) and is out of
> scope here.

**Goal:** Emit semantically-equivalent, runnable PHP 8.x source from a type-checked
Phorge program, via a new `phg transpile <file>` subcommand that prints PHP to stdout.

**Architecture:** A new `src/transpile.rs` codegen module walks the **untyped AST** (same
AST the evaluator walks), tracking the same global tables (functions / enums / classes)
and a per-function variable/field scope so it can resolve identifiers and dispatch calls
exactly as the evaluator does. `phorge::cli::cmd_transpile(src) -> Result<String, String>`
gates on the type-checker (reuse `parse_checked`) then calls `transpile::emit(&Program)`.
`main.rs` adds `transpile` to its subcommand set.

**Tech:** Rust, std only. No new deps. Output is a single PHP file beginning with `<?php`.

---

## CLI Surface (decided)

- `phg transpile <file>` → PHP source to **stdout**, exit 0.
- Gated on the type-checker: ill-typed input → `type error at L:C: msg` on stderr, exit 1
  (identical contract to `run`). Only well-typed programs are emitted.
- Fits the existing `cmd_*(src) -> Result<String,String>` pattern (Ok = print verbatim,
  Err = stderr + exit 1). Composable + unit-testable as a pure string function.
- `main.rs` subcommand set becomes `run | check | parse | lex | transpile`; usage string
  updated. Exit codes unchanged (0 ok / 1 compile-or-IO / 2 usage).

## Enum encoding (decided): abstract base + subclass per variant

```php
abstract class Shape {}
final class Circle extends Shape {
    public function __construct(public float $radius) {}
}
final class Rect extends Shape {
    public function __construct(public float $w, public float $h) {}
}
```
- One `abstract class <EnumName> {}` base.
- One `final class <Variant> extends <EnumName>` per variant, with **promoted public
  props** named after the variant's field names (`Param.name`). Nullary variants → empty
  ctor (or none).
- Variant construction `Circle(2.0)` → `new Circle(2.0)`.

## `match` encoding (decided): `instanceof` chain

A `match` over an enum scrutinee emits an ordered `instanceof` chain; each arm binds its
payload fields to locals from the subclass's promoted props, then yields the arm body:

```php
if ($s instanceof Circle) { $r = $s->radius; return 3.14159 * $r * $r; }
if ($s instanceof Rect)   { $w = $s->w; $h = $s->h; return $w * $h; }
throw new \UnhandledMatchError();
```
- **Positional binding:** a pattern var binds to the subclass prop at the *same index*,
  not by name. `Circle(r)` → `$r = $s->radius;` (r is the 1st field, `radius`). `Rect(w,h)`
  → `$w = $s->w; $h = $s->h;`.
- Wildcard arm `_ => e` → trailing unconditional `{ ...; }` (no final throw).
- Both forms (return-position and var-decl-init) end with `throw new \UnhandledMatchError();`
  after the chain unless a wildcard arm is present (matches the checker's exhaustiveness
  guarantee; the throw is a defensive backstop).
- **Position restriction (judgment call — flagged for review):** the M1 transpiler emits
  `match` only in **return position** (`return match …`) and **var-decl-init position**
  (`T x = match …;` → assigns `$x` in each arm). `match` in any other expression position
  → clean transpile error `transpile error: match in this position is not yet supported`
  (never emit broken PHP). This covers all realistic M1 programs (incl. the §6 sample) and
  avoids the IIFE-with-`use`-capture complexity. Lifting this is a later enhancement.

## Construct mapping (full set)

| Phorge | PHP |
|---|---|
| `import a.b.c;` | dropped (no-op; std is implicit) |
| `function f(int a) -> float { }` | `function f(int $a): float { }` |
| no return type | `: void` |
| `class C { private int x; constructor(private int y){} function m()->int{} }` | `class C { private int $x; function __construct(private int $y){} function m(): int {} }` |
| field decl `private int x;` | `private int $x;` (visibility preserved) |
| promoted ctor param `private int y` | PHP promoted param `private int $y` (native, 1:1) |
| `T name = expr;` (local) | `$name = expr;` (PHP locals are untyped) |
| `return expr;` / `return;` | `return expr;` / `return;` |
| `if (c) {} else {}` | `if (c) {} else {}` |
| `for (T x in it) {}` | `foreach ($it as $x) {}` |
| `{ … }` block | `{ … }` |
| `expr;` | `expr;` |
| int/float/bool literals | same literal (`12.0` → `12.0`) |
| string literal w/ interpolation | **concatenation** (see below) |
| ident `name` (local/param) | `$name` |
| ident `name` (current-class field) | `$this->name` |
| `obj.field` / `obj.method(a)` | `$obj->field` / `$obj->method(a)` |
| binary `+ - * / % == != < <= > >= && \|\|` | same operators |
| unary `-x`, `!x` | same |
| `println(x)` (builtin) | `echo (x) . "\n";` |
| free call `f(a)` | `f(a)` |
| variant call `Circle(2.0)` | `new Circle(2.0)` |
| class call `Greeter("Tak")` | `new Greeter("Tak")` |
| list literal `[a, b]` | `[a, b]` (PHP array) |
| `List<T>` / `Map<K,V>` / `Set<T>` type hints | `array` |

### Type hints (param/return/field)
`int→int`, `float→float`, `bool→bool`, `string→string`, unit→`void`,
`List/Map/Set→array`, enum/class name → that class name.

### String interpolation → concatenation (judgment call — flagged for review)
Phorge interpolation allows **free function calls** inside `{…}` (e.g. `"area = {area(s)}"`),
which PHP's `"{$…}"` syntax does **not** support. To be always-correct and avoid PHP
interpolation edge cases, every interpolated string is emitted as a concatenation of
string-literal segments and parenthesized expressions:
- `"Hello {name}"` → `"Hello " . $name` (or `$this->name`)
- `"area = {area(s)}"` → `"area = " . (area($s))`
- a pure literal `"hi"` → `"hi"` (no concat)

### Identifier / call resolution (mirrors the evaluator)
The transpiler tracks, per function/method, the set of in-scope local/param names and
(inside a method) the current class's field set — so a bare ident resolves to `$name`
vs `$this->name` exactly as the evaluator's `eval_ident` does. Call dispatch mirrors
`eval_call`: builtin `println` → `echo`; name in variants → `new`; name in classes →
`new`; else free function call.

## Deferred / unsupported (clean transpile error, never broken PHP)
Same spirit as the interpreter's deferred corners: `null`/`T?`, `|>` pipe, indexing,
Map/Set literals, overloading, bare nullary-variant refs, and `match` outside
return/var-decl-init position → `transpile error: <feature> is not yet supported`.

## Error handling
`transpile::emit` returns `Result<String, String>` (the inner err is a `transpile error:
…` message). `cmd_transpile` maps the checker gate to `type error …` and emit failures to
`transpile error …`; both → stderr + exit 1. No panics.

## Testing
- **Unit (`src/transpile.rs`)**: per-construct emit tests asserting substrings of the PHP
  output (functions, enum subclasses, promoted ctor, match→instanceof, interpolation→
  concat, println→echo, for→foreach, `new` dispatch, field→`$this->`). Plus the full §6
  sample → assert key PHP fragments.
- **Round-trip (the strong test)**: if a `php` CLI is available on PATH, transpile the §6
  sample and the `examples/*.phg`, run the emitted PHP with `php`, and assert its stdout
  equals `phg run`'s stdout (`Hello Tak\narea = 12.56636\narea = 12\n`). If `php` is
  absent, skip with a printed notice (don't fail the suite). [Verified at plan time: check
  `command -v php` before relying on this.]
- **CLI subprocess (`tests/cli.rs`)**: `phg transpile examples/grades.phg` exits 0 and
  stdout starts with `<?php`; ill-typed input exits 1.

## Out of scope (this milestone)
PHP → Phorge import; PHP namespaces/autoloading/composer output; formatting beyond simple
indentation; preserving comments.
