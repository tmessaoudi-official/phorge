# Track C — DX & syntax ergonomics — roadmap gap audit

## Track summary

Phorj already has a surprisingly mature **diagnostics** core for a pre-1.0 language: caret-underlined
spans, stable error codes, `phg explain <CODE>`, an edit-distance ≤2 "did you mean" suggester (unknown
idents, unknown fields/methods, casing fixes), `phg check --json` (stage/severity/message/line/col/code/hint
for editors), and a non-fatal **warning channel** (`W-FORCE-UNWRAP`). Trailing commas are already accepted
everywhere (calls, lists, params, type lists). What is **entirely absent** is the *external developer-tooling*
layer that a PHP/TypeScript developer expects day-to-day: there is **no LSP**, **no formatter** (`phg fmt`),
**no REPL**, **no scaffolder** (`phg new`), **no doc generator**, **no watch mode**, and **no doctests**.
The ROADMAP parks all editor/formatter work in "M7 — Tooling" with no decomposition, so the LSP/`fmt`
surface is a single undifferentiated bullet rather than a sequenced plan. On the *syntax* side the language
is already clean; the one genuine, philosophy-perfect micro-gap is **numeric separators** (`1_000_000`), a
PHP 7.4 feature that strips at lexing (zero runtime/transpile impact) and is pure legibility. Static
analysis the checker is structurally positioned to provide cheaply — **unused-import** and **unused-local /
dead-code** lints — is missing and would ride the existing warning channel. Most items here are `port`
(bring a PHP/TS DX affordance across) or `new` (LSP/REPL/doc-gen are beyond-PHP tooling). Recommendations
are gated hard by the philosophy: adopt the legibility wins and the tooling a PHP dev assumes exists; defer
the heavyweight tooling (LSP, doc-gen) behind a sequencing decomposition of M7; reject nothing outright
except syntax that would *add surprise* (e.g. significant-whitespace or macro sugar).

## Gap table

| id | title | kind | fit | rec | milestone | effort |
|---|---|---|---|---|---|---|
| C-numsep | Numeric separators `1_000_000` | port | strong | adopt | M3 (S-ergonomics) | S |
| C-fmt | `phg fmt` canonical formatter | new | strong | adopt | M7 (new M7.1) | L |
| C-fmt-check | `phg fmt --check` / `--diff` (CI gate) | new | strong | adopt | M7 (with C-fmt) | S |
| C-unused-import | Unused-import lint (`W-UNUSED-IMPORT`) | new | strong | adopt | M3 / M8 (warning channel) | S |
| C-unused-local | Unused-local / unreachable-code lint | new | strong | adopt | M3 / M8 (warning channel) | M |
| C-repl | `phg repl` interactive shell | new | ok | adopt | M7 (new M7.2) | M |
| C-new | `phg new` project scaffolder | port | strong | adopt | M5/M7 (new M7.3) | S |
| C-lsp | Language server (`phg lsp` / LSP) | new | strong | defer | M7 (new M7.4) | L |
| C-editor-clients | VSCode + PhpStorm thin clients | new | ok | defer | M7 (after C-lsp) | M |
| C-doc-gen | `phg doc` API doc generator + doc-comments | new | ok | defer | M7 (new M7.5) | L |
| C-doctest | Doctests (runnable `///` examples) | new | ok | defer | M7/M9 | M |
| C-watch | `phg watch` / `phg check --watch` | new | ok | defer | M7 | S |
| C-api-didyoumean | "did you mean" for stdlib/native APIs | port | strong | adopt | M3 / M8 | S |
| C-fix-it | Machine-applicable fix-its (`--fix`) | new | ok | defer | M7 (after C-lsp/C-fmt) | M |
| C-interp-line | Fix line=1 reporting inside `"{…}"` | map | strong | adopt | M3 / M8 (bug, not feature) | M |
| C-explain-list | `phg explain --list` (browse all codes) | port | ok | adopt | M3 (trivial) | S |
| C-heredoc | Heredoc / flexible multiline string sugar | omit | weak | reject | — | — |
| C-shorthand-arrow | Single-expr fn shorthand beyond lambdas | omit | weak | reject | — | — |
| C-color-diag | Colorized diagnostics (opt-in) | new | ok | defer | M7 | S |
| C-init-config | `phg init` (phorj.toml + .gitignore) | port | ok | adopt | M5/M7 (with C-new) | S |

## Rationale per ADOPT item

**C-numsep — Numeric separators `1_000_000`.** The single best syntax win in this track. PHP 7.4 added
underscore separators in all numeric literals and strips them at the lexing stage (confirmed: each `_` must
sit directly between two digits), so the runtime is unaffected — which maps *perfectly* onto Phorj's
constraints: `scan_number` in `src/lexer.rs` is the only file that changes, and the underscore is dropped
before the token carries a value, so the interpreter/VM/transpiler and the PHP output are byte-identical by
construction (no new `Op`, no parity surface). A PHP dev recognizes it instantly and it makes long literals
provably more legible. Pure adoption-strategy fit; effort is genuinely S (one lexer function + invalid-usage
rejection: trailing `_`, doubled `__`, `_` adjacent to `.`).

**C-fmt / C-fmt-check — Canonical formatter.** A PHP/TS developer expects a formatter (`php-cs-fixer`,
`prettier`, `gofmt`). Phorj has none. Because Phorj is single-developer and pre-1.0, the *gofmt* model —
**one canonical style, no options** — is the highest-leverage, lowest-bikeshedding choice and fits the
"removes surprises" philosophy (formatting is never a debate). It reuses the existing lexer/parser; the only
new surface is a pretty-printer over the AST (comments are the hard part and the L driver). `--check`/`--diff`
is a trivial follow-on that gates CI (M9 already enforces the oracle, so a fmt gate slots in naturally). This
is the tooling item most likely to be *missed every day*, so it ranks above the LSP.

**C-unused-import / C-unused-local — Static-analysis lints.** Phorj already has the two things needed: a
checker that resolves every import and every local binding, and a **warning channel** (`W-FORCE-UNWRAP` is
the precedent). An import that is never referenced, and a local that is bound but never read, are both
cheaply detectable at the existing resolution chokepoints and surface as non-fatal `W-UNUSED-IMPORT` /
`W-UNUSED-LOCAL` warnings (never gating the build — matching the warning-channel contract). This is a
"provably clearer code" win the philosophy explicitly rewards, and it is front-end-only (zero parity risk).
Unused-import is S (one pass over the import map); unused-local + simple unreachable-after-`return` is M
(needs a use-set walk per scope, but the compiler already tracks dead-code-after-`return` for height).

**C-repl — Interactive shell.** A REPL is the canonical "tiny path from idea to running program" affordance
(PHP has `php -a`). Phorj already has `-e '<code>'` inline eval and the testable `cli` module, so a
read-eval-print loop that wraps a synthesized `main()` per line and runs it on the VM is mechanically within
reach. It is `ok` fit rather than `strong` because Phorj's immutable-by-default, package-mandatory model
makes a stateful REPL slightly awkward (each line is a fresh program unless state is threaded), but the
value for learning/exploration is high and it is genuinely M effort.

**C-new / C-init-config — Scaffolding.** `phg new <name>` (a `phorj.toml` + `src/main.phg` + `.gitignore`
skeleton) and `phg init` (add a manifest to an existing dir) are the Go/Cargo affordance a packaged language
needs — and Phorj made packaging *mandatory* (M5), so the friction of hand-writing `phorj.toml` +
`package Main;` + folder=path is real for newcomers. Both are S, ride the existing manifest writer, and
directly serve adoption. Fits cleanly alongside M5's project model or in the M7 tooling slice.

**C-api-didyoumean — "did you mean" for stdlib/native APIs.** The checker already suggests the nearest
in-scope identifier and the nearest field/method (edit distance ≤2). It does **not** yet suggest the nearest
*native* when a `Core.*` call is misspelled (`Core.Text.uppr` → "did you mean `upper`?") — the native
registry is keyed by `(module, name)` and is fully enumerable, so the same edit-distance helper extends to
it trivially. A high-value, low-effort sharpening of the existing diagnostic surface; S.

**C-interp-line — Fix line=1 in interpolation.** Documented in KNOWN_ISSUES: a fault or type error inside a
`"{…}"` interpolation reports line 1 because the sub-lexer resets position, so the S0.4 caret underlines the
wrong place. This is a *legibility regression in an existing feature* (`map`, not a new feature) — it
undermines the "sharp diagnostics" promise. Worth fixing by threading the outer offset into the sub-lexer;
M effort. Adopt because it degrades a shipped DX guarantee.

**C-explain-list — Browse all diagnostic codes.** `phg explain <CODE>` exists; a `phg explain --list` (or
`--all`) that prints every code + one-line summary makes the dictionary discoverable instead of requiring
the dev to already know the code. Trivial (the dictionary is already a static table); S.

## Notable defers and rejects

- **C-lsp / C-editor-clients / C-doc-gen / C-doctest / C-fix-it — defer to a decomposed M7.** All are
  `strong`/`ok` fit and clearly *wanted* (the memory note records the developer's VSCode+PhpStorm goal), but
  each is L-or-M and the ROADMAP currently collapses them into one "Editor/LSP, formatter" bullet. The real
  gap is **sequencing**: M7 should be split into ordered slices (M7.1 `fmt` → M7.2 `repl`/`new` → M7.3 LSP
  core reusing the checker's `Diagnostic`/`check --json` surface → M7.4 thin editor clients → M7.5 doc-gen).
  The LSP is the keystone (hover/go-to-def/find-refs/rename/completion all reuse the checker), but it should
  follow `fmt` because formatting is a smaller, higher-frequency win and de-risks the AST-printer the LSP's
  rename/quick-fix later needs.
- **C-heredoc / C-shorthand-arrow — reject.** Phorj strings are already multi-line (lexer.rs:180), so PHP
  heredoc/nowdoc add a *second* multiline syntax with new surprises (indent-stripping rules, `<<<`/`<<<'`)
  for no capability gain — it violates "removes surprises, never adds them." Likewise, lambdas (`fn(x) => e`)
  already provide the single-expression shorthand; a further top-level `function f(x) => e` shorthand is
  marginal sugar that fragments the one function-declaration form. Both fail the surprise-budget test.

Sources: [PHP RFC: numeric_literal_separator](https://wiki.php.net/rfc/numeric_literal_separator), [PHP 7.4 underscore numeric separator (php.watch)](https://php.watch/versions/7.4/underscore_numeric_separator)

## Critic pass

Re-checked every listed item against `FEATURES.md`, `KNOWN_ISSUES.md`, the project `CLAUDE.md`
milestone log, and direct `grep` of `src/`. **No mis-listings found** — every "absent" claim holds:
`grep` of `src/cli.rs`/`src/main.rs` confirms the CLI surface is
`run|runvm|check|parse|lex|transpile|disasm|bench|build|vendor|serve|explain` with **no**
`fmt`/`repl`/`new`/`init`/`watch`/`doc`/`lsp` command; `scan_number` in `src/lexer.rs` (lines 59–100)
has **no** underscore handling (C-numsep valid); `cmd_explain` (cli.rs:476) has no `--list`/`--all`
path (C-explain-list valid); `nearest_name`/`levenshtein` (checker.rs:215, 3770) power did-you-mean
for idents/fields/methods but the native registry is **not** wired into it (C-api-didyoumean valid);
the interpolation line=1 bug is still in KNOWN_ISSUES "Behavioral quirks" (C-interp-line valid).
`removed_mislisted = 0`.

**Newly-found items** (long tail the first pass missed):

| id | title | kind | fit | rec | milestone | effort |
|---|---|---|---|---|---|---|
| C-int-base | Integer base literals `0x1F` / `0b1010` / `0o17` | port | strong | adopt | M3 (S-ergonomics, with C-numsep) | S |
| C-completions | Shell completions (`phg completions bash\|zsh\|fish`) | new | ok | defer | M7 (with C-new/C-init) | S |
| C-explain-fuzzy | `phg explain <typo>` did-you-mean for unknown codes | port | strong | adopt | M3 (with C-explain-list) | S |
| C-numsep-bases | Numeric separators in *all* bases (hex/binary/octal/float) | port | strong | adopt | M3 (rolled into C-numsep/C-int-base) | S |

**C-int-base — Integer base literals (hex/binary/octal).** A genuine syntax-legibility gap the first
pass entirely missed. PHP has `0x`, `0b`, **and** `0o`/`0` octal; Phorj's `scan_number`
(`src/lexer.rs:59`, verified) only parses decimal — `0x1F` lexes `0` then errors on `x`. A PHP dev
reaches for `0xFF` / `0b1010` for masks and flags constantly; the value the token carries is a plain
`i64` (parse with the right radix), so interpreter/VM/transpiler and the PHP output are byte-identical
by construction (no new `Op`, no parity surface) — the exact same "lexer-only, zero runtime impact"
shape as C-numsep, and it should ship in the **same ergonomics slice**. Strong philosophy fit (pure
familiarity, removes a surprise: a PHP dev is surprised `0xFF` *doesn't* work). Effort S. Bundle with
C-numsep so the separator logic (`_` between digits) and the base-prefix logic land together — and so
separators work *inside* a hex/binary literal too (`0xFF_FF`, `0b1010_1010`), matching PHP exactly
(this is the C-numsep-bases row, folded in here rather than treated as a distinct feature).

**C-explain-fuzzy — did-you-mean for misspelled diagnostic codes.** `cmd_explain` (cli.rs:476) errors
flatly on an unknown code with no suggestion, yet the project *already* has a `levenshtein` helper
(checker.rs:3770) and the explain dictionary is a fully-enumerable static table. `phg explain
E-UNKOWN-IDENT` → "did you mean `E-UNKNOWN-IDENT`?" is the same trivial sharpening as C-api-didyoumean,
on the same machinery, and pairs naturally with C-explain-list (both make the code dictionary
forgiving + discoverable). Strong fit, effort S. Adopt alongside C-explain-list / C-api-didyoumean as
one small "diagnostics polish" bundle.

**C-completions — shell completions.** `cargo`, `rustup`, `gh`, `kubectl` all ship
`<tool> completions <shell>`; a packaged CLI a PHP/TS dev installs is expected to tab-complete its
subcommands and flags. The CLI subcommand/flag set is static and enumerable, so emitting bash/zsh/fish
completion scripts is mechanical (a hand-written generator stays zero-dep — no `clap`). `ok` fit
(quality-of-life, not a language surprise) and **defer**: lower frequency than fmt/lint and best
bundled into the M7 tooling decomposition alongside C-new/C-init (the "first five minutes" newcomer
affordances). Effort S.

**Deliberately NOT added** (considered and rejected as out-of-track or surprise-budget failures):
significant-whitespace / off-side rule (rejected — adds surprise, no PHP precedent); a macro system
(beyond-PHP, not DX-ergonomics, and the `html"…"`/named-tag macros are an *internal* mechanism, not a
user surface); a `#[deprecated]` attribute + lint (Phorj has no attribute syntax at all — that is a
language-design item for another track, not a DX micro-gap); custom operators (purism, rejected by
philosophy). Per-file `// phg-disable W-…` lint suppression was considered as a companion to the
warning channel but is premature with only two lint codes shipped — note it as a *follow-on* to the
C-unused-* lints, not a standalone gap.
