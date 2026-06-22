# Method & Function Overloading Plan (M-RT)

Multiple functions/methods sharing a name with distinct parameter signatures. The M-RT slice inserted
after S5 (intersections); revisits S5's `E-INTERSECT-SIG`.

## Decisions Log

- [2026-06-22] AGREED (developer, after a challenge round): **DYNAMIC multiple dispatch**, NOT static
  (Java/C++) overloading. Dispatch is by the **runtime** type of the arguments, most-specific-wins,
  **identical in interpreter + VM + PHP**. Rationale: (1) static overloading resolves by *declared*
  type — a famous footgun (a supertype-typed variable holding a subtype value calls the general
  overload), and the project philosophy is "removes surprises NEVER capability"; (2) to keep one
  idiomatic PHP method, PHP *must* dispatch at runtime — static-Phorge would then diverge from
  runtime-PHP and **break the byte-identical spine** (the VM calls by static index, PHP has no
  static-type info at runtime); (3) dynamic dispatch is byte-identical *by construction* (all three
  backends dispatch on the runtime tag) and matches what a PHP dev hand-writes (`if (is_int($x)) …`).
  This is the spine-safe realization of the developer's "more powerful / permissive" lean.
- [2026-06-22] AGREED: **same return type required** across an overload set (`E-OVERLOAD-RETURN`).
  Under dynamic dispatch the compiler cannot know which overload fires at a polymorphic call, so a
  same return keeps every overloaded call statically typed (no surprise unions). The legitimate
  "return co-varies with input" case (`abs(int)->int`/`abs(float)->float`) is steered toward
  **generics** (parametric) or accepted later as a union-return relaxation. The anti-pattern
  ("different operations under one name, unrelated returns") is blocked early. Union-of-returns is a
  documented future relaxation.
- [2026-06-22] AGREED: scope = **free functions + class methods** (shared machinery). **Constructors
  deferred** (PHP can't overload constructors; Phorge has promotion + the deferred default-args path).
- [2026-06-22] PINNED: **one new `Op::CallOverload(set_id, argc)`** — extends the three coupled matches
  (`vm.rs` `exec_op`, `chunk.rs` `validate`, `compiler.rs` `stack_effect`) in the commit that adds it.
- [2026-06-22] AGREED: ambiguity policy — primitive overloads (`int`/`string`/`float`/`bool`/`bytes`)
  are pairwise runtime-disjoint → never ambiguous; a single-argument class/interface overload set
  always has a unique most-specific match. **Multi-argument cross-cutting ambiguity** (e.g.
  `f(Animal, Dog)` vs `f(Dog, Animal)` called `f(Dog, Dog)`) → a **clean byte-identical runtime
  fault** (`FaultKind`-classified, like index-OOB). Compile-time ambiguity *detection* (rejecting such
  a set at declaration) is a documented future refinement; an identical-signature pair is rejected now
  (`E-OVERLOAD-DUPLICATE`).

## Architecture

Overload resolution is inherently a **runtime** operation here (dynamic dispatch). The checker validates
and computes the call's result type (the shared return); the backends select the body at runtime.

- **Checker:** `funcs: HashMap<String, Vec<FnSig>>` and class `methods: HashMap<String, Vec<FnSig>>` —
  an *overload set* per name. Collection builds the set (lifting the `overloading not supported` gate),
  enforcing `E-OVERLOAD-RETURN` (same return) + `E-OVERLOAD-DUPLICATE` (identical params). A call
  validates the args are assignable to **at least one** overload (`E-OVERLOAD-NO-MATCH`) and types the
  result as the shared return. Generic overloads are out of scope (reuse existing generic-method gates).
- **Runtime dispatch descriptors:** each overload's parameter types lower to a runtime-checkable
  `ParamKind` (Int/Float/Bool/String/Bytes/List/Map/Set/Class(name)/Any). A dispatch table maps a
  set id → `[(Vec<ParamKind>, target_fn_idx)]`. Most-specific = a class kind beats `Any`; a subclass
  beats a superclass (via the shared `class_implements`/extends oracle); primitives are leaves.
- **VM:** `Op::CallOverload(set_id, argc)` reads the top `argc` values' runtime kinds, selects the
  unique most-specific overload from the table, and calls its `target_fn_idx` (reusing the normal call
  path). No match → the no-match fault; tie → the ambiguity fault.
- **Interpreter:** the same selection over `Value` kinds, then the normal call.
- **Transpiler:** emit each overload body as a distinct valid-PHP-named function/method
  (`f__ovl_0` …) and synthesize **one** PHP `f(...$args)` (or fixed-arity) dispatcher with an
  `is_int`/`is_string`/`instanceof` if-chain (most-specific-first) calling the right body; call sites
  emit the bare original name. Single-overload names are unchanged → byte-identical to today's output.

## Tasks (each a green, byte-identical commit)

- **T1 — front-end: overload sets + checker** (no backend dispatch; no new Op). Representation change
  (`Vec<FnSig>`), collection + `E-OVERLOAD-RETURN`/`E-OVERLOAD-DUPLICATE`, call validation + result
  typing. Green because no multi-overload program exists in the suite yet (single-overload sets behave
  exactly as today). Codes self-document via `phg explain`.
- **T2 — backend dispatch core: `Op::CallOverload`** + VM + interpreter runtime most-specific dispatch +
  compiler emission + the dispatch table in `BytecodeProgram`. Checkpoint: a multi-overload program
  runs **byte-identical `run ≡ runvm`**. Differential cases incl. primitive overloads, single-arg class
  overloads (subtype), and the multi-arg ambiguity fault (`agree_err`).
- **T3 — transpiler**: distinct-named bodies + synthesized PHP dispatcher; call sites bare. Full 3-way
  oracle incl. real PHP 8.4.
- **T4 — example + docs**: `examples/guide/overloading.phg` (free fns + methods; primitive + class
  overloads), README row, CHANGELOG, KNOWN_ISSUES (deferrals: constructors, union-return, compile-time
  ambiguity detection, generic overloads), plan + memory; revisit S5 `E-INTERSECT-SIG` note.

## Acceptance
Byte-identical `run ≡ runvm ≡ real PHP 8.4` for `examples/guide/overloading.phg`; full suite + clippy +
fmt green on the PHP-8.4 floor; exactly one new `Op`; `phg explain` documents every new code.

## Rollback
Each task is an isolated commit; revert the offending commit. The representation change (T1) is the only
broad refactor — if it destabilizes, `git revert` restores the single-`FnSig` map.
