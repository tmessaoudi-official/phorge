# M-DX — Error Experience & Build Profiles (design)

> **Status:** design-locked 2026-07-01 (developer approved order + scope). NOT yet planned/implemented.
> Next step: spec review → `writing-plans` → slice-by-slice build.
> **Milestone id:** M-DX. **Author flow:** superpowers:brainstorming.

## 1. Motivation

The developer's directive: *the diagnostics / LSP / debugger must be flawless — give exactly the right
information and the exact fix instruction on error — and add a **secure, opt-in way to inspect runtime
values when an error occurs**. This must be secure and well thought.*

"Flawless" is not buildable or verifiable, so this milestone reframes it as **measurable diagnostic
quality with a regression net**, plus a **new runtime post-mortem value-inspection capability** and an
**interactive debugger**, all gated by a new **build-profile** system so none of the value-exposing
machinery can ship in production.

Grounding evidence (from the W1 enforcement audit run the same day — diagnostics are demonstrably
*not* flawless today):
- **14 real checker error codes have no `phg explain` entry** (`E-WITH-TYPE`, `E-NEW-REQUIRED`,
  `E-GENERIC-PARAM`, `E-STATIC-*`, `E-UFCS-AMBIGUOUS`, `E-BREAK-OUTSIDE-LOOP`,
  `E-CONTINUE-OUTSIDE-LOOP`, `E-DECL-NONFOREIGN`, `E-DECL-PACKAGE`, `E-NEW-ON-NONCONSTRUCT`,
  `E-STATIC-INIT-TYPE`, `E-STATIC-NO-INIT`, `E-STATIC-UNKNOWN`, `E-WITH-*`).
- At least two diagnostics carry **no code at all** ("type `C` is already defined"; "type `Box`
  expects 1 type argument, got 2").
- A genuine **soundness hole**: a return-type-incompatible override type-checks clean and then faults
  at runtime (the type system lies). See W1 findings B/C/D (LSP return covariance, duplicate enum
  variant, duplicate static field) — folded into S1.

## 2. The keystone principle (governs every slice)

> **A build profile may change diagnostics, observability, and side-channels ONLY — never observable
> program behavior or output. `run ≡ runvm ≡ real PHP` must hold *identically* under both Dev and
> Release.**

Corollaries:
- **Assertions are always-checked; they are NOT stripped in Release** (unlike C `NDEBUG` / Rust
  `debug_assert!`). Stripping an assert changes control flow → breaks the spine. Release may only make
  the *failure diagnostic* terser. This is a deliberate, justified divergence from C/Rust.
- No profile-conditional semantics (e.g. no "checked overflow in Dev, wrapping in Release" — overflow
  is already always-checked and faults byte-identically, `value.rs:659 checked_add`).
- All value-exposing/observability output goes to **stderr**, never stdout, and lives **outside** the
  `tests/differential.rs` + PHP-oracle correctness spine.
- None of the value-exposing machinery is transpiled to PHP (transpile is a *bridge, not a runtime* —
  see `[[transpile-is-a-bridge-not-a-runtime]]`). It is an interpreter+VM-only concern.

## 3. Architecture — six slices, dependency-ordered

| Slice | Name | Depends on | Delivers |
|---|---|---|---|
| **S1** | Diagnostics quality | — | The "exact info + one exact fix" bar + a golden-diagnostic CI corpus; closes audit gaps |
| **S0** | Build profiles (`Dev`/`Release`) | — | Secure-by-construction dev/prod gate; folds in `serve --dev` |
| **S2** | Secure value renderer | S0 | Shared substrate: `Secret`-redacting, capped, deterministic `Value`→text |
| **S3** | Value-dump on fault | S0, S2 | Opt-in auto post-mortem (faulting-frame locals + backtrace headers); Dev-only |
| **S4** | Assertions | S2 | `assert` language feature; always-checked; profile-gated failure richness |
| **S5** | Interactive debugger | S0, S2, S3 | Interpreter-only pause/step/inspect engine; **REPL + DAP** frontends |

**Order rationale (developer-approved):** S1 first — it has zero dependencies, is already in flight
(W1 audit), delivers value immediately with no new infra, and *sets the diagnostic-quality bar the
later slices inherit*. S0 is built just-in-time before its first consumer (S2), not speculatively
first. Everything after S0 is dependency-forced. S3↔S4 are swappable; the headline value-dump stays S3.

## 4. Slice designs

### S1 — Diagnostics quality (foundation)

**Goal:** every diagnostic carries `{stable code, exact caret span, one concrete fix instruction}`, and
every code self-documents via `phg explain`. A golden corpus makes any regression a CI failure.

- **Golden-diagnostic corpus** (`tests/diagnostics/` or `conformance/diagnostics/`): each case is a
  `.phg` that must fail + a sibling `.expected` pinning the exact rendered diagnostic (code, span
  caret, message, fix hint). Mirrors the `conformance/` golden-output pattern. CI-gated.
- **Coverage ratchet:** a test asserts every `E-*`/`W-*` code emitted anywhere in `src/` has (a) a
  `phg explain` entry and (b) ≥1 golden corpus case. Fails if a new code is added without both.
- **Close audit gaps:** add the 14 missing `explain` entries; give the two uncoded diagnostics codes
  (`E-DUP-TYPE`, `E-TYPE-ARG-COUNT` or similar); fix the soundness holes B/C/D (LSP return covariance
  = new `E-OVERRIDE-SIG`; duplicate enum variant = `E-DUP-VARIANT`; duplicate static field).
- **LSP:** inherits all of the above for free (one checker `Diagnostic` surface feeds LSP + CLI).
  Verify hover/quick-fix surfaces the `fix` hint.
- **Testing:** golden corpus + the coverage ratchet + should-error tests for B/C/D. No runtime change,
  so byte-identity spine untouched.

### S0 — Build profiles

**Goal:** a first-class `Profile { Dev, Release }` that everything env-sensitive gates on, chosen at
build/run time (compile-time per `[[config-must-be-compile-time]]`), never a runtime env var.

- **Model:** `enum Profile { Dev, Release }`. Determined by the CLI verb / build flag:
  `phg run`/`runvm`/`debug` = **Dev**; `phg build` = **Release by default**, `phg build --debug` =
  Dev-in-artifact (opt-in). A `phg run --release` / `phg build --dev` may be offered for parity.
- **Secure-by-construction:** a Release artifact has the value-exposing machinery *absent*, not merely
  flagged off. Prefer compiling it out (Rust `cfg`/feature or a build-time constant the embedded-run
  path reads) so no attacker-controlled input can enable it.
- **Fold in the ad-hoc switch:** replace the hand-plumbed `serve(dev: bool)` with the profile
  (`serve` Dev → rich HTML error page; Release → bare 500). Consider Release `phg build` embedding
  **bytecode not readable source** (IP/tamper) — flagged, may be its own follow-up.
- **Testing:** unit tests that Release gates each consumer; a test that no runtime env var flips a
  Release artifact into Dev behavior.

### S2 — Secure value renderer

**Goal:** a single `Value → String` renderer used by S3, S4, S5, safe by construction.

- **Secret redaction [Verified design]:** `Secret<T>` is an injected wrapper class (runtime
  `Instance{class:"Secret"}` with a private `value` field — no runtime marker). The renderer
  special-cases an instance whose class is `Secret` and renders `Secret(<redacted>)` without
  descending into `value`. Mirrors the transpiler's existing `#[\SensitiveParameter]`. Redaction
  boundary = the `Secret` wrapper (same as the type system's guarantee; once `expose()`d, `W-SECRET`
  already warns).
- **Caps:** max depth, max element count per collection, max total bytes — truncate with `…` markers.
- **Determinism:** stable ordering (Maps/Sets are already insertion-ordered `Rc<Vec<…>>`); never render
  addresses, `Rc` counts, or hash order. Must be reproducible for golden testing.
- **Output:** stderr-only; a compact, human-readable form (name = value). Never stdout.
- **Testing:** unit tests over every `Value` variant incl. cycles-are-impossible (immutable+acyclic
  heap), Secret redaction, cap truncation, determinism.

### S3 — Value-dump on fault (the headline feature)

**Goal:** when a fault occurs with the dump enabled (Dev + opt-in flag), auto-emit a secure
post-mortem to stderr.

- **Enablement:** Dev profile + explicit opt-in (`--dump-on-fault` or similar); off by default even in
  Dev (opt-in, per the security posture). Absent entirely in Release.
- **Capture (developer-chosen default):** **faulting frame's locals + the faulting expression's
  operands, PLUS a backtrace of every frame up the stack showing only function name + line (no
  locals).** Deep at the fault, shallow (no value leak) elsewhere. The debugger (S5) drills into any
  frame's locals on demand.
- **Both backends:** interpreter and VM each expose their frame/local state at fault time to the shared
  renderer. Output must be byte-identical between backends (it's tested like the fault path).
- **Testing:** fault-path tests asserting the dump content (via the deterministic renderer) for a known
  faulting program on both backends; a Secret-bearing local is redacted; Release emits nothing.

### S4 — Assertions

**Goal:** `assert(cond)` / `assert(cond, msg)` as an always-checked language feature.

- **Semantics:** always evaluated (keystone). On false → a **fault** (`FaultKind::Assert`, new),
  byte-identical across backends. Dev: failure message shows the asserted expression + operand values
  (via S2 renderer); Release: `assertion failed at <loc>`.
- **Transpile:** `assert(c)` → PHP `if (!(c)) { <trigger a fault-equivalent> }` — **not** PHP
  `assert()` (which `zend.assertions=-1` can disable in prod, breaking the spine).
- **Op impact:** likely no new `Op` (lower to existing branch + fault ops, like `MatchFail`→`Fault`);
  confirm during planning.
- **Testing:** byte-identical `run≡runvm≡real PHP` for pass + fail; guide example; `phg explain
  E-... / FaultKind::Assert`.

### S5 — Interactive debugger (last, biggest)

**Goal:** an interpreter-only pause/step/inspect engine with two frontends (REPL + DAP) sharing S2.

- **Backend: interpreter only.** No VM stepping (would need a bytecode↔source line table). The
  tree-walker has native spans + a natural pause point per node. Faithful by construction: the spine
  guarantees `run ≡ runvm ≡ PHP`, so debugging on the interpreter provably reflects the others.
- **Shared engine:** pause/step/continue/breakpoint control over the interpreter + S2 renderer for
  inspection. `phg debug` is a Dev-only subcommand; its engine is compiled out of Release artifacts.
- **Frontends (built together — developer chose Option 3):** (a) a terminal **REPL**
  (step/continue/inspect/backtrace); (b) a **DAP** server so the existing VSCode/JetBrains extensions
  get in-editor breakpoints. Shared engine, two thin adapters.
- **v1 scope (tight):** breakpoints (line), step over/into/out, continue, inspect frame/locals,
  backtrace. **Deferred:** conditional breakpoints, watchpoints, hot-reload, VM stepping.
- **Testing:** engine-level tests (deterministic step sequences over a fixture program); DAP protocol
  round-trip tests; REPL command tests. Never touches the differential spine.

## 5. Deliberately out of scope (with reasons)
- `Core.Reflection`, `Core.Runtime` (memory/time), `Core.Secret` — legit prod APIs, not gated.
- Logging facility, serve debug endpoints, optimization levels — greenfield; the profile system is
  designed to host them later, not built now.
- LSP param **contravariance**, generic bounds/variance — documented deferrals; only override return
  **covariance** (S1) is the soundness hole being closed.

## 6. Decisions log
- [2026-07-01] AGREED: build **all of it** — diagnostics+LSP quality + runtime value-dump +
  interactive debugger — as one milestone (M-DX), sliced.
- [2026-07-01] AGREED: **keystone** — a profile changes side-channels/diagnostics only, never program
  output; `run≡runvm≡PHP` holds under both Dev and Release.
- [2026-07-01] AGREED: debugger is **interpreter-only**, **REPL + DAP frontends built together**
  (shared engine), **disabled/absent in prod**.
- [2026-07-01] AGREED: introduce a first-class **build-profile (Dev|Release)** as a foundation;
  Release is **secure-by-construction** (machinery absent, not flag-off), compile-time-chosen.
- [2026-07-01] AGREED: profile gates — value-dump, debugger engine, stack-trace verbosity, `serve
  --dev` + build source-embed, internal-panic presentation. Value redaction handled in the renderer
  (Secret wrapper).
- [2026-07-01] AGREED: **assertions in scope** — always-checked, `FaultKind::Assert`, profile changes
  only failure richness, transpiles to explicit `if (!c)` not PHP `assert()`.
- [2026-07-01] AGREED: dump default capture = **faulting-frame locals + backtrace headers**.
- [2026-07-01] AGREED: slice order = **S1 diagnostics → S0 profiles → S2 renderer → S3 value-dump →
  S4 assert → S5 debugger**.
