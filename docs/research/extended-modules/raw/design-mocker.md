# Design — Auto-Mocker (Reflect-based, for `phg test`)

> **Stage 2 — DESIGN.** A test-double generator: given an interface, synthesize a mock that
> implements it, records every call (args + count), returns canned/configured values, and verifies
> expectations (`timesCalled` / `calledWith`). Sibling to the `phg test` runner and the seeded Faker.
>
> **Verdict: Tier A — front-end codegen (AST synthesis), zero backend changes, no new `Op`/`Value`.**
> Byte-identical by construction because a mock is *ordinary Phorge code* the front end injects before
> any backend runs — the same discipline as the `Json`/`RoundingMode` injected-type preludes
> (`cli::inject_json_prelude`). Confidence: **medium** (the mechanism is proven; the open design
> tension is the configuration ergonomics under an immutable heap — see §8).

---

## 1. The reframing that makes this Tier A

The naive reading is "synthesize a mock *at runtime* using `Core.Reflect`." That is **rejected** and
also impossible here: `docs/specs/2026-06-25-core-reflect-design.md` locks reflection as **read-only,
name-level — no invoke-by-name, no instantiate-by-name, no field mutation by name** (principle 2).
There is no `Reflect.construct(name)` and there must never be (it would be a runtime construct PHP's
erased side could not reproduce identically, a spine break).

So the mock is not built by reflecting at runtime. It is built by **codegen at compile time**:

> The mocker reads the interface's `InterfaceDecl` (which already carries full method **signatures** —
> `Vec<FunctionDecl>` with empty bodies, see `src/ast/mod.rs:769`), synthesizes a real `ClassDecl` that
> `implements` it, and injects that class into the program's AST **before the checker runs** — exactly
> like `inject_json_prelude`.

From that point the synthesized class is *indistinguishable from hand-written Phorge*. It type-checks
(the checker proves it implements the interface — `E-IFACE-UNIMPL`/`E-IFACE-SIG` are free coverage),
runs on the interpreter, compiles to bytecode for the VM, and transpiles to a plain PHP `class`. All
three legs execute the **same generated source**, so byte-identity is guaranteed the same way it is for
any user class. **This is the single most important property of the design.**

Where does `Core.Reflect` come in, then? Not at runtime. The **`ClassTables`** built once from the
program (`native/mod.rs:102`) already hold each interface's sorted method/field names; the mocker
reuses the *same* sorted-name discipline so its codegen is deterministic. But the load-bearing source
is the `InterfaceDecl` itself — `ClassTables.methods` gives names only; the mocker needs the full
**parameter list + return type** of each method, which lives in `InterfaceDecl.methods[i].params/ret`.
So the mocker is "Reflect-*adjacent*": it consumes the same compile-time class metadata, by reading the
AST directly rather than going through the lossy name-level native surface.

---

## 2. What the mock must provide

For an interface

```phorge
interface Clock {
    function now() -> int;
    function tick(int by) -> int;
}
```

a mock must support, in test code:

1. **Construction** — `var c = Mock.of<Clock>();` (an instance usable wherever a `Clock` is expected).
2. **Canned returns** — `c.returns("now", 42);` or per-call stubbing.
3. **Call recording** — every invocation records the method name + arg values.
4. **Verification** — `c.timesCalled("now")` → `int`; `c.calledWith("tick", [7])` → `bool`;
   `c.neverCalled("now")` → `bool`.
5. **Determinism** — recordings, default returns, and verification are pure functions of the call
   sequence; no clock, no random, no map-iteration-order leak (the recording log is an *ordered* list,
   not a hash map).

The synthesized class therefore needs **mutable state** (the call log grows as methods are invoked).
This is the one real design constraint and is addressed in §5/§8.

---

## 3. The synthesized class (codegen target)

For the `Clock` interface above, the mocker generates this Phorge source and injects it as a
`ClassDecl`:

```phorge
class Clock$Mock implements Clock {
    // The ordered call log: each entry "method|arg0,arg1,..." (a deterministic string key).
    mutable List<string> __calls = [];
    // Canned returns, keyed by method name. A Map<string, V> is awkward across heterogeneous
    // return types, so v1 stores a per-method scalar via a small injected helper enum (see §5/§8).
    mutable Map<string, int> __intReturns = [:];   // shape per return-type kind

    // --- one synthesized impl per interface method ---
    function now() -> int {
        this.__record("now", []);
        return this.__intReturns["now"] ?? 0;   // configured value or the type's zero
    }
    function tick(int by) -> int {
        this.__record("tick", [Convert.toString(by)]);
        return this.__intReturns["tick"] ?? 0;
    }

    // --- recording + verification surface (fixed, not per-method) ---
    function __record(string m, List<string> args) -> void {
        this.__calls = List.append(this.__calls, m + "(" + Text.join(args, ",") + ")");
    }
    function returns(string m, int v) -> void { this.__intReturns = Map.put(this.__intReturns, m, v); }
    function timesCalled(string m) -> int { /* count entries whose method == m */ }
    function calledWith(string m, List<string> args) -> bool { /* membership test on __calls */ }
    function neverCalled(string m) -> bool { return this.timesCalled(m) == 0; }
}
```

Key codegen rules:

- **Class name** `<Interface>$Mock`. The `$` is reserved/mangled in the transpiler exactly like
  property-hook synthetic methods (`<Class>::<name>$get`), so it cannot collide with a user class and
  emits a valid PHP identifier (`Clock_Mock` after the established `php_variant_name`-style mangle).
- **One method body per interface method**: `record(name, [stringified args])` then `return <canned or
  zero>`. The args are stringified through `Core.Convert.toString` / `Text` so the call log is a
  `List<string>` (a single homogeneous, ordered, comparable type — the determinism workhorse).
- **The return value** is the configured canned value if present, else the **type's zero/default**
  (`0` / `0.0` / `false` / `""` / `null` for `T?`). The default-by-type table is the same one the
  checker already owns for `var`/default-params; for a non-optional **object** return type with no
  configured value, the mock cannot synthesize an instance → that is a **clean checker error**
  (`E-MOCK-NO-DEFAULT`, "method returns a non-optional object; configure it with `.returns(...)`"),
  not a runtime null (which would violate the non-null guarantee, M3 S2).
- **The recording/verification methods are fixed** (`__record`/`returns`/`timesCalled`/`calledWith`/
  `neverCalled`) — added once regardless of the interface, so the surface is stable and small.

Because this is just a class, the transpiler emits a real PHP class:

```php
final class Clock_Mock implements Clock {
    public array $__calls = [];
    public array $__intReturns = [];
    public function now(): int { $this->__record("now", []); return $this->__intReturns["now"] ?? 0; }
    public function tick(int $by): int { $this->__record("tick", [(string)$by]); return $this->__intReturns["tick"] ?? 0; }
    public function __record(string $m, array $args): void { $this->__calls[] = $m."(".implode(",", $args).")"; }
    // ...
}
```

— and the byte-identity argument is *the same one that already holds for every user class*: there is
no new mechanism on the PHP side, only generated-instead-of-handwritten source.

---

## 4. The public API surface (`Core.Test.Mock`)

```phorge
import Core.Test;            // the test module (assertions + runner live here too)

// Construction: a generic free function, T inferred-from-annotation (NOT a runtime type arg).
Clock c = Mock.of<Clock>();  // synthesizes/instantiates Clock$Mock

c.returns("now", 42);        // canned value
var t = c.now();             // -> 42, and records the call
var n = c.timesCalled("now");// -> 1
var ok = c.calledWith("now", []); // -> true
```

**`Mock.of<T>()` is the only entry point.** It is **not** a runtime native — it is a parser/checker
**intrinsic** recognized by type name: `Mock.of<Clock>()` is rewritten at the front end to
`new Clock$Mock()` (the synthesized class is injected on demand for each interface that appears in a
`Mock.of<...>` position). This mirrors `Json`/`RoundingMode` lazy injection: walk the program, collect
every `Mock.of<I>()` target interface, inject one `I$Mock` `ClassDecl` per distinct `I`, then rewrite
the calls to plain constructions. By `check_and_expand` time the program is mock-intrinsic-free.

> **Why an intrinsic, not a native:** a native returns a `Value` computed from arg *values*; `Mock.of`
> needs the *type* `T` and must emit a *class* — neither is expressible through the `(module,name)`
> native-registry `eval`/`php` contract. The generic-type-argument-driven, type-directed-rewrite shape
> is exactly `typeName`'s checker-pass shape (span-keyed substitution), reused.

---

## 5. The mutable-state problem (and its resolution)

The mock records calls — it **mutates**. Phorge's heap is `Rc`-shared with `Instance` being
**shared-mutable** (the Mutation milestone, [[mutation-milestone]]: `List/Map/Set` are COW,
`Instance` fields are mutable in place). So a mock instance *can* accumulate state:
`this.__calls = List.append(this.__calls, key)` reassigns a `mutable` field — the COW list is rebuilt
and the field rebound, and because `Instance` is shared (one `Rc`), all aliases of the mock see the
update. **This already works** — it is exactly how a stateful counter object works today, byte-identical
on `run`/`runvm`/PHP. **No new mechanism.**

Determinism of the log is preserved because:
- the log is an **ordered `List<string>`**, appended in call order — never a hash map iterated for
  output (Invariant 8);
- each entry is a deterministic `method(arg,arg)` string built via `Core.Convert.toString` /
  `Text.join` (already byte-identical natives);
- `timesCalled`/`calledWith` are pure folds over that ordered list.

The genuinely awkward part is **heterogeneous canned returns** (`returns("now", 42)` vs
`returns("name", "x")`). Three options, in preference order:

1. **(v1, recommended) one typed-return slot per primitive kind** — `__intReturns`/`__strReturns`/… ,
   selected by the method's declared return type at codegen. Ugly internally but invisible to the user
   and trivially byte-identical (plain maps). Object/optional returns require `.returns` (no zero
   default).
2. **A single `Map<string, Json>`** once `Core.Json` is the universal dynamic carrier — cleaner, defers
   to the Json-as-Any direction. Adds a Json dependency to every mock.
3. **Per-call programmable stubs** (`returnsWith("tick", fn(int by) => by * 2)`) using S3 lambdas/
   first-class fns — strictly additive on top of (1).

Recommend shipping (1) now; (3) is a clean follow-up (the closure machinery exists, M3 S3).

---

## 6. Byte-identity argument (the spine)

| Concern | Why it's byte-identical |
|---|---|
| The mock class | Generated *Phorge source* injected pre-checker; runs as an ordinary class on all 3 legs. |
| Call log | Ordered `List<string>`, appended in call order; rendered via existing byte-identical natives. |
| Canned returns | Plain `Map` lookups with `??` defaults — existing byte-identical ops. |
| Verification | Pure folds over the ordered log (`timesCalled`/`calledWith`). |
| `Mock.of<T>()` | Front-end rewrite to `new T$Mock()` before any backend — like `Json` injection. |
| Mutation | `Instance` shared-mutable field reassign — already spine-safe (Mutation milestone). |

**No native is impure.** The mocker introduces **zero** Tier-B surface. It is fully gated and belongs
in `tests/differential.rs` coverage via a guide example (`examples/guide/mocking.phg` — or, since it is
a test-tooling feature, an `examples/test/` walkthrough + companion `.phg`, per the CLAUDE.md
examples-ship-with-features rule for non-single-program features).

---

## 7. New `Op` / `Value` needed?

**None.** No new VM `Op` (everything desugars to class construction, method calls, list/map ops — all
existing). No new `Value` (the mock is a `Value::Instance`). The only additions are:
- a front-end **intrinsic recognizer + injector** for `Mock.of<T>()` (a checker/loader pass, sibling to
  `inject_json_prelude`);
- a **codegen function** `synthesize_mock(iface: &InterfaceDecl) -> ClassDecl`;
- new diagnostic codes `E-MOCK-NOT-INTERFACE`, `E-MOCK-NO-DEFAULT`, `E-MOCK-UNKNOWN-METHOD` (+ `phg
  explain`).

This keeps it inside the cheapest possible change class — front-end only, the same as the totality
cluster and generics.

---

## 8. Feasible vs deferred

**Feasible now (v1):**
- Mocking an **interface** (the locked, recommended scope): full method synthesis, canned primitive
  returns, ordered call recording, `timesCalled`/`calledWith`/`neverCalled`.
- Default-by-type returns for primitives + optionals; object returns require explicit `.returns`.
- Deterministic, byte-identical, fully gated.

**Deferred:**
- **Mocking a concrete (`open`/`abstract`) class** — synthesize a *subclass* overriding methods. Harder:
  must call (or stub) the parent constructor, decide which methods to override vs inherit, and respect
  `final`. Phorge is final-by-default, so most classes can't be subclassed at all — interface-only is
  the natural v1 boundary and matches how PHP mock frameworks behave best. Recommend deferring class
  mocking until there's demand; gate it on `open`/`abstract` (a `final` class → `E-MOCK-FINAL`).
- **Programmable stubs** (`returnsWith(name, closure)`) — additive, S3 lambdas exist; ship after v1.
- **Argument matchers** (`any()`, `eq(x)`, `that(predicate)`) for `calledWith` — needs a matcher value
  type or closure predicates; defer.
- **Strict mocks** (fail on an unstubbed call) vs **loose** (return zero) — v1 is loose; strict is a
  one-flag follow-up (`Mock.strict<T>()`).
- **Spies / partial mocks** (wrap a real instance, record but delegate) — needs class mocking first.
- **Auto-verify on test end** (`expect(...).toBeCalledOnce()` integrated with the runner) — couples to
  the `phg test` runner's lifecycle; design jointly with it.

---

## 9. Determinism risks (named)

1. **Heterogeneous-return map iteration** — if canned returns were rendered by iterating a map, order
   could leak. *Mitigation:* never render the returns map; only look up by key. (Invariant 8.)
2. **Arg stringification divergence** — the recorded `method(args)` key must stringify each arg through
   a *byte-identical* native (`Convert.toString`/`Text.join`), never a Rust `Debug`/PHP `var_export`
   that could differ. *Mitigation:* the codegen emits the stringification as Phorge calls, so all three
   legs use the same path.
3. **`$` mangling collision** — the synthetic class/field names (`Clock$Mock`, `__calls`) must mangle to
   valid, collision-free PHP identifiers. *Mitigation:* reuse the proven property-hook/`php_variant_name`
   mangle; reserve the `$Mock`/`__` prefixes in the checker (a user class named `Clock$Mock` is already
   unlexable — `$` isn't an identifier char — so collision is structurally impossible).
4. **Float canned values** — a `returns("rate", 0.1)` round-tripped through PHP could diverge on the
   14-digit `echo` rule (KNOWN_ISSUES). *Mitigation:* the mock stores/returns the float unchanged; the
   divergence is only at *display*, identical to any other float — examples keep to representable
   values, the `run≡runvm` spine is always exact.

---

## 10. Effort

**Medium.** Concretely:
- `synthesize_mock(&InterfaceDecl) -> ClassDecl` — ~150 lines (per-method body builder reusing existing
  AST constructors + the default-by-type table).
- `Mock.of<T>()` intrinsic recognizer + injector pass — ~80 lines, modeled line-for-line on
  `inject_json_prelude` + the `typeName` span-keyed rewrite.
- Fixed recording/verification method templates — parsed once from a `MOCK_PRELUDE` string constant
  (like `JSON_PRELUDE`), spliced per interface.
- Diagnostics + `phg explain` entries — ~40 lines.
- Example + differential coverage — `examples/test/mocking/`.
- No backend, no `Op`, no `Value`, no transpiler-helper work beyond the existing class path.

Single self-contained slice; lands green + byte-identical in one change.

---

## 11. Feasibility & confidence

- **Feasibility: ~80%.** The codegen-as-injection mechanism is *proven* (`Json`/`RoundingMode`), the
  interface AST already carries everything needed, mutation already works spine-safely, and zero new
  `Op`/`Value` is required. The one genuinely open area is the heterogeneous-return ergonomics (§5) —
  a design choice, not a feasibility risk.
- **Confidence: medium.** High on the mechanism and byte-identity; medium overall because the API shape
  (string method-name keys vs a more type-safe surface) and the canned-return representation want a
  developer decision, and the feature is most valuable *coupled to* the `phg test` runner and the
  seeded Faker, which are designed in sibling stages — the three should be reconciled before building.

---

## 12. Open questions for the developer

1. **API shape:** string method-name keys (`c.returns("now", 42)`, `c.timesCalled("now")`) are simple
   but stringly-typed — a typo is a runtime no-op, not a checker error. Acceptable for a test helper, or
   do you want a checked surface (which needs per-method generated stub-setters, e.g.
   `c.returnsNow(42)` — more codegen, fully checked)?
2. **Canned-return representation:** per-primitive-kind slots (v1, ugly-but-simple) vs a single
   `Map<string, Json>` once Json-as-Any lands vs requiring programmable closure stubs from day one?
3. **Scope:** interfaces only for v1 (recommended), or do you want concrete-class mocking (subclass an
   `open` class) in the first cut?
4. **Strict vs loose:** should an unstubbed call return the type's zero (loose) or fault
   (strict)? v1 default?
5. **Runner coupling:** should mocks auto-register with the `phg test` runner so unmet expectations fail
   a test at teardown, or stay a pure value the test asserts on explicitly? (Determines whether this
   slice can ship independently of the runner.)
