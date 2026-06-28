# Class entry points (Batch-1 D) — design

**Status:** design-locked (developer decisions 2026-06-27, plan
`docs/plans/2026-06-27-big-chunk-entry-native-lift.plan.md` Decisions Log).

## Goal

A program entry — `main` (and later `handle`) — may be declared in **either** form:

```phorj
// (1) Go-style top-level free function (unchanged, the existing form)
function main(): int { ... }

// (2) Java-style static method on a class (NEW)
class App {
  static function main(): int { ... }
}
```

The `List<string> args` parameter stays **optional** (0 or 1 param) in both forms — `main(): void`
keeps working, no breaking migration. An instance method named `main` is **not** an entry (only a
top-level function or a `static` method is).

## Decisions (locked)

- **Both forms allowed** (developer overruled my "top-level only" recommendation — wants the
  flexibility; I argued it's a Java-ism solving a non-problem, see plan).
- **args optional**, both forms (`main()` or `main(List<string> args)`, returning `void` | `int`).
- **Ambiguity is an error, never silent:** if a program has more than one entry (a top-level `main`
  **and** a class-static `main`, or two classes each with a static `main`) → **`E-MULTIPLE-MAIN`**.
- **Transpile:** a class-static entry bootstraps as `\Main\App::main(...)` (vs `\Main\main(...)`).
- `handle` gets the same treatment in a later slice (C); Core.Http (slice B) lands first with a
  top-level `handle`.

## Mechanism — one shared resolver, consumed by every backend

The byte-identity spine requires all three backends to agree on *which* function is the entry. So a
single shared query in `ast` is the source of truth (mirrors `class_implements`):

```rust
// Some((Some("App"), &decl)) for a static-method entry; Some((None, &decl)) for a top-level one.
pub fn entry_point<'a>(program: &'a Program, name: &str) -> Option<(Option<&'a str>, &'a FunctionDecl)>
```

It scans top-level `Item::Function`s named `name`, then every class's `static` method named `name`.
A companion `entry_point_count(program, name)` powers `E-MULTIPLE-MAIN`.

### Checker

- Scope the existing entry-only rules (`check_main_signature`, the `throws`-on-`main` ban, `cur_is_main`)
  to entry positions: `f.name == "main" && (cur_class.is_none() || in_static_method)` — so an
  *instance* method named `main` is an ordinary method, not constrained.
- After collection, `entry_point_count(program, "main") > 1` → `E-MULTIPLE-MAIN`.
- `E-STATIC-THIS` already forbids a static method touching `this`, so a static `main` can't read
  instance state — exactly right for an entry.

### Interpreter (`interpret_main`)

Resolve via `entry_point`. Free → `run_call("main", …, this=None)` as today. Static → `run_call`
with the static method's params/body and `this=None` (statics have no receiver). argv is supplied to
a 1-param entry, unchanged.

### Compiler / VM

`compile_method` always reserves slot 0 for `$this` (`arity = 1 + params`), even for a static method.
So `BytecodeProgram` gains two fields beside `main`:
- `main_is_static: bool` — push a dummy receiver (`Value::Unit`) into slot 0 before the call.
- `main_params: usize` — the user-declared param count (0 or 1); push argv as the next slot iff `== 1`.

`compile_program` resolves the entry index from `entry_point`: free → `fns["main"].index`; static →
the `(class,"main")` entry in the `methods` table. The static method's body already ends in
`Return`, so no other VM change.

### Transpiler

`main_bootstrap_stmt` emits `\Main\App::main(<argv?>)` when the entry is static (the class is already
emitted in its `namespace Main { … }` block), else `\Main\main(<argv?>)` as today.

## Scope / non-goals

- This slice ships **`main`** in both forms. `handle` (slice C) reuses `entry_point("handle")`.
- No change to user-written static-method *call sites* (`ClassName.method()` is still unsupported as
  an expression — out of scope; the entry is invoked by the runtime, not user code).
- No visibility requirement on a static `main` (the runtime calls it; visibility is irrelevant).

## Tests + example

- Checker: accept top-level `main`; accept class-static `main` (0-arg + argv forms); reject two
  entries (`E-MULTIPLE-MAIN`); an instance method named `main` is allowed and unconstrained.
- Differential: a gated `examples/guide/class-main.phg` runs byte-identically on run/runvm/real PHP,
  plus an exit-code parity case for a class-static `main(): int`.
