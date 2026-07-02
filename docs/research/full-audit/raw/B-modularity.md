# Agent B — Modularity Census & Decomposition Specification

> Date: 2026-07-02. Read-only audit; no code changed. Census command:
> `find src -name '*.rs' | xargs wc -l | sort -rn`. All line numbers below are from
> `rg -n '^(pub…)?(fn|impl|struct|enum|mod|trait)'` structure dumps on the current tree
> [Verified: ran both; outputs reproduced in the per-file sections].
> Inherits the conventions of `docs/specs/2026-06-23-decomposition-milestone-design.md`
> (M-Decomp): hybrid by-phase backbone + selective thin-dispatcher; splits live inside one
> `mod foo { … }` as sibling files with additional `impl Type` blocks (child modules see the
> parent's private fields — the `bundle/` precedent); moves-only, zero behavior change; every
> spine-touching wave gated by the full differential
> (`PHORJ_PHP=… PHORJ_REQUIRE_PHP=1 cargo test`) + clippy + fmt.

## 0. Census summary

> **Perimeter (scope update mid-audit): the whole repo**, not just `src/` — all `.rs` including
> `tests/`, plus `tools/`, `scripts/`, `playground/`, `editors/`, `docs/`, `examples/`,
> `conformance/`, `selftest/`, `bench/`. Excluded: `target/`, vendored fixture internals,
> `dist/` binaries. Rust findings outside `src/` are in §8; non-Rust code in §9; docs in §10;
> examples in §11.

- **In `src/`: 52 files > 500 lines** (81,979 total) [Verified: census output].
  - 44 production files, 8 dedicated test files (`*/tests*.rs`, `*_tests.rs`).
  - 14 production files > 1000 lines (the regrown whales).
- **Outside `src/`: 3 more `.rs` files > 500** — `tests/differential.rs` **2966** (the single
  biggest file in the repo), `tests/serve.rs` 564, `tests/cli.rs` 547 [Verified: repo-wide
  census]. `tests/differential.rs` gets a full split spec (§8.1).
- **Non-Rust code: zero god files** — largest are `playground/web/main.js` 428 and
  `tools/return_type_codemod.py` 142 [Verified: §9 census].
- The 2026-06-23 M-Decomp layout held its *shape* (checker/, compiler/, transpile/,
  interpreter/, parser/ are directories with per-phase children) but several children regrew
  past 1000 (`transpile/program.rs` 2689, `checker/calls.rs` 1981, `checker/collect.rs` 1892)
  and new whales appeared post-M-Decomp (`fmt/printer.rs`, `lift/parser.rs`, `cli/explain.rs`)
  [Verified: those paths did not exist in the M-Decomp design's target layouts §5].
- Proposed: **23 file splits → ~40 new files** (22 in `src/` → ~34, plus
  `tests/differential.rs` → ~6, §8.1); **24 production do-not-split** justifications;
  8 `src/` inline-test companions left alone.

Risk classes used below:
- **mechanical** — pure text move, off the byte-identity spine (fmt/lift/cli/serve/lexer front
  half); failure mode is `use`-drift, caught by `cargo build`.
- **spine** — file participates in run≡runvm≡PHP identity (vm/compiler/interpreter/value/
  transpile) or is a front-end whose output feeds all backends (checker/parser/lexer); still
  moves-only, but every commit must pass the full differential with the PHP oracle on the 8.5
  floor.

Effort: S = one sitting/one commit; M = 2–4 commits (one cluster per commit); L = a wave.

---

## 1. Per-file specifications (files > 1000 first, descending)

### 1.1 `src/transpile/program.rs` — 2689 lines — SPLIT (spine, L)

Clusters [Verified: method list at lines 13–2689]:
- A. Free helpers + collect + program emission: `runtime_static_inits` 13, `main_entry_shape` 42,
  `main_bootstrap_stmt` 57, `class_has_restricted_ctor` 82, `collect` 93, `emit_program` 195,
  `emit_program_namespaced` 292 (~350 lines).
- B. **Runtime helper text**: `emit_runtime_helpers` 363→1192 (~830 lines, mostly PHP source
  strings gated on `self.uses_*` flags), `emit_json_helpers` 1192, `emit_reflect_table` 1309
  (~1000 lines total).
- C. Function emission: `emit_function`/`emit_function_named`/`emit_free_fn`/`emit_overload_set`/
  `overload_branch_test` 1358–1549 (~190).
- D. Type-declaration emission: `emit_enum` 1549, `emit_class` 1580, `emit_trait` 1676,
  `emit_class_members(_inner)` 1739/1753, `emit_synth_construct` 2024, `emit_decomposed_class`
  2065, `emit_multi_class` 2154, `emit_interface` 2456 (~900).
- E. Trait/MI plumbing: `build_trait_clauses` 2212, `build_use_trait_clauses` 2295,
  `mi_parent_aliases` 2385, `class_field_context` 2429; plus the parent-call scan free fns
  `collect_parent_method_calls`/`pc_block`/`pc_stmt`/`pc_expr` 2497–2689 (~450).

Proposed layout (all children of the existing `mod program` family inside `transpile/`):
```
src/transpile/
├── program.rs      # cluster A: collect + emit_program(+namespaced) + free helpers   (~350)
├── runtime.rs      # cluster B: emit_runtime_helpers / json helpers / reflect table  (~1000)
├── functions.rs    # cluster C: fn/overload-set emission                             (~190)
├── classes.rs      # cluster D: enum/class/trait/interface + members + synth/multi   (~900)
└── traits.rs       # cluster E: trait/use clauses + MI aliases + pc_* scan           (~450)
```
Visibility: cluster methods are already `pub(super)` [Verified: listing]; the private
`emit_json_helpers`/`emit_class_members_inner` and the bottom free fns become `pub(super)`
within `transpile`. `runtime.rs` at ~1000 lines is acceptable: it is homogeneous PHP text
(one concern, "the PHP runtime helper library"), and the alternative (per-helper files) shreds
a lookup table.
**Order-sensitivity guard**: invariant §3.6 (position-sensitive `emit_stmt` arms) lives in
`transpile/stmt.rs`, untouched here; but keep clusters D+E's emission *order* inside
`emit_program` unchanged — only bodies move. Risk: **spine**. Effort **L** (one cluster per
commit, PHP-oracle gate each).

### 1.2 `src/value.rs` — 2095 lines — SPLIT (spine, M) — convert to `value/` module

Clusters [Verified: structure dump]:
- A. Core types: `FnvHasher` 20, `ClassLayout` 53, `Value` 109, `ClosureData` 174, `Instance`
  202, `EnumVal` 248, `HKey` 257, `impl Value` 437–599 (display/as_display etc.),
  `const_literal` 599, fault `pub const`s 624–655 (~650).
- B. Container kernels: `iter_elements` 295, `build_map` 319, `build_set` 339, `map_index` 355,
  `list_set` 368, `map_set` 380, `set_nested` 396, `MAX_RANGE_LEN`/`build_range` 1409–1439 (~180).
- C. Scalar arithmetic kernels: `int_add..int_shr` 658–848, `float_add..float_rem` 849–884,
  `int_pow`/`float_pow` 1391–1408, `number_format` 745, `compare_ord` 1439 (~300).
- D. Decimal kernels: `dec_parts` 885 → `decimal_of` 1340 incl. `RoundMode`, `round_div`,
  `fmt_decimal` (~520).
- E. `mod tests` 1458–2095 (**637 lines of tests inline**).

Proposed layout:
```
src/value/
├── mod.rs          # cluster A + `pub use` re-exports of every kernel     (~660)
├── containers.rs   # cluster B                                            (~180)
├── arith.rs        # cluster C (int/float kernels, compare_ord)           (~300)
├── decimal.rs      # cluster D (RoundMode + all decimal_*)                (~520)
└── tests.rs        # cluster E                                            (~640)
```
**Invariant preservation**: INVARIANTS.md §3 says the kernels "live once, in `src/value.rs`" —
the split preserves the *single-sourcing* (one module, both backends still call
`crate::value::int_add` etc. — `mod.rs` `pub use`s every kernel so **zero caller churn**)
[Verified: kernels are free `pub fn`s, so `pub use containers::*;` keeps paths stable].
INVARIANTS.md §3 text must be updated to say `src/value/` in the same commit (doc-only).
Fault `pub const`s stay in `mod.rs` (they are the `agree_err` classification contract).
Risk: **spine** but the purest move in the whole plan. Effort **M**.

### 1.3 `src/checker/calls.rs` — 1981 lines — SPLIT (spine front-end, M)

M-Decomp §8 already anticipated exactly this split ("calls.rs may further split into
calls+members — decide when editing") [Verified: design doc §8]. Clusters [Verified: fn list]:
- A. Call checking core: `check_call` 33, `check_named_call` 142, `check_parent_call` 218,
  `check_parent_ctor_call` 342, `check_overload_call` 443, `check_method_sigs` 493,
  `discharge_call_throw` 568, `check_generic_call` 587, `unify` 633, `check_native_call` 674,
  `arg_is_secret_expose` 749, `check_arg`/`record_pending_fill`/`check_args_defaulted`/
  `check_args` 769–899, `try_variant_or_class_call` 899, `native_default_expr` 1971 (~1000).
- B. Concurrency call checks: `check_spawn` 1024, `is_channel_new` 1052,
  `check_concurrency_static` 1065, `check_concurrency_method` 1100 (~135).
- C. Member/method access: `check_method_call` 1160, `check_static_method_call` 1403,
  `check_member` 1452, `class_subst` 1622, `enforce_member_vis` 1641, `enforce_ctor_vis` 1684,
  `enum_subst` 1721 (~580).
- D. UFCS: `UfcsNav`/`UfcsSite` 14–32, `try_ufcs` 1744, `ufcs_first_accepts` 1842,
  `check_ufcs_call` 1853, `finish_ufcs` 1903 (~260).

Proposed layout (all stay `mod` children of `checker`, methods stay `pub(super)`):
```
src/checker/
├── calls.rs        # cluster A (+ B folded in: spawn/receive are call checks)  (~1140)
├── members.rs      # cluster C: method/static/member access + vis + substs     (~580)
└── ufcs.rs         # cluster D: UfcsNav/UfcsSite + the 4 UFCS fns              (~260)
```
`calls.rs` remains ~1140 — acceptable as the thin-dispatcher head for every call form; if a
further cut is wanted, `generics.rs` (`check_generic_call`+`unify`+`class_subst`/`enum_subst`,
~200) is the next natural seam (substs would move there from members). Risk: **spine
front-end** (checker output feeds all backends; UFCS rewrite map is span-keyed — see memory
`ufcs-and-interpolation-span-fix` — but moves don't touch spans). Effort **M**.

### 1.4 `src/checker/collect.rs` — 1892 lines — SPLIT (spine front-end, M)

Clusters [Verified: fn list]:
- A. Symbol collection: `php_erasure_key` 9, `overloads_erase_alike` 21, `prebind_types` 40,
  `collect` 87, `collect_trait` 154, `collect_interface` 173, `collect_function` 1059,
  `collect_enum` 1381, `collect_class` 1432–1860, `reserved_symbol_decl` 1860,
  `literal_ty` 1878 (~950).
- B. Inheritance/interface graph: `check_interface_graph` 252→734 (**one ~480-line fn**),
  `inherit_class_members` 734, `merge_inherited` 838, `iface_in_cycle(_to)` 931/1034,
  `iface_flat_methods` 960, `iface_collect_methods` 969, `sig_conforms` 995, `is_subtype` 1007,
  `ty_assignable` 1055 (~810).
- C. Signature validation: `collect_param_defaults` 1116, `reject_member_defaults` 1179,
  `reject_dup_param_names` 1197, `validate_new_overload` 1219, `mixed_overload_err` 1348,
  `validate_type_params` 1360 (~265).

Proposed layout:
```
src/checker/
├── collect.rs      # cluster A: prebind + per-item collectors                 (~950)
├── inherit.rs      # cluster B: interface graph, member inheritance, subtype  (~810)
└── signatures.rs   # cluster C: param defaults + overload/type-param checks   (~265)
```
Note `is_subtype`/`ty_assignable` are consumed widely — they stay `pub(super)` (already are).
**Smell flagged**: `check_interface_graph` is a single ~480-line fn — file-splitting does not
fix that; log it for a later (non-move) decomposition commit under the project's own >150-line
rule. Risk: **spine front-end**. Effort **M**.

### 1.5 `src/fmt/printer.rs` — 1443 lines — SPLIT (mechanical, M) — convert to `fmt/printer/`

Off the byte-identity spine (the formatter is gated by its own idempotency/meaning-preserving
tests, memory `phg-fmt-milestone`). Clusters [Verified: fn list]:
- A. Driver + item printing: `format_program` 28, `Printer` struct 40, `line`/comment flushing
  49–84, `program` 84, `item` 103, `interface` 134, `trait_decl` 150, `fn_signature` 166,
  `function` 191, `class` 222, `declare_class` 267, `member` 306, `enum_decl` 366, `params`/
  `ctor_params` 389–413 (~410).
- B. Statements: `stmt` 413, `try_stmt` 567, `destructure_pat` 602, `block_stmt` 627,
  `if_stmt` 642, `close_else` 698, `stmt_inline` 736, `stmt_inline_any` 977 (~430).
- C. Expressions: `expr` 758, `inline_block` 969, `str_lit` 1112, `operand` 1131,
  `postfix_operand` 1150, `pattern` 1159 (~330).
- D. Free atoms: `ty` 1198, `vis_str`/`modifiers_str` 1233–1265, precedence consts + `bin_prec`/
  `prec_of`/`binary_op`/`unary_op` 1265–1342, `item_start`/`stmt_start` 1342–1379, escape fns
  1379–1443 (~250).

Proposed layout (struct stays in `mod.rs`; children access private fields — Rust privacy is
module-and-descendants, the M-Decomp §3.3 mechanism):
```
src/fmt/printer/
├── mod.rs          # cluster A: format_program + Printer + item/decl printing (~420)
├── stmt.rs         # cluster B: impl Printer — statement printing             (~430)
├── expr.rs         # cluster C: impl Printer — expression/pattern printing    (~330)
└── atoms.rs        # cluster D: free fns — types, precedence, escapes         (~250)
```
Risk: **mechanical**. Effort **M**.

### 1.6 `src/cli/mod.rs` — 1442 lines — SPLIT (mechanical, S) — plus a structural smell

Clusters [Verified: fn list]:
- A. CLI surface: `version_line`/`help_text`/`help_for` 30–232, `SourceSpec`/`resolve_source*`
  245–300, `on_deep_stack` 300, `lex_parse`/`parse_program` 314–338 (~330).
- B. **Injected preludes** (structural smell — stdlib source text living in the command
  dispatcher): `JSON_PRELUDE` 338 → `inject_time_prelude` 914 — six `const` prelude strings +
  six `inject_*` fns (Json, RoundingMode, Http+respond bridge, Regex, Secret, Time) (~610).
- C. Orchestrators (invariant §3.8 — MUST stay in `mod.rs`): `check_and_expand` 949,
  `check_and_expand_reified` 958, `parse_checked*` 1022–1054, `foreign_runtime_gate` 1054,
  `check_and_expand_for_debug` 1073, `cmd_run`/`cmd_runvm`/`*_exit`/`cmd_check` 1079–1161,
  `run_program`/`runvm_program`/`*_exit`/`check_program`/`transpile_program`/`serve_program`/
  `cmd_build`/`cmd_parse`/`cmd_lex`/`cmd_lift`/`cmd_transpile` 1161–1362 (~420).
- D. Disassembler: `cmd_disasm` 1362, `annotate` 1373, `disasm_program` 1398 (~80).

Proposed layout:
```
src/cli/
├── mod.rs          # clusters A + C (orchestrators stay per invariant §3.8)   (~750)
├── preludes.rs     # cluster B: the 6 *_PRELUDE consts + inject_* fns         (~610)
└── disasm.rs       # cluster D: cmd_disasm + annotate + disasm_program        (~80)
```
The `inject_*` fns are called from `check_and_expand` — they become `pub(super)`. The prelude
extraction also fixes the name-mismatch smell: "cli/mod" currently contains a mini embedded
stdlib. (A future non-move improvement: preludes are conceptually `native/`-adjacent — the
injected-type pattern, memory `core-json-and-injected-types` — but relocation across module
boundaries is out of scope for a moves-only wave.) Risk: **mechanical** (preludes are consumed
via the single chokepoint; differential still gates). Effort **S**.

### 1.7 `src/lift/parser.rs` — 1312 lines — SPLIT (mechanical, S/M) — convert to `lift/parser/`

Off-spine (PHP→Phorj lift is Tier-1-gated by `lift/parser_tests.rs`). Clusters [Verified]:
- A. Token machinery + items: consts 22–47, `PParser` 47, `parse_php` 57, peek/eat/expect
  66–147, `parse_program`/`parse_item`/`parse_function`/`parse_class`/`parse_member`/
  `parse_method`/`parse_enum`/`parse_params`/`parse_type` 147–468 (~450).
- B. Statements: `parse_block`/`parse_body`/`parse_stmt`/`parse_if`/`parse_for`/`parse_foreach`
  468–648 (~180).
- C. Expressions: `parse_expr` 648 → `parse_match` 993, `infix_op`/`compound_op`/`is_lvalue`
  1034–1098 (~450).
- D. String interpolation: `parse_interp` 1098 → `is_php_access_chain` 1303 (~215).

Proposed layout:
```
src/lift/parser/
├── mod.rs          # cluster A (+ B folded: stmts are small)                  (~630)
├── exprs.rs        # cluster C                                                (~450)
└── interp.rs       # cluster D: PHP string-interpolation mini-parser          (~215)
```
Risk: **mechanical**. Effort **S/M**.

### 1.8 `src/cli/explain.rs` — 1308 lines — DO NOT SPLIT

One `pub fn explain_text` lookup table with **392 diagnostic-code arms** + `cmd_explain`
[Verified: `rg -c '^\s+"E-|^\s+"W-'` = 392]. Homogeneous table; the file name says exactly
what it contains; navigation is `grep E-CODE` → lands on the arm. Any cut (E-*/W-*, or by
subsystem) is an arbitrary boundary that makes "which file has my code?" a question where
today there is none. M-Decomp §5 already carved it out as a terminal unit ("explain
(168-arm table)"). It will keep growing with each new diagnostic — that is healthy growth of
a table, not entropy. Optional future (non-move): make it data (`&[(&str, &str)]`) — no need.

### 1.9 `src/lexer/mod.rs` — 1224 lines — SPLIT (spine front-end, S)

M-Decomp §5 already named this split (`lexer/{mod,scan}`, "lowest priority") — it is now
warranted. Clusters [Verified]:
- A. Core: `Lexer` struct + peek/bump/whitespace 8–59, `scan_number` 59, comments 213–248,
  `scan_ident` 782, `current_char` 802, `parse_decimal_literal` 816, `keyword` 828,
  entry fns `lex`/`lex_with_comments` 917+ (~530).
- B. String-family scanners: `scan_string` 248, `scan_text_block` 409, `scan_raw_string` 535,
  `scan_unicode_escape` 587, `scan_html` 654, `scan_bytes` 710, `hex_digit` 768,
  `dedent_block` 885 (~560).

Proposed layout:
```
src/lexer/
├── mod.rs          # cluster A: Lexer core, numbers/idents/keywords, entry    (~660 incl. tests mod decl)
└── scan.rs         # cluster B: impl Lexer — string/html/bytes/text-block     (~560)
```
Risk: **spine front-end** (feeds everything; purely a method move). Effort **S**.

### 1.10 `src/checker/expr.rs` — 1147 lines — SPLIT one cluster out (spine front-end, S)

Thin-dispatcher already applied (`check_expr_inner` match head + per-construct helpers).
One cleanly separable concern [Verified]: the cast machinery — `check_cast` 531,
`check_cast_primitive` 609, `record_cast_call` 723 (~215; the `as`-cast matrix, memory
`checked-as-cast-and-contextual-as`).
```
src/checker/
├── expr.rs         # dispatcher + unary/binary/instanceof/str/html/list/map/index/range/if/lambda (~930)
└── casts.rs        # check_cast + check_cast_primitive + record_cast_call     (~215)
```
Keep the `check_expr_inner` match whole (thin-dispatcher rule). Risk: **spine front-end**.
Effort **S**.

### 1.11 `src/compiler/expr.rs` — 1073 lines — SPLIT (spine, M)

M-Decomp §5 target listed `compiler/{…,binary,call,…}` — `call` never materialized. Clusters
[Verified]:
- A. Dispatcher + operands: `resolve_parent_target` 12, `parent_ret_cty` 47, `expr` 66 (297-line
  match — stays whole), `compile_str` 363, `compile_binary` 383 (~540).
- B. Call family: `compile_call` 552→782, `compile_safe_access` 782, `compile_force` 810,
  `compile_propagate` 830, `compile_intrinsic` 856, `compile_clone_with` 900 (~390).
- C. `compile_lambda` 943 (~130) — stays with A (touches the trailing function-table layout,
  memory `lambda-function-table-layout`; keep next to the dispatcher that references it).

Proposed layout:
```
src/compiler/
├── expr.rs         # clusters A + C                                           (~680)
└── call.rs         # cluster B: impl Compiler — call/safe-access/force/propagate/intrinsic/clone (~390)
```
**Scratch-slot discipline warning**: `compile_safe_access`/`compile_force`/`compile_propagate`
carry the `m_slot = self.height - 1` invariant (INVARIANTS §3.5 / memory `null-op-scratch-slot`)
— verbatim moves only, and the differential's two-ops-in-one-expression cases are the gate.
Risk: **spine**. Effort **M**.

### 1.12 `src/chunk.rs` — 1060 lines — SPLIT tests only (mechanical, S)

Design mandate: `chunk.rs` stays single — `validate` (487–654) next to `Op` (82–343) is the
load-bearing adjacency (INVARIANTS §5). But `mod tests` 654–1060 is **~405 lines** of inline
tests. Convert:
```
src/chunk/
├── mod.rs          # ConstKey/FaultMsg/Op/Chunk/descs/BytecodeProgram+validate (~655)
└── tests.rs        # the existing #[cfg(test)] mod, verbatim                   (~405)
```
Zero production-code movement; `Op` and `validate` remain co-located. (Matches the existing
`compiler/mod.rs → mod tests;` precedent [Verified: compiler/mod.rs:1053].) Risk: nominally
spine, in practice **mechanical**. Effort **S**.

### 1.13 `src/compiler/mod.rs` — 1053 lines — SPLIT (spine, M)

Clusters [Verified]:
- A. Data + entry: `NumTy` 30, `CTy` 50, `Local`/`FnMeta`/`VariantMeta`/`PathSeg`/`MatchBinding`
  82–135, `Compiler` struct 135, `TryCtx`/`LoopFrame` 240–263, `compile`/`compile_with` 263–292,
  small free helpers 292–335, `new` 425, `emit*`/jump-patching/scopes/locals 478–630 —
  **including `stack_effect` 488 (one of the three coupled Op matches — stays in `mod.rs`)**
  (~560).
- B. Type resolution: `resolve_cty` 335, `ty_to_cty` 389, `native_ret_cty` 407, `num_ty` 630,
  `ctype` 649, `ctype_normal` 679→914 (the big class-aware resolver), `as_num` 914 (~330).
- C. Static/hook/binding helpers: `field_name_index` 931, `static_slot`/`static_cty`/
  `const_value`/`const_cty` 942–1002, `hook_get_method`/`hook_set_method` 1002–1026,
  `resolve_binding`/`emit_path`/`emit_load_path` 1026–1053 (~120).

Proposed layout (M-Decomp §5 target already listed `compiler/types`):
```
src/compiler/
├── mod.rs          # cluster A (struct, stack_effect, emit/scope machinery) + C (~680)
└── types.rs        # cluster B: NumTy/CTy resolution — resolve_cty/ctype/num_ty (~330)
```
(`CTy`/`NumTy` enum *definitions* stay in `mod.rs` — they are fields of `Compiler`-adjacent
structs; only the resolver fns move.) Risk: **spine** (CTy resolution is the operand-
specialization brain — memory `cty-tracks-operand-types`); moves-only + differential gate.
Effort **M**.

### 1.14 `src/interpreter/mod.rs` — 1002 lines — SPLIT (spine, S)

Clusters [Verified]:
- A. Frame + entry: `Signal` 21, helpers 46–109, `CallScopes` 109–169, `Interp` 169, entry fns
  `interpret*` 266–287, `run_program_main` 287, misc 378–412, `call_named` 412, existing `mod`
  decls 478–486, `collect` 491, `eval_static_inits` 641, `run_call` 673, debug/fault plumbing
  738–787 (~780).
- B. Operator wrappers + pattern matching: `arith` 787, `bitwise` 863, `compare` 888 (thin
  shims over the `value::` kernels), `match_pattern` 913–1002 (~215).

Proposed layout:
```
src/interpreter/
├── mod.rs          # cluster A (struct, entry, collect, run_call)             (~790)
└── ops.rs          # cluster B: arith/bitwise/compare shims + match_pattern   (~215)
```
Risk: **spine** — but the shims delegate to `value::` kernels (INVARIANTS §3), so moves cannot
change semantics; differential gates. Effort **S**.

---

## 2. 500–1000 band — splits worth doing

### 2.1 `src/checker/program.rs` — 977 — SPLIT (spine front-end, S)
Two concerns [Verified]: the program/type-body/function driver (9–716) and the **totality
engine** (`block_terminates` 717, `stmt_terminates` 721, `block_assigns_field` 787,
`stmt_diverges_no_return` 821, `expr_is_never` 851, `check_return_totality` 883, `check_body`
921, `guard_if_narrowings` 956, `check_block` 972 — the M-RT totality cluster, ~260).
→ `checker/program.rs` (~715) + **`checker/totality.rs`** (~260). Name telegraphs the feature.

### 2.2 `src/parser/items.rs` — 950 — SPLIT (spine front-end, S)
Clusters [Verified]: general items (`parse_item`/`parse_program`/package/import/alias/test/
function/attributes/params 5–502, plus `parse_declare(_class)` 284–433) vs the class-shaped
family (`parse_enum` 502, `parse_class` 541, `parse_use_traits` 620, `parse_trait` 639,
`parse_resolution` 657, `parse_interface` 699, `parse_name_list` 764, `parse_class_member` 775,
`parse_property_hook` 846, `parse_modifiers` 899, `parse_ctor_params` 925 — ~450).
→ `parser/items.rs` (~500) + **`parser/classes.rs`** (~450). Soft-dispatch cross-reference
comment at both heads (M-Decomp §5 parser note).

### 2.3 `src/serve.rs` — 836 — SPLIT (mechanical, S) — convert to `serve/`
Already quarantined off-spine by design (M6). Clusters [Verified]: core (`Transport` trait,
`serve`, `respond_once`, error pages 39–214) / TCP transport (`TcpTransport` 214–373,
`read_http_request` 571, `request_wants_keepalive` 615) / pool (`serve_banner` 373,
`serve_tcp_pool`/`serve_pool(_with)` 401–500, `worker_loop` 500).
→ `serve/mod.rs` (~240) + **`serve/tcp.rs`** (~330) + **`serve/pool.rs`** (~230). The green-
threads work (marathon A1) keeps touching pool/worker code — this split shrinks that blast
radius.

### 2.4 `src/lift/lifter.rs` — 885 — SPLIT (mechanical, S)
Natural seam at the impl/free-fn boundary [Verified]: `Lifter` impl (items/members/stmts,
33–477) vs the free expr-lowering family (`lift_expr` 477 → `named` 879, ~410).
→ `lift/lifter.rs` (~475) + **`lift/exprs.rs`** (~410).

### 2.5 `src/checker/stmt.rs` — 916 — SPLIT optional (spine front-end, S)
`check_stmt` match head 32–473 stays whole. The loop family (`check_for` 771, `bind_loop_var`
844, `check_while` 862, `check_cfor` 884, ~145) plus narrowing (`check_block_narrowed` 666,
`narrow_from_condition` 690, ~105) are separable → **`checker/loops.rs`** (~250 combined with
narrowing staying put) — *optional*; below the pain threshold. Defer unless it regrows past
1000.

### 2.6 `src/interpreter/call.rs` — 588 / `src/transpile/expr.rs` — 681 / `src/compiler/stmt.rs` — 817 / `src/parser/{stmts,exprs}.rs` — 767/762
All are M-Decomp products, single-concern, thin-dispatcher heads with their arm helpers.
**Do not split** — each is exactly one phase×construct-family; further cuts would separate a
match head from its arms.

---

## 3. Do-not-split list (production files > 500, with justification)

| File | Lines | Justification |
|---|---|---|
| `cli/explain.rs` | 1308 | 392-arm homogeneous code→text table; grep-navigable; any cut is arbitrary (§1.8) |
| `checker/mod.rs` | 802 | Design-mandated home of `Checker` struct + info structs + diag/scope primitives (M-Decomp §4); children need its private fields |
| `vm/exec.rs` | 811 | One `exec_op` exhaustive match — design mandates keep whole (M-Decomp §5); splitting forks the dispatch loop (INVARIANTS §3.5) |
| `native/list.rs` | 814 | Flat registry of ~40 tiny native fns + one factory; one concern ("Core.List natives"); the per-module-factory shape IS the design (M-Decomp §5) |
| `native/text.rs` | 773 | Same registry shape as list.rs |
| `native/math.rs` | 559 | Same registry shape |
| `transpile/mod.rs` | 904 | Orchestrator + `Transpiler` struct + PHP naming atoms; design home (M-Decomp §4/§5); `emit` entry keeps pass order (INVARIANTS §3.8) |
| `ast/mod.rs` | 923 | Pure data dictionary (Type/Pattern/Expr/Stmt/decl structs); navigation is by type name via goto-def; the M-Decomp "optional data split" stays optional — splitting data defs adds import churn, zero cohesion gain |
| `ast/classes.rs` | 968 | One concern: derived class-shape tables (implements/supertypes/MRO/layout/consts/ctor_plan) — the shared oracle consumed verbatim by checker+backends; splitting would scatter a single contract |
| `ast/walk.rs` | 607 | Exhaustive free-var/uses-concurrency walkers — exhaustiveness safety net, keep whole |
| `loader/resolve.rs` | 648 | Design-mandated: "keep exhaustive walk whole" (M-Decomp §5) — it is the mangle/rewrite pass |
| `loader/mod.rs` | 625 | `load_project` sequencing is pass-ordering (INVARIANTS §3.8) |
| `lift/printer.rs` | 829 | Cohesive Phorj-source printer, off-spine, stable |
| `lsp/mod.rs` | 680 | Already has `scope`/`symbols`/`tests` children; `Server::handle` + per-request handlers is one concern |
| `manifest.rs` | 663 | Self-contained TOML-subset parser + `Project::detect`; single concern, heavily unit-tested in place |
| `checker/rewrite_generics.rs` | 590 | One erasure pass, one exhaustive walk — keep whole (same rule as loader/resolve) |
| `checker/overloads.rs` | 590 | One feature (return-overload resolution + finalization) |
| `checker/assign.rs` | 548 | One concern (assignment/place checking + propagate/force) |
| `types.rs` | 551 | `Ty` + assignability algebra + its tests; the checker's core vocabulary — splitting `assignable_with` from `Ty` hides the contract |
| `interpreter/call.rs`, `transpile/expr.rs`, `compiler/stmt.rs`, `parser/stmts.rs`, `parser/exprs.rs` | 588–817 | M-Decomp products; thin-dispatcher heads + their arms (§2.6) |
| `main.rs` | 543 | See smell #1 below — needs fn decomposition (not a move), defer |
| All 8 test files (`cli/tests` 686, `lexer/tests` 660, `parser/tests/stmts` 603, `lift/parser_tests` 559, `transpile/tests` 544, `native/list_tests` 540, `loader/tests` 534, `parser/tests/items` 523) | | Leaf files, already mirror source clusters (W3.1b convention [Verified: header of parser/tests/stmts.rs]); no navigation pain — tests are found by test name |

---

## 4. Structural smells beyond size

1. **`main.rs::main()` is a ~510-line fn** (13–524) [Verified: only 3 top-level fns in the
   file]. It is the argv dispatcher for every command. File-splitting doesn't apply (one fn);
   the fix is extracting per-command arms into `cli::` fns — behavior-preserving but **not** a
   pure text move, so it belongs to a separate, later commit class. P3.
2. **`checker/collect.rs::check_interface_graph` is one ~480-line fn** (252–734) [Verified:
   next fn at 734]. Same class as #1 — log to KNOWN_ISSUES/backlog, don't fold into the move
   wave (M-Decomp §7 scope-creep rule).
3. **`compiler/program.rs::compile_program_with` is a ~667-line fn** (13–680) [Verified: next
   fn at 680]. Same class. The file itself (861) has no other seam — do-not-split as a file.
4. **Stdlib preludes live in `cli/mod.rs`** — name/content mismatch; fixed by the §1.6
   `cli/preludes.rs` extraction (move-only). The deeper home question (native/-adjacent?) is
   deferred.
5. **Inline `mod tests` bloat inside production files**: `value.rs` (637 test lines),
   `chunk.rs` (~405), `types.rs` (~220), `ast/walk.rs` (~47). The first two are fixed above;
   adopt the standing rule (§6) so new inline test mods > 150 lines move to a sibling file.
6. **`checker/` is a 20-child directory** [Verified: mod decls in checker/mod.rs] and this spec
   adds 6 more (members, ufcs, inherit, signatures, totality, casts). That is still the right
   shape (rustc's `rustc_hir_analysis` has more), but the `rewrite_*` passes (6 files) could
   later group under `checker/rewrites/` for directory legibility — cosmetic, optional, not
   part of this wave.
7. **No cyclic-coupling findings**: the phase pipeline (lexer→parser→checker→{interpreter,
   compiler→vm, transpile}) has clean one-way deps; shared vocabulary lives in `ast`/`types`/
   `value`/`chunk` as designed [Inferred: from the module map + no `use` inversions observed
   in the structure dumps; a full `cargo modules` graph was not run].

---

## 5. Global execution order (safest first, one commit per cluster move)

Gate for every commit: `cargo build` → `clippy --all-targets` → `fmt --check` → full
`PHORJ_PHP=<8.5> PHORJ_REQUIRE_PHP=1 cargo test`. Spine waves additionally re-run `phg bench`
spot-check only if any hot-path file moved (none should change perf — moves only).

**Wave 1 — mechanical, off-spine (warm-up):**
0. `tests/differential.rs` → `tests/differential/{main,milestones,features,errors_traits,php_oracle,runtime}.rs`
   (§8.1) — M; gate = test-count parity (126) + full green with `PHORJ_REQUIRE_PHP=1`
1. `chunk.rs` → `chunk/{mod,tests}.rs` (§1.12) — S
2. `cli/mod.rs` → `+ preludes.rs, disasm.rs` (§1.6) — S
3. `serve.rs` → `serve/{mod,tcp,pool}.rs` (§2.3) — S
4. `lift/parser.rs` → `lift/parser/{mod,exprs,interp}.rs` (§1.7) — S/M
5. `lift/lifter.rs` → `+ lift/exprs.rs` (§2.4) — S
6. `fmt/printer.rs` → `fmt/printer/{mod,stmt,expr,atoms}.rs` (§1.5) — M

**Wave 2 — front-end (checker/parser/lexer; differential-gated, no runtime semantics):**
7. `lexer/mod.rs` → `+ scan.rs` (§1.9) — S
8. `parser/items.rs` → `+ parser/classes.rs` (§2.2) — S
9. `checker/program.rs` → `+ checker/totality.rs` (§2.1) — S
10. `checker/expr.rs` → `+ checker/casts.rs` (§1.10) — S
11. `checker/calls.rs` → `+ members.rs, ufcs.rs` (§1.3) — M
12. `checker/collect.rs` → `+ inherit.rs, signatures.rs` (§1.4) — M

**Wave 3 — spine (backends + value + transpile; strictest gate):**
13. `value.rs` → `value/{mod,containers,arith,decimal,tests}.rs` (§1.2) — M
14. `interpreter/mod.rs` → `+ ops.rs` (§1.14) — S
15. `compiler/mod.rs` → `+ compiler/types.rs` (§1.13) — M
16. `compiler/expr.rs` → `+ compiler/call.rs` (§1.11) — M
17. `transpile/program.rs` → `{program,runtime,functions,classes,traits}.rs` (§1.1) — L
    (one cluster per commit; PHP oracle each)

Post-wave-3: the §3.2 exhaustiveness smoke test (add a dummy `Op` variant → must fail to
compile in all three coupled sites) + `cargo build --release` (standing rule).

**The 3 riskiest splits**: §1.1 `transpile/program.rs` (position-sensitive emission order +
1000 lines of PHP helper text where a one-character drift breaks the PHP leg), §1.2 `value.rs`
(the single-sourcing invariant itself; mitigated by `pub use` re-export = zero caller churn),
§1.11 `compiler/expr.rs` (scratch-slot `self.height - 1` discipline rides along in
safe-access/force/propagate).

## 6. Proposed standing rule (stop the regrowth)

Adopt in `docs/INVARIANTS.md` (or CONTRIBUTING) — the M-Decomp design set no threshold, which
is why the whales regrew unnoticed:

1. **Soft cap 800 production lines per file** (inline `#[cfg(test)]` mods excluded from the
   count). Crossing it is a prompt, not a violation.
2. **Hard review trigger at 1000**: the PR/commit that crosses 1000 must either split the file
   (moves-only, same rules as this spec) or add a one-line justification to a tracked
   exemption list (`vm/exec.rs`, `cli/explain.rs`, `transpile/runtime.rs` are the standing
   exemptions — single exhaustive match / homogeneous table).
3. **Cohesion test, not just line count**: a file passes if its content can be named in ≤3
   words such that *every* top-level item fits the name ("Core.List natives", "decimal
   kernels", "UFCS checking"). If a second name is needed for a subset, that subset is the
   split seam.
4. **Inline test mods > 150 lines** move to a sibling `tests.rs` (the `compiler/mod.rs`
   precedent).
5. **Enforcement**: extend the existing CI gate (`scripts/perf-gate.sh` precedent) with a
   ~15-line `scripts/size-gate.sh` — warn > 800, fail > 1000 unless the path is in the
   exemption list. [Speculative: mechanism choice; the numbers above are calibrated to this
   census — 800 keeps every do-not-split file legal, 1000 catches exactly the current whales.]

## 7. Tally (src/ core deliverable)

- Files > 500 in `src/`: **52** (44 production + 8 tests).
- Splits specified: **22 files → ~34 new files** (17 primary + 5 optional/deferred seams noted).
- Do-not-split: **22 production files** justified (§3) + 8 test files.
- Everything is moves-only; the differential harness + PHP oracle is the correctness gate;
  the three coupled `Op` matches never leave their files.

---

# Repo-wide perimeter (scope update)

## 8. `tests/` — the top-level integration crates

Census [Verified: `find . -name '*.rs' -not -path './target/*' -not -path './src/*'
-not -path '*/vendor/*' | xargs wc -l`]: `tests/differential.rs` **2966**, `tests/serve.rs` 564,
`tests/cli.rs` 547, then build.rs 444, project.rs 411, conformance.rs 222, and 15 more under
250. `playground/src/lib.rs` is 309 (fine). Root `build.rs` is 37.

### 8.1 `tests/differential.rs` — 2966 lines, 126 `#[test]`s — SPLIT (mechanical*, M)

This is the **largest file in the repository** and the correctness spine's own harness.
Cargo supports directory-form integration tests: `tests/differential/main.rs` + `mod` children
compile to the **same single test binary** — no compile-time or invocation change
[Verified: standard Cargo behavior; the crate uses the same pattern inside `src/` already].
Clusters [Verified: full fn dump, lines 17–2961]:
- A. **Harness kernel**: `check_errs` 17, `transpile_ok` 31, `with_pkg` 42, `agree` 50,
  `classify`/`FaultKind` 111, `agree_err` 148, `agree_out_php` 392, `uses_impure_native` 930,
  the example/project walkers `collect_phg`/`collect_projects`/`find_main_phg` 946–1017, and
  the PHP-oracle plumbing `php_bin`/`php_or_gate`/`php_n_args`/`run_php` 1921–2006 (~350).
- B. Milestone program tables + fixed suites: `P2_PROGRAMS` 165, S6 inheritance 223–557,
  `P3_PROGRAMS` 705, `P4A/B/C_PROGRAMS` 757–930, `WAVE4_PROGRAMS` 1101, `ERR_PROGRAMS` 1140
  (~700).
- C. Language-feature suites: S0–S2 557–699 + 1211–1337, lambdas/pipe/first-class 1337–1400 +
  1563–1630, mutation 1396–1563, generics/overloads 1630–1826, html 1826–1921 (~700).
- D. Error-model + traits + patterns: faults/throw/catch/finally 2275–2447, S8 traits
  2447–2576, pattern cluster 2576–2837 (~560).
- E. PHP-oracle gated end-to-end: exit codes, decomposition-vs-PHP, `all_examples_transpile_
  and_match_php`, M7 emitter checks 2006–2275 (~270).
- F. Decimal + concurrency: 2837–2966 (~130).

Proposed layout (one test binary, unchanged name → `cargo test --test differential` still works):
```
tests/differential/
├── main.rs         # cluster A: harness kernel + FaultKind + walkers + php plumbing (~360)
├── milestones.rs   # cluster B: P2/P3/P4/WAVE4/ERR program tables + their runners    (~700)
├── features.rs     # cluster C: S0–S2, lambdas, mutation, generics/overloads, html   (~700)
├── errors_traits.rs# cluster D: error model, S8 traits, pattern cluster              (~560)
├── php_oracle.rs   # cluster E: exit codes + examples-vs-PHP + M7 emitter            (~270)
└── runtime.rs      # cluster F: decimal faults + concurrency (m6w4)                  (~130)
```
Helpers in `main.rs` become `pub(crate)` within the test crate. **Verification twist**: the
harness is the gate, so the gate for *this* split is test-count parity — `cargo test --test
differential -- --list | wc -l` must report the same 126 tests before and after, plus a full
green run with `PHORJ_REQUIRE_PHP=1`. (*mechanical: zero production code moves; the only risk
is dropping a test on the floor, which the count-parity check catches.) Effort **M**.
Slot into **Wave 1** (it de-risks every later wave by making the gate itself navigable).

### 8.2 `tests/serve.rs` (564) and `tests/cli.rs` (547) — DO NOT SPLIT
Each is one concern (serve integration: 14 tests; CLI surface: 27 tests) [Verified: `#[test]`
counts]; both sit just over threshold and are leaf files navigated by test name. Revisit only
past ~800.

## 9. Non-Rust code — no god files, three hygiene flags

Census [Verified: `wc -l` over tools/, scripts/, playground/, editors/ excluding
node_modules/pkg/dist]:

| Area | Largest files | Verdict |
|---|---|---|
| `tools/` | `return_type_codemod.py` 142, `core_rename.py` 134, `core_rename2.py` 60 | No split needed |
| `scripts/` | `perf-gate.sh` 55 | Fine |
| `playground/web/` | `main.js` 428, `style.css` 137, `examples.js` 113, `worker.js` 35 | Fine — `main.js` is one editor-page controller, under any threshold |
| `editors/` | `vscode/extension.js` 28, tmLanguage 109 | Fine |

Flags (name/content hygiene, not size):
1. **`tools/core_rename.py` + `tools/core_rename2.py`** — a numbered-suffix pair is the
   codemod version of a god file smell: the name doesn't say what differs. Both are retired
   one-shot codemods (Core rename, 2026-06-20/23); per the one-shot-migration doctrine they
   are stale-and-potentially-harmful if rerun. Recommend: delete, or move under
   `tools/archive/` with a README line each [Inferred: CLAUDE.md marks the rename complete;
   the scripts' job is done].
2. **`dist/`** holds git-ignored stale binaries still named `phorge-v0.3/v0.4` (pre-rename)
   plus `hello-*` cross-build outputs (~60 MB) [Verified: `git check-ignore` lists them as
   ignored; `git ls-files dist/` is empty]. Local clutter only — flag for manual cleanup,
   not a repo change.
3. **`scripts/` vs `tools/` boundary** is currently "sh vs py", not purpose. Minor; note for
   the standing rule discussion, no action proposed.

## 10. `docs/` — filename↔content check (no size-based splitting, per scope rules)

- **No stale-name drift found in living docs**: `rg -ci phorge` = 0 across README, FEATURES,
  ROADMAP, KNOWN_ISSUES, VISION, STABILITY, docs/ARCHITECTURE, docs/INVARIANTS,
  docs/MILESTONES [Verified: ran the scan]. Titles match filenames for all five top-level
  `docs/*.md` [Verified: headers read].
- `docs/plans/` (66 files) and `docs/specs/` (81 files) use dated frozen-record names — that
  IS the convention (records of decisions at a point in time), not a mismatch. Historical
  "Phorge" inside old specs is legitimate.
- **`CHANGELOG.md` (2719 lines / 205 KB)** — append-only log, name matches content; fine.
- **`KNOWN_ISSUES.md` (1125 lines / 94 KB)** — borderline: the name promises *current* known
  issues, and at 94 KB it reads as an archive. Only 3 "FIXED/RESOLVED" markers found
  [Verified: rg count], so it is mostly live — but memory records at least one entry as
  "Former gap now FIXED" being annotated in place rather than removed. Recommend a periodic
  prune pass (move resolved entries to CHANGELOG) so the filename stays honest. [Inferred]
- One INVARIANTS.md edit is *required* by this spec: §3's "live once, in `src/value.rs`"
  wording must become `src/value/` in the same commit as split §1.2.

## 11. `examples/` — brochure-readability check

Census [Verified: `wc -l` over `examples/**/*.phg`, vendor excluded]: 114 guide examples;
largest are `guide/pattern-matching.phg` **186**, `web/server.phg` **160**,
`web/handler.phg` 129, then ≤99. Median well under 80.

- **No example is broken as a brochure**, but two are at the edge:
  - `guide/pattern-matching.phg` (186) — heavily commented walkthrough covering guards +
    destructuring + narrowing in one file [Verified: header read]. It scrolls. Candidate:
    split into `pattern-guards.phg` / `destructuring.phg` (each self-contained, each still
    auto-gated by the differential glob). [Speculative: whether one survey file or two focused
    ones showcases better is a developer taste call.]
  - `web/server.phg` (160) — a full server is inherently longer; acceptable as the capstone
    web example. No action.
- **`examples/README.md` is 71 KB** — the index + coverage matrix has outgrown "brochure".
  A reader looking for one example scrolls a 71 KB wall. Recommend: thin root index (per-area
  tables of name + one-liner) + per-directory `README.md`s carrying the detail. This is the
  strongest examples-area finding. [Verified: 71 KB on disk; recommendation Speculative in
  form, the size is not.]
- Proposed standing rule for examples: soft cap **~150 lines** per guide example; a feature
  needing more gets a directory with a README walkthrough + companion programs (the existing
  `examples/build/`, `examples/cli/` precedent).

## 12. Repo-wide tally (final)

- Files > 500 lines, repo-wide, human-authored: **55** (52 in `src/` + 3 in `tests/`).
- Splits specified: **23 files → ~40 new files**; riskiest three unchanged (§5) —
  `tests/differential.rs` joins Wave 1 as the safest large win.
- Do-not-split: 24 production files (22 `src/` + `tests/serve.rs` + `tests/cli.rs`) + the 8
  `src/` test companions.
- Non-Rust: no god files; 3 hygiene flags (§9). Docs: no name/content mismatch in living
  docs; 1 required INVARIANTS wording update + KNOWN_ISSUES prune suggestion (§10).
  Examples: 2 borderline programs + the 71 KB README as the real finding (§11).
