# M-RT — Rich Types Milestone Plan

> TypeScript-grade type system for Phorge, mapped to PHP 8.0/8.1 natives. Built slice by slice,
> each an independent green commit with a byte-identical (`run ≡ runvm ≡ real PHP ≥8.6`) example.
> Full design: `docs/specs/2026-06-20-m-rt-rich-types-design.md`. Approved plan mirror:
> `~/.claude/plans/misty-honking-lynx.md`.

## Decisions Log

- [2026-06-20] AGREED: `is` value-equality stub is a GA blocker (parses + type-checks but
  `transpile.rs:623` rejects; `interpreter.rs:515` is a misleading `eq_val` alias). Resolve it.
- [2026-06-20] AGREED: keyword is **`instanceof`** (lowercase, PHP-style), RHS parsed as a Type.
  `is` ambiguity (reads like equality) is what caused the original stub bug — conceded over my
  initial `is`-keyword preference.
- [2026-06-20] AGREED: **maximal scope** — full TS-grade type system (interfaces, instanceof, unions,
  intersections, erased generics, inheritance, Map/Set, traits). Feasible because PHP 8.0/8.1 has
  union/intersection/interface/instanceof natively. Chosen over my "coherent cluster only" + "defer"
  recommendations after I challenged hard at each step; developer: "put a real effort here".
- [2026-06-20] AGREED: discipline guardrails — enum-vs-union coherence rule, `W-INSTANCEOF-CHAIN`
  lint, `extends` final-by-default + explicit `override`, generics fully erased (no monomorph),
  no silent Op growth.
- [2026-06-20] AGREED: build order S1 instanceof → S2 interfaces → S3 Map/Set → S4 unions →
  S5 intersections → S6 extends → S7 generics → S8 traits. Only S1+S3 add Ops.
- [2026-06-20] AGREED (pace): proceed autonomously, gate per commit; commit green self-contained
  slices (project git autonomy). Plan approved via ExitPlanMode.
- [2026-06-20] AGREED (S2 design, locked at implementation): (a) interfaces reuse `FunctionDecl`
  (empty body) for method *signatures* — no new sig struct, no new exhaustive surface beyond
  `Item::Interface`. (b) `class_implements` is a SINGLE shared pure fn `ast::class_implements(program)`
  (transitively flattened, sorted, cycle-safe via a visited guard) called by checker + interpreter +
  compiler — one algorithm, no divergence (the `free_vars` discipline); the VM bakes the compiler's
  result into `BytecodeProgram.class_implements`. (c) nominal subtyping (class → interface it
  implements) threads through `Ty::assignable_with(from,to,&subtype_oracle)`; the old
  `Ty::assignable` is `assignable_with(_,_,|_,_|false)` — keeps the single chokepoint. (d) interfaces
  are **`package main`-only** this slice (E-PKG-TYPE extended to reject library interfaces), matching
  the S2c class/enum restriction. (e) interface-typed receivers dispatch via interface method sigs
  (flattened through `extends`); narrowing `if (x instanceof I)` reuses the S1 push_scope+declare. New
  codes: `E-IFACE-IMPL` (unknown name in `implements`), `E-IFACE-UNIMPL`/`E-IFACE-SIG` (conformance),
  `E-IFACE-CYCLE` (interface-extends cycle); also backfilled the missing `E-INSTANCEOF-TYPE` explain
  entry from S1.

- [2026-06-20] AGREED (sequencing): after S2, proceed to **S3 (Map/Set)** next — keep the planned
  order (chosen over reordering S4 unions ahead). S3 adds Ops (`MakeMap`/`MakeSet`/`IndexMap`) and
  carries the iteration-order parity risk (insertion-ordered maps in both Rust backends).
- [2026-06-20] AGREED (S3 pace): run S3 with **full gates per phase** (3C/6C convergence + Phase 4
  plan-approval stop) — chosen over the milestone's "autonomous, gate per commit" default, because S3
  opens new bytecode surface (3 Ops) and carries the top milestone risk (R1 iteration-order parity).
  `_AUTONOMOUS_3C` is therefore NOT set for this slice.
- [2026-06-20] AGREED (S3 scope): **Map + Set foundation** — `Map<K,V>` literals `[k => v]` + indexing
  `m[k]`, `Set<T>` literals + value/equality, both with **insertion-ordered `Rc<Vec>`** representation
  (future-proofs R1). Discovery that drove this: the *useful* Map/Set ops (`keys`/`has`/`size`/
  `contains`/iteration) are generically typed and hit the **same wall that deferred `core.list`** (R5 —
  native sigs are concrete `Ty`, no type variables). So those ops are deferred to generics.
- [2026-06-20] AGREED (sequencing change): **reorder generics (S7) to immediately follow S3.** Rationale:
  generics is the single unblocker that makes Map/Set *and* `core.list` fully featured (keys/has/size/
  contains/map/filter), so doing it next avoids a thin intermediate state. New order: S1✓ → S2✓ → S3
  (Map/Set foundation) → **S7 generics** → S4 unions → S5 intersections → S6 extends → S8 traits.
- [2026-06-20] AGREED (S3 op design): improve on the plan's 3 Ops — add `Op::MakeMap(n)`; make the
  existing `Op::Index` **runtime-polymorphic** (List→int-bounds; Map→HKey lookup) rather than a separate
  `IndexMap` (the compiler's `CTy` is too coarse to pick statically, and the checker already guarantees
  type-correctness). Lookup single-sourced in a `value.rs` kernel (`run≡runvm`).
- [2026-06-20] AGREED (Set sequencing, final): **Set is folded into the reordered generics slice, not
  shipped thin now.** Discovery: without the generic-typed query ops (`contains`/`size`/iter), a Set's
  ONLY observable this slice is `==`, and byte-identical set equality forces an associative-array PHP
  encoding (`[e => true]`) + order-independent `eq_val` — real surface/subtlety for a feature
  demonstrable only through equality. Since generics lands next and gives Set its full ergonomics, Set
  ships *complete* there in one go. **S3 = Map foundation only** (`Op::MakeMap`, polymorphic `Index`).

- [2026-06-20] AGREED (S7 pace): run S7 **fully autonomously** (`_AUTONOMOUS_3C=1`) — design → plan →
  implement → commit green self-contained sub-slices without per-phase stops; only risky/destructive
  actions pause. Chosen over S3's "full gates per phase" because S7 adds **zero new `Op`s** (pure
  erasure), so its bytecode-surface risk is low; the residual risk (a type variable leaking into a
  backend) is covered structurally by the erase-before-backend pass + byte-identity oracle.
- [2026-06-20] AGREED (S7 sub-slicing): ship S7 as green sub-commits rather than one change. **S7a =
  erased-generics core** (the headline + the unblocker): `Ty::Param`, `<T>` on free functions,
  call-site unification, the `erase_generics` pass, backend erasure (`CTy::Other`/PHP `mixed`). **S7b
  = the consumers** built on it (Set + Map/Set query ops + `core.list`). S7a landed first.
- [2026-06-20] AGREED (S7a design, locked at implementation): (a) the parser emits `T` as an ordinary
  `Type::Named`; the **checker** turns a name into `Ty::Param` only while a function's `type_params`
  are active (`resolve_type` `other` arm), so no scope state threads into the parser. (b) Call-site
  inference is a structural first-binding-wins `unify(declared, actual, θ)` descending `List`/`Map`/
  `Set`/`Optional`/`Function`; the result type is `apply_subst(ret, θ)`; `θ` never touches the AST.
  (c) Erasure mirrors `expand_aliases`/`resolve_html`: a new `Type::Erased` AST node + `erase_generics`
  pass wired into the single `cli::check_and_expand` chokepoint, so all four backends + the project
  loader are covered. (d) **Free functions only** this slice — generic *methods* are a clean parse
  error; type params shadowing a built-in or duplicated → `E-GENERIC-PARAM`; type params are PascalCase
  (`E-TYPE-CASE`). (e) Deferred (KNOWN_ISSUES): generic methods/types/classes, a generic function used
  as a first-class *value*, an empty `[]` passed straight to a generic parameter, bounds, and variance.

- [2026-06-20] AGREED (generics reach): generics will cover **all of free functions, methods, and
  generic types/classes** — not just free functions (developer: "I want generics all options").
  Implemented incrementally on top of S7a; all stay fully erased (a generic class `Box<T>` erases its
  `<T>` and instances carry no type argument at runtime — `instanceof Box<int>` is just `instanceof Box`).
- [2026-06-20] AGREED (stdlib namespace casing): the standard-library root and its leaf modules become
  **PascalCase** — `core.console` → `Core.Console`, `core.text` → `Core.Text`, etc. (developer: "even
  native core should be PascalCase Core"), consistent with the namespace-reshape rule that package
  *segments* are PascalCase. Function names stay camelCase (`println`, `splitOnce`). `import core.console;`
  → `import Core.Console;`, call site `console.println` → `Console.println`. A milestone-scale breaking
  codemod across every `.phg`, fixture, inline test program, and doc.
- [2026-06-20] AGREED (`core.list` HOF mechanism): **Option B — a higher-order native variant**
  (`NativeEval::HigherOrder(fn(&[Value], &mut dyn FnMut(&Value,&[Value])->Result<Value,String>))`) that
  receives a backend-supplied closure-invoker. **No new `Op`**, pure natives keep their signature, and
  `map`/`filter`/`reduce` transpile to `array_map`/`array_filter`/`array_reduce`. Needs a re-entrant
  `vm.run_until(depth)` + `call_closure_value` mirroring `Op::CallValue` [Verified feasible: vm.rs call
  model inspected]. Chosen over backend intrinsics (would force a VM list-builder op) and dedicated Ops
  (pollutes the Op set with stdlib concerns). All of `map`/`filter`/`reduce` ship.
- [2026-06-20] AGREED (sequence): **Core rename → S7b → generics-all**, each a green byte-identical
  commit. Core-first so the new `Core.List`/`Core.Set` land PascalCase and are not renamed twice.
- [2026-06-20] AGREED (Core-rename scope): this slice renames the **stdlib namespace only** — `core.*`
  → `Core.*` with PascalCase leaf modules (`Core.Console`/`Core.Math`/`Core.Text`/`Core.File`/
  `Core.Bytes`/`Core.Html`; function names stay camelCase), reserve `Core` as the package root, sweep
  every `.phg`/fixture/inline-test/doc. The broader namespace reshape (`package main` → `package Main`,
  `E-PKG-CASE` on user package segments, manifest `name`→`module`, lifting `E-PKG-TYPE`) stays pending.

- [2026-06-20] AGREED (generics-all pace + sub-slicing): proceed **fully autonomously**
  (`_AUTONOMOUS_3C=1`), sub-slice by sub-slice — **(1) generic methods → (2) generic types/classes
  `Box<T>` → (3) E-PKG-TYPE lift / cross-package types** — each its own green byte-identical commit.
  Developer chose "Autonomous, sub-slice by sub-slice" over a design-pass-first gate.
- [2026-06-20] AGREED (generic-methods design, locked at implementation): generic methods **reuse the
  entire S7a free-fn machinery, zero backend changes** (the type variable is erased before any backend,
  exactly like free-fn generics). Mechanism: (a) **parser** — drop the now-vestigial `allow_generics`
  gate (both callers allow generics), so a method may declare `<T>` via the existing `parse_type_params`
  [Verified: parser.rs:1129 is the only `false` caller; interface methods build `FunctionDecl` directly
  at parser.rs:1081 with empty `type_params`, so generic *interface* methods stay a non-parse — naturally
  deferred]. (b) **checker registration** (the class-collect phase, checker.rs:694–708) — mirror the
  free-fn path: `validate_type_params` + set `active_type_params` before resolving the method sig + store
  `type_params: f.type_params.clone()` in the `FnSig` (was hardcoded `Vec::new()`), so `T` in a method
  param/ret becomes `Ty::Param`. (c) **call-site** (`check_method_call`, checker.rs:2112) — when the
  method sig is generic (`params/ret` contain `Ty::Param`), route through the existing `check_generic_call`
  (same first-binding-wins `unify`); else the unchanged `check_args` path — identical to how
  `check_native_call` branches. (d) **body** already handled — `check_function` (shared by methods) sets
  `active_type_params` from `f.type_params`. (e) **erasure** — extend `erase_generics` with an
  `Item::Class` arm that rewrites any method with non-empty `type_params` (reusing the existing
  `rty`/`rparam`/`rstmt` helpers), guarded so a class with no generic method is returned byte-for-byte
  untouched. Scope: generic methods on a **non-generic class**, inferred from arguments only (the class's
  own `<T>` is the next sub-slice). Deferred (KNOWN_ISSUES): generic interface methods, generic
  classes/types, a generic method referenced as a first-class value.

## Formal Plan

See the approved plan (`~/.claude/plans/misty-honking-lynx.md`) and the design spec. Slice table:

| # | Slice | New Op? | Status |
|---|-------|---------|--------|
| S1 | `instanceof` (class-only) + smart-cast, retire `is` | `Op::IsInstance` | **DONE** (gate green: 394 lib + 10 PHP-oracle differential; clippy+fmt clean; example byte-identical run≡runvm≡PHP) |
| S2 | interfaces + `implements`/`extends` (+ instanceof interface table) | no | **DONE** (404 lib + PHP-oracle differential incl. `guide/interfaces.phg`; clippy+fmt clean; byte-identical run≡runvm≡PHP; subtyping via `Ty::assignable_with`, shared `ast::class_implements`) |
| S3 | **Map foundation**: `Map<K,V>` literals `[k=>v]` + `m[k]` indexing (fault on miss); insertion-ordered `Rc<Vec>` rep; `CTy::Map` so `m[k]` is an arithmetic operand. Set + all generic-typed ops (keys/has/size/contains/iter) → S7. | `MakeMap` (Index made polymorphic, no `IndexMap`) | **DONE** (413 lib + PHP-oracle differential incl. `guide/maps.phg`; clippy+fmt clean; byte-identical run≡runvm≡PHP) |
| S7 | erased generics `<T>` (+ unblock `core.list` **and** full Map/Set: keys/has/size/contains/map/filter, **plus Set itself**) — **reordered to follow S3** | no (erase) | **S7a DONE** (generics core: `Ty::Param` + `<T>` on free functions + call-site unify + `erase_generics` pass; 424 lib + PHP-oracle differential incl. `guide/generics.phg`; clippy+fmt clean; byte-identical run≡runvm≡PHP). **S7b** (Set + Map/Set query ops + `core.list`) = next |
| S4 | union `A\|B` + match-over-union exhaustiveness | no | pending |
| S5 | intersection `A&B` (requires S2) | no | pending |
| S6 | `extends` (final-by-default, `override`) | no (flatten) | pending |
| S8 | traits/mixins | no (flatten) | pending |

## S3 task checklist (Map foundation; 3C-converged 8/8)

- [ ] `value.rs`: `Value::Map` → insertion-ordered `Rc<Vec<(HKey,Value)>>`; `HKey::from_value`/`to_value`;
      shared kernels `build_map(pairs)` (dedup **first-position/last-value**, PHP-identical — F2) and
      `map_index(map,key)` (fault `"map key not found"`; non-HKey key → clean `Err`, EV-7 — F3);
      `eq_val` Map arm **order-independent** (F6). [Verified: no existing `Value::Map` construction site.]
- [ ] `ast.rs`: `Expr::Map(Vec<(Expr,Expr)>, Span)` + `span()` + casing walker + `expand_aliases` +
      free-var walkers (`in_expr` ~2198, `rexpr` ~2429).
- [ ] `parser.rs`: in `[ … ]`, after first element peek `=>` (FatArrow) → map mode (`k => v` pairs, ≥1;
      empty map deferred). `[]` stays empty list. Commit to list-or-map after first element; mixed
      separators error cleanly (F5). Lambda `=>` is consumed by the lambda parser before the peek (F4).
- [ ] `checker.rs`: `check_map` (K ∈ {int,bool,string} else `E-MAP-KEY`; unify V) → `Ty::Map(K,V)`;
      un-reject `Ty::Map(k,v)` in `check_index` (idx ~ K, returns V).
- [ ] `compiler.rs`: **add `CTy::Map(Box<CTy>,Box<CTy>)`** (F7 — fixes `m[k]+1` VM compile error);
      `resolve_cty` `Map<K,V>` → `CTy::Map` (split from the `Map|Set`→Other arm, line ~578); `as_num`
      Map arm → None; `ctype(Expr::Map)` → `CTy::Map`; `ctype(Expr::Index)` Map arm → `*v`;
      `Expr::Map` → emit pairs + `Op::MakeMap(n)`; `stack_effect(MakeMap(n)) = 1 - 2n`.
- [ ] `chunk.rs`: `Op::MakeMap(n)` `validate` arm (no pool index; like `MakeList`).
- [ ] `vm.rs`: `Op::MakeMap` (build via `build_map` kernel); make `Op::Index` **polymorphic**
      (List→int bounds; Map→`map_index` kernel).
- [ ] `interpreter.rs`: `Expr::Map` eval (via `build_map`); polymorphic Index (via `map_index`).
- [ ] `transpile.rs`: `Expr::Map` → `[k => v, …]`; add `Expr::Map` to the compound-classification
      match (~1111, treat like `List`). (Index already emits `$o[$i]` — map-correct.)
- [ ] `examples/guide/maps.phg` (lookup table; incl. an `intMap[k] + 1` line to gate F7) + README
      index/matrix; FEATURES/KNOWN_ISSUES (empty-map + Set-deferral + missing-key fault)/CHANGELOG/CLAUDE.md.
- [ ] gate (`cargo test` w/ `PHORGE_REQUIRE_PHP=1`, clippy, fmt) — `maps.phg` byte-identical
      run≡runvm≡PHP — then commit.

## S1 task checklist

- [ ] `token.rs` + `lexer.rs`: `instanceof` keyword
- [ ] `ast.rs`: `Expr::InstanceOf { value, type_name, span }`; remove `BinaryOp::Is`
- [ ] `parser.rs`: parse `x instanceof TypeName` (RHS = type name); remove `T::Is` op mapping
- [ ] `checker.rs`: typecheck + true-branch narrowing; remove 2 `BinaryOp::Is` arms; `E-INSTANCEOF-TYPE`
- [ ] `interpreter.rs`: eval `Expr::InstanceOf` (class-name compare); remove `BinaryOp::Is` arm
- [ ] `chunk.rs`: `Op::IsInstance(usize)` + `type_tests: Vec<String>` + validate bounds arm
- [ ] `compiler.rs`: compile `Expr::InstanceOf`; `stack_effect` arm
- [ ] `vm.rs`: `exec_op` arm
- [ ] `transpile.rs`: emit `$x instanceof Name`; remove the `is` rejection
- [ ] `examples/guide/instanceof.phg` + `examples/README.md` entry
- [ ] `KNOWN_ISSUES.md` / `FEATURES.md` / `CHANGELOG.md` updates
- [ ] gate (`cargo test` w/ `PHORGE_REQUIRE_PHP=1`, clippy, fmt) + commit
