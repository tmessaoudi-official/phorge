# Third-Party Notices

## Runtime dependencies: none

Phorge has **no third-party runtime dependencies**. The crate is `std`-only — it links **zero**
external crates. `Cargo.lock` contains a single entry: `phorge` itself. There is therefore no
third-party code compiled into the `phorge` binary or into any executable produced by `phg build`,
and no third-party license obligations attach to distributing them.

This is a deliberate design constraint (see [VISION.md](VISION.md)): it keeps the language buildable
in seconds, easy to audit in full, and free of supply-chain surface.

## Build- and distribution-time tooling (not linked, not distributed)

Some optional workflows shell out to external tools. These are **invoked as separate processes** at
build time — none of their code is linked into Phorge or into produced binaries, so their licenses do
not propagate to Phorge's output. They are only required for the workflows noted:

| Tool | Used for | License |
|---|---|---|
| Rust toolchain (`cargo`, `rustc`) | building Phorge | MIT OR Apache-2.0 |
| `llvm-objcopy` (LLVM) | embedding the program section in `phg build` | Apache-2.0 WITH LLVM-exception |
| [`cargo-zigbuild`](https://github.com/rust-cross/cargo-zigbuild) | cross-target builds (`build --target`/`--all`) | MIT |
| [`zig`](https://ziglang.org) | the C/linker driver for cross builds | MIT |
| `php` (optional) | round-trip-testing the transpiler output | PHP License |

If you build only the host target with the Rust toolchain, you need none of the cross-build tools.

## Phorge's own license

Phorge is dual-licensed MIT OR Apache-2.0 — see [LICENSE-MIT](LICENSE-MIT) and
[LICENSE-APACHE](LICENSE-APACHE).
