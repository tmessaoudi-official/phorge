# Foreign PHP interop (`declare`) — M8.5

These examples are **PHP-target-only**. They use `declare` to describe existing PHP functions/classes
so Phorge can type-check calls into them and transpile to idiomatic PHP that uses them directly. This is
the **migration bridge** — adopt Phorge incrementally over a PHP codebase.

Because foreign PHP only exists in the PHP runtime, the Rust backends (`phg run` / `phg runvm`) **refuse**
a program that uses `declare` (`E-FOREIGN-RUNTIME`). Run them by transpiling:

```sh
phg transpile builtins.phg > out.php && php out.php
```

The pure-Phorge byte-identity spine (`run ≡ runvm ≡ real PHP`) is untouched: these programs are
quarantined from the `differential.rs` example gate and validated instead by `tests/interop.rs`
(transpile → real PHP → golden output, the sibling `.out`).

| file | shows |
|------|-------|
| `builtins.phg` | foreign PHP free functions: `declare function strtoupper(string) -> string;` etc.; calls transpile to `\strtoupper(...)`; the `declare` lines emit no PHP. Foreign results compose with native Phorge + `Core.*`. |
| `classes.phg` | foreign PHP classes (S2): `declare class DateTimeImmutable { … }` — construction (`new \DateTimeImmutable`), instance methods (`$d->format(…)`), static factories (`\DateTimeImmutable::createFromFormat(…)`). |
| `exceptions.phg` | catching a foreign PHP exception (S3a): `declare class DivisionByZeroError implements Error { … }` makes it catchable; `intdiv(10, 0)` raises it; `catch` emits `catch (\DivisionByZeroError $e)` (caught by its own global name, so an `\Error`-family class works). |
| `withdecls/` | a project that shares its foreign surface in a `*.d.phg` declaration file (S3b) instead of repeating `declare` in every consumer. |

## How it works

- `declare function name(params) -> ret;` — a bodyless signature for an existing PHP function. Its name
  is the **real PHP name** (snake_case like `str_repeat` is fine — the camelCase rule is waived for
  foreign symbols, since the name is emitted verbatim). It produces no PHP definition; a call emits the
  global form `\name(...)` so it resolves to the PHP builtin even inside a namespace.
- `declare class Name [extends A] [implements I] { … }` — a bodyless foreign PHP class. `implements
  Error` (the built-in exception marker) makes a foreign exception catchable; references emit the global
  form (`new \Name`, `$o->m(…)`, `\Name::s(…)`, `catch (\Name $e)`) and no class definition.
- A `*.d.phg` file holds only `declare`s, carries **no `package`**, and is loaded ambiently into the
  project (the `.d.ts` analog) — its presence in the source tree is the opt-in, so foreign symbols are
  declared once and shared by every file. See `withdecls/`.
- `check` type-checks calls against the declared signatures; `transpile` emits the PHP; `run`/`runvm`
  refuse (foreign code needs the PHP runtime).
