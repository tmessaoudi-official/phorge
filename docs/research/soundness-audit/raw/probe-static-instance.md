# Soundness probe — static vs instance access not confused

**Stage 2.** Probe whether Phorj confuses static and instance access: a static method called on an
instance, and an instance method called on the class name — both should be rejected.

**Verdict: GAP (multiple, one P0).** The `static` modifier on a **method** is parsed and retained on
the AST but its semantics are NOT enforced. Static and instance methods share one `info.methods` table
and `FnSig` carries no `static` flag, so the checker cannot distinguish them at any call site. The most
severe consequence: a `static` method body can freely reference `this` and read instance fields, with
no diagnostic, on both backends. (The static *field* read direction IS correctly enforced — the gap is
methods-only.)

BIN = `/stack/projects/phorj/target/release/phg` (prebuilt, not rebuilt).

---

## Finding 1 (P0, UNSOUND) — a `static` method may reference `this` / read instance state

A `static` method has no receiver (`E-OPEN-STATIC` confirms statics are not bound to an instance), so
`this` must be out of scope in its body. It is not.

Program `static-uses-this.phg`:
```phorj
package Main;
import Core.Console;

class Box {
    constructor(public int v) {}
    // BAD: a STATIC method has no receiver, so `this` must not be accessible. Should be rejected.
    static function leak(): int { return this.v; }
}

function main(): void {
    Box b = new Box(7);
    Console.println("{b.leak()}");
}
```

Command + output:
```
$ phg check static-uses-this.phg
OK (type-checks clean)
exit=0

$ phg run static-uses-this.phg
7
exit=0
```

It checks clean and runs `7`. A `static function` reading `this.v` is accepted — `this` is in scope in
a context where it semantically must not be. This is the unsound core of the gap: it lets provably-wrong
code (a static method depending on per-instance state) through.

The PHP transpile masks the divergence only by *also* dropping the `static` modifier (Finding 3):
```
$ phg transpile static-uses-this.phg
<?php
final class Box {
    function __construct(public int $v) {}
    function leak(): int {            <-- `static` dropped; emitted as a plain instance method
        return $this->v;
    }
}
...
```
Had the transpiler honestly emitted `static function leak()`, PHP would fatal at runtime
(`Using $this when not in object context`), breaking the byte-identity spine. So the language is unsound
*and* the transpiler hides it by silently demoting the modifier.

---

## Finding 2 (P1) — a `static` method is callable through an instance with no diagnostic

PHP issues a deprecation for `$instance::staticMethod()` and conceptually a static method is a
class-level operation; Phorj accepts the instance call form silently and identically on both backends.

Program `static-on-instance.phg`:
```phorj
package Main;
import Core.Console;

class Math2 {
    constructor() {}
    static function square(int n): int { return n * n; }
}

function main(): void {
    Math2 m = new Math2();
    // BAD: calling a STATIC method through an INSTANCE. Should be rejected.
    Console.println("{m.square(5)}");
}
```

Command + output:
```
$ phg check static-on-instance.phg
OK (type-checks clean)
exit=0

$ phg run static-on-instance.phg
25
exit=0

$ phg runvm static-on-instance.phg
25
exit=0
```

Accepted, runs `25` on both backends. The checker has no way to flag it: `FnSig` does not record
whether a method is static, and `check_method_call` resolves any `info.methods.get(name)` regardless.
(This direction is the milder of the two — no `this` is leaked because `square` does not use it — but it
is still a declared rule silently ignored: static membership is unenforced.)

---

## Finding 3 (P1) — the `static` modifier on methods is dropped by the transpiler (declared-but-ignored)

Root demonstration (same transpile output as Finding 1, and Finding 2's transpile):
```
$ phg transpile static-on-instance.phg
<?php
final class Math2 {
    function __construct() {}
    function square(int $n): int {   <-- emitted as a plain method, no `static`
        return $n * $n;
    }
}
...
```
The transpiler never emits `static function` for a static method — `grep -n "static function" src/transpile/*.rs`
returns nothing. The modifier is parsed (`src/parser/items.rs:632` maps `TokenKind::Static → Modifier::Static`,
fed into `parse_function(modifiers, ...)`), retained on `FunctionDecl.modifiers`, used only for the
`E-OPEN-STATIC` collect check, and then ignored everywhere else. This is precisely the
"parsed and silently ignored" defect class this audit hunts (the same shape as the seed
private-constructor bug).

---

## Finding 4 (P2) — instance method on the class name: rejected, but for the WRONG reason

The complementary direction (the probe's second half): `ClassName.instanceMethod()`.

Program `instance-on-class.phg`:
```phorj
package Main;
import Core.Console;

class Counter {
    constructor(public int n) {}
    function get(): int { return this.n; }
}

function main(): void {
    // BAD: calling an INSTANCE method on the CLASS NAME (no instance, no `this`). Should be rejected.
    Console.println("{Counter.get()}");
}
```

Command + output:
```
$ phg check instance-on-class.phg
type error at 1:1: unknown identifier `Counter`
package Main;
^
  [E-UNKNOWN-IDENT]
exit=1

$ phg run instance-on-class.phg
type error at 1:1: unknown identifier `Counter`
package Main;
^
  [E-UNKNOWN-IDENT]
exit=1
```

It IS rejected, so this direction is *not* unsound. But the rejection is accidental:
`check_method_call` calls `check_expr(object)` on the bare class name, which fails as an unknown
identifier — the checker never recognizes "instance method invoked without a receiver." The diagnostic
is misleading (`unknown identifier Counter`, code `E-UNKNOWN-IDENT`) and the span is wrong (`1:1`, the
`package` line, not the call). Note the same wrong-reason rejection means a *legitimate* static-method
call via the class name (`Math2.square(5)`) is ALSO rejected with `E-UNKNOWN-IDENT` — i.e. static
methods have no working call path at all via the class name (only the bogus instance form of Finding 2
"works"). Cosmetic/diagnostic severity for the instance-on-class direction; the static-no-call-path
issue is incidental confirmation that static methods are an incomplete feature.

CONTROL — the static *field* read-via-instance direction is correctly enforced (proves the gap is
methods-only):
```
$ phg check static-field-via-instance.phg     # class Reg { static int answer = 42; } ; r.answer
type error at 1:2: type `Reg` has no field `answer`
exit=1
```
`check_member` keeps `statics` disjoint from `fields`, so an instance-field lookup correctly misses a
static field. The method table has no such separation — that is the asymmetry.

---

## Root cause (file + line)

1. **`FnSig` (src/checker/mod.rs:46)** has no `is_static` field, so the call-site checks
   (`check_method_call`, src/checker/calls.rs:675; `check_member`, :884) cannot distinguish a static
   from an instance method. Static and instance methods both live in
   `ClassInfo.methods: HashMap<String, Vec<FnSig>>`.

2. **`check_type_body` (src/checker/program.rs:264)** checks every method with
   `ClassMember::Method(f) => self.check_function(f)` while `self.cur_class` is set to the class —
   including `static` methods. This is why `this` is in scope inside a static method body (Finding 1).
   The fix mirror is in the SAME file: the static-field-initializer path at program.rs:184 explicitly
   does `let prev = self.cur_class.take();  // statics have no instance — this is out of scope here`.
   The static-method path simply omits that.

3. **Transpiler (src/transpile/, no `static function` emission)** drops the modifier (Finding 3),
   which both hides the unsoundness and makes the modifier non-functional.

---

## Recommended fix (concrete)

- **Track static-ness on the signature.** Add `is_static: bool` to `FnSig` (src/checker/mod.rs:46),
  set it during collection (src/checker/collect.rs — where `methods` is populated; the
  `Modifier::Static` check at collect.rs:358 already reads the flag for `E-OPEN-STATIC`).
- **Clear `this` for static-method bodies.** In `check_type_body` (src/checker/program.rs:~264),
  branch on `f.modifiers.contains(&Modifier::Static)`: when static, `self.cur_class.take()` for the
  duration of `check_function(f)` (mirroring the static-init path at program.rs:184), restoring it
  after. This makes Finding 1 a clean error (`this` becomes unknown inside a static method — ideally a
  dedicated `E-STATIC-THIS`).
- **Enforce call-site separation.**
  - In `check_method_call` (calls.rs:675), after resolving the sig, reject calling a `is_static` method
    through an instance receiver — new code e.g. `E-STATIC-VIA-INSTANCE` with a hint to use
    `ClassName.method(...)` (closes Finding 2).
  - Add a static-method *call* path on a class-name head in `check_member`/the call resolver (parallel
    to the existing static-field path at calls.rs:884), so `ClassName.staticMethod(args)` resolves
    instead of failing as `E-UNKNOWN-IDENT` — and reject an *instance* method on a class-name head with
    a real diagnostic (`E-INSTANCE-VIA-CLASS`) instead of the misleading `E-UNKNOWN-IDENT`
    (closes Finding 4 and gives static methods a working call path).
- **Emit `static function` in the transpiler** for `is_static` methods (src/transpile/), so the PHP
  output is faithful — once Finding 1 is fixed this cannot break byte-identity, because a sound static
  method never touches `$this`.

Each change is front-end / transpiler only; no new `Op` and no `Value` change. Ship with a guide example
plus differential cases (a legitimate static-method call, and the three rejection cases) per the
"examples ship with features" rule.
