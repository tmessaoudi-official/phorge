# Phorj external-dependency policy

> Status: **adopted** 2026-06-27 (developer). Phorj has been `std`-only / zero-dependency since
> inception. This document defines the **single, narrow exception** under which an external crate may
> be admitted, and records the first one.

## The rule

Phorj's core (lexer, parser, checker, interpreter, VM, transpiler, loader, bundler) **remains
`std`-only**. The build admits an external crate **only** when ALL of these hold:

1. **Domain is a security-critical primitive where rolling-your-own is the anti-pattern.** Two
   sub-domains qualify, no others:
   - **Crypto** (password hashing, AEAD, signatures, constant-time comparison) — *"never roll your
     own."*
   - **Untrusted-input parsers where a safe engine cannot be built in `std`** — specifically a
     **regex** engine: a from-scratch matcher over attacker-controlled patterns/input is a ReDoS and
     correctness hazard, and a *vetted* linear-time (finite-automaton) engine is strictly safer than
     anything hand-rolled. The same "never roll your own" logic applies.

   Convenience, performance, general-purpose, or *parsing-for-formats* crates (JSON, TOML, YAML,
   HTTP) do **not** qualify — those are done in `std` today. The bar is *security-critical primitive
   that `std` lacks and that is dangerous to implement by hand*, not "parsing" broadly.
2. **The crate is independently audited / widely-vetted** (e.g. the RustCrypto org, ring) with an
   active maintenance record. A from-scratch or unaudited crypto implementation is **never** admitted —
   that would be *more* dangerous than the dependency.
3. **There is no `std`-only path that is both secure and Phorj-native.** Delegating the capability to
   the PHP transpile target is **not** an acceptable substitute: the transpile/lift bridges exist only
   to migrate from PHP and to test Phorj against a reference — Phorj's own runtime (the Rust
   interpreter/VM) must implement every feature natively. A feature that runs *only* after transpiling
   to PHP is a delegation and is disallowed.
4. **It is feature-gated** so the WASM playground (which must stay tiny + browser-safe) can build
   without it.

If a candidate fails any clause, the feature is deferred — it does not justify a dependency.

## Why these two domains, and nothing wider

Both admitted domains share one shape: **a security-critical primitive that `std` does not provide
and that is dangerous to implement by hand.**

- *Crypto* — rolling your own password hash / AEAD is the canonical security anti-pattern; `std`
  ships no crypto. One responsible source: a vetted crate.
- *Regex* — a matcher over attacker-controlled patterns/input is a ReDoS hazard (catastrophic
  backtracking) and a subtle-correctness hazard. A *vetted, linear-time finite-automaton* engine
  (RE2-style) is **strictly safer** than a hand-rolled NFA, which would itself be new
  security-sensitive code with far less testing. `std` has no regex.

Clauses 1–3 keep this principled, not a slippery slope: format parsers (JSON/TOML/HTTP) are done in
`std` and do **not** qualify; the bar is "vetted security-critical primitive `std` lacks, not
PHP-delegated." Each engine must run **natively on the Rust backends** — the PHP transpile is a
migration/test bridge, never a runtime Phorj depends on.

## Admitted dependencies

| Crate | Version | Domain | Used by | Feature gate | Justification |
|-------|---------|--------|---------|--------------|---------------|
| `argon2` (RustCrypto) | 0.5.x | Argon2id password hashing | `Core.Crypto` | `crypto` (default; off for `phorj-playground`) | OWASP #1 password KDF; audited; no `std` equivalent; must run on the Rust backends (not PHP-delegated). Emits standard PHC strings → interoperates with PHP `password_verify`. |
| `regex` (Rust project / BurntSushi) | 1.x | ReDoS-safe regex engine | `Core.Regex` | `regex` (default; off for `phorj-playground`) | RE2-style finite automaton, **guaranteed linear-time / ReDoS-immune**, exhaustively fuzzed; no `std` regex; runs on the Rust backends. Its restricted feature set (no backref/lookaround) is exactly the regular subset PHP `preg_*` matches identically, so the byte-identity spine holds; unsupported patterns are rejected at `Regex.compile`. |

Transitive (argon2): `password-hash`, `base64ct`, `rand_core`/`getrandom` (salt entropy) — same audit
umbrella. Transitive (regex): `regex-automata`, `regex-syntax`, `aho-corasick` — all Rust-project/BurntSushi,
same umbrella.

## Process to admit the next one

A new crate requires: (1) an entry in the table above with the clause-by-clause justification, (2) a
note in `CHANGELOG.md`, (3) feature-gating verified against the playground build. Anything outside the
two admitted domains (crypto, ReDoS-safe regex) requires revisiting this policy itself, not just
adding a row.
