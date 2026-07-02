# Agent A — Software Craftsmanship Audit (whole repo)

Date: 2026-07-02 · Scope: **whole repo** — Part 1: `src/` (~216 .rs files); Part 2 (scope widened mid-flight): `tests/`, `examples/`, `.github/`, `scripts/`, `tools/`, `playground/`, `editors/`, `bench/`, `conformance/`, `selftest/`. Excluded per instruction: `target/` and line-by-line review of committed vendored fixtures (audited as policy/presence only). Method: grep-driven sweeps + deep reads of highest-risk files + **live execution of the CI gates** (perf-gate exercised in pass, regression, and malformed-output modes). Read-only audit — no code modified.

> **Headline: one P0, found in Part 2** — the local pre-commit gate is silently void: `core.hooksPath` still points at the pre-rename `/stack/projects/phorge/` path, which no longer exists. See A-CI-1.

Baseline: **1617 tests passed, 0 failed, 28 suites, 183.6s** [Verified: `cargo test --workspace`, exit 0].

---

## 1. SECURITY

### A-SEC-1 · CLEAN · `forbid(unsafe_code)` holds crate-wide
`src/lib.rs:3` and `src/main.rs:3` both carry `#![forbid(unsafe_code)]`; every `rg 'unsafe' src/` hit is a comment/doc reference [Verified: rg sweep — 19 hits, all in `//`/`//!` lines]. The two `unsafe`-adjacent externals (`corosensei` stack switching, `ctrlc` sigaction) are confined to their crates as documented in `Cargo.toml:38–68`.

### A-SEC-2 · CLEAN · Bundle readers comply with the checked-arithmetic rule (EV-7)
62 `checked_add`/`checked_*` call sites across `src/bundle/*.rs` [Verified: rg count]. Every offset read in the reader paths (`macho.rs:30–73`, `pe.rs:24–25`, `elf.rs`) goes through `checked_add` + `Option` propagation. The unchecked `+` sites (`pe.rs:68`, `macho.rs:124,166,188`, `section.rs:54`) are all inside `#[cfg(test)]` fixture builders, not reader paths [Verified: read `pe.rs:50–80`, `macho.rs:110–130` — fixture constructors].
`container.rs:56–86` validates header CRC **before** trusting `payload_len`, then `checked_add` + explicit `end > blob.len()` bounds check before slicing [Verified: read].

### A-SEC-3 · P3 · `u64 as usize` truncation in container decode
`src/bundle/container.rs:74` — `u64::from_le_bytes(...) as usize`. On a 32-bit target this silently truncates `payload_len`; the subsequent bounds check keeps it memory-safe (worst case: wrong-length payload → CRC mismatch → `None`), so no exploitability — but it violates the spirit of the checked-arithmetic rule. Remediation: `usize::try_from(...).ok()?`. [Verified: read lines 74–83]

### A-SEC-4 · CLEAN · HTTP server input hardening (serve.rs)
`MAX_REQUEST = 8 MiB` cap enforced during framing (`serve.rs:564,585`), `Content-Length` is `saturating_add`-ed and `.min(MAX_REQUEST)`-clamped (`:595–596`), per-connection read/write timeout defaults to **30 s** via the CLI (`main.rs:307`, `--timeout 0` opt-out), keep-alive capped at 100 requests/conn (`:609`), transport-error circuit breaker at 64 (`:70`) [Verified: rg + reads]. An `InfiniteReader` DoS test exists (`serve.rs:829`). Single-threaded by design (documented Rc-heap constraint).

### A-SEC-5 · CLEAN · Vendor/loader path traversal guarded
Dependency names and source paths from `phorj.toml` pass `validate_path_component` (`manifest.rs:271–290`): rejects `..` segments, absolute paths, drive prefixes, and a bad charset — explicitly labeled "GA blocker B2" with negative tests (`manifest.rs:534` `"../../etc"`, `:548` charset, `:563` `source = "../outside"`) [Verified: rg + read]. `loader/fs.rs:176–188` canonicalizes for the folder=path check.

### A-SEC-6 · CLEAN · Transpiler PHP string escaping
`transpile/mod.rs:828–877` — `php_escape` escapes `\`, `"`, `$` unconditionally; `php_escape_interp` escapes `$` exactly where PHP would interpolate (conservative on segment ends); `php_escape_bytes` emits always-two-digit `\xHH` so PHP's greedy `\x` cannot merge with a following hex char [Verified: read all three fns]. Raw control characters pass through unescaped, which is legal and value-preserving inside PHP double quotes — not a finding.

### A-SEC-7 · P3 · `Secret<T>` leak lint is direct-flow only
`checker/calls.rs:698–745` — `W-SECRET` fires only when a syntactic `<recv>.expose()` flows *directly* into a sink. `var v = s.expose(); Output.printLine(v);` is not flagged. The core guarantee (opaque class, private field, must call `expose()`) is by construction and holds; the lint is best-effort by design. Remediation: document the one-hop limitation in the lint's explain text if not already there. [Verified: read comment block — "flowing *directly* into a sink"]

### A-SEC-8 · CLEAN · Lexer UTF-8 `unwrap`/`expect` sites are unreachable-safe
12 `from_utf8(...).unwrap()/expect(...)` sites in `src/lexer/mod.rs` (e.g. `:144,:179,:326,:387`). Source only enters via `fs::read_to_string` (`main.rs:525`, `loader/fs.rs:262`) or `stdin().read_to_string` (`main.rs:538`), which guarantee valid UTF-8; number-literal slices additionally consume only ASCII [Verified: rg on entry points + read of `scan_number`]. Classification (a): genuinely unreachable with proof.

---

## 2. ERROR HANDLING

Sweep counts (non-test code, `rg -g '!*test*'`) [Verified]:

| macro | non-test hits | classification |
|---|---|---|
| `unwrap()` | 66 | majority in inline `#[cfg(test)]` modules the glob can't exclude (manifest/lock/serve/bundle test mods); remainder = lexer UTF-8 (A-SEC-8) and hex-digit conversions guarded by `is_ascii_hexdigit` (`lexer/mod.rs:770`) |
| `expect(` | 161 | ~153 are the **parser's own `self.expect(&TokenKind…)`** — a `Result`-returning method, not `Result::expect` [Verified: read `parser/exprs.rs` sample]. True `Result::expect` remainder: coop/green invariants below |
| `panic!` | 3 | all in `#[cfg(test)]` helpers (`value.rs:1654,1948` — test mod starts `:1457`; `manifest.rs:585`) [Verified: awk cfg(test) position check] |
| `unreachable!` | 48 | all but 4 carry a justification message naming the checker/parser guarantee that gates them |
| `todo!`/`unimplemented!` | 0 | — |

### A-ERR-1 · P3 · Four bare `unreachable!()` without justification message
`compiler/mod.rs:808,829`, `compiler/expr.rs:524,541`. Every other site states its gating invariant (e.g. "checker rejects other assignment targets"). Remediation: add the one-line proof message — it's the project's own convention. [Verified: rg output]

### A-ERR-2 · P3 · Two production `expect` on scheduler invariants
`interpreter/coop.rs:103` ("a cooperative task interpreter holds its program") and `green/exec.rs:131` ("a ready task id always has a registered executor"). Both are internal-invariant panics reachable only via a scheduler bug, not user input — classification (a), but they are the only prod panics standing between a scheduler regression and a user-facing crash. Remediation: acceptable as-is; consider converting to a fault for EV-7 uniformity.

### A-ERR-3 · P2 · DAP transport write errors silently swallowed
`dap.rs:50–51` — `let _ = write!(self.out, "Content-Length: …")` + `let _ = self.out.flush()`. A broken client pipe means every subsequent DAP response is silently lost while the session keeps running. Remediation: track a `dead: bool` and end the session (or log once) on write failure. [Verified: rg `let _ =` sweep]

### A-ERR-4 · P2 · Malformed `Content-Length` treated as absent in DAP/LSP framing
`dap.rs:90` and `lsp/mod.rs:664` — `v.trim().parse().ok()` maps a garbled header to `None`, which desynchronizes the length-prefixed protocol stream (next read starts mid-message) rather than failing loudly. Remediation: on parse failure, return a protocol error / close the stream. [Verified: rg]

### A-ERR-5 · CLEAN · Remaining `let _ =` sites are justified
`debug_repl.rs` (interactive REPL output), `mem.rs:29` (documented graceful degradation writing `/proc/self/clear_refs`), `cli/bench.rs:158` / `vendor.rs:316` / `elf.rs:84–94` (best-effort temp cleanup), `checker/calls.rs:1026` (errors accumulate in `self`; the return value is redundant — commented) [Verified: rg + inline comments].

---

## 3. PERFORMANCE

Context: `Value` heap objects are `Rc`-shared; `Rc::clone` is fine, inner-data clones are not. `Op::GetField` has an S1b inline cache (ptr-compare on the layout, no name clone on the monomorphic path) [Verified: read `vm/exec.rs:554–590`] — the pattern exists in-tree; the findings below are places it wasn't extended.

### A-PERF-1 · P2 · `Op::CallMethod` allocates two `String`s per call
`vm/exec.rs:666–676` — `self.program.names[name_idx].clone()` + `inst.class.clone()` build a fresh `(String, String)` HashMap key on **every** method call. Field reads got the inline cache; method dispatch did not. On method-heavy code this is the dominant per-call allocation. Remediation: per-site inline cache keyed on layout ptr → function index, same shape as `field_caches`. [Verified: read]

### A-PERF-2 · P2 · `Op::CallValue` deep-clones the captures `Vec` per closure invocation
`vm/exec.rs:757` — `cd.as_ref().clone()` clones the whole `ClosureData` (including the `Vec<Value>` captures buffer) on every first-class-function call; higher-order stdlib (`List.map`/`filter`/`reduce`) pays this per element. Remediation: destructure by reference and `extend_from_slice` the captures directly onto the stack. [Verified: read `:748–767`]

### A-PERF-3 · P2 · Interpreter static/const reads clone the `(class, name)` key per access
`interpreter/expr.rs:119–122` — two `String` clones to build a lookup key on every static/const member read (twice on the miss path). Remediation: borrow-keyed lookup (`HashMap<(String,String),_>` supports `get` with a tuple of `&str` via a wrapper, or restructure to nested maps). [Verified: read]

### A-PERF-4 · P3 · Overloaded-call candidate vectors rebuilt per call
`vm/exec.rs:682–683` — `set.iter().map(|(k, _)| k.clone()).collect::<Vec<Vec<ParamKind>>>()` allocates the full candidate signature list on every overloaded method call (also at `:453` for the static-overload path). Overloads are rarer than plain calls, hence P3. Remediation: make `select_overload` take the set by reference.

### A-PERF-5 · P3 · Bytes literal re-allocated per evaluation
`interpreter/expr.rs:36` — `Value::Bytes(Rc::new(b.clone()))` deep-copies the literal and allocates a fresh `Rc` each time the expression is evaluated (a `b"…"` in a loop clones per iteration). The VM constant pool doesn't have this problem. Remediation: cache the `Rc` in the AST node or intern at check time.

### A-PERF-6 · CLEAN · No O(n²) regressions found in swept paths
The known index-assign O(n²) was fixed (`Op::SetIndexLocal`, commit `b8a2877`). Hook/trait member lookups in `interpreter/construct.rs` are linear over `class.members` but bounded by class size, not data size [Verified: rg `iter().find` sweep].

---

## 4. SOLID / DESIGN

### A-DES-1 · P2 · ~40 functions exceed the project's own 150-line rule
[Verified: awk fn-length sweep over non-test files]. Two classes:

**Documented-exempt dispatchers** (ARCHITECTURE.md §Module decomposition states the big matches "stay whole in one method", verified by a dummy-variant check): `vm/exec.rs exec_op` (802), `transpile/expr.rs emit_expr` (474), `interpreter/expr.rs eval` (325), `ast/mod.rs span` (319), `compiler/expr.rs expr` (297) and kin. These are exhaustive `match` tables the project deliberately keeps whole — not findings.

**Genuine decomposition candidates** (procedural, not match tables):
- `main.rs:13 fn main()` — 511 lines of argv routing; the `serve` arm alone parses 4 flags inline. Extract per-subcommand parsers.
- `compiler/program.rs:13 compile_program_with` — 667 lines.
- `transpile/program.rs:363 emit_runtime_helpers` — 829 lines of PHP helper text (arguably a data table, but it mixes conditional logic with emission).
- `cli/mod.rs:390 inject_rounding_mode_prelude` (249) and `:746 inject_secret_prelude` (168) — near-identical injected-prelude machinery, a single-sourcing candidate.
- `loader/mod.rs:215 load_project` (219), `transpile/mod.rs:93 decomposed_classes` (275).
- `cli/explain.rs:4 explain_text` — 1293 lines, but it is a pure `code → text` data table; splitting would add nothing. Exempt-by-nature.

### A-DES-2 · CLEAN · Value kernels remain single-sourced
`build_map`/`build_set`/`map_index`/`eq_val` live once in `value.rs:319–499` and are consumed by both backends; the interpreter's arith wrappers (`interpreter/mod.rs:801–902`) are op-routing shims whose `unreachable!` messages document their guard [Verified: rg fn definitions — no duplicate kernel definitions found in vm/ or interpreter/]. No drift detected.

### A-DES-3 · CLEAN · No `#[allow(dead_code)]` anywhere in src/
[Verified: rg — zero hits.] Dead-pub sweep not exhaustively run (216 files); nothing surfaced incidentally.

### A-DES-4 · P3 · Nesting-depth rule not mechanically enforced
The >4-nesting rule has no lint backing (clippy pedantic is off; `cognitive_complexity` not enabled). The long-function sweep suggests several offenders inside `exec_op`/`emit_expr` arms. Remediation: enable `clippy::cognitive_complexity` at a generous threshold as a ratchet.

---

## 5. TEST HEALTH

**1617 passed / 0 failed / 28 suites / 183.6 s** [Verified: full run output].

Distribution [Verified: per-directory `#[cfg(test)]` file counts]:
- Strong: `native/` 50/51 files with tests, `checker/` 35/57, `lexer/` 2/2, `value.rs` 40 tests, `manifest.rs` 26, `chunk.rs` 13, `serve.rs` 11 (incl. DoS-shaped `InfiniteReader`).
- **A-TEST-1 · P2 · Zero direct tests**: `dispatch.rs` (overload selection — pure logic, highly unit-testable, currently only exercised end-to-end) and `json.rs` (powers both DAP and LSP framing). Remediation: unit tests for `select_overload` ambiguity/no-match edges and `json.rs` escaping/nesting edges.
- **A-TEST-2 · P3 · Thin in-file coverage** in `transpile/` (2/8), `compiler/` (2/6), `interpreter/` (3/7) — mitigated structurally: the differential harness gates `run ≡ runvm ≡ real PHP` byte-identity over every `examples/**/*.phg`, which is behavioral assertion of exactly these modules. Acceptable by design; noted for completeness.
- Quality: tests assert behavior (byte-identical output, fault-kind parity, real-PHP oracle with `PHORJ_REQUIRE_PHP=1` fail-not-skip), not just "doesn't crash" [Verified: differential harness design + sampled test bodies in serve.rs/manifest.rs].

---

## 6. DOCS DRIFT (quick pass)

- `docs/INVARIANTS.md` — the 12 invariants sampled against code all hold: EV-7 checked arithmetic (A-SEC-2), Op-match coupling (`unreachable!` messages reference it), kernels single-sourced (A-DES-2), quality gate green [Verified: headings read + spot checks].
- `docs/ARCHITECTURE.md` — pipeline diagram uses conceptual `lexer.rs`-style names, but §"Module decomposition (M-Decomp)" (lines 34–35, 56+) explicitly documents that each is now a directory; the dispatcher-stays-whole rule is also documented there. **No material drift found.**

---

---

# PART 2 — Repo-wide extension (scope widened mid-flight)

## 7. CI WORKFLOWS & GATES (`.github/`, `scripts/`) — priority area: broken gate = P0

### A-CI-1 · **P0** · Local pre-commit gate is silently VOID (stale `core.hooksPath`)
`.git/config` sets `core.hooksPath = /stack/projects/phorge/scripts/git-hooks` — an **absolute** path from before the phorge→phorj directory rename. That directory no longer exists (`ls: cannot access '/stack/projects/phorge': No such file or directory`), and git runs **no hooks at all** from a missing hooksPath, with no warning [Verified: `git config --show-origin core.hooksPath` → `file:.git/config /stack/projects/phorge/scripts/git-hooks`; `ls -ld /stack/projects/phorge` → ENOENT]. Consequence: **every local commit since the directory rename has bypassed the fmt+clippy+full-test pre-commit gate** — and this project's workflow commits autonomously and holds long unpushed runs (everything after `0d952a8` is unpushed), so CI's push-triggered gate has not seen them either. The gate the pre-commit hook exists to provide has been void for the entire post-rename commit run. The hook file itself is healthy (`set -euo pipefail`, no masking) — it just never executes. Note: the hook's own header documents the wiring command with a **relative** path (`git config core.hooksPath scripts/git-hooks`), which would have survived the rename; the config was evidently set with an absolute path instead.
**Remediation (one line):** `git config core.hooksPath scripts/git-hooks` (relative — rename-proof), then verify with a no-op commit. Additionally, consider re-running the gate (`cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`) over the unpushed range before pushing — mitigating: my Part-1 baseline (1617/0) shows the *current* tree is green, so historical intermediate commits are the only unknowns.

### A-CI-2 · CLEAN · Main CI gate is real and fail-closed
`.github/workflows/ci.yml` `gate` job: `cargo fmt --check` → `clippy --workspace --all-targets -- -D warnings` → `cargo test --workspace` with `PHORJ_REQUIRE_PHP=1` (missing/diverging PHP oracle **fails**, never skips), on the pinned 1.96.0 toolchain (`rust-toolchain.toml`) and PHP pinned to the 8.5 transpile floor. No exit-code masking anywhere in the gating path [Verified: full-file read + `rg '\|\| true|continue-on-error|set \+e'` over all workflows — the only hits are `release.yml:60` `strip … || true` (packaging nicety, not a gate) and `ci.yml:95` `continue-on-error: true` on the **documented non-gating** 8.6-dev canary].

### A-CI-3 · CLEAN (live-verified) · perf-gate.sh actually gates
Exercised in all three modes [Verified: live runs]:
- Real binary: `vm_speedup=17.28 ≥ floor 10.8` → `PASS`, exit **0**.
- Simulated regression (fake `phg` emitting `vm_speedup: 1.0`): `FAIL`, exit **1**.
- Malformed output (non-JSON): jq parse error propagates through `set -eEuo pipefail`, exit **5** — fail-closed, not fail-open.
Script hygiene is exemplary: `set -eEuo pipefail`, `LC_ALL=C` (locale-proof awk decimals), `runs=0` degenerate case fails closed (best=0 < floor), missing jq/binary/baseline exit 2. `bench/baseline.json` present and coherent (floor 10.8 vs observed ~17–22).

### A-CI-4 · P2 · `curl | sh` unpinned installer in the Pages deploy pipeline
`.github/workflows/playground.yml` — `curl https://rustwasm.github.io/wasm-pack/installer/init.sh -sSf | sh` executes an unpinned remote script in the workflow that **deploys the public playground site**. A compromised installer endpoint = arbitrary code in the deploy job (its `id-token: write`/`pages: write` permissions are scoped, but the shipped site content is the asset at risk). Remediation: `cargo install wasm-pack --locked --version <pin>` or download a versioned release artifact with a checksum. [Verified: workflow read]

### A-CI-5 · P3 · Supply-chain pinning is version-level, not content-level
(a) Zig tarball fetched from ziglang.org pinned by version (0.16.0) but no checksum verification (`ci.yml:147`, same pattern in `stub-registry.yml` via `mlugg/setup-zig@v2`); (b) GitHub Actions pinned by tag (`@v7`, `@v2`) not by commit SHA — a moved tag is executable in CI. Standard practice, but content-pinning is the hardened form. [Verified: workflow reads]

### A-CI-6 · P3 · `cargo install cargo-zigbuild` runs before the cache-restore step
`ci.yml:155–159` — the install step precedes `Swatinem/rust-cache@v2`, so the tool is rebuilt from source on every cross-build run instead of restored. Pure CI-minutes waste, no correctness impact. Remediation: move the cache step above the install (rust-cache caches `~/.cargo` including installed bins). [Verified: step order in workflow]

### A-CI-7 · CLEAN · release.yml / stub-registry.yml
Release builds are native-per-OS with the pinned toolchain; every multi-line bash block sets `set -euo pipefail`; the stub-registry's two-pass manifest bake (stubs hashed with host `sha256sum`, verified client-side by the hand-rolled SHA-256) is a deliberate cross-implementation integrity check. `fail-fast: false` on the matrix is appropriate (independent targets). [Verified: full reads]

## 8. TEST HARNESSES (`tests/`)

### A-TEST-3 · CLEAN · `tests/differential.rs` — the harness itself is high quality
The correctness spine holds up under audit [Verified: reads of the harness core]:
- `agree` compares `Result`s **structurally, never `.expect()`** — a release-mode divergence reports as a clean mismatch, not a panic.
- Fault parity via semantic `FaultKind` classification (`agree_err`), not raw-string compare — immune to the CLI's stage-prefix asymmetry.
- `with_pkg` prepends `package Main;` **on the same line**, preserving line numbers so fault-line assertions stay valid.
- Example discovery is a recursive glob: **any `.phg` added under `examples/` is auto-gated** with no test edit; projects are discovered by `phorj.toml` presence (structural, not name-based).
- Exclusions are policy-verified, not silent: `examples/interop/` (PHP-target-only, gated instead by `tests/interop.rs` golden output) and `examples/process/` (walkthroughs, gated by `tests/process.rs`) — each exclusion names its replacement harness in a comment.
- The PHP oracle's `php_or_gate` is fails-not-skips under `PHORJ_REQUIRE_PHP=1` and skips **loudly** (stderr) otherwise — the comment explicitly records this as closing a past "self-skip-to-PASS" P0. CI sets the variable; the design flaw class is closed.

### A-TEST-4 · CLEAN · `tests/conformance.rs` — golden-output corpus is strictly stronger than agreement
Each `conformance/**/*.phg` pins **exact expected stdout** for interpreter + VM + PHP — catching the wrong-but-consistent drift class that `agree` cannot (all three backends drifting identically). Same fails-not-skips PHP gating. [Verified: header + structure read]

### A-TEST-5 · P3 · `tests/build.rs` cross-parity tests skip silently-ish on dev machines
Toolchain-gated graceful skips (`skipping: cargo-zigbuild unavailable`, etc.) mean a local `cargo test` green does not include cross-build parity. Mitigated by design: the CI `cross-build` job installs zigbuild + targets + llvm-objcopy and then runs `cargo test --test build`, so the skips never happen where it matters, and the skip messages are loud on stderr. Documented in-file. [Verified: rg skip sites + ci.yml cross-build steps]

### A-TEST-6 · CLEAN · Test network usage is loopback-only
`tests/serve.rs` binds `127.0.0.1:0` (ephemeral) exclusively; `tests/vendor.rs` drives git over a `file://` local fixture — no real network in the suite. [Verified: rg bind/localhost]

## 9. EXAMPLES (`examples/`)

- **A-EX-1 · CLEAN · All 171 `.phg` files are behaviorally gated** — the differential glob + project discovery byte-identity-gates every example on both backends (and the oracle adds real-PHP parity for runnable ones); the 1617-green baseline includes them. "Do they still compile" is answered by execution, not inspection. [Verified: suite pass + glob design]
- **A-EX-2 · P3 · README index drift (2 entries)** — `web/json-api.phg` and `random/dice.phg` exist but are absent from `examples/README.md`, violating the project's own "examples ship with features (index + coverage matrix, same change)" rule. [Verified: scripted diff of `find` vs README mentions]
- **A-EX-3 · CLEAN (policy) · Committed vendor fixture** — `examples/project/withdeps/vendor/` is the documented Go-model vendoring example (offline-deterministic, lockfile-pinned); presence is intentional and documented. Audited as policy only, per instruction.

## 10. SCRIPTS & TOOLS (`scripts/`, `tools/`)

- **A-TOOL-1 · CLEAN · `scripts/perf-gate.sh`** — see A-CI-3. **`scripts/git-hooks/pre-commit`** — content is clean (`set -euo pipefail`, no masking, PATH fallback guarded); its non-execution is A-CI-1, not a script defect.
- **A-TOOL-2 · P3 · One-shot codemods retained without a "spent" marker** — `tools/core_rename.py`, `core_rename2.py`, `return_type_codemod.py` are historical migration codemods that rewrite files in place. Their docstrings document the safety net (compiler + full gate), but nothing marks them as already-executed/stale; the global working rule for one-shot migration tools ("a script written for a previous version of the system may be stale and actively harmful") argues for a header line `# SPENT <date> — do not re-run` or relocation to `tools/attic/`. [Verified: reads]

## 11. PLAYGROUND (`playground/`)

- **A-PG-1 · CLEAN · No XSS sink in the web UI** — the single `innerHTML` (`main.js:193`) is a static template with no interpolation; every dynamic value (diagnostics, program output, hints, explain text) flows through `textContent` [Verified: rg all sinks + read of `renderDiagnostics`]. WASM runs client-side per-visitor; no shared-state injection surface.
- **A-PG-2 · CLEAN · `gen_examples.py`** — skips `Core.File` examples with a logged reason ("no silent truncation" is in its docstring and honored); path resolution is script-relative. The playground crate is a workspace member, so `cargo test --workspace`/clippy gate it too [Verified: `Cargo.toml` `[workspace] members = ["playground"]`].

## 12. EDITOR EXTENSIONS (`editors/`)

- **A-ED-1 · CLEAN · Repo hygiene** — only 6 files tracked for the VSCode extension; `node_modules/`, `*.vsix`, `out/`, `package-lock.json` all gitignored (the on-disk copies are local build artifacts, not committed) [Verified: `git ls-files editors/vscode` = 6 files, 0 node_modules].
- **A-ED-2 · P3 · `package-lock.json` is gitignored** — the extension's dependency tree (vscode-languageclient et al.) is consequently not reproducible from the repo. For a thin LSP client this is low-risk, but committing the lockfile is the zero-cost hardened form (and standard for published extensions).

## 13. BENCH / CONFORMANCE / SELFTEST ASSETS

- **A-AS-1 · CLEAN** — `bench/baseline.json` is the live perf-gate config (verified by execution, A-CI-3); `conformance/` is the golden corpus consumed by `tests/conformance.rs` (8 category dirs); `selftest/` showcases `phg test` and is CI-gated via `tests/mtest.rs` (its README says so, and the test file exists) [Verified: listings + harness reads].

## 14. DOCS DRIFT — Part 2 addendum

- `CONTRIBUTING`/hook-header wiring instruction uses the rename-proof relative form — the **actual** config diverged from the documented command (absolute path). This is the root cause of A-CI-1, worth a doc callout ("verify with `git config core.hooksPath`" in CONTRIBUTING).
- Project `CLAUDE.md` references (`scripts/perf-gate.sh`, differential harness contract, `PHORJ_REQUIRE_PHP` gate, examples-glob) all match observed reality [Verified: cross-checks throughout this audit].

---

## Summary (whole repo)

| Dimension / area | P0 | P1 | P2 | P3 | Clean checks |
|---|---|---|---|---|---|
| Security (src/) | 0 | 0 | 0 | 2 (A-SEC-3, A-SEC-7) | 6 |
| Error handling (src/) | 0 | 0 | 2 (A-ERR-3, A-ERR-4) | 2 (A-ERR-1, A-ERR-2) | 1 |
| Performance (src/) | 0 | 0 | 3 (A-PERF-1..3) | 2 (A-PERF-4..5) | 1 |
| SOLID/design (src/) | 0 | 0 | 1 (A-DES-1) | 1 (A-DES-4) | 2 |
| Test health (src/) | 0 | 0 | 1 (A-TEST-1) | 1 (A-TEST-2) | baseline green |
| Docs drift (src/) | 0 | 0 | 0 | 0 | 2 |
| **CI & gates** | **1 (A-CI-1)** | 0 | 1 (A-CI-4) | 2 (A-CI-5, A-CI-6) | 3 |
| Test harnesses | 0 | 0 | 0 | 1 (A-TEST-5) | 3 |
| Examples | 0 | 0 | 0 | 1 (A-EX-2) | 2 |
| Scripts & tools | 0 | 0 | 0 | 1 (A-TOOL-2) | 1 |
| Playground | 0 | 0 | 0 | 0 | 2 |
| Editors | 0 | 0 | 0 | 1 (A-ED-2) | 1 |
| Bench/conformance/selftest | 0 | 0 | 0 | 0 | 1 |
| **Total** | **1** | **0** | **8** | **14** | — |

Overall: an unusually disciplined codebase whose remote CI gates are real, fail-closed, and (for the perf gate) now live-verified in both directions. The one P0 is operational, not code: the local pre-commit gate has been silently void since the phorge→phorj directory rename because `core.hooksPath` was set with an absolute path — combined with this project's long unpushed commit runs, that gate's guarantee was fully suspended. It is a one-line fix. Everything else is polish: two debug-tooling protocol-robustness gaps, three VM/interpreter hot-path allocation patterns, one unpinned installer in the deploy pipeline, and small drift/hygiene items.
