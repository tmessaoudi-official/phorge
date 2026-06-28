# Track R — Docs & learnability (GA M12)

## Track summary

Phorj's *reference* documentation is unusually strong for a pre-1.0 project: `FEATURES.md`
(capability matrix), `KNOWN_ISSUES.md` (every deferral, with rationale), `ROADMAP.md`/`VISION.md`,
five Nygard ADRs, `docs/INVARIANTS.md` + `docs/ARCHITECTURE.md`, and above all the **`examples/`
living surface** — a coverage matrix in `examples/README.md` that is byte-identity-gated by
`tests/differential.rs`, so it can never drift from what actually runs. `phg explain <CODE>` already
provides error-as-docs for ~48 of the diagnostic codes. What is **missing is the entire
learning-path superstructure that turns "exhaustive reference" into "a newcomer can learn this in an
afternoon"**: there is no narrative language reference, no guided tour / book, no PHP→Phorj
migration guide (the single highest-leverage doc given that *familiarity-first IS the adoption
strategy*), no generated stdlib (`Core.*`) API reference, no formal published grammar, and `phg
explain` has silent coverage holes (~13–20 real emitted codes have no entry, and no test enforces
completeness). The GA exit criteria already commit to a "complete language-reference doc" (M9/M12),
a "frozen 1.0 grammar" (M12), and a "semver stability commitment" (M12) — those are the load-bearing
ADOPT items; the rest are pragmatic newcomer-facing additions that the philosophy strongly favours.
None of these touch the byte-identity spine (all are pure docs/front-end), so risk is uniformly low;
the constraint is author-effort, not design.

## Gaps

| id | title | kind | fit | rec | milestone | effort |
|---|---|---|---|---|---|---|
| R-langref | Formal language reference doc | port | strong | adopt | M12 | L |
| R-tour | Guided tour / "the book" (narrative learning path) | new | strong | adopt | M12 | L |
| R-migration | PHP → Phorj migration guide (concept-mapping, gotchas) | new | strong | adopt | M12 | M |
| R-explain-coverage | `phg explain` coverage completeness + enforcement test | port | strong | adopt | M9 | S |
| R-stdlib-apidoc | Generated `Core.*` stdlib API reference | new | strong | adopt | M11 | M |
| R-grammar-ref | Published formal grammar (EBNF) as reference | port | ok | adopt | M12 | M |
| R-stability-policy | Semver / language-surface stability policy doc | port | strong | adopt | M12 | S |
| R-cheatsheet | One-page syntax cheat sheet / quick reference | new | strong | adopt | M12 | S |
| R-error-index | Browsable diagnostic-code index (web/markdown) | map | ok | adopt | M12 | S |
| R-rustdoc-internal | Rustdoc on the compiler crate (`src/lib.rs`) | new | weak | defer | v2 | M |
| R-repl-learn | REPL as a learning tool (try-it loop) | new | ok | defer | M12 | M |
| R-website | Docs website / static-site publishing | new | weak | defer | post-1.0 | M |
| R-versioned-docs | Versioned docs (per-release doc snapshots) | new | weak | reject | — | M |
| R-i18n-docs | Localized / translated docs | omit | weak | reject | — | L |
| R-interactive-playground | In-browser interactive playground (WASM) | new | weak | defer | v2 | L |
| R-video-tutorials | Video / screencast tutorials | new | weak | reject | — | M |
| R-contrib-arch-guide | Contributor architecture deep-dive (beyond ARCHITECTURE.md) | new | ok | defer | M12 | S |
| R-example-difficulty | Examples laddered by difficulty (learning order) | map | ok | defer | M12 | S |
| R-changelog-userfacing | User-facing changelog / release notes (vs the dev CHANGELOG) | new | ok | defer | M12 | S |

## Rationale for ADOPT items

**R-langref — Formal language reference.** This is an explicit GA exit criterion ("a complete
language-reference doc", M9/M12) and currently does not exist — `FEATURES.md` is a capability
*matrix*, not a reference that defines each construct's syntax, semantics, type rules, and PHP
mapping. A reference is what a developer searches mid-task ("how exactly does `match` over `T?`
narrow?"). Strong philosophy fit: a legible, provably-correct language must have a single
authoritative description of its surface, and the byte-identity spine means each entry can cite its
runnable example. Large effort because it must cover the full M-RT + mutation + M5/M6 surface, but
it is the keystone deliverable of this track.

**R-tour — Guided tour / "the book".** The reference answers "what does X do"; the tour answers "how
do I get started and build something". Rust (the Book), Go (the Tour), Gleam, and TypeScript all
treat a narrative, progressively-disclosing learning path as the primary on-ramp. Phorj has none —
a newcomer today must read `examples/README.md` and reverse-engineer the mental model. Strong fit:
"approachable on the outside" (design principle 5) is unmet without a guided path. It can be built
*entirely* from existing `examples/guide/*.phg` (already byte-identity-gated), wrapping them in prose
— so it stays drift-proof.

**R-migration — PHP → Phorj migration guide.** The single highest-leverage doc for adoption given
the core philosophy: *familiarity-first IS the adoption strategy* and Phorj : PHP :: TypeScript :
JavaScript. A PHP developer needs a concept map ("your `array` is a `List<T>` or `Map<K,V>`; your
nullable `?int` is `int?`; PHP's silent coercions are now compile errors — here's the fix") plus a
gotcha list (which already half-exists scattered in `KNOWN_ISSUES.md`'s transpile caveats). This is
exactly the doc that converts the target audience. Medium effort; strong fit. (Note: distinct from
the M8 PHP→Phorj *tool* — this is the human guide, valuable before any importer ships.)

**R-explain-coverage — `phg explain` completeness + enforcement.** Concrete, verifiable gap: of ~71
emitted diagnostic codes, ~13–20 have no `explain_text` entry (`E-BREAK-OUTSIDE-LOOP`,
`E-CONTINUE-OUTSIDE-LOOP`, `E-GENERIC-PARAM`, `E-FIELD-INIT`, `E-WITH-FIELD`/`-NONCLASS`/`-TYPE`,
`E-STATIC-INIT-CONST`/`-INIT-TYPE`/`-NO-INIT`/`-UNKNOWN`, `E-TYPE-IMPORT-{UNKNOWN,CONFLICT,BUILTIN,SHADOW}`,
`E-PKG-CASE`). The existing tests only spot-check a few codes — there is **no test that every emitted
code has an explanation**, so coverage silently rots. Error-as-docs is already a shipped pillar;
closing the holes + adding a completeness test is small effort, strong fit, and prevents recurrence.
Belongs in M9 (engineering-hygiene / doc-SSOT) where the no-drift discipline lives.

**R-stdlib-apidoc — Generated `Core.*` API reference.** The `Core.Console`/`Math`/`Text`/`File`/
`Bytes`/`Html`/`List`/`Map`/`Set` modules are the surface a working developer calls daily, yet their
signatures live only in `src/native.rs` registry entries and scattered KNOWN_ISSUES notes. A
generated reference (driven from the registry — single-sourced, so it can't drift, matching the
project's existing "single-source then generate" discipline) gives each native its signature,
semantics, and PHP-erasure mapping. Medium effort; strong fit; sequenced at M11 when stdlib breadth
(`core.list`/`json`, `Map`/`Set`) lands so it documents the final 1.0 stdlib scope.

**R-grammar-ref — Published formal grammar.** A GA exit criterion already names a "frozen 1.0
grammar" and M12 lists a "TextMate / tree-sitter grammar". An EBNF grammar in the reference serves a
distinct purpose: it is the authoritative answer to parsing/precedence questions (e.g. `A | B & C` ≡
`A | (B & C)`, `A | B?` ≡ `A | (B?)` — both currently documented only in CLAUDE.md/KNOWN_ISSUES) and
the contract a stability promise is made against. Medium effort; ok-to-strong fit; M12.

**R-stability-policy — Semver / stability commitment.** Explicit GA exit criterion. A 1.0 language
must document what "stable" means: which surface is frozen, what can change in minor vs major, and
the deprecation process. Small effort (a policy doc, not code), strong fit — it is part of the
trustworthiness story the VISION leads with.

**R-cheatsheet — One-page syntax cheat sheet.** A high-density quick reference (every operator,
keyword, type-form, and `Core.*` call on one page) is the most-used artifact for a developer who
already knows the concepts and just needs the syntax — directly serving "familiarity, recognition
not replication". Small effort, strong fit; derivable from the reference.

**R-error-index — Browsable diagnostic-code index.** `phg explain` is great in-terminal, but a
flat markdown/web index of every code (with cause + fix + example) is searchable and linkable from
error output. Once R-explain-coverage single-sources the text, this index is a near-free generated
artifact (map kind — the data already exists in `explain_text`). Small effort; ok fit; M12.

## Critic pass

Read the full shipped state before judging: `FEATURES.md`, `KNOWN_ISSUES.md`, `docs/MILESTONES.md`,
`ROADMAP.md`, `VISION.md`, `docs/INVARIANTS.md`, `CLAUDE.md`, the GA roadmap plan
(`docs/plans/2026-06-19-phorj-ga-roadmap.plan.md`), `README.md`, `examples/README.md`, `src/cli.rs`
(explain), `src/lexer.rs` (comment/doc-comment surface). Verified counts below.

### Verification of the original list

- **R-explain-coverage is real and the numbers check out [Verified].** Emitted codes in `src/`:
  61 distinct `E-*`/`W-*` (`grep -rhoE '"E-[A-Z0-9-]+"|"W-[A-Z0-9-]+"' src | sort -u`). `explain_text`
  arms in `src/cli.rs`: **47** (`grep -cE '"E-[A-Z0-9-]+"\s*=>' src/cli.rs`). So **~14 codes have no
  explanation** — E-ASSIGN-*, E-HOOK-*, E-STATIC-*, E-WITH-*, E-VIS-*, E-DUP-DEF, E-VENDOR-*,
  E-GENERIC-PARAM, E-NAME-CASE, E-IFACE-*, etc. And the GA plan **already lists this exact task**
  (`docs/plans/…ga-roadmap.plan.md`: "`phg explain` known-codes list derived from `explain_text` (no
  omissions)" under M9). So R-explain-coverage is not only correct, it is *already an M9 commitment* —
  keep ADOPT, M9; the researcher slightly over-claims novelty but the recommendation stands.
- **No mis-listings found [Verified].** Each ADOPT item was checked against the shipped docs: there is
  no narrative reference, tour, migration guide, generated stdlib API ref, EBNF grammar, stability
  policy, cheat sheet, or error index today (the repo has `FEATURES.md` matrix + `KNOWN_ISSUES.md` +
  ADRs + `examples/README.md` only). The GA plan's M9/M12 criteria *commit* to langref + grammar +
  semver policy but they are unbuilt checkboxes, so listing them as gaps is correct, not double-listing.
  **removed_mislisted = 0.**
- **Philosophy sanity-check passes.** Every ADOPT item is pure docs/front-end (no byte-identity-spine
  risk) and squarely serves familiarity-first / "approachable on the outside" (design principle 5).
  The DEFER/REJECT calls (rustdoc-internal, website, versioned-docs, i18n, video, playground) are all
  correctly graded weak-fit / premature for a pre-1.0 single-dev project. No PL-theory maximalism to cut.

### Newly-found gaps (missed by the first pass)

| id | title | kind | fit | rec | milestone | effort |
|---|---|---|---|---|---|---|
| R-doc-comments | Phorj doc-comment syntax (`/** */` docblocks) + checker awareness | port | strong | adopt | M11 | M |
| R-getting-started | Standalone "Getting started / installation" page (5-minute first program) | new | strong | adopt | M12 | S |
| R-transpile-contract-doc | "How Phorj maps to PHP" reference (the transpile contract, per-construct) | new | strong | adopt | M12 | M |
| R-doctest | Doc examples are executable (doctest / example-as-test for the docs) | map | ok | adopt | M9 | S |
| R-faq-troubleshooting | FAQ / troubleshooting ("why won't this compile / why does the VM reject what the interpreter accepts") | new | ok | adopt | M12 | S |
| R-readme-badges-status | README accuracy/status discipline (test count, milestone state, feature matrix freshness) | map | ok | defer | M9 | S |
| R-man-pages | `man phg` / shell-completion docs for the CLI | new | weak | defer | M12 | S |

**R-doc-comments — Phorj doc-comment syntax. [Verified: `src/lexer.rs` has only `skip_line_comment`
(`//`) + `skip_block_comment` (`/* */`); no `///`/`/**` token; the `///` hits in src are Rust
compiler doc-comments, not a Phorj construct.]** This is the single biggest *missed* learnability
gap and it is **PHP-shaped to its core**: PHPDoc `/** … */` docblocks (`@param`/`@return`/`@throws`/
`@deprecated`) are the de-facto universal PHP source-documentation convention — every IDE, phpDocumentor,
and static analyzer (PHPStan/Psalm) consume them. Phorj having *no* in-source doc-comment means user
code cannot be self-documenting, the planned LSP has nothing to surface on hover, and the
stdlib-apidoc story (R-stdlib-apidoc) stops at *native* functions with no path to documenting *user*
library packages. The legible-Phorj form: a `///`-or-`/** */` doc-comment lexed as a trailing-attached
token on the following declaration, **erased before the backends** (front-end-only, transpiles to a PHP
`/** */` docblock — a 1:1 idiomatic-PHP mapping, zero byte-identity risk). Strong fit; it is the
foundation R-stdlib-apidoc and the future LSP both want. Sequence at M11 (alongside stdlib breadth) so
the stdlib reference can be generated from the same doc-comment pipeline user code uses. **This is
arguably more load-bearing than several of the L-effort narrative docs and should not have been omitted.**

**R-getting-started — Standalone getting-started page. [Verified: `README.md` has `## Install` +
`## Quick start`, but no dedicated first-run page; no top-level GETTING-STARTED.md or `docs/` entry.]**
The README quick-start is buried mid-file under Status/How-it-works; a newcomer landing from a link
needs a *single* "install → write `hello.phg` → `phg run` → see output → next step" page. Distinct from
R-tour (the multi-chapter book) — this is the 5-minute on-ramp that decides whether someone tries Phorj
at all. Small effort, strong familiarity-first fit; the content is mostly extractable from the README's
Install + Quick start sections plus `examples/hello.phg`. M12.

**R-transpile-contract-doc — "How Phorj maps to PHP" reference. [Inferred: the contract
`Phorj : PHP :: TypeScript : JavaScript` is stated everywhere in CLAUDE.md/VISION and is the project's
defining promise, but there is no single doc that shows, per construct, the emitted PHP — only scattered
KNOWN_ISSUES transpile caveats + the `examples/transpile/` bridge.]** This is *the* doc that makes the
central pitch concrete and is squarely a PHP-relevance bullseye: a PHP dev's first question is "what
does my Phorj become?". It differs from R-migration (PHP→Phorj, helping a PHP dev *write* Phorj) —
this is Phorj→PHP, showing the *output* (how `int?` becomes `?int`, how a union becomes `A|B`, how
generics erase to `mixed`, how packages become `namespace` blocks). It is also the human-readable
companion to ADR-0004 (brace-namespace emission). Medium effort, strong fit. M12.

**R-doctest — Executable doc examples. [Inferred: `examples/` is byte-identity-gated by
`tests/differential.rs`, but a *narrative doc* (langref/tour/migration) embeds prose-adjacent code
snippets that are NOT gated — they can rot exactly like the README's `phorj <cmd>` drift the GA plan
already had to fix (QW-9).]** The project's entire credibility rests on "docs can't drift from runnable
reality." The adopted L-effort docs (R-langref, R-tour) will contain dozens of inline code blocks; if
those are hand-typed prose they reintroduce the drift the examples-gating eliminated. The discipline:
every doc code block is either an *include* of a real `examples/**/*.phg` (already gated) or extracted
and run by a test. Small effort if folded into the doc-authoring workflow; ok-to-strong fit (it is the
no-drift invariant applied to docs). M9 (doc-SSOT discipline) so it is in place *before* the big docs
are written, not retrofitted.

**R-faq-troubleshooting — FAQ / troubleshooting page. [Inferred: KNOWN_ISSUES.md documents deferrals
with rationale, but there is no newcomer-facing "I hit X, here's why and the fix" page; several
foot-guns are real and recurring — the CTy-operand trap (VM rejects `id(7)+1` that the interpreter
accepts), the `V()` zero-payload-variant call form, the `E-SHADOW-IMPORT` "don't name a local `text`"
gotcha.]** These are exactly the surprises a new user *will* hit, scattered across CLAUDE.md memory and
KNOWN_ISSUES. A short curated FAQ ("why does the VM reject this?", "why must I write `V()` in a match?")
turns confusing failures into learnable lessons — directly serving "removes surprises." Small effort,
ok fit (a curated subset of KNOWN_ISSUES + the gotcha memories, reader-oriented). M12.

**R-readme-badges-status — README/status freshness discipline.** The GA plan already records repeated
doc-drift findings (hard-coded test counts, M5/M6 status, CLI table) needing a milestone-close sweep
(P1-#21 + the QW doc items). A *standing* discipline (status/counts derived or checked, not hand-typed)
prevents recurrence. **Defer** — it is largely subsumed by the M9 doc-SSOT sweep the plan already owns;
noted here so the learnability track is aware the drift problem is being handled at M9, not in this track.

**R-man-pages — `man phg` + shell completions. [Verified: `src/cli.rs` has rich `--help` text per
command, but no generated man page or completion scripts.]** A nicety for CLI discoverability
(`man phg`, tab-completion in bash/zsh/fish). Weak fit for a *language* learnability track (it is CLI
tooling, overlaps Track Q/T), low priority pre-1.0 — the per-command `--help` already covers the need.
**Defer** to M12 tooling; listed for completeness.

### Notes on the long tail (considered, deliberately NOT added)

- **Worked-out example *applications* / "cookbook".** Subsumed by the existing `examples/realworld/`
  set (ledger/library/shop/rpg) + R-tour; not a distinct gap.
- **API stability *test* (surface-snapshot test).** A test asserting the frozen 1.0 surface hasn't
  changed is real, but it is an *engineering* artifact (M9/M12 hygiene), not a docs/learnability item —
  belongs in another track's scope, not R.
- **Glossary of Phorj terms.** Low marginal value over R-langref + R-cheatsheet; would fold into the
  reference's front-matter. Not a standalone gap.

**Summary: new_found = 7, removed_mislisted = 0.**
