# Deprecation policy

How Phorge retires a part of its public surface without breaking users silently. This complements
[`../SEMVER.md`](../SEMVER.md) (when breaks are allowed) and [`../STABILITY.md`](../STABILITY.md) (which
surface is stable vs experimental vs deprecated).

## The lifecycle

A symbol (a stdlib function, a language construct, a CLI command/flag) goes through three stages before
it is removed:

1. **Live** — fully supported, in the *stable* or *experimental* tier.
2. **Deprecated** — still works, but slated for removal. It is:
   - moved to the **deprecated** tier in `STABILITY.md`, with its replacement and removal version;
   - announced under a **`### Deprecated`** heading in `CHANGELOG.md`;
   - flagged so that *using* it emits the **`W-DEPRECATED`** lint, naming the replacement and the
     removal version (`phg explain W-DEPRECATED`).
3. **Removed** — deleted. The removal is a **`### Breaking`** `CHANGELOG.md` entry. Before `1.0` this
   may happen in a minor release; at/after `1.0` removal of a *stable* symbol is a MAJOR bump (a
   deprecated symbol may be removed in the next MAJOR — see `SEMVER.md`).

## Guarantees

- **At least one minor release** elapses between a symbol becoming *deprecated* and being *removed*, so
  every user gets a build that warns before the build that breaks.
- **`W-DEPRECATED` never fails the build** — it rides the warning channel (stderr), like every `W-…`
  lint. It is advisory; the deprecated symbol behaves exactly as before until removal.
- **A replacement is always named.** A deprecation that offers no migration path is not shipped; if a
  capability is going away entirely, that is a *breaking removal* (with rationale), not a deprecation.

## How it's wired (for contributors)

Deprecations live in a single side table, `native::deprecation_of(module, name)` in
`src/native/mod.rs` — *not* a field on every `NativeFn` (so flagging a symbol is a one-line change and
non-deprecated natives pay nothing). To deprecate a stdlib native, add an entry returning a
`Deprecated { replacement, removed_in }`; the checker's `check_native_call` emits `W-DEPRECATED`
automatically. Then update `STABILITY.md` (move it to the deprecated tier) and `CHANGELOG.md`.

The table is **empty in the shipping build** today — the mechanism is in place ahead of the first real
deprecation (a `#[cfg(test)]` sample exercises the lint end-to-end). Deprecating a *language construct*
or a *CLI command* follows the same lifecycle; only the detection point differs (the checker / the CLI
dispatch) — a `deprecated` user-facing modifier on user code is a later addition, intentionally not
part of this minimal mechanism.
