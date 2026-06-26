# Stage 2b — Adversarial Review (REFUTE): "Full HTTP (pure type hierarchy + Tier-B client)"

**Target:** `docs/research/extended-modules/raw/design-http.md` (tier=mixed, feasibility A≈92% / B≈78%).
**Mandate:** refute. Default `determinism_holds=false` / `feasible_std_only=false` if a real hole exists.
**Method:** every load-bearing claim re-verified against the live tree this session (file:line cited).

## Verdict summary

- **Part A (pure response factories) — survives as Tier A.** I could not find a *new* non-determinism
  surface. The factories are pure data over already-gated encoders. The one real correction: a couple of
  the design's stated worries are either non-issues (false-quarantine direction) or already-controlled
  (inherited `Core.Json` float divergence). One genuine latent footgun found (leaf-collision in
  `index_of_by_leaf`), but it does not break Part A as specced.
- **Part B (HTTP client) — survives as Tier B, with two quarantine-airtightness caveats that must be
  closed before shipping, neither fatal.** The TLS-wall reasoning is sound; the curl-subprocess escape is
  genuinely std-only. But the differential exclusion for the *example* is import-driven with **no
  path-based backstop** under `examples/web/`, and the loopback fixture test reintroduces real timing/port
  non-determinism *inside the test* that the design hand-waves.

Overall: the design is **not refuted** — it holds as `mixed`. But `determinism_holds` and
`feasible_std_only` are graded against the *gated* surface (Part A) per the harness contract, and I
return them **true** because no gated claim has a real hole. The Part-B caveats are quarantine-hardening
items, not determinism breaks in the gated spine.

---

## 1. Claims VERIFIED true (refutation failed — design is correct here)

### 1.1 Three-segment module `Core.Http.Client` resolves [Verified — refutes the design's own open-Q4]
The design hedged (open question 4): "deepest current is `Core.X`; confirm the resolver handles a
three-segment dotted module." It does, cleanly:
- `src/parser/items.rs:128-130` — `parse_import` loops `while Dot { path.push(seg) }` → arbitrary depth
  into a `Vec<String>`.
- `src/native/mod.rs:372-374` — `import_map` binds qualifier = `path.last()` → `Client` ⇒ `Core.Http.Client`.
- `src/native/mod.rs:349-352` — `index_of_by_leaf("Client","get")` matches
  `n.module.rsplit('.').next() == Some("Client")`; for `module="Core.Http.Client"` that is `"Client"`. ✅
**Refutation failed.** Three-segment modules work today; no resolver change needed.

### 1.2 The substring quarantine is *coincidentally safe* for the Http/Client pair [Verified — refutes open-Q5]
Design open-Q5 feared `import Core.Http` (pure Part A) being false-quarantined by the `src.contains`
scan because it shares a prefix with the impure `Core.Http.Client`. **It cannot:**
`tests/differential.rs:923` searches `src.contains("import Core.Http.Client")` (the impure needle is the
*longer* string). A pure Part-A source contains `import Core.Http;` which does **not** contain the longer
needle. Verified by direct string test. **The false-positive direction the author worried about does not
occur for this pair.** (The opposite — a Part-B source contains `import Core.Http` as a prefix — is
irrelevant because `Core.Http` is *pure*, never a needle.) Open-Q5 is a non-issue **as long as Part-A
module is exactly `Core.Http` and Part-B is exactly `Core.Http.Client`.**

### 1.3 The quarantine seam, serve.rs Transport, and curl-core claims are real [Verified]
- `uses_impure_native` derives the impure set from `NativeFn::pure` at runtime
  (`tests/differential.rs:916-924`) — adding `Core.Http.Client` natives with `pure:false` auto-excludes,
  no harness edit. Confirmed it is invoked at both gate sites (`:1004`, `:1904`).
- `src/serve.rs` exists with a `Transport` trait + `TcpTransport` (`:25`, `:177-208`) and `SERVE_ENTRY`
  (`:20`) — the design's "mirror this quarantine shape" is grounded.
- `tests/process.rs:27` proves the Rust-leg parity discipline for an impure module:
  `cmd_runvm(src) == cmd_run(src)` even when PHP is quarantined. So the structured-output sub-question
  "are native faults byte-identical run≡runvm" is **yes by the established pattern** — both Rust legs
  share the same native `eval`, so a transport failure → `null` is identical on `run`/`runvm`.

### 1.4 Std-only feasibility of the curl escape [Verified]
`std::process::Command` is std; the project already shells `git`/`php`/`rustc`/`cargo-zigbuild`. HTTPS via
subprocess is genuinely zero-crate, no `unsafe`. The TLS-wall table (no std TLS on the Rust legs) is
accurate. **Refutation failed** — `feasible_std_only` holds for the mechanism.

---

## 2. REAL holes found (the refutation that lands)

### 2.1 [Part B, MEDIUM] No path-based backstop — an example leaks into the differential if the impure import is omitted
`tests/differential.rs:932 collect_phg` recurses **all** of `examples/` and excludes a `.phg` only if
(a) it is under a `phorge.toml` project root, or (b) `uses_impure_native(src)` is true (import-driven).
There is **no `examples/web/` path exclusion**. Verified: the existing `examples/web/{handler,router,
json-api,server}.phg` are gated *today* precisely because they are pure (canned `b"…"` requests, pure
imports) — `server.phg` is socket-*themed* but its `main()` is pure, so it correctly runs in the
differential.

**The hole:** the design's Part-B walkthrough (B.5) is excluded **only** if it literally contains
`import Core.Http.Client;`. If a future edit puts the client call behind a pure-looking helper in another
file, or a contributor writes a `examples/web/fetch-demo.phg` that performs a live request without the
impure import on that exact line, the glob will run it on `run`/`runvm` — hanging on the network or
producing non-deterministic output, with **no structural guard**. `examples/process/` is safe only by the
same fragile discipline (it imports `Core.Process`). **This is a real quarantine-airtightness gap**, not
hand-waved away by the design (B.5 asserts "the glob doesn't gate — same as examples/process/" but that
equivalence is *import-conditional*, not path-conditional).
*Fix before shipping:* either (a) add a path exclusion for the Tier-B walkthrough dir, or (b) gate the
glob on a per-file marker, mirroring the project-root structural exclusion. The design should not claim
"airtight" without one of these.

### 2.2 [Part B, MEDIUM] The loopback fixture test reintroduces non-determinism *inside the test*
B.5 proposes `tests/http_client.rs` spinning a real `TcpListener` on `127.0.0.1:0` and driving the client
(which, for the Rust legs, **spawns a `curl` subprocess**). This is honest Tier-B (outside the
differential), but the design calls it "deterministic + offline" — that is **overstated**:
- A real `TcpListener` + a spawned `curl` introduces port-bind races, subprocess-presence dependence
  (curl absent on a CI runner → test failure unrelated to the feature), and timing/teardown ordering.
  `tests/serve.rs` avoids exactly this by using the **in-memory `Transport`** (`src/serve.rs:23`,
  "swaps an in-memory transport… so the conformance test never touches a socket"), *not* `TcpTransport`.
- The design says "reuses serve.rs verbatim" but conflates the in-memory transport (what serve.rs tests
  use) with `TcpTransport` (what the design's fixture proposes). The client has **no `Transport` seam** —
  it goes straight to `curl`/`TcpStream`. So there is nothing to swap; the fixture *must* touch a real
  socket or shell curl. **This is a genuine design gap:** Part B needs its own injectable transport seam
  (an `HttpTransport` trait mirroring `serve.rs`), or the fixtures are non-deterministic. The design does
  not provide this seam — it assumes serve.rs's seam transfers, and it does not.
*Not fatal* (Tier B can tolerate a less-pure test), but the "deterministic + offline" claim is refuted as
written; the seam is missing.

### 2.3 [Part A, LOW — latent] `index_of_by_leaf` resolves on the leaf alone → silent mis-resolution on any future leaf collision
`src/native/mod.rs:349-352` returns `position(...)` — the **first** module whose leaf matches. Today only
one `Client` leaf would exist, so Part B is safe. But the design's recommendation to use a **sub-leaf**
namespace (`Core.Http.Client`, open-Q4 "sub-leaf reads better") *increases* the probability of a future
collision (`Core.Grpc.Client`, `Core.Ws.Client` would all bind qualifier `Client` and silently resolve to
whichever registers first). There is no `E-*` guard for duplicate leaves. **Not a blocker for this
design**, but the design recommends the very pattern that makes it more likely and does not flag it.
*Fix:* if sub-leaf modules proliferate, add a duplicate-leaf registry assertion (debug-only) or resolve by
the full import-mapped path, not the bare leaf.

### 2.4 [Part A, LOW] `Http.json` float divergence is inherited, not eliminated — examples must self-restrict
The design (cross-cutting #3) correctly notes the `Core.Json` float-extreme divergence (KNOWN_ISSUES) is
inherited. This is honest, but it means a gated `examples/web/responses.phg` that puts a non-exactly-
representable float in a JSON body **will break the three-leg byte-identity** (Rust `__phorge_float` Ryū
vs PHP 14-digit `echo`/`json_encode`). The design says "examples keep to exactly-representable values" —
this is a real constraint the example author must honor, and it narrows what a Part-A "showcase" can
demonstrate (no `1.0/3.0` in a response body). Not a refutation of the tier, but a real limit on the
gated example's expressiveness that should be stated as a hard rule, not a footnote.

---

## 3. Adversarial determinism sweep (Part A, gated surface) — results

| Risk hunted | Finding |
|---|---|
| Hash-map / iteration order in headers | **No leak.** Headers are `List<string>` raw lines (insertion-ordered `Rc<Vec>`), not a `Map` — the W1 shape. `withHeader` appends. No HashMap iteration in the wire path. ✅ |
| Float formatting in JSON bodies | Inherited `Core.Json` divergence (2.4) — controlled by example discipline, not eliminated. ⚠ documented |
| Clock / random / addresses | None in Part A (pure data). ✅ |
| `Content-Length` recompute drift | Single-sourced in `Http.serializeResponse` native `eval`+`php` — same discipline as W1; no per-leg arithmetic. ✅ (claim plausible; not yet code, but mechanism is the proven one) |
| Header casing/spacing drift across legs | Single-sourced in each factory native (`process.rs` discipline). ✅ mechanism sound |
| `php -n` missing ext | Part A uses no `mb_*`, no Composer — `htmlspecialchars`/`json` core helpers present. ✅ |
| `StreamResponse` finite-producer | Reduces to `Bytes.concat` of a fixed `List<bytes>` → deterministic; PHP `implode('')`. ✅ Tier-A boundary correctly drawn; live producer correctly pushed to Tier B. |

**No new gated non-determinism surface found in Part A.**

## 4. Adversarial sweep (Part B, quarantined) — quarantine airtightness

| Check | Finding |
|---|---|
| Does any glob leak the Tier-B program into differential? | **Gap 2.1** — import-driven only, no path backstop. Must fix. |
| Are the fixtures actually deterministic? | **Gap 2.2** — `TcpListener` + curl subprocess; no injectable seam; "deterministic+offline" overstated. |
| Native fault parity run≡runvm? | ✅ Same `eval`, `tests/process.rs:27` precedent — failure→`null` identical on both Rust legs. |
| TLS wall genuinely closed by curl-subprocess? | ✅ std-only, zero-crate, no `unsafe`. Capability (not determinism) claim, honestly Tier-B. |
| `pure:false` auto-quarantine works for the new module? | ✅ derived from registry, no harness edit (1.3). |

---

## 5. Could either part be rejected as incoherent? No.
Part A is a thin pure layer over shipped, gated modules — coherent and low-risk. Part B is the textbook
Tier-B feature with a working precedent (`Core.Process`/serve.rs). Neither is incoherent; the holes are
hardening items (path backstop, fixture seam), not coherence failures. **Reject is not warranted.**

## 6. Net grade
- **Part A:** confidence the design holds as Tier A — **high**. Honest feasibility ~90% (design's 92% is
  fair; -2 for the float-example constraint being a real limit, not a footnote).
- **Part B:** mechanism **high**, quarantine-airtightness **medium-pending-two-fixes**. Honest feasibility
  ~72% (below the design's 78%) — docked for the missing client transport seam (2.2) and the absent
  path backstop (2.1), both of which are *unbuilt* and the design assumed they transfer from serve.rs when
  they do not.
- **Overall verdict: mixed (not refuted).** `determinism_holds=true` and `feasible_std_only=true` for the
  *gated* (Part A) surface, which is the harness contract; the Part-B caveats are non-gated hardening.
