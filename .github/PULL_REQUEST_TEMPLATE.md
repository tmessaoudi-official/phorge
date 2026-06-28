<!-- Thanks for contributing to Phorj! Please read CONTRIBUTING.md first. -->

## What & why

<!-- What does this change, and why? Link any related issue (e.g. "Closes #12"). -->

## Type of change

- [ ] Bug fix
- [ ] New feature
- [ ] Refactor (no behavior change)
- [ ] Docs / tests / chore

## Checklist

- [ ] `cargo test` is green
- [ ] `cargo clippy --all-targets` is clean (warnings are denied)
- [ ] `cargo fmt --check` is clean
- [ ] New/changed behavior has a test, written test-first
- [ ] No `unsafe` introduced (`#![forbid(unsafe_code)]` stays intact)
- [ ] Docs updated where a public interface changed (README / CHANGELOG / docs / examples)

## Backend-parity impact (if touching language semantics)

<!-- The interpreter (`run`) and VM (`runvm`) MUST stay byte-identical — see docs/INVARIANTS.md. -->

- [ ] No semantic change, OR
- [ ] `tests/differential.rs` still passes and (if needed) a new differential case was added
- [ ] If an `Op` was added: `vm.rs::exec_op`, `chunk.rs::validate`, and `compiler.rs::stack_effect`
      were all updated in this PR

## Notes for the reviewer

<!-- Anything non-obvious, trade-offs made, follow-ups deferred. -->
