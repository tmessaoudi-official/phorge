# Track 3 — Byte-Identity Spine Under Mutation

**Milestone:** Mutation + garbage collection (the "deferred" milestone in the GA roadmap).
**Non-negotiable invariant:** `run ≡ runvm ≡ real PHP`, byte-identical stdout (INVARIANT #1).
**Thesis:** Mutation is the single biggest threat to that spine, and the threat is *not* "mutation is
hard" — it is that **PHP's value/handle split is a semantic fork** that Phorj's current
"everything is `Rc`-shared with an immutable interior" model does not encode. Get the value/handle
rule right and almost every sub-question (aliasing, foreach, copy-on-assign, clone, closure capture)
falls out of one decision. Get it wrong and *every* program that mutates a list silently diverges.

All PHP behaviors below were executed against the repo's real interpreter
(`/stack/tools/phpbrew/php/php-master/bin/php`, **PHP 8.6.0-dev**, run with `php -n` to match the
M7 oracle's no-ini policy) — `[Verified]` means "I ran this exact program and pasted the output."

---

## 0. The ground state (what the code actually is today)

[Verified: read `src/value.rs`, `src/ast.rs`, `src/vm.rs`, `src/chunk.rs`, `src/interpreter.rs`]

- **Every heap value is `Rc<T>` with an immutable interior, no `RefCell`/`Cell`** — `value.rs`:
  `List(Rc<Vec<Value>>)`, `Map(Rc<Vec<(HKey,Value)>>)`, `Set(Rc<Vec<HKey>>)`, `Instance(Rc<Instance>)`,
  `Enum(Rc<EnumVal>)`, `Closure(Rc<ClosureData>)`, `Bytes(Rc<Vec<u8>>)`. Cloning a `Value` is a
  refcount bump (`Op::GetLocal` hot path). `grep` confirms **zero** `RefCell`/`Cell`/`make_mut`/
  `get_mut` on `Value` anywhere in `src/`. [Verified]
- **There is no assignment statement.** `Stmt` is `VarDecl | Return | If | For | Block | Expr` — a
  `VarDecl` *introduces* a binding (`self.frame.declare(name, v)`), it never *reassigns* one. The VM
  has `Op::SetLocal` but the compiler only emits it to **initialize** a slot, never to overwrite a
  live binding. [Verified: `ast.rs` `Stmt`, `interpreter.rs:252`]
- **The interpreter's scope model is `Vec<HashMap<String,Value>>`** (block-scope stack);
  `eval_ident` walks it `rev()` and **clones** the found value out (`interpreter.rs:78`). A name read
  always produces a *fresh `Value`* (refcount bump). The VM's locals are a stack window;
  `Op::GetLocal` clones the slot. So **today, no two live bindings ever observe each other** — every
  read detaches via clone. This is exactly why the heap is "immutable + acyclic" and `Rc`/`Drop`
  reclaims fully (CLAUDE.md, `value.rs` header). [Verified]
- **Closures capture by value** — `ClosureData::Tree { env: Vec<(String,Value)> }` and
  `ClosureData::Byte { captures: Vec<Value> }`; `ast::free_vars` snapshots the names and the call
  site clones the values in. `E-LAMBDA-THIS` forbids capturing `this`. [Verified: `value.rs:52-67`,
  `ast.rs:249`]
- **`for (T x in iter)` iterates a materialized `List`** (`interpreter.rs:297`): it `eval`s the
  iterator to a `Value::List(Rc<Vec<Value>>)` *once*, then `for item in items.iter()` over that
  snapshot, cloning each element into the loop var. Ranges and Maps already materialize to a `List`.
  [Verified]
- **The Op coupling rule is hard** (INVARIANT #5): a new `Op` is a compile error until arms are added
  to all three of `vm::exec_op`, `compiler::stack_effect`, and `BytecodeProgram::validate`. Any
  mutation Op (`SetField`, `SetIndex`, compound-assign lowering, …) pays this tax. [Verified]

**Implication for the whole milestone:** the byte-identity spine is currently *free* because nothing
can be observed-after-mutation — there is no mutation. The moment a mutation primitive lands, the
spine becomes a property that must be *engineered into the value model*, not assumed. The rest of
this document enumerates exactly where it breaks and what each backend must enforce.

---

## 1. PHP's value/handle split — THE root semantic, and the master decision

[Verified: ran `mut_test.php`, `mut_test3.php`, `mut_test5.php` — outputs inline]

PHP has exactly two assignment/parameter behaviors, split by type:

| PHP type | Assignment `$b = $a` | Pass to function | Observed mutation? |
|---|---|---|---|
| **array** (and all scalars) | **copy** (value, copy-on-write) | **by value** (copy) | No — disjoint after assign |
| **object** | **handle copy** (alias) | **by handle** (alias) | **Yes** — both names see it |

Empirical proof (PHP 8.6, `php -n`):

```
arr_copy:   a=3 b=4              # $b=$a; $b[]=4  → $a untouched (array = value)
obj_ref:    o->v=99 p->v=99      # $p=$o; $p->v=99 → both 99 (object = handle)
nested_arr: a[0]=1 b[0]=2        # $b=$a; $b[0][]=99 → DEEP value copy, even nested arrays
pass_handle: 1                   # bump(Box) mutates caller's object
pass_array:  2                   # push($arr) does NOT mutate caller's array
obj_arr_alias: o=1 p=1           # $p=$o; $p->items[]=5 → both see it (array reached via shared handle)
```

The deepest subtlety (`nested_arr`): **array value semantics are deep** — copying an array copies its
nested arrays too (logically; physically COW-separated on first write). But an array *field inside a
shared object* (`obj_arr_alias`) is reachable through the shared handle, so mutating it is visible —
the array didn't alias, the *object* did.

### Why this is a fork for Phorj specifically

Phorj today represents **both** `List` and `Instance` as `Rc<T>`. Cloning either is a refcount bump
→ aliasing. While immutable, aliasing is unobservable, so List-as-`Rc` is harmless. **Under
mutation, `Rc`-sharing gives a List PHP-*object* semantics (aliasing visible), but PHP gives arrays
*value* semantics (aliasing invisible).** That is a direct, guaranteed byte-identity break the
instant any program does:

```phorj
mutable List<int> a = [1, 2, 3];
List<int> b = a;        // PHP: COPY.  Naive Rc-share: ALIAS.
a.push(4);              // PHP: b stays [1,2,3].  Naive: b becomes [1,2,3,4]. DIVERGENCE.
```

### The master decision (forced, not a fork)

**Phorj must split its value model along PHP's exact line: collections (`List`/`Map`/`Set`) get
*value* semantics; class instances (`Instance`) get *handle* semantics.** This is not a stylistic
choice — it is the only way `run ≡ runvm ≡ real PHP` survives mutation. [Inferred from the verified
PHP table + the verified `Rc`-everything representation; the conclusion is forced once both are
established.]

The good news: **the substrate already exists and is the industry-standard answer.** Swift solves
the identical problem with **copy-on-write value types** (structs/Array/Dictionary) vs **reference
types** (classes), implemented with `isKnownUniquelyReferenced()` on a shared buffer
([Apple/Swift COW](https://medium.com/@lucianoalmeida1/understanding-swift-copy-on-write-mechanisms-52ac31d68f2f),
[Swift value/reference types](https://medium.com/capital-one-developers/reference-and-value-types-in-swift-de792db330b2)).
PHP itself implements arrays the same way — **copy-on-write zval separation** keyed on refcount
([PHP Internals Book — Memory management](https://www.phpinternalsbook.com/php5/zvals/memory_management.html),
[Zend — Copy on Write](https://www.zend.com/resources/php-extensions/copy-on-write)). PHP objects are
"just an ID that looks up the content"
([PHP Internals — references](https://www.phpinternalsbook.com/php7/zvals/references.html)).

Rust's exact analog of `isKnownUniquelyReferenced()` is **`Rc::make_mut`** (clones iff
`strong_count > 1`, else mutates in place) and `Rc::strong_count`/`Rc::get_mut`. So Phorj can keep
`List(Rc<Vec<Value>>)` *unchanged in representation* and get PHP-identical array value semantics by
routing every list mutation through `Rc::make_mut` — **no representation change, no new `Value`
variant.** Instances, by contrast, must become *shared-mutable*, which requires interior mutability
(`Rc<RefCell<Instance>>`) and is what introduces cycles → the GC. [Inferred: `make_mut` semantics are
documented std behavior; the mapping to PHP's two cases is direct.]

---

## 2. Enumeration — every way mutation can break three-way identity

For each: PHP's verified behavior → the divergence risk → what each backend must enforce.

### 2.1 Reassignment of a local (`mutable x = …; x = …`)

- **PHP:** trivially supported; `$x = 5; $x = 6;`. A scalar reassign is pure value replacement.
- **Risk:** low for scalars (the slot just holds a new `Value`). The interpreter must reassign **in
  the same scope frame** where the binding lives — `eval_ident` walks `rev()` and the *nearest*
  binding wins, so a reassign must target that same `HashMap` entry, not `declare` a shadow in a
  child scope. The VM must emit `SetLocal(slot)` to the binding's *original* slot.
- **Enforce:** the checker resolves the assignment target to a specific declared `mutable` binding
  (reassigning an immutable binding = `E-REASSIGN-IMMUTABLE`); both backends write to the resolved
  slot/scope-entry, never introduce a new binding. A scalar reassign is byte-identical by
  construction (no sharing involved). [Inferred]

### 2.2 Aliasing — two bindings to one collection, mutate via one

- **PHP arrays:** **no aliasing** — `$b=$a; $b[]=x` leaves `$a` untouched (`arr_copy` Verified).
- **PHP objects:** **aliasing IS the semantics** — `$p=$o; $p->v=99` ⇒ `$o->v==99` (`obj_ref` Verified).
- **Risk:** the headline break (§1). Naive `Rc`-share makes a List alias like an object.
- **Enforce:**
  - **List/Map/Set:** `b = a` clones the binding's `Value` (already a refcount bump). The *first
    mutation* through `a` calls `Rc::make_mut`, which detaches `a`'s buffer from `b`'s. Result is
    byte-identical to PHP's COW. The VM does the same in its `Op::SetIndex`/`push`-native handler.
  - **Instance:** `b = a` shares the handle (the desired semantics); mutation through either is
    visible through both. Requires `Rc<RefCell<Instance>>` (or equivalent) so the interior is
    actually shared-mutable. **Both backends must use the same representation** or one will detach
    where the other shares. [Inferred — this is the load-bearing parity requirement of the milestone.]

### 2.3 Array/list copy-on-assignment vs object reference-on-assignment

- **PHP:** the §1 table — array copies, object aliases, *deeply* for arrays (`nested_arr` Verified:
  `a[0]=1 b[0]=2`).
- **Risk:** (a) getting the split wrong (covered); (b) **depth** — a List of Lists assigned-then-
  mutated-deep must not let the inner List alias either. With `Rc::make_mut` this is handled
  *lazily and correctly*: `b[0]` is itself an `Rc<Vec>`; mutating `b[0]` calls `make_mut` on the
  outer (detaching the outer `Vec` of `Rc`s, refcount-bumping each inner `Rc`), then `make_mut` on
  the inner (detaching just `b[0]`). PHP's COW separates identically — same observable result,
  same lazy cost. (c) **A List field inside an Instance** — `obj_arr_alias` (Verified both see `5`):
  here the array is reached through the shared instance handle, so mutating it *is* visible. With the
  split model this is automatic: the instance is shared, so `inst.items` resolves to the same
  `Rc<Vec>`, and `make_mut` finds `strong_count==1` *within that one shared instance* → in-place,
  visible to both names. **The depth rule is: value semantics compose with handle semantics exactly
  as PHP composes them, for free, if `make_mut` is the only mutation path.** [Inferred from verified
  PHP outputs + `make_mut` semantics.]
- **Enforce:** *single mutation kernel.* Every list/map mutation in BOTH backends goes through one
  `value::list_set`/`list_push`/`map_set` kernel that does `Rc::make_mut` — never a hand-inlined
  `Rc::get_mut`/clone in `interpreter.rs` or `vm.rs` (the INVARIANT #3 discipline, extended to
  mutation). This is the structural guarantee that the two backends cannot drift.

### 2.4 Mutation inside a loop; iteration over a collection being mutated

- **PHP:** `foreach ($a as $x) { … $a[]=99; … }` iterates **a COPY** — the appended element is **not
  visited** (`mut_test2.php` Verified: prints `1 2 3`, final count 4). PHP snapshots the array for the
  loop (its COW gives the loop its own view).
- **Phorj today:** `Stmt::For` already materializes the iterator once into a `List` and iterates
  that snapshot (`interpreter.rs:297-307`). So **Phorj already has PHP's "iterate a copy" semantics
  for free** — even after mutation lands, as long as the loop keeps eval-once-then-iterate-snapshot.
- **Risk:** if a future optimization makes `for` iterate the *live* `Rc` buffer (to avoid the
  snapshot), and the body mutates the same binding, the two backends could disagree on whether the
  in-flight mutation is seen — and both could disagree with PHP. The VM must materialize the same
  snapshot the interpreter does.
- **Enforce:** keep the eval-once-materialize-then-iterate model; add a differential case "mutate the
  iterated collection inside the loop" so a regression is caught. With `make_mut`, the body's first
  mutation detaches from the loop's snapshot automatically → byte-identical to PHP. [Verified PHP
  behavior + Verified current loop code; the enforcement is "don't regress".]

### 2.5 Default argument expressions — evaluated once vs per-call

- **PHP:** default arg values must be **constant expressions** (PHP forbids non-const defaults at the
  language level), so the "evaluated once" footgun (Python's mutable-default trap) **does not exist in
  PHP** — every call gets a fresh value-copy of the constant (`mut_test2.php` `default_array: 11`
  Verified — array default is fresh and value-copied each call). [Verified]
- **Risk:** if Phorj allows *arbitrary-expression* defaults (more powerful than PHP), it must decide
  evaluate-once-at-definition (Python footgun, breaks PHP parity) vs evaluate-per-call (PHP-like).
  Evaluate-once with a mutable default would make the default **alias across calls** — an instant
  three-way divergence (PHP has no such concept to match).
- **Enforce:** **evaluate-per-call** (matches PHP; avoids the footgun the philosophy says to remove).
  Restrict defaults to expressions with no observable side effect, or evaluate them fresh in the
  callee prologue on each call. Transpile to PHP defaults only when the default is a const expr; for
  richer defaults, lower to a `param ?? <default-expr>` in the callee body (the `??` is already a
  no-new-Op lowering). A mutable default that is evaluate-once is **FORBIDDEN** (see §3). [Inferred —
  the recommendation follows from the verified PHP rule + the no-surprise philosophy.]

### 2.6 Mutation through a closure capture (Phorj captures BY VALUE today)

- **PHP:** `use($x)` captures **by value at definition** for arrays/scalars (snapshot — `mut_test3.php`
  `closure_array_byval: 1` Verified), **by handle** for objects (`closure_obj_byhandle: 77` Verified),
  and `use(&$x)` captures **by reference** (`closure_byref: 5` Verified).
- **Phorj today:** captures by value, period (`ClosureData.env`/`captures` are cloned `Value`s).
  - For **scalars and Lists/Maps/Sets**, capture-by-value already matches PHP's `use($x)` value
    capture *exactly* — a later mutation of the outer binding is a `make_mut` detach, invisible to
    the closure's snapshot. Byte-identical, **no change needed.** [Verified PHP + Verified capture
    model.]
  - For **objects**, PHP's `use($o)` captures the *handle*, so a later mutation of `$o` **is** seen
    by the closure. Phorj's clone-the-`Value` capture — once `Instance` is `Rc<RefCell>` — would
    clone the *handle* (refcount bump), which **also shares the interior**. So capturing an instance
    by value in Phorj would, with the shared-mutable-instance model, *coincidentally* match PHP's
    handle capture (both see later field mutations), because cloning a handle shares the cell. So
    **the existing by-value capture is correct for both axes after the split** — this is a pleasant
    consequence of the value/handle split, not a separate mechanism. [Inferred — needs a differential
    test "capture an instance, mutate it after, call the closure" to confirm on both backends.]
- **Risk:** if instances are *not* made shared-mutable (e.g. someone tries `Rc::make_mut` on a
  captured instance), the closure's capture detaches and PHP's "closure sees the mutation" breaks.
- **Enforce:** instance capture = clone the handle (shares the cell). Do **not** route a closure-
  captured instance through `make_mut`. By-reference capture (`use(&$x)`) of a *scalar/list* is the
  one case Phorj cannot match without true reference cells — **reject `use(&$x)` value-aliasing of
  non-objects** (PHP reference operator is already a `reject` in the parity matrix, line 165/229).
  [Verified: the matrix rejects `&` references.]

### 2.7 `foreach`-by-reference (`foreach ($a as &$v) { $v = … }`)

- **PHP:** `&$v` mutates the array **in place** (`mut_test2.php` `byref: 10,20,30` Verified) — and
  leaves a dangling reference in `$v` after the loop (the famous PHP footgun requiring `unset($v)`).
- **Risk:** this *requires* per-element aliasing into the live array — the exact aliasing PHP arrays
  otherwise forbid. Replicating it on a `make_mut` value model is impossible without reference cells;
  it is also the source of one of PHP's most notorious bugs.
- **Enforce:** **REJECT `foreach`-by-reference.** It is the same `&`-aliasing the matrix already
  rejects (line 165/229: "Aliasing breaks the immutable + acyclic heap"). Preserve the *capability*
  (in-place transform of a list) via an **index-based mutating loop** (`for (int i in 0..a.len()) {
  a[i] = f(a[i]); }`) once indexed-assignment lands, or via `Core.List.map` (already shipped, no
  mutation). The capability is preserved; the footgun syntax is removed — exactly the philosophy's
  "remove surprises, never capability." [Verified PHP footgun; Inferred enforcement.]

### 2.8 Static-local persistence across calls (`static $n = 0;`)

- **PHP:** a `static` local **persists across calls** to the same function (`mut_test2.php`
  `static: 123` Verified — three calls return 1,2,3). It is per-function shared mutable state.
- **Risk:** this is *global mutable state with a function-scoped name*. It survives between calls,
  so it lives outside any frame's lifetime — it must be rooted somewhere the GC scans, and it makes
  function calls **non-pure** (same args, different result), which complicates the determinism story
  but does NOT break it (the order of mutation is still deterministic in a single-threaded program).
- **Enforce:** **defer to the mutation+GC milestone** (the matrix already defers `static` locals,
  line 450). When built: store statics in a program-level slab indexed by `(function, name)`, rooted
  for GC, shared by **both** backends from the same slab so the persisted value is identical. Initialize
  on first call (PHP semantics: the initializer runs once). Transpiles 1:1 to PHP `static`. The
  byte-identity requirement: both backends must agree on *when* the initializer runs (first call) and
  share the same persisted cell. [Verified PHP; Inferred enforcement; the matrix confirms the defer.]

### 2.9 Clone semantics (PHP `clone`, shallow vs deep, `__clone`)

- **PHP `clone` is SHALLOW** (`mut_test.php` Verified): `clone $o` copies the object's own properties;
  a nested **object** property stays **shared** (`shallow_clone: x.inner.v=42 y.inner.v=42` — both
  mutated), but a nested **array** property is value-copied (`obj_clone_arr: h1=2 h2=3` — independent).
  `__clone()` is a hook to deep-copy manually.
- **Risk:** Phorj must reproduce "shallow for objects, value-copy for arrays" *exactly*. With the
  split model this is automatic: `clone` makes a new `Instance` cell whose fields are **cloned
  `Value`s** — an object field clones the handle (shared, PHP-shallow ✓), an array field clones the
  `Rc<Vec>` (refcount bump; first mutation `make_mut`-detaches → value-independent, PHP-array ✓).
  **`clone` is just "construct a fresh instance cell, shallow-copy the field `Value`s" — and the
  value/handle split makes the shallow/deep distinction correct per-field for free.** [Inferred from
  verified PHP shallow-clone + the split model.]
- **`__clone`:** the parity matrix rejects `__clone` (line 176) and defers `clone` itself (line 139)
  as "meaningful only with mutation." **Clone-with** (PHP 8.5 `clone($o, [...])`, functional update)
  is `defer/high-ROI` (line 140/285) — and note PHP 8.5's `clone with` **respects `readonly`** at the
  language level (`mut_test4.php` Verified: `Cannot modify ... readonly property` — clone-with is the
  *only* sanctioned way to "change" a readonly field). For Phorj's immutable-by-default model,
  **clone-with is the idiomatic "change an immutable value"** and aligns perfectly. [Verified PHP
  8.5 behavior; matrix-confirmed ROI.]
- **Enforce:** when mutation lands, `clone` = fresh instance cell + shallow field-`Value` copy
  (split handles shallow/deep). `clone with [field => v]` builds the fresh cell with overrides — the
  preferred public form for immutable values; transpiles to PHP 8.5 `clone($o, [...])` (or a manual
  copy-ctor for ≤8.4 targets). Skip `__clone` (rejected; capability = clone-with). [Inferred]

### 2.10 Object identity vs equality after mutation

- **PHP:** `==` is **structural** (same class + equal props, recursive), `===` is **identity** (same
  handle) (`mut_test2.php` Verified: `$m==$n` true, `$m===$n` false, `$m===$o` true). **Mutation does
  not change identity** (`mut_test2.php` Verified: after `$m->v=5`, `$m===$o` still true).
- **Phorj today:** `eq_val` is structural for instances (compares class + fields recursively,
  `value.rs:238`) — matches PHP `==`. There is **no identity operator** (`===`) — Phorj has no
  handle notion yet. `eq_val` on a *cyclic* graph would **infinite-loop** (it recurses unbounded).
- **Risk:** (a) once instances are shared-mutable, an `===` identity test becomes meaningful and
  *differs* from `==`; if Phorj wants `===`, both backends must compare `Rc` pointer identity
  (`Rc::ptr_eq`) identically. (b) **Cyclic `==`**: PHP's `==` on a cycle is protected (it tracks
  visited pairs / has recursion guards); Phorj's `eq_val` recurses without a guard → stack overflow
  on a cyclic graph, and the two backends could overflow at *different* depths (the VM and tree-walker
  have the same `MAX_CALL_DEPTH` but `eq_val` is native Rust recursion, NOT frame-counted) →
  potential non-identical crash, breaking `agree_err`.
- **Enforce:**
  - If `===` is adopted: `Rc::ptr_eq` on the instance cell, identical in both backends; transpile to
    PHP `===`. (Lists/Maps as value types have no stable identity → `===` on a list should be
    `E-IDENTITY-VALUE-TYPE`, matching PHP where `===` on arrays is structural-deep, an awkward case to
    avoid.) [Inferred]
  - **`eq_val` must become cycle-safe** (visited-set of `Rc` pointer pairs, like PHP) the moment
    cycles are constructible — otherwise the first cyclic-graph `==` is a non-deterministic crash.
    This is a **P0 prerequisite** of allowing object-to-object mutation. [Verified: `eq_val` recurses
    unguarded `value.rs:238`; the cycle is Verified-buildable in PHP `mut_test5.php`.]

### 2.11 Cycles → the actual reason a tracing GC is needed

- **Verified buildable:** `mut_test5.php` `cycle_built: yes` — `$x->next = $y; $y->next = $x;` makes a
  cycle. **Only object-to-object mutation can build a cycle** — value-typed Lists/Maps/Sets cannot
  (assigning a list copies it; it can never refer back to a container that holds it). So the cycle
  surface is *exactly* shared-mutable instances with instance-typed (or optional-instance-typed)
  fields.
- **Risk:** `Rc`/`Drop` **leaks cycles** (CLAUDE.md states this is precisely why GC is deferred to the
  mutation milestone). A leak is not a byte-identity break per se (output is unaffected), but:
  (a) an OOM-abort under a cycle-heavy workload would be a non-clean crash (violates EV-7 / no-crash);
  (b) any *finalizer/destructor* ordering (if ever added) would diverge. PHP runs a **cycle-collecting
  GC** (`gc_collect_cycles`), but its *observable* effect is essentially nil for byte-identical stdout
  unless destructors are observable.
- **Enforce:** the tracing/cycle GC is required for **memory correctness**, not stdout identity — so
  it can be a `Rc`-with-cycle-collector (e.g. trial-deletion à la PHP/CPython, or a simple
  mark-sweep over the instance arena) that runs *between* observable operations and **must not** emit
  anything. **Phorj has no destructors** (and the matrix should keep it that way for determinism) →
  the GC is observationally invisible, so it cannot affect the spine as long as it never reorders or
  emits output. Build the GC scoped to the **instance arena only** (value types are still `Rc`/`Drop`
  acyclic). [Verified cycle buildability; Inferred GC scoping.]

### 2.12 Compound assignment, `++`/`--`, `??=`, indexed assignment

- **PHP:** `$a[i] = x`, `$a += b`, `$n++`, `$x ??= y` — all reassign/mutate. `$a[]=x` appends.
- **Risk:** each is a *lowering* decision, and each lowering must be identical in interpreter + VM +
  transpiler. `++` on a string is a PHP quirk (`"a"++ == "b"`) the matrix correctly says **not to
  replicate** (line 143/220).
- **Enforce:** these are the *consumers* of the mutation primitive, all gated on it (matrix lines
  141-143, 217-220). Each lowers to the single mutation kernel: `a[i]=x` → `list_set`/`map_set`
  (`make_mut`); `a += b` → `a = a + b` (reassign, scalar — trivial); `n++` → `n = n + 1`;
  `x ??= y` → `x = x ?? y`. Indexed-assignment (`a[i]=x`) needs **one new Op** (`Op::SetIndex`,
  paying the three-match tax) or can reuse a `CallNative`-style mutating kernel; compound/`++`/`??=`
  need **zero new Ops** if reassignment is a `SetLocal` to the resolved slot. Reject PHP string-`++`.
  [Verified PHP quirk; Inferred lowerings.]

---

## 3. Features that CANNOT be made byte-identical → must be restricted

| Feature | Why it can't be three-way identical | Restriction | Preserved capability |
|---|---|---|---|
| **`&` references / aliasing** (`$b = &$a` on non-objects) | Per-name aliasing of a *value* type has no `make_mut` encoding; would need true ref-cells, breaking the value model and the GC scoping | **REJECT** (already matrix line 165/229) | Object handles give shared mutation where PHP would; `clone`-with for functional update |
| **`foreach` by reference** (`foreach($a as &$v)`) | Same value-aliasing as `&`; also a notorious footgun (dangling `$v`) | **REJECT** | Index-mutating loop `a[i]=f(a[i])` (after indexed-assign) or `Core.List.map` (shipped, no mutation) |
| **`static`/`global` mutable locals** | Inherently non-pure persistent state; needs GC rooting | **DEFER** to mutation+GC milestone (matrix line 450); when built, share one program-level slab across both backends | Module/class constants (immutable, already roadmapped); pass-through-and-return |
| **Mutable default arg evaluated once** (Python footgun) | Evaluate-once + mutable ⇒ cross-call aliasing PHP has no concept of → unmatchable | **FORBID evaluate-once for mutable defaults**; defaults are **per-call** (PHP rule) | Per-call fresh default (PHP-identical) |
| **`__clone` / `__get`/`__set`/`__destruct` magic** | Reflective/dynamic interception; `__destruct` ordering is GC-observable | **REJECT** (matrix lines 174/176); **no destructors** | `clone with` (explicit, deterministic); explicit accessors |
| **PHP string `++`** (`"a"++ == "b"`) | A type-coercion quirk; no sound typed equivalent | **REJECT the sub-behavior** (matrix line 143) | Numeric `++` only |
| **`WeakMap` / weak refs** | Needs weak refs + GC + mutation; observably tied to collection timing | **REJECT/DEFER** (matrix line 183) | Strong Map; explicit removal |
| **`===` on value types (List/Map/Set)** | PHP `===` on arrays is deep-structural, not handle-identity — confusing dual of `==` | **`E-IDENTITY-VALUE-TYPE`**: `===` only on instances (`Rc::ptr_eq`) | `==` structural equality (shipped) |

---

## 4. The minimal forced-decision set (what the milestone MUST get right)

These are *forced* by the verified PHP semantics + the existing representation — not open forks:

1. **Split the value model:** `List`/`Map`/`Set` = value (COW via `Rc::make_mut`); `Instance` =
   handle (shared-mutable, `Rc<RefCell<_>>`-equivalent). Forced by §1.
2. **One mutation kernel** in `value.rs` (`list_set`/`list_push`/`map_set`/`set_field`), called by
   both backends — never hand-inlined. Forced by INVARIANT #3 + the parity spine.
3. **`eq_val` becomes cycle-safe** before any object-to-object mutation ships (visited `Rc::ptr_eq`
   set). Forced by §2.10 — else first cyclic `==` is a divergent crash. **P0 prerequisite.**
4. **GC scoped to the instance arena only**, observationally invisible, **no destructors**. Forced by
   §2.11 — value types stay `Rc`/`Drop`; cycles only exist among instances.
5. **Loop keeps eval-once-materialize-then-iterate.** Forced by §2.4 — it already gives PHP's
   "iterate a copy" semantics; don't regress it.
6. **Defaults are per-call.** Forced by §2.5 + the no-footgun philosophy.
7. **Reject `&`/`foreach &`/string-`++`/`__clone`/value-type `===`.** Forced by §3.
8. **Every mutation primitive ships with a differential case that mutates-then-observes** through a
   *second* binding (the aliasing oracle) — the only test shape that catches a value/handle slip.
   (Mirrors the [null-op scratch-slot] lesson: a single-binding test is vacuously green.)

---

## 5. Differential-harness additions the milestone needs

The current `agree`/`agree_err` oracle is **necessary but not sufficient** for mutation, because a
value/handle slip can produce *identical single-run output on both Rust backends* while diverging
from PHP (e.g. if BOTH Rust backends naively alias a List, they agree with each other but not PHP).
The PHP oracle (`PHORJ_REQUIRE_PHP=1`, M7) is what catches that — so **every mutation example must
be in the `examples/**/*.phg` glob** (auto-PHP-gated) and must:

- assign a collection to a second binding, mutate via one, **print both** (catches §2.2/§2.3);
- mutate the iterated collection inside a loop, **print the loop trace and final** (catches §2.4);
- mutate an instance through an alias and through a closure capture, **print via the other name**
  (catches §2.2/§2.6);
- `clone`/`clone with` an instance holding both an object field and a list field, mutate the copy,
  **print both** (catches §2.9 shallow/deep);
- build a cycle and run to completion (catches §2.11 — must not OOM-crash; needs the arena GC).

A mutation feature whose example does not include a **two-binding observe-after-mutate** is, by the
§4.8 rule, not done.

---

## 6. Sequencing recommendation (within the milestone)

[Speculative — design judgment, not a forced order]

1. **Reassignment of `mutable` scalars** (no sharing) — pure `SetLocal`-to-resolved-slot; zero
   value-model change; unblocks `+=`/`++`/`??=` for scalars immediately.
2. **`Rc::make_mut` value-COW for List/Map/Set + indexed-assignment** (`a[i]=x`, `a.push`) — one
   mutation kernel; one possible new `Op::SetIndex`; no GC needed (value types stay acyclic).
3. **Shared-mutable instances** (`Rc<RefCell<Instance>>`) + cycle-safe `eq_val` + the arena GC — the
   heavy lift; this is where cycles and the GC actually arrive.
4. **`clone`/`clone with`, optional `===`** — fall out of 3.
5. **`while`/`do-while`/C-for, while-let, static locals** — the loop/state consumers, once a
   condition can be mutated.

Steps 1-2 deliver most of the *ergonomic* PHP-parity wins (`+=`, `++`, indexed assign) **without a
GC at all**, because value types never cycle — so the GC can be deferred to step 3 even within the
milestone. [Inferred: cycles are instance-only per §2.11.]

---

## Sources

- [PHP Internals Book — Memory management (zval refcounting, COW separation)](https://www.phpinternalsbook.com/php5/zvals/memory_management.html)
- [PHP Internals Book — References (objects are handles/IDs)](https://www.phpinternalsbook.com/php7/zvals/references.html)
- [Zend — Copy on Write](https://www.zend.com/resources/php-extensions/copy-on-write)
- [PHP Manual — References Explained](https://www.php.net/manual/en/language.references.php)
- [Understanding Swift Copy-on-Write mechanisms](https://medium.com/@lucianoalmeida1/understanding-swift-copy-on-write-mechanisms-52ac31d68f2f)
- [Reference and Value Types in Swift (Capital One)](https://medium.com/capital-one-developers/reference-and-value-types-in-swift-de792db330b2)
- Real-PHP verification: PHP 8.6.0-dev (`/stack/tools/phpbrew/php/php-master/bin/php`), all programs run with `php -n` (M7 no-ini oracle policy). Programs: `mut_test{,2,3,4,5}.php` (outputs pasted inline).
- Repo ground truth: `src/value.rs`, `src/ast.rs`, `src/vm.rs`, `src/chunk.rs`, `src/interpreter.rs`, `docs/INVARIANTS.md`, `docs/specs/2026-06-21-php-parity-and-beyond.md`, `docs/plans/2026-06-21-ga-direction-and-autonomy.plan.md`.
