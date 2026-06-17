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

## Progress
- [ ] Task 1 — `NativeModule` foundation: registry + `Op::CallNative`; migrate `println`; retire `Op::Print`
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
