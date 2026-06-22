# generic-enums Plan (M-RT)

`Option<T>` / `Result<T,E>` — TypeScript-style generic enums, reified in the checker, erased before
any backend. Mirrors the shipped generic-CLASS machinery (`Box<T>`); **no new `Op`, no `Value` change**;
byte-identical `run ≡ runvm ≡ real PHP` by construction (type args are checker-only `Ty`, `EnumDecl`
type params are erased pre-backend).

## Decisions Log
- [2026-06-22] AGREED: next slice = generic enums (developer asked "what do you recommend & why"; I
  recommended enums-first because Error-model Slice 2's `Result<T,E>` IS a generic enum → enums unblock
  it; lowest-risk/highest-leverage; closes a standing KNOWN_ISSUES deferral). Developer: "Yes — build it,
  fully autonomous."
- [2026-06-22] AGREED: scope mirrors generic classes — `package main` only; inference-only construction
  (no `Option<int>(…)` explicit-arg construction); invariant; no bounds; un-inferred params default to
  `Ty::Error` (permissive, like generic-class ctors). Generic *enum methods* N/A (enums have no methods).

## Formal Plan

### Files & ordered steps
1. **`src/ast.rs`** — add `type_params: Vec<String>` to `EnumDecl` (mirror `ClassDecl`); doc it.
2. **`src/parser.rs`** `parse_enum` — parse optional `<T, …>` after the enum name via the existing
   `parse_type_params()`; set the field.
3. **`src/checker.rs`**
   - `EnumInfo` — add `type_params: Vec<String>`.
   - `collect_enum` — `validate_type_params`; store params; set `active_type_params` while resolving
     each variant's field types so a bare `T` → `Ty::Param("T")`; clear after.
   - `resolve_type` (~426) — replace the enum `no_args` branch with arity-aware resolution mirroring
     the class branch (`Option<int>` ⇒ `Ty::Named("Option",[Int])`; arity mismatch → error;
     non-generic enum still takes no args).
   - `try_variant_or_class_call` (~2767) — when the owning enum is generic, infer its type args by
     unifying the variant's declared field types against the call args (first-binding-wins `unify`),
     emit `Ty::Named(enum, inst_args)`; un-inferred → `Ty::Error`.
   - `check_match` `Pattern::Variant` (~3718) — build `enum_subst` from the scrutinee's type args and
     `apply_subst` over the variant field types before binding (so `Some(n)` over `Option<int>` binds
     `n: int`). New helper `enum_subst` (mirror `class_subst`).
   - `erase_generics` (items loop ~4747) — new `Item::Enum(e) if !e.type_params.is_empty()` arm:
     erase the enum's params across every variant field, clear `type_params`.
   - `expand_aliases` (~5088) — carry `type_params: e.type_params.clone()`.
4. **No backend change** — interpreter/vm/compiler/transpile/loader read only `e.name`/`e.variants`.
5. **`examples/guide/generic-enums.phg`** + `examples/README.md` entry — `Option<T>` + `Result<T,E>`:
   inferring construction (`Some(7)`), annotated construction for non-inferring variants
   (`Option<int> n = None();`), `match` with concrete-typed payload binding. Must print `Ok` and run
   byte-identically (auto-gated by the `examples/**/*.phg` glob).

### Acceptance
- `cargo test` green (lib + differential PHP-oracle + integration); add focused checker tests
  (infer at construction; match binds concrete; arity error; non-generic enum unchanged; erase strips
  params) + a differential case (`examples/guide/generic-enums.phg`).
- `cargo clippy --all-targets` + `cargo fmt --check` clean.
- `PHORGE_REQUIRE_PHP=1 cargo test` — real PHP agrees.
- New diagnostics reuse `E-GENERIC-PARAM`; no new codes expected.

### Rollback
Single self-contained commit; `git revert` if needed. No data/migration.

### Known limitation (carry to KNOWN_ISSUES)
A generic-typed *result* erases to `mixed` (`CTy::Other`) ⇒ not a specialized VM arithmetic operand
(pre-existing since S7a). Match-bound payloads are concretely typed (via `enum_subst`), so `n + 1` on a
bound `Some(n): int` is fine; only a raw generic-fn result needs binding to a typed local first.
