# Secret<T> design (Fork B)

> Status: **design-locked** 2026-06-28 (developer-resolved). Resolves GA-sequence Fork B.
> SSOT: `php-parity-and-beyond.md:269` (`K-secrets-type` — `Secret<T>` + `#[\SensitiveParameter]`).

## Resolved fork (developer, 2026-06-28)

**Path 1 — opaque & non-printable (loud).** Not a runtime-`***`-rendering wrapper (that would need a
new `Value` variant + a *silent* `***`). Instead: a `Secret<T>` value simply **isn't a string and has
no display**, so any attempt to print/interpolate it is a clean **compile error** — the strongest,
loudest guarantee, and it falls out of the type system for free. `.expose()` is the sole read path; a
`W-SECRET` lint nudges when an exposed plaintext flows to a sink. Most Phorj-idiomatic (loud > silent;
no new `Op`/`Value`).

Decided after an implementation discovery reopened the earlier "displays as `***`" wording: Phorj's
display path (`as_display`) renders only primitives, so a class-typed `Secret` is already unprintable —
no `***` machinery is needed or wanted.

## Model — an injected generic class, zero new `Op`/`Value`/`Ty`

`Secret<T>` is a compiler-injected generic class (reusing generics-all + methods + visibility +
construction wholesale — the `Box<T>` machinery):

```phorj
class Secret<T> {
  constructor(private T value) {}
  function expose(): T { return this.value; }
}
```

- **Injected** by `cli::inject_secret_prelude` when a program imports `Core.Secret;` (mirrors
  `inject_regex_prelude`). `import Core.Secret;` is a valid import even with no native module under it
  (import existence is checked lazily, only at call-qualifier resolution — verified). A user-declared
  `Secret` class wins (injection is skipped), like every other prelude.
- **Construction**: `new Secret(apiKey)` → `Secret<string>` (the type argument is inferred at
  construction by the generic-class unifier, exactly like `Box(7)`).
- **The field is `private`**, so `s.value` is a visibility error — `.expose()` is the only read path.
- **Non-printable**: a `Secret` instance is not a `string`, so `Console.println(s)` / `"{s}"` is a
  clean type error (`println` expects `string`). This is the primary guarantee — by construction.

## `W-SECRET` lint — the secondary nudge

A non-fatal warning (the warning channel — `check()` returns `Ok(warnings)`, rendered to stderr, never
gates the build, same as `W-FORCE-UNWRAP`/`W-UNREACHABLE`). It fires when **`<recv>.expose()` appears
as a direct argument to a known sink** — `Console.println`, `Console.print`, `Core.File.write` — and
the receiver's type is `Secret<_>`. Message: *"exposing a Secret directly into a sink — the plaintext
will be logged/persisted."* Code `W-SECRET`, documented via `phg explain W-SECRET`.

**Scope (documented in KNOWN_ISSUES):** the lint is *syntactic* on the direct sink argument — a value
laundered through a local (`var p = s.expose(); println(p);`) is **not** flagged. Full taint/flow
analysis is out of scope (the SSOT notes by-construction `Secret` dominates taint tracking); the
type-system non-printability is the real guarantee, the lint is a convenience for the common mistake.

## Transpile (peer emission target)

`Secret<T>` → a PHP `final class Secret` with a `#[\SensitiveParameter]`-annotated promoted constructor
parameter (so the value is redacted in PHP stack traces — the `K-secrets-type` intent) and an
`expose()` method:

```php
final class Secret {
  function __construct(#[\SensitiveParameter] private mixed $value) {}
  function expose(): mixed { return $this->value; }
}
```

(`T` erases to `mixed` via the existing generic-class erasure — no special handling.) `final` because a
secret wrapper must not be subclassable. The transpiler adds the `#[\SensitiveParameter]` attribute +
`final` only for the injected `Secret` class (keyed by class name), leaving all other classes unchanged.

## Build slices

1. **Inject + spec**: `cli::inject_secret_prelude` + wire into `check_and_expand`; the type resolves and
   `new Secret(x)`/`.expose()` work on both Rust backends (pure class machinery, no backend change).
2. **W-SECRET lint**: at the sink-native call check, flag a direct `expose()` argument on a `Secret`
   receiver; `phg explain W-SECRET`.
3. **Transpile tweak**: emit `final` + `#[\SensitiveParameter]` for the injected `Secret` class.
4. **Example + tests**: `examples/guide/secret.phg` (construct, expose for legitimate use, show the
   non-printable type error in a comment/README); checker tests for non-printability + W-SECRET;
   byte-identity gate run≡runvm≡real PHP 8.5.
