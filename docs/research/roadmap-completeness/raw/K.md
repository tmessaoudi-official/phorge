# Track K — Security & safety posture (GA M8)

## Track summary

Phorj's security story today is **strong on the toolchain-as-software axis and weak on the
application-as-product axis** — the two are different threat models and only the first is mostly
closed. The *toolchain* is excellent: `#![forbid(unsafe_code)]` crate-wide, never-panics-on-input
(EV-7) for both adversarial source and adversarial binaries, depth-limited recursion on a 256 MB
worker, zero external runtime dependencies (no supply-chain surface for the runtime itself),
offline-only vendoring with `--`-separated/scheme-allowlisted git invocations, path-traversal and
symlink-escape guards, lockfile content-hash verification, atomic state writes, and a single-threaded
`phg serve` with body caps, slowloris timeout, and `catch_unwind`-isolated handlers + prod-no-leak
500s. Most of this is M8 work that is *already designed and largely landed* (the GA roadmap M8 list,
plus the shipped serve/vendor hardening). XSS-safe-by-construction is real and shipped (`Core.Html`).

The gap is everything an **application written in Phorj** needs to be secure: there is **no crypto
(`password_hash`/`hash_hmac`/`random_bytes`)**, **no CSPRNG**, **no SQL story at all** (no PDO, no
prepared statements, so the language has no answer to SQL injection — the #1 PHP CWE), **no
command/path injection-safe natives**, **no secrets/env handling**, **no auth/session/CSRF helpers**,
and **no `phg audit`** for the vendored supply chain (vendoring is pinned + hashed, but nothing checks
those pins against an advisory database). The transpile-determinism rule (`run ≡ runvm ≡ PHP`
byte-identical) is the reason most of these are absent — crypto, RNG, time, and network are all
non-deterministic — so the *mechanism* that unblocks them is a **fixture/seam discipline** (injected,
seedable, deterministic-under-test), the same pattern the parity spec already names for
`Core.Random`/`Core.Time`. The honest 1.0 security posture is: **"a safe toolchain and an XSS-safe
view layer, with a documented, deterministic path to the rest of the OWASP surface that mostly lands
post-1.0."** Several language-feature-level items (opaque newtypes, `#[SensitiveParameter]`,
capability-passing, `Core.Process`/`Core.Random`/`Core.BigInt`) are **already captured in the Track A
PHP-parity deliverable** (`docs/specs/2026-06-21-php-parity-and-beyond.md`) — I cross-reference rather
than duplicate those, and add the systemic posture items they don't cover.

## Gaps

| id | title | kind | fit | rec | milestone | effort |
|----|-------|------|-----|-----|-----------|--------|
| K-sql-prepared | SQL access via parameterized queries only (no string-built SQL) | new | strong | defer | M6/M11 (`Core.Sql`) | L |
| K-crypto-stdlib | `Core.Crypto`: password hashing, HMAC, constant-time compare, digests | new | strong | adopt | M8 + M11 | M |
| K-csprng | `Core.Random` seedable CSPRNG (deterministic-under-test seam) | new | strong | adopt | M8/M11 | M |
| K-secrets-type | Secret/redacted type + `#[SensitiveParameter]` (trace redaction) | port | strong | adopt | M8 | S |
| K-env-config | `Core.Env` config/secret reading (no ambient superglobals) | new | strong | adopt | M8/M11 | S |
| K-audit-cmd | `phg audit` — advisory check of vendored deps + lockfile integrity | new | strong | defer | M12 | M |
| K-shell-path-safe | Injection-safe `Core.Process` (argv-array) + `Core.Path` join/normalize | new | strong | defer | M11 | M |
| K-file-capability | `Core.File` capability/sandbox model (root-jailed paths) | new | ok | defer | post-1.0 | L |
| K-html-context-escape | Context-aware escaping (URL/JS/CSS/attr) beyond text+attr-value | port | strong | adopt | M11 | M |
| K-auth-csrf-session | Auth/CSRF/session helpers in the web layer | new | ok | defer | post-1.0 (M6 follow-up) | L |
| K-taint-tracking | Taint tracking (untrusted-string flow into sinks) | new | weak | reject | — | L |
| K-deser-safe | Safe (de)serialization — no PHP `unserialize` gadget surface | map | strong | adopt | M11 (`Core.Json`) | S |
| K-supply-chain-vendor-min | Minimal/auditable vendored tree (copy only `src/`, not arbitrary files) | port | strong | adopt | M8 | S |
| K-security-doc | A first-class "Security model" doc (threat model + app-dev guidance) | port | strong | adopt | M12 | S |
| K-csp-headers | Security response headers helper (CSP/HSTS/X-Frame) in web layer | new | ok | defer | post-1.0 (M6) | S |
| K-int-overflow-story | Document checked-arithmetic as a security property (no silent wrap) | map | strong | adopt | M12 | S |
| K-fuzz-harness | Continuous fuzzing of lexer/parser/binary-readers (cargo-fuzz, CI) | new | strong | adopt | M9 | M |
| K-timing-safe-eq | Constant-time string/bytes equality primitive | new | strong | adopt | M8 (with K-crypto) | S |

## Rationale for ADOPT items

**K-crypto-stdlib** — A 1.0 language that transpiles to PHP and targets web work cannot ship without a
hashing/HMAC/digest story. PHP devs reach for `password_hash`, `hash_hmac`, `hash_equals` reflexively;
their absence forces every Phorj web app to drop into raw PHP or hand-roll crypto. The transpile maps
cleanly to PHP's `password_hash`/`password_verify`/`hash_hmac`/`hash`/`hash_equals` (1:1 erasure, like
every other native). The determinism objection (hashing with a random salt is non-deterministic) is
**not a blocker for the digest/HMAC subset** (`sha256`, `hmac` with a given key are pure functions —
byte-identical) and is handled for password hashing by **excluding it from the byte-identity oracle**
the same way faults are excluded (it's a side-effecting native, tested by round-trip property not
stdout equality). Ship the pure subset (digests, HMAC, constant-time compare) in M8 alongside the
hardening work; ship `passwordHash`/`verify` in M11 behind the fixture seam.

**K-csprng** — `random_bytes`/`random_int` is the foundation under tokens, session ids, CSRF, and
crypto. It is irreducibly non-deterministic, so it must follow the **injected-seam pattern the parity
spec already named for `Core.Random`**: a `Core.Random` whose default source is the OS CSPRNG but is
*seedable for tests*, and excluded from the byte-identity oracle (or run with a fixed seed in the
oracle). This is the single primitive that unblocks K-secrets, K-auth-csrf, and the token half of
K-crypto. Adopt the seam design in M8 (it's the gating mechanism), land the native in M11.

**K-secrets-type** — `#[SensitiveParameter]` (PHP 8.2) is **already an `adopt` in the Track A
deliverable** (line 319, "pure passthrough on a parameter target"). I re-surface it here because it is
the security-posture half of the now-shipped stack-trace work: the `phg serve --dev` HTML error page
and the CLI stack trace can leak secrets in frame arguments, exactly the leak `#[SensitiveParameter]`
exists to prevent. Coupling it to the trace renderer (redact a `Secret`-typed or annotated value) is a
small, high-leverage win that makes the prod-no-leak guarantee complete. Pairs naturally with a
`Secret<T>` opaque newtype (K-secrets-type ⊂ the opaque-newtype work already adopted at line 347/434).

**K-env-config** — Phorj's "nothing in the wind" philosophy *forbids* PHP's ambient `$_ENV`/`getenv`
superglobals, which is the right call — but it leaves no sanctioned way to read configuration/secrets.
A small `Core.Env` (read-only, explicit-key) native fills the hole the philosophy creates; it erases
to `getenv()`/`$_ENV[...]`. Non-deterministic, so excluded from the oracle (a test injects fixed
values). Small effort, removes a real adoption blocker for any deployable app.

**K-html-context-escape** — KNOWN_ISSUES already documents that `Core.Html` escaping covers *text and
quoted-attribute-value contexts only* — it is explicitly **not safe** for URL contexts
(`href="javascript:…"`), inline CSS, or `<script>` bodies. That is a real XSS gap in the one place
Phorj claims injection-safety-by-construction. Closing it (a `urlAttr`/`safeUrl` builder that
validates the scheme, a `jsValue`/`cssValue` context escaper) is the natural completion of the
shipped Html work and keeps the "XSS-safe by construction" claim honest at 1.0.

**K-deser-safe** — PHP's `unserialize()` is the classic object-injection gadget vector
(CWE-502). Phorj's planned `Core.Json` (M11) is the chance to ship a **data-only** deserialization
story (JSON → a typed `Any`/`Json` value, never object instantiation), structurally closing the
gadget-chain class that plagues PHP. This is a `map` (already-planned `Core.Json` gets a security
framing) more than new work — adopt the *constraint* (no object materialization from untrusted input)
into the M11 `Core.Json` design now.

**K-supply-chain-vendor-min** — `phg vendor` copies a dependency's source into `vendor/`. Restricting
the copy to the dependency's declared `src/` source-root (it already validates folder=path per dep)
and refusing to vendor symlinks/executables/dotfiles outside it shrinks the attack surface of a hostile
dependency to exactly its Phorj source. Small hardening that rides the M8 symlink-escape work already
on the roadmap.

**K-security-doc** — SECURITY.md today is a good *toolchain* threat model + reporting policy, but a 1.0
needs a first-class **security model** doc aimed at the *application developer*: what Phorj guarantees
(no panics, checked arithmetic, XSS-safe Html, prod-no-leak), what it does NOT (no taint tracking, no
sandbox, `Core.File` is unrestricted), and the recommended patterns for the OWASP top categories. This
is the artifact that lets a GRDF-style reviewer sign off; it costs a doc, not code.

**K-int-overflow-story** — Phorj's checked arithmetic (overflow/div-by-zero → clean fault, never
silent wrap) is a genuine **security property** — integer-overflow-to-wrap is a real CWE class that
PHP's silent float-promotion hides. It is shipped; the gap is purely that it is framed as a
"correctness" feature, not a security one. A `map` — surface it in the security doc as a guarantee.

**K-fuzz-harness** — EV-7 ("never panics on input") is the toolchain's central security claim, and it
is currently defended by hand-written adversarial fixtures only. A continuous `cargo-fuzz` harness over
the lexer/parser and the ELF/PE/Mach-O binary readers (the two untrusted-input surfaces named in
SECURITY.md) would turn EV-7 from an asserted invariant into a continuously-tested one, in CI. This is
the highest-ROI *verification* investment for the security story and fits the M9 engineering-hygiene
milestone. Std-only-runtime is preserved — cargo-fuzz is dev-tooling, exempt like clippy/zig.

**K-timing-safe-eq** — Constant-time comparison (PHP `hash_equals`) is a tiny primitive but the
required building block for any token/HMAC verification done in Phorj itself; ship it with the
K-crypto digest subset in M8 so the crypto natives have a safe comparison to recommend.

## Notes on DEFER / REJECT items

- **K-sql-prepared (defer):** the single biggest *application* security gap (SQL injection is PHP's
  dominant CWE), but it needs a DB connectivity story that is M6's stated scope (Postgres) and is
  irreducibly stateful/non-deterministic — quarantined like the socket. The *design constraint* worth
  locking now: when `Core.Sql` lands, **string-built SQL must be impossible** — only a parameterized
  `query(sql, params)` / a typed query builder, never `query("... " + userInput)`. Capture the
  constraint; build post-keystone-generics (M11).
- **K-shell-path-safe / K-file-capability (defer):** `Core.Process` is already named as a future native
  (argv-array, never a backtick operator — the parity spec rejects backticks for exactly this reason).
  A path-join/normalize native and a root-jailed `Core.File` are the path-traversal analogue; defer to
  M11/post-1.0 but lock the argv-array-only constraint.
- **K-auth-csrf-session / K-csp-headers (defer):** these are web-framework concerns that sit on top of
  M6's `handle(Request) -> Response` model and the CSPRNG seam; they are genuinely post-1.0 (a 1.0
  language is not obligated to ship a web framework), but the M6 design should leave room.
- **K-taint-tracking (reject):** automatic taint tracking is research-grade (PL-theory maximalism that
  doesn't earn its surprise budget for a PHP-familiar audience) and is **strictly dominated** by the
  by-construction approach Phorj already takes — opaque `Secret` types, XSS-safe `Html`, and
  parameterized-only SQL make the *unsafe form unrepresentable*, which is more legible to a PHP dev than
  a flow analysis they must trust. Reject in favor of the type-driven story.

---

## Critic pass

**Verification basis** (all [Verified] against the repo at the current `master`): read `FEATURES.md`,
`KNOWN_ISSUES.md`, `SECURITY.md`, `ROADMAP.md`, `docs/MILESTONES.md`, the GA roadmap plan
(`docs/plans/2026-06-19-phorj-ga-roadmap.plan.md`), the parity deliverable
(`docs/specs/2026-06-21-php-parity-and-beyond.md`), and `src/vendor.rs`; grepped `src/native.rs` for
the shipped stdlib surface (`Core.{Bytes,Console,File,Html,List,Map,Math,Set,Text}` — no crypto, no
random, no env, no secret natives, confirming K-crypto/K-csprng/K-env/K-secrets are genuinely absent).
One WebSearch confirmed PHP 8.2's `Random\Engine\Secure` / `Random\Randomizer` is the *mockable* OOP
CSPRNG seam — i.e. K-csprng's injected-seam framing maps to a real, modern PHP API.

### Mis-listings (already shipped / over-scoped)

- **K-supply-chain-vendor-min — LARGELY ALREADY SHIPPED; narrow it.** [Verified: `src/vendor.rs:203-236`,
  `copy_phg_tree`/`copy_phg_rec`] `phg vendor` already copies **only `.phg` files** (it walks the dep's
  source root and copies a file *only if* `extension == "phg"`), and already rejects any path that
  escapes the source root (`"escaped its source root"`). The headline of this gap — "copy only `src/`,
  not arbitrary files" — is therefore **done**: executables, dotfiles, and non-`.phg` content are never
  vendored. The genuine residual delta is **symlink refusal during the copy walk**, and that is already
  on the M8 roadmap as **P2-#36** ("manifest source / vendor copy / project walk follow symlinks →
  escape root"). Recommendation: keep the row but **re-scope to the symlink-refusal residual only**, and
  note it is a duplicate of M8 P2-#36 rather than new work. Net: not removed (a real residual exists),
  but its effort and novelty shrink to ~nil. *(Counts as a mis-listing: the row as written claims
  unshipped work that is shipped.)*

- **K-fuzz-harness — already on the roadmap (not a new finding).** [Verified: GA plan M12, **P2-#44**
  "No lexer/parser fuzzing/property testing … (new fuzz harness)"] The continuous-fuzz item already
  exists in the committed GA roadmap (M12). The researcher proposed M9; the existing placement is M12.
  Keep the row (the *binary-reader* fuzz target — ELF/PE/Mach-O — is a worthwhile explicit addition the
  roadmap's "lexer+parser" wording omits) but mark it **adopt-as-already-planned**, and prefer the
  roadmap's M12 home (it wants CI from M9 in place first) unless the dev pulls it forward. Not removed.

### Newly-found gaps (genuine long-tail this track missed)

| id | title | kind | fit | rec | milestone | effort |
|----|-------|------|-----|-----|-----------|--------|
| K-header-injection | `phg serve` response header-injection / smuggling guard (reject CR/LF in header names+values; one Content-Length; reject Transfer-Encoding) | port | strong | adopt | M8 | S |
| K-hashdos-immunity | Document Map/Set HashDoS-immunity as a security property (Vec-backed, no hash table → no collision flooding) | map | strong | adopt | M12 | S |
| K-redos-constraint | Lock a ReDoS-safe constraint for a future `Core.Regex` (linear-time engine or input/step cap; never raw PCRE backtracking) | new | ok | defer | M11/post-1.0 | M |
| K-artifact-integrity | Reproducible-build + per-artifact SHA-256 framed as a *supply-chain integrity* property (deterministic `.phorj` embed; checksums published) | map | strong | adopt | M12 | S |
| K-dep-provenance | `phg vendor` surfaces each dep's resolved commit SHA + license (SBOM-lite provenance record) | new | ok | defer | M12 (with `phg audit`) | S |
| K-serve-handler-budget | Per-request wall-clock/step budget for `phg serve` handlers (busy-loop DoS; depth-limits cover recursion, not a tight loop) | new | ok | defer | post-1.0 (M6) | M |

**Rationale for the new items:**

- **K-header-injection** — M6 W1's `Response` carries headers as **raw `List<string>` lines** (KNOWN_ISSUES
  / M6 W1 design); a handler that interpolates user input into a header line can inject CRLF and split the
  response (header injection / cache poisoning / response splitting — CWE-113). M8's planned Content-Length
  *framing* fix (P2-#28) is about *parsing the request*, not *sanitizing the response* — this is a distinct
  surface. The fix (reject CR/LF in header names/values at `serialize_response`, enforce a single
  Content-Length, reject a Transfer-Encoding the single-threaded server doesn't honor) is small, rides the
  shipped serve path, and keeps the "resilient server" claim honest. **Strong fit** — a by-construction
  rejection, the same legible style as the rest of the security story.

- **K-hashdos-immunity** — [Verified: `KNOWN_ISSUES.md` "Maps"/"Generic natives" + `eq_val` notes; `Value::Map`/
  `Value::Set` are insertion-ordered `Rc<Vec<…>>`, **not** a `HashMap`/`HashSet`] PHP arrays are hash tables
  and historically a HashDoS target; Phorj's Vec-backed insertion-ordered maps have **no hash bucket to
  flood** — collision attacks are structurally impossible (the cost is O(n) lookup, a perf trade, not a
  security hole). This is a genuine *beyond-PHP security property* that ships today and is currently framed
  only as an R1-ordering/perf choice. Pure `map`/doc work, exactly parallel to **K-int-overflow-story**;
  belongs in the same K-security-doc guarantees list.

- **K-redos-constraint** — Phorj ships **no regex engine today** [Verified: `Core.*` grep shows no
  `Core.Regex`], so there is no ReDoS surface *now* — but `Core.Text` will eventually want pattern matching,
  and PHP's PCRE is a notorious catastrophic-backtracking (ReDoS, CWE-1333) vector. Lock the constraint *now*
  (mirroring K-sql-prepared's "lock the constraint before building"): a future `Core.Regex` must be a
  linear-time engine (RE2/Thompson-NFA style) or carry a step/length budget — never expose raw PCRE
  backtracking, even though the transpile target *is* PCRE. **Defer** (no engine yet) but capture so the gap
  isn't re-discovered when text-matching lands. Fit is `ok` (the transpile-to-PCRE mismatch needs a careful
  determinism story).

- **K-artifact-integrity** — The GA plan already schedules "SHA-256 checksums + release automation" (M12,
  the unblocked half of M2.5 Phase 3), but frames it as *release mechanics*, not as the **supply-chain
  integrity** guarantee it actually is: a reproducible build + per-artifact checksum is what lets a consumer
  verify a `phg build` binary or a release tarball wasn't tampered with. This is a `map` (re-frame existing
  M12 work in the security doc as an integrity property) — the security analogue of K-int-overflow-story for
  the *distribution* axis. Small.

- **K-dep-provenance** — `phg vendor` already records the resolved commit SHA + a content hash in
  `phorj.lock` [Verified: M5 S3 in CLAUDE.md / `src/lock.rs` — `name`/`git`/`rev`/`hash`], so the data exists;
  the missing piece is *surfacing* it (and each dep's declared license) as a readable provenance/SBOM-lite
  record. Complements **K-audit-cmd** (advisory check) on the same supply-chain axis: audit answers "is this
  dep known-vulnerable?", provenance answers "what exactly am I shipping and under what license?". Defer to
  the same M12 release-automation milestone as `phg audit`. Fit `ok` (license parsing is mild scope creep).

- **K-serve-handler-budget** — `src/limits.rs` depth caps + the 256 MB worker stack defend against *recursive*
  blowup and the request body cap + slowloris timeout defend the *I/O* path [Verified: SECURITY.md, M8
  P2-#29], but **nothing bounds a handler that enters a tight CPU loop** (`while (true) {}`) — a single
  request can wedge the single-threaded server indefinitely. A per-request wall-clock or VM-step budget
  (the VM already has a dispatch loop where a step counter is cheap to thread) is the by-construction fix.
  Genuinely post-1.0 (web-layer DoS hardening), fit `ok` — it is real engineering, not a doc.

### Sanity-check of the original recommendations against the philosophy

All original ADOPTs survive the philosophy lens — each maps 1:1 to a PHP API (`password_hash`/`hash_hmac`/
`hash_equals`/`random_bytes`/`getenv`/`#[SensitiveParameter]`/`htmlspecialchars`-family) or is pure doc/map
framing of a shipped guarantee, and each removes a *surprise* (silent overflow, secret-in-trace, unsafe URL
escape) without removing capability. The single **reject** (K-taint-tracking) is correctly reasoned: a flow
analysis is PL-theory maximalism strictly dominated by Phorj's by-construction story (opaque `Secret`, XSS-
safe `Html`, parameterized-only SQL), which is the more PHP-legible answer — **concur, reject stands.** The
defers (SQL/Process/File-capability/auth-CSRF/CSP) are correctly gated on M6 DB/web scope and the CSPRNG seam;
no defer should be pulled forward. No original recommendation needs reversing.
