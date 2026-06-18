# Track B — std-only stdlib + I/O + real `import std.*` — Implementation Plan

> Design source: `docs/specs/2026-06-18-m3-next-intuitive-features-and-io-design.md` (Track B, L-2).
> Build order D→B→A (L-1); D done. **Autonomous inline**, one green commit per task, byte-identical on
> both backends, PHP round-tripped. **Draft.**

**Goal:** a rich **std-only** standard library exposed through a `NativeModule` registry (dual+
registration: checker signature + interpreter impl + VM impl + PHP-emit mapping), with **real
`import std.*` resolution**, unlocking file-reading + real-import examples. URL/network deferred to M6
(L-2). Non-deterministic capabilities (clock, env, random-unseeded) may exist but are excluded from the
byte-identity-gated example set.

## Decisions Log
- [2026-06-18] AGREED (NAMESPACE RESHAPE → `docs/specs/2026-06-18-m3-namespace-system-design.md`):
  everything is namespaced ("nothing in the wind"), default behavior. **Go-style module-qualified**
  (reject Java `System.out.println` object-path — breaks the PHP transpile contract). Reserved
  **`core.`** root for the stdlib; jargon-free leaves **`console`** (was io) + **`file`** (was fs) +
  math/text/list/json/time. **`println` → `core.console.println`**; bare global retired. **User code
  mandatorily packaged** (stricter than PHP/TS by choice; emits idiomatic PHP `namespace`s) — leaning
  explicit `package a.b;` + strict folder=path, final syntax deferred ("decide later"). Task 1 is
  **reshaped**: the native registry is keyed by `(module, name)` and `import core.console` becomes
  load-bearing. Open fork before Wave 1: call-site form — full-path `core.console.println` vs
  **leaf-qualified** `import core.console; console.println(...)` (recommended).
- [2026-06-18] AGREED (L-1): build D→B→A. (D = `bench --vs-php`, shipped `b3ba602`.)
- [2026-06-18] AGREED (L-2): std-only stdlib now; URL/network deferred to M6; determinism (not the
  dependency) gates examples.
- [2026-06-18] DESIGN: generalize the special-cased `println` into a `NativeModule` registry. The four
  facets register together (checker sig / interpreter eval / VM eval / PHP-emit), single-sourcing each
  native so backends can't drift. `println` becomes the first registry entry (migrated, not duplicated).
- [2026-06-18] DESIGN (Op): native dispatch needs a runtime call path. Add **one** `Op::CallNative(idx,
  argc)` (extends the three coupled matches — `vm::exec_op`, `compiler::stack_effect`,
  `chunk::validate`). Justified by genuine need (Rule of Three: println + fs + math + …). `Op::Print`
  is retired in favor of `CallNative` to the `println` entry (no two mechanisms).
- [2026-06-18] DESIGN (eval sharing): the interpreter and VM call the **same** `eval: fn(&[Value]) ->
  Result<Value, String>` per native — the parity guarantee is structural (one impl, two callers), like
  the value kernels.
- [2026-06-18] DESIGN (imports): `import std.io;` / `import std.fs;` resolve to enabling those modules'
  natives (today `import` is decorative). File-based `import a.b.c` of user `.phg` modules stays M5.
  Open fork: are std.* natives always in the prelude (import optional) or import-gated? Lean **prelude
  for `std.io` (println)** for back-comat; **import-gated** for `std.fs` etc. — decide in Task 3.
- [2026-06-18] DESIGN (determinism): file examples read a **committed fixture** under `examples/`, so
  both backends read identical bytes → byte-identical. `read_file` returns `string?` (null on missing).
- [2026-06-18] AGREED (Wave 2 scope split): the native registry's `params`/`ret` are **concrete** (`Ty`
  has no type variable) and **lambdas (S3) don't exist yet**. So of the five planned leaf modules, only
  **`core.file`, `core.math`, `core.text`** are cleanly typeable today (all-concrete signatures,
  determinism-gateable). **`core.list` is deferred** (`map`/`filter`/`reduce` need S3 lambdas;
  `reverse`/`sum`/`first`/`last` need generics — `List<T>`) and **`core.json` is deferred** (`parse ->
  Json?` needs a dynamic/`Json` type; `stringify(v)` needs a generic `v`). Wave 2 ships the three
  buildable modules now; list+json land once generics or S3 exist. Each module = one green commit + a
  byte-identity-gated guide example.

## Progress
- [x] Task 1 / **Wave 1** — namespaced native foundation: `src/native.rs` registry keyed by
  `(module,name)` (`OnceLock`, pinned `CONSOLE_PRINTLN`); `Op::Print`→`Op::CallNative`; import-driven
  resolution in all four backends; `import core.console;` + `console.println` (global `println`
  retired); `E-SHADOW-IMPORT` guard; full call-site migration + example `is_ok` hardening. 367 tests
  green, clippy + fmt clean, real-PHP round-tripped. *(Reshaped from the original Task 1 per the
  namespace design.)*
- [ ] Task 2 — `std.fs`: `read_file(string) -> string?`, `write_file`, `exists` (std::fs ↔ PHP)
- [ ] Task 3 — real `import std.*` resolution (prelude vs import-gated decision)
- [ ] Task 4 — `std.math` (`abs`/`min`/`max`/`pow`/`sqrt`/…), `std.string` (`len`/`upper`/`split`/…)
- [ ] Task 5 — `std.list` ops, hand-rolled `std.json`, seeded `std.random` (all deterministic)
- [ ] Task 6 — examples (file-reading realworld program + per-module guide examples) + docs; coverage audit

## Foundation surface (Task 1)
| File | Change |
|---|---|
| `src/native.rs` (new) | `NativeFn { name, params: Vec<Ty>, ret: Ty, eval, php }`; `registry()`; index lookup |
| `src/chunk.rs` | `Op::Print` → `Op::CallNative(usize, usize)`; `validate` arm (native idx bound) |
| `src/checker.rs` | prelude registers every native's sig from the registry (drop the hard-coded `println`) |
| `src/interpreter.rs` | a call to a native name dispatches to `eval` (drop `builtin_println`) |
| `src/compiler.rs` | a native call lowers to args + `Op::CallNative(idx, argc)`; `stack_effect` arm |
| `src/vm.rs` | `Op::CallNative` pops argc, calls `registry()[idx].eval`, pushes result |
| `src/transpile.rs` | a native call emits its `php(args)` mapping (drop the println special-case) |

Cross-cutting: adding `Op::CallNative` and removing `Op::Print` touches the three coupled `Op` matches
(invariant) — same commit. `println`'s observable behavior must stay byte-identical (existing tests gate it).

## Self-Review
- **No new footgun:** natives are typed (checker sig), so calls type-check; no raw dynamic dispatch.
- **Parity:** one `eval` per native, shared by interpreter + VM — structural byte-identity.
- **Transpile contract (D-L9):** every native ships its PHP mapping or it doesn't land.
- **Determinism gate:** only deterministic natives appear in `examples/` (fixtures for files); clock/env/
  unseeded-random natives (if added) are excluded from the differential glob, documented in KNOWN_ISSUES.
