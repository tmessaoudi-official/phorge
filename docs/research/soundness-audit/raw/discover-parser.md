# Soundness Audit — STAGE 1 DISCOVER (parser)

Scope: `src/parser/` — every place a modifier/keyword/annotation is PARSED then DROPPED, consumed-and-ignored,
or accepted with no enforcement. Empirical verdicts use the prebuilt binary
`BIN=/stack/projects/phorj/target/release/phg`; probes live under the scratchpad `audit/` dir (never committed).

Severity: P0 (unsound — provably-wrong code through) · P1 (declared rule silently ignored) · P2 (missing
diagnostic, not unsound) · P3 (cosmetic).

---

## C1 [P0] — ALL modifiers on a `constructor` are consumed and dropped (the seed, generalized)

`src/parser/items.rs:512-527` (`parse_class_member`): line 514 calls `parse_modifiers()`, then the
`TokenKind::Constructor` branch builds `ClassMember::Constructor { params, body, span }` **without the
`modifiers` binding** — so every modifier preceding `constructor` is silently discarded. The comment at
line 511 admits only the visibility case ("M1: constructors are implicitly public"), but the drop is total:
`static`, `abstract`, `const`, `open`, `mutable`, `public`, `private`, `protected` all vanish.

The two genuinely UNSOUND cases:

### C1a — `private`/`protected constructor` bypass (the literal seed)
```
$ cat p1_private_ctor.phg
package Main;
import Core.Console;
class Secret {
    private constructor(public int v) {}
}
function main(): int {
    var s = new Secret(42);
    Console.println("{s.v}");
    return 0;
}
$ phg check p1_private_ctor.phg
OK (type-checks clean)
exit=0
$ phg run p1_private_ctor.phg
42
exit=0
```
`protected constructor` identical (`p2_protected_ctor.phg` → `7`, exit 0). External `new` on a class with a
non-public constructor checks AND runs — the access-control rule is parsed and ignored. This is the P0 the
audit was launched to find.

Note (blast radius, verified): the transpiler emits a bare `function __construct(...)` with NO visibility
keyword (`$ phg transpile p1_private_ctor.phg` → `function __construct(public int $v) {}`), so the dropped
`private` does NOT reach PHP — there is no run↔real-PHP fatal-divergence here. The defect is purely the
soundness/access-control hole, not a byte-identity break. (PHP's own `__construct` defaults to public, so the
emitted PHP also permits the external `new` — the two backends agree on the *wrong* answer.)

### C1b — `abstract constructor` accepted (nonsense, unsound shape)
```
$ cat p3_abstract.phg   # class C { abstract constructor(public int v) {} }
$ phg check p3_abstract.phg
OK (type-checks clean)
exit=0
```
An `abstract constructor` (with a body!) is meaningless and should be rejected; instead it checks clean and
the class is freely instantiable.

### C1c — `static`/`const`/`open`/`mutable`/`public constructor` all accepted, no effect
```
$ for m in static mutable const open public; do phg check p3_$m.phg; done
OK (type-checks clean)   # static constructor
OK (type-checks clean)   # mutable constructor
OK (type-checks clean)   # const constructor
OK (type-checks clean)   # open constructor
OK (type-checks clean)   # public constructor
```
These are P1/P2 (declared-but-ignored garbage) rather than P0, but they share the same root cause: the
constructor branch never inspects `modifiers`.

**Fix shape:** store `modifiers` on `ClassMember::Constructor` (or reject any modifier other than the three
visibility keywords at the parser, then enforce visibility on `new` in the checker — `MemberVis::of` already
exists and `calls.rs` already enforces member visibility for fields/methods; constructors must route through
the same gate).

---

## C2 [P2] — contradictory / duplicate member modifiers accepted with no diagnostic

`src/parser/items.rs:619-641` (`parse_modifiers`) loops greedily and pushes every modifier token with no
dedupe and no conflict check. The checker's `MemberVis::of` (`src/checker/mod.rs:153-161`) resolves by
`contains(Private)` first, then `Protected`, else `Public` — so a conflict resolves silently (Private wins),
never errors.

```
$ cat p7_dup_mod.phg
package Main;
class C {
    public private function m(): int { return 1; }
}
function main(): int { return 0; }
$ phg check p7_dup_mod.phg
OK (type-checks clean)
exit=0
```
`public private function` checks clean and the method is treated as **private** (contains-Private wins),
contradicting the written `public`. Same for `static static int x = 1;` (`p7b_dup.phg` → clean). Not unsound
(deterministic resolution), but a declared `public` being silently overridden by a co-located `private` is a
"declared but not enforced" footgun exactly in the spirit of this audit. P2.

---

## C3 [P2] — meaningless modifiers on methods accepted, no effect, no diagnostic

`const function` and `mutable function` are nonsensical (`const`/`mutable` describe storage, a method has
none) yet `parse_modifiers` accepts them and the checker never rejects:
```
$ cat p8_const_method.phg   # class C { const function m(): int { return 1; } }
$ phg check p8_const_method.phg
OK (type-checks clean)
exit=0
$ cat p9_mut_method.phg      # class C { mutable function m(): int { return 1; } }
$ phg check p9_mut_method.phg
OK (type-checks clean)
exit=0
```
Compare the property-hook path (`items.rs:537-540`) which DOES reject stray modifiers
(`"a property hook to carry no modifiers"`) — so the language clearly intends modifier validity to be
checked, but methods/fields have no equivalent guard. P2.

---

## NON-FINDINGS (enforced for the right reason — documented to avoid re-flagging)

- **Interface method modifiers** — REJECTED at parse (`items.rs:455-458` expects `Function` directly):
  `static function foo();` / `private function foo();` inside an interface → `parse error … expected
  'function' … found Static`/`Private` (exit 1). Enforced (the hardcoded `modifiers: Vec::new()` at line 479
  is reachable only when no modifier was present). Correct.
- **Property-hook modifiers** — REJECTED (`items.rs:537-540`). Correct.
- **Top-level declaration visibility** (`private`/`internal function|class|enum|interface`) — STAMPED by
  `stamp_visibility` (`parser/mod.rs:15-30`) and ENFORCED at the cross-package boundary in
  `src/loader/mod.rs:64-85` (`E-VIS-PRIVATE`/`E-VIS-INTERNAL`). Within a single `package Main` file nothing
  imports cross-package so `private function helper()` has no *observable* effect there
  (`p5_freefn_vis.phg` runs → `5`), but that is by-design: the rule lives where it bites (the importer).
  NOT a parser-drop gap.
- **Double-optional `T??`** — `??` lexes as a single `QuestionQuestion` token, so `int?? x` is a parse error
  (`expected a parameter name, found QuestionQuestion`), not a silent collapse. The `while eat(Question)`
  loops in `types.rs:69,120` only fire on genuinely repeated single `?` tokens which the lexer never
  produces adjacently. NOT a gap.
- **`-> T` return alias / `=>` function-type `->` alias** (`items.rs:172`, `types.rs:96`) — intentional
  transition aliases, both produce the same AST. NOT a drop.

---

## Summary table

| ID  | Sev | Rule (parsed, then…) | Locus | Status |
|-----|-----|----------------------|-------|--------|
| C1a | P0  | `private`/`protected constructor` dropped → external `new` allowed | items.rs:516-527 | GAP (seed) |
| C1b | P1  | `abstract constructor` accepted (with body) | items.rs:516-527 | GAP |
| C1c | P2  | `static`/`const`/`open`/`mutable`/`public constructor` ignored | items.rs:516-527 | GAP |
| C2  | P2  | contradictory/duplicate member modifiers (`public private`) | items.rs:619-641 | GAP |
| C3  | P2  | meaningless `const`/`mutable function` accepted | items.rs:619-641 | GAP |

Root cause for C1*: the `Constructor` branch never threads `modifiers`. Root cause for C2/C3:
`parse_modifiers` has no validity/dedupe/conflict gate and no per-member-kind allow-list, and the checker
has no compensating diagnostic (unlike the property-hook path).
