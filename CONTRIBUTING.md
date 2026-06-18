# Contributing to Phorge

Thanks for your interest in Phorge! This guide covers how to set up, the quality bar every change
must clear, and the correctness invariants that keep the project sound. By participating you agree to
the [Code of Conduct](CODE_OF_CONDUCT.md).

Phorge is pre-1.0 and developed by a single maintainer; the surface is still moving. Before investing
in a large change, please open an issue to discuss it first (see [SUPPORT.md](SUPPORT.md)).

## Development setup

```sh
git clone https://github.com/tmessaoudi-official/phorge
cd phorge
cargo build              # std-only — no dependencies to fetch
cargo test               # run the full suite
```

You need a stable Rust toolchain (edition 2021). Cross-builds for `phg build --target` additionally
need [`cargo-zigbuild`](https://github.com/rust-cross/cargo-zigbuild), `zig`, and `llvm-objcopy`, but
those are **not** required for normal development.

## The quality gate (must be green before every commit)

```sh
cargo test                   # all tests pass
cargo clippy --all-targets   # zero warnings — warnings are DENIED in the manifest
cargo fmt --check            # formatting matches rustfmt
```

A pre-commit hook enforces this. CI runs the same gate. A change is not done until all three are
clean. There is no `unsafe` in this crate — `#![forbid(unsafe_code)]` is set crate-wide and must stay.

## Test-driven by default

Write the failing test **before** the implementation, watch it fail, then make it pass. Every new or
changed behavior ships with a test that exercises it. Tests must be *run* (not just compiled) before a
change is considered complete.

- Unit tests live in `#[cfg(test)] mod tests` next to the code.
- Integration tests live in `tests/` (`cli.rs`, `build.rs`, `differential.rs`, the `*_integration.rs`
  suites).

## Correctness invariants (read before touching backends)

These are non-negotiable. The full list is in [`docs/INVARIANTS.md`](docs/INVARIANTS.md); the
load-bearing ones:

1. **Backend parity is the spine.** `phg run` (interpreter) and `phg runvm` (VM) must produce
   **byte-identical** output. The interpreter is the reference semantics; the VM matches it. Enforced
   by `tests/differential.rs`, which globs `examples/**/*.phg`. A standalone built binary is a third
   surface on the same spine and must match `runvm`.
2. **Adding an `Op` touches three exhaustive matches in the same commit:** `vm.rs::exec_op`,
   `chunk.rs::BytecodeProgram::validate`, and `compiler.rs::stack_effect`. Miss one and the build
   won't compile (by design).
3. **Value kernels are single-sourced.** Arithmetic and comparison live once in `value.rs`
   (`int_*`/`float_*`/`compare_ord`); both backends call them — never re-inline `checked_*`/
   `partial_cmp` in a backend, or the two will drift.
4. **Never panic on input (EV-7).** Lexer/parser/checker and the object-file section readers must
   reject adversarial or malformed input cleanly (a diagnostic or `None`), never a panic/SIGABRT. All
   offset arithmetic in the readers uses `checked_add`/`checked_mul`.
5. **Run a perf number before/after a perf change.** `phg bench <file>` measures both backends
   (median-of-N, output-identity gated).

## Architecture orientation

[`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) is a one-page module map. In short: `lexer` → `parser`
→ `checker` → (`interpreter` | `compiler`+`vm` | `transpile`). Design rationale and decision logs live
in `docs/specs/` (frozen designs) and `docs/plans/` (per-milestone plans) — these *are* the ADRs;
extend the relevant log rather than adding a separate `adr/` tree.

## Commit & PR conventions

- **Conventional-style prefixes:** `feat:`, `fix:`, `docs:`, `test:`, `refactor:`, `chore:`,
  optionally scoped (`feat(bundle):`, `feat(cli):`).
- **Small, green, self-contained commits.** Don't commit a broken build or red tests.
- One logical change per PR; fill in the [pull request template](.github/PULL_REQUEST_TEMPLATE.md).
- Update docs (`CHANGELOG.md`, the relevant `docs/` files, `examples/README.md`) in the same change
  when you alter a public interface or add an example.

## Adding an example

Drop a `.phg` file under `examples/` (or `examples/guide/` / `examples/realworld/`). It is
automatically byte-identity-gated by `tests/differential.rs` the moment it lands. Add a row to
`examples/README.md`.

## Reporting bugs & requesting features

Use the [issue templates](.github/ISSUE_TEMPLATE). For security issues, **do not** open a public
issue — follow [SECURITY.md](SECURITY.md).

## License of contributions

Contributions are dual-licensed under MIT OR Apache-2.0, matching the project (see
[README](README.md#license)). By submitting a contribution you agree to this.
