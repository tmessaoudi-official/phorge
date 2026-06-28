# Probe — private/protected constructor blocks external `new`

**Rule under test:** a `private`/`protected` modifier on a `constructor` must restrict
external instantiation. External `new C(...)` on a class with a `private constructor`
should be **rejected** by the checker (`E-METHOD-VISIBILITY`-class diagnostic), mirroring
PHP semantics and Phorj's existing field/method visibility enforcement.

**Verdict: GAP (P0 — unsound).** The modifier is parsed and silently dropped; external
`new` on a `private`/`protected` constructor checks and runs cleanly on **all three
backends**. Provably-wrong code (an external caller bypassing an intended-private ctor) is
accepted as valid.

---

## Evidence

### Probe 1 — `private constructor`

`$TMP/private-ctor.phg`:
```phorj
package Main;
import Core.Console;

class Secret {
    private constructor(public int x) {}
}

function main(): int {
    Secret s = new Secret(42);
    Console.println("{s.x}");
    return 0;
}
```

```
$ /stack/projects/phorj/target/release/phg check $TMP/private-ctor.phg
OK (type-checks clean)
exit=0

$ /stack/projects/phorj/target/release/phg run $TMP/private-ctor.phg
42
exit=0

$ /stack/projects/phorj/target/release/phg runvm $TMP/private-ctor.phg
42
exit=0
```

The external `new Secret(42)` should have been rejected (`private` ctor). Instead it
checks clean and prints `42` on both the interpreter and the VM.

### Probe 2 — `protected constructor`

`$TMP/protected-ctor.phg` (same shape, `protected` instead of `private`):
```
$ /stack/projects/phorj/target/release/phg check $TMP/protected-ctor.phg
OK (type-checks clean)
exit=0

$ /stack/projects/phorj/target/release/phg run $TMP/protected-ctor.phg
7
exit=0
```

`protected` is dropped identically. No diagnostic, no runtime error.

---

## Root cause (verified)

The modifier is discarded at **parse time** — it never reaches the checker:

`src/parser/items.rs:510-527`:
```rust
/// One class member: a field, a constructor, or a method. Modifiers preceding
/// `constructor` are consumed and dropped (M1: constructors are implicitly public).
pub(super) fn parse_class_member(&mut self) -> Result<ClassMember, Diagnostic> {
    let sp = self.peek_span();
    let modifiers = self.parse_modifiers();   // <-- parsed
    match self.peek() {
        TokenKind::Constructor => {
            self.advance();
            ...
            Ok(ClassMember::Constructor { params, body, span: sp })  // <-- `modifiers` dropped
        }
        TokenKind::Function => Ok(ClassMember::Method(self.parse_function(modifiers, sp)?)),
        ...
```

`modifiers` is captured then never used on the `Constructor` arm (only the `Method` arm
threads it on). The AST node itself has **no visibility field** to carry it:

`src/ast/mod.rs:637-641`:
```rust
Constructor {
    params: Vec<CtorParam>,
    body: Vec<Stmt>,
    span: Span,
},
```

So the gap is structural, not a missed check — the information is destroyed before any
backend sees it. This is distinct from field/method visibility, which IS enforced: the
checker routes every external field read/write, method call, clone-with, let-destructure,
and match-struct-pattern through `enforce_member_vis` (`src/checker/calls.rs:1073`,
emitting `E-FIELD-VISIBILITY`/`E-METHOD-VISIBILITY`). Constructors have no analogous path
because their visibility never survives parsing.

---

## Concrete fix

Three coordinated edits, front-end only (no new `Op`, no `Value` change — byte-identity
spine untouched since the construction site is the same; only the checker gains a
rejection path):

1. **`src/ast/mod.rs`** (`ClassMember::Constructor`): add a `visibility: Visibility`
   field (reuse the same `Visibility` enum that fields/methods use).

2. **`src/parser/items.rs:516`** (the `Constructor` arm of `parse_class_member`): stop
   dropping `modifiers` — derive the constructor visibility from them (default `public`),
   and reject any non-visibility modifier on a ctor with a clear diagnostic. Populate the
   new AST field.

3. **`src/checker/calls.rs`** (the `new C(...)` / instantiation check path): when the
   resolved constructor is `private`/`protected` and the call site is outside the class
   (resp. outside the class hierarchy), emit a new `E-CTOR-VISIBILITY` diagnostic — reuse
   the `enforce_member_vis` scope logic so the "inside the class" exemption matches
   field/method behavior. Add a `phg explain E-CTOR-VISIBILITY` entry
   (`src/cli/explain.rs`, next to the existing `E-*-VISIBILITY` entries) and a guide
   example + checker test mirroring `src/checker/tests/visibility.rs`.

Until fixed, the comment at `src/parser/items.rs:510` ("constructors are implicitly
public") accurately describes the behavior but the `private`/`protected` keywords are a
false promise: they parse without error and do nothing.
