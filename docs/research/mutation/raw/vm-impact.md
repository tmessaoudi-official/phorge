# TRACK 5 — VM / Bytecode / Backend impact of the Mutation + GC milestone

Research basis: read of `src/value.rs`, `src/ast.rs`, `src/chunk.rs`, `src/vm.rs`, `src/compiler.rs`,
`src/interpreter.rs`, `src/transpile.rs`, `docs/INVARIANTS.md`, the parity spec
(`docs/specs/2026-06-21-php-parity-and-beyond.md`) and the GA plan
(`docs/plans/2026-06-21-ga-direction-and-autonomy.plan.md`). External: Rust std `Rc` docs,
Crafting Interpreters, CPython/Swift/Lua GC literature. Every claim graded inline.

Governing filter: craftsmanship-apex; PHP is the floor; transpile contract Phorj:PHP :: TS:JS;
`run ≡ runvm ≡ real PHP` byte-identical is the spine; additive power coexists.

---

## 0. The headline finding (read this first)

**Mutation is far smaller in the VM/bytecode than the name "mutation + GC" implies, and the GC half
is almost entirely *avoidable* for the locked design.** Three reasons, all Verified:

1. **`Op::SetLocal` already exists and is fully wired** through all three coupled matches
   (`exec_op` src/vm.rs:199-204; `stack_effect` src/compiler.rs:675 `-1`; `validate` no-index arm
   src/chunk.rs:379). It is used today only for in-place arithmetic on a local already in its slot
   (see the `locals_get_and_set` VM test, vm.rs:796). Local *reassignment* (`x = e;`) needs **no new
   Op** — it reuses `SetLocal`. *[Verified: read all three matches + the test.]*

2. **The heap representation already cleanly separates "the binding" from "the heap object."** A
   local is a `Value` slot on the VM operand stack (vm.rs `Frame.slot_base + slot`) / a `HashMap`
   entry in the interpreter (`CallScopes`, interpreter.rs:55-79). The compound objects
   (`Instance`/`List`/`Map`/`Set`) are `Rc<T>` *behind* that slot. Reassigning a binding to a new
   value never touches the heap object; it overwrites a `Value`. *[Verified: value.rs:26-43,
   vm.rs:193-204, interpreter.rs:71-78.]*

3. **The locked design — "immutable-by-default + explicit `mutable`" (ACCEPTED)** — means the *vast
   majority* of values stay immutable+acyclic, so `Rc`/`Drop` keeps reclaiming them fully. Only
   `mutable` fields/locals can create cycles, and only `mutable` reference-typed fields at that.
   This shrinks the GC obligation from "the whole heap" to "the cyclic subset reachable from mutable
   reference fields." *[Inferred from GA plan lines 21-37, 97; value.rs module doc lines 1-6.]*

The real cost is in the **place-expression / lvalue compiler** (compiling assignment *targets*) and
in the **aliasing semantics decision** (PHP value-vs-reference copy semantics), NOT in the opcode
count. The opcode count is small and bounded.

---

## 1. What the language gains (the dependent-feature surface)

From the parity spec, the mutation+GC milestone unblocks one tightly-coupled cluster (all currently
`defer` with reason "blocked on mutation"):

- **Local reassignment** `x = e;` (no `Stmt::Assign` exists today — ast.rs `Stmt` is
  `VarDecl/Return/If/For/Block/Expr` only, confirmed ast.rs:511-546). *[Verified.]*
- **Compound assigns** `+= -= *= /= %=` and the bitwise family `&= |= ^= <<= >>=` (spec lines 142,
  217, 219). *[Verified.]*
- **`++`/`--`** increment/decrement (spec 143, 220) — reject the PHP string-increment sub-behavior.
- **`??=`** null-coalesce-assign (spec 141, 218).
- **`while` / `do-while` / classic C-`for`** (spec 131-133) — a condition loop needs a way to
  *advance* the condition, i.e. mutation.
- **Static (mutable) class properties + `global`** (spec 127, 450) — shared mutable state.
- **Field/element set** `o.f = e`, `xs[i] = e`, `m[k] = e` — implied by all of the above (the lvalue
  forms). The parity matrix folds these under compound assigns.
- **`clone` / clone-with** (spec 139, 221) — only meaningful with mutation.
- **Property set-hooks** (spec 120, 329) — get-hooks are immutability-OK and can ship earlier.
- **`while-let`** (spec 365, 348) and **stateful iterator protocol** (spec 161) — need a mutating
  source.
- **`unset`, `WeakMap`** (spec 183, 450) — `WeakMap` is `reject`; `unset` is mutation-milestone.

So the milestone is *one* runtime capability (mutable storage + reclamation) that pays out across
~8 syntactic features. *[Verified: spec grep on lines above.]*

---

## 2. The new Op set — enumerated, with stack effect + validate rule

Each Op must extend the **three** exhaustive matches in the same commit (INVARIANTS §5):
`vm::Vm::exec_op` (vm.rs), `Compiler::stack_effect` (compiler.rs:669), `BytecodeProgram::validate`
(chunk.rs:305). All three are already `_`-wildcard-free, so a missing arm is a compile error.

### Op summary table

| New Op | Stack effect | validate arm | Notes |
|---|---|---|---|
| `SetLocal(slot)` | **already exists** | no-index (exists) | reuse for `x = e;`; **0 new Ops** |
| `SetField(name_idx)` | `-2` (pop instance, pop value) → push nothing | `name_idx < names.len()` (join the existing `GetField`/`CallMethod` arm, chunk.rs:342) | mirror of `GetField` |
| `SetIndex` | `-3` (pop container, index, value) → nothing | no-index arm (like `Index`) | polymorphic: List int-slot / Map key-insert-or-update |
| `GetGlobal(idx)` / `SetGlobal(idx)` | `+1` / `-1` | new global-table bound | only if static/global mutable state is in scope this milestone |
| `IncLocal(slot)` / `DecLocal(slot)` | `0` (in-place) | no-index arm | **OPTIONAL** perf op; `++x` can lower to `GetLocal;Const(1);AddI;SetLocal` with zero new Ops |
| `Dup` | `+1` | no-index arm | **likely needed**: compound-assign on a field/index target wants to evaluate the target expr once and reuse it (`o.f += e` must not double-evaluate `o`) |
| (loop ops) | — | — | **NONE needed** — see §2.3 |

**Minimum viable new-Op count: 2** (`SetField`, `SetIndex`) **+ probably `Dup`** = **3**. Everything
else (reassignment, `++`, compound assign on a local, while/for loops) reuses existing ops.
*[Inferred from the existing Op set in chunk.rs:66-186 + stack_effect coverage compiler.rs:669-708.]*

### 2.1 `SetField(name_idx)` — the field-set op

```
exec_op:  let v = self.pop();             // RHS value
          match self.pop() {              // the receiver instance
            Value::Instance(inst) => { /* see §3 aliasing — make_mut then insert */ }
            v => Err(format!("cannot set `.{name}` on {}", v.type_name())),
          }
stack_effect:  Op::SetField(_) => -2,     // pops instance + value, pushes nothing
validate:      join `Op::GetField(idx) | Op::CallMethod(idx, _)` arm → add `| Op::SetField(idx)`
               (the name index is bounded by `names.len()`, chunk.rs:342)
```

The fault parity rule: a write to a field absent from the class descriptor must fault the same body
on both backends (the dual of `GetField`'s `no field \`{name}\` on \`{class}\``, vm.rs:379). With
typed immutable fields, the checker forbids the bad write so the runtime fault is a defensive
backstop (EV-7). *[Inferred from GetField mirror, vm.rs:372-383.]*

### 2.2 `SetIndex` — polymorphic element-set

`Op::Index` (vm.rs:245-265) is already runtime-polymorphic (List/Map). `SetIndex` is its dual:

```
exec_op:  let v = self.pop(); let index = self.pop();
          match self.pop() {
            Value::List(xs) => { bounds-check int index, write via Rc::make_mut (§3) }
            Value::Map(m)   => { build_map-style insert-or-update via make_mut + a shared kernel }
            v => Err(format!("cannot index-assign {}", v.type_name())),
          }
stack_effect:  Op::SetIndex => -3,
validate:      no-index arm (carries nothing static, like `Index`).
```

**CRITICAL parity hazard (P0-class):** List OOB-write and Map key-insert semantics must be
single-sourced. Today `map_index`/`build_map` live once in value.rs (INVARIANTS §3). A
`map_set(&mut Vec<(HKey,Value)>, key, val)` and a `list_set` kernel MUST live in value.rs too and be
called by *both* backends — re-inlining the insert-or-update logic in vm.rs and interpreter.rs is
exactly the `Op::Neg` drift class that INVARIANTS §3 forbids. *[Verified: INVARIANTS §3 + the
existing build_map dedup discipline, value.rs:121-164.]*

PHP target: `$xs[$i] = $v;` / `$m[$k] = $v;` — idiomatic, byte-identical (PHP arrays are
insertion-ordered, which is precisely why `Value::Map` is a `Vec` of pairs not a `HashMap`, value.rs
comment lines 27-32). *[Verified.]*

### 2.3 Loop ops — NONE needed

`while (c) { body }` and classic `for (init; cond; step)` compile entirely from existing ops:
`Jump`/`JumpIfFalse` (chunk.rs:97-100) + `SetLocal` for the step. The compiler already emits a loop
in `compile_for` (it lowers `for x in range` over a materialized list). A `while` is a backward
`Jump` to a `JumpIfFalse` guard — the same primitives. **0 new loop ops.** *[Verified: Jump/
JumpIfFalse exist + are exhaustive in all three matches; the back-edge is a compiler emission, not a
new instruction. Crafting Interpreters confirms while/for need no dedicated opcode in a
jump-based VM — benhoyt.com/goawk + craftinginterpreters.com.]*

One subtlety: a backward `Jump` target must pass `validate`'s `target > code_len` check
(chunk.rs:350) — backward targets are always `< code_len`, so they pass trivially. *[Verified:
chunk.rs:350 only rejects `> code_len`.]*

### 2.4 `Dup` — needed for compound-assign-on-complex-target

`o.f += e` must evaluate `o` **once**, read `o.f`, add `e`, write back to the *same* `o`. Without a
`Dup`, the compiler would emit `o` twice (double side effects if `o` is itself a call). The clean
lowering:

```
<eval o>          ; stack: [o]
Dup               ; stack: [o, o]
GetField(f)       ; stack: [o, o.f]
<eval e>          ; stack: [o, o.f, e]
AddI/AddF         ; stack: [o, sum]
SetField(f)       ; stack: []   (SetField pops value then instance)
```

`Dup` (+1, no-index arm) is the standard CLox/CPython primitive for exactly this. For a *local*
compound-assign (`x += e`) no `Dup` is needed (`GetLocal;…;SetLocal`). *[Inferred: standard
stack-VM practice, Crafting Interpreters; confirmed against the slot/stack model in vm.rs.]*

---

## 3. The representation decision: `Rc::make_mut` vs `RefCell` — THE key fork

Today every heap value is `Rc<T>` with an **immutable interior, no RefCell** (value.rs:26-43,
module doc lines 1-6). The `GetLocal` hot path is a refcount bump (`self.stack[idx].clone()`,
vm.rs:196), which is the whole point of M2 P5a (the 2.4× win). Any mutation strategy must NOT
regress that. *[Verified: value.rs module doc + vm.rs:193-198.]*

### Option A — `Rc::make_mut` (clone-on-write). **RECOMMENDED.**

Rust std `Rc::make_mut(&mut Rc<T>) -> &mut T`: **O(1) if `strong_count == 1`** (mutate in place);
**clones the inner value to a fresh allocation if `strong_count > 1`** (so other holders keep their
old value). *[Verified: doc.rust-lang.org/std/rc/struct.Rc.html — quoted: "If there are other Rc
pointers to the same allocation, then make_mut will clone the inner value… clone-on-write."]*

- **GetLocal hot path: UNCHANGED.** Reads still `clone()` the `Rc` (refcount bump). `make_mut` is
  only called on the *write* path (`SetField`/`SetIndex`). *[Verified: make_mut is a distinct call;
  reads never invoke it.]*
- **Semantics: this is exactly PHP's value-copy semantics for arrays** (`$b = $a; $b[0]=1;` does not
  touch `$a` — copy-on-write). So `make_mut` on `Value::List`/`Value::Map` gives **byte-identical
  PHP array semantics for free.** *[Verified: PHP array COW is documented behavior; make_mut COW is
  the precise structural match.]*
- **Cost: a write to a list/map shared by N holders costs one clone of that container.** For
  immutable-by-default Phorj this is rare (mutation is opt-in), and it is the *same* cost PHP pays.
  *[Inferred.]*
- **GC: `make_mut` cannot create a cycle for value types** (List/Map/Set hold values, and writing a
  value into them via COW doesn't alias back). Cycles are only possible if a `mutable` field of an
  `Instance` can point to an `Instance` that (transitively) points back. *[Inferred.]*

**The aliasing mismatch to resolve (P0 design decision):** PHP **objects** are handle/reference
semantics (`$b = $a;` where `$a` is an object → both see mutations), but PHP **arrays** are value/COW
semantics. `make_mut` gives COW for *everything*. So:
- For `Value::List`/`Map`/`Set` → `make_mut` COW **matches PHP exactly.** ✅
- For `Value::Instance` → PHP is reference-semantics; `make_mut` COW would **diverge** (a mutation
  through one binding wouldn't be seen through an alias). ❌ For instances we'd need shared
  mutability (Option B for instances only), OR forbid instance-aliasing-then-mutation at the type
  level (the immutable-by-default model already does most of this).

This split is the genuine fork — see §7.

### Option B — `Rc<RefCell<T>>` (interior mutability)

- **Matches PHP object reference-semantics exactly** (all aliases see the mutation). ✅ for
  instances.
- **Cost: a runtime borrow-check on every access** + the `RefCell` is `+8 bytes` and a non-`Copy`
  borrow guard. The `GetLocal` read path would either still clone the `Rc<RefCell>` (cheap, but now
  every *field read* `GetField` must `.borrow()`) — a measurable per-access cost the immutable model
  doesn't pay. *[Inferred: Rust book ch15-05 — "RefCell incurs a slight runtime overhead due to its
  dynamic checks."]*
- **Cycle risk: REAL.** `Rc<RefCell<Instance>>` with a mutable self-referential field is the classic
  Rust reference cycle → `Drop` never fires → leak. **This is what forces a tracing GC.** *[Verified:
  Rust book ch15-06 reference cycles; the value.rs module doc explicitly anticipates this: "no cycle
  can leak… deferred to M3, when mutation could create cycles."]*
- **`#![forbid(unsafe_code)]` (INVARIANTS §10) stays satisfiable** — `RefCell` is safe. ✅

### Recommendation (Speculative — this is a design judgment for the developer)

**Hybrid, scoped by the immutable-by-default model:**
- Keep `List`/`Map`/`Set` as `Rc<T>` + **`make_mut` COW** → matches PHP arrays, no GC, no hot-path
  regression. *[Verified mechanism.]*
- For mutable `Instance` fields, prefer **`make_mut` + a checker rule that an aliased instance is not
  mutable through two paths**, i.e. lean on immutable-by-default so the COW-vs-reference divergence
  is *unobservable* (you can only mutate through a uniquely-owned `mutable` binding). If the
  developer wants true PHP object-reference aliasing-with-mutation, **only then** introduce
  `RefCell` for instances and **only then** does the tracing-GC milestone become mandatory.
  *[Speculative — depends on the §7 fork.]*

This is the craftsmanship-apex call: the COW path is provably correct (no cycles, no GC, no
unsafe), legible, and PHP-array-identical; the `RefCell`+GC path is only paid for the exact feature
that demands it (shared-mutable objects), never globally.

---

## 4. Compiling lvalues / assignment targets (place expressions)

The compiler today only ever *reads* places (`Expr::Ident`/`Member`/`Index` all lower to `Get…`
ops, compiler.rs:1004-1044). Assignment introduces a **place-expression** compilation mode: the same
syntactic forms compiled as *write targets*.

A new `Stmt::Assign { target: Expr, op: Option<BinaryOp>, value: Expr }` (or a dedicated `AssignTarget`
enum) drives a `compile_place_store(target)` that dispatches:

| Target form | Read (exists) | Store (new) |
|---|---|---|
| `x` (local) | `GetLocal(slot)` | `<eval v>; SetLocal(slot)` |
| `x` (`this` field, bare) | `GetLocal(this); GetField(idx)` | `GetLocal(this); <eval v>; SetField(idx)` |
| `o.f` | `<eval o>; GetField(idx)` | `<eval o>; <eval v>; SetField(idx)` |
| `xs[i]` / `m[k]` | `<eval o>; <eval i>; Index` | `<eval o>; <eval i>; <eval v>; SetIndex` |
| `a.b[i].c` (nested) | chained `GetField`/`Index` | walk to the penultimate place, then one terminal `SetField`/`SetIndex` |

The nested case (`a.b[i].c = v`) is the subtle one: with **COW (`make_mut`)** semantics, writing `c`
must propagate up — mutating the innermost object then re-storing each enclosing container that got
cloned. The clean implementation is a **read-modify-write chain** where each level uses `make_mut`,
which for a uniquely-owned chain is all-O(1) (no clones). This is exactly how Rust/Clojure persistent
structures lower nested updates. *[Inferred: make_mut COW chain semantics; standard functional-update
lowering.]*

**The checker must classify a place as assignable** (it must be a `mutable` local/field, not a
`const`/immutable one) — a new `E-ASSIGN-IMMUTABLE` / `E-NOT-ASSIGNABLE` diagnostic. This is a
front-end gate; both backends consume an already-validated assignment. *[Inferred from the
modifier-model decision, GA plan 28-37.]*

---

## 5. Interpreter mirror (parity is non-negotiable, INVARIANTS §1, §2)

The tree-walker is the reference oracle. Each new Op needs a structurally identical interpreter path:

- **Local reassign** → `CallScopes` needs an `assign(name, v)` that finds the *innermost scope
  declaring `name`* and overwrites it (`scopes.iter_mut().rev().find_map(...)`). Today `declare`
  always inserts into the last scope (interpreter.rs:71-76) and `lookup` returns `&Value`
  (interpreter.rs:77) — there is no mutate path yet. Adding `assign` is ~6 lines. *[Verified:
  interpreter.rs:55-79 has no setter.]*
- **`SetField`/`SetIndex`** → the interpreter must use the **same value.rs kernels** (`map_set`,
  `list_set`) and the **same `Rc::make_mut` COW discipline** the VM uses, or the two diverge on the
  aliasing-observability boundary. This is the single most parity-fragile part of the milestone:
  the moment one backend mutates-in-place and the other clones, `agree(src)` breaks on a program
  that reads an alias after a write. *[Verified: INVARIANTS §1-3 + the value.rs single-source rule.]*
- **`while`/`for`** → the interpreter loops natively (it already does for `for..in`); a `while` is a
  `loop { eval cond; if false break; eval body }`. No Op, just a `Stmt::While` arm. *[Inferred.]*

**Mandatory new harness discipline:** the differential harness (`tests/differential.rs`) compares
**Ok output** (`agree`) and **fault kind** (`agree_err`). Neither catches *aliasing observability*
unless a test program explicitly **writes through one binding and reads through an alias**. New
required differential cases (see §6).

---

## 6. Transpiler — emit idiomatic PHP mutation

The transpiler already emits `$x = expr;` for `VarDecl` (transpile.rs:639) and `foreach`/`if`
(transpile.rs:648-697). Mutation maps **1:1 to PHP**, which is the whole transpile-contract bet:

| Phorj | PHP emission | Notes |
|---|---|---|
| `x = e;` | `$x = e;` | reuses the existing `$name = …` path; just stop emitting a *fresh* `declare` |
| `x += e;` | `$x += e;` | direct; but `/`-family must route through `__phorj_div`/`__phorj_rem` runtime helpers (transpile.rs:264-274) → `$x = __phorj_div($x, e);` to preserve intdiv/fmod parity |
| `o.f = e;` | `$o->f = e;` | needs the field `public` (the W1 gotcha: PHP enforces `private`, Phorj backends don't — already in KNOWN_ISSUES) |
| `xs[i] = e;` | `$xs[$i] = e;` | PHP array COW = `make_mut` COW → byte-identical |
| `m[k] = e;` | `$m[$k] = e;` | insertion-order preserved both sides |
| `x++;` | `$x++;` | but reject PHP string-increment — checker restricts `++` to numeric (spec 143) |
| `while (c) {}` | `while (c) {}` | direct |
| `??=` | `$x ??= e;` (PHP 7.4+) | direct |

**Transpiler parity traps (from the existing KNOWN_ISSUES discipline):**
1. **Compound `/=` and `%=`** can't emit naked `$x /= e` — they must go through `__phorj_div`/
   `__phorj_rem` or PHP float-division diverges from Phorj intdiv. *[Verified: transpile.rs:264-274
   shows the existing div/rem helpers + the M7 memory "transpile correctness uses RUNTIME HELPERS."]*
2. **Float compound-assign display** still routes through `__phorj_str`/`__phorj_float`
   (transpile.rs:283-310) only at *print* time, not assign time — assign is pure value, so no trap
   there. *[Verified.]*
3. **`-n` ini extensions**: any new compound-assign helper must be tier-1 PHP only (no mbstring) —
   the oracle runs `php -n` (memory "transpile-no-ini-extensions"). Arithmetic compound-assigns are
   all core, so safe. *[Verified via memory + transpile.rs uses only core fns.]*

---

## 7. Parity-risk surface + how to test it

### Risk ranking

| Risk | Severity | Why |
|---|---|---|
| Aliasing observability (`make_mut` COW vs `RefCell` reference) diverging across backends | **P0** | The one thing `agree`/`agree_err` won't catch unless a test writes-then-reads-an-alias; and PHP array(COW) vs object(ref) semantics differ, so the *correct* answer differs by type |
| `map_set`/`list_set` kernels re-inlined per backend | **P0** | Re-opens the `Op::Neg` drift class (INVARIANTS §3) |
| Tracing GC (only if `RefCell` instances chosen) — incremental mark-sweep over the *cyclic mutable subset* | **P1** | Must be deterministic (INVARIANTS §8): GC timing must NOT affect observable output. Finalizer ordering is the classic non-determinism leak. CPython/Lua both isolate cycle collection to container types only — same applies here (only mutable instance fields) |
| Nested place-store (`a.b[i].c = v`) propagating COW up the chain | **P1** | Easy to mutate a clone and drop it on the floor |
| `++`/`--` on a `T?` or non-numeric | **P2** | Checker must gate; reject PHP string-increment |
| `while` infinite loop → must hit `MAX_CALL_DEPTH`-style guard, not OOM/hang | **P2** | A `while(true){}` is not recursion, so `MAX_CALL_DEPTH` won't catch it; need a step/iteration budget or accept hangs as user error (PHP hangs too). EV-7 says no crash, but a hang isn't a crash |

### Differential harness extensions (concrete, required before any mutation lands)

`tests/differential.rs` (`agree` Ok-output + `agree_err` FaultKind, lines 50-124) needs new cases:

1. **Reassignment value test:** `var x = 1; x = 2; Console.println(x);` → `agree` "2".
2. **Compound-assign + intdiv parity:** `var x = 7; x /= 2; println(x);` → `agree` "3" AND the PHP
   oracle must agree (routes through `__phorj_div`). The PHP-oracle glob already gates `examples/`
   (differential.rs project-aware glob) — ship `examples/guide/mutation.phg`.
3. **THE aliasing case (the P0 catcher):** for List/Map (COW), `var a = [1,2]; var b = a; b[0] = 9;
   println(a[0]); println(b[0]);` must produce identical output on run/runvm/PHP — and that output
   *defines* the chosen semantics (COW → "1" then "9"). This is the test that catches a backend
   mutating-in-place when the other clones. **Add TWO of them** (one List, one Map) per the
   "null-op scratch slot" memory lesson (a single case can hide a slot/aliasing bug).
4. **Nested store:** `m["k"][0] = 5;` (Map-of-List) round-tripped — exercises the COW-up-the-chain.
5. **`while` loop:** a counted `while` whose body mutates the counter → `agree`.
6. **Fault parity:** `SetIndex` OOB write on a List → identical `FaultKind` both backends (extend the
   `FaultKind` enum, differential.rs:64, with e.g. `IndexOob` already covers reads — reuse it).
7. **GC determinism (only if RefCell path):** a program building then dropping a cycle must produce
   identical output regardless of *when* the collector runs — assert output is collector-independent
   (run with collection forced at every allocation vs never).

`phg bench` (INVARIANTS §11) must show **no regression on the immutable hot path** — the
`GetLocal`/`GetField` read path must stay a refcount bump (run an object-heavy bench before/after,
the M2 P5a 634ms workload is the baseline). *[Verified: INVARIANTS §11 + the P5a number in CLAUDE.md.]*

---

## 8. Forced decisions (by the existing invariants / transpile contract)

1. **No new loop opcodes** — `while`/C-`for` MUST lower to existing `Jump`/`JumpIfFalse`+`SetLocal`
   (the Op set is intentionally minimal; adding loop ops would be redundant). *[Forced by the
   existing jump ops being exhaustive and sufficient.]*
2. **`SetField`/`SetIndex` value logic single-sourced in value.rs** — INVARIANTS §3 forbids
   per-backend re-inlining; a `map_set`/`list_set` kernel is mandatory, not optional.
3. **Every new Op extends all three matches in one commit** — INVARIANTS §5, compile-error-enforced.
4. **The PHP `/=`/`%=` emission routes through `__phorj_div`/`__phorj_rem`** — forced by the M7
   runtime-helper correctness model (memory) and transpile.rs:264-274; a naked PHP `/=` diverges.
5. **Reassignment reuses `Op::SetLocal`** — the op exists; inventing a parallel op would be waste.
6. **GC, if needed at all, is scoped to the mutable-instance cyclic subset** — forced by
   immutable-by-default (value types stay `Rc`/`Drop`-reclaimable, value.rs module doc + GA plan).
7. **`#![forbid(unsafe_code)]` holds** — both `make_mut` and `RefCell` are safe; no `unsafe` arena.

## 9. Genuine open forks (no single forced answer — for the developer)

- **F1 — Aliasing model for `Instance`:** COW (`make_mut`, no GC, diverges from PHP object-reference
  semantics unless immutability hides it) vs `Rc<RefCell>` (PHP-exact object aliasing, forces a
  tracing GC, per-access borrow cost). Recommendation: COW + immutable-by-default first; add
  RefCell+GC only if/when shared-mutable-objects is an explicit, demanded feature. This is THE
  milestone-shaping fork.
- **F2 — `Dup` op vs re-emit:** add a `Dup` op (clean compound-assign-on-target) vs spill-to-a-scratch-
  local (no new op, but slot bookkeeping like the null-op scratch-slot gotcha). Recommendation: add
  `Dup` — it's the standard primitive and avoids the scratch-slot footgun memory warns about.
- **F3 — `while`-loop runaway:** accept hangs as user error (PHP parity — PHP hangs too) vs an
  iteration budget (EV-7 "no crash" arguably extends to "no hang"). Recommendation: match PHP
  (accept hang) for byte-identity; revisit if it bites.
- **F4 — Static/global mutable state in this milestone or split out:** the spec buckets `static`
  mutable properties + `global` here, but they need a `GetGlobal`/`SetGlobal` + a global table (new
  validate bound). Recommendation: split — ship local/field/index mutation + loops first
  (zero global-state Ops), then static/global as a focused follow-up.
