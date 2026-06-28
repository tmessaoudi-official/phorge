# Track F — Tooling & ecosystem maturity (1.0)

## Track summary

A 1.0 language is judged as much by its *surrounding tooling* as by its grammar. Phorj already
ships a remarkably mature CLI for a pre-1.0, single-developer, zero-dependency project: `run`,
`runvm`, `check` (with `--json` machine-readable diagnostics — explicitly an LSP seam), `transpile`,
`parse`, `lex`, `disasm`, `bench` (with `--vs-php` and a `/proc` memory sampler), `build` (standalone
executables, cross-OS), `serve` (with a `--dev` HTML error page), `vendor` (offline, lockfile-pinned
git deps), and `explain` (diagnostic-code dictionary). The GA roadmap (`docs/plans/2026-06-19-phorj-ga-roadmap.plan.md`,
**M12**) has *already scheduled* the headline DX multipliers — **LSP** (on the `--json` seam),
**TextMate/tree-sitter grammar**, **REPL**, a **language-reference doc**, **lexer/parser fuzzing**,
and **release automation + SHA-256 checksums**. So this track's job is not to re-discover those;
it is to find what is *still uncovered* once M12 is honored, and to judge each against the
philosophy (a pragmatic, legible PHP upgrade — familiarity-first, no surprises, every feature with
an idiomatic PHP analogue where one exists).

The genuinely-missing tooling, ranked by how much a PHP developer would expect it on day one:
a **formatter** (`phg fmt` — PHP has `php-cs-fixer`/`pint`; this is table-stakes and trivially
PHP-legible), a **test framework + coverage** (PHPUnit/Pest are central to PHP culture — its
absence is the single biggest ecosystem gap), a **project scaffolder** (`phg new`/`init` —
Composer/`symfony new` analogue, and Phorj's *mandatory* package model makes hand-writing
`phorj.toml` + folder layout a real friction), a **`phg add`/dependency resolver** (the consume
side of `phg vendor` — `composer require`), a **package registry + `phg publish`** (Packagist
analogue — the long-pole ecosystem item), a **docs extractor** (`phg doc` — phpDocumentor), a
**docs *site*** (the project has excellent in-repo markdown but no rendered site), a **web
playground** (compile-in-browser — Phorj's three-backend determinism + zero-dep Rust core make a
WASM playground unusually clean to build and an outstanding adoption lever), a **debugger** (Xdebug
analogue — the heaviest lift, defer), a **standalone profiler** (`bench` covers micro-benchmarking
but not per-call-site flame profiling), a **JetBrains/PhpStorm plugin** (the developer explicitly
wants *both* VSCode and JetBrains; M12's LSP gives VSCode-class editors for free but a first-class
PhpStorm plugin is separate work), and **CI templates** (a `phg`-aware GitHub Action / reusable
workflow, distinct from the repo's *own* CI).

Verdicts skew **adopt** for the cheap, high-familiarity, PHP-analogous tools (fmt, test, scaffold,
doc extractor, playground) and **defer** for the heavy or post-1.0 ecosystem pieces (registry,
debugger, profiler, JetBrains plugin) — none are *reject*, because every one has a direct PHP-world
analogue a PHP developer would immediately reach for; the only question is sequencing.

## Gaps

| id | title | kind | fit | rec | milestone | effort |
|----|-------|------|-----|-----|-----------|--------|
| F-fmt | Code formatter `phg fmt` | port | strong | adopt | M12 | M |
| F-test | Test framework + runner `phg test` | port | strong | adopt | M11 | L |
| F-coverage | Code coverage reporting | port | ok | defer | M12 | M |
| F-scaffold | Project scaffolder `phg new` / `phg init` | port | strong | adopt | M11 | S |
| F-add | Dependency add/resolve `phg add` (consume side of vendor) | port | strong | adopt | M11 | M |
| F-registry | Package registry + `phg publish` (Packagist analogue) | port | ok | defer | GA-M12+/v1.1 | L |
| F-doc | Doc-comment extractor `phg doc` (phpDocumentor analogue) | port | strong | adopt | M12 | M |
| F-docsite | Rendered documentation site | new | ok | adopt | M12 | M |
| F-playground | Web playground (compile-in-browser, WASM) | new | strong | adopt | M12 | M |
| F-lsp | LSP server (on the `--json` seam) | port | strong | defer | M12 (already planned) | L |
| F-vscode | VSCode extension (LSP client + grammar) | port | strong | defer | M12 | M |
| F-jetbrains | JetBrains / PhpStorm plugin | port | ok | defer | GA-M12+/v1.1 | L |
| F-grammar | TextMate / tree-sitter grammar | port | strong | defer | M12 (already planned) | M |
| F-repl | Interactive REPL | port | ok | defer | M12 (already planned) | M |
| F-debugger | Step debugger (Xdebug/DAP analogue) | port | ok | defer | v1.1+ | L |
| F-profiler | Standalone profiler / flame output | port | weak | defer | v1.1+ | M |
| F-citemplates | `phg`-aware CI templates (GitHub Action / reusable workflow) | new | ok | adopt | M12 | S |
| F-langref | Complete language-reference doc | omit | strong | defer | M12 (already planned) | M |
| F-fuzz | Lexer/parser fuzzing + property tests in CI | omit | strong | defer | M12 (already planned) | M |
| F-release | Release automation + SHA-256 checksums per artifact | omit | strong | defer | M12 (already planned) | M |

## Rationale for ADOPT items

**F-fmt — Code formatter `phg fmt` (M12, M).** A formatter is table-stakes for any 1.0 language
and the single most-expected tool a PHP developer reaches for (`pint`, `php-cs-fixer`). It is a
*pure front-end* tool — pretty-print the existing parser's AST — so it never touches the
`run ≡ runvm ≡ php` byte-identity spine and adds zero runtime surprise. The philosophy fit is
strong: it removes a class of bikeshedding surprises (whitespace/style drift) without removing
capability. It also has a natural CI/format-check mode (`phg fmt --check`) mirroring this repo's own
`cargo fmt --check` gate. Cheap relative to its visibility; ship it before GA.

**F-test — Test framework + runner `phg test` (M11, L).** Testing culture is *central* to PHP
(PHPUnit, Pest); a typed PHP-upgrade with no first-party test story would feel conspicuously
incomplete. The honest read is that this is the biggest tooling gap, not the cheapest. It is placed
in M11 because an idiomatic assertion/spec library wants the M10 generics keystone (`assertEquals<T>`,
typed test fixtures) and the S3 lambda investment (`test("name", fn() -> { … })`), and a clean
`Result`/exception model (M11 S4) makes failure reporting natural. The PHP-legible form is a
convention-discovered `*_test.phg` / `test*` function set run by `phg test`, transpiling to PHPUnit
or a thin runner — recognizable to any PHP developer. Strong fit: it makes code *provably* safer,
the core promise.

**F-scaffold — Project scaffolder `phg new` / `phg init` (M11, S).** Phorj's package model is
*mandatory* (`package Main`, `phorj.toml`, strict folder=path) — stricter than PHP by choice — which
means a new project currently requires hand-writing a manifest and the correct directory layout from
memory. A `phg new <name>` (fresh tree) / `phg init` (in-place) that emits a valid `phorj.toml`,
`src/` root, and a runnable `package Main` removes real onboarding friction that the strictness
*creates*. Direct analogue: `composer init` / `symfony new`. Tiny effort, high first-five-minutes
payoff; strong fit.

**F-add — Dependency add/resolve `phg add` (M11, M).** `phg vendor` already does the *fetch +
lock + vendor* half; what is missing is the ergonomic *consume* half — `phg add acme/strutil@1.2`
that edits `[require]` in `phorj.toml` and triggers a vendor pass. This is `composer require`, the
single most-used Composer verb. It is sequenced with M11 because it pairs naturally with transitive
dependency resolution (a current KNOWN_ISSUE deferral) — adding a dep should walk its own
`[require]`. Strong fit: it is pure ergonomics over an already-shipped, already-offline-safe
mechanism, no new surprise.

**F-doc — Doc-comment extractor `phg doc` (M12, M).** PHP has phpDocumentor; a typed language with
real signatures can do *better* than PHP here because types are not in comments — `phg doc` can emit
API docs straight from the checker's typed AST (signatures, generics, visibility) plus doc comments.
It is front-end-only (reads the AST, emits HTML/markdown) so it is spine-safe. Strong fit and a
natural feeder for the docs site (F-docsite). Effort is medium mostly because of the output
rendering.

**F-docsite — Rendered documentation site (M12, M).** The repo's in-repo markdown (`FEATURES.md`,
`ROADMAP.md`, `KNOWN_ISSUES.md`, `docs/`) is unusually thorough but unrendered; a 1.0 language needs
a browsable site (the `php.net`/`docs.rs` expectation). This is `new` (no PHP-language analogue at
the *tool* level, though every language has one) and `ok` fit — it is adoption infrastructure, not a
language feature. Pairs with F-doc (generated API reference) and the language-reference doc already
planned for M12. Can be a static-site generator over the existing markdown + `phg doc` output;
medium effort, mostly content/CI wiring.

**F-playground — Web playground (compile-in-browser, WASM) (M12, M).** This is the strongest
*beyond-PHP* adoption lever in the track. Phorj's zero-external-dependency, std-only Rust core
compiles to WASM cleanly, and its three-backend determinism means a browser playground can show
`run` output, the bytecode `disasm`, *and* the transpiled PHP side-by-side — a uniquely compelling
"try it / see the PHP it becomes" demo (the TypeScript-playground relationship made literal). Strong
fit: it directly serves the familiarity-first adoption strategy by letting a PHP developer paste
code and instantly see the idiomatic PHP it maps to. Medium effort: a `wasm32-unknown-unknown`
target plus a thin web shell; the interpreter path is the obvious first backend to expose (the VM
and transpiler follow).

**F-citemplates — `phg`-aware CI templates (M12, S).** Distinct from the repo's *own* CI
(`.github/workflows/ci.yml`, already shipped in M9): a *publishable* reusable GitHub Action / workflow
template (`phorj/setup-phg` + a `phg check && phg test && phg fmt --check` job) so downstream Phorj
projects get CI in one line. Analogue: `shivammathur/setup-php`. Small effort, real ecosystem
multiplier, `ok` fit (ecosystem infra rather than a language feature). Worth shipping with GA so the
first external projects have a paved path.

## Critic pass

I read FEATURES.md, ROADMAP.md, docs/MILESTONES.md, the GA roadmap plan, and KNOWN_ISSUES.md before
this pass, and grepped `src/`/docs for `watch`, `completion`, `install`, `--fix`, `lint`, toolchain
pinning. Verdict on the original 21: all sound, none mis-listed (verified — there is no `phg fmt`,
`phg test`, `phg lint`, watch mode, completions, or installer in `src/` today; `phg check --json` is
the only LSP-adjacent surface and it ships). One nuance: **F-lsp/F-grammar/F-repl/F-fuzz/F-release/
F-langref are genuinely already-scheduled in GA M12** (confirmed against the GA roadmap plan's exit
criteria), so their `defer` + "(already planned)" labels are correct, not new gaps — keep them as
completeness placeholders.

**Newly-found long-tail items** (all spine-safe / front-end / CLI ergonomics — the philosophy lens
favors them: each is a tool a PHP/Go/Rust dev reaches for on day one, none touches the
`run≡runvm≡php` byte-identity spine):

| id | title | kind | fit | rec | milestone | effort |
|----|-------|------|-----|-----|-----------|--------|
| F-lint | Static-analysis linter + autofix `phg lint [--fix]` | port | strong | adopt | M12 | M |
| F-watch | Watch mode `phg run/check --watch` | new | strong | adopt | M12 | S |
| F-completions | Shell completions `phg completions bash\|zsh\|fish` | port | strong | adopt | M12 | S |
| F-installer | One-line installer / version manager (`phgup`, rustup analogue) | new | ok | defer | GA-M12+/v1.1 | M |
| F-toolchain-pin | `phorj`-version field in `phorj.toml` (toolchain pin) | port | strong | adopt | M11 | S |
| F-migrate | PHP → Phorj migration tool `phg migrate` | port | ok | defer | M8 (already planned) | L |

**Rationale (new items):**

- **F-lint — `phg lint [--fix]` (M12, M).** Distinct from F-fmt (whitespace) and from `phg check`
  (type errors). PHP's static-analysis layer (PHPStan/Psalm/`pint` lint rules) catches dead code,
  unused locals, unreachable arms, redundant casts, and shadowed bindings *that still type-check*.
  Phorj already has a **warning channel** (`check()` returns `Ok(warnings)`, e.g. `W-FORCE-UNWRAP`),
  so a lint surface is a natural extension of an existing seam — and `--fix` (mechanical rewrites via
  the same AST-pretty-printer F-fmt builds) is pure front-end, spine-safe. Strong fit: removes a class
  of latent bugs without removing capability; a PHP dev expects it.

- **F-watch — `phg run/check --watch` (M12, S).** A re-run-on-change loop (cargo-watch / vite /
  `deno --watch` analogue). PHP's edit-refresh loop is implicit via the web server; a static language's
  edit-check-run loop is exactly where a watcher pays off most (instant type feedback). Tiny effort
  (std-only stat-poll is enough — no inotify crate needed, preserving zero-dep), pure CLI ergonomics,
  zero spine risk. Strong fit; the first-five-minutes feel of a modern toolchain.

- **F-completions — `phg completions <shell>` (M12, S).** Table-stakes CLI polish every Go/Rust/Deno
  binary ships; the arg surface (`run`/`runvm`/`check`/`transpile`/`build`/`serve`/`vendor`/`bench`/
  `disasm`/`explain` + flags) is already fully enumerated in the CLI parser, so emitting a static
  completion script is mechanical. Strong fit, small effort.

- **F-installer — one-line installer / `phgup` (v1.1, M).** The *install* counterpart to F-release's
  checksummed artifacts: a `curl … | sh` bootstrap + a lightweight version manager (rustup / phpenv
  analogue) that fetches the right `phg` for the platform. `ok` fit (adoption infra, not a language
  feature) and deferred past 1.0 because it depends on F-release's artifact pipeline existing first
  and on hosted download infra — pair it with the registry/distribution wave.

- **F-toolchain-pin — `phorj` version in `phorj.toml` (M11, S).** Reproducibility: Go's `go 1.22`
  directive and Rust's `rust-version` field let a project declare the minimum/expected toolchain.
  Phorj's manifest already parses `[require]`/`name`; adding a `phorj = "1.0"` field and a startup
  check is tiny and pays off the moment the language has more than one released version (i.e. at GA).
  Strong fit — it makes a project's build *provably* reproducible, the core promise, in a form a
  Composer/`go.mod` user recognizes immediately. Sequence with M11 (manifest/stdlib completion).

- **F-migrate — `phg migrate` PHP→Phorj (M8, L).** The inverse of the transpiler — import PHP and
  infer static types. **Already scheduled** (FEATURES.md "PHP → Phorj migration 🔲 M8"; ROADMAP M8 of
  the historical ecosystem numbering); the original track-F list omitted it as a *tooling* item, so
  it is added here for completeness with its existing slot. `ok` fit (heavy, inference-hard, but a
  direct adoption on-ramp for existing PHP codebases — the strongest "upgrade your PHP" story). Defer
  to its planned M8 slot; not a new prioritization.

**Mis-listings found:** none. Every original row reflects an un-shipped capability (verified against
`src/` and FEATURES.md). `removed_mislisted = 0`.
