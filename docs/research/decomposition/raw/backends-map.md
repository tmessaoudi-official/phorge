# Backend whale-file map + exhaustive-match coupling inventory

Research for the codebase decomposition milestone. Read-only. Source of truth: the six
backend files as of the read date. NO OOP/SOLID/visitor recommendations below — the spine
is the compile-time exhaustiveness of the big enum matches, and every option is judged
against "does this defeat the forgotten-arm compile error?"

Files in scope (line counts at read time):

| File | Lines | Role |
|------|-------|------|
| `src/compiler.rs` | 3281 | AST → `BytecodeProgram` (the M2 stack-VM codegen) |
| `src/transpile.rs` | 2782 | AST → PHP source (the M1/M7 transpiler) |
| `src/interpreter.rs` | 2092 | tree-walking evaluator (`run`) |
| `src/vm.rs` | 1375 | stack VM (`runvm`) |
| `src/chunk.rs` | 888 | `Op` set + `Chunk`/`BytecodeProgram` + `validate` |
| `src/dispatch.rs` | 167 | runtime overload selector (shared by both backends + transpiler) |

Note: a large share of each file's line count is `#[cfg(test)]` tests at the bottom
(compiler ~310 lines of tests from L2974; vm ~460 from L913; interpreter ~330 from L1755;
transpile ~370 from L2411; chunk ~370 from L520). The *production* surfaces are smaller than
the totals suggest — the matches are the real bulk.

---

## 1. Per-file structural map

### `src/chunk.rs` (888) — the Op set + the validate trio member

| Lines | Item | Purpose |
|------|------|---------|
| 17–39 | `enum ConstKey` + `ConstKey::of` | dedup key for constant interning (scalars only) |
| 47–77 | `enum FaultMsg` + `message()` | single-sourced runtime-fault bodies (6 variants); shared so VM/interpreter stay byte-identical |
| 81–241 | **`pub enum Op`** | the instruction set — **~60 variants** (heavily doc-commented; this is the spine enum) |
| 248 | `const THROW_SENTINEL` | fault-channel token for `Op::Throw` |
| 252–292 | `struct Chunk` + `add_const`/`emit` | code+const-pool+line-table; build-time interning |
| 299–307 | `struct Function` | name/arity/n_captures/chunk |
| 312–328 | `struct EnumDesc` / `ClassDesc` | static descriptor tables |
| 335–364 | `struct BytecodeProgram` | functions + all program-level descriptor tables |
| **382–517** | **`BytecodeProgram::validate`** | **COUPLED TRIO #1 — exhaustive over `Op`** |
| 520–888 | tests | hand-built `BytecodeProgram`s; ~12 validate tests |

Candidate sub-modules: `chunk.rs` is already cohesive and is the natural *shared core* —
splitting it would be counter-productive (the `Op` enum is the contract every other file
keys against). At most: `chunk/op.rs` (the `Op` enum + `FaultMsg`), `chunk/program.rs`
(`Chunk`/`Function`/descriptors/`BytecodeProgram` + `validate`). But `validate` *must* sit
next to the `Op` definition for the forgotten-arm guarantee to be obvious to a maintainer.

### `src/dispatch.rs` (167) — already the model of by-construct cohesion

| Lines | Item | Purpose |
|------|------|---------|
| 21–37 | `enum ParamKind` (11 variants) | runtime-checkable summary of a param type |
| 42 | `type OverloadSet` | `Vec<(Vec<ParamKind>, fn-index)>` |
| 45–61 | `param_kind(&Type) -> ParamKind` | static type → runtime kind (small match over `Type`) |
| 65–101 | `is_subtype` / `kind_matches` / `at_least_as_specific` | the matching kernels |
| 107–115 | `dominates` | specificity ordering (used by transpiler) |
| 132–167 | `select_overload` | the shared selector — same code drives interpreter + VM + (via `dominates`) the PHP dispatcher |

This is the proof-of-concept for "one concept, one file, consumed identically by every
backend." No split needed; it is the template.

### `src/vm.rs` (1375) — the VM + the exec_op trio member

| Lines | Item | Purpose |
|------|------|---------|
| 23–26 | `enum Flow` | Next/Done run-loop signal |
| 34–47 | `struct Frame` / `struct Handler` | call frame; exception handler |
| 51–56 | `throw_display` (free fn) | name a thrown value for an uncaught message |
| 58–73 | `struct Vm<'a>` | program + stack + frames + statics + out + handlers + pending_throw |
| 76–86 | `Vm::new` | seed `statics` from `static_inits` |
| **90–172** | `Vm::run` | the dispatch loop + fault/trace assembly + throw-unwind |
| **177–690** | **`Vm::exec_op`** | **COUPLED TRIO #2 — exhaustive over `Op`** (~60 arms) |
| 698–715 | `unwind_throw` | search the handler stack |
| 719–738 | `do_return` | unwind a frame |
| 747–781 | `call_closure_value` | re-entrant closure call from a higher-order native |
| 788–820 | `run_until` | nested run loop for re-entrancy (duplicates the run-loop skeleton) |
| 822–893 | `pop`/`pop_n_start`/`frame_slot`/`split_off`/`pop2_*`/`push_i` | stack helpers |
| 899–911 | `compare` (free fn) | `Op`→bool projection for Lt/Gt/Le/Ge (small match over `Op`) |
| 913–1375 | tests | hand-built chunks + run≡runvm trace-parity tests |

Candidate sub-modules: `vm/mod.rs` (struct + run + run_until + helpers), `vm/exec.rs`
(`exec_op`). But `exec_op` *is* the VM — see §3 for why splitting its arms into per-op files
is the most invasive option of all.

### `src/interpreter.rs` (2092) — the tree-walker

| Lines | Item | Purpose |
|------|------|---------|
| 21–43 | `enum Signal` | Return/Break/Continue/Throw/Runtime unwinding |
| 46–101 | `stmt_line`/`rt`/`signal_msg`/`lit_msg`/`as_bool` (free fns) | small helpers |
| 104–141 | `struct CallScopes` + impl | lexical scope stack (declare/lookup/assign) |
| 143–180 | `struct Interp` | funcs/classes/implements/method_origins/variants/statics/frame/this/out/trace/depth/pending_throw |
| 189–229 | `pub fn interpret` | top-level entry |
| 231–254 | `throw_what`/`catch_type_names` (free fns) | |
| 256–308 | `pub fn call_named` | external entry (used by the loader/tests) |
| 311–425 | `Interp::collect` | gather decls into the tables (Item dispatch lives here, as an `if let` cascade, not a clean match) |
| 427–471 | `run_call` | push frame + exec body + handle Signal |
| 477–489 | `exec_stmts`/`exec_scoped` | |
| **491–705** | **`exec_stmt`** | **exhaustive over `Stmt`** (13 arms) |
| 710–734 | `match_catch`/`value_is_a` | |
| 736–808 | `exec_while`/`exec_cfor`/`cfor_loop` | loop drivers |
| **810–1047** | **`eval`** | **exhaustive over `Expr`** (24 arms) |
| 1049–1135 | `eval_ident`/`eval_str`/`eval_unary`/`eval_binary` | sub-evaluators |
| 1136–1275 | `eval_call` | call resolution (free-fn/overload/closure/variant/ctor/method/native) cascade |
| 1276–1397 | `select_free_overload`/`call_closure`/`call_tree_closure`/`eval_args` | |
| 1399–1503 | `ctor_plan`/`construct` | constructor folding |
| 1505–1623 | `call_method`/`hook_get`/`hook_set`/`run_hook_get` | method + property-hook dispatch |
| 1624–1640 | `eval_match` | drives `match_pattern` per arm |
| 1642–1700 | `arith`/`compare` (free fns) | **small matches over `BinaryOp`** |
| **1706–1753** | `match_pattern` (free fn) | **exhaustive over `Pattern`** (9 arms) |
| 1755–2092 | tests | |

Candidate sub-modules (by cohesion): `interpreter/{stmt,expr,call,construct,match,scope}.rs`
+ a `mod.rs` holding `struct Interp` and the shared helpers.

### `src/transpile.rs` (2782) — the PHP emitter

| Lines | Item | Purpose |
|------|------|---------|
| 9–20 | `pub fn emit` | entry |
| 22–50 | `decomposed_classes` (free fn) | which classes need MI decomposition |
| 52–106 | `struct Transpiler` | out/indent/locals/funcs/variants/classes/namespaced/uses_* helper flags/variant_fields |
| 108–113 | `enum MatchTarget` | Return/Assign/—; how a match arm lands its value |
| 115–196 | namespace/type/escape free fns (`namespace_of`/`php_type_ref`/`php_catch_type`/`looks_like_global_call`) | |
| 198–223 | `Transpiler::new` | |
| 224–260 | `collect` | gather decls |
| 261–312 | `emit_program` | **Item dispatch (clean match, 7 arms)** + main + helpers |
| 321–381 | `emit_program_namespaced` | multi-package brace-namespace variant (a second Item dispatch) |
| 382–486 | `emit_runtime_helpers` | the `__phorj_div`/`_rem`/`_range`/`_unwrap`/`_clone_with` PHP helpers |
| 487–600 | `line`/scope/`static_ref`/`type_pos_ref`/`emit_type`/`ret_hint` | helpers (`emit_type` is a small match over `Type`) |
| 601–736 | `emit_function*`/`emit_free_fn`/`emit_overload_set` | function emission |
| 736–765 | `overload_branch_test` | match over `ParamKind` |
| 767–1410 | `emit_enum`/`emit_class`/`emit_trait`/`emit_class_members`/`emit_synth_construct`/`emit_decomposed_class`/`emit_multi_class`/`build_trait_clauses`/`class_field_context`/`emit_interface` | the type-declaration emitters (the bulk of the file) |
| **1411–1673** | **`emit_stmt`** | **match over `Stmt`** — NOT cleanly per-variant: leads with 3 guard-arms (`Return`/`VarDecl` with a `Match`/`Propagate` init) before the generic arms |
| 1674–1694 | `emit_for_clause` | |
| **1695–1950** | **`emit_expr`** | **exhaustive over `Expr`** (24 arms) |
| 1955–2347 | `emit_string`/`variant_ref`/`emit_call`/`emit_args`/`emit_member_call`/`emit_match`/`is_primary`/`paren_if_compound`/`binop`/`resolve_ident` | sub-emitters (`binop` is a match over `BinaryOp`) |
| 2348–2409 | `lit_arg`/`php_escape`/`php_escape_bytes`/`is_promoted`/`vis` (free fns) | |
| 2411–2782 | tests | |

Candidate sub-modules: `transpile/{program,types,stmt,expr,call,match,helpers}.rs` + a
`mod.rs` for `struct Transpiler`.

---

## 2. THE KEY DELIVERABLE — exhaustive-match inventory

### 2a. The documented coupled trio (all exhaustive over `Op`, all `~60` arms)

| # | Location | Function | Arms | What each arm does (gist) |
|---|----------|----------|------|---------------------------|
| **TRIO-1** | `src/chunk.rs:407–498` | `BytecodeProgram::validate` (inner `match op`) | ~60, NO `_` | Each arm either range-checks the op's pool index (returns `Some(err)` on OOB) or is listed in the giant no-index `None` arm. The comment at L401–406 explicitly states it is exhaustive with NO wildcard so a new `Op` is a compile error here. |
| **TRIO-2** | `src/vm.rs:178–688` | `Vm::exec_op` (`match op`) | ~60, NO `_` | The actual execution semantics: arithmetic kernels, stack ops, jumps, calls (Call/CallOverload/CallNative/CallMethod/CallValue), enum/class/instance ops, statics, throw/handler ops. The hot path. |
| **TRIO-3** | `src/compiler.rs:1056–1105` | `Compiler::stack_effect` (`match op`) | ~60, NO `_` | The net stack-height delta of each op (e.g. `AddI => -1`, `Const => 1`, `MakeList(n) => 1 - n`, `CallMethod(_,argc) => -(argc)`). Used to track `self.height` during codegen. |

These three are coupled by construction: adding an `Op` variant is a hard compile error in
all three until handled. The project's `op-variant-match-coupling` memory and CLAUDE.md both
codify "extend three exhaustive matches in the same commit." There is also a 4th, partial
coupling: `compiler::patch_jump_to` (L1132–1140) matches the *jump-shaped* ops
(`Jump`/`JumpIfFalse`/`PushHandler`) with an `unreachable!` fallthrough — not exhaustive but
sensitive to the jump-op set.

### 2b. Big AST-enum matches (one per backend; these are the per-construct surfaces)

| Location | Function | Enum | Arms | Exhaustive? | Notes |
|----------|----------|------|------|-------------|-------|
| `src/interpreter.rs:811–1046` | `eval` | `Expr` | 24 | YES (no `_`; `Html` is `unreachable!`) | tree-walk evaluation |
| `src/compiler.rs:1539–1718` | `expr` | `Expr` | 24 | YES (`Html` unreachable) | AST→bytecode emit |
| `src/transpile.rs:1696–1949` | `emit_expr` | `Expr` | 24 | YES (`Html` unreachable) | AST→PHP emit |
| `src/compiler.rs:1192–1329` | `ctype` | `Expr` | partial (`other =>` catch-all) | NO | class-aware operand-type inference; falls through to an error for un-nameable exprs |
| `src/interpreter.rs:497–704` | `exec_stmt` | `Stmt` | 13 | YES (no `_`) | statement execution |
| `src/compiler.rs:1348–1535` | `stmt` | `Stmt` | 13 | YES (no `_`) | statement codegen |
| `src/transpile.rs:1412–1673` | `emit_stmt` | `Stmt` | 13 + **3 guard-arms first** | YES on the base set, but the leading guard-arms (`Return{value:Some(Match)}`, `VarDecl{init:Match}`, `VarDecl{init:Propagate}`) are *position-specific specializations*, not new variants | the only "impure" big match — see §5 |
| `src/interpreter.rs:1706–1753` | `match_pattern` (free fn) | `Pattern` | 9 | YES (no `_`) | runtime pattern test |
| `src/compiler.rs:2849–2890` | `emit_pattern_test` | `Pattern` | 9 (Wildcard+Binding grouped → 7 textual) | YES (no `_`) | pattern-test codegen |
| `src/transpile.rs:280–301` | `emit_program` | `Item` | 7 | YES (no `_`) | top-level item dispatch |
| `src/transpile.rs` (`emit_program_namespaced`) | second Item dispatch | `Item` | partial | NO (`_ => false` in the namespaced-detection closure at L265–273) | duplicate item routing |

Item dispatch in the **interpreter** and **compiler** is NOT a single clean match — it is an
`if`/`cascade` inside `Interp::collect` (L311–425) and `compile_program` (L214–767)
respectively, walking `program.items` and branching on the variant. (So the *Item* surface
is the least uniformly-matched of the AST enums.)

### 2c. Small enum matches over `Op`, `BinaryOp`, `UnaryOp`, `Type`, `Pattern`, `ParamKind`

| Location | Function | Enum | Exhaustive? | Purpose |
|----------|----------|------|-------------|---------|
| `src/vm.rs:899–911` | `compare` (free fn) | `Op` (Lt/Gt/Le/Ge) | partial (`_ => unreachable!`) | op→bool projection |
| `src/compiler.rs:1132–1140` | `patch_jump_to` | `Op` (jump ops) | partial (`other => unreachable!`) | rewrite jump target |
| `src/compiler.rs:1805–1816` | `compile_binary` (inner) | `(BinaryOp,NumTy)` | partial (`_ => unreachable!`) | pick AddI/AddF/… |
| `src/compiler.rs:1751–1846` | `compile_binary` (outer) | `BinaryOp` | YES across the two matches (And/Or/Coalesce + arithmetic/cmp; Pipe `unreachable!`) | binary codegen |
| `src/interpreter.rs:1642–1679` | `arith` (free fn) | `BinaryOp` (×inner) | partial (`_ => unreachable!`) | int/float arithmetic kernels |
| `src/interpreter.rs:1681–1700` | `compare` (free fn) | `BinaryOp` | partial (`_ => unreachable!`) | comparison kernel |
| `src/transpile.rs:2298–2320` | `binop` | `BinaryOp` | (maps to PHP operators) | operator spelling |
| `src/compiler.rs:1335–1341` | `as_num` | `CTy` | YES | numeric refinement |
| `src/dispatch.rs:45–61` | `param_kind` | `Type` | partial (`_ => Any`) | type→ParamKind |
| `src/transpile.rs:537–593` | `emit_type` | `Type` | (PHP type spelling) | type emission |
| `src/dispatch.rs:70–86` | `kind_matches` | `(ParamKind,Value)` | partial (`_ => false`) | overload arg match |
| `src/transpile.rs:736–765` | `overload_branch_test` | `ParamKind` | (PHP `is_*` spelling) | overload dispatcher emit |

`Ty` (the **checker-only** type enum) does NOT appear in these backend files — it lives in
the checker. The backend files use `ast::Type` (the surface type, 7 variants) and the
compiler-local `CTy` (operand-type, 6 variants: Int/Float/Class/Other/List/Map/Fn).

---

## 3. Thin-dispatcher feasibility (the central milestone question)

**Verdict: NOT feasible for the coupled trio without making the backend internals
`pub(crate)` and shredding the hot path. The exhaustiveness guarantee survives a split, but
the *cost* is high and the byte-identity risk is real. Recommend AGAINST per-op files for
`exec_op`/`stack_effect`/`validate`; recommend FOR per-construct files only where arms are
already self-contained (the AST-enum matches in the transpiler, and `match_pattern`).**

### Why the trio resists per-op files

The proposed shape is `Op::Add => ops::add::exec(vm, fr, func)`. The problem is that every
arm of `exec_op` mutates **private `Vm` state** through private helpers:

Real example 1 — `Op::AddI` (`vm.rs:188–191`):
```rust
Op::AddI => {
    let (a, b) = self.pop2_int()?;          // private fn
    self.push_i(crate::value::int_add(a, b))?; // private fn, mutates self.stack
}
```
`pop2_int`, `push_i`, `pop`, `split_off`, `pop_n_start`, `frame_slot` are all **private
methods on `Vm`**, and `self.stack`/`self.frames`/`self.statics`/`self.out`/`self.handlers`/
`self.pending_throw`/`self.program` are all **private fields**. A `ops::add::exec(&mut Vm,…)`
free fn would force every one of those to become `pub(crate)` — i.e. the entire `Vm`
internals become a public-within-crate surface, exactly the encapsulation the single-file
design currently guarantees. Same for the compiler:

Real example 2 — `Op::MakeClosure` in `stack_effect` (`compiler.rs:1089–1097`):
```rust
Op::MakeClosure(idx) => {
    let lo = self.base_fn_idx;
    let n = if *idx >= lo && *idx < lo + self.lambda_n_captures.len() {
        self.lambda_n_captures[idx - lo]
    } else { 0 };
    1 - n as isize
}
```
This arm reads three private `Compiler` fields (`base_fn_idx`, `lambda_n_captures`, and via
the surrounding fn `arities`/`enum_descs`/`class_descs`). `stack_effect` arms read
`self.arities`, `self.enum_descs`, `self.class_descs`. Pulling these into `ops::makeclosure`
needs all of them `pub(crate)`.

The `Compiler<'a>` struct has **~22 fields** (L110–182), most of them borrowed references
into program-wide tables, all private. A per-op or per-construct split of `compiler.rs`
multiplies the surface of that struct across files.

### Invasiveness estimate

- **Coupled trio per-op files: HIGH invasiveness, NET NEGATIVE.** Requires `pub(crate)` on
  ~10 `Vm` fields + ~7 helper methods, and ~22 `Compiler` fields. The hot `exec_op` becomes
  a fan-out of function calls (a deopt risk for the VM unless `#[inline]` everywhere, which
  re-couples). The exhaustiveness *guarantee is preserved* (the central `match` still lists
  every variant, each arm a one-liner delegate), but the win is illusory: you've moved the
  body, not removed the coupling. You now maintain `ops/add.rs`, `ops/sub.rs`, … (~60 files)
  AND the three central dispatch matches.

- **AST-enum matches in the transpiler: LOW–MEDIUM, NET POSITIVE.** `emit_expr` arms mostly
  call `self.emit_expr` recursively and string-format — they touch `self.out` indirectly via
  returned `String`s and read a handful of flags (`self.namespaced`, `self.uses_*`). Moving
  `emit_expr`/`emit_stmt`/`emit_call`/`emit_match` into `transpile/{expr,stmt,call,match}.rs`
  as `impl Transpiler` blocks in separate files needs ZERO visibility changes (an `impl`
  block can span files within a module via `mod` + `impl Transpiler { … }` per file). This is
  the cleanest win.

- **`match_pattern` / `emit_pattern_test` / `dispatch.rs`: already movable.** `match_pattern`
  is a free fn taking only `(&Pattern, &Value, &BTreeMap, &mut Vec)` — it could live in a
  `pattern.rs` shared module *today* with no state coupling. `dispatch.rs` already is this.

**Key insight: the cheap, behavior-safe decomposition is by `impl <Struct>` split across
files, NOT by free-fn-per-op.** Rust lets you write `impl Compiler<'a> { fn expr(…) }` in
`compiler/expr.rs` and `impl Compiler<'a> { fn stmt(…) }` in `compiler/stmt.rs`, both folded
into one `Compiler` type. No field needs to leave private; the methods stay methods. The
central `match` (and thus exhaustiveness) stays in one place; only the *helper* fns that each
arm delegates to move out. This preserves the forgotten-arm compile error AND avoids the
`pub(crate)` explosion.

---

## 4. By-phase-subsplit vs by-construct-thin-dispatcher (from these files)

**From what these six files show, BY-PHASE sub-split is the more natural fit — strongly so.**

Reasons grounded in the code:

1. **Each backend's private state is phase-bound, not construct-bound.** `Vm`'s stack/frames,
   `Compiler`'s 22 fields, `Interp`'s scopes/this/trace are all *per-phase* context. A
   by-construct file (`enums.rs` holding "everything about enums") would need to reach into
   the `Vm` AND the `Compiler` AND the `Interp` AND the `Transpiler` private states
   simultaneously — pulling all-phase types into one file means that file imports and touches
   four different structs' internals. That is the "soup" the prompt warns about, and here it
   is concrete: an `enum`-construct file would contain `Op::MakeEnum` exec (needs `Vm`),
   `Op::MakeEnum` stack-effect + the `Expr::Call`→`MakeEnum` codegen (needs `Compiler`),
   `eval` enum arms (needs `Interp`), and `emit_enum` (needs `Transpiler`). Four private
   surfaces in one file.

2. **The exhaustiveness spine is per-phase.** The three coupled matches live in three
   different phases (chunk/vm/compiler). The AST-enum matches are one-per-backend. A
   by-construct split would *scatter* each exhaustive match's arms across construct files,
   which either (a) defeats exhaustiveness (the central match becomes a dispatcher with arms
   in other files — but the compiler still enforces the dispatcher lists every variant, so
   this is survivable) or (b) keeps the central match and only moves bodies (which is the
   by-`impl`-split described in §3, and that is naturally organized by *phase sub-file*:
   `compiler/expr.rs`, `compiler/stmt.rs`).

3. **dispatch.rs is the ONE genuinely by-construct, cross-phase module — and it works
   precisely because it has NO backend state.** It is pure functions over `(ParamKind, Value,
   oracle)`. The lesson: by-construct cross-phase extraction is clean *only* for
   state-free shared kernels (the `value::*` kernels and `dispatch::*` are the existing
   examples). Construct logic that needs a backend's mutable context cannot follow that model
   without exporting the context.

**Recommended shape (behavior-preserving, exhaustiveness-safe):** per-backend module dirs
with **phase/role sub-files via `impl` splitting**:
- `compiler/{mod,program,expr,stmt,binary,call,match,pattern,classes,control,types}.rs`
- `transpile/{mod,program,types,stmt,expr,call,match,helpers}.rs`
- `interpreter/{mod,stmt,expr,call,construct,match,scope}.rs`
- `vm/{mod,exec,closure}.rs` (keep `exec_op` whole in `exec.rs`)
- `chunk.rs` stays single (the shared contract; `validate` next to `Op`)
- `dispatch.rs` stays as the by-construct shared template
- Optionally promote `match_pattern` + `Pattern` codegen knowledge into a shared
  `pattern.rs` only for the *free-fn* parts (the interpreter's `match_pattern` qualifies).

This is "by-phase sub-split with `impl`-across-files," NOT "by-construct thin-dispatcher."

---

## 5. Risk notes for behavior-preservation

1. **The three coupled matches must stay textually exhaustive (no `_`).** Any split that
   introduces a wildcard or an `_ => unreachable!()` in `validate`/`exec_op`/`stack_effect`
   silently destroys the #1 safety net. If arms are delegated to per-op fns, the central
   match must still list every variant explicitly (so the delegate-call set is the
   exhaustiveness witness). Verify post-split that adding a dummy `Op` variant still fails to
   compile in all three.

2. **`emit_stmt` in the transpiler has order-dependent guard-arms** (`transpile.rs:1415–1456`):
   `Stmt::Return{value:Some(Match)}` and `Stmt::VarDecl{init:Match/Propagate}` MUST precede
   the generic `Return`/`VarDecl` arms. A split that reorders or relocates these (e.g. moving
   "match handling" to a `match.rs`) can change which arm fires → different PHP → byte-identity
   break. These specializations are *position-sensitive*, not variant-sensitive.

3. **`self.height` tracking in the compiler is stateful and threaded through `emit`**
   (`compiler.rs`, the `height` field + `stack_effect` driving it). Splitting `expr`/`stmt`/
   `compile_binary`/`compile_match` across files is safe *only if they remain `impl Compiler`
   methods* sharing the same `&mut self` — a free-fn refactor that passes `height` by value
   would desync the scratch-slot math (the `m_slot = self.height - 1` trick at L2814, L1785,
   L2006). The `null-op-scratch-slot` and `lambda-function-table-layout` memories document
   exactly this class of silent break.

4. **Re-entrancy duplication in the VM** (`run` vs `run_until`, vm.rs:100–172 and 788–820)
   share the dispatch-loop skeleton by copy. If `exec_op` moves to `vm/exec.rs`, both loops
   must still call the same `exec_op` — do not let a split fork them.

5. **`unreachable!` arms are load-bearing parity assertions, not dead code.** `Expr::Html`,
   `BinaryOp::Pipe`, `Expr::Propagate` (in `emit_expr`) all assume an earlier phase erased/
   lowered them. A split must keep these arms (they document the pipeline contract); removing
   one to "clean up" would mask a real divergence if the erasure ever regresses.

6. **The byte-identity gate is the only acceptable proof.** `tests/differential.rs` (run ≡
   runvm ≡ real PHP over every `examples/**/*.phg` + project roots) must stay green after
   every split step. Per CLAUDE.md, also run with `PHORJ_REQUIRE_PHP=1` and the PHP 8.4
   oracle (the local php-master is too permissive). No split is "done" without that.

7. **Adding a `pub(crate)` field is a permanent encapsulation downgrade** — if the milestone
   chooses per-op free fns for the trio, that decision is hard to walk back. Prefer the
   `impl`-split (no visibility change) and treat per-op files as out of scope.
