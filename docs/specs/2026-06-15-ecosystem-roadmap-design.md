# Phorj Ecosystem Strategy & ROI Roadmap — Design

> Strategy for the whole Phorj ecosystem (backends, PHP interop, extensions, stdlib,
> packages, concurrency, server, tooling) and a priority/ROI-ranked milestone+sprint
> roadmap. Builds on the language design (`2026-06-15-phorj-language-design.md`) and the
> M2 VM design (`2026-06-15-m2-bytecode-vm-design.md`). Brainstormed & locked 2026-06-15.

## 1. Strategic reframe — Phorj has two backends; that's the key asset

- **Phorj → PHP transpiler** (M1, done, round-trip-verified): runs anywhere PHP runs;
  transpiled output can use Composer/Symfony/Laravel. **This is the PHP-ecosystem bridge —
  mostly already unlocked.**
- **Native VM** (M2, in progress): the standalone "Go model" single-binary server.

Same source language, two targets: PHP backend = ecosystem/compat; native VM = standalone
server. **Bootstrap lever:** while native infra (stdlib/connectors/server) matures, real
Phorj apps test/deploy via the PHP backend (PHPUnit, Composer, PHP hosting). Native
equivalents flip in per-capability as they land — so the native track is never on the
critical path for real usage.

## 2. PHP interop — what's in, what's rejected

| Idea | Verdict | Why |
|---|---|---|
| Transpile-to-PHP backend (deploy on PHP infra, use Composer) | ✅ keep, first-class | Already built; the ecosystem bridge |
| Native Rust connectors for the VM (HTTP, Postgres, MySQL, Redis) | ✅ build | Clean, no PHP-engine coupling; /stack has the services to test against |
| PHP→Phorj migration tool (typed PHP 8.x subset, **batch/offline**) | ✅ later (M8) | One-way; best-effort with human review; reuses transpiler infra |
| "Rebuild PHP→Phorj on the spot" (live transpile) | ❌ reject | Sound static typing vs dynamic PHP; runtime type failures; undecidable inference over arbitrary PHP |
| Support PHP C extensions via FFI / embed PHP engine | ❌ reject | Drags the entire PHP engine in; shatters the clean break. Use the PHP backend (extensions free there) or native connectors instead |
| Dynamic `.so`/`dylib` plugins | ❌ park (v2+) | Breaks single-binary model; Rust has no stable ABI |

## 3. The extension-system crux (statically-typed ⇒ dual registration)

Unlike PHP's loose `register_function`, every native module or imported symbol in Phorj
must register **both**:
- a **type signature** — consumed by `checker.rs` so calls type-check at compile time;
- an **implementation** — consumed by `interpreter.rs` **and** the VM; plus an optional
  **PHP-emission mapping** for the transpile backend.

This dual+ registration (`NativeModule` trait) is the real design work and the foundation
the stdlib and connectors ride on.

## 4. Locked decisions

| # | Decision | Choice |
|---|---|---|
| E-1 | Backends | Two (native VM + PHP-transpile) behind a clean `Backend` trait; **PHP backend optional/deactivatable** (feature-gated/pluggable). _As-built M2 P3.5: the trait is **planned, not yet present** (`grep 'trait ' src/` = 0) — today the three pipelines (`cmd_run`/`cmd_runvm`/`cmd_transpile`) are free functions dispatched by a string `match` in `main.rs`; the trait lands with the 4th backend (`phg build`) per Rule of Three._ |
| E-2 | PHP ecosystem | PHP backend for eco + native connectors for VM; migration = typed-subset batch. Reject live-transpile, C-ext FFI, dynamic `.so` |
| E-3 | Package distribution | Git-based/decentralized first behind a `PackageSource` trait; central-registry-capable later with no rework |
| E-4 | Ecosystem sequencing | Extension API + stdlib → module resolution → packages → connectors |
| E-5 | Architecture principle | Clean, pattern-based, extensible — pluggable traits throughout: `Backend`, `PackageSource`, `NativeModule`, `Scheduler`, `TestRunner` |
| E-6 | Testing | One Phorj test surface + `TestRunner` trait; PHPUnit-backed now (bootstrap), native `phorj test` later |
| E-7 | First connector | HTTP server + Postgres (modern default; already in /stack) |
| E-8 | Concurrency | **Uncolored Go-style surface** (`spawn` + channels) + pluggable `Scheduler` (OS-threads first, green-threads/coroutines later, zero user-code change). **No async/await coloring** (irreversible — deliberately avoided) |

## 5. ROI-ranked milestone + sprint roadmap

| Milestone | Scope | Sprints | ROI rationale |
|---|---|---|---|
| **M2** *(active)* | Bytecode VM + mark-sweep GC | P1✅ Chunk+Op+VM · P2 compiler+`runvm`+differential · P3 functions · P4 classes/enums+arena · P5 GC collector · P6 strings/full differential sweep | Runtime foundation; native "Go model" path |
| **M2.5** | Single-binary bundling (`phg build`) | embed bytecode in the runtime binary (bun-compile style) | Cheap once the VM works; tangible "ship one file" artifact |
| **M3** | Language enrichment (once, on VM+interp+transpile) | 3a exceptions + null safety/Option · 3b Map/Set/tuples + collection ops · 3c traits + method overloading *(design-risky → prototype on tree-walker first)* · 3d value types + operator overloading + sized ints/`decimal` + `const`/`final` | Makes Phorj a *good* language; **prerequisite for a real stdlib** (needs Map/Set/traits/generics) |
| **M4** | Native extension API + stdlib | 4a `NativeModule` dual+registration (checker sig + native impl + PHP-emit) · 4b stdlib: io/string/math/collections/json/time/fs | Foundational ecosystem piece (E-4); unlocks native modules on **both** backends |
| **M5** | Modules + package manager | 5a real `import a.b.c` resolution (load `.phg`) · 5b git-based pkg mgr behind `PackageSource` (`phorj.toml` + lockfile) | Ecosystem plumbing; reuse/sharing; registry slots in later |
| **M6** | Concurrency + native server + Postgres | 6a `spawn`+channels + OS-thread `Scheduler` · 6b native HTTP server · 6c Postgres connector | Delivers the "Go model" payoff end-to-end: request→query→response on one native binary |
| **M7** | Native test runner + breadth + DX | native `phorj test` (E-6) · MySQL/Redis connectors · HTTP client · `phorj fmt` | DX + ecosystem breadth |
| **M8** | PHP→Phorj migration tool (typed-subset, batch) | reverse transpiler infra + type inference over typed PHP 8.x | Onboard existing PHP codebases (clean subset) |
| **future** | central package registry · LSP/debugger · green-thread scheduler · async (if ever) · NaN-boxing · native-AOT (v2) | — | Lower ROI / deferred |

**Ordering rationale:** the bootstrap lever (PHP backend) keeps native infra (M6/M7) off the
critical path for real usage, so the highest ROI is **language quality (M3) + extension
API/stdlib (M4)** — improving *both* backends — before native servers/packaging that PHP
already provides.

## 6. Open items (decide when reached)

- Concurrency: green-thread scheduler design (M6+), only after OS-thread scheduler ships.
- Package manifest format details (`phorj.toml` schema) — at M5.
- Central registry design — only if/when git-based proves insufficient.
- `NativeModule` PHP-emission mapping ergonomics — at M4.
