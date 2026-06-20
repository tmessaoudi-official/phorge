# CLAUDE.md — phorge

Phorge is a statically-typed, PHP-inspired language implemented in Rust (edition 2021, std-only,
no external crates): lexer → parser → type-checker → tree-walking interpreter + Phorge→PHP
transpiler (M1) + bytecode compiler + stack VM (M2). Single developer, commits direct to `master`,
remote is GitHub (`tmessaoudi-official/phorge`).

This sub-project lives under `/stack/projects/` and is handled with the global reasoning framework
(`~/.claude/CLAUDE.md`). It is NOT `/stack` infrastructure — do not route work here to
`global-stack-lead-dev`. The parent `/stack/CLAUDE.md` is excluded via
`/stack/projects/.claude/settings.json` `claudeMdExcludes`.

## Git autonomy (overrides global Rule 10 — authorized by the developer, 2026-06-16)

Autonomous `git add` and `git commit` are **authorized** in this project: stage and commit ready
work without asking, when tests pass (`cargo test`) and the quality gate is clean
(`cargo clippy --all-targets`, `cargo fmt --check`). This mirrors the `/stack` auto-commit
precedent and overrides global Rule 10 **for this project only**.

Scope and limits:
- **Authorized:** `git add`, `git commit` (descriptive messages — `feat:`/`fix:`/`docs:`/`test:`
  prefixes, matching existing history; no `Co-Authored-By` line).
- **NOT authorized without an explicit request:** `git push` (and any force-push — `push --force`
  remains denied globally).
- Commit only green, self-contained changes. Do not commit a broken build or red tests.
- If the safety classifier blocks a specific `git commit`, present the exact command for manual
  execution rather than retrying — do not attempt to bypass it.

## Toolchain & gate

`export PATH=/stack/tools/cargo/bin:$PATH`. Baseline: ~453 tests green, clippy clean (pedantic off).
The differential harness (`tests/differential.rs`) is the correctness spine — `run`, `runvm`, **and
(since M7) the transpiled PHP** must stay byte-identical. The M7 **PHP oracle** there transpiles every
example/project, runs it under a real `php`, and asserts stdout matches the interpreter; run the full
gate with `PHORGE_REQUIRE_PHP=1` so a missing `php` **fails** (not skips). `PHORGE_PHP=<path>`
overrides the binary. Adding an `Op` variant requires extending three exhaustive matches in
the same commit: `src/vm.rs` `exec_op`, `src/chunk.rs` `BytecodeProgram::validate`, and
`src/compiler.rs` `stack_effect`. `phg bench <file>` measures the two backends (median-of-N,
output-identity gated) — run it for a before/after number before any perf change.

**Examples ship with features** (developer rule, 2026-06-17): every shipped feature lands with a
runnable example under `examples/` (auto-gated by the `tests/differential.rs` glob, so it must run
byte-identically on both backends) and an `examples/README.md` entry (index + coverage matrix), in
the **same change** as the feature. CLI/tooling features that aren't a single program (e.g.
`phg build`, `explain`) get a walkthrough README + a small companion `.phg` (see `examples/build/`,
`examples/cli/`). Faults can't be a runnable example (every example must produce identical *Ok*
output) — capture them in a README instead.

## Active plan

The M2 P3.5 hardening roadmap (`docs/plans/2026-06-16-m2-p3.5-hardening-roadmap.md`, Waves 0–4) is
**complete**. **M2 P4 is COMPLETE** (`docs/plans/2026-06-16-m2-p4-classes-enums-match.md`): P4a
(enums + `match`), P4b (classes + constructor promotion + field reads), and P4c (methods + `this`)
all landed — **`runvm` now covers the full M1 language surface** and `examples/grades.phg` runs
byte-identically on both backends (VM ≈3.2×). The VM object model is value-native (reuses the shared
`Value::Enum`/`Instance`). **M2 Wave 4 is COMPLETE**
(`docs/plans/2026-06-16-m2-wave4-compiler-types.md`): the compiler's operand-type inference is now
class-aware (`enum CTy { Int, Float, Class(String), Other }` + a recursive `ctype(&Expr)` resolver),
so a field read on an arbitrary instance (`p.x + 1`), a method-call result (`c.get() + 1`), a nested
`a.inner.x`, and a class-typed enum payload all compile and run byte-identically — closing the last
known `run`↔`runvm` parity gaps. (M3 S1.1 later extended `CTy` with a `List(elem)` variant so a
list-element read `xs[i]` resolves as an arithmetic operand too — indexing is now part of the surface,
no longer rejected.)

**M2 P5a is COMPLETE** (`docs/specs/2026-06-16-m2-p5-object-model-design.md`,
`docs/plans/2026-06-16-m2-p5a-rc-shared-heap.md`): heap objects are now **`Rc`-shared**
(`Value::Instance`/`Enum`/`List`), so the `Op::GetLocal` hot path is a refcount bump, not a deep
clone — object-heavy VM run **1537 ms → 634 ms (2.4×)**, recovering the VM's advantage to 9.35× (≈
scalar's 10.92×). **There is no tracing GC and none is planned for M2:** the M1 heap is immutable +
acyclic, so `Rc`/`Drop` reclaims completely (a tracing collector is deferred to **M3**, when
mutation could create cycles). **Phase B** (slot-indexed `Vec` field layout, replacing the
per-instance `HashMap`) is **bench-gated and unopened** — after P5a the object path is within ~15% of
scalar's advantage, so field access no longer dominates; the slab-arena was rejected (no locality
evidence).

**M2 is now formally CLOSED** (`docs/MILESTONES.md`, `33c6b78`): all design §10 success criteria met
(backends byte-identical, quality gate green; the mark-sweep GC criterion was revised — `Rc`/`Drop`
reclaims the immutable+acyclic heap fully, tracing GC deferred to M3). A **full-coverage example set**
also landed (`docs/specs/2026-06-16-examples-coverage-design.md`): four real-world programs
(`examples/realworld/`), six focused guide programs (`examples/guide/`), and the Phorge→PHP transpile
bridge (`examples/transpile/`) — `tests/differential.rs` now **globs `examples/**/*.phg`** so every
example (and any added later) is byte-identity-gated automatically; `examples/README.md` is the
living surface showcase. **Gotcha:** zero-payload enum variants need call form `V()` both to
construct AND in a `match` pattern (bare `V =>` is a silent catch-all binding).

**M2.5 `phg build` (standalone executables) — Phases 1 & 2 COMPLETE** (released as **v0.4.0**).
Phase 1 (host `x86_64-linux-gnu`): `phg build foo.phg` embeds the program **source** in a `.phorge`
section (versioned CRC-guarded container + hand-rolled ELF64 reader); `main()` self-detects + runs it
on the VM. Phase 2 (`docs/plans/2026-06-16-m2.5-phase2-cross-os.md`): `src/bundle.rs` split into a
`bundle/` module — `container`, per-format readers `elf`/`pe`/`macho` (thin + fat), a magic-sniffing
`section::find_section`, and a `cross` orchestrator — plus `phg build --target/--all` cross-compiling
stubs via **cargo-zigbuild** (zig linker) for Linux `x86_64-musl`/`aarch64-{gnu,musl}` +
`x86_64-pc-windows-gnu`, cached under an **FNV-1a-64 of the phg binary's bytes**. All readers honor
EV-7 (checked arithmetic, `None` on bad input). macOS reader ships + fixture-tested; apple `--target`
is **rejected** (Mac stub deferred to Phase 3). `tests/build.rs` gates cross-parity (musl native exec +
real windows-PE round-trip). **Hard-won gotcha (verified):** `llvm-objcopy --add-section` on **PE**
needs `--set-section-flags …=noload,readonly` or it writes a zero-data section — applied unconditionally
for ELF + PE (a prior "skip on PE" attempt was the bug; only the real-binary windows test caught it).

**CLI UX (v0.4.0):** global `-v`/`--version`, `-h`/`--help`; run-family source forms `<file>` | `-`
(stdin) | `-e`/`--eval <code>` (inline) | `--` (literal path). `cli::resolve_source` is the pure,
tested resolver; built binaries still ignore argv (run their embedded program).

**Profiling & introspection (v0.4.0):** `phg bench` now reports **memory** (cold-execution
peak-RSS growth + process `VmHWM`/`VmRSS`) beside its timing, via a std-only **Linux** `/proc`
sampler (`src/mem.rs` — `/proc/self/status` + `clear_refs`=5 peak reset; non-Linux prints
"unavailable"). Per-phase/sequential per-backend RSS is *deliberately not* reported — it reads ~0
after the 101× timing loop warms the allocator (glibc rarely returns freed pages). `phg disasm
<source>` dumps the compiled bytecode (per-function listings via `Op` `Debug` + a `_`-fall-through
annotator, so no second match surface to drift; plus enum/class/method descriptor tables).
Showcase: `examples/bench/workload.phg` (+ its README), auto byte-identity-gated like every example.

**Docs:** a full OSS doc set landed at v0.4.0 (README rewrite, dual **MIT OR Apache-2.0**, CONTRIBUTING,
CODE_OF_CONDUCT, SECURITY, ROADMAP, VISION, FEATURES, KNOWN_ISSUES, THIRD-PARTY-NOTICES, CITATION.cff,
`.github/` templates). See **`ROADMAP.md`** / **`VISION.md`** for the forward plan.

**M3 is now the active milestone** (`docs/specs/2026-06-17-m3-language-roadmap-design.md` +
`docs/specs/2026-06-17-m3-slice1-s0-s1-s2-design.md`). The transpile contract is **Phorge : PHP ::
TypeScript : JavaScript** — every feature maps to idiomatic PHP; PHP-absent features (generics) are
compile-time-only and erased. **Slice S0 (developer experience) is COMPLETE**
(`docs/plans/2026-06-17-m3-s0-dx.md`): per-command `--help` with worked examples; `var` local type
inference (`Type::Infer`, resolved in the checker; the VM derives the local's operand `CTy` from the
initializer so arithmetic still specializes); `type` aliases (`Item::TypeAlias`, resolved +
cycle/duplicate/built-in-shadow-checked in the checker, then expanded out of the AST by
`checker::expand_aliases` so the interpreter/VM/transpiler — and the PHP output — are alias-free);
sharper diagnostics (caret-underlined span + did-you-mean hints + stable codes, `Diagnostic`
construction centralized through `Diagnostic::new`, front-end-only so runtime parity is untouched); and
`phg explain <CODE>`. **Slice S1 (core ergonomics) is COMPLETE**
(`docs/plans/2026-06-17-m3-s1-ergonomics.md`): list indexing `xs[i]` (un-rejected in both backends —
the checker already typed it — reusing the bounds-checked `Op::Index`; OOB → byte-identical
`"list index out of range"` fault, classified `FaultKind::IndexOob` in the differential harness);
integer ranges `a..b`/`a..=b` (the one new `Op::MakeRange(bool)`, extending the three coupled matches;
both backends materialize a `List<int>` via native Rust ranges, so `for (int i in 0..n)` works
unchanged; transpiles to PHP `range()`); and expression `if` (`if (c) { e } else { e }` in value
position — parens + mandatory `else`, single-expression arms; lowers via the existing branch ops like
`&&`/`||`, transpiles to a PHP ternary). All three are byte-identical on `run`/`runvm` **and**
round-tripped through real PHP; `examples/guide/ergonomics.phg` showcases them. **Slice S2
(null-safety) is COMPLETE** (`docs/plans/2026-06-17-m3-s2-null-safety.md`): optionals `T?`
(`Ty::Optional` + `Value::Null`) with a compile-time non-null guarantee (a non-optional `T` is never
null — TypeScript `strictNullChecks` over PHP's nullable runtime); `??` null-coalesce; `?.` safe
access (PHP `?->`); `if (var x = opt)` if-let binding + smart-cast (S1.4 landed here); `opt!` checked
force-unwrap (clean `force-unwrap of null` fault, `FaultKind::ForceUnwrap` parity) with the
**`W-FORCE-UNWRAP`** lint; and `match` over `T?` with null-arm narrowing. Two cross-cutting additions:
the **warning channel** (first non-fatal lint — `check()` returns `Ok(warnings)`, rendered to stderr,
never gating the build) and the generalization of `Op::MatchFail` → **`Op::Fault(FaultMsg)`** (so S2
adds **no new `Op` variant**). All byte-identical on `run`/`runvm` + round-tripped through real PHP;
`examples/guide/null-safety.phg` showcases the suite. **Gotcha (fixed this slice):** `??`/`?.`/`opt!`
stash their receiver in a scratch slot that must be `self.height - 1` (the receiver's frame slot), not
`add_local()`'s `locals.len()-1` — two such ops in one expression (e.g. `"{a ?? -1} {b ?? -1}"`) put a
live transient below the receiver and the old slot was off, a silent `run`↔`runvm` break.

**Post-S2 direction (designed 2026-06-18, `docs/specs/2026-06-18-m3-next-intuitive-features-and-io-design.md`):**
developer asked for more intuitive features + exhaustive examples (file/URL/imports) + a Phorge-vs-PHP
benchmark. Locked: **build order D→B→A**; **URL/network deferred to M6** (Rust std has no HTTP client →
breaks zero-dep, *and* network is non-deterministic → breaks the byte-identical spine; determinism, not
the dependency, gates examples); **rich std-only stdlib now**; multiple inheritance = traits/mixins at
S5 (rejected as MI, D-L3). **Track D DONE** — `phg bench --vs-php` (3-way interpreter/VM/PHP, VM ≈3.2×
faster than a debug PHP 8.6 on the workload).

**NAMESPACE RESHAPE (designed 2026-06-18, `docs/specs/2026-06-18-m3-namespace-system-design.md`):** the
developer chose **everything namespaced, "nothing in the wind", as the default** — no free-floating
globals. Locked: **Go-style module-qualified** calls (the Java `System.out.println` object-path was
rejected — no idiomatic PHP target, breaks D-L9); **reserved `core.` root** for the stdlib; jargon-free
leaf modules **`console`** (was io) + **`file`** (was fs) + `math`/`text`/`list`/`json`/`time`; **`println`
→ `Console.println` after `import Core.Console;`** (the bare global is RETIRED); **leaf-qualified call
sites** (root in the import, leaf at the call — Go's `import "fmt"` → `fmt.Println`); **explicit import
required** even for stdlib; **user code mandatorily packaged** (stricter than PHP/TS by choice, emits real
PHP `namespace`s — leaning explicit `package a.b;` + strict folder=path, final syntax deferred). Native
registry is keyed by `(module, name)`; `import Core.Console` becomes load-bearing.

**Track B Wave 1 — namespaced native foundation — COMPLETE** (reshaped Task 1) — plan
`docs/plans/2026-06-18-trackB-stdlib-io-imports.md`, design spec above. Landed: `src/native.rs`
registry keyed by `(module,name)` (`OnceLock`, pinned `const CONSOLE_PRINTLN=0`, single-sourcing each
native's checker sig + shared `eval: fn(&[Value], &mut String)->Result<Value,String>` + `php` mapping);
`Op::Print`→`Op::CallNative(idx,argc)` (3 coupled matches + a `validate` native-index bound; pushes the
result, no `emit_const(Unit)`); import-driven resolution in all four backends (interpreter+compiler
resolve a `Member`-call's head qualifier **locals-first then `index_of_by_leaf`**; checker+transpiler use
the import map — only they lack scope tracking); the **global `println` is RETIRED** (bare `println` →
unknown fn) and `Console.println` requires `import Core.Console;`; an **`E-SHADOW-IMPORT`** guard keeps a
value binding from shadowing an imported qualifier (else transpiler vs run-backends diverge); the example
differential test now also asserts each example **runs** (`Ok`), not just that backends agree. Full
migration done (all `.phg`, fixtures, ~189 inline test programs via `tools/wave1_migrate.py`). 367 tests
green, clippy + fmt clean, real-PHP round-tripped.

**Track B Wave 2 — stdlib breadth — COMPLETE (buildable subset).** Three modules landed as `(module,
name)` registry entries (shared `eval` + PHP erasure), each with a byte-identity-gated guide example
round-tripped through real PHP: **`Core.Math`** (`548e1d0` — `sqrt`/`pow`/`floor`/`ceil` float,
`abs`/`min`/`max` int), **`Core.Text`** (`026094c` — `len`/`upper`/`lower`/`trim`/`contains`/`split`/
`join`/`replace`; `split`↔`join` carry `List<string>`), **`Core.File`** (`read`→`string?`/`exists`/
`write`; reads a committed fixture for determinism, composes with S2 `??`/if-let). The four-backend
call path was already fully generic (multi-arg, typed, value-returning), so each module was purely
additive — no plumbing changes. **`core.list` and `core.json` are DEFERRED**: `core.list` needs S3
lambdas (`map`/`filter`/`reduce`) or `List<T>` generics (`reverse`/`sum`/…), and `core.json` needs a
dynamic `Json`/`Any` type (`Ty` has no type variable) — both land once generics or S3 exist.
**Gotcha (this wave):** a guide example importing `Core.Text`/`Core.File` must not name a local `text`/
`file`/`console` — that trips `E-SHADOW-IMPORT` (the Wave 1 guard). And irrational floats (`sqrt(2.0)`)
diverge between the Rust backends (full round-trip) and PHP's 14-digit `echo` — examples keep to
exactly-representable values (now in KNOWN_ISSUES; the run↔runvm spine is always identical).

**Track B Wave 3 (user packages) was promoted to a full milestone — M5 is now ACTIVE.** The developer
chose to build the complete Go-shaped, `src/`-rooted, mandatory-packaged, strict-folder=path project
model (design `docs/specs/2026-06-18-m5-project-model-design.md`, plan
`docs/plans/2026-06-18-m5-modules-packages.md`). Decisions: **mandatory `package` everywhere, never
inferred** (even `-e`/stdin); reserved **`package main;`** = runnable entry (Go model); `core` reserved;
**single-file brace-namespace PHP emission** (no Composer/autoloader — chosen because PSR-4 can't
autoload free functions, and Phorge is function-heavy); project detection = `phorge.toml` walk-up;
git-based deps pinned + vendored for determinism. **M5 S1 COMPLETE** (single-file `package` decl + parse
+ checker `E-NO-PACKAGE`/`E-RESERVED-PACKAGE` + flat PHP unchanged → byte-identical; all 24 examples +
every test program migrated to `package main;`; also fixed Wave-1 `README.md` drift). **M5 S2a COMPLETE**
(`src/manifest.rs`: std-only `phorge.toml` parser → `Manifest`/`Dependency`/`Pin` + `Project::detect`
walk-up + source-root + PSR-4 `namespace_root()`; 18 unit tests; byte-safe — unconsumed, no backend
touched). **Manifest = Composer's *vocabulary* in an honest TOML container** (developer-chosen): `name =
"vendor/package"`, `[require]`/`[require-dev]`, deps `{ git, tag|rev }` or `"url@tag"` shorthand,
exact-pin only (no `^`/`~` ranges — lockfile pins exact). Literal `composer.json` was **rejected** — the
`composer` tool can't process it (no Packagist/autoloader Phorge uses), so the filename is a false
promise; familiarity is vocabulary, not the tool. **M5 S2b COMPLETE** (`src/loader.rs`: `load`/
`load_loose_src` → `Unit{program,diag_src}`; project-mode walk-up + parse every `.phg` under source
root + folder=path `E-PKG-PATH` (directory=package, `main` exempt) + flat AST merge; loose-mode
`main`-only). Enforcement is **path-aware in the loader, never in `check()`** → `cmd_run(&str)` +
differential untouched. `main.rs` routes `<file>` run/runvm/check/transpile through the loader via new
`cli::{run,runvm,check,transpile}_program`; `-e`/stdin/parse/lex/disasm/bench/build stay on the string
path. **M5 S2c COMPLETE** — qualified cross-package calls (`import acme.util;` → `util.compute(x)`) +
namespaced PHP + import aliasing, via a **loader-side resolution + name-mangling pass** (chosen over
backend-aware resolution): the loader mangles every non-`main` def to a global PHP-FQN key
(`acme.util`+`compute` ⇒ `Acme\Util\compute`; `main` stays bare), rewrites same-package bare + qualified
user calls to bare mangled calls (`core.*` natives untouched), then flat-merges. Backends consume the
rewritten AST **unchanged** ⇒ run==runvm structural; only the transpiler de-mangles into
`namespace Acme\Util {}` brace-blocks + `\Main\main()` bootstrap (single-package programs have no `\`
names ⇒ flat path, byte-identical to pre-S2c). Aliasing: `import a.b as c;` (`Item::Import.alias`,
contextual `as`). **Scope: library packages export functions only** (`E-PKG-TYPE` rejects non-`main`
types — cross-package types are a follow-up); the S2b bare cross-package interim is tightened
(unqualified now fails on both backends). Verified `42` on run/runvm/**real PHP 8.6**. 409 tests green.
**M5 S2d COMPLETE** — first public multi-file project (`examples/project/tempconv/`, a two-package
C→F converter) showcasing mandatory packages + folder=path, a cross-package qualified call, import
aliasing (`as`), a same-package bare call across files, and namespaced PHP; runs `freezing = 32F` /
`boiling = 212F` byte-identically on run/runvm/**real PHP 8.6** (exact integer math, so PHP's float `/`
agrees). `tests/differential.rs` is now **project-aware**: it discovers every project root (a dir with
`phorge.toml`) under `examples/`, loads via `loader::load`, and gates `run` ≡ `runvm`; the single-file
glob skips any dir holding a `phorge.toml` (structural exclusion). 410 tests green.

**M5 S3 COMPLETE — M5 is now CLOSED** (`docs/plans/2026-06-18-m5-modules-packages.md`): git dependencies
+ `phorge.lock` + `phg vendor` + auto-offline. `src/lock.rs` is a strict TOML-subset lockfile
(`[[package]]` → `name`/`git`/`rev`/`hash`, round-tripping). `src/vendor.rs` (`phg vendor`) is the
**only network-touching command**: clone → checkout the pinned tag/rev → copy the dep's source into
`vendor/<vendor>/<package>/` (its own mini source-root, so folder=path validates per-dep) → FNV-1a-64
content hash (reuses `bundle::cross::fnv1a_64`; the resolved 40-hex commit SHA is the real pin) → write
`phorge.lock`; idempotent + crash-safe (stage in a temp dir, atomic swap, touch only each dep's owned
subtree). `loader::load_project` merges vendored packages **exactly like first-party library packages**
(mangle + resolve *before* any backend ⇒ run≡runvm structural; the transpiler de-mangles to
`namespace Acme\Strutil { … }`), and is **offline-only** — vendor is consulted only when `[require]` is
non-empty and `run`/`check`/`transpile` **never fetch** (`E-VENDOR-MISSING` when a required dep isn't
vendored). New guards: **`E-VENDOR-MAIN`** (a vendored `package main` would collide with the entry) and
**`E-DUP-DEF`** (duplicate `(package,name)` after the flat merge — previously a silent `HashMap`
overwrite since S2c). Example `examples/project/withdeps/` (consumes a vendored `acme/strutil`) ships its
committed `vendor/` + `phorge.lock`; the project-aware harness loads it offline → byte-identical on
run/runvm + **real PHP 8.6**. `tests/vendor.rs` drives the real git path against a **`file://` local-git
fixture** (offline, deterministic): fetch+lock+load, idempotent re-vendor, `E-VENDOR-MISSING`.
**Gotcha:** the example's `[require]` git URL is a documented public-style placeholder; the dep's source
is committed under `vendor/` (Go's vendoring model), so the example runs with zero network — `rev`/`hash`
in `phorge.lock` are the real values for the committed source. **Deferred (KNOWN_ISSUES, not regressions):**
transitive deps (a dep's own `[require]`); `phg build` stays single-file (won't merge `vendor/`).
421 tests green.

**M6 WEB CAPABILITIES — design-locked + spike in progress** (research
`docs/plans/2026-06-18-m6-web-capabilities-research.md`, design `docs/specs/2026-06-18-m6-web-design.md`).
4 parallel research agents (raw in `docs/research/m6/raw/`) + a 30/8 3C gate converged on: **the portable
unit is `handle(Request) -> Response` at the VALUE level** (PSR-7/15 — the socket/superglobal bridge is
runtime glue, NOT transpiled 1:1; only `handle` round-trips); **Shape A** (pure-Phorge `Request`/`Response`
classes) is the ONE public API ("do both?" resolved to one-API/evolving-engine — a native header map is a
later invisible optimization, not a 2nd API); **single-threaded is FORCED** by the `Rc`-shared heap (`Value`
isn't `Send`), real concurrency = M6 green-threads under an unchanged contract; socket quarantined in a future
`src/serve.rs` behind a `Transport` trait, tested outside `differential.rs`. Build order (tasks #16–#20):
**W0 bytes → W1 handler(Shape A) → W2 static router → W3 `src/serve.rs`+Transport → W4 `phg serve` CLI +
PHP front-controller + docs**. **M6 W0 COMPLETE** (`446bcb9`, `docs/specs/2026-06-18-m6-w0-bytes-design.md`):
`bytes` primitive + `b"…"` literals (`\xHH`) + `Core.Bytes` interop (`from_string`/`to_string`→`string?`/`len`
byte-count/`concat`/`slice` clamped) — **no new `Op`** (literal via `Op::Const`, interop via `Op::CallNative`,
`==` via `Op::Eq`); erases to PHP `string`; `examples/guide/bytes.phg` byte-identical on run/runvm/**real PHP**.
**M6 W1 COMPLETE** (`docs/specs/2026-06-18-m6-w1-handler-design.md`): the portable `handle(Request) ->
Response` model in **pure Phorge** — `Request`/`Response` classes + `parse_request(bytes) -> Request?` +
`serialize_response(Response) -> bytes`, bodies are `bytes`, headers as `List<string>` raw lines with a
`req.header(name)` linear-scan accessor (the method-call API is the one public surface; typed `Header`
deferred to S3). Two new natives (**no new `Op`** — both `Op::CallNative`): `bytes.find(bytes,bytes) ->
int?` (CRLFCRLF boundary; `find(h,b"")`=0 per PHP `strpos`) and `text.split_once(string,string) ->
List<string>` (robust `Name: value`; → PHP `explode($sep,$s,2)`). `examples/web/handler.phg` byte-identical
run/runvm/**real PHP**. **Two transpile gotchas found + in KNOWN_ISSUES:** (1) `package main` fns become
*global* PHP fns → a name like `serialize` collides with a PHP builtin (renamed `serialize_response`);
(2) PHP enforces `private` but the Phorge backends don't → externally-read promoted fields must be
**`public`** (or use an accessor). **W2 (static exact-match router) is next.** Also this session: **the
CLI binary was renamed `phorge` → `phg`** (`70ea75d`; package/lib/`PHORGE_*`/`.phorge` section/`phorge.toml`
stay `phorge` — ripgrep model). See [[binary-renamed-to-phg]].

**M3 S3 Track A — lambdas + first-class functions + pipe `|>` — COMPLETE**
(`docs/specs/2026-06-18-m3-s3-lambdas-pipe-design.md`, plan `docs/plans/2026-06-18-m3-s3-lambdas-pipe.md`;
subagent-driven, 8 tasks T1–T8 + a first-class-fn parity fix). Landed: `Ty::Function`/`Type::Function`,
`Expr::Lambda` + `LambdaBody::{Expr,Block}`, `ast::free_vars`, `Value::Closure`, `CTy::Fn`, and **two new
VM ops** `Op::MakeClosure`/`Op::CallValue` (extend the three coupled matches). **Expression-body**
`fn(int x) => e` (return inferred) and **statement-body** `fn(int x) -> int { … }` (explicit `-> T`
required; `E-LAMBDA-THIS` rejects a lambda that touches `this`); capture enclosing locals **by value**
(immutable+acyclic heap ⇒ no GC). **First-class function values**: a bare named fn is a value
(`twice(3, dbl)`) — on the VM a zero-capture `MakeClosure`, in PHP a first-class callable `dbl(...)`.
**Pipe `|>`** is `x |> f ≡ f(x)`, left-assoc, **lowered to a `Call` in the parser** (no new Op; the four
dead `BinaryOp::Pipe` stubs retired to `unreachable!`). Transpile targets: arrow fn / `function(){}use()`
/ first-class callable / `(fn…)(args)` for a lambda-literal call target. Byte-identical run≡runvm +
real-PHP; `examples/guide/lambdas-pipe.phg`. **Gotcha (caught by the example, fixed):** a named-fn ref as
a *value* and a lambda *literal* in call position each diverged on one backend until fixed — the VM now
compiles a named-fn ref to `MakeClosure` and the `stack_effect` `MakeClosure` discriminator range-checks
this function's lambda indices (so a forward-referenced named fn doesn't panic); the transpiler emits
`(<lambda>)(args)`. Deferrals (KNOWN_ISSUES): this-capture, cross-package fn *values*, block-body return
inference, function-type variance, `core.list` map/filter/reduce. **Deferred next:** the rest of Track A's
sugar where applicable + `core.list`/`core.json` (need `List<T>`-generic natives) and the M6 router's
middleware/closure-route layer. **Parked:** M2.5 Phase 3 (CI stub registry + `--sign`)
— `docs/specs/2026-06-17-m2.5-phase3a-stub-registry-design.md`.

**M-RT (Rich Types) is the active milestone** (approved plan
`docs/plans/2026-06-20-m-rt-rich-types.plan.md`): a TypeScript-grade type system mapped to PHP
8.0/8.1 — slices **S1 `instanceof`** → S2 interfaces+`implements` → S3 Map/Set → S4 unions `A|B` →
S5 intersections `A&B` → S6 `extends` (`final`-by-default) → S7 erased generics `<T>` → S8 traits.
Each slice ships independently green + byte-identical (`run≡runvm≡real PHP`) with a guide example.
**S1 COMPLETE:** real `instanceof` type test (`value instanceof ClassName` → `bool`, smart-cast
narrowing in `if`, transpiles to PHP `instanceof`) replacing the retired value-equality `is` stub;
one new `Op::IsInstance(String)` (carries the name inline, no pool entry); `is` is no longer a
keyword. `examples/guide/instanceof.phg`. **S2 COMPLETE:** interfaces + `implements`/`extends`
(`Item::Interface`, `ClassDecl.implements`) — a class that implements an interface is a **nominal
subtype** (instances flow into interface-typed slots; polymorphic calls through an interface type),
and `instanceof` now accepts an interface RHS (with smart-cast). **No new `Op`** (the table powers
S1's `Op::IsInstance`): a single shared `ast::class_implements(program)` (transitively flattened,
sorted, cycle-safe) is computed once and consumed verbatim by checker + interpreter + VM
(`BytecodeProgram.class_implements`); subtyping threads through `Ty::assignable_with(_, _, &oracle)`
(old `Ty::assignable` = no-subtype delegate). Transpiles to PHP `interface`/`implements`/`extends`,
byte-identical run≡runvm≡real PHP; `examples/guide/interfaces.phg`; codes `E-IFACE-IMPL`/`-UNIMPL`/
`-SIG`/`-CYCLE` (+ backfilled `E-INSTANCEOF-TYPE` explain). Interfaces are `package main`-only this
slice (`E-PKG-TYPE`); exact signature match (no variance yet). **S3 COMPLETE (Map foundation):**
`Map<K,V>` literals `[k => v]` + indexing `m[k]` — keys are the hashable subset (`int`/`bool`/`string`,
else `E-MAP-KEY`), a missing key faults cleanly (`"map key not found"`, like list-OOB), insertion-ordered
`Value::Map(Rc<Vec<(HKey,Value)>>)` rep (future-proofs R1 iteration order). **One new `Op::MakeMap`**;
the existing `Op::Index` is made **runtime-polymorphic** (List→int-bounds; Map→`map_index` kernel) rather
than a separate `IndexMap`; compiler gains **`CTy::Map(K,V)`** so a map-index result is a first-class
arithmetic operand (`m["k"]+1` specializes on the VM — without it the VM would reject what the interpreter
accepts, the documented CTy-operand trap). Build/lookup single-sourced in `value::build_map`/`map_index`
kernels (`run≡runvm`); transpiles to a PHP `[k=>v]` array. `examples/guide/maps.phg` byte-identical
run≡runvm≡**real PHP**. **Set + the generic-typed query ops (`keys`/`has`/`size`/`contains`/iteration)
are deferred to erased generics (S7), which is REORDERED to immediately follow S3** — they hit the same
no-type-variable wall that defers `core.list`.

**S7a COMPLETE — erased generics core** (pace: **fully autonomous**, `_AUTONOMOUS_3C=1`; design-locked
in the plan's Decisions Log). TypeScript-style `<T>` type parameters on **free functions**
(`function id<T>(T x) -> T`, `firstOr<T>(List<T>, T)`, `applyTwice<T>(T, (T)->T)`), inferred at the
call site by a structural first-binding-wins `unify` that descends `List`/`Map`/`Set`/`Optional`/
`Function`; the call's result is the substituted return type. **No new `Op`, no monomorphization:** a
new `Ty::Param(String)` lives only in a generic fn's stored signature + body (opaque there), and a new
post-check pass `checker::erase_generics` rewrites every type-param annotation to the new `Type::Erased`
and clears the param list **before any backend** (wired into the single `cli::check_and_expand`
chokepoint — covers all backends + the project loader), the same "expanded out before backends"
discipline as `type` aliases / `html"…"`. Erased types → compiler `CTy::Other` / PHP `mixed`
(containers `array`, fns `\Closure`). Free functions only (generic *methods* = clean parse error;
`E-GENERIC-PARAM` on a built-in-shadowing/duplicate param; type params are PascalCase). Byte-identical
run≡runvm≡**real PHP** (`examples/guide/generics.phg`); 424 lib + PHP-oracle differential + 48 integration
green, clippy+fmt clean. Deferred (KNOWN_ISSUES): generic methods/types/classes, a generic fn as a
first-class *value*, an empty `[]` into a generic param, bounds, variance.

**STDLIB NAMESPACE RENAME COMPLETE** (`c4479d6`, namespace reshape part 1): the stdlib is now PascalCase
— `Core.Console`/`Core.Math`/`Core.Text`/`Core.File`/`Core.Bytes`/`Core.Html` (fn names stay camelCase);
`import Core.Console;` + `Console.println(...)`; `Core` reserved. E-SHADOW-IMPORT now only bites a
lowercase **user**-package leaf. Codemod `tools/core_rename*.py` retained. *Pending broader reshape:*
`package main`→`package Main`, user-package casing (E-PKG-CASE), manifest `name`→`module`, lift E-PKG-TYPE.

**Developer decisions (2026-06-20, post-S7a):** generics reach = **ALL** (methods + generic types/classes
too); `core.list` map/filter/reduce = **higher-order native** (`NativeEval::HigherOrder` + backend
closure-invoker + re-entrant `vm.run_until`/`call_closure_value`, no new Op, → array_map/filter/reduce);
sequence = Core rename✓ → **S7b** → generics-all. **Next: S7b** — `Core.List` (map/filter/reduce via the
HOF native + reverse/sum), `Set` (Value::Set + construction native + contains/size), Map keys/values/has/
size; no-callback ops are `Ty::Param`-sig natives unified in `check_native_call`. Then generics-all
(methods → `Box<T>`) → S4 unions → S5 → S6 → S8. Locked decisions + slice order live in the plan's
Decisions Log; full design `~/.claude/plans/misty-honking-lynx.md`. See [[m-rt-progress]] memory.

Project invariants and layout now live in-repo: **`docs/INVARIANTS.md`** (the load-bearing
correctness rules — read before touching backends, value kernels, or the `Op` set) and
**`docs/ARCHITECTURE.md`** (pipeline + module map). `CHANGELOG.md` tracks milestone progress.
