# Roadmap

This is the forward-looking plan for Phorge. For *delivered* status with commit references see
[`docs/MILESTONES.md`](docs/MILESTONES.md); for the long-term ambition see [VISION.md](VISION.md). The
frozen designs that back each milestone live in `docs/specs/`.

Phorge is pre-1.0 and single-developer. Dates are intentionally omitted — milestones ship when they
ship, and the version number tracks milestone progress, not a release cadence.

---

## ✅ M1 — Tree-walking interpreter + transpiler

The socle. A complete pipeline — lexer → parser → type-checker → tree-walking evaluator — plus a
**Phorge → PHP transpiler** whose output runs under a real PHP and matches the interpreter. Delivered
the core language surface: static types, immutable-by-default bindings, functions, classes with
constructor promotion, single-payload enums with exhaustive `match`, string interpolation, `List<T>`
literals, `for…in`, and checked arithmetic.

## ✅ M2 — Bytecode compiler + stack VM

A second backend: an AST→bytecode compiler and a stack VM (`phg runvm`) that is **byte-identical**
to the interpreter across the full M1 surface, kept in lock-step by the differential test harness. The
VM covers expressions, functions + recursion (clox-style call frames), enums + `match`, and classes +
methods. Heap objects are `Rc`-shared (an `Op::GetLocal` is a refcount bump, not a deep clone),
recovering the VM's speed advantage. The M1 heap is immutable + acyclic, so `Rc`/`Drop` reclaims it
fully — a tracing GC is deliberately deferred to M3, when mutation can create cycles.

## 🔨 M2.5 — Standalone executables (`phg build`)

Compile a program into a single native executable that runs on the VM with no Phorge install. The
program source is embedded in a named object-file section (a versioned, CRC-guarded container); the
binary self-detects and runs it at startup — a third surface on the parity spine.

- **✅ Phase 1 — host build.** `phg build foo.phg` for the host (`x86_64-linux-gnu`): CRC container,
  hand-rolled ELF64 reader, the `main()` self-detect hook, and a built-binary-≡-`runvm` test.
- **🔨 Phase 2 — cross-OS builds.** `--target`/`--all` via `cargo-zigbuild` (zig as the linker). The
  `bundle/` module split + hand-rolled, std-only **PE/COFF**, **Mach-O 64**, and **fat/universal**
  section readers (checked arithmetic, fixture-tested) + a magic-sniffing dispatcher, so a produced
  binary self-reads its own object format. A per-target stub cache keyed on the phg binary's own
  hash (a rebuilt phorge invalidates stale stubs, protecting the parity spine). Targets: Linux
  `x86_64-musl`, `aarch64-{gnu,musl}`, and `x86_64-pc-windows-gnu`. The Mach-O reader ships and is
  tested; macOS *stub production* is deferred to Phase 3.
- **🔲 Phase 3 — distribution & signing.** A CI stub registry (build/sign per-target stubs once per
  release; `phg build` fetches + caches them, so a distributed phorge can cross-build without a
  source checkout); opt-in `--sign` for Windows Authenticode and macOS codesign + notarize via
  `rcodesign` from Linux (no Mac needed).

## 🔨 M3 — Language enrichment

Grow the language beyond the M1 surface. **Landed so far:** developer-experience polish (S0 — `var`
inference, `type` aliases, sharper diagnostics, `phg explain`), core ergonomics (S1 — indexing
`xs[i]`, integer ranges `0..n`/`0..=n`, expression `if`), and **null safety** (S2 — optionals `T?`,
`??`, `?.`, `if (var x = opt)`, checked `opt!`, `match` over `T?`; a non-optional `T` is never null,
enforced at compile time). The **Rich Types (M-RT)** track within M3 has shipped `instanceof` + smart
cast, interfaces, `Map`/`Set`, erased generics (incl. methods and classes), unions `A | B`, and
**intersections `A & B`**; next in that track is **method overloading** (`foo(int)` / `foo(string)` as
distinct signatures — a Phorge-level feature lowered to one dispatching PHP method, since PHP forbids
same-name redeclaration; the TypeScript-over-JavaScript relationship the transpile contract is built
for), then class `extends` and traits. Planned next beyond M-RT, roughly in order: the pipe operator
(`|>`), exceptions (try/catch/throw), and **mutation**. Mutation is the trigger for the **real tracing garbage collector** — once values can
be reassigned and fields mutated, the `Rc` graph can form cycles that refcounting alone would leak, so
M3 is where the mark-sweep collector finally earns its place.

## 🔲 M4–M8 — Ecosystem

The full ecosystem strategy is frozen in `docs/specs/2026-06-15-ecosystem-roadmap-design.md`: two
backends (native VM + optional PHP-transpile) behind clean pluggable traits, with the PHP backend as a
bootstrap-ecosystem lever. ROI-ranked:

- **M4 — Extension API + standard library.** A real stdlib and an extension surface.
- **M5 — Modules + packages.** Real module resolution and git-based package management.
- **M6 — Concurrency + servers.** Uncolored `spawn` + channels (green threads on the VM's reified call
  frames), a native HTTP server, and Postgres connectivity.
- **M7 — Tooling + connectors.** Editor/LSP support, formatters, and ecosystem connectors.
- **M8 — PHP → Phorge migration tool.** A front-end that imports PHP and infers static types — the
  inverse of the M1 transpiler.

Explicitly rejected (with rationale in the spec): live PHP transpilation at runtime, PHP C-extension
FFI, and dynamic `.so` plugins.

## 🔲 v2 — Native & systems

A longer-horizon direction: native AOT compilation, an ownership model that removes the GC, and
sized-integer performance work — pushing Phorge toward the systems-programming end of the spectrum.

---

> The pluggable `Backend` trait is intentionally **not** built yet (`grep 'trait ' src/` returns
> nothing today). The three pipelines are free functions dispatched by a string `match`; the trait is
> deferred to the point where a fourth backend justifies it (the Rule of Three) — `phg build` is
> that fourth backend.
