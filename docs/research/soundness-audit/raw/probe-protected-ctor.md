# Probe: protected constructor restricts to subclasses/same-class

**Rule under test:** A `protected constructor` should restrict instantiation to the same class
and its subclasses — an *external* `new C(...)` (outside the class hierarchy) must be REJECTED.

**Verdict: GAP — P0 (unsound).** The `protected` modifier on a constructor is parsed and
silently dropped. External `new` on a class with a `protected constructor` checks clean and runs.
This is the same root cause as the seed bug (private constructor) — constructor modifiers carry no
semantics at all.

## Probe 1 — external `new` on a protected-ctor class

Program (`$TMP/protected-ctor.phg`):

```phorj
package Main;
import Core.Console;

class Secret {
    protected constructor(public int x) {}
}

function main(): int {
    Secret s = new Secret(7);
    Console.println("{s.x}");
    return 0;
}
```

Command + output:

```
$ /stack/projects/phorj/target/release/phg check protected-ctor.phg
OK (type-checks clean)
exit=0

$ /stack/projects/phorj/target/release/phg run protected-ctor.phg
7
exit=0
```

External `new Secret(7)` outside the class is accepted and runs → **the `protected` modifier has
no effect.** (Should be rejected with a visibility error.)

## Probe 2 — subclass `new` (legitimate) vs external `new` (illegitimate) in one program

Program (`$TMP/protected-ctor-subclass.phg`):

```phorj
package Main;
import Core.Console;

open class Secret {
    protected constructor(public int x) {}
    function value(): int { return this.x; }
}

class Derived extends Secret {
    function doubled(): int { return this.x * 2; }
}

function main(): int {
    Derived d = new Derived(10);   // subclass: legitimate
    Console.println("{d.doubled()}");
    Secret s = new Secret(42);     // external: SHOULD be rejected
    Console.println("{s.value()}");
    return 0;
}
```

Command + output:

```
$ /stack/projects/phorj/target/release/phg check protected-ctor-subclass.phg
OK (type-checks clean)
exit=0

$ /stack/projects/phorj/target/release/phg run protected-ctor-subclass.phg
20
42
exit=0
```

Both the legitimate subclass construction (`20`) **and** the illegitimate external construction
(`42`) succeed. There is no enforcement boundary at all — Phorj cannot distinguish "subclass
access" from "external access" for constructors because the visibility is discarded at parse time.

## Root cause (confirmed in source)

`src/parser/items.rs:510-526`:

```rust
/// One class member: a field, a constructor, or a method. Modifiers preceding
/// `constructor` are consumed and dropped (M1: constructors are implicitly public).
pub(super) fn parse_class_member(&mut self) -> Result<ClassMember, Diagnostic> {
    let sp = self.peek_span();
    let modifiers = self.parse_modifiers();      // <-- parses `protected`/`private`/`public`
    match self.peek() {
        TokenKind::Constructor => {
            self.advance();
            self.expect(&TokenKind::LParen, "'(' after 'constructor'")?;
            let params = self.parse_ctor_params()?;
            self.expect(&TokenKind::RParen, "')' to close constructor parameters")?;
            let body = self.parse_block()?;
            Ok(ClassMember::Constructor {                 // <-- `modifiers` NOT threaded in
                params,
                body,
                span: sp,
            })
        }
        ...
```

`modifiers` is parsed at line 514 but the `Constructor` arm constructs `ClassMember::Constructor`
without it — there is no visibility field on the constructor AST node, so `private`/`protected`/
`public` on a constructor are all no-ops.

## Severity

**P0 (unsound).** A modifier that is part of the documented surface (it parses without error and
is suggested by the example/visibility machinery) is silently ignored, letting provably-wrong code
(external construction of a hierarchy-restricted type) through. For a language whose pitch is a
*provably-correct* upgrade of PHP this is worse than a missing feature — `protected constructor`
*looks* enforced and is not. PHP itself enforces `protected __construct` (factory-method pattern),
so this is a regression against the very baseline Phorj claims to improve.

## Concrete fix

1. **AST** (`src/ast/...`, the `ClassMember::Constructor` / constructor info struct): add a
   `visibility: Visibility` field (default `Public`), mirroring how methods/fields already carry
   visibility (`enforce_member_vis` machinery — see member-visibility checker work).
2. **Parser** (`src/parser/items.rs:516-526`): stop dropping `modifiers`; extract the single
   visibility modifier (reusing the existing "at most one declaration visibility" validation noted
   at items.rs:11) and thread it into `ClassMember::Constructor`. Reject non-visibility modifiers
   on a constructor (e.g. `static`/`open`) with a diagnostic, and update the misleading
   doc-comment at lines 510-511.
3. **Checker** (`src/checker/...`, the `new` / constructor-call check, same site that resolves
   `new C(...)`): when the resolved constructor's visibility is `protected`, require the call site
   to be inside `C` or a subclass of `C`; when `private`, require the same class. Reuse the
   existing access-context logic that powers `E-METHOD-VISIBILITY` / `E-FIELD-VISIBILITY`; emit a
   new code, e.g. `E-CTOR-VISIBILITY`, and back it with a `phg explain` entry.
4. **Transpiler:** emit the visibility keyword on the generated PHP `__construct` so the lowered
   PHP enforces the same restriction (byte-identity-preserving for the public default; for the
   multi-parent trait-lowering path note that PHP trait `__construct` visibility must be carried
   through).

This single fix closes both the seed private-constructor bypass and this protected-constructor GAP
(one shared mechanism — constructor visibility).
