# Mutation Milestone — Design (LOCKED)

> Status: **Designed — all forks resolved by the developer (2026-06-21); not yet implemented.**
> Research basis: `docs/research/mutation/SYNTHESIS.md` (+ `raw/*.md`), 5-track workflow `wf_e87dd08d-c75`.
> Decisions log: `docs/plans/2026-06-21-ga-direction-and-autonomy.plan.md` → "Mutation milestone — LOCKED".
> Filter: craftsmanship-apex; PHP is the floor; transpile contract `Phorj : PHP :: TS : JS`; the spine is
> `run ≡ runvm ≡ real PHP`, byte-identical (Invariant #1, M7 oracle `PHORJ_REQUIRE_PHP=1`).

## 0. The one-paragraph design

Phorj gains in-place mutation while preserving the byte-identical-with-PHP spine. The heap splits exactly
as PHP's does: **`List`/`Map`/`Set`/`Bytes` are copy-on-write VALUE types** (assignment copies; provably
acyclic; reclaimed by `Rc`/`Drop`; **no GC**), and **`Instance` is a shared-mutable HANDLE** (assignment
shares; mutation visible through every binding; can form cycles → an **instance-only cycle collector**, the
single deferrable final slice). Mutability is opt-in: bindings are immutable by default, `mutable` marks the
exception. ~70% of the user-visible surface (locals, compound-assign, `++`/`--`, `??=`, all loops, value-type
element-set, `clone with`) ships with **zero GC**; only shared-mutable instance fields (`M-mut.6`) cross the
GC boundary.

## 1. Forced decisions (invariants/contract decide them — not open)

| # | Decision | Why forced |
|---|---|---|
| F1 | `List`/`Map`/`Set`/`Bytes` = value semantics, COW via `Rc::make_mut`. | PHP arrays are COW value types (oracle-verified). `make_mut` is the std analog. |
| F2 | `Instance` = handle/reference semantics (shared-mutable). | PHP objects are handles (oracle-verified). Anything else fails `PHORJ_REQUIRE_PHP=1`. |
| F3 | One mutation kernel per op in `value.rs` (`list_set`/`list_push`/`map_set`/`set_field`), called by **both** backends — never hand-inlined. | Invariant #3; re-inlining is the `Op::Neg` drift class. |
| F4 | `eq_val` must become **cycle-safe** (visited `Rc::ptr_eq` set) **before** any object→object mutation ships. | Unguarded recursion on a cycle overflows the native stack at *different* depths per backend → breaks `agree_err`. PHP `==` is cycle-protected. **P0 prerequisite for M-mut.6.** |
| F5 | **No new loop opcodes.** `while`/do-while/C-`for` lower to existing `Jump`/`JumpIfFalse` + `SetLocal`. | Jump ops exist, are exhaustive, backward targets pass `validate`. |
| F6 | Local reassignment reuses `Op::SetLocal` — **zero new Op**. Interpreter gains an `assign(name,v)` that overwrites the binding in the scope `lookup` finds it (not a child shadow). | `Op::SetLocal` already wired through all three matches. |
| F7 | `/=` and `%=` route through `__phorj_div`/`__phorj_rem` helpers, not naked PHP `$x /= e`. | M7 runtime-helper model: naked PHP `/` is float-division, diverges from Phorj intdiv. |
| F8 | Loops keep **eval-once-materialize-then-iterate** (already gives PHP's "foreach iterates a copy"). | Oracle-verified; don't regress to live-buffer iteration. |
| F9 | Defaults are **per-call** (never evaluate-once). | PHP forbids non-const defaults; evaluate-once + mutable = cross-call aliasing PHP can't express. Also kills the Python mutable-default footgun. |
| F10 | Rejected: `&` references, `foreach as &$v`, PHP string-`++`, `__clone`/`__get`/`__set`/`__destruct`, `===` on value types, mutable-evaluate-once defaults. | Aliasing / non-determinism / coercion footguns the parity spec already rejects; capability preserved another way (§5). |
| F11 | GC, if built, is the `Rc`-cycle-collector family scoped to the instance subset — **never** a mark-sweep tracing arena. | Tree-walker has no enumerable root set → tracing needs conservative stack scanning = `unsafe` = Invariant #10 ban. |
| F12 | GC stays observationally invisible — `__destruct`/finalizers stay rejected forever. | Collection timing is non-deterministic; spine-safe only because nothing observable fires on reclamation. |
| F13 | Every mutation primitive ships a **two-binding observe-after-mutate** PHP-gated example. | `agree`/`agree_err` compare only the two Rust backends — both can alias wrongly and still agree. Only the PHP oracle + a 2-binding test catches a value/handle slip. |

## 2. Resolved forks (developer's call, 2026-06-21)

- **Fork 1 = (A) PHP-faithful handles.** Objects shared-mutable; byte-identical with PHP by construction;
  instance-only cycle collector is the deferrable final slice. `clone with` + `inout` offered additively.
- **Fork 2 = (B) `clone with` bypasses the constructor** (PHP 8.5 `clone with` / C# record target). `with`
  is total + fast. Invariant-revalidation deferred to a future `requires`/refinement feature.
- **Fork 3 = (C) defer the collector to per-process + per-request reclaim** (HHVM model). Build a
  trial-deletion `Gc<T>` only if a hard long-lived-cycle requirement appears outside `serve`.
- **Fork 4 = (A) immutable params + `for..in` loop vars** (`mutable` opt-in); loop var scoped to body.

## 3. The modifier model (confirmed — not a fork)

| Axis | Default | Opt-in | Precedent |
|---|---|---|---|
| Mutability | immutable | `mutable` | Kotlin `val`/`var`, Swift `let`/`var`, Rust `let`/`mut` |
| Compile-time const | — (decl form) | `const NAME = <const-expr>` | Kotlin `const val`, C# `const`, Rust `const` |
| Association | instance | `static` | universal |
| Extensibility | closed/final | `open` | Kotlin final-by-default + `open` |

Refinements: `final`/`readonly` **dropped** as value modifiers (immutable-default subsumes `readonly`;
`final`-for-inheritance becomes the default, `open` opts in; the transpiler MAY still *emit* PHP `readonly`
as intent). `mutable` is a **binding** modifier on `VarDecl`/field (`ast::Modifier` already has the slot) —
never a type modifier, so no `mutable T`/`T` pair-explosion across `T?`/`A|B`/`A&B`/`List<T>`/generics, and
**no new `CTy` variant**. `const` and immutable-local are distinct axes (keep both, like Kotlin
`val`/`const val`). `open` enforcement gates on `extends` (S6) — reserve/parse now, wire at S6.

## 4. Slice sequence

```
Tier 1 — local rebinding (no new Op, no GC)
  M-mut.1  mutable locals + reassignment      Stmt::Assign; modifier model; smart-cast invalidation
                                              on reassign (Kotlin/TS rule, S2 interaction); interpreter
                                              assign(); VM resolve_local + SetLocal; transpiler $x = …
                                              E-ASSIGN-IMMUTABLE / E-ASSIGN-TYPE
  M-mut.2  compound-assign + ++/-- + ??=       += -= *= /= %= (NOT .=); ??=; n++/n-- (stmt form);
                                              /= %= via __phorj_div/__phorj_rem (F7)
  M-mut.3  condition loops                     while, do-while, C-for; while-let (if-let sugar);
                                              break/continue generalize from Wave A; jumps only (F5)
  M-mut.4  clone with + get-hooks              p with { f = e } → fresh instance (bypass ctor, F2);
                                              get-hooks = computed/virtual props (method-on-read)
────────  GC BOUNDARY — everything above is GC-free  ────────
Tier 2 — interior mutation
  M-mut.5  value-type element set              xs[i]=e, m[k]=e, list_push — COW via make_mut;
                                              value types acyclic ⇒ STILL no GC. New: SetIndex, Dup.
  M-mut.6  shared-mutable instance fields + collector   o.f = e; eq_val cycle-safe (F4, P0);
                                              instance-subset cycle collector (Fork-3); optional ===
                                              via Rc::ptr_eq. New: SetField.
  M-mut.7  static mutable + set-hooks          program-lifetime mutable state; set-hooks.
                                              New: Get/SetStatic.
```

Each slice ships green + byte-identical (`run ≡ runvm ≡ real PHP`) with a guide example under
`examples/**/*.phg` (auto byte-identity-gated), per the developer's "examples ship with features" rule.

## 5. Capability-preservation map (per philosophy — removing a form preserves the power another way)

| Removed | Preserved via |
|---|---|
| `&` references / `foreach as &$v` | object handles; index-mutating loop `a[i] = f(a[i])`; `Core.List.map` |
| `__clone` / `__get`/`__set` | `clone with` (deterministic); typed property hooks (get-hooks early) |
| PHP string-`++` | numeric `++` only |
| mutable evaluate-once default | per-call fresh default (PHP-identical) |
| `readonly`/`final` modifiers | immutable-by-default; `open` opt-in (transpiler may still emit PHP `readonly`) |

## 6. New Op budget + parity-risk surface

**New Ops (minimal): `SetField` + `SetIndex` + `Dup`** for the core; `Get/SetStatic` only for M-mut.7. Each
extends the **three coupled exhaustive matches** (`vm::exec_op`, `compiler::stack_effect`,
`chunk::BytecodeProgram::validate`) in one commit (Invariant #5, all `_`-wildcard-free).

| Op | Stack effect | `validate` arm | Slice |
|---|---|---|---|
| `SetLocal` | already exists | exists | M-mut.1 (reuse) |
| `SetIndex` | −3 (container, index, value) | no-index arm (like `Index`) | M-mut.5 |
| `Dup` | +1 | no-index arm | M-mut.5 (compound-assign on a target without re-evaluating the receiver) |
| `SetField(name_idx)` | −2 (instance, value) | join the `GetField` name-bound arm | M-mut.6 |
| `Get/SetStatic(idx)` | +1 / −1 | new static-table bound | M-mut.7 |

**Parity risks (ranked):** P0 — aliasing observability (COW vs reference) diverging across backends / vs
PHP; mutation kernels re-inlined per backend; `eq_val` unguarded recursion on a cycle. P1 — nested
place-store `a.b[i].c = v` COW-up-the-chain; collector timing observable. P2 — `++`/`--` on
non-numeric / PHP string-`++`; `while(true){}` runaway (match PHP: hang is user error).

## 7. Differential-harness extensions (`tests/differential.rs`, all PHP-gated)

`agree` (Ok) + `agree_err` (FaultKind) are necessary but **not sufficient** for mutation. New required cases:
reassignment value; compound-assign intdiv parity (`x /= 2`); **two aliasing cases (one List, one Map)** —
the P0 value/handle catcher; object-alias case (mutate via one binding, observe via another → both see it);
nested store (`m["k"][0] = 5`); loop-mutates-iterated-collection (F8 non-regression); closure-capture +
mutate-after (handle capture shares the cell); `clone`/`clone with` shallow/deep; `SetIndex` OOB →
identical `FaultKind` (reuse `IndexOob`); cycle-to-completion (GC slice — must not OOM; output
collector-independent). `phg bench` (Invariant #11): the immutable `GetLocal`/`GetField` read path must
stay a refcount bump — re-run the M2 P5a 634 ms object-heavy workload before/after; the specific thing to
measure is any per-field-read borrow cost introduced by handle semantics.

## 8. Open questions still needing a real-PHP check (resolve during the relevant slice, not blocking design)

1. PHP 8.5 `clone with` + property-hook interaction (does it run a `set` hook or write the backing store?) — M-mut.4.
2. `++`/`--` at `PHP_INT_MAX` (PHP promotes to float; Phorj `int_add` faults) — confirm + document — M-mut.2.
3. Compound `%=` with negative operands (sign-follows-dividend) — four sign combinations — M-mut.2.
4. `static mutable` initializer timing (once-on-first-call; const-ish expr) — M-mut.7.
5. `foreach` over a Map being mutated mid-loop (snapshot semantics) — M-mut.3/M-mut.5.

---

*STATUS: Designed — not implemented. Spine and all four forks locked by the developer 2026-06-21. Next:
writing-plans → M-mut.1.*
