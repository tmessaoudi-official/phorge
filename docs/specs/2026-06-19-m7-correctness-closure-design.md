# M7 — Correctness Closure — Design Spec

> **Status:** ✅ Implemented (this session, on `8c6fbb2`). Oracle + 4 P0 fixes + range guard all
> landed; ~453 tests green, clippy + fmt clean, oracle green over all examples/projects under
> `PHORJ_REQUIRE_PHP=1`. The design below is as-built (runtime-helper approach adopted).
> **Milestone:** M7 (first, non-negotiable) of the GA roadmap
> (`docs/plans/2026-06-19-phorj-ga-roadmap.plan.md`).
> **Source findings:** `~/.claude/projects/-stack-projects-phorj/REVIEW-2026-06-19.md`
> (P0-1…4, P0-ROOT, QW-13, P1-#9). Code state at spec time: master `8c6fbb2` (docs-only since
> the review; code spine still `687a7bd`), tree clean, 452 tests green, clippy + fmt clean,
> `php 8.6.0-dev` present on PATH.

---

## 1. Problem

The project's central correctness claim is **`run ≡ runvm ≡ php`, byte-identical**. Today only
**2 of the 3 legs are enforced**: `tests/differential.rs` gates `run ≡ runvm` over the
`examples/**/*.phg` glob and the example projects, but **the transpiled PHP is never executed by
the harness**. The only PHP-running tests live in `tests/cli.rs`
(`transpiled_php_runs_and_matches_interpreter`, `safe_access_transpiles_and_runs_in_php`) and
**self-skip to PASS** when `php` is absent (`cli.rs:107-139`, `:144-`). So transpiler→PHP
divergences ship silently — including inside examples that advertise three-way byte-identity.

The 2026-06-19 global review found **four live P0 silent-wrong-output bugs** in shipped examples,
plus two range correctness items:

| ID | Divergence | Site | Fix vehicle |
|----|-----------|------|-------------|
| P0-1 | integer `/` → PHP float `/` (`7/2` ⇒ `3.5`, must be `3`); LIVE in `operators.phg` | `src/transpile.rs:855` (`Div => "/"`) | `__phorj_div` helper |
| P0-2 | dropped operand grouping parens (`a-(b-c)`, `-(a+b)`, `!(a&&b)`) | `src/transpile.rs:517-536` (Unary/Binary emit) | syntactic precedence parens |
| P0-3 | `bool` interpolation `true`/`false` vs PHP `"1"`/`""`; LIVE | `src/transpile.rs` `emit_string` (`:673`) | `__phorj_str` helper |
| P0-4 | float `%` → PHP integer `%` (`5.5%2.0` ⇒ `1.5`, PHP gives `1`) | `src/transpile.rs:856` (`Rem => "%"`) | `__phorj_rem` helper |
| P0-ROOT | no PHP execution oracle; `cli.rs` PHP tests self-skip-to-PASS | `tests/differential.rs`, `tests/cli.rs:107-139` | the oracle (§3) |
| QW-13 | empty/reversed range → PHP `range()` *descends* instead of yielding `[]` | `src/transpile.rs:575-588` | `__phorj_range` helper |
| P1-#9 | large range materialization OOM/abort (exit 101), breaks EV-7 | `src/vm.rs:252-263`, `src/interpreter.rs:380-389` | shared size cap → clean fault |

The `run ≡ runvm` spine itself held up under every review lens; M7 closes the **third leg** and
the value-correctness edges adjacent to it.

## 2. Goals / Non-Goals

**Goals**
- A `php`-gated **3-way oracle** over the full `examples/**/*.phg` glob **and** the example
  projects: transpiled PHP, run by a real `php`, prints byte-for-byte what the interpreter prints
  (`= runvm`, already gated ⇒ all three identical).
- `PHORJ_REQUIRE_PHP=1` makes a missing `php` **fail, not skip** (no silent-green path survives).
- All four P0 emitter divergences fixed, enforced on first commit by the oracle.
- Range edges closed: QW-13 (empty/reversed emit) and P1-#9 (large-range clean fault, byte-identical
  on both backends, `agree_err`-gated).
- Every divergence class has a dedicated regression test (§6 matrix).

**Non-Goals (explicit)**
- **Not** introducing a transpiler-side static type system (that's M10 `Ty::Var`). P0-1/3/4 use
  runtime helpers precisely to avoid pulling type inference forward.
- **Not** fixing the `package Main` PHP-builtin-name collision (P1-#12) or promoted-field visibility
  (P1-#13) — those are PHP *fatals*, not wrong numbers, and are front-end lints scheduled for **M8**.
- **Not** building GitHub Actions CI — that's **M9**; M7 ships the test + the `PHORJ_REQUIRE_PHP`
  contract, M9 wires CI to *set* it (the seam is documented in §3.4).
- **Not** making irrational/large-magnitude floats byte-identical to PHP — a pre-existing
  KNOWN_ISSUE; examples already restrict to exactly-representable values (§3.5).

## 3. The PHP Oracle

### 3.1 Location & discovery reuse
Add the oracle to **`tests/differential.rs`** (the single correctness-spine file), reusing its
existing discovery helpers `collect_phg` (single-file glob, skips project roots), `collect_projects`,
and `find_main_phg`. No new discovery code; a newly-added example is auto-gated, same contract as the
`run≡runvm` glob. Rejected putting it in a separate `tests/oracle.rs` (would duplicate discovery).

### 3.2 Per-example oracle (single-file glob)
For each file from `collect_phg("examples")`:
1. `let expected = cmd_run(&src)?;` — the interpreter is the oracle of record. (Skip the file only
   if `cmd_run` is `Err` — a faulting program is not a runnable example; the existing
   `all_examples_match_between_backends` already asserts every example is `Ok`, so in practice all
   pass this.)
2. `let php_src = cli::cmd_transpile(&src)?;`
3. Write `php_src` to a **unique** temp path (avoid the current fixed-`sample.php` collision under
   parallel `cargo test`): `std::env::temp_dir().join(format!("phorj_oracle_{}.php", sanitized_stem))`
   — or a per-call counter. One file per example.
4. Run `php -n <tmp>` (see §3.5 for flags), capture stdout + status.
5. Assert `status.success()` (on failure, surface php **stderr** in the panic message).
6. Assert `php_stdout == expected` (byte-identical).
7. Remove the temp file.

### 3.3 Per-project oracle (multi-file projects)
For each project from `collect_projects("examples")`:
1. `let unit = loader::load(&find_main_phg(project))?;`
2. `let expected = cli::run_program(&unit.program, &unit.diag_src)?;`
3. `let php_src = cli::transpile_program(&unit.program, &unit.diag_src)?;` — this is the **namespaced**
   emit path (`namespace Acme\Util { … }` + `\Main\main()` bootstrap), a distinct code path from the
   flat single-file emit, so it must be oracle-covered too.
4. Same write → `php -n` → compare-stdout → cleanup as §3.2.

### 3.4 Gating contract (`PHORJ_REQUIRE_PHP`) — the fails-not-skips rule
A single helper `php_bin() -> Option<PathBuf>` resolves php once:
- honors an optional `PHORJ_PHP` env override (absolute path to a php binary; convenience for
  non-PATH installs), else probes `php --version` on PATH.

A single helper `require_or_skip() -> Option<PathBuf>`:
- **`PHORJ_REQUIRE_PHP=1`** (the CI / enforced mode): if php is **absent**, `panic!` — the test
  **fails** ("php required (PHORJ_REQUIRE_PHP=1) but not found on PATH / PHORJ_PHP"). This is the
  inversion of today's self-skip-to-PASS.
- **unset/empty** (dev convenience): if php is absent, emit a **loud** skip line to stderr
  (`eprintln!("SKIP php oracle: php not found — set PHORJ_REQUIRE_PHP=1 to make this a failure")`)
  and `return None`. Loud, never a silent green that reads as "ran".

**M7/M9 seam:** M7 ships the oracle + this contract; the developer runs it locally (php present on
this box). **M9's GitHub Actions** adds a PHP job that exports `PHORJ_REQUIRE_PHP=1` so CI fails if
php regresses or is missing. M7 is therefore *not* blocked on M9 — the contract is honored locally
from day one.

**Removed/migrated tests:** the two self-skipping php tests in `cli.rs`
(`transpiled_php_runs_and_matches_interpreter`, `safe_access_transpiles_and_runs_in_php`) are
**deleted** — the glob oracle subsumes them (it runs every example, including the `?.` safe-access
example `examples/guide/null-safety.phg`). The **golden-file** test that asserts the committed
`examples/transpile/demo.php` matches a fresh transpile (`cli.rs:~80-101`) is a *different* check
(drift of a checked-in artifact, no php needed) and is **kept**.

### 3.5 Determinism constraints
- The box php is `8.6.0-dev (ZTS DEBUG)`. DEBUG builds may print notices, but to **stderr**; the
  oracle compares **stdout** only. Run with `php -n` (ignore `php.ini`) for a hermetic baseline; if a
  notice ever reaches stdout in practice, switch to `php -d display_errors=stderr -d error_reporting=0`
  (decided empirically during impl by running the suite — Completion-Gate evidence).
- **Float formatting:** `__phorj_str`/PHP `(string)$float` matches Rust `format!("{x}")` only for
  exactly-representable values; irrational/large-magnitude floats (`sqrt(2.0)`, `1e20`) diverge from
  PHP's 14-digit `echo` — a pre-existing KNOWN_ISSUE. Examples already restrict to representable
  floats; the oracle inherits, not widens, that constraint. (`run≡runvm` is always identical.)
- **Bytes:** `examples/guide/bytes.phg` transpiles bytes to PHP `"\xHH"` strings — byte-boundary
  parity is auto-covered by the glob oracle; no extra fixture.

## 4. The Four P0 Emitter Fixes

### 4.0 Design decision — runtime helpers over static type inference
P0-1, P0-3, P0-4 each need to know an expression's *type* at emit time (int vs float `/`; bool vs
not in interpolation). The transpiler holds **no type information** today (`Transpiler` carries only
name sets — `funcs`, `locals: Vec<HashSet<String>>`, `cur_class_fields`; `src/transpile.rs:15-23`).
Two ways to get types:

- **(A) static** — port the compiler's `ctype(&Expr)→CTy` resolver (`src/compiler.rs:758`) into the
  transpiler with a parallel `Vec<HashMap<String,Ty>>` local-type map. Cost: a **second** operand-type
  resolver to keep in sync with the compiler's — the exact duplication M9's single-sourcing theme
  targets — *and* an inference-completeness risk (a missed local-decl site ⇒ `Other` ⇒ wrong emit ⇒
  silent P0-1 regression).
- **(B, chosen) runtime helpers** — emit PHP that inspects operand types at **PHP runtime**, mirroring
  Phorj's own type-driven value kernels (`src/value.rs`). No static inference, no duplication, no
  completeness risk; each helper maps 1:1 to a kernel / `as_display`.

**Chosen: (B).** The transpile contract is *correctness/byte-identity first*; the helpers are honest
about Phorj's type-driven `/`/`%`/display semantics. The minor loss of "idiomatic" PHP
(`__phorj_div($a,$b)` vs `intdiv($a,$b)`) is an explicit, accepted trade — a later optional pass
*may* specialize to `intdiv`/bare-`%` when a static type is provably int (deferred; not M7).

Helpers follow the existing `__phorj_unwrap` precedent exactly (`src/transpile.rs:557-566`): a
once-per-file emission gated by a `uses_*` flag, and in namespaced mode placed in the nameless global
block and called with a leading `\` (the existing `bs` pattern).

### 4.1 Emitted helper bodies
```php
function __phorj_div($a, $b) {
    // Phorj `/`: int/int truncates toward zero (intdiv); float/float is real division.
    return (is_int($a) && is_int($b)) ? intdiv($a, $b) : $a / $b;
}
function __phorj_rem($a, $b) {
    // Phorj `%`: int/int integer modulo; float/float fmod (sign of dividend, matches Rust `%`).
    return (is_int($a) && is_int($b)) ? $a % $b : fmod($a, $b);
}
function __phorj_str($v) {
    // Mirror Value::as_display (src/value.rs:106): bool -> "true"/"false"; everything else PHP-cast.
    if (is_bool($v)) { return $v ? "true" : "false"; }
    return (string)$v;
}
function __phorj_range($a, $b, $inclusive) {
    // Phorj range: empty when start > hi; never descends (PHP range() descends — QW-13).
    $hi = $inclusive ? $b : $b - 1;
    return ($a <= $hi) ? range($a, $hi) : [];
}
```
Kernel-parity rationale (verified against `src/value.rs`): Phorj int `/` = i64 truncate-toward-zero
= PHP `intdiv`; Phorj float `%` = Rust f64 `%` (truncated, sign-of-dividend) = PHP `fmod`;
`as_display` bool ⇒ `"true"/"false"`, int/float/string ⇒ their `{}`-format ≈ PHP `(string)` for
representable values. Div/mod-by-zero stays a **fault** on all backends (PHP `intdiv($x,0)` /
`$x/0` / `fmod($x,0)` and Phorj's `DivZero`/`ModZero`) — a non-example case, not oracle-covered.

### 4.2 P0-1 / P0-4 wiring (Div, Rem)
In `emit_expr`'s `Expr::Binary` arm (`transpile.rs:525-536`), special-case `Div` and `Rem` **before**
the generic `{l} {op} {r}` join (exactly as `Coalesce` is already special-cased at `:531-534`):
```rust
if matches!(op, BinaryOp::Div) { self.uses_div = true;
    return Ok(format!("{bs}__phorj_div({l}, {r})")); }
if matches!(op, BinaryOp::Rem) { self.uses_rem = true;
    return Ok(format!("{bs}__phorj_rem({l}, {r})")); }
```
`Div`/`Rem` then become `unreachable!` in `binop()` (`:855-856`), matching the existing
`Coalesce`/`Is`/`Pipe` arms. The helper output is a PHP call ⇒ a primary ⇒ never needs operand parens.

### 4.3 P0-3 wiring (bool interpolation)
In `emit_string` (`transpile.rs:673`), wrap **each interpolated expr** (`StrPart::Expr`) in the
str helper instead of bare `({code})`:
```rust
StrPart::Expr(e) => { let code = self.emit_expr(e)?; self.uses_str = true;
    chunks.push(format!("{bs}__phorj_str({code})")); }
```
Scope: only the string-interpolation/`println` path coerces (that is the only context where Phorj's
`as_display` semantics apply). A bool used in `==`, a condition, or a function argument is untouched.
For int/float/string the helper is a semantic no-op (matches today's concat); for bool it corrects
`"1"/""` → `"true"/"false"`. **No regression** to currently-passing examples (verified by the matrix).
**`src/value.rs:110` is NOT touched** — `as_display`⇒`"true"` is the correct `run`/`runvm` truth the
helper is mirroring *toward*, not away from.

### 4.4 P0-2 (precedence parens) — the one syntactic fix
Helpers can't fix precedence. Add a small predicate and wrap non-primary operands:
```rust
// A PHP "primary": emits self-contained, never needs wrapping parens.
fn is_primary(e: &Expr) -> bool {
    matches!(e, Expr::Int(..) | Expr::Float(..) | Expr::Bool(..) | Expr::Str(..)
        | Expr::Bytes(..) | Expr::Ident(..) | Expr::This(..) | Expr::Null(..)
        | Expr::Call{..} | Expr::Member{..} | Expr::Index{..} | Expr::Force{..}
        | Expr::Range{..} | Expr::List(..))
    // Force→__phorj_unwrap(...), Range→__phorj_range(...)/range(...), Call/Member/Index →
    // PHP primaries; all self-contained. Binary/Unary/If/Match/Lambda/Coalesce are NOT primary.
}
```
- **Unary** (`:517-523`): wrap `inner` in `()` when `!is_primary(expr)` ⇒ `-($a + $b)`, `!($a && $b)`.
- **Binary** generic join (`:535`): wrap each side in `()` when `!is_primary(side)` ⇒
  `$a - ($b - $c)`, `($a + $b) * $c`.

This **conservatively over-parenthesizes** (a non-primary child always gets parens even when
precedence wouldn't strictly require it). Correctness > prettiness; a precedence-table refinement that
emits the *minimal* parens is an optional later polish, explicitly deferred. Coalesce already
self-parenthesizes (`(l ?? r)`), so it's primary-safe at its own call site but is *not* in `is_primary`
(a `??` as an operand of `*` still wants wrapping) — handled because `Coalesce` returns early before
the generic join, emitting `(...)` itself; treat its returned string as already-parenthesized (no
double-wrap needed since `is_primary` is applied to the *Expr*, and a `Coalesce` Expr is non-primary
⇒ it would be wrapped → `((l ?? r))`, harmless double parens; acceptable, or special-case to avoid —
decide in impl, oracle-verified).

## 5. Range Fixes

### 5.1 QW-13 — empty/reversed range emit
Replace the inline `range($s, $e)` / `range($s, $e - 1)` emit (`transpile.rs:583-587`) with
`{bs}__phorj_range({s}, {e}, true|false)` (§4.1). PHP `range()` descends for `a > b`; Phorj yields
`[]`. The helper restores Phorj semantics. `uses_range` flag.

### 5.2 P1-#9 — large-range clean fault (both backends, lockstep)
Both `src/vm.rs:252-263` (`Op::MakeRange`) and `src/interpreter.rs:380-389` (`Expr::Range`) currently
`(start..=end).map(Value::Int).collect()` with no size guard ⇒ a huge range allocates a giant `Vec`
⇒ OOM/abort (exit 101), violating EV-7 (graceful fault, never panic on user input).

Fix, applied **identically** in both backends (a value-correctness lockstep edit, like the value
kernels):
- Compute the length with **`checked_sub`** (EV-7): `len = hi.checked_sub(start)` where `hi = if
  inclusive { end } else { end - 1 }` (guard the `end - 1` underflow too); a `None` or a `len + 1`
  (inclusive count) exceeding `MAX_RANGE_LEN` ⇒ return the shared fault.
- `const MAX_RANGE_LEN: i64 = 10_000_000;` — single-sourced (proposed in `src/value.rs` or a shared
  consts module so both backends import the *same* literal). Rationale: 10M × 16-byte `Value` ≈ 160 MB,
  a defensible ceiling well above any realistic example yet below uncontrolled OOM; **tunable**,
  documented. (Empty/reversed ranges are length 0 — never tripped.)
- Shared fault message **`"range too large"`** → classified in the differential harness as a new
  `FaultKind::RangeTooLarge` (body substring `"range too large"`), so `agree_err` confirms both
  backends fault identically. Mirrors the existing `IndexOob`/`ForceUnwrap` body-substring pattern.

This is a `run ≡ runvm` fault concern (PHP `range(0,1e9)` would also OOM, but it's a fault case, not a
runnable example ⇒ not oracle-covered). It rides M7 because it's a value-correctness divergence needing
the same `agree_err` machinery, exactly as the GA roadmap sequences it.

## 6. Test Plan & Divergence-Class Matrix

### 6.1 The oracle (P0-ROOT)
- `all_examples_transpile_and_match_php` — §3.2, glob.
- `all_example_projects_transpile_and_match_php` — §3.3, projects.
Both gated by `require_or_skip()`; under `PHORJ_REQUIRE_PHP=1` a missing php fails.

### 6.2 Per-P0 targeted tests (TDD — write red first)
| Fix | Test (Ok-output, oracle-eligible) | Asserts |
|-----|-----------------------------------|---------|
| P0-1 | `7/2`, `-7/2`, `7/-2` interpolated | run/runvm ⇒ `3`/`-3`(trunc)/`-3`; php ⇒ identical (was `3.5`) |
| P0-2 | `a-(b-c)`, `-(a+b)`, `!(a&&b)`, `a*(b+c)` | php matches run (was mis-grouped) |
| P0-3 | `"{true}"`, `"{1<2}"`, `"{!false}"`, bool local | php ⇒ `true`/`false` (was `1`/``) |
| P0-4 | `7%3` (int), `5.5%2.0` (float) | php int ⇒ `1`, float ⇒ `1.5` (was `1`) |
| QW-13 | `for i in 5..2`, `5..5`, `5..=2` | php yields `[]` (was descending) |

These land as **new examples** (e.g. fold into `examples/guide/operators.phg` / a new
`examples/guide/division.phg`) so the oracle gates them automatically (developer rule: examples ship
with features), plus a couple of inline `agree`/exact-output unit tests for the boundary values.

### 6.3 Divergence-class regression matrix
| Class | Case | Kind | Where gated |
|-------|------|------|-------------|
| int-div trunc | `7/2`, neg operands | Ok | oracle example + inline `agree` |
| float-div | `7.5/2.5` | Ok | oracle example |
| int/float mod | `7%3`, `5.5%2.0` | Ok | oracle example |
| precedence | `a-(b-c)`, `-(a+b)`, `!(a&&b)` | Ok | oracle example |
| bool display | `{true}`, `{a&&b}` | Ok | oracle example |
| empty/reversed range | `5..2`, `5..5` | Ok | oracle example |
| **i64::MIN / -1** | `(-9223372036854775807-1)/-1` | **fault** | inline `agree_err` (IntOverflow; php `intdiv` also throws — both abort, not oracle-covered) |
| **large range** | `0..2_000_000_000` | **fault** | inline `agree_err` (`RangeTooLarge`) — §5.2 |
| OOB index | `xs[5]` on len-2 | fault | existing `s1_index_oob_faults_identically` |
| neg-zero / float-fmt | `-0.0`, representable floats | Ok | oracle example *if* PHP agrees; else document in KNOWN_ISSUES (verify empirically) |
| bytes boundary | `examples/guide/bytes.phg` | Ok | oracle (auto) |
| div/mod by zero | `1/0`, `1%0` | fault | existing `error_parity_between_backends` |

Fault classes stay `run ≡ runvm` inline (`agree_err`) — they are not runnable examples, so the oracle
(stdout-parity on `Ok` programs) does not cover them. This boundary is stated so we don't mistakenly
try to PHP-gate faults.

### 6.4 Adding `FaultKind::RangeTooLarge`
Extend the `FaultKind` enum + `classify()` in `tests/differential.rs` (body substring
`"range too large"`), mirroring `IndexOob`.

## 7. Risks, Trade-offs, Deferrals

- **Idiomatic PHP loss** (helpers vs `intdiv`/bare-`%`): accepted; correctness-first. Optional
  static-specialization pass deferred (would want M10 types).
- **Helper duplication of kernel logic**: each helper is a 4-line mirror of a `value.rs` kernel /
  `as_display`. This is *intentional* re-statement across a language boundary (Rust ↔ PHP), not the
  same-language duplication M9 targets — but the mapping is logged so M9/M10 can revisit if a typed
  emit path supersedes it.
- **`MAX_RANGE_LEN = 10M`** is a magic number; documented + tunable, defensible by the ≈160 MB
  estimate. Flagged for developer veto.
- **php-in-tests cost**: ≈27 examples + 2 projects × (transpile + `php -n` spawn); DEBUG php is
  slowish but bounded — expected single-digit seconds added to the suite. Acceptable; measured at
  Completion Gate.
- **Cross-platform**: oracle is php-presence-gated; absent-php dev machines skip loudly. Fine.

## 8. Implementation Order (see the companion plan)
`docs/plans/2026-06-19-m7-correctness-closure.plan.md` sequences this as: **W1** oracle harness (red:
proves the 4 P0 bugs by failing) → **W2** the four emitter fixes + helpers (turns W1 green) → **W3**
range fixes (QW-13 + P1-#9 cap + `FaultKind`) → **W4** examples + docs (KNOWN_ISSUES, FEATURES,
CHANGELOG, MILESTONES doc-truth for the closed leg). Each wave is TDD (failing test first) and ends
green + clippy/fmt clean before commit.
