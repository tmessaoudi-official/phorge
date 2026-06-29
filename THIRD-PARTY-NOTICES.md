# Third-Party Notices

## Runtime dependencies: a tiny, feature-gated, all-permissive set

Phorj's **core is `std`-only**: built with `--no-default-features` it links **zero** external crates.
The default build enables four narrowly-scoped, **feature-gated** dependencies, admitted only for
capabilities `std` cannot provide safely from phorj's own `#![forbid(unsafe_code)]` code — crypto,
a ReDoS-safe regex engine, OS-signal handling, and stackful coroutines (the full policy and
clause-by-clause justification live in `docs/specs/2026-06-27-dependency-policy.md`). All four are
permissively dual-licensed (MIT OR Apache-2.0), compatible with Phorj's own license; each can be
switched off at build time.

| Crate | Feature (default-on) | Domain | License |
|---|---|---|---|
| [`argon2`](https://github.com/RustCrypto/password-hashes) (RustCrypto) | `crypto` | Argon2id password hashing | MIT OR Apache-2.0 |
| [`regex`](https://github.com/rust-lang/regex) (rust-lang) | `regex` | ReDoS-safe regex engine | MIT OR Apache-2.0 |
| [`ctrlc`](https://github.com/Detegr/rust-ctrlc) | `signals` | SIGINT/SIGTERM for `phg serve` | MIT OR Apache-2.0 |
| [`corosensei`](https://github.com/Amanieu/corosensei) | `green` (non-wasm only) | stackful coroutines for green threads | MIT OR Apache-2.0 |

Their transitive dependencies (argon2: `password-hash`/`base64ct`/`rand_core`/`getrandom`; regex:
`regex-automata`/`regex-syntax`/`aho-corasick`; corosensei: `cfg-if`/`libc`/`autocfg`) are likewise
MIT- or Apache/MIT-licensed. The **WASM playground** (`phorj-playground`) builds with all four features
off (plus `corosensei` is non-wasm-gated), so the in-browser build stays minimal.

Keeping the dependency set this small is a deliberate design constraint (see [VISION.md](VISION.md)):
the language stays buildable in seconds, auditable in full, and low on supply-chain surface.

## Build- and distribution-time tooling (not linked, not distributed)

Some optional workflows shell out to external tools. These are **invoked as separate processes** at
build time — none of their code is linked into Phorj or into produced binaries, so their licenses do
not propagate to Phorj's output. They are only required for the workflows noted:

| Tool | Used for | License |
|---|---|---|
| Rust toolchain (`cargo`, `rustc`) | building Phorj | MIT OR Apache-2.0 |
| `llvm-objcopy` (LLVM) | embedding the program section in `phg build` | Apache-2.0 WITH LLVM-exception |
| [`cargo-zigbuild`](https://github.com/rust-cross/cargo-zigbuild) | cross-target builds (`build --target`/`--all`) | MIT |
| [`zig`](https://ziglang.org) | the C/linker driver for cross builds | MIT |
| `php` (optional) | round-trip-testing the transpiler output | PHP License |

If you build only the host target with the Rust toolchain, you need none of the cross-build tools.

## Phorj's own license

Phorj is dual-licensed MIT OR Apache-2.0 — see [LICENSE-MIT](LICENSE-MIT) and
[LICENSE-APACHE](LICENSE-APACHE).
