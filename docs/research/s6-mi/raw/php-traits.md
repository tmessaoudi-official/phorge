# PHP 8.4 Traits / Interfaces / Abstract Classes — capability map for lowering multiple inheritance

All findings below were **verified by running PHP 8.4.22** (`/stack/tools/phpbrew/php/php-8.4.22/bin/php`,
PHP 8.4.22 cli, Zend 4.4.22). Every snippet + its real output is included. Orientation: *can we faithfully
emulate `class C extends A, B` by lowering each MI-parent to an interface (type side) + a trait (impl side)?*

---

## 1. What a trait CAN carry — VERIFIED

```php
<?php
trait T {
    public int $prop = 1;              // instance property w/ type + default
    public static int $sprop = 10;     // static property
    const C = 100;                     // trait constant (since PHP 8.2)
    public function m(): string { return "m"; }
    public static function sm(): string { return "sm"; }
    abstract public function must(): string;   // abstract method (forces impl)
    public function callMust(): string { return $this->must(); }
}
class A { use T; public function must(): string { return "impl"; } }
$a = new A();
echo $a->prop," ",A::$sprop," ",A::C," ",$a->m()," ",A::sm()," ",$a->callMust(),"\n";
```
Output: `1 10 100 m sm impl`

So a trait carries: **instance properties (typed + default), static properties, constants (8.2+),
instance methods, static methods, abstract methods, a constructor, final methods, private/protected
members, and `use` of other traits.** Confirmed individually:

- **Constructor in a trait** — allowed:
  ```php
  trait Ta { public function __construct() { echo "Ta::ctor\n"; } }
  class Ca { use Ta; }   // new Ca()  =>  "Ta::ctor"
  ```
- **Trait uses trait** — allowed and flattens transitively:
  ```php
  trait Inner { public function inner(): string { return "inner"; } }
  trait Outer { use Inner; public function outer(): string { return "outer+".$this->inner(); } }
  class C { use Outer; }   // (new C())->outer()  =>  "outer+inner"
  ```
- **final + private property in trait** — both honored:
  ```php
  trait T { final public function m(): string {return "m";} private int $secret = 5;
            public function reveal(): int { return $this->secret; } }
  class C { use T; }   // => "m 5"
  ```
- **`parent::` inside a trait method** — resolves against the **using class's** parent, not the trait:
  ```php
  class P { public function greet(): string { return "P"; } }
  trait T { public function greet(): string { return "T+".parent::greet(); } }
  class C extends P { use T; }   // (new C())->greet()  =>  "T+P"
  ```

---

## 2. Precedence — VERIFIED: **current class > trait > inherited parent**

```php
trait T { public function m(): string { return "trait"; } }
class P { public function m(): string { return "parent"; } public function p2(): string { return "parent2"; } }
class CwnOwn          extends P { use T; public function m(): string { return "own"; } } // own  > trait,parent
class CtraitOverParent extends P { use T; }                                              // trait > parent
class CinheritsP       extends P { use T; }
```
Output:
```
own>both: own
trait>parent: trait
parent-only: parent2
```
Exact rule confirmed: a method defined in the **using class itself** wins; otherwise the **trait** method
wins over an **inherited parent** method; a method only in the parent is inherited normally.

---

## 3. Conflict resolution — `insteadof` / `as` — VERIFIED

```php
trait A { public function hi(): string {return "A";} public function bye(): string {return "Abye";} }
trait B { public function hi(): string {return "B";} public function bye(): string {return "Bbye";} }
class C {
    use A, B {
        A::hi insteadof B;             // choose A's hi, discard B's
        B::hi as hiFromB;              // expose B's hi under a NEW name
        B::bye insteadof A;
        A::bye as protected aliasBye;  // alias + CHANGE VISIBILITY
    }
}
```
Output: `A B Bbye` ; `aliasBye visibility protected? yes`

- `insteadof` picks which trait's method **wins** a name collision (the others are dropped from that name).
- `as` **aliases** a (possibly losing) trait method under a new name; **`as` can also change visibility**
  (`as protected`, `as private`, `as public`), with or without renaming.
- **Same trait method can be aliased to MULTIPLE names** — VERIFIED:
  ```php
  trait A { public function m(): string { return "m"; } }
  class C { use A { A::m as alias1; A::m as alias2; } }   // m, alias1, alias2 all callable => "m m m"
  ```

---

## 4. What traits CANNOT do — VERIFIED

- **A trait is NOT a type.** `instanceof` against a trait name is always `false` (no error, just false):
  ```php
  trait T {} class C { use T; }
  var_dump((new C()) instanceof T);   // bool(false)
  ```
  You cannot type-hint a trait, and `instanceof TraitName` never matches. This is *the* reason the
  decomposition needs a parallel **interface** for the type side.

- **Two trait methods of the same name collide FATALLY unless resolved** with `insteadof`/`as`:
  ```php
  trait A { public function hi(){...} } trait B { public function hi(){...} }
  class C { use A, B; }
  // Fatal: Trait method B::hi has not been applied as C::hi, because of collision with A::hi
  ```

- **Two trait constructors collide fatally** (constructor is just a method named `__construct`):
  ```php
  trait Ta { public function __construct(){...} } trait Tb { public function __construct(){...} }
  class C { use Ta, Tb; }
  // Fatal: Trait method Tb::__construct has not been applied as C::__construct,
  //        because of collision with Ta::__construct
  ```

- **Property collisions** — subtler than methods. `insteadof`/`as` do **NOT** apply to properties.
  - **Identical** definition (same name, type, default) in two traits → **silently allowed** (deduped):
    ```php
    trait A { public int $x = 1; } trait B { public int $x = 1; } class C { use A, B; }   // ok, x == 1
    ```
  - **Different default** → **FATAL**:
    `A and B define the same property ($x) in the composition of C. However, the definition differs and is
     considered incompatible.`
  - **Different type** → **FATAL** (same message).
  - **Static property, different value** → **FATAL** (same message).
  There is no `insteadof` for properties: the only fix is to make the definitions identical or rename one
  in the source.

- **Constant collisions** — same rule as properties; `insteadof` does NOT apply to constants:
  ```php
  trait A { const K = 1; } trait B { const K = 2; } class C { use A, B; }
  // Fatal: A and B define the same constant (K) in the composition of C ... definition differs
  ```
  Identical constant values would coexist; differing values are fatal.

- **No `super`-to-a-specific-trait.** A trait method cannot call "the next trait's version" the way Python
  MRO `super()` chains. `parent::` only reaches the using-class's actual parent class (§1). There is no
  trait-linearization / MRO at all — traits are **flattened** (copy-pasted) into the class at compile time.

- **Diamond is only auto-resolved when the shared method is the *identical* flattened method** — VERIFIED,
  and this is a real trap:
  ```php
  // (a) both branches inherit the SAME base method unchanged -> OK, deduped:
  trait Tbase { public function id(): string { return "base"; } }
  trait TA { use Tbase; } trait TB { use Tbase; }
  class C { use TA, TB; }            // (new C())->id() => "base"   (no conflict)

  // (b) ONE branch overrides id() -> the two id() bodies now differ -> FATAL collision,
  //     even though TB only *inherited* Tbase::id:
  trait TA { use Tbase; public function id(): string { return "A"; } }
  trait TB { use Tbase; }
  class C { use TA, TB; }
  // Fatal: Trait method TB::id has not been applied as C::id, because of collision with TA::id

  // (c) both override differently -> FATAL (same message).
  ```
  Takeaway: PHP's "diamond works" only holds when both diamond arms resolve to the **byte-identical**
  method. Any divergence is a hard collision the generator must resolve with `insteadof`.

---

## 5. Interfaces (PHP 8.4) — VERIFIED

```php
interface IA { const VA = 1; public function fa(): string; }
interface IB { const VB = 2; public function fb(): string; }
interface IC extends IA, IB { public function fc(): string; }   // interface multiple-extends: ALLOWED
class C implements IC {
    public function fa(): string { return "a"; }
    public function fb(): string { return "b"; }
    public function fc(): string { return "c"; }
}
$c = new C();
echo IA::VA, IB::VB, " ", ($c instanceof IA?"Y":"N"),($c instanceof IB?"Y":"N"),($c instanceof IC?"Y":"N"),"\n";
echo C::VA, C::VB, "\n";
```
Output: `12 YYY` ; `12`

- A class may `implements` **multiple** interfaces.
- An interface may `extends` **multiple** interfaces (genuine multiple inheritance — on the *type* side only).
- **Interface constants** exist and are inherited onto the implementing class (`C::VA`).
- **Interfaces have NO method bodies in 8.4** (no default/trait-like impl) — VERIFIED fatal:
  ```php
  interface IA { public function fa(): string { return "default"; } }
  // Fatal error: Interface function IA::fa() cannot contain body
  ```
  (PHP has no Java-style `default` interface methods; the *impl* must come from a trait or the class.)

---

## 6. `instanceof` + type compatibility through the decomposition — VERIFIED (THE key run)

```php
interface IA { public function fa(): string; }
interface IB { public function fb(): string; }
trait TA { public int $ax = 10; public static int $asx = 100; public function fa(): string { return "fa:".$this->ax; } }
trait TB { public int $bx = 20; const BK = 7;                 public function fb(): string { return "fb:".$this->bx; } }
class C implements IA, IB {
    use TA, TB;
    public function fc(): string { return $this->fa()." ".$this->fb(); }
}
$c = new C();
echo "methods: ", $c->fc(), "\n";
echo "field ax: ", $c->ax, " bx: ", $c->bx, "\n";
echo "static asx: ", C::$asx, "\n";
echo "const BK: ", C::BK, "\n";
echo "instanceof IA: ", ($c instanceof IA ? "Y":"N"), "\n";
echo "instanceof IB: ", ($c instanceof IB ? "Y":"N"), "\n";
function takesIA(IA $x): string { return $x->fa(); }
function takesIB(IB $x): string { return $x->fb(); }
echo "takesIA(c): ", takesIA($c), "\n";
echo "takesIB(c): ", takesIB($c), "\n";
```
Output:
```
methods: fa:10 fb:20
field ax: 10 bx: 20
static asx: 100
const BK: 7
instanceof IA: Y
instanceof IB: Y
takesIA(c): fa:10
takesIB(c): fb:20
```

**CONFIRMED:** `class C implements IA, IB { use TA, TB; }` yields a `C` that:
- inherits both interfaces' methods (impl supplied by traits),
- carries fields, **static fields**, and **constants** from both traits,
- `$c instanceof IA` AND `$c instanceof IB` are both **true**,
- can be passed to a function typed `IA` AND a function typed `IB`.

This is the complete "is-a both parents + has both parents' state" behavior MI needs, on the runtime side.

Abstract methods inherited from a trait are enforced exactly like class-level abstracts — an unfulfilled
one makes the class abstract:
```php
trait T { abstract public function must(): string; } class C { use T; }
// Fatal: Class C contains 1 abstract method and must therefore be declared abstract
//        or implement the remaining methods (C::must)
```

---

## 6b. Constructor composition — VERIFIED

Two trait `__construct`s collide (§4). Two faithful orchestration patterns:

**Pattern A — init methods (cleanest, recommended for generated code).** Don't name them `__construct`;
let the synthesized class ctor call each parent's init in order:
```php
trait TA { public int $ax = 0; public function initA(int $v): void { $this->ax = $v; } }
trait TB { public int $bx = 0; public function initB(int $v): void { $this->bx = $v; } }
class C { use TA, TB; public function __construct(int $a, int $b) { $this->initA($a); $this->initB($b); } }
$c = new C(5, 9);   // => "5 9"
```

**Pattern B — alias the trait ctors and call both.** If parents genuinely have `__construct`, alias each to
a unique name (and still resolve the name collision with `insteadof`), then call both from the class ctor:
```php
trait TA { public function __construct() { echo "A "; } }
trait TB { public function __construct() { echo "B "; } }
class C {
    use TA, TB {
        TA::__construct as initA;
        TB::__construct as initB;
        TA::__construct insteadof TB;   // still required to resolve the __construct collision
    }
    public function __construct() { $this->initA(); $this->initB(); echo "C\n"; }
}
new C();   // => "A B C"
```
Output: `A B C`. So the generator CAN call both parent constructors deterministically — but it must
synthesize the orchestrating ctor and choose the call order; PHP will never chain them automatically.

---

## 7. Synthesis for Phorge

### The decomposition `class C extends A, B  ⟶  interface IA + trait TA, interface IB + trait TB; class C implements IA, IB { use TA, TB; }`

WORKS, confirmed by run, for **all** of: instance methods, **fields**, **static fields**, **constants**,
abstract-method enforcement, **`instanceof IA`/`instanceof IB`**, and **type-hint acceptance** (`IA $x`
accepts `C`). The interface supplies the *type identity* a trait cannot; the trait supplies the *state +
impl* an interface cannot. Together they reproduce "C is-a A, is-a B, has A's and B's members."

### Faithfully-lowerable MI subset

| MI feature | Lowerable? | How |
|---|---|---|
| Inherit methods from ≥2 parents | ✅ | trait methods |
| `instanceof Parent`, type-hint as Parent | ✅ | parallel interface per parent |
| Fields (instance) from ≥2 parents | ✅ | trait properties (names must not collide incompatibly) |
| Static fields from ≥2 parents | ✅ | trait static properties (same collision caveat) |
| Constants from ≥2 parents | ✅ | trait/interface constants (same collision caveat) |
| Abstract methods (force override) | ✅ | trait abstract methods |
| Override a parent method in C | ✅ | method in C wins (precedence: own > trait > parent) |
| Visibility-changing re-export | ✅ | `as protected/private/public` |
| Constructors from ≥2 parents | ⚠️ | NOT automatic; synthesize a C ctor that calls each parent's init (Pattern A) or aliased ctor (Pattern B) |
| Diamond with **identical** shared member | ✅ | both arms `use` the common base trait → PHP dedupes |

### Breakpoints + required generated-PHP workarounds

1. **Method-name collision across parents** (both define `m`) → fatal. **Workaround:** Phorge must already
   have resolved this at the type-check level (its own MI conflict rule). For PHP, emit
   `use TA, TB { TA::m insteadof TB; }` choosing the winner Phorge's semantics dictate, and optionally
   `TB::m as <mangled>;` if the loser still needs to be reachable.

2. **Constructor collision** → fatal if two parents have `__construct`. **Workaround:** never emit two
   trait `__construct`s as-is. Lower each parent's constructor to a uniquely-named init method
   (`__ctor_A`, `__ctor_B`) and synthesize `C::__construct` that invokes them in Phorge's defined order
   (Pattern A). This also gives Phorge full control of ctor sequencing (PHP gives none).

3. **Incompatible property/constant collision** (same name, differing type/default/value) → fatal, and
   `insteadof` does NOT help (it's method-only). **Workaround options:** (a) Phorge rejects such MI at
   check time (cleanest — matches Phorge's "provably correct" stance); or (b) the generator name-mangles
   one parent's field/const so PHP sees distinct names, and rewrites accesses. Identical defs are safe and
   need no work.

4. **Diamond with diverging override** → fatal even when only one arm overrode the shared method
   (the inherited-but-unchanged arm is treated as a distinct competing body). **Workaround:** emit an
   explicit `insteadof` for the shared method whenever ≥2 arms expose a same-named method, regardless of
   whether one merely inherited it. Don't rely on PHP's auto-dedup except for the byte-identical case.

5. **No trait-`super`/MRO.** PHP flattens; there is no linearized "call the next parent's version" chain.
   **Workaround:** if Phorge MI needs MRO-style chained dispatch, the generator must materialize the order
   itself (aliased trait methods called explicitly in sequence). For a TS/PHP-pragmatic language this is
   likely out of scope — prefer Phorge forbidding ambiguous diamonds over emulating C3 linearization.

6. **Interfaces can't carry impl** (no default bodies in 8.4) — which is exactly why the **trait** half of
   the decomposition is mandatory; the interface is type-only. Not a breakpoint, just the reason the pair
   is needed.

### Bottom line

**Faithfully lowerable:** methods, fields, static fields, constants, abstract-method enforcement,
`instanceof`/type-compatibility against every parent, member override precedence, and visibility
re-exposure — i.e. the *state + behavior + type-identity* of multiple inheritance.

**NOT free / needs generator work or a Phorge-side restriction:** constructor composition (must be
synthesized, never auto), any incompatible same-name field/const/method collision (must be resolved by
Phorge's checker or by name-mangling before emission), and MRO/`super`-chaining (no PHP equivalent;
recommend Phorge disallow ambiguous diamonds rather than emulate linearization). PHP's diamond auto-dedup
is reliable ONLY for byte-identical shared members.
