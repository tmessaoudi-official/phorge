# Track D — Consolidate Already-Found Gaps (harvest of KNOWN_ISSUES.md)

**Track summary.** This track does no fresh research: it reads `KNOWN_ISSUES.md` in full and promotes
every documented deferral/limitation that is a real roadmap candidate into the consolidated gap list,
with a proposed milestone. Phorj's deferrals are almost all *deliberate scope boundaries* — out-of-scope
constructs reject cleanly (type/parse error, non-zero exit) and never panic, which is itself the
philosophy working as intended. After cross-checking against `MILESTONES.md`/`ROADMAP.md`/`FEATURES.md`,
several KNOWN_ISSUES headings are already **superseded** (the file lags reality): the **Mutation
milestone is FEATURE-COMPLETE**, **generics-all is CLOSED** (methods + classes shipped), **declaration
visibility is DONE**, **stack-traces Slice 1 (reporting) is DONE**. I therefore drop the "still-open"
status of items the milestones table marks shipped, and harvest only what remains genuinely deferred.
The dominant pattern: a cluster of deferrals all root to **one keystone** — reified-not-just-erased
generic result types (the `id(7)+1` operand gap) and the broader stdlib/`Any` work — which the GA
roadmap already names **M10 (type-system keystone) → M11 (stdlib completion)**. The other large cluster
is **fault handling Slice 2** (`try`/`catch` vs `Result`) and the **language slices still owed**
(overloading → extends → traits → exceptions). Everything here is a `port` (a PHP feature we lack) or a
`defer`/`omit` refinement; nothing is beyond-PHP `new`. Most are *strong* philosophy fits because they
remove a surprise or close a `run`↔`runvm`/PHP divergence; a handful are honest `reject`s where the
"feature" is a footgun PHP itself would not reward.

## Gap table

| id | title | kind | fit | rec | milestone | effort |
|---|---|---|---|---|---|---|
| D-faults-catch | Catchable error model (`try`/`catch` vs `Result<T,E>`) — fault Slice 2 | port | strong | adopt | M11 | L |
| D-trace-method-fileline | Method/ctor/closure frames are `line`-only (no `file:line`) | port | strong | adopt | M-faults Slice 1.1 | M |
| D-trace-expr-granularity | Frame lines are statement-granularity (multi-line expr → start line) | defer | ok | defer | M-faults Slice 1.1 | M |
| D-generic-result-operand | Generic-typed result is not a numeric operand (`id(7)+1` run↔runvm break) | port | strong | adopt | M10 | M |
| D-generic-iface-methods | Generic *interface* methods (`<T>` on an interface sig) | port | ok | defer | M-RT generics-followup | M |
| D-generic-crosspkg-types | Cross-package generic *library* types (`Box<T>` in a non-`main` pkg) | port | ok | defer | M5-followup | M |
| D-generic-explicit-args | Explicit type args at construction (`Box<int>(7)`) | port | weak | reject | — | S |
| D-generic-enums | Generic *enums* (`enum Opt<T>`) | port | strong | adopt | M-RT generics-followup | M |
| D-generic-fn-value | A generic function used as a first-class *value* | port | ok | defer | M-RT generics-followup | M |
| D-empty-list-generic | Empty `[]` passed straight to a generic param (no element to infer) | defer | ok | defer | M10 | S |
| D-bounds-variance | Generic bounds + variance (in/out) | port | weak | reject | — | L |
| D-overloading | Method/ctor overloading (`foo(int)`/`foo(string)`) | port | strong | adopt | M-RT (next slice) | M |
| D-extends | Class `extends` (single inheritance, final-by-default) | port | strong | adopt | M-RT S6 | M |
| D-traits | Traits / mixins | port | strong | adopt | M-RT S8 | L |
| D-operator-overload | Operator overloading | omit | weak | reject | — | M |
| D-property-accessors-backed | Backed property hooks (own slot + `$this->name`) + static/iface/abstract hooks | port | ok | defer | M-mut-followup | M |
| D-sized-ints-decimal | Sized integers / `decimal` | port | ok | defer | v2 | L |
| D-const-final-enforce | `const`/`final` enforcement | port | ok | defer | M-RT S6 | S |
| D-match-position | `match` outside return / var-init position (statement/expr-any) | port | strong | adopt | M11 | S |
| D-cycle-collector | Cycle collector (leaked `a.next=b; b.next=a`) | port | ok | defer | M11-GC / v2 | L |
| D-identity-eq | Identity `===` (`Rc::ptr_eq`) | port | ok | defer | M11 | S |
| D-nested-place-store | Nested place-stores (`this.f[i]=e`, indexed field target) | port | strong | adopt | M-mut-followup | M |
| D-member-visibility | Member-level `private`/`protected` enforced by Phorj backends | port | ok | defer | M-RT / M11 | M |
| D-vis-on-alias-import | Visibility keyword on `type` alias / `import` re-export | port | weak | reject | — | S |
| D-core-list-hof-done | `core.list` map/filter/reduce | port | strong | adopt | M11 (SHIPPED M-RT S7b — verify) | — |
| D-core-json | `core.json` (dynamic `Any`/`Json` type) | port | strong | adopt | M11 | L |
| D-tuples | Tuples | port | ok | defer | M11 | M |
| D-map-iteration | Map iteration (`for (k,v in m)`) | port | strong | adopt | M11 | M |
| D-set-union-intersect | `Set` union / intersection | port | strong | adopt | M11 | S |
| D-empty-map-literal | Empty/growable map literal + builder | port | ok | defer | M11 | S |
| D-float-key | `float` map keys | omit | weak | reject | — | S |
| D-html-url-css-script | `Core.Html` escaping for URL / inline-CSS / `<script>` contexts | port | strong | adopt | M-html-followup | M |
| D-html-quoted-hole | `html"…"` hole cannot hold a quoted string literal (shared interp rule) | defer | weak | reject | — | M |
| D-html-all-tags | Named helper for every HTML tag (vs curated set + `el`) | map | weak | reject | — | S |
| D-interp-line1 | Faults inside `"{…}"` interpolation report line 1 (caret wrong) | port | strong | adopt | M-faults Slice 1.1 | M |
| D-interp-nested-quotes | Nested quotes in interpolation (`"{m["k"]}"`) end string early | defer | ok | defer | M-faults / lexer | M |
| D-transpile-php-builtin | `package Main` fn names collide with PHP builtins (transpile) | port | strong | adopt | M8 | S |
| D-transpile-private-field | Externally-read fields must be `public` (PHP enforces `private`) | port | ok | defer | M-RT (with member-vis) | S |
| D-transpile-float-divzero | Non-finite float `/0` → PHP `DivisionByZeroError` (fault-domain) | defer | weak | reject | — | S |
| D-transpile-optbang-msg | `opt!`-on-null transpiles to a different fault message | defer | weak | reject | — | S |
| D-build-vendor-merge | `phg build` is single-file, can't merge `vendor/` | port | ok | defer | M2.5 / post-M5 | M |
| D-build-transitive-deps | Transitive dependency resolution (`phg vendor`) | port | strong | adopt | M5-followup | M |
| D-build-macos-stub | macOS stub production (`--target` apple rejected) | defer | ok | defer | M2.5 Phase 3 | L |
| D-build-argv | Built binaries ignore argv / always exit 0 | port | strong | adopt | M2.5 Phase 3 | M |
| D-build-cross-noskip | aarch64 / Windows artifacts not *executed* in CI | defer | ok | defer | M9 (CI) | M |
| D-concurrency | Concurrency (`spawn` + channels, green threads) | port | strong | adopt | M6 | L |
| D-lambda-this | Lambda cannot reference `this` (`E-LAMBDA-THIS`) | port | ok | defer | M-RT / M3-followup | M |
| D-lambda-lib-pkg | Lambdas / fn-values inside library (non-`main`) packages | port | strong | adopt | M5-followup | M |
| D-lambda-block-infer | Statement-body lambda return inference (drop mandatory `-> T`) | port | ok | defer | M3-followup | S |
| D-fn-type-variance | Function-type assignability variance (`(int)->int` vs `(int)->int?`) | port | weak | reject | — | M |
| D-listsum-overflow | `List.sum` faults on i64 overflow; PHP `array_sum` widens to float | defer | weak | reject | — | S |

## ADOPT rationales

**D-faults-catch — catchable error model (`try`/`catch` vs `Result`).** Slice 1 (reporting) is done;
the explicitly-named Slice 2 is the big remaining language gap. PHP devs reach for `try`/`catch` daily —
omitting a catch mechanism is a *missing capability*, not a removed surprise. The pragmatic, PHP-familiar
form is `try`/`catch`/`throw` mapping 1:1 to PHP's; a `Result<T,E>` layer is the legible-upgrade option
but must not become the *only* form (that would be PL-theory maximalism over a PHP-native audience). Sequence
in M11 as the GA roadmap already plans ("S4 = exceptions"). L effort — needs a value-carrying error model
across all three backends + the byte-identity spine on the fault path.

**D-trace-method-fileline — full `file:line` on method/ctor/closure frames.** Slice 1 shipped traces but
method frames are `line`-only because their names are backend-synthesized, not in the loader's
function→file map. Threading the file attribution through closes the gap so every frame reads
`file:line` — a clear legibility upgrade a PHP dev expects from a stack trace. M effort, a natural
Slice 1.1 follow-up.

**D-generic-result-operand — make a generic result a numeric operand.** This is a *verified*
`run`↔`runvm` divergence (`id(7)+1` prints `8` on the interpreter, errors on the VM) — exactly the class
of surprise Phorj exists to eliminate, and the GA roadmap's keystone M10 is where reified generic result
types belong. Highest-value adopt: it removes an inconsistency between two backends that are contractually
byte-identical.

**D-generic-enums — `enum Opt<T>`.** Generic enums are the idiomatic PHP-absent-but-erasable shape for
`Option`/`Result`-style sum types, which directly enables D-faults-catch's `Result<T,E>` form and
`core.json`'s `Any`. Erases like every other generic; a PHP dev reads `enum Opt<T> { Some(T), None }`
instantly. Adopt as a generics follow-up.

**D-overloading / D-extends / D-traits — the owed M-RT slices.** All three are confirmed roadmap items
(overloading is literally the next slice; `extends` then traits follow). Each is core PHP/OOP a developer
expects, each lowers to idiomatic PHP (overloading → one dispatching method; `extends`/`implements`/traits
→ their PHP keywords). Strong fits; adopt in their already-sequenced slots.

**D-match-position — `match` in statement/any-expression position.** Today `match` is restricted to
return / var-init position; PHP's `match` is a general expression. Lifting the restriction removes an
arbitrary surprise and is small (the lowering already exists). Adopt into M11.

**D-nested-place-store — `this.f[i] = e` and indexed field targets.** Mutation is feature-complete but
this corner rejects cleanly; an indexed write into a field is everyday PHP (`$this->items[$i] = $x`).
Closing it removes a surprise on the mutation surface a PHP dev would immediately hit. Adopt as a mutation
follow-up.

**D-core-json / D-map-iteration / D-set-union-intersect / D-core-list-hof — stdlib completion.** These are
the stdlib breadth the GA roadmap's M11 already targets (and `core.list` map/filter/reduce + Set of/contains
already SHIPPED in M-RT S7b — flag to verify the KNOWN_ISSUES "pending" note is stale). `core.json` needs a
dynamic `Any`/`Json` type (rooted in M10); map iteration and Set union/intersection build on the shipped
generic-native path. All are bread-and-butter PHP (`json_decode`, `foreach ($m as $k=>$v)`,
`array_intersect`) — strong fits, adopt in M11.

**D-html-url-css-script — context-aware escaping.** `Core.Html` escapes text + attribute values but is
explicitly *unsafe* for URL/CSS/`<script>` contexts. Phorj's whole HTML pitch is "XSS-safe by
construction"; leaving these contexts unescaped is a security surprise. Adopt a follow-up wave adding
context-specific escapers (URL-encode, CSS, JSON-in-script) — strong fit with the provably-safe philosophy.

**D-interp-line1 — accurate location for faults inside interpolation.** A fault inside `"{…}"` reports
line 1 with the caret at column 1 — a wrong diagnostic, the opposite of the "sharp caret" S0.4 promise.
Front-end-only fix (sub-lexer position threading), squarely in the fault-reporting follow-up. Adopt.

**D-transpile-php-builtin — `package Main` fn-name/PHP-builtin collisions.** A `package Main` function
named `serialize`/`strlen`/`header` transpiles to a global PHP fn and fails to redeclare — a real
transpile-target footgun the M8 hardening slot should catch with a checker warning/error (e.g.
`W-PHP-BUILTIN-NAME`). Small, high-value: it turns a confusing PHP-side failure into a clear Phorj-side
diagnostic. Adopt.

**D-build-transitive-deps — transitive dependency resolution.** `phg vendor` fetches only the direct
`[require]` set; real package management resolves the dependency graph. PHP devs expect Composer-grade
transitivity; this is a completion of the M5 model. Adopt as an M5 follow-up.

**D-build-argv — built binaries honor argv.** A standalone `phg build` binary ignores command-line args
and always exits 0 — surprising for any CLI program. Passing argv through to the embedded program (and
propagating its exit code) is expected behavior. Adopt in the M2.5 Phase 3 polish.

**D-concurrency — `spawn` + channels.** Named M6; uncolored green threads on the VM's reified call frames
is the planned, PHP-pragmatic concurrency story (no async/await colouring surprise). Strong fit; adopt in
its milestone.

**D-lambda-lib-pkg — lambdas / fn-values in library packages.** Lambdas work in `package Main` but a
fn-value or lambda body in a dotted library package isn't rewritten by the loader's mangling pass and
rejects cleanly. This is an arbitrary surprise (the same lambda works in `main` but not in a package) —
closing it is a loader follow-up that makes the language uniform. Adopt as an M5 follow-up.

## Critic pass

Adversarial completeness + mis-listing sweep against `KNOWN_ISSUES.md`, `FEATURES.md`, the project
CLAUDE.md milestone log, and `src/`.

### Mis-listed (already SHIPPED) — remove from the open list

- **D-match-position — ALREADY SHIPPED.** [Verified] `match` is parsed in `parse_primary`
  (`src/parser.rs:405`, `parse_match` at :713) so it is a **general expression** usable in any
  expression position; a runnable `examples/guide/match-expr.phg` exercises it as the left operand of
  `+`, as a call argument, and in a `return` — and `KNOWN_ISSUES.md` (lines 290–295) itself states
  literal/expression-position `match` *"were completed in M11"* and are PHP-oracle byte-identity-gated.
  The `KNOWN_ISSUES.md:65` line ("`match` outside return / variable-declaration-initializer position")
  is **stale**. Recommendation: drop the open item; flag `KNOWN_ISSUES.md:65` for deletion.
- **D-core-list-hof-done — ALREADY SHIPPED (the original list already flags it).** [Verified] `Core.List`
  `map`/`filter`/`reduce` ship in M-RT S7b-3 (`FEATURES.md:37`, KNOWN_ISSUES *Generic natives*); the
  trailing "Still pending … higher-order `Core.List`" note (`KNOWN_ISSUES.md:257-258`) is **stale**.
  Kept in the merged list only as a "verify/sync KNOWN_ISSUES" docs flag (no language work).

### Newly-found (MISSED) gaps — full rows

These are documented `KNOWN_ISSUES.md` deferrals the harvest skipped. All are PHP-familiar-or-erasable
and reject cleanly today; none is beyond-PHP `new`.

| id | title | kind | fit | rec | milestone | effort |
|---|---|---|---|---|---|---|
| D-instanceof-intersect-rhs | `instanceof` with an **intersection right side** (`x instanceof (A & B)`) | port | strong | adopt | M-RT S5-followup | S |
| D-union-flow-narrow | Negative/flow narrowing on a union (else-branch narrows to remaining members) | port | ok | defer | M-RT unions-followup | M |
| D-union-common-member | Common-member access on a raw union without narrowing (`(A\|B).foo()`) | port | ok | defer | M-RT unions-followup | M |
| D-whole-union-optional | Whole-union / whole-intersection optional `(A\|B)?` / `(A & B)?` | port | weak | reject | — | M |
| D-type-pattern-nested | Type pattern nested in a variant payload (`match w { Wrapper(Circle c) => … }`) | port | ok | defer | M-RT unions-followup | M |
| D-module-qualified-type | Module-qualified type form (`import acme.geometry;` then `Geometry.Point`) | port | ok | defer | M5-followup | M |
| D-crosspkg-fn-value | Cross-package function *value* (passing `acme.util.compute` itself, not calling it) | port | strong | adopt | M5-followup | M |
| D-field-set-intersection | Field-set on an intersection-typed object (`x.f = e` where `x: A & B`) | port | weak | defer | M-mut-followup | S |
| D-readonly-final-emit | `readonly`/`final` field emission in transpiled PHP | port | ok | defer | M-RT S6 / M8 | S |
| D-fault-cause-chain | No "cause chain" on faults (nested cause / previous-exception) | port | ok | defer | M11 (with fault Slice 2) | M |
| D-bidir-infer | Full bidirectional inference (empty `[]` in `return`/decl-init, not only call-arg) | port | ok | defer | M10 | M |
| D-map-bool-int-key-coerce | Map `bool`/int-like-string key coercion diverges from PHP arrays | defer | weak | reject | — | S |

**ADOPT rationales (new):**

- **D-instanceof-intersect-rhs — `instanceof (A & B)`.** [Verified `KNOWN_ISSUES.md:38-39,46`] An
  intersection-typed *operand* already works; only the intersection **right side** is deferred because
  `Op::IsInstance` carries a single name. The PHP-legible lowering is trivial and PHP-target-faithful —
  `x instanceof A && x instanceof B` — no new `Op` needed. Small, removes an asymmetry a developer hits
  the moment they have an `A & B` and want to test it. Pairs naturally with the S5 follow-up.
- **D-crosspkg-fn-value — a cross-package function used as a value.** [Verified `KNOWN_ISSUES.md:148`]
  *Calling* `acme.util.compute(x)` cross-package works; passing the function itself as a value does not
  (the loader's mangling rewrites call sites, not bare references). This is the same uniformity surprise
  as D-lambda-lib-pkg ("works in `main`, not in a package") and belongs in the same loader follow-up —
  closing both makes first-class functions package-uniform. Strong fit.

**DEFER/REJECT rationales (new):**

- **D-union-flow-narrow / D-union-common-member / D-type-pattern-nested** — real union-ergonomics
  completions, all rejected-clean today and all rooted in the same narrowing engine; bundle as a
  unions follow-up rather than block. *Ok* fit (PHP has no unions, so the "PHP-familiar form" is weak,
  but TypeScript devs expect exactly these — legibility-positive).
- **D-whole-union-optional `(A|B)?` — reject.** `?` is postfix on a single member by design
  (`A | B?` parses as `A | (B?)`); `T?` already covers nullability and a union-of-optional is just
  `A | B | Null`-shaped. Adding parenthesized-postfix-optional is parser surface for a form `T?` already
  expresses — purism over pragmatism. Reject cleanly (matches the existing footgun-or-purism rejects).
- **D-module-qualified-type — defer.** The terminal `import type Pkg.Path.Type` form shipped; the
  Go-style module-qualified `Geometry.Point` was explicitly deferred (`KNOWN_ISSUES.md:56-61`). It's a
  pure ergonomics alternative to a shipped form, not a capability gap — M5 follow-up.
- **D-field-set-intersection — defer.** A field *set* on an intersection-typed object is deferred
  (`KNOWN_ISSUES.md:88`); narrow (or `instanceof`) to a concrete class first. Niche corner of the
  already-feature-complete mutation surface; pair with D-nested-place-store as a mutation follow-up.
- **D-readonly-final-emit — defer.** Immutable fields are already write-prevented by the checker, so the
  missing PHP `readonly`/`final` *emission* (`KNOWN_ISSUES.md:94`) is a transpile-fidelity nicety, not a
  correctness gap. Land alongside `final`-by-default in S6 (or M8 hardening).
- **D-fault-cause-chain — defer.** A cause/previous-exception chain needs the value-carrying error model
  from fault **Slice 2** (`KNOWN_ISSUES.md:19`) — it cannot exist before D-faults-catch. Fold into M11.
- **D-bidir-infer — defer.** Empty `[]` infers only in call-argument position (`KNOWN_ISSUES.md:270-275`);
  generalizing to `return`/decl-init is the same bidirectional-inference completeness that defers
  D-empty-list-generic — co-locate at M10.
- **D-map-bool-int-key-coerce — reject (caveat).** PHP arrays coerce integer-like string keys and
  `bool` keys to `int`; Phorj keeps them distinct (`KNOWN_ISSUES.md:233-235`, :246-248). The Phorj
  behavior is *more* correct (no silent key collapse — same class as D-float-key); the `run`↔`runvm`
  spine is always byte-identical. Documented caveat, not a gap. Reject cleanly.

### Deliberately NOT promoted (sanity-checked, correctly absent)

Three `KNOWN_ISSUES.md` "Behavioral quirks" are *philosophy working as intended*, not gaps, and are
correctly omitted: **recursion depth-limited** (`limits.rs`, prevents a native stack overflow — a
safety feature), **zero-payload variant `V()` call form** (a documented footgun the differential
harness can't catch — a doc item, not a roadmap port), and the **interpolation line-1 quirk** which
*is* promoted as D-interp-line1. No further sanity-flip needed on the existing rejects: operator
overloading, function-type variance, `float` keys, explicit-type-args, and the two transpile
fault-domain divergences all correctly stay `reject` (each would import a surprise or PL-theory
machinery a PHP-familiar developer neither expects nor benefits from).

---

(Rejects/defers carry their reason inline in the table; the recurring reject reason is footgun-or-purism:
operator overloading, function-type variance, `float` keys, explicit-type-args, and the transpile
fault-domain divergences (`opt!` message, float `/0`) are all cases where adding the feature would buy a
surprise or PL-theory complexity that a PHP-familiar developer neither expects nor benefits from, and the
clean-rejection status quo already matches the philosophy.)
