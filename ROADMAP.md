# Roadmap

This is the forward-looking plan for Phorj. For *delivered* status with commit references see
[`docs/MILESTONES.md`](docs/MILESTONES.md); for the long-term ambition see [VISION.md](VISION.md).

> **⚠ Forward SSOT = [`docs/plans/MASTER-PLAN.md`](docs/plans/MASTER-PLAN.md)** — the consolidated
> roadmap to 100% (Waves 0–6) is the per-item authority; this file remains the higher-level narrative.
> Design specs were consolidated in the 2026-07-02 pass; dead `docs/specs/…` links resolve to git
> history (≤`60540fc`) + the `docs/research/full-audit/raw/C-decisions.md` register.

Phorj is pre-1.0 and single-developer. Dates are intentionally omitted — milestones ship when they
ship, and the version number tracks milestone progress, not a release cadence.

---

## ✅ M1 — Tree-walking interpreter + transpiler

The socle. A complete pipeline — lexer → parser → type-checker → tree-walking evaluator — plus a
**Phorj → PHP transpiler** whose output runs under a real PHP and matches the interpreter. Delivered
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

Compile a program into a single native executable that runs on the VM with no Phorj install. The
program source is embedded in a named object-file section (a versioned, CRC-guarded container); the
binary self-detects and runs it at startup — a third surface on the parity spine.

- **✅ Phase 1 — host build.** `phg build foo.phg` for the host (`x86_64-linux-gnu`): CRC container,
  hand-rolled ELF64 reader, the `main()` self-detect hook, and a built-binary-≡-`runvm` test.
- **🔨 Phase 2 — cross-OS builds.** `--target`/`--all` via `cargo-zigbuild` (zig as the linker). The
  `bundle/` module split + hand-rolled, std-only **PE/COFF**, **Mach-O 64**, and **fat/universal**
  section readers (checked arithmetic, fixture-tested) + a magic-sniffing dispatcher, so a produced
  binary self-reads its own object format. A per-target stub cache keyed on the phg binary's own
  hash (a rebuilt phorj invalidates stale stubs, protecting the parity spine). Targets: Linux
  `x86_64-musl`, `aarch64-{gnu,musl}`, and `x86_64-pc-windows-gnu`. The Mach-O reader ships and is
  tested; macOS *stub production* is deferred to Phase 3.
- **🔲 Phase 3 — distribution & signing.** A CI stub registry (build/sign per-target stubs once per
  release; `phg build` fetches + caches them, so a distributed phorj can cross-build without a
  source checkout); opt-in `--sign` for Windows Authenticode and macOS codesign + notarize via
  `rcodesign` from Linux (no Mac needed).

## 🔨 M3 — Language enrichment

Grow the language beyond the M1 surface. **Landed so far:** developer-experience polish (S0 — `var`
inference, `type` aliases, sharper diagnostics, `phg explain`), core ergonomics (S1 — indexing
`xs[i]`, integer ranges `0..n`/`0..=n`, expression `if`), and **null safety** (S2 — optionals `T?`,
`??`, `?.`, `if (var x = opt)`, checked `opt!`, `match` over `T?`; a non-optional `T` is never null,
enforced at compile time). The **Rich Types (M-RT)** track within M3 has shipped `instanceof` + smart
cast, interfaces, `Map`/`Set`, erased generics (incl. methods and classes), unions `A | B`, and
**intersections `A & B`**. **Mutation** (immutable-by-default, `mutable` opt-in) and **fault
reporting / stack traces (Slice 1)** have also shipped; there is **no tracing GC** — the heap is `Rc`
+ COW (value types can't cycle), and a cycle collector is deferred to v2 only if mutation ever needs it.

**M-RT remaining sequence (locked 2026-06-22, post roadmap-completeness audit):**
**(1) the totality cluster first** — return-on-all-paths + `never` type + unreachable-after-return +
duplicate-match-arm (the audit's #1 soundness leak: a `-> T` function can fall off the end, leak
`unit`, and detonate with *different* fault messages per backend; front-end-only, no new `Op`); **(2)
method overloading** (`foo(int)` / `foo(string)` lowered to one dispatching PHP method); **(3) `extends`**
(S6, `final`-by-default) + `abstract` + late static binding; **(4) traits** (S8). In parallel,
**generic enums** (`enum Result<T,E>` / `Option<T>`, riding `erase_generics`, no new `Op`) unlock the
error model and a typed `core.json`/`Any`, plus the **pattern cluster** (guards, or-patterns, payload
+ structural destructuring, flow narrowing).

**Error model (decided 2026-06-22) — three tiers, one principle:** enforced typed **`throws E`** (the
PHP-familiar default, fixes PHP's unchecked `@throws`, transpiles to PHP exceptions) · **`Result<T,E>`**
(error-as-value, functional) · **unchecked faults/panics** for programmer bugs (crash with a stack
trace, never declared up the chain — the fix to Java's checked-everything mistake). Both checked tiers
are typed, checker-enforced, and `?`-composable; lands as **M-faults Slice 2**.

## 🔲 M4–M8 — Ecosystem

The full ecosystem strategy is frozen in `docs/specs/2026-06-15-ecosystem-roadmap-design.md`: two
backends (native VM + optional PHP-transpile) behind clean pluggable traits, with the PHP backend as a
bootstrap-ecosystem lever. ROI-ranked:

- **M4 — Extension API + standard library.** A real stdlib and an extension surface.
- **M5 — Modules + packages.** Real module resolution and git-based package management.
- **M6 — Concurrency + servers.** Uncolored `spawn` + channels (green threads on the VM's reified call
  frames), a native HTTP server, and Postgres connectivity.
- **M7 — Tooling + connectors.** Editor/LSP support, formatters, and ecosystem connectors.
- **M8 — PHP → Phorj migration tool.** A front-end that imports PHP and infers static types — the
  inverse of the M1 transpiler.

Explicitly rejected (with rationale in the spec): live PHP transpilation at runtime, PHP C-extension
FFI, and dynamic `.so` plugins.

## 📋 Roadmap-completeness audit (2026-06-22) — the gap SSOT

A one-shot 20-track (A–S + V) multi-agent review enumerated **every** gap vs PHP 8.0–8.4 parity,
beyond-PHP "upgrade" capability, DX/tooling, correctness, security, stdlib, numerics, i18n, testing,
perf, build, observability, docs, and governance — so gaps stop being found ad hoc. **555 candidates →
290 adopt · 187 defer · 81 reject.** The deduplicated master triage table, per-milestone rollup,
top-10 spine, reject-list-with-reasons, and 10 cross-track themes are the SSOT in
**`docs/specs/2026-06-21-php-parity-and-beyond.md`** (raw per-track reports under
`docs/research/roadmap-completeness/`).

**Top-10 spine (the immediate priority order):** (1) totality cluster · (2) generic enums →
`Result`/`Option` + `?` · (3) the three-tier error model (Slice 2) · (4) overloading → `extends` →
traits · (5) pattern cluster (guards, destructuring, flow narrowing) · (6) `decimal`/money ·
(7) collection breadth behind a written stdlib charter · (8) `phg fmt` + lexer-ergonomics (numeric
separators, `0x`/`0b`/`0o`) + unused-import/local lints · (9) `core.json` encode + safe parse + console
I/O · (10) the GA governance doc-bundle (semver/BC/conformance-corpus/security-model).

**New milestones created to hold the breadth** (skeletons — scope in the spec's §3 rollup):

- **M4 — stdlib charter** (naming, subject-first arg-order, optional-vs-fault discipline, determinism
  tiers, native-vs-`.phg` policy) — *precedes* the M11 breadth push. **Adopted** (2026-06-29):
  `docs/specs/2026-06-29-m4-stdlib-charter.md`.
- **M-NUM** — numerics & business data: typed **`decimal`/money** (float-for-currency becomes a compile
  error) + rounding modes, numeric parse, `intdiv`/conversions, `int`=i64 pinning, float predicates,
  numeric literals (hex/bin/octal, exponent). Defers `BigInt`, `Money`+currency to M-NUM-2.
- **M-TIME** — immutable timezone-mandatory `DateTime`/`Instant`/`Duration`/civil `Date`. IANA tz +
  `now()` clock (quarantined) defer to M-TIME-2 / M6.
- **M-text** — i18n core: codepoint-aware length/iteration, UTF-8 `string` contract, `core.regex`
  (PCRE `/u`), `\u{…}` escapes, non-locale `number_format`, ASCII case-insensitive compare. ICU-locale
  features defer to a tier-3 extension policy.
- **M-Test** — first-class `phg test` runner + `Core.Test` assertions + the seedable-Random /
  injectable-Time determinism seam + fixtures/selection/skip/`assertFaults` + PHPUnit bridge. Defers
  property-based / snapshot / coverage.
- **M-perf** — VM optimization behind a CI perf-regression gate: `Rc`-share `Value::Str`, intern
  `IsInstance`, faster dispatch, const-fold, peephole, lazy `for`-range. Defers superinstructions /
  inline caches.
- **M-Batteries** — impure stdlib (env/args/dir/file-breadth/csv/random/uuid) on the M6
  quarantine seam (excluded from `differential.rs`); folds into M11 if the charter's quarantine tier is
  crisp.
- **M8.5 — interop** — declaration files (`.d.phg`) for untyped PHP/Composer deps + call-a-Composer-lib;
  defers mixed `.php`+`.phg` projects, raw-PHP escape hatch.
- **M13 — governance evolution (post-1.0)** — Rust-style **editions** mechanism (policy stated at GA,
  built post-1.0), RFC process, per-API stability tiers.

**Where audit items slot into the existing GA milestones (M8–M12):** interop/migration + security
(injection guards, `Secret<T>`, header-injection, crypto digests) → **M8**; perf-regression gate, fuzz
harness, release-profile, unused-import/local lints, `explain` coverage, `--php-target` floor → **M9**;
**generic enums + the `id(7)+1` generic-result-operand fix** → **M10**; stdlib breadth (collections,
`core.json`, `core.regex`, `sprintf`, hash/encoding/path/url/log) + iterators + API doc-gen → **M11**;
language reference, the tour, migration guide, transpile-contract doc, `phg fmt`/repl/LSP/playground,
release automation, audit command, and the GA governance doc-bundle → **M12 / M7**.

**Namespace PascalCase reshape (tracked here — the audit missed it):** `package Main`, `E-PKG-CASE`
(PascalCase package/folder segments, **enforced including vendored deps**; PHP/Composer deps are
case-mapped to PSR-4 at the importer boundary, never by relaxing the rule), manifest `name → module`,
and lifting `E-PKG-TYPE`. A milestone-scale **breaking codemod** — **✅ SHIPPED** (`E-PKG-CASE`
enforced incl. vendor; `package Main` reshape complete); design in git history (≤`60540fc`).

## 🔲 v2 — Native & systems

A longer-horizon direction: native AOT compilation, an ownership model that removes the GC, and
sized-integer performance work — pushing Phorj toward the systems-programming end of the spectrum.

---

> The pluggable `Backend` trait is intentionally **not** built yet (`grep 'trait ' src/` returns
> nothing today). The three pipelines are free functions dispatched by a string `match`; the trait is
> deferred to the point where a fourth backend justifies it (the Rule of Three) — `phg build` is
> that fourth backend.
