# Agent G — Showcase & Adoption Surface Audit

Date: 2026-07-02 · Scope: examples/, conformance story, CI/tests/tools/playground/editors gap-fill, onboarding.
Binary audited: `target/release/phg` 0.5.1-alpha.1. All claims graded per Rule 18.

---

## PART 0 — Severity summary

| Sev | Count | Headline |
|---|---|---|
| P0 | 2 | README front-door examples fail to compile; copy-paste CLI commands across docs are dead (renamed commands, no aliases) |
| P1 | 6 | README status/claims stale (zero-deps false, M2.5 "in progress", lift "future"); killer-app gap (flagship DDD hidden in conformance/); benchmark credibility (3.23× vs a *dev* PHP); no normative spec; examples index gaps + self-contradiction; CONTRIBUTING "no dependencies" false |
| P2 | 5 | Grammar missing new keywords + extension at 0.2.0; stale one-shot codemods in tools/; `phg --help` omits `lsp`/`debug`; examples "Not yet supported" lists shipped `decimal`; walkthrough dirs unindexed |
| P3 | 3 | Committed `.vsix` build artifact; stale `println`/`recv` in example comments; "oracle-nightly" job runs on push, not nightly |

---

## PART 1 — Examples as a brochure

### P0-1 — The README hero example and both quick-start one-liners DO NOT COMPILE

`function main() {` with no return type is now rejected (`E-MISSING-RETURN-TYPE`, the S0b return-type
mandate). Affected, all verified by running the current binary:

- README.md:36 hero block — `phg run` → `type error at 16:1: `main` must declare a return type`
  [Verified: extracted the block and ran `./target/release/phg run` — exact error above]
- README.md:107 (stdin one-liner) and README.md:110 (`-e` one-liner) — same failure
  [Verified: piped the exact line 107 snippet into `phg run -` — `E-MISSING-RETURN-TYPE` at 1:35]
- examples/cli/README.md:11,12,65,67,89,92,95 — every inline snippet uses `function main() {`;
  the "diagnostics demo" snippets (div-by-zero, index OOB) never reach their intended error because
  the missing-return-type check fires first [Inferred: identical shape to the verified failures;
  snippets grepped, not individually executed]

This is the single worst adoption defect: a PHP developer's first 60 seconds with the project is a
compile error on the front page. The `.phg` files themselves are fine (`examples/hello.phg` declares
`: void` and runs [Verified: `phg run examples/hello.phg` → `Hello, Phorj!`]) — only the *embedded
markdown snippets* were missed by the return-type codemod (`tools/return_type_codemod.py` covers
`.phg` + `.rs` string literals, not `.md`).

### P0-2 — Renamed CLI commands are hard-removed; docs still teach the old names

`bench`/`fmt`/`disasm`/`lex` no longer parse — no alias, just the usage error
[Verified: `phg bench examples/bench/workload.phg` → usage error; same for `fmt --check`, `lex`].
Current names: `benchmark`/`format`/`disassemble`/`tokenize` [Verified: `phg --help`].
19 stale occurrences across the adoption surface [Verified: grep count]:
`README.md` CLI table (rows `lex`, `disasm`, `bench`, `fmt` at lines 128–136 — the table contradicts
the `phg format` section heading at line 201), `CONTRIBUTING.md`, `examples/bench/README.md` (all 5
command lines in "Running it"), `examples/bench/workload.phg` + `manual/` (comments),
`examples/README.md` (index rows), `examples/cli/README.md`, `editors/vscode/README.md`,
`editors/phpstorm/README.md`, `editors/README.md`, `docs/GA-CHECKLIST.md`, `docs/MILESTONES.md`.
Every one of these is a command a new user will copy-paste and watch fail.

### Pedagogy — strong (genuinely good)

- Each `guide/*.phg` teaches one named thing, with a design-rationale comment header that explains
  *why* (e.g. `as-cast.phg` explains the Kotlin/Swift `as?` model vs the C-cast lie, then proves
  single-evaluation with a side-effecting scrutinee) [Verified: read several via the generated
  playground examples.js, which embeds the sources verbatim].
- Narrative arc exists: `hello`/`fib`/`grades` → `guide/` (114 programs) → `realworld/` (4) →
  `web/` (10) → `project/` (8 multi-file projects) → `transpile`/`build`/`cli` walkthroughs.
- The "Sharp edges" section of examples/README.md documenting *rejected* programs (visibility,
  definite assignment) is an honest, clever way to showcase compile errors that can't be runnable.
- Anti-persuasion residue is minor but real: the exactly-representable-float discipline is documented
  as a corpus rule (conformance/README.md), and outputs like tempconv's `freezing = 32F` are thin —
  but most guide programs print meaningful multi-line output. [Verified: read conformance/README.md]

### Coverage vs surface

- The index table + coverage matrix in examples/README.md is exhaustive for guide/, **but written as
  a changelog, not a brochure**: rows are dense with internal jargon ("no new `Op`/`Value`",
  milestone codes "M-mut.7b", "F13 aliasing catcher"). A PHP developer evaluating the language
  cannot use this as a tour. [Verified: read all 288 lines]
- **Index gaps** [Verified: grep for each name → 0 hits]: `web/json-api.phg`, `examples/random/`
  (whole directory, `dice.phg`), `process/args-env.phg`, `debug/README.md`. These shipped features
  are invisible from the index.
- **Self-contradiction**: examples/README.md:286 "Not yet supported … `decimal`" while the same
  file's index lists `guide/decimals.phg`, `decimal-div.phg` and the coverage matrix has three
  decimal rows [Verified: same file, lines 169–171 vs 286]. Also stale: "sized ints" phrasing and
  the M2.5 framing.
- Stale name residue in example comments (P3): `guide/concurrency.phg` says `recv` (now `receive`)
  in 4 comments; `guide/secret.phg` comment says "println / file" (retired name)
  [Verified: grep — code itself is clean, comments only].

### The killer-app gap — CONFIRMED, with a twist

The four `realworld/` programs are 42–57 lines each [Verified: wc -l] — vignettes, not applications.
The **actual flagship exists but is hidden**: `conformance/ddd/` is a multi-file, multi-package
Domain-Driven-Design ordering domain (bounded contexts → packages, aggregates → classes,
cross-package `import type`, namespaced PHP) — exactly the composed showcase the project needs —
but it lives in the conformance corpus, is absent from README.md and examples/README.md
[Verified: grep "ddd" → 0 hits in both], and no adopter will ever open `conformance/`.
`web/json-api.phg` (69 lines, router + JSON) is the best web composition and is also unindexed.
There is still no program composing web + Json + decimal + Result/throws + concurrency into
something a developer *wants* (a small REST service with money math is the obvious candidate — every
ingredient ships today).

---

## PART 2 — Conformance story

### What exists (the brief's assumption was outdated — this is further along than "nothing")

1. **`conformance/` golden-output corpus — 55 programs + `.out` goldens** across `lang/`, `types/`,
   `collections/`, `stdlib/`, `errors/`, `web/`, plus the `ddd/` flagship project
   [Verified: find count]. `tests/conformance.rs` asserts interpreter, VM, **and real PHP** all
   equal the golden — strictly stronger than the differential's agree-only check (pins the *value*,
   catches identical-drift). Glob-discovered (zero test edits to add a program), with a ≥10-file
   tripwire against silent corpus loss, and the same `PHORJ_REQUIRE_PHP=1` fails-not-skips gating
   [Verified: read tests/conformance.rs in full].
2. **`conformance/diagnostics/` negative corpus** — programs that must FAIL with `.expected`
   diagnostics, gated by `tests/diagnostics.rs` [Verified: ls].
3. **The differential harness** (`tests/differential.rs`, 152KB) — run≡runvm≡PHP over
   `examples/**/*.phg` + inline programs, with semantic `FaultKind` classification (13 kinds:
   IntOverflow, DivZero, IndexOob, ForceUnwrap, DecimalInexact, Concurrency, …) so *fault parity* is
   asserted by meaning, not string equality [Verified: grep of enum + `agree`/`agree_err`].
4. **STABILITY.md tier declaration** — stable/experimental/deprecated, with the explicit contract
   "every stable construct is exercised by the conformance corpus, so a regression fails CI"
   [Verified: read].
5. **`phg explain` code registry** + `selftest/` (`phg test` showcase) as further de-facto surface.

### What's missing — the normative spec

There is **no normative language specification**. `docs/specs/` holds 81 *design* docs (decision
records, not normative text); `2026-06-15-phorj-language-design.md` is the frozen *M1* design, ~15
milestones stale [Verified: ls docs/specs + README.md:267 still points to it as "the frozen language
design"]. Nothing defines grammar, typing rules, or evaluation semantics as a versioned reference.
Consequences: (a) the conformance corpus is keyed to STABILITY.md's *bullet list*, so coverage is
unverifiable — no way to know which stable construct lacks a conformance program; (b) "byte-identical
across three backends" is an *implementation* invariant, not a *language* definition — a fourth
implementation (or a user disputing behavior) has no arbiter.

### Proposed conformance architecture (for the master plan)

1. **`docs/spec/` normative spec, one file per chapter**, mechanically numbered:
   `01-lexical.md` (tokens, literals, interpolation, raw/text-block strings), `02-types.md`
   (primitives, decimal, optionals, unions/intersections, generics-erasure), `03-expressions.md`,
   `04-statements-flow.md` (totality, narrowing), `05-classes-traits.md`, `06-enums-match.md`
   (incl. the `V()` zero-payload rule), `07-packages-imports.md`, `08-stdlib-core.md` (per-module
   function contracts), `09-errors-faults.md` (throws/Result/FaultKind taxonomy), `10-php-mapping.md`
   (the transpile contract — unique to Phorj and a marketing asset: "here is exactly the PHP your
   code becomes"). Each normative clause gets a stable ID (`§3.4.2-as-cast-single-eval`).
   Source it *from* the existing assets: STABILITY.md's stable list is the chapter checklist,
   `phg explain` codes become the error-clause appendix, guide-example prose is 60% of the text.
2. **Spec-tag the corpus**: header comment `// conformance: §3.4.2` in each `conformance/*.phg`;
   a build-time check (extend `tests/conformance.rs`) parses tags and emits a **spec-coverage
   matrix** — every normative clause must name ≥1 conformance program or carry an explicit
   `untested` marker. That converts "is the corpus complete?" from vibes to a failing test.
3. **Harness upgrade path**: `tests/differential.rs` stays the *agreement* net (examples);
   `tests/conformance.rs` becomes the *specification* net (golden + spec tags). The two already
   share the PHP-oracle plumbing — factor `php_or_gate`/`run_php` into a shared test-support module
   (currently duplicated verbatim [Verified: conformance.rs comment "mirrors tests/differential.rs"]).
4. **Versioning/editions hook**: the spec carries the language version in front-matter; SEMVER.md
   already defines the tiers. When M13 (editions) lands, spec clauses gain `since:`/`edition:`
   fields and the corpus splits per edition directory. Freeze at 1.0 = tag the spec + corpus
   together.

Effort estimate: the spec skeleton + tagging of the existing 55 programs is days, not weeks, because
the raw material (STABILITY.md, examples/README.md rows, explain registry) already exists.

---

## PART 3 — Gap-fill craftsmanship audit

### CI workflows — the gates are REAL (this is the strongest area audited)

- **fmt gate**: `cargo fmt --check` as a bare step — non-zero exit fails the job; no exit-code
  masking, no `|| true` anywhere in ci.yml [Verified: read .github/workflows/ci.yml in full].
- **PHP oracle**: `PHORJ_REQUIRE_PHP: '1'` on `cargo test --workspace`, PHP pinned **8.5**
  (the floor) with bcmath, and an explicit comment explaining why floor-testing is the strict gate
  [Verified: ci.yml lines 42–66]. The **8.6-dev canary is `continue-on-error: true`** — non-gating
  as designed [Verified: line 95].
- **perf-gate**: `scripts/perf-gate.sh` is `set -eEuo pipefail`, `LC_ALL=C` (locale-decimal
  corruption guarded), exit 1 on regression / 2 on setup error, best-of-N on the machine-independent
  `vm_speedup` ratio with a documented generous floor [Verified: read all 55 lines]. It genuinely
  fails the build.
- **pre-commit hook** (scripts/git-hooks/pre-commit): `set -euo pipefail`, fmt+clippy+test, no
  masking [Verified: read].
- **cross-build job**: real — installs zig pinned 0.16.0 from the canonical URL, llvm-objcopy from
  the sysroot with a hard failure if absent, then runs `tests/build.rs` single-threaded.
- Nits: (P3) job named `oracle-nightly` but triggers on push/PR — cosmetic; (P2) `phg --help`'s
  command list omits `lsp` and `debug` although the usage string includes them
  [Verified: help output vs usage error string].

### tests/ harness quality

- `differential.rs`: FaultKind-classified fault parity (`agree_err`), `agree` for output parity,
  project-aware discovery, quarantine conventions for `pure:false`/no-PHP features — high quality.
- `conformance.rs`: min-count tripwire, project goldens through `loader::load`, diagnostics dir
  structurally excluded — good.
- Duplication: the PHP-oracle helpers are copy-pasted between the two harnesses (acknowledged in a
  comment) — consolidation candidate, not a defect.
- Nothing order-dependent spotted in the harnesses read; `tests/build.rs` is already forced
  `--test-threads=1` in CI.

### tools/ codemods — stale, project rule violation (P2)

`tools/core_rename.py` (superseded by v2 *in the same directory* — v1's scanner "desynced on
multi-line raw strings" per v2's own docstring), `core_rename2.py` (Core-rename executed 2026-06-23,
long done), `return_type_codemod.py` (S0b executed) [Verified: read all three headers]. The
project's own standing rule says stale one-shot migration scripts are actively harmful — v1
especially, since it contains a *known-broken* scanner. Recommend deleting all three (git history
preserves them); the CLAUDE.md reference to `tools/wave1_migrate.py` is already a dangling pointer
(file absent) [Verified: ls tools/].

### playground/ + editors/

- **playground deploy is reproducible**: playground.yml rebuilds wasm + regenerates examples.js from
  `examples/guide/` on every master push (path-filtered), Pages deploy, stack-size and cap-lints
  handled [Verified: read workflow]. Committed `examples.js` is 111 keys vs 114 guide programs —
  the 3 `Core.File` programs are *deliberately* dropped (browser has no fs, logged not silent)
  [Verified: gen_examples.py docstring + key count]. Committed copy is regenerated at deploy → fresh.
- **VS Code grammar drift (P2)**: `editors/vscode/syntaxes/phorj.tmLanguage.json` knows
  `when`/`spawn`/`receive`/`discard`/`never`/`decimal`/`bytes`/`b|r` string prefixes — good — but is
  **missing** `empty` (the naming-overhaul keyword), `todo`, `unreachable`, `parent`, and the
  `html"…"` literal prefix (only `b|r` matched) [Verified: keyword-by-keyword grep]. Extension
  version 0.2.0 vs phg 0.5.1-alpha.1; a built `phorj-0.2.0.vsix` binary is committed to the repo
  (P3 — artifact in git). `editors/*/README.md` teach the retired `phg fmt`-era names (counted in
  P0-2).

### Benchmark narrative (P1 — credibility)

`examples/bench/README.md` publishes "winner: Phorj (vm) — **3.23× faster than PHP**" measured
against **PHP 8.6.0-dev** [Verified: read the README, the transcript names the version]; README.md:16
repeats "already shows the VM ahead of PHP". A dev/debug PHP without opcache/JIT is not a defensible
public baseline — HN's first comment writes itself. The harness itself is honest (output-identity
gate before timing, median-of-101, process-spawn cost disclosed) — the *number* is the problem, plus
all five command lines in that README use the dead `phg bench`/`phg disasm` names. A credible story
needs: release-built PHP 8.5 (the floor) with `opcache.enable_cli=1` + JIT on/off as two columns,
≥3 workloads (scalar, object-heavy, string/collection), the spawn-per-sample caveat quantified, and
a methodology doc; publish the ratio range, not one cherry.

---

## PART 4 — Onboarding

- **README first five minutes: broken** (P0-1/P0-2 above). Beyond the breakage, staleness (all P1):
  - "std-only, with zero external crates" (line 5–6) and "no third-party runtime dependencies"
    (line 305) are **false** — argon2, regex, corosensei, ctrlc ship in Cargo.toml
    [Verified: read Cargo.toml deps section; the manifest itself numbers them FIRST/FOURTH].
  - Status table: "M2.5 🔨 in progress", "M3+ 🔲 planned" — M3, M5, M6, M-RT, M-DX are complete;
    the version is 0.5.1-alpha.1. The table undersells by ~15 shipped milestones.
  - "PHP → Phorj import is a separate future milestone" (line 280) — `phg lift` ships in the binary
    today [Verified: --help lists it]. The *bidirectional bridge is the pitch* and the README denies
    half of it.
  - "Language at a glance" lists the M1 subset only (no traits, unions, generics, decimal,
    concurrency, web, throws/Result) — a PHP dev skimming it sees a toy.
  - Transpiler description stale ("match → an instanceof chain" — native `match` shipped).
- **Install story**: source build OK; "Prebuilt binary" section references per-release binaries
  [Unverified: release assets not checkable offline]. CONTRIBUTING "std-only — no dependencies to
  fetch" false (P1, same as above).
- **Docs entry points**: ARCHITECTURE/INVARIANTS/FEATURES/ROADMAP/VISION/KNOWN_ISSUES all exist and
  FEATURES.md grepped clean of retired names [Verified: grep → 0]. CONTRIBUTING's quality-gate and
  TDD sections are accurate against ci.yml and the pre-commit hook.

---

## SHOWCASE ROADMAP (ranked by adoption impact)

1. **Fix the front door (P0, hours)** — one sweep: add `: void` to every markdown-embedded `main`,
   rename `bench/fmt/disasm/lex` → `benchmark/format/disassemble/tokenize` across the 12 stale files;
   extend `return_type_codemod.py`'s rule to `.md` fenced blocks *or* (better) add a CI doc-snippet
   check: extract every ` ```phorj ` block from README/examples-READMEs and `phg check` it — the
   same discipline the examples get, applied to the brochure. Delete the stale codemods in the same
   change.
2. **Promote the flagship (days)** — move/copy `conformance/ddd/` to `examples/flagship/` (keep the
   conformance golden pointing at it), and build the missing killer app: a ~300-line REST
   micro-service (`examples/flagship/api/`) composing Core.Http routing + Json + decimal money +
   throws/Result + Secret + a test file under `phg test` — every ingredient ships today. Feature it
   as README section 2 with its transpiled-PHP twin side by side (the bridge IS the pitch).
3. **README rewrite (days)** — truthful status table, dependency policy stated proudly (4 vetted
   deps + link to the policy spec), "Language at a glance" regenerated from STABILITY.md's stable
   list, `phg lift` promoted to a headline (both directions of the bridge).
4. **Spec + conformance formalization (1–2 weeks)** — the Part 2 architecture: `docs/spec/`
   chapters with clause IDs sourced from STABILITY.md/explain-registry, spec-tags in the 55 corpus
   programs, coverage matrix as a failing test, shared oracle module. This is the "provably-correct
   upgrade of PHP" claim made checkable by outsiders.
5. **Benchmark credibility (days)** — release PHP 8.5 + opcache/JIT columns, 3 workloads,
   methodology doc, regenerate the bench README with honest ranges; wire an optional CI job that
   re-measures the ratio against packaged PHP so the published number can't rot.
6. **Editor surface refresh (day)** — grammar additions (`empty`, `todo`, `unreachable`, `parent`,
   `html"`), bump extension to track phg versions, drop the committed .vsix, fix editors READMEs
   (covered by #1's rename sweep).
