# Adversarial Byte-Identity Review — `Core.Http` (Stage 2b refutation)

**Target claim (spike):** tier=B, feasibility=55%, strategy = "No spine gating for the live
request — by design. Mark `Core.Http` natives `pure:false` so `differential.rs`'s
`uses_impure_native()` auto-SKIPs any importing program from the PHP oracle with ZERO harness
edits (exact `Core.Process`/`Core.Env` precedent). Parity gated outside `differential.rs`: a
`tests/http.rs` with an in-memory `HttpTransport` feeds canned bytes and asserts run ≡ runvm
produce the identical `Response` value; M6 W1 `serialize_response`/`parse_request` round-trip
pins the value codec. Transpile checked structurally, not by oracle."

**Verdict: determinism_holds = FALSE.** The *happy-path* value-parity claim survives, but the
strategy as written contains a load-bearing factual error on the fault path, and the "ZERO harness
edits" claim is conditionally false. The 55% feasibility is also a generous read of the spec's own
gate. Below, every refutation is grounded in the live source I read this session.

---

## What I verified the spike got RIGHT (so the refutation is targeted, not blanket)

- `uses_impure_native(src)` *is* derived from `NativeFn::pure == false`, not hardcoded. [Verified:
  `tests/differential.rs:916-925` builds the impure set from `phorj::native::registry().filter(|n|
  !n.pure)`, matches `src.contains("import {module}")`.] The `pure` field exists on `NativeFn`
  [`src/native/mod.rs:62`] and `Core.Process` ships three `pure: false` natives
  [`src/native/process.rs:70,80,96`]. So the *single-file* quarantine mechanism is real.
- `src/serve.rs` `Transport` trait + in-memory test transport seam is real [Verified: read
  `src/serve.rs:1-70`], so an `HttpTransport` dual with a canned fixture impl is mechanically
  plausible. `bytes` / `Value::Bytes` / M6 W1 `Request`/`Response` reuse is sound.
- No new `Op` is needed (a native is `Op::CallNative`). Correct.

The mechanisms exist. The refutation is about **where the spike's parity guarantee actually holds
vs. where it silently breaks**, and about an incorrect harness claim.

---

## REFUTATION 1 (decisive) — native faults are NOT byte-identical run≡runvm; the spike's risk #4 mitigation is factually wrong

The spike (risk #4) asserts: *"both Rust legs share the same value-kernel fault path."* This is
false. A native that returns `Err(String)` does **not** go through the value kernel, and the two
backends render that error **differently**:

- **Interpreter:** `src/interpreter/call.rs:28` calls `NativeEval::Pure(f)` and lifts the native's
  plain `String` into a `Signal` *without a line prefix* (the native contract is "report failure as
  a plain String", per the comment block at `call.rs:20-26`).
- **VM:** `src/vm/closure.rs:54-57` — *"A fault propagates as a raw `String` — the outer `run` loop
  (still executing the `CallNative` op) **attaches the source line**, exactly as for any native
  fault."*

So a `Http.send` connection-error fault renders as:
- interpreter: `connection refused` (no prefix)
- VM: `runtime error at N:C: connection refused` (line/col prefix)

These are **not byte-identical strings.** The differential harness only survives this because
`agree_err` compares by semantic `FaultKind` [`tests/differential.rs:64-148`], and every existing
runtime fault has a `classify()` arm that matches a **body substring** precisely so the VM's line
prefix doesn't split it from the interpreter's prefix-less render (see the `IndexOob`/`NoField`/
`ForceUnwrap`/`RangeTooLarge` doc-comments — each one exists *only* to absorb this exact line-prefix
divergence). [Verified: `classify()` at `differential.rs:102-148`.]

`classify()` has **NO connection/IO/network arm.** An unrecognized fault body falls to
`FaultKind::Other(err.to_string())` — which carries the **verbatim** string. Interpreter
`Other("connection refused")` ≠ VM `Other("runtime error at N: connection refused")`.

**Consequence for the strategy as written:** the spike's `tests/http.rs` sketch says only "assert
run ≡ runvm produce the identical Response value." It is silent on the fault path. If `tests/http.rs`
canned a transport that returns an `io::Error` (the realistic negative case — connection failure is
the #1 thing a client test must cover) and compared the resulting fault, a naive
`assert_eq!(run_err, runvm_err)` would **fail** on the line prefix, and even an `agree_err`-style
comparison would land both in `FaultKind::Other(...)` and **still fail** because the `Other` payload
differs. The spike's "ZERO harness edits" is therefore false for any test that exercises a fault: a
**new `classify()` arm** (e.g. `FaultKind::Network` keyed on a single-sourced fault body substring)
is required — exactly the harness edit the precedent modules each had to make. The spike asserts the
opposite.

This is the load-bearing refutation: the claim "byte-identical run≡runvm" holds on the success path
but is **untrue on the fault path without a harness edit the spike denies needing.**

---

## REFUTATION 2 — "ZERO harness edits" is conditionally false: the project glob has NO quarantine

The single-file glob at `differential.rs:1004` and the PHP-oracle single-file glob at `:1904` both
call `uses_impure_native` and `continue`. But the **multi-file project globs** —
`all_example_projects_match_between_backends` (run≡runvm, ~`:1048`) and
`all_example_projects_transpile_and_match_php` (`:1916`) — **never call `uses_impure_native`.**
[Verified: read both loop bodies; the only `uses_impure_native` call sites are 1004 and 1904, both
single-file.]

The `Core.Process` precedent is **single-file** (`examples/process/args-env.phg`, no `phorj.toml`)
[Verified: `find examples/process` → one `.phg` + README, no manifest], so the latent gap never
fires. But the spike's API sketch (§5) uses `package Main;` and presents `Http.send(Request(...))`
as "the portable unit, mirror of the server's `handle`" — and M6 web examples already live as
multi-file shapes (`examples/web/`, `examples/project/`). If a `Core.Http` walkthrough is authored
as a **project** (`examples/project/httpdemo/` with a `phorj.toml`) — the natural shape for "a real
program" — it is picked up by the project oracle test with **no SKIP**, transpiled, and run against
**real PHP**, which (a) hits the live-network/`allow_url_fopen` problem the spike spent §7 avoiding,
and (b) requires a *new* quarantine branch in two project-glob tests. "ZERO harness edits" holds
**only** under an unstated constraint: *the Http example must be single-file.* The spike never states
this; it is a real, undocumented harness dependency.

---

## REFUTATION 3 — the success-path value-parity claim is real but NARROW, and the spike over-credits it

`tests/http.rs` with a canned transport feeding *identical bytes* to both backends will produce an
identical `Response` value **iff** the request-build + response-parse logic is pure Phorj over
`bytes`/`List<string>` (M6 W1 codec). That is plausible. BUT: this proves parity of the **codec**,
not of the **module** — the one thing that makes `Core.Http` `Core.Http` (the socket round-trip) is,
by the spike's own admission, never exercised in any byte-identity test. The harness gates the part
that was already gated by M6 W1 and explicitly does *not* gate the impure part. That is the correct
design, but it means the "55% feasibility, high confidence the engineering works" framing rests on
re-verifying already-shipped codec parity, plus an unverified socket layer. The genuinely new code
(the `TcpStream` `HttpTransport` impl) has **no parity gate at all** — only a fixture mock of it
does. Calling that "byte-identical" is a category slip: the mock is identical because it is canned,
not because the two backends agree about the network.

---

## REFUTATION 4 — transpile divergence is not "structural-only safe"; HTTPS asymmetry IS a run-divergence (just hidden by the SKIP)

The spike frames the Rust=http-only / PHP=https-capable asymmetry (§7 risk #1) as "acceptable,
parallels the mbstring asymmetry." It is worse than mbstring: with mbstring the *output* is still
identical for tier-1 inputs. Here the transpiled PHP for `Http.get("https://...")` **succeeds**
(core stream wrapper has TLS) while the Rust legs **fault** (`http://`-only). That is a hard
run-divergence — the program produces a `Response` under PHP and a fault under run/runvm. It is
"safe" only because the `pure:false` SKIP removes it from the oracle. So the asymmetry is not
mitigated; it is *hidden*. Any future move of `Core.Http` toward the spine (or a developer who
expects the transpile to be a faithful mirror) re-exposes a divergence the spike labels acceptable.
The `file_get_contents` target is also `$http_response_header` magic-global dependent and
`allow_url_fopen`-gated (spike risk #7) — a transpile whose correctness can't be oracle-checked
*and* whose semantics depend on a deployment ini flag is structurally unverifiable, not merely
"checked structurally."

---

## SECONDARY non-determinism the spike under-weights (would bite if the module ever neared the spine)

- **Header ordering / `Content-Length` recomputation:** spike risk #5 is right that `List<string>`
  raw lines preserve order — but a PHP `file_get_contents` target gets headers from
  `$http_response_header`, whose normalization (folded headers, case) is the wrapper's, not the
  Rust reader's. Two different parsers, two orderings/casings. Only the SKIP saves it.
- **gzip mtime / `Content-Encoding`** (risk #6): correctly flagged; `Accept-Encoding: identity` is
  the right mitigation, but it must be *enforced in the native*, not left to the caller, or a server
  ignoring it reintroduces a non-deterministic gzip mtime + needs absent ext-zlib.
- **`std::io::Error` Display is locale/OS-worded** — irrelevant in a canned fixture, but the moment
  a real connect error reaches a user-visible fault, the body itself diverges across machines (on
  top of the VM line prefix from Refutation 1).
- **Float formatting / object ids:** N/A — `Response` carries `int`/`bytes`/`List<string>`, no float
  and no identity surface. Not a risk here. (Stated for completeness so the verdict isn't seen as
  hand-wavy.)

---

## Honest scoring

- The spike's **tier=B** classification is correct and well-argued.
- The **engineering plausibility** (Transport dual, no new Op, value reuse) is correct.
- The **"byte-identical run≡runvm" + "ZERO harness edits"** claims are FALSE as stated: the fault
  path diverges (VM line prefix) and needs a new `classify()` arm; the project-glob path has no
  quarantine and needs a branch if the example is multi-file.
- Revised feasibility: I'd hold ~50% (was 55%) — the discount is not the TLS ceiling (the spike owns
  that) but the **two unstated harness edits** plus the fact that the only genuinely new code (the
  socket impl) is parity-gated only against a mock of itself.

**Confidence: high** on Refutations 1 & 2 (read the exact source: `call.rs:28`, `closure.rs:54-57`,
`classify()` has no network arm, project globs lack `uses_impure_native`). **Medium** on the
feasibility re-score (judgment).
