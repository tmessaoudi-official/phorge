# M3 Slice-1 S1 (Core Ergonomics) Implementation Plan

> **For agentic workers:** implement task-by-task; each task ends green (`cargo test` +
> `cargo clippy --all-targets` + `cargo fmt --check`) and is committed. Steps use checkbox
> (`- [ ]`) tracking.

**Goal:** Add three byte-identical (`run`≡`runvm`) ergonomics features to Phorge — list indexing
`xs[i]`, integer ranges `a..b` / `a..=b`, and expression-`if` — per
`docs/specs/2026-06-17-m3-slice1-s0-s1-s2-design.md` §S1.

**Architecture:** Front-end-light additions threaded through the whole pipeline (lexer → parser →
checker → interpreter + compiler/VM + transpiler). Exactly **one** new `Op` (`MakeRange(bool)`),
extending the three coupled exhaustive matches in lockstep (`vm::exec_op`, `compiler::stack_effect`;
`chunk::validate` needs no arm — no static index). The parity spine (`tests/differential.rs`
`agree`/`agree_err`) gates every feature.

**Tech Stack:** Rust 2021, std-only, `#![forbid(unsafe_code)]`, `warnings = "deny"`, clippy pedantic-off.

**Scope note — S1.4 (smart-cast narrowing) is DEFERRED to S2.** Its entire described surface
(`if (var x = opt)` = S2.5, and null-excluding `match` arms) operates on optionals (`T?`), which
do not exist until S2. There is no exercisable behavior in S1 → it cannot be test-driven → building
it now would be untestable dead code (YAGNI). It is folded into the S2 plan. Documented in Task 4.

**Syntax decisions (flagged for review):**
- Expression-`if` uses **parens + mandatory `else`**: `if (c) { e } else { e }` — consistent with
  statement-`if` (which requires parens). The design's grammar line agrees (`if (cond) …`); its one
  worked example omitted parens. Missing `else` is a **parse error** (so `E-IF-EXPR-ELSE` is unused).
- Expression-`if` arms are **single expressions** (`{ e }`), not statement blocks — so `Expr::Block`
  is NOT introduced (deferred; nothing in S1 needs statements-in-arms, and the clean PHP ternary
  mapping only works for expression arms).

---

## Grounding (verified against the code, 2026-06-17)

| Fact | Consequence |
|---|---|
| Checker `check_index` already returns the element type for `List<T>` + int index; test `list_indexing_yields_element` is green | S1.1 needs **no checker change** — the rejection is in the backends only |
| Interpreter rejects `Expr::Index` (`interpreter.rs:267`); compiler rejects it (`compiler.rs:873`); transpiler rejects it (`transpile.rs:391`) | S1.1 = un-reject these three; reuse the working `Op::Index` |
| VM `Op::Index` works + is bounds-checked → OOB body is exactly `"list index out of range"` (`vm.rs:245`); `stack_effect(Op::Index) == -1` already | interpreter OOB must produce the identical body; no `stack_effect` change for Index |
| `compile_for` evaluates `iter` as any list-producing expr (`compiler.rs:1083`) | a range compiling to `Op::MakeRange` (→ List) iterates for free; interpreter `Stmt::For` likewise iterates any `Value::List` |
| `classify` (`differential.rs:47`) has no OOB arm → falls to `Other(full_string)`; VM prefixes `runtime error at N:` | S1.1 MUST add `FaultKind::IndexOob` or an OOB `agree_err` spuriously fails |
| disasm uses `Op` `Debug` + `_`-fallthrough annotator (`cli.rs:323`) | a new `Op` needs **no** disasm match edit |
| `expand_aliases` clones `Expr` nodes unchanged (no `Expr` match) | new `Expr` variants need **no** `expand_aliases` arm |
| `emit_expr` (transpile) + `check_expr_inner` + `expr_span` (checker) + `eval` (interpreter) + `expr` (compiler) are exhaustive over `Expr` | adding `Expr::Range`/`Expr::If` forces arms in each (compile error if missed = coupling enforced) |

**`Expr` match sites to extend for `Range`+`If`:** `checker::check_expr_inner`, `checker::expr_span`,
`interpreter::eval`, `compiler::expr`, `compiler::ctype` (+`num_ty` via ctype), `transpile::emit_expr`.
(`parser::sexpr` test helper has a `_` catch-all — optional.)

**`Op` match sites to extend for `MakeRange`:** `compiler::stack_effect` (exhaustive — required),
`vm::exec_op` (exhaustive — required). `chunk::validate` (`_ => None`, no static index — none).

---

## Task 1 — S1.1 Indexing `xs[i]`

**Files:** `tests/differential.rs` (classify + cases), `src/interpreter.rs`, `src/compiler.rs`,
`src/transpile.rs`, plus unit tests in each.

- [ ] **Step 1 — failing tests.** In `tests/differential.rs`: add `FaultKind::IndexOob`; add a
  `classify` arm `else if err.contains("list index out of range") { FaultKind::IndexOob }` (before
  the `Unsupported` arm). Add a success-parity test and an OOB error-parity test:
  ```rust
  #[test]
  fn s1_indexing_is_byte_identical() {
      agree(r#"function main() { List<int> xs = [10, 20, 30]; println("{xs[0]} {xs[2]}"); }"#);
      agree(r#"function main() { for (int i in [0,1,2]) { println("{[5,6,7][i]}"); } }"#);
  }
  #[test]
  fn s1_index_oob_faults_identically() {
      agree_err(r#"function main() { List<int> xs = [1, 2]; println("{xs[5]}"); }"#);
  }
  ```
- [ ] **Step 2 — run red.** `cargo test --test differential s1_index` → both fail (interpreter/compiler reject `Expr::Index`).
- [ ] **Step 3 — interpreter.** Replace `interpreter.rs` line 267 `Expr::Index { .. } => rt(...)` with:
  ```rust
  Expr::Index { object, index, .. } => {
      let obj = self.eval(object)?; // object first (matches compiler emit / VM pop order)
      let idx = self.eval(index)?;
      let i = match idx {
          Value::Int(n) => n,
          v => return rt(format!("expected int index, found {}", v.type_name())),
      };
      let list = match obj {
          Value::List(xs) => xs,
          v => return rt(format!("cannot index {}", v.type_name())),
      };
      match usize::try_from(i).ok().filter(|i| *i < list.len()) {
          Some(i) => Ok(list[i].clone()),
          None => rt("list index out of range"), // byte-identical to vm.rs:245
      }
  }
  ```
- [ ] **Step 4 — compiler.** Replace `compiler.rs:873` `Expr::Index { .. } => return Err(...)` with:
  ```rust
  Expr::Index { object, index, .. } => {
      self.expr(object)?;
      self.expr(index)?;
      self.emit(Op::Index, /* span */ ); // use the Index span's line
  }
  ```
  (Bind `span` in the match arm: `Expr::Index { object, index, span } => { … self.emit(Op::Index, span.line); }`.)
- [ ] **Step 5 — transpiler.** Replace `transpile.rs:391` `Expr::Index { .. } => Err(...)` with:
  ```rust
  Expr::Index { object, index, .. } => {
      let o = self.emit_expr(object)?;
      let i = self.emit_expr(index)?;
      Ok(format!("{o}[{i}]"))
  }
  ```
- [ ] **Step 6 — unit tests.** `interpreter.rs`: `out(r#"… List<int> xs=[7,8,9]; println("{xs[1]}"); …"#)=="8\n"`.
  `compiler.rs`: same program via VM `out()`. `transpile.rs`: assert emitted PHP contains `$xs[0]`.
- [ ] **Step 7 — run green.** `cargo test` → all pass. Then `cargo clippy --all-targets` + `cargo fmt`.
- [ ] **Step 8 — commit.** `feat(lang): list indexing xs[i] (un-reject backends; M3 S1.1)`

---

## Task 2 — S1.2 Ranges `a..b` / `a..=b`

**Files:** `src/token.rs`, `src/lexer.rs`, `src/ast.rs`, `src/parser.rs`, `src/chunk.rs`,
`src/compiler.rs`, `src/vm.rs`, `src/interpreter.rs`, `src/transpile.rs`, `src/checker.rs`,
`src/cli.rs` (explain), `tests/differential.rs`, unit tests.

- [ ] **Step 1 — failing differential tests.** In `tests/differential.rs`:
  ```rust
  #[test]
  fn s1_ranges_are_byte_identical() {
      agree(r#"function main() { for (int i in 0..3) { println("{i}"); } }"#);       // 0 1 2
      agree(r#"function main() { for (int i in 1..=3) { println("{i}"); } }"#);      // 1 2 3
      agree(r#"function main() { for (int i in 5..5) { println("{i}"); } println("done"); }"#); // empty
      agree(r#"function main() { var xs = 0..3; for (int i in xs) { println("{i}"); } }"#);
  }
  ```
- [ ] **Step 2 — run red.** `cargo test --test differential s1_ranges` → fail (`..` lexes as two `.`/parse error).
- [ ] **Step 3 — tokens.** `token.rs`: add `DotDot, DotDotEq` to `TokenKind` (near `Dot`).
- [ ] **Step 4 — lexer.** Add `fn peek3(&self) -> Option<u8> { self.src.get(self.pos + 2).copied() }`.
  In `lex()`, **before** the two-char-operator block, add (longest-match `..=` > `..` > `.`):
  ```rust
  if b == b'.' && lx.peek2() == Some(b'.') {
      if lx.peek3() == Some(b'=') {
          lx.bump(); lx.bump(); lx.bump();
          out.push(Token { kind: TokenKind::DotDotEq, span: Span { start, len: 3, line, col } });
      } else {
          lx.bump(); lx.bump();
          out.push(Token { kind: TokenKind::DotDot, span: Span { start, len: 2, line, col } });
      }
      continue;
  }
  ```
  (`scan_number` already keeps `0..3` as `Int(0)` — its float check needs a digit after `.`.)
  Unit test: `kinds("0..3")==[Int(0),DotDot,Int(3),Eof]`, `kinds("0..=3")` uses `DotDotEq`.
- [ ] **Step 5 — AST.** `ast.rs` `Expr`: add
  `Range { start: Box<Expr>, end: Box<Expr>, inclusive: bool, span: Span }`.
- [ ] **Step 6 — parser.** `parse_expr` → `self.parse_range()`. New:
  ```rust
  fn parse_range(&mut self) -> Result<Expr, Diagnostic> {
      let start = self.parse_binary(0)?;
      let inclusive = match self.peek() {
          TokenKind::DotDot => false,
          TokenKind::DotDotEq => true,
          _ => return Ok(start),
      };
      let sp = self.peek_span();
      self.advance();
      let end = self.parse_binary(0)?;
      Ok(Expr::Range { start: Box::new(start), end: Box::new(end), inclusive, span: sp })
  }
  ```
  Unit test: `parse_expr("0..3")` is `Expr::Range{inclusive:false,..}`; `"0..=3"` inclusive.
- [ ] **Step 7 — Op + coupling.** `chunk.rs`: add `MakeRange(bool)` to `Op` (doc: pop end+start ints,
  push `List<int>`; `inclusive` flag; no static index ⇒ no `validate` arm, like `GetEnumField`).
  `compiler.rs::stack_effect`: add `Op::MakeRange(_) => -1` (pops 2, pushes 1).
  `vm.rs::exec_op`: add
  ```rust
  Op::MakeRange(inclusive) => {
      let end = self.pop_int()?;
      let start = self.pop_int()?;
      let list: Vec<Value> = if inclusive {
          (start..=end).map(Value::Int).collect()
      } else {
          (start..end).map(Value::Int).collect()
      };
      self.stack.push(Value::List(Rc::new(list)));
  }
  ```
  (Rust's native `start..=end` handles `end == i64::MAX` without counter overflow — EV-7.)
- [ ] **Step 8 — compiler emit + ctype.** `compiler.rs::expr`: add
  ```rust
  Expr::Range { start, end, inclusive, span } => {
      self.expr(start)?;
      self.expr(end)?;
      self.emit(Op::MakeRange(*inclusive), span.line);
  }
  ```
  `compiler.rs::ctype`: add `Expr::Range { .. } => Ok(CTy::Other)` (a list, never an arithmetic operand).
- [ ] **Step 9 — interpreter.** `interpreter.rs::eval`: add
  ```rust
  Expr::Range { start, end, inclusive, .. } => {
      let s = match self.eval(start)? { Value::Int(n) => n, v => return rt(format!("range start must be int, found {}", v.type_name())) };
      let e = match self.eval(end)? { Value::Int(n) => n, v => return rt(format!("range end must be int, found {}", v.type_name())) };
      let list: Vec<Value> = if *inclusive { (s..=e).map(Value::Int).collect() } else { (s..e).map(Value::Int).collect() };
      Ok(Value::List(Rc::new(list)))
  }
  ```
- [ ] **Step 10 — checker.** `check_expr_inner`: add `Expr::Range { start, end, span } => self.check_range(start, end, *span)`
  (bind `inclusive` too if needed; not needed for typing). New method:
  ```rust
  fn check_range(&mut self, start: &Expr, end: &Expr, span: Span) -> Ty {
      let s = self.check_expr(start);
      let e = self.check_expr(end);
      let ok = |t: &Ty| matches!(t, Ty::Int | Ty::Error);
      if !ok(&s) || !ok(&e) {
          return self.err_coded(span, format!("range bounds must be `int`, found `{s}` and `{e}`"), "E-RANGE-TYPE", None);
      }
      Ty::List(Box::new(Ty::Int))
  }
  ```
  `expr_span`: add `Expr::Range { span, .. } => *span` (extend the existing arm group).
- [ ] **Step 11 — transpiler.** `emit_expr`: add (per design — PHP `range` is inclusive):
  ```rust
  Expr::Range { start, end, inclusive, .. } => {
      let s = self.emit_expr(start)?;
      let e = self.emit_expr(end)?;
      Ok(if *inclusive { format!("range({s}, {e})") } else { format!("range({s}, {e} - 1)") })
  }
  ```
  Unit test: emitted PHP for `for (int i in 0..3)` contains `range(0, 3 - 1)`; `1..=3` → `range(1, 3)`.
- [ ] **Step 12 — explain code.** `cli.rs`: add `"E-RANGE-TYPE"` arm to `explain_text` (a paragraph:
  "range bounds must both be `int`; ranges materialize to `List<int>` for `for…in`") and append it to
  the known-codes list in `cmd_explain`. Unit test: `cmd_explain("E-RANGE-TYPE").is_ok()`.
- [ ] **Step 13 — checker unit tests.** `for (int i in 0..3)` checks clean; `for (int i in 0..3.0)`
  (float end) → an error containing "range bounds must be `int`".
- [ ] **Step 14 — run green + gate.** `cargo test`, `cargo clippy --all-targets`, `cargo fmt`.
- [ ] **Step 15 — commit.** `feat(lang): integer ranges a..b / a..=b via Op::MakeRange (M3 S1.2)`

---

## Task 3 — S1.3 Expression-`if`

**Files:** `src/ast.rs`, `src/parser.rs`, `src/checker.rs`, `src/interpreter.rs`,
`src/compiler.rs`, `src/transpile.rs`, `tests/differential.rs`, unit tests.

- [ ] **Step 1 — failing differential tests.**
  ```rust
  #[test]
  fn s1_expression_if_is_byte_identical() {
      agree(r#"function main() { var x = if (1 < 2) { 10 } else { 20 }; println("{x + x}"); }"#);
      agree(r#"function pick(bool b) -> int { return if (b) { 1 } else { 2 }; }
               function main() { println("{pick(true)} {pick(false)}"); }"#);
      agree(r#"function main() { for (int i in 0..3) { println(if (i == 1) { "one" } else { "x" }); } }"#);
  }
  ```
- [ ] **Step 2 — run red.** `cargo test --test differential s1_expression_if` → parse error (no expr-`if`).
- [ ] **Step 3 — AST.** `ast.rs` `Expr`: add
  `If { cond: Box<Expr>, then_expr: Box<Expr>, else_expr: Box<Expr>, span: Span }`.
- [ ] **Step 4 — parser.** `parse_primary`: add `TokenKind::If => self.parse_if_expr(sp)`. New:
  ```rust
  fn parse_if_expr(&mut self, sp: Span) -> Result<Expr, Diagnostic> {
      self.expect(&TokenKind::If, "'if'")?;
      self.expect(&TokenKind::LParen, "'(' after 'if'")?;
      let cond = self.parse_expr()?;
      self.expect(&TokenKind::RParen, "')' after if condition")?;
      self.expect(&TokenKind::LBrace, "'{' to open the then-branch")?;
      let then_expr = self.parse_expr()?;
      self.expect(&TokenKind::RBrace, "'}' to close the then-branch")?;
      self.expect(&TokenKind::Else, "'else' (an expression `if` must have an else branch)")?;
      self.expect(&TokenKind::LBrace, "'{' to open the else-branch")?;
      let else_expr = self.parse_expr()?;
      self.expect(&TokenKind::RBrace, "'}' to close the else-branch")?;
      Ok(Expr::If { cond: Box::new(cond), then_expr: Box::new(then_expr), else_expr: Box::new(else_expr), span: sp })
  }
  ```
  (Statement-`if` is unaffected — `parse_stmt` matches `If` first and routes to `parse_if`.)
  Unit test: `parse_expr("if (true) { 1 } else { 2 }")` is `Expr::If{..}`.
- [ ] **Step 5 — checker.** `check_expr_inner`: add `Expr::If { cond, then_expr, else_expr, span } => self.check_if_expr(cond, then_expr, else_expr, *span)`. New:
  ```rust
  fn check_if_expr(&mut self, cond: &Expr, then_e: &Expr, else_e: &Expr, span: Span) -> Ty {
      let c = self.check_expr(cond);
      if !Ty::assignable(&c, &Ty::Bool) {
          self.err(span, format!("`if` condition must be `bool`, found `{c}`"));
      }
      let t = self.check_expr(then_e);
      let e = self.check_expr(else_e);
      if t != Ty::Error && e != Ty::Error && !Ty::assignable(&e, &t) && !Ty::assignable(&t, &e) {
          self.err(span, format!("`if` branches must share one type; found `{t}` and `{e}`"));
      }
      if t == Ty::Error { e } else { t }
  }
  ```
  `expr_span`: add `Expr::If { span, .. } => *span`.
- [ ] **Step 6 — interpreter.** `eval`: add
  ```rust
  Expr::If { cond, then_expr, else_expr, .. } => {
      if as_bool(&self.eval(cond)?)? { self.eval(then_expr) } else { self.eval(else_expr) }
  }
  ```
- [ ] **Step 7 — compiler.** `expr`: add (mirrors the `Or`/`And` height handling):
  ```rust
  Expr::If { cond, then_expr, else_expr, span } => {
      self.expr(cond)?;
      let else_j = self.emit_jump(Op::JumpIfFalse(0), span.line); // pops cond
      let h = self.height;                                        // both arms converge above here
      self.expr(then_expr)?;
      let end_j = self.emit_jump(Op::Jump(0), span.line);
      self.patch_jump(else_j);
      self.height = h;                                            // else path starts at merge height
      self.expr(else_expr)?;
      self.patch_jump(end_j);
  }
  ```
  `ctype`: add `Expr::If { then_expr, .. } => self.ctype(then_expr)` (branches share a type → infer
  from then, so `var x = if (c) {1} else {2}` specializes arithmetic).
- [ ] **Step 8 — transpiler.** `emit_expr`: add
  ```rust
  Expr::If { cond, then_expr, else_expr, .. } => {
      let c = self.emit_expr(cond)?;
      let t = self.emit_expr(then_expr)?;
      let e = self.emit_expr(else_expr)?;
      Ok(format!("({c} ? {t} : {e})"))
  }
  ```
  Unit test: emitted PHP for `return if (b) { 1 } else { 2 };` contains `($b ? 1 : 2)`.
- [ ] **Step 9 — unit tests.** interpreter+VM `out()` for `var x = if (true) {7} else {9}; println("{x}")` == `7\n`;
  checker: branch-type-mismatch (`if (b) { 1 } else { true }`) errors; missing-else is a parse error.
- [ ] **Step 10 — run green + gate.** `cargo test`, `cargo clippy --all-targets`, `cargo fmt`.
- [ ] **Step 11 — commit.** `feat(lang): expression if (if (c) { e } else { e }) (M3 S1.3)`

---

## Task 4 — S1 example + docs + S1.4 deferral

**Files:** `examples/guide/ergonomics.phg` (new), `README.md`, `FEATURES.md`, `KNOWN_ISSUES.md`,
`CHANGELOG.md`, `CLAUDE.md`.

- [ ] **Step 1 — example (auto byte-identity-gated by the `examples/**/*.phg` glob).**
  `examples/guide/ergonomics.phg` exercising indexing + ranges + expression-`if`:
  ```phorge
  import std.io;

  function main() {
      List<int> xs = [10, 20, 30];
      println("first={xs[0]} last={xs[2]}");
      for (int i in 0..3) {
          string parity = if (i == 1) { "one" } else { "other" };
          println("i={i} sq={xs[i]} {parity}");
      }
      for (int n in 1..=3) { println("n={n}"); }
  }
  ```
- [ ] **Step 2 — verify example.** `phorge run examples/guide/ergonomics.phg` and `runvm` match;
  `cargo test --test differential all_examples` green (auto-gated).
- [ ] **Step 3 — docs.**
  - `KNOWN_ISSUES.md`: remove "Indexing (`xs[i]`)" from "not yet implemented"; add a **Behavioral
    quirks** note that PHP-transpiled ranges use `range()`, which (unlike Phorge) reverses for an
    empty/descending range (`a..b`, `a>=b`) — a transpile-only caveat; the Phorge backends are
    byte-identical and unaffected. (Parallel to the OOB-indexing caveat.)
  - `README.md` "Language at a glance": add indexing, ranges (`0..n`/`0..=n`), expression-`if`.
  - `FEATURES.md`: flip indexing/ranges/expression-`if` to implemented.
  - `CHANGELOG.md` `[Unreleased]`: an `M3 S1` block (indexing, ranges + `Op::MakeRange`, expression-`if`).
  - `CLAUDE.md` "Active plan": mark S1 complete; note S1.4 deferred to S2; next = **S2** (null-safety).
- [ ] **Step 4 — S1.4 deferral is recorded** in CLAUDE.md + KNOWN_ISSUES (smart-cast narrowing arrives
  with optionals in S2).
- [ ] **Step 5 — gate + commit.** `cargo test`, `cargo clippy --all-targets`, `cargo fmt`.
  `docs(m3): S1 example + feature docs; mark S1 complete, S2 next; defer S1.4`

---

## Self-review

- **Spec coverage:** S1.1 (Task 1), S1.2 (Task 2), S1.3 (Task 3); S1.4 explicitly deferred with
  rationale (Task 4). Cross-cutting: each feature gets a `differential.rs` `agree`/`agree_err` case +
  the auto-gated example; transpile assertions per feature. `E-RANGE-TYPE` gets a `phorge explain`
  entry. The one new `Op` extends the coupled matches in its own commit.
- **Type consistency:** `Expr::Range{start,end,inclusive,span}` and `Expr::If{cond,then_expr,else_expr,span}`
  used identically across parser/checker/interpreter/compiler/transpiler. `Op::MakeRange(bool)`.
- **Parity spine:** every feature's primary test is `agree()` (byte-identical `run`≡`runvm`); OOB uses
  `agree_err` with the new `FaultKind::IndexOob`.
