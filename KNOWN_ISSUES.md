# Known Issues & Limitations

Phorge is pre-1.0. This page lists current limitations and known rough edges. Most "limitations" are
**deliberate scope boundaries** — features that are *planned* (see [ROADMAP.md](ROADMAP.md)) rather
than broken. The key property is that out-of-scope constructs are **rejected cleanly** (a type or
parse error, non-zero exit) — never a crash.

## Language features not yet implemented

These are designed but not in the current surface; using them produces a clean compile-time error,
not a panic:

- `Map` / `Set` / tuples
- The pipe operator (`|>`) and the `is` operator
- Exceptions (try / catch / throw)
- Mutation (reassignment and field writes) — Phorge is immutable-by-default today
- Method/function overloading, traits, operator overloading, property accessors
- Sized integers / `decimal`, `const`/`final` enforcement
- `match` outside return / variable-declaration-initializer position

## Git dependencies (M5 S3)

- **Transitive dependencies are not resolved.** `phorge vendor` fetches the direct `[require]` set;
  a dependency's *own* `[require]` is not walked. Vendor flat-named leaf libraries for now (the
  shipped `examples/project/withdeps/` does exactly this).
- **`phorge build` is single-file and does not merge `vendor/`.** A program that imports a vendored
  (or any cross-package) dependency runs via `run`/`runvm`/`transpile` (which go through the project
  loader) but cannot yet be compiled to a standalone executable. `build` embeds one source file only
  (M2.5 Phase 1 scope), unchanged by S3.
- **Resolution is offline by design.** `run`/`check`/`transpile` never fetch — they read the
  committed `vendor/`. Only `phorge vendor` touches the network; commit `vendor/` + `phorge.lock` so
  builds stay deterministic and reproducible (the same determinism rule that defers URL/network to M6).

## `phorge build` limitations (M2.5, in progress)

- **macOS targets are rejected.** The Mach-O/fat section *reader* ships and is tested, but producing a
  signed macOS *stub* is deferred to Phase 3. An apple/darwin `--target` errors with a clear message
  rather than emitting a broken binary.
- **Cross-builds need a source checkout.** `--target`/`--all` compile a stub from source via
  `cargo-zigbuild`, so they must run from a phorge source tree. A *distributed* (sourceless) phorge
  can still do a **host** build (it reuses the running binary as the stub) but not a cross build until
  the Phase 3 prebuilt-stub registry lands.
- **Built binaries ignore argv and always exit 0.** A standalone built binary runs its embedded
  program; command-line arguments passed to it are currently ignored. (`--version`/`--help` are
  features of the `phorge` CLI itself, not of built binaries.)
- **aarch64 / Windows artifacts aren't executed in CI here.** They're validated by an object-section
  round-trip; native execution is verified for the host-runnable `x86_64-musl` target.

## Behavioral quirks

- **Errors inside string interpolation report line 1 (and the caret points there).** A fault *or* a
  type error raised within a `"{ … }"` interpolation is reported at line 1 because the interpolation
  sub-lexer resets position — so the diagnostic caret (S0.4) underlines column 1 of the program rather
  than the real sub-expression. (VM runtime errors carry an accurate line; the interpreter's runtime
  errors generally do not. Errors *outside* interpolation are located and underlined accurately.)
- **Recursion is depth-limited.** Recursion runs on a fixed-size (256 MB) worker stack with explicit
  depth caps (`src/limits.rs`); extremely deep recursion faults cleanly rather than overflowing the
  native stack.
- **Zero-payload enum variants need call form.** A nullary variant `V` must be written `V()` both to
  construct **and** in a `match` pattern. A bare `V =>` arm is parsed as a catch-all *binding*, not a
  variant match — so it silently matches everything. Always use `V()` in patterns for nullary
  variants.
- **Transpiled ranges differ from Phorge for an empty/reversed range.** A Phorge range `a..b` with
  `a >= b` is *empty*; the emitted PHP uses `range($a, $b - 1)`, and PHP's `range()` *descends* when
  the start exceeds the end rather than yielding `[]`. This is a transpile-only caveat — the Phorge
  backends (`run`/`runvm`) treat an empty range as empty and stay byte-identical; only the
  PHP-transpiled output diverges, and only for empty/reversed ranges. Use ascending, non-empty ranges
  when round-tripping through PHP. (Parallel to the indexing-OOB transpile note.)
- **Irrational `float` values render with more digits on the Phorge backends than in transpiled PHP.**
  The Phorge backends stringify a `float` with Rust's shortest-round-trip formatting (e.g.
  `sqrt(2.0)` → `1.4142135623730951`), while the transpiled PHP relies on PHP's default `echo`
  precision (`precision=14` → `1.4142135623731`). For *exactly representable* values (integers-as-
  floats, short terminating decimals) both render identically, so `guide/math.phg` keeps to such
  values. This is a transpile-only caveat — the `run`/`runvm` spine is byte-identical (both Rust); it
  predates `core.math` (any irrational float interpolation hits it) and `core.math` merely makes it
  easy to reach via `sqrt`/`pow`. Round-trip through PHP only with exactly-representable floats.
- **`opt!`-on-null transpiles to a different message than the Phorge backends.** A null force-unwrap
  faults `force-unwrap of null` on `run`/`runvm` (located, classified `FaultKind::ForceUnwrap`); the
  transpiled PHP throws a `RuntimeException("force-unwrap of null")` via the `__phorge_unwrap()`
  helper without the source name/line. The *present-value* case is byte-identical; only the null-fault
  message differs (a transpile-only caveat, parallel to the range/index-OOB notes). The differential
  harness excludes fault cases by design.

## Reporting

Found something not listed here — especially a panic, hang, or crash on any input? That's a bug.
Please report it (see [SUPPORT.md](SUPPORT.md); for security, [SECURITY.md](SECURITY.md)).
