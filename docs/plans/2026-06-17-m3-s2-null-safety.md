# M3 S2 — Null-safety Implementation Plan

> **For agentic workers:** implement task-by-task, TDD, one green commit per task. Steps use
> checkbox (`- [ ]`) syntax. Execution mode for this run: **autonomous inline** (developer chose
> "plan + build all of S2"); gates run internally, only destructive actions pause.

**Goal:** Add PHP-native null-safety to Phorge — `T?` optionals, `??`, `?.`, `if (var x = opt)`,
`opt!`, and `match` over `T?` — with the compile-time guarantee that a non-optional `T` can never be
`null` (TypeScript `strictNullChecks` over PHP's nullable runtime), `run`≡`runvm` byte-identical and
transpiling 1:1 to PHP.

**Architecture:** `T?` is the existing `null` value at runtime; the guarantee lives in the checker.
One new internal type `Ty::Optional(Box<Ty>)` + an extended `Ty::assignable` (the single
type-compat chokepoint) carry the discipline. One new runtime value kind `Value::Null` (the Rust
compiler enforces its match coverage like `Op`). **No new `Op`** (decision S2-OPS): `??`/`?.`/`!`/
if-let lower to existing `SetLocal`/`GetLocal` + a null-test (`Eq` vs a `null` const) + `JumpIfFalse`/
`Jump`; `opt!` faults via the existing runtime-error channel.

**Tech Stack:** Rust 2021, std-only. Gate: `cargo test && cargo clippy --all-targets && cargo fmt --check`
(pre-commit hook re-runs it). Parity spine: `tests/differential.rs` (`agree`/`agree_err`, globs
`examples/**/*.phg`) + `tests/cli.rs` PHP round-trip.

**Source spec:** `docs/specs/2026-06-17-m3-slice1-s0-s1-s2-design.md` §S2.

## Decisions Log
- [2026-06-17] AGREED: build all of S2 autonomously, one green commit per sub-feature; each feature
  ships with its example (standing rule). Source: AskUserQuestion "Plan then build all of S2".
- [2026-06-17] AGREED: `null` literal types as `Ty::Optional(Box::new(Ty::Error))` — reuse `Error`'s
  unify-with-anything; no separate `Ty::Null` variant.
- [2026-06-17] AGREED: `Value::Null` is a new value kind; S2-OPS ("no new `Op`") is about the `Op`
  set, not the `Value` set. `??`/`?.` use a temp local for the non-consuming null-test (no `Dup` op).
- [2026-06-17] AGREED: `opt!`/OOB faults are the two transpile-divergent points — excluded by the
  differential harness (fault cases) and documented in `KNOWN_ISSUES.md`, not parity breaks.

---

## Progress
- [x] Task 1 — foundation (`Value::Null`, `Ty::Optional`/`Ty::Null`, non-null discipline) — `4ab9e36`
- [x] Task 2 — `??` null-coalesce (scratch-local lowering, no new `Op`) — `35b2b23`
- [x] Task 3 — `?.` safe access (nullsafe field + method, short-circuit, `E-OPT-USE`) — `f6266b2`
- [ ] Task 4 — `if (var x = opt)` · Task 5 — `opt!` · Task 6 — `match` over `T?` · Task 7 — example + docs

## File Structure

| File | Responsibility for S2 |
|---|---|
| `src/token.rs` | + `TokenKind::QuestionQuestion`, `QuestionDot` |
| `src/lexer.rs` | longest-match `??` / `?.` (mirror the S1 `..`/`..=` block: `peek2`/`peek3` before two-char ops) |
| `src/types.rs` | + `Ty::Optional(Box<Ty>)`; extend `assignable`; `Display` arm |
| `src/ast.rs` | + `BinaryOp::Coalesce`; `Expr::Force { inner, span }`; `safe: bool` on `Expr::Member` & method `Call`; `bind: Option<(String, Box<Expr>)>` on `Stmt::If` |
| `src/value.rs` | + `Value::Null`; arms in `type_name`, `eq_val` (`(Null,Null)=>true`), `compare_ord`, `as_display` |
| `src/parser.rs` | parse `??` (prec below `||`), `?.`, postfix `!`, `if (var x = e)` |
| `src/checker.rs` | un-reject `Type::Optional`/`Expr::Null`; non-null discipline; type `??`/`?.`/`!`/if-let/`match T?`; codes |
| `src/interpreter.rs` | `Expr::Null`→`Value::Null`; eval `??`/`?.`/`!`/if-let; `Pattern::Null` matches `Value::Null` |
| `src/compiler.rs` | `Expr::Null`→`add_const(Value::Null)`; `resolve_cty`/`ctype` for `Optional`; lower `??`/`?.`/`!`/if-let; `Pattern::Null` compile |
| `src/vm.rs` | `Value::Null` arms (exhaustive matches) |
| `src/transpile.rs` | `null`; `($a ?? $b)`; `$o?->m`; `opt!`→`__phorge_unwrap` helper; if-let; erase `Optional` |
| `src/cli.rs` | `explain` arms: `E-OPT-ASSIGN`, `E-OPT-USE`, `E-OPT-UNWRAP`, `W-FORCE-UNWRAP` |
| `tests/differential.rs` | a parity case per feature (+ `FaultKind` for force-unwrap) |
| `tests/cli.rs` | a PHP round-trip per feature |
| `examples/guide/null-safety.phg` (+ README rows) | the S2 showcase (standing rule) |
| `KNOWN_ISSUES.md`, `FEATURES.md`, `CHANGELOG.md`, `CLAUDE.md`, `ROADMAP.md` | docs |

Cross-cutting coupling reminders: a new `Value` variant must be covered in every exhaustive `Value`
match (compiler-enforced). No new `Op` ⇒ `vm.rs::exec_op` / `compiler.rs::stack_effect` /
`chunk.rs::validate` are untouched in shape.

---

### Task 1: Foundation — runtime `null`, optional types, non-null discipline (§2.1)

**Files:**
- Modify: `src/value.rs` (+ `Value::Null` and its match arms)
- Modify: `src/types.rs` (+ `Ty::Optional(Box<Ty>)`, `assignable`, `Display`)
- Modify: `src/checker.rs:118` (resolve `Type::Optional`), `src/checker.rs:520` (type `Expr::Null`)
- Modify: `src/interpreter.rs:243`, `src/compiler.rs:885`, `src/transpile.rs:390` (un-reject `Expr::Null`)
- Modify: `src/vm.rs` (Value::Null arms surfaced by the compiler)
- Test: `src/types.rs` unit, `src/checker.rs` unit, `tests/differential.rs`

- [ ] **Step 1: Failing test — `assignable` optional rules** in `src/types.rs` tests:
```rust
#[test]
fn optional_assignability() {
    let int_opt = Ty::Optional(Box::new(Ty::Int));
    assert!(Ty::assignable(&Ty::Int, &int_opt));        // T -> T?  (widen)
    assert!(!Ty::assignable(&int_opt, &Ty::Int));       // T? -/-> T (must unwrap)
    assert!(Ty::assignable(&int_opt, &int_opt));        // T? -> T?
    assert!(!Ty::assignable(&Ty::Optional(Box::new(Ty::Int)), &Ty::Optional(Box::new(Ty::Float))));
}
```
- [ ] **Step 2: Run** `cargo test -p phorge --lib types:: 2>&1 | tail` → FAIL (no `Ty::Optional`).
- [ ] **Step 3: Implement** in `src/types.rs`: add `Optional(Box<Ty>)`; rewrite `assignable`:
```rust
pub fn assignable(from: &Ty, to: &Ty) -> bool {
    if *from == Ty::Error || *to == Ty::Error { return true; }
    match to {
        Ty::Optional(inner) => match from {
            Ty::Optional(f) => Ty::assignable(f, inner), // T? -> U?
            other => Ty::assignable(other, inner),        // T  -> U?
        },
        _ => from == to, // T? -/-> non-optional T
    }
}
```
add `Display`: `Ty::Optional(e) => write!(f, "{e}?")`.
- [ ] **Step 4: Run** the types test → PASS.
- [ ] **Step 5: Failing test — null + optional in the checker** (`src/checker.rs` tests):
```rust
#[test]
fn optional_binding_and_null_discipline() {
    assert!(check_ok("function main() { int? x = null; }"));
    assert!(check_ok("function main() { int? x = 5; }"));            // widen
    assert!(check_err("function main() { int x = null; }", "E-OPT-ASSIGN"));
    assert!(check_err("function main() { int? x = null; int y = x; }", "E-OPT-ASSIGN"));
}
```
(use the file's existing `check_ok`/`check_err` helpers; confirm their names first.)
- [ ] **Step 6: Run** → FAIL (`Type::Optional`/`Expr::Null` rejected).
- [ ] **Step 7: Implement checker:**
  - `resolve_type` (line 118): `Type::Optional { inner, .. } => Ty::Optional(Box::new(self.resolve_type(inner)))`.
  - `check_expr_inner` (line 520): `Expr::Null(_) => Ty::Optional(Box::new(Ty::Error))`.
  - Where assignment errors are raised (lines 454/468/828/946 etc.), when the *declared* side is
    non-optional and `actual` is an `Optional`, emit code `E-OPT-ASSIGN` (use `err_coded`). Add a
    helper so the code attaches without rewording each site (e.g. classify in one place by checking
    `matches!(actual, Ty::Optional(_)) && !matches!(declared, Ty::Optional(_))`).
- [ ] **Step 8: Failing test — runtime null** (`tests/differential.rs`):
```rust
#[test] fn s2_null_binding_is_byte_identical() { agree("function main() { int? x = null; int? y = 5; println(\"{y}\"); }"); }
```
Plus a per-backend `out()` assertion that `int? y = 5; println("{y}")` prints `5` on both.
- [ ] **Step 9: Run** → FAIL (interpreter/compiler/transpile reject `Expr::Null`).
- [ ] **Step 10: Implement runtime null:**
  - `src/value.rs`: add `Value::Null`; `type_name` → `"null"`; `eq_val` → add `(Null, Null) => true`
    (before the `_ => false` tail); `compare_ord` → `Null` is unordered (return the existing
    "cannot compare" error path); leave `as_display`'s `_ => None` (null isn't directly displayable —
    the checker forbids `println` of a `T?`).
  - `src/interpreter.rs:243`: `Expr::Null(_) => Ok(Value::Null)`.
  - `src/compiler.rs:885`: `Expr::Null(_) => { let k = self.chunk().add_const(Value::Null); self.emit(Op::Const(k), span_line); Ok(()) }` (match the surrounding emit signature).
  - `src/transpile.rs:390`: `Expr::Null(_) => Ok("null".into())`.
  - `cargo build` → fix every exhaustive `Value` match the compiler flags (vm.rs/compiler.rs/value.rs)
    with a `Value::Null` arm (error/unreachable where null is checker-prevented).
- [ ] **Step 11: Run** `cargo test` → all green (332 + new). `cargo clippy --all-targets`, `cargo fmt --check`.
- [ ] **Step 12: Commit** `feat(lang): runtime null + optional types + non-null discipline (M3 S2.1)`.

### Task 2: `??` null-coalesce (§2.2)

**Files:** Modify `src/token.rs`, `src/lexer.rs`, `src/ast.rs` (`BinaryOp::Coalesce`), `src/parser.rs`,
`src/checker.rs`, `src/interpreter.rs`, `src/compiler.rs`, `src/transpile.rs`; Test `tests/differential.rs`, `tests/cli.rs`.

- [ ] **Step 1: Failing lexer test** (`src/lexer.rs`): `??` lexes as one `QuestionQuestion`, not two `Question`.
- [ ] **Step 2: Run** → FAIL. **Step 3:** add `TokenKind::QuestionQuestion`; in the lexer's longest-match
  block (where `..`/`..=` are handled) add: `if b == b'?' && peek2 == Some(b'?') { bump×2; push QuestionQuestion }`
  **before** the single `?`. **Step 4:** Run → PASS.
- [ ] **Step 5: Failing parser test:** `parses("a ?? b")` → `Expr::Binary { op: Coalesce, .. }`; precedence
  below `||` (so `a || b ?? c` parses as `(a || b) ?? c`... confirm desired assoc against spec — spec
  says "lowest-but-one, below `||`", so `??` binds *looser* than `||`: `a || b ?? c` = `(a || b) ?? c`).
- [ ] **Step 6: Run → FAIL. Step 7:** add `BinaryOp::Coalesce`; in `parse_expr` insert a `??` level
  just below the `||` level. **Step 8:** Run → PASS.
- [ ] **Step 9: Failing checker test:** `int? x = null; int y = x ?? 3;` type-checks (`x ?? 3 : int`);
  `x ?? null : int?`. **Step 10–11:** type `Coalesce`: `a:T?` required (else `E-OPT-USE`-adjacent),
  result is `inner(a)` if `b: inner(a)` else `Optional(inner)`.
- [ ] **Step 12: Failing parity + behavior tests** (`tests/differential.rs`):
```rust
#[test] fn s2_coalesce_is_byte_identical() {
    agree("function main() { int? x = null; println(\"{x ?? 7}\"); int? y = 9; println(\"{y ?? 0}\"); }");
}
```
(prints `7` then `9`). Add a per-backend `out()` asserting exactly that, and a short-circuit test
(rhs with an observable side effect — e.g. a call that prints — is not run when lhs non-null).
- [ ] **Step 13–14: Implement** interpreter (`eval a; if a is Null eval b else a`) and compiler
  (eval a → store temp local → `GetLocal tmp; Const null; Eq; JumpIfFalse keep; (null) eval b; Jump end; keep: GetLocal tmp; end:` — mirror the S1 expr-`if` height bookkeeping). Run → PASS.
- [ ] **Step 15: Transpile** `Coalesce` → `($a ?? $b)`; add `tests/cli.rs` round-trip. Run.
- [ ] **Step 16: Commit** `feat(lang): ?? null-coalesce (M3 S2.2)`.

### Task 3: `?.` safe access (§2.3)

**Files:** `src/token.rs` (`QuestionDot`), `src/lexer.rs`, `src/ast.rs` (`safe: bool` on `Member`/method `Call`), `src/parser.rs`, `src/checker.rs`, `src/interpreter.rs`, `src/compiler.rs`, `src/transpile.rs`; tests.

- [ ] **Step 1–4:** Failing lexer test `?.` → `QuestionDot`; implement in the longest-match block
  (before single `?` and before `.`). Note ordering vs `??`: test `?.`, `??`, and `?` all disambiguate.
- [ ] **Step 5–8:** Parser: where postfix `.member`/`.method()` is parsed, also accept `?.`; set
  `safe: true`. Failing test: `parses("a?.b")` and `parses("a?.b?.c")`.
- [ ] **Step 9–11:** Checker: receiver must be `T?` (or a chain already optional); result is
  `Optional(member_ty)`. Reading a non-optional with `?.` is allowed but lints/normal? — spec: applies
  to `T?`; on a non-optional, treat as error or just-non-optional — follow spec (`opt: T?`). Failing
  test: `null?.x : (x's type)?` and chain stays optional.
- [ ] **Step 12: Parity test** (`tests/differential.rs`): a class with a field; `C? c = null;`
  `println("{ (c?.x) ?? -1 }")` → `-1`; with `c` present → the field. `agree(...)` + `out()` both backends.
- [ ] **Step 13–14: Implement** interpreter (null receiver → `Value::Null`, else normal member/method)
  and compiler (temp local + null-test + branch; skip the member op when null, push `Value::Null`).
- [ ] **Step 15: Transpile** → `$o?->m`. Round-trip in `tests/cli.rs`.
- [ ] **Step 16: Commit** `feat(lang): ?. safe access (M3 S2.3)`.

### Task 4: `if (var x = opt)` binding + smart-cast (§2.4 + S1.4)

**Files:** `src/ast.rs` (`Stmt::If.bind`), `src/parser.rs`, `src/checker.rs`, `src/interpreter.rs`, `src/compiler.rs`, `src/transpile.rs`; tests.

- [ ] **Step 1–4:** AST: add `bind: Option<(String, Box<Expr>)>` to `Stmt::If`. Parser: in `if (`, if
  the next tokens are `var ident =`, parse the binding then the rest as the condition's stand-in
  (`expr`); else parse a normal condition (`bind: None`). Failing test: `parses("if (var x = e) {} else {}")`.
- [ ] **Step 5–7:** Checker: `expr: T?` required; inside `then_block`, `x: T` (the **smart-cast** —
  the non-optional inner) is in scope; not in `else`. `E-IF-LET-TYPE` if `expr` isn't optional.
  Failing test: `int? o = 5; if (var x = o) { int y = x; }` ok; using `x` in `else` → unknown ident.
- [ ] **Step 8: Parity test:** `int? o = 5; if (var x = o) { println("got {x}"); } else { println("none"); }`
  prints `got 5`; with `int? o = null` prints `none`. `agree(...)` + `out()`.
- [ ] **Step 9–10: Implement** interpreter (eval expr; if non-null bind `x` in a fresh then-scope, run
  then; else run else) and compiler (eval expr → temp/local slot → null-test → `JumpIfFalse else` →
  bind local = value, compile then → `Jump end` → else: compile else → end:).
- [ ] **Step 11: Transpile** → `if (($x = <expr>) !== null) { … } else { … }`. Round-trip.
- [ ] **Step 12: Commit** `feat(lang): if (var x = opt) binding + smart-cast (M3 S2.4)`.

### Task 5: `opt!` checked force-unwrap + lint (§2.5)

**Files:** `src/ast.rs` (`Expr::Force`), `src/parser.rs`, `src/checker.rs`, `src/interpreter.rs`, `src/compiler.rs`, `src/transpile.rs`, `src/cli.rs`; `tests/differential.rs` (`FaultKind`), `tests/cli.rs`.

- [ ] **Step 1–4:** AST `Expr::Force { inner, span }`; parser: postfix `!` (reuse `TokenKind::Bang`)
  after a primary/postfix expr. Disambiguate from prefix `!` (logical not): `!x` is unary-not (prefix);
  `x!` is force (postfix). Failing test: `parses("o!")` → `Expr::Force`; `parses("!b")` stays `Unary`.
- [ ] **Step 5–7:** Checker: `inner: T?` → `T`; `!` on a non-optional → `E-OPT-UNWRAP` error; emit a
  warning diagnostic `W-FORCE-UNWRAP` on every use (nudge to `??`/`?.`/if-let). Failing test:
  `int? o = 5; int x = o!;` ok (+ warning present); `int n = 3; n!;` → `E-OPT-UNWRAP`.
- [ ] **Step 8: Parity + fault test** (`tests/differential.rs`):
```rust
#[test] fn s2_force_unwrap_present_is_byte_identical() { agree("function main() { int? o = 5; println(\"{o!}\"); }"); } // prints 5
#[test] fn s2_force_unwrap_null_faults_identically() { agree_err("function main() { int? o = null; int x = o!; }"); }   // FaultKind::ForceUnwrap
```
Add `FaultKind::ForceUnwrap` + a classify arm matching the body substring `force-unwrap of null`.
- [ ] **Step 9–10: Implement** interpreter (`if inner is Null → rt("force-unwrap of null")` else value)
  and compiler (eval inner → temp → null-test → `JumpIfFalse ok` → emit the fault via the existing
  channel; reuse the `MatchFail`-style fault op if one carries a message, else the string-error return
  the VM already line-prefixes — confirm in `vm.rs`; **no new `Op`**).
- [ ] **Step 11: Transpile** → emit a once-per-file helper `__phorge_unwrap($v,'name',line)` that
  `throw`s on null else returns `$v`; `o!` → `__phorge_unwrap($o, 'o', <line>)`. Round-trip (present case
  only; the null-fault case is a documented divergence). **Step 12:** `explain` arms for the 2 new codes.
- [ ] **Step 13: Commit** `feat(lang): opt! checked force-unwrap + W-FORCE-UNWRAP lint (M3 S2.5)`.

### Task 6: `match` over `T?` + null-excluding arm narrowing (§2.6 + S1.4)

**Files:** `src/checker.rs`, `src/interpreter.rs:640` (`Pattern::Null`), `src/compiler.rs:1270`, `src/transpile.rs`; tests.

- [ ] **Step 1: Failing test:** `match opt { null => "none", v => "{v}" }` is exhaustive for `T?`; the
  binding arm narrows `v: T`. Missing-null-arm with no catch-all → non-exhaustive error.
- [ ] **Step 2–4: Implement** checker exhaustiveness for `Optional` scrutinee (null arm + binding/
  wildcard covers it; binding arm binds `v: inner`); interpreter `Pattern::Null` matches `Value::Null`
  (replace the `=> false` at line 640 with `matches!(scrut, Value::Null)`); compiler `Pattern::Null`
  compiles to the null-test.
- [ ] **Step 5: Parity test** (`tests/differential.rs`): `int? o = null` and `int? o = 7` both
  `match`ed; `agree(...)` + `out()` both backends.
- [ ] **Step 6: Transpile** the `T?` match (null arm → `$x === null`). Round-trip.
- [ ] **Step 7: Commit** `feat(lang): match over T? with null-arm narrowing (M3 S2.6)`.

### Task 7: example + docs (standing rule)

**Files:** Create `examples/guide/null-safety.phg`; Modify `examples/README.md`, `FEATURES.md`,
`CHANGELOG.md`, `KNOWN_ISSUES.md`, `CLAUDE.md`, `ROADMAP.md`, `src/cli.rs` (explain list).

- [ ] **Step 1:** Write `examples/guide/null-safety.phg` exercising `T?`, `??`, `?.`, `if (var x = opt)`,
  `opt!` (present), and `match opt`. Verify `run`≡`runvm` + PHP round-trip (faults excluded — they
  can't appear in a passing example). Add README index + matrix rows.
- [ ] **Step 2:** `FEATURES.md` — flip "Null safety / optionals (`T?`)" to ✅ with the operator list.
  `CHANGELOG.md` — S2 entry. `KNOWN_ISSUES.md` — note the two transpile-divergent fault cases
  (`opt!`-on-null, OOB). `ROADMAP.md` — mark S2 landed. `CLAUDE.md` — S2 complete, next milestone item.
  `cli.rs` `cmd_explain` known-codes list — add the 4 new codes.
- [ ] **Step 3: Final gate** `cargo test && cargo clippy --all-targets && cargo fmt --check` green.
- [ ] **Step 4: Commit** `docs(m3): S2 null-safety example + feature docs; mark S2 complete`.

---

## Self-Review
- **Spec coverage:** §2.1→Task1, §2.2→Task2, §2.3→Task3, §2.4(+S1.4)→Task4, §2.5→Task5, §2.6(+S1.4)→Task6;
  cross-cutting (examples + PHP round-trip + explain codes) → each task + Task7. All §S2 covered.
- **Type consistency:** `Ty::Optional(Box<Ty>)`, `Value::Null`, `BinaryOp::Coalesce`, `Expr::Force{inner,span}`,
  `Stmt::If.bind: Option<(String,Box<Expr>)>`, `safe: bool` — names used consistently across tasks.
- **No-new-`Op` invariant:** every lowering (Tasks 2–6) uses existing `SetLocal`/`GetLocal`/`Eq`/
  `JumpIfFalse`/`Jump`/fault-channel — `exec_op`/`stack_effect`/`validate` shape unchanged. Verify at
  build time that `Dup` is not required (temp-local lowering avoids it).
- **Risk:** the null-literal-as-`Optional(Error)` typing must not make `null` assignable to a
  non-optional via `Error` leakage — guarded because `assignable(Optional(_), non-optional) == false`.
