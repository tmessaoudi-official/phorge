# Nested-value index-assignment — design

> Status: designed, not implemented. M-DOGFOOD follow-on (surfaced by porting `benchforge`).
> Goal: allow assignment into a nested *place* — `this.f[i]=e`, `obj.f[i]=e`, `m[k1][k2]=e`,
> `grid[i][j]=e` — not just a bare local `xs[i]=e`. Unblocks the Matrix benchmark and general
> nested-container mutation, keeping value semantics (COW) intact. Byte-identical `run≡runvm≡real PHP`.

## Problem

Today a value-type element set (`M-mut.5`) requires the container to be a **simple local variable**:
the checker rejects any other target with `E-ASSIGN-TARGET` ("the container of an element assignment
must be a simple variable"). So `this.data[i]=e` and `grid[i][j]=e` are compile errors. Field *paths*
(`a.b.c=e`) work (handle semantics), and `map[k].field=e` works (the retrieved instance is a shared
handle) — but an **indexed value-container** target does not. This blocks in-place matrix/2-D algorithms
and field-held-collection mutation.

## Model: a place is a base + a chain of steps, made mutable root-to-leaf

An assignment target is a **place expression**: a base binding, then a sequence of steps
(`.field` | `[index]`), ending in a settable step. Assignment navigates to the innermost container,
obtaining a mutable reference, and sets the final element/field. Two invariants make this sound under
Phorj's memory model:

- **Instances are shared-mutable handles** — navigating through `.field` on an instance mutates the
  shared instance in place (propagates through every binding). No copy.
- **Lists/Maps are value-type (COW)** — a nested value container is made unique with `Rc::make_mut`
  **at each level, root-first**. After `make_mut` on the outer container (in its slot), the inner `Rc`
  is uniquely held, so `make_mut` on it is in-place too. COW is preserved: a genuinely shared level
  still copies (correctly). The **root** must be mutated in its slot (as `Op::SetIndexLocal` already
  does for the flat case) so the outer `make_mut` sees refcount 1 — otherwise the whole chain copies.

Supported targets (all forms of `base (.field | [index])* <final settable step>`):

| Target | Base | Steps | Final |
|---|---|---|---|
| `xs[i]=e` | local | — | `[i]` (exists today) |
| `this.f[i]=e` / `obj.f[i]=e` | field of instance | `.f` | `[i]` |
| `m[k1][k2]=e` / `grid[i][j]=e` | local | `[k1]` | `[k2]` |
| `a.b.c[i]=e` | field chain | `.b .c` | `[i]` |
| `obj.f[i].g=e` | mixed | `.f [i]` | `.g` (field set on a retrieved instance) |

## Surfaces

- **Parser** — no change. These targets already parse (`Expr::Index{ object: Expr::Member{…} }`,
  nested `Expr::Index`). Only the checker rejects them.
- **Checker** — generalize `check_index_assign` / `check_field_assign` into a **place walker**:
  recursively type the base and each step, verify each index step's container is a `List`/`Map`
  (element/key types), each field step resolves on an instance/interface, the final assigned value is
  assignable to the slot type, and the **root binding is `mutable`** (a shared-instance base is always
  mutable — a field write goes through the handle). New/kept codes: `E-ASSIGN-TARGET` narrows to
  genuinely-illegal bases (call results, literals); add `E-ASSIGN-PATH-TYPE` for a mid-path type
  mismatch. The nested-place deferral note in `KNOWN_ISSUES.md` is removed.
- **Interpreter** — a recursive lvalue eval: resolve the base to a `&mut Value` (local slot via
  `lookup_mut`, or an instance field via `borrow_mut`), then for each step descend with `Rc::make_mut`
  on value containers / field access on instances, and set the final element via the shared
  `value::list_set`/`map_set` kernels. Eval order: **all index expressions left-to-right, then the RHS
  value**, matching the flat case + the VM.
- **VM** — one new op **`Op::SetPath(PlaceDesc)`** carrying an inline path descriptor: the root
  (local slot **or** an "eval base off the stack" marker for a non-local instance base) + an ordered
  list of steps `Field(name_idx)` | `Index`. The compiler emits the base (if non-local) + each dynamic
  index value + the RHS value, then `SetPath`. `exec_op` pops the RHS + index values, navigates the
  root **in place** (`make_mut` per value-container level, `borrow_mut` per instance field), and sets —
  never placing a mutable reference on the value stack. Extends the three coupled matches
  (`exec_op` + `chunk::validate` + `stack_effect`), per the Op-coupling rule. Reuses `list_set`/`map_set`.

## Scope / decisions

- **Depth is general** (arbitrary `.field`/`[index]` chain) — a single descriptor handles depth-2
  (matrix) and deeper uniformly; no artificial depth cap beyond the existing recursion guard.
- **COW preserved, no new `Value`** — only navigation changes; the kernels and value reps are untouched.
- **Root mutability enforced** — an immutable local root is `E-ASSIGN-IMMUTABLE` (as today); a
  shared-instance field write follows handle semantics (the field itself must be `mutable`).
- **Out of scope:** compound-assign on a deep path (`grid[i][j] += 1`) rides the same place walker once
  the base lands (desugars to read-path + set-path) — include if cheap, else a fast follow.

## Testing / byte-identity

- Checker acceptance/rejection tests (each new target form + `E-ASSIGN-PATH-TYPE`, immutable root).
- Differential `agree()` cases: `grid[i][j]=e` (matrix fill + read-back), `this.f[i]=e` (field list
  mutate), `m[k1][k2]=e` (nested map) — all `run≡runvm`, plus PHP-oracle (transpiles to PHP `$g[$i][$j]=…`
  / `$this->f[$i]=…`, byte-identical).
- A `guide/nested-assign.phg` example (byte-identity-gated) + the benchforge Matrix benchmark ported as
  the real-world proof.
