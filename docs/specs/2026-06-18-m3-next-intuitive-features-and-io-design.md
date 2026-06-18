# Phorge — "More Intuitive Features" + I/O / stdlib / imports — Research & Design

> Brainstorm output, 2026-06-18. Trigger: developer asked for (a) **more intuitive language
> features**, and (b) **exhaustive examples with imports — file reading, URL reading, complex
> real-world programs**. This doc grounds both asks in the existing roadmap
> (`2026-06-17-m3-language-roadmap-design.md`, `2026-06-15-ecosystem-roadmap-design.md`, `VISION.md`),
> surfaces two hard constraints the file/URL ask collides with, and proposes a principled path.
> **Draft — under review. No code written yet.**

## 1. The ask vs. the current surface

S0/S1/S2 are **done** (DX, ergonomics, null-safety). The developer wants more *intuitive* features and
a much broader example set — specifically examples that **read files**, **read URLs**, and **use
imports** beyond the decorative `import std.io;`.

Reality check (verified against the repo): Phorge today has **`println` as its only builtin**, **no
stdlib**, **no I/O**, `import` is **decorative** (parses, resolves nothing), and there are no `Map`/`Set`.
So the file/URL/import examples **cannot be written today** — they require new language capability.
This is why "research/design first" is the right order (developer's choice).

## 2. Two hard constraints the file/URL ask collides with

These are load-bearing Phorge invariants (`VISION.md`, `docs/INVARIANTS.md`), not preferences:

- **C1 — byte-identical backends (the correctness spine).** Every feature must produce *identical*
  output on `run` (interpreter) and `runvm` (VM), enforced by `tests/differential.rs`, which globs
  `examples/**/*.phg`. A feature that can't be proven equivalent across backends **doesn't ship**.
- **C2 — std-only, zero-dependency core.** The runtime links **no external crates**
  (`#![forbid(unsafe_code)]`, no supply-chain surface).

Consequences for the ask:

| Capability | Std-only feasible? | Byte-identical? | Verdict |
|---|---|---|---|
| **File read/write** | ✅ Rust `std::fs` ↔ PHP `file_get_contents`/`file_put_contents` | ✅ **if** both backends read a **committed fixture** (deterministic) | **Feasible now** |
| **URL read (HTTP GET)** | ❌ Rust **std has no HTTP/TLS client** — would need a crate (breaks C2) | ❌ network is non-deterministic (breaks C1) | **Blocked** — needs a principle change (M6 native HTTP connector) or PHP-backend-only divergence |
| **Real `import`** | ✅ resolve `import std.io;` to a built-in native module (no network) | ✅ pure resolution, no runtime effect | **Feasible** as the first real-import step |

**Headline finding:** *file* examples are achievable and stay byte-identical with committed fixtures;
*URL* examples are **not** achievable without either (a) adding an HTTP dependency — the project's
first-ever non-std crate, a deliberate M6 decision — or (b) making URL I/O **PHP-backend-only** (a
documented divergence, excluded from the differential glob like the `opt!`-on-null fault case). I do
**not** recommend pulling in an HTTP dep now; it would breach C2 for a single example category.

## 3. The foundation both asks ride on: `NativeModule` dual-registration

The ecosystem design (decision E-1/§3) already names the real work: in a statically-typed language,
every builtin/module must register **four** facets at once —

1. a **type signature** (consumed by `checker.rs` so calls type-check),
2. an **interpreter implementation** (`interpreter.rs`),
3. a **VM implementation** (`vm.rs`) — identical observable behavior (C1),
4. a **PHP-emission mapping** (`transpile.rs`) — the idiomatic-PHP target (D-L9).

`println` is today a hard-coded special case of exactly this. Generalizing it into a `NativeModule`
registry is the unlock for *all* stdlib (io/fs/string/math/collections), and it's the principled home
for `read_file`. This is originally **M4** in the ecosystem roadmap; the file-examples ask pulls a
*minimal* slice of it forward.

**S2 synergy:** `read_file(string path) -> string?` returns an **optional** — null on a missing file —
so it exercises the null-safety we just shipped (`??`, `if (var x = read_file(p))`). The stdlib and the
language features reinforce each other.

## 4. Proposal — two independently-shippable tracks

### Track A — "Intuitive language features" (highest-ROI ergonomics, no I/O)

Ordered by ROI (from the M3 roadmap §6, all PHP-mapped per D-L9):

1. **S3 — lambdas + pipeline.** First-class functions/lambdas, `.map`/`.filter`/`.reduce` (and/or
   comprehensions), and the **`|>` pipe** (PHP 8.5). *The* most intuitive next step — makes data
   transformation fluent. PHP: closures + `array_map`/`array_filter`/`array_reduce` / native `|>`.
2. **Named arguments + default parameter values** (PHP 8.0) — `f(name: "x")`, `function f(int n = 0)`.
   Low cost, high intuitiveness.
3. **S4 — `Map`/`Set`/tuples + destructuring** — needed for genuinely real-world programs.
4. **S5 — records / data classes** — `record Point(int x, int y)` with auto equality/display/`clone
   with`; kills getter/setter boilerplate.

### Track B — I/O + stdlib + imports (unlocks the file/real-import examples)

1. **`NativeModule` registry** — generalize `println`; the dual+ registration foundation.
2. **`std.io` + `std.fs`** — `print`/`println`, `read_file(path) -> string?`,
   `write_file(path, contents)`, `read_lines(path) -> List<string>?`. File-only (no URL — see C1/C2).
3. **Real `import std.io;` / `import std.fs;`** — resolve to native modules (first real-import step;
   file-based `import a.b.c` of user `.phg` modules stays M5).
4. **URL reading — explicitly deferred** to M6 (native HTTP connector, the project's first non-std dep)
   *or* shipped PHP-backend-only as a documented divergence. Not in the byte-identity-gated set either way.

**"What can we import / do?" — the stdlib menu Track B would unlock** (each a `NativeModule`, PHP-mapped):

| Module | Functions (illustrative) | PHP target | Std-only native? |
|---|---|---|---|
| `std.io` | `print`, `println`, `eprintln`, `read_line() -> string?` | `echo` / `fwrite(STDERR)` / `fgets(STDIN)` | ✅ |
| `std.fs` | `read_file(p) -> string?`, `write_file(p, s)`, `read_lines(p) -> List<string>?`, `exists(p) -> bool` | `file_get_contents`/`file_put_contents`/`file` | ✅ |
| `std.string` | `len`, `upper`, `lower`, `split`, `join`, `trim`, `contains`, `replace` | `strlen`/`strtoupper`/`explode`/`implode`/… | ✅ |
| `std.math` | `abs`, `min`, `max`, `clamp`, `pow`, `sqrt`, `floor`, `ceil` | `abs`/`min`/`max`/`pow`/… | ✅ |
| `std.list` | `map`, `filter`, `reduce`, `sort`, `reverse`, `sum`, `first`/`last` | `array_map`/`array_filter`/… (or S3 methods) | ✅ |
| `std.json` | `parse(s) -> Json?`, `stringify(v) -> string` | `json_decode`/`json_encode` | ✅ |
| `std.time` | `now() -> int` (epoch), `format(ts, fmt)` | `time`/`date` | ✅ (epoch is fine; wall-clock is non-deterministic → not byte-identity-gateable, like URL) |
| `std.http` | `get(url) -> string?` | `file_get_contents($url)` | ❌ **blocked** (no std HTTP client; non-deterministic) |

So "many things to import" is real and mostly std-only-feasible — **except** anything inherently
**non-deterministic** (URL fetch, wall-clock `now()`, randomness): those can't be byte-identity-gated
examples and need the C1/C2 decision (Q2).

### Track C — exhaustive examples (the final phase, after A and/or B land)

Independent of A/B, the **existing** surface can already be showcased far more exhaustively. Audit of
`examples/` vs. the implemented language shows good guide coverage but room for: more `realworld/`
programs, a "kitchen-sink" feature tour, and per-feature focused examples for anything under-shown.
File/real-import examples land *with* Track B; URL examples wait for the M6 decision.

### Track D — Phorge-vs-PHP benchmark ("who's the winner?") — achievable NOW

The developer wants a head-to-head Phorge-vs-PHP performance comparison. This is the **lowest-friction
high-delight win**: it needs **no new language features**, only the existing `phg bench` (median-of-N
timing of `run` vs `runvm`, output-identity-gated) + the existing PHP round-trip (`transpile` → run
`php`). Extend `bench` (or add `bench --vs-php`) to also transpile + time the PHP backend, producing a
**3-way table: tree-walk interpreter vs bytecode VM vs transpiled PHP**, all gated on identical output.

- **Feasibility:** ✅ now. `phg bench` infra exists; `php` is on PATH; output-identity already enforced.
- **Caveats to report honestly:** the comparison is *Rust VM vs the PHP interpreter on the same
  algorithm* — informative but apples-to-oranges (different runtimes, JIT/opcache on/off matters). Report
  PHP version + whether opcache/JIT is enabled; offer a `--php-args` passthrough; median-of-N like today.
- **Showcase:** a `examples/bench/` companion + README table. A natural, fun artifact that also stress-
  tests the transpile bridge on perf-shaped programs.

## 5. Decisions to lock (developer steer)

- **Q1 — first deliverable:** Track D (Phorge-vs-PHP benchmark — achievable now, no new features),
  Track B (I/O/stdlib/imports — unlocks file + real-import examples), or Track A (S3 lambdas/pipeline —
  highest-ROI ergonomics)? Recommendation: **D first** (instant payoff + showcases the bridge), then B.
- **Q2 — URL reading:** defer to M6 (recommended), PHP-backend-only divergence, or pull in an HTTP dep
  now (breaches C2)?
- **Q3 — intuitive-features scope:** how much of Track A's menu to commit to once we get there.

## 5b. Locked decisions (2026-06-18, developer-confirmed)

- **L-1 — build sequence: D → B → A.** Phorge-vs-PHP benchmark first (no new features), then I/O +
  std-only stdlib + real `import std.*`, then S3 lambdas/pipeline. Each ships byte-identical with
  examples + docs (the S2 cadence).
- **L-2 — URL/network deferred to M6.** Build a **rich std-only stdlib now** (`std.fs`, `std.math`,
  `std.string`, `std.list`, `std.map`/`std.set`, hand-rolled `std.json`, seeded `std.random`) — all
  byte-identity-gateable. Native HTTP/URL waits for M6's connector-trait architecture. The developer
  heard the full challenge (Option 3 breaches zero-dep *and* can't yield byte-identical URL examples,
  since C1 — not the missing client — is the blocker) and chose to keep the zero-dep core. The
  dependency axis (std-only vs crate) and the determinism axis (gateable example vs not) are
  orthogonal; non-deterministic capabilities (clock, env, subprocess, unseeded random, network) may
  exist in the language but are excluded from the differential-gated example set.
- **L-3 — multiple inheritance: rejected (D-L3), realized as traits/mixins + interfaces at S5.** No
  example until it lands; "examples ship with features" applies to every unimplemented feature.
- **L-4 — Track C example coverage: every *implemented* feature must have a runnable example.** Audit +
  fill gaps; unimplemented features (S3 lambdas, S4 Map/Set, S5 traits/records, generics, exceptions)
  get theirs the moment they land, never retroactively or speculatively.

## 6. Open items

- `NativeModule` PHP-emission ergonomics (E-4, originally M4) — pin when Track B starts.
- Exact `std.fs` surface + fixture-file convention for byte-identical file examples.
- S3 fork: comprehensions vs `.map/.filter/.reduce` pipeline (or both) — from roadmap §8.
