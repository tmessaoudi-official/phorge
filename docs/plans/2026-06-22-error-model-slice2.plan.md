# error-model-slice2 Plan (M-faults Slice 2)

The three-tier error model: enforced typed **`throws E`** (→ idiomatic PHP exceptions), **`Result<T,E>`**
value surface (the shipped generic enum + `?`), and unchecked **faults/panics** (crash + Slice-1 stack
trace, never declared up-chain). Byte-identical `run ≡ runvm ≡ real PHP`. Design-first (brainstorming),
then writing-plans.

## Decisions Log
- [2026-06-22] AGREED: next slice = **error model (Slice 2)** over method overloading — biggest GA
  lever, unblocked now that generic enums ship (`Result<T,E>` is expressible), completes the `never`
  story (`throw`/`panic` become the real `never` producers).
- [2026-06-22] AGREED: **design the full three-tier model in one spec**; build cadence (one-shot vs
  sub-sliced) **deferred to plan time** — my standing lean is sub-sliced (isolate the try/catch runtime
  risk), the developer leans one-shot; decide once the seams are visible.
- [2026-06-22] AGREED: `throw`/`try`/`catch` use **native unwinding** (not desugar-to-Result) — the
  locked decision requires *idiomatic PHP exception* output, so the backends must reproduce real
  catch/unwind. Realistically **one new VM Op** for the handler/landing-pad stack; the interpreter
  catches at the `try` boundary (Rust `Result`). The `throws` **declaration** still erases pre-backend
  (front-end-only, no Op) — only the control flow needs the Op. Full `Op`-coupling discipline applies.
- [2026-06-22] AGREED: **Section A** — three tiers as above; `throw`/`panic*` are **`never`-typed**
  (satisfy return-on-all-paths); call-site rule = **enforce-or-propagate-or-catch**; propagation operator
  is **postfix `?`** (locked by spec lines 41/43), disambiguated from `?.`/`??` by one-char lookahead
  (propagation `?` only when not followed by `.` or `?`). Panic tier = `panic(string)`/`todo()`/
  `unreachable()`/`assert(bool, string?)`, all reusing the existing `Op::Fault`.
- [2026-06-22] AGREED: **Section B** — a thrown type is a subtype of a **core `Error` base**
  (interface/class), transpiling to a PHP class extending `\Exception` (home for `.message()` +
  cause-chain). Enforcement = enforce-or-propagate-or-catch; **declare specific** (`E-THROWS-TOO-BROAD`
  on the bare root), **catch broad**; **`main()` may not throw** (`E-UNCAUGHT-THROW`); `throws A | B`
  reuses S4 unions. `?` is type-directed: throws-call → propagate throw; `Result` value → unwrap/early-Err.

### Decisions Log (2b plan-shaping — 2026-06-22, AskUserQuestion)
- AGREED: **`finally` lands in 2b**, with `try`/`catch` (not deferred to 2c). Rationale: the spec's
  headline example shows `finally` beside `try`/`catch`; splitting would ship `errors.phg` without it
  and re-open the totality `try`-engine + the unwind-path codegen twice. The unwind path is exactly the
  codegen 2b already writes, so doing it together is *less* total work. **2c shrinks to cause-chain +
  imported-PHP catch bridge.**
- AGREED: **throw-across-a-higher-order-native boundary is handled in 2b** (a closure passed to
  `Core.List.map`/`filter`/`reduce` that throws, caught by an outer `try`). Mechanism (chosen to avoid
  changing the `ClosureInvoker`/`NativeEval` signatures): a **`pending_throw: Option<Value>` side-channel
  + a reserved sentinel fault body** — the re-entrant invoker stashes the thrown `Value` and returns the
  sentinel through the existing `Result<_, String>` channel; the native propagates it unchanged; the
  `CallNative` op site reconstructs a `Throw` and unwinds via the handler stack. Native code is untouched.
- AGREED: **both multi-catch forms ship in 2b** — (1) **multiple sequential clauses**
  `catch (X e) { … } catch (Y e) { … }`, each its own binding/scope/body (the baseline — `catches:
  Vec<CatchClause>`); (2) **union-in-one-clause** `catch (X | Y e) { … }`, `e` typed as the S4 union.
  Both lower to a per-clause `IsInstance`→`JumpIfFalse` chain over the landed thrown value (reuses M-RT
  S1 `Op::IsInstance`); re-throw if none match.
- AGREED: a **catch clause shadowed by an earlier broader/duplicate clause** in the same `try` is a
  **`W-CATCH-UNREACHABLE`** warning (not a hard error, not silent) — exact parallel to the totality
  cluster's `W-MATCH-UNREACHABLE`/`W-UNREACHABLE`; rides the non-gating warning channel, self-documents
  via `phg explain`. (Java errors here; PHP is silent — Phorge picks the legible-but-non-blocking middle,
  consistent with its existing dead-code lints.)
- PINNED: **3 new `Op` variants** — `Op::Throw`, `Op::PushHandler(usize)`, `Op::PopHandler`
  (`PushHandler`/`PopHandler` are one handler *mechanism*; `Throw` is the second). Each extends the three
  coupled matches (`vm.rs` `exec_op` + `chunk.rs` `validate` + `compiler.rs` `stack_effect`) in one commit.
- AGREED (2b.2 build, AskUserQuestion + worked-code review): **Error → PHP mapping = field-based
  marker.** `Error` is a built-in **marker interface** (no required method); a thrown class promotes a
  `string message` field (conventional, **not** mandated — option C's `E-ERROR-MESSAGE` rejected as too
  rigid); `class P implements Error` transpiles to PHP `class P extends \Exception` with the promoted
  `message` routed through **`parent::__construct($message)`** so native `getMessage()` works (interop +
  2c bridge); a read `e.message` on an `Error`-typed value emits `$e->getMessage()`. The explicit
  `message()`-method option was rejected (boilerplate per class + breaks PHP's `getMessage()` convention).
  The one special-case: a promoted field literally named `message` on an `Error` subtype gets this
  treatment. (Class `extends` is a future slice S6, so `Error` is necessarily an interface this slice.)
- PINNED: **`?`-throws mode is front-end-only — no backend codegen.** A `?` on a `throws`-call is a
  checker propagation *marker*; since the call's own `Op::Throw` already unwinds, the lowering is just the
  bare call. The checker **erases the throws-mode `Propagate` node to its inner expr before any backend**
  (same "expand out before backends" discipline as type aliases / generics / `html"…"`), so the backends
  only ever see a `Propagate` in *Result* mode — 2a's lowering is untouched, zero backend change for
  throws-`?`.

## Status
- **PHASE 2a — COMPLETE** (`46c8d2a` `?` propagation, `f35ff6c` fault intrinsics). `Result` `?` +
  `panic`/`todo`/`unreachable`/`assert`, no new `Op` (`?` reuses MatchTag/GetEnumField/Return; intrinsics
  reuse `Op::Fault` via new data-carrying `FaultMsg` variants). Byte-identical `run≡runvm≡real PHP`;
  600 lib + PHP-oracle differential + 64 integration green; 5 new codes self-document via `phg explain`.
  `examples/guide/result.phg`. **NEXT: review checkpoint → author the detailed 2b plan (exceptions).**
- **PHASE 2b — NOT STARTED** (outline below): core `Error` base, `throws E` + enforcement, `throw`,
  `try`/`catch`, `?`-throws mode, native VM unwinding (≈2 new Ops), PHP exception mapping.
- **PHASE 2c — NOT STARTED**: `finally` + cause-chain + imported-PHP catch bridge.

## Decisions Log (execution refinements)
- [2026-06-22] AGREED (during 2a execution): **`?`-on-Result is restricted to a let-initializer
  position** — the *entire* initializer of a `var`/typed binding (`int a = lookup()?;`) — where the PHP
  lowering is a clean 3-line hoist (`$t = expr; if ($t instanceof Err) return $t; $x = $t->value;`). A `?`
  anywhere else (nested, e.g. `f(g()?)`, or `return foo()?` — which would return the unwrapped `T` where
  the fn returns `Result`, a type error anyway) is `E-PROPAGATE-POSITION` (hint: bind to a local first).
  Reason
  (verified): PHP cannot caller-return from an expression, and a general A-normal-form hoist is
  out-of-scope for 2a; the VM/interpreter handle `?` at expression level fine (`do_return` truncates to
  the frame base — early-return-on-`Err` works even nested), so the restriction is a PHP-fidelity
  constraint enforced uniformly by the checker. Nested-`?` (the hoist pre-pass) is deferred.
- [2026-06-22] AGREED: tasks 2a.1–2a.3 land as **one commit** ("Result `?` propagation") — Rust's
  exhaustive-match requirement means the `Expr::Propagate` variant can't compile green until parse +
  check + all-backend lowering are all wired.

## Formal Plan

> Plan style = the project house format (ordered steps + acceptance + rollback), which overrides the
> superpowers bite-sized-full-code default (`User preferences override`). One plan, **phased**; a review
> checkpoint between phases; **each phase is its own green, byte-identical commit** with a guide example.
> Per the skill's scope-check, the three phases are independent subsystems — **phase 2a is detailed
> below and built first; 2b and 2c each get their own detailed plan appended here once the prior phase
> lands** (the full design for all three already lives in the approved spec).

### Global constraints (every task)
- `export PATH=/stack/tools/cargo/bin:$PATH`. Gate before every commit: `cargo test`
  (`PHORGE_REQUIRE_PHP=1 PHORGE_PHP=/stack/tools/phpbrew/php/php-master/bin/php` so the PHP oracle
  *fails*, not skips) + `cargo clippy --all-targets` + `cargo fmt --check`. The pre-commit hook reruns
  fmt+clippy+test.
- **Byte-identity spine:** `run ≡ runvm ≡ real PHP` on every example/test program. TDD: add the
  differential/checker test first, watch it fail, implement, watch it pass.
- **`Op`-coupling discipline** (only relevant from 2b on): each new `Op` extends `src/vm.rs` `exec_op`,
  `src/chunk.rs` `BytecodeProgram::validate`, and `src/compiler.rs` `stack_effect` **in the same commit**.
- **Examples-ship-with-features:** every phase lands a runnable `examples/guide/*.phg` (byte-identity
  gated by the `examples/**/*.phg` glob) + an `examples/README.md` row, same commit.
- Git autonomy authorized here: commit green self-contained work; never push.

### Lexer fact (locks the `?` design — verified)
The lexer already maximal-munches `??`→`QuestionQuestion`, `?.`→`QuestionDot`, and a lone `?`→`Question`
(`src/lexer.rs:535-569`, `src/token.rs:70-72`). So the propagation operator is the **existing `Question`
token consumed in postfix position** — **no new token, no lookahead**. The "one-char lookahead" in the
spec is already done by the tokenizer.

---

### PHASE 2a — value tier + panics (front-end only, NO new `Op`) — built first

Self-contained: `Result` `?` propagation + the `panic`/`todo`/`unreachable`/`assert` intrinsics. Lowers
to existing machinery (enum-match + `return`, and `Op::Fault`). Completes the `never` story.

**Files touched:** `src/ast.rs` (`Expr::Propagate`), `src/parser.rs` (`parse_postfix` `Question` arm),
`src/checker.rs` (propagate typing + intrinsic recognition), `src/interpreter.rs` + `src/compiler.rs`
+ `src/vm.rs` (lower propagate via existing enum-tag-test + return; intrinsics via `Op::Fault`),
`src/transpile.rs` (`__phorge_try` helper for Result-`?`; intrinsics → PHP throw),
`examples/guide/result.phg`, `examples/guide/errors.phg` is **2b** (this phase is Result+panic only).

**Task 2a.1 — `Expr::Propagate` parse.** Add `Expr::Propagate(Box<Expr>, Span)` to `ast.rs`. In
`parse_postfix` (`src/parser.rs:258`), add a `TokenKind::Question` arm *after* the `Bang` arm, wrapping
the current expr: `e = Expr::Propagate(Box::new(e), sp)`. TDD: parser test asserting `a?` parses as
`Propagate(Ident a)` and `a?.b` still parses as a safe `Member` (proves no collision). Update
`ast::free_vars`/any exhaustive `Expr` match (`collect_free_expr`, the transpiler/compiler/interpreter
`match` arms — the compiler will flag every non-exhaustive site; fix each). Commit.

**Task 2a.2 — checker: `?` typing (Result mode only this phase).** In `check_expr`, add an
`Expr::Propagate(inner)` arm: type `inner`; if it is `Ty::Named("Result", [t, e])`, the propagate value
is `t`, and the **enclosing function must return `Result<_, e'>` with `e <: e'`** (track the current
fn's return type — the checker already stores it for return-checking; reuse that) else
`E-PROPAGATE-CONTEXT`. (A `throws`-call operand is **2b** — until then, `?` on a non-Result is
`E-PROPAGATE-CONTEXT`.) TDD: checker tests — `?` on a `Result` inside a `Result`-returning fn is clean;
inside a non-`Result` fn errors; `?` on an `int` errors. `phg explain E-PROPAGATE-CONTEXT`. Commit.

**Task 2a.3 — lower `?` on the three backends (no new `Op`).** `x?` where `x: Result<T,E>` ≡
`match x { Ok(v) => v, Err(e) => return Err(e) }`. Implement by lowering in each backend exactly as the
existing variant-`match` + `return` do:
- *Interpreter* (`src/interpreter.rs`): eval `inner`; if `Ok` payload → value; if `Err` → return the
  `Err` instance as the function result (reuse the existing `return` signal).
- *Compiler/VM* (`src/compiler.rs`/`src/vm.rs`): emit the enum-tag test (reuse `Op::IsInstance`/the
  variant-discriminant test the compiler already emits for a `match` arm) + `JumpIfFalse` to an
  Err-return path that reconstructs/forwards the `Err` and emits the existing return op. **No new `Op`.**
- *Transpiler* (`src/transpile.rs`): a once-per-file `__phorge_try` helper — `function __phorge_try($r){
  if ($r is Err) return [false,$r]; return [true,$r->value]; }` pattern, or inline an
  `if ($r instanceof Err) { return $r; } $v = $r->value;` at the call site (match the existing
  `__phorge_*` helper convention; pick inline if cleaner). TDD: `tests/differential.rs` case — a
  `Result`-returning fn using `a?` + `b?` runs byte-identical on run/runvm/PHP for both the `Ok` and the
  early-`Err` path. Commit.

**Task 2a.4 — panic/todo/unreachable intrinsics (`never`).** In `check_expr`'s `Expr::Call` arm,
recognize a bare callee `panic`(1 string arg)/`todo`(0)/`unreachable`(0); type them `Ty::Never`
(reserve the names in `is_builtin_type_name`-adjacent validation so a user can't shadow them — add
`E-RESERVED-INTRINSIC`). Lower: interpreter → `Err(Fault(msg))`; VM → `Op::Fault(FaultMsg)` (reuse —
**no new Op**, the message is the panic string / a fixed `"not yet implemented"` / `"unreachable"`);
transpiler → `throw new \RuntimeException($msg)` (panic/todo) / `\LogicException` (unreachable). Add
`FaultKind::Panic` to `tests/differential.rs` so `agree_err` classifies them. TDD: differential
`agree_err` case — `panic("boom")` faults identically on run/runvm; a `never`-typed `panic` at a fn tail
satisfies return-on-all-paths (no `E-MISSING-RETURN`). Commit.

**Task 2a.5 — `assert(bool, string?)`.** Recognize `assert` in `check_expr` (1-2 args, returns `unit`);
lower to `if (!cond) <fault "assertion failed: {msg}">` using the existing branch ops + `Op::Fault`
(interpreter `Err(Fault)`); transpiler → `if (!$c) { throw new \RuntimeException(...); }`. TDD:
differential — `assert(true)` is a no-op (byte-identical), `assert(false,"x")` faults identically. Commit.

**Task 2a.6 — example + docs.** `examples/guide/result.phg`: a `Result<T,E>`-returning pipeline using
`a?`/`b?` (both `Ok` and `Err` paths, printed) + a `panic`/`assert` shown in prose comments (faults can't
be in a runnable example). `examples/README.md` row + coverage-matrix line. KNOWN_ISSUES: panics are
uncatchable-by-design (until 2b there's no `catch` anyway). Update `CHANGELOG.md` + `m-rt-progress`
memory. Run the full gate with `PHORGE_REQUIRE_PHP=1`. Commit.

**Phase 2a acceptance:** `?` on `Result` + `panic`/`todo`/`unreachable`/`assert` byte-identical
run≡runvm≡real PHP; new checker codes self-document via `phg explain`; full suite green; clippy+fmt
clean; **no new `Op`**. → review checkpoint, then write the detailed 2b plan.

---

### PHASE 2b — exceptions (control-flow core, **3 new `Op`s**) — DETAILED

Headline landing: built-in `Error` base, `throws E` declaration + enforcement, `throw`,
`try`/`catch`/`finally` (native unwinding), `?`-throws mode, throw-across-native, PHP exception mapping.
Decisions for this phase are pinned in the **Decisions Log (2b plan-shaping)** above — read it first.

**New surface (all phase-2b):**
- Keywords: `throw` `try` `catch` `finally` `throws`.
- AST: `Stmt::Throw { value: Expr, span }`; `Stmt::Try { body: Vec<Stmt>, catches: Vec<CatchClause>,
  finally_block: Option<Vec<Stmt>>, span }`; `pub struct CatchClause { ty: Type, name: String, body:
  Vec<Stmt>, span }` (`ty` may be a union `Type` for `catch (A | B e)`); `FunctionDecl.throws: Vec<Type>`.
- Built-in `Error` interface (one method `message() -> string`) → PHP base `\Exception` (`.message()` ⇒
  `getMessage()`); a throwing class is `class P implements Error` ⇒ PHP `class P extends \Exception`.
- Checker error codes: `E-THROW-UNDECLARED`, `E-CALL-UNHANDLED`, `E-UNCAUGHT-THROW`,
  `E-THROWS-TOO-BROAD`, `E-CATCH-TYPE` (catch type not `<: Error`); warning `W-CATCH-UNREACHABLE`.
  (`E-PROPAGATE-CONTEXT`/`-ERR` from 2a are extended to the throws mode.)
- VM: `enum VmError { Throw(Value), Fault(String) }` (`From<String>`=`Fault`); `Op::Throw`,
  `Op::PushHandler(usize)`, `Op::PopHandler`; `handlers: Vec<Handler>` + `pending_throw: Option<Value>`.

**TDD discipline (every task):** add the failing checker/interpreter/differential test first, watch it
fail, implement, watch it pass. Each task ends green under the full gate; the byte-identity differential
case for a task only lands once *both* backends for that behavior exist (staged so untouched arms are
never exercised → green between commits, the same trick 2a used).

---

**Task 2b.1 — keywords + AST + parser + exhaustive-match stubs (one commit).**
- `src/token.rs`/`src/lexer.rs`: add the five keywords. Parser tests assert `throw`/`try`/etc. tokenize.
- `src/ast.rs`: add `Stmt::Throw`, `Stmt::Try`, `pub struct CatchClause`, `FunctionDecl.throws`
  (default `vec![]` at every construction site — grep `FunctionDecl {`). Extend `ast::stmt_*`
  helpers and `collect_free_*` for the two new statements (a `Try` introduces the catch binding into its
  catch body's free-var scope).
- `src/parser.rs`: `parse_throws()` after the `-> Ret` clause (`throws T (| T)*`, reuse `parse_type`);
  `parse_statement` arms for `throw expr;` and `try { } catch (Type name) { } … [finally { }]` (≥1 catch
  **or** a finally required — a bare `try {}` is `E`-… handled in checker, but the parser accepts
  `try`+(catch+|finally)). A `catch (A | B e)` parses `ty` via the existing union `parse_type`.
- **Stub** the new `Stmt`/`throws` arms in every exhaustive match so the crate compiles:
  `src/checker.rs`, `src/interpreter.rs`, `src/compiler.rs`, `src/transpile.rs`, `src/loader.rs`
  (`resolve_*`). Stubs are `unreachable!("2b: throw/try not yet lowered")` / no-op rebuilds — **never hit**
  because no program/example uses the surface yet, so the full suite stays green.
- Parser unit tests: `parses_throw_stmt`, `parses_try_catch_finally`, `parses_multi_catch`,
  `parses_union_catch`, `parses_fn_throws_clause`. Run the gate. **Commit** (`feat: parse throw/try/catch/finally + throws clause`).

**Task 2b.2 — built-in `Error` base type (checker + transpile).**
- Register a built-in `Error` interface (method `message() -> string`) in the checker's type universe;
  reserve the name in `is_builtin_type_name` (cannot be user-shadowed). A class may `implements Error`
  through the existing `implements` machinery; `instanceof Error` works for free (S1/S2).
- Transpile: a class `implements Error` emits PHP `extends \Exception` (not `implements`); the built-in
  `Error` itself emits nothing (it's `\Throwable`/`\Exception`). `.message()` on an `Error`-typed value ⇒
  `->getMessage()`. **Message-collision wrinkle (resolve here, with a test):** a throwing class with a
  promoted `string message` field must route construction through `parent::__construct($message)` so PHP's
  `getMessage()` returns it — do **not** emit a colliding `public $message`. Differential: a class
  `implements Error` constructed + `.message()` read prints byte-identical on run/runvm/**real PHP**.
- Checker tests: `class implements Error` accepted; `instanceof Error`; `Error` name reserved
  (`E`-shadow). Run gate. **Commit** (`feat: built-in Error base type (-> PHP \Exception)`).

**Task 2b.3 — checker: `throws` enforcement + `?`-throws + catch typing + totality (front-end, no backend).**
- Track the enclosing fn's declared throws-set: add `cur_throws: Vec<Ty>` to the checker, saved/restored
  exactly like `cur_ret` (the fn-body checking sites at `checker.rs:1130/1505/2523`). A type is
  *discharged* if it is `<:` some member of `cur_throws` (reuse `assignable_with`/the union subtype path).
- **Enforcement:**
  - `throw e` (`e: E`, `E <: Error` else `E-CATCH-TYPE`-sibling `E-THROW-TYPE`): `E` must be discharged in
    context (declared-throws **or** inside a `try` whose catches cover it) else `E-THROW-UNDECLARED`.
  - Calling a `throws E` fn: `E` must be discharged via propagation (`?` + enclosing `throws`), or be
    inside a covering `try` — else `E-CALL-UNHANDLED`.
  - `main()` may not declare `throws`; any throw reaching it uncaught ⇒ `E-UNCAUGHT-THROW`.
  - A `throws` declaration naming the bare `Error` root ⇒ `E-THROWS-TOO-BROAD` (declare specific). A
    `catch (Error e)` is allowed (catch broad).
- **`?` typing — extend the 2a `Expr::Propagate` arm:** if the operand is a `throws`-call → result is the
  call's return `T`, and the call's `E` must be discharged (declared-throws/`?`); **erase the node to its
  inner expr** (record it for the post-check erase pass — `?`-throws has no backend codegen, Decisions
  Log). If the operand is a `Result<T,E>` → the 2a path (unchanged). Neither ⇒ `E-PROPAGATE-CONTEXT`.
- **Catch typing:** each `CatchClause.ty` must be `<: Error` (`E-CATCH-TYPE`); bind `name: ty` in the
  catch body scope (smart-cast: inside the body `name` *is* `ty`); a clause whose `ty` is `<:` an earlier
  clause's `ty` (or equal) ⇒ **`W-CATCH-UNREACHABLE`** (push to the warning channel; one per shadowed
  clause). A `catch (A | B e)` types `e` as the S4 union `A | B`.
- **Totality:** extend `block_terminates`/`stmt_terminates` (totality cluster) with a `Stmt::Try` arm — a
  `try` terminates iff its body **and** every catch terminate, **and** (if present) `finally` does not
  fall through. `throw` is `never`-typed (a `throw` statement diverges — its expr is `never`).
- Erase pass: in `checker::erase_*` (the chokepoint that already erases aliases/generics), unwrap every
  throws-mode `Expr::Propagate` to its inner expr. (Result-mode `Propagate` is left for the backends.)
- Checker tests (programs that *fail in the checker* never reach the stubbed backends, so this is green):
  each new code + `W-CATCH-UNREACHABLE` + `?`-throws discharge + catch union typing + `try` totality
  (a `try` whose arms all `throw`/`return` satisfies a `-> T` tail). `phg explain` for each code.
  Run gate. **Commit** (`feat: throws enforcement + ?-throws + catch typing + try totality (checker)`).

**Task 2b.4 — interpreter: native unwinding (`Signal::Throw` + try/catch/finally + native side-channel).**
- `src/interpreter.rs`: add `Signal::Throw(Value)`. `Stmt::Throw` ⇒ `Err(Signal::Throw(v))`. `Stmt::Try`:
  run `body`; on `Err(Signal::Throw(v))` walk `catches` in order, first whose `ty` matches `v` by
  `instanceof` (reuse the S1/S2 `class_implements` oracle + union membership) binds `name=v` and runs its
  body; if none match, the `Throw` re-propagates. **`finally` runs on every exit edge** (normal, caught,
  re-propagated, and a `Return`/`Break`/`Continue` escaping the try) — implement by running the finally
  block in all arms before returning the arm's result. `Signal::Fault`-equivalent (`Signal::Runtime`)
  **passes straight through** every catch (panics uncatchable) — only `Signal::Throw` is caught.
- **Native boundary (Decisions Log):** the higher-order invoker (`call_closure`) currently maps a
  `Signal` to a `String` via `signal_msg`. Extend: on `Signal::Throw(v)`, stash `v` in
  `self.pending_throw` and return the reserved sentinel body `__phorge_throw__`; everywhere a native call
  returns `Err(msg)` with `msg == SENTINEL && self.pending_throw.is_some()`, rebuild
  `Err(Signal::Throw(self.pending_throw.take()))`. So a `throw` inside a `Core.List.map` closure unwinds
  to an outer `try`.
- Interpreter-targeted unit tests (call `interpret` directly — VM still stubbed, so **no differential
  case yet**): caught exception returns the catch value; uncaught (in a non-`main` helper, caught in
  `main`) works; `finally` runs on normal + exceptional exit; a `panic` is **not** caught by a
  surrounding `catch`; `throw` inside `map` caught outside. Run gate. **Commit**
  (`feat: interpreter native unwinding — try/catch/finally + throw-across-native`).

**Task 2b.5 — VM: 3 new `Op`s + handler stack + compiler codegen (run ≡ runvm).**
- `src/chunk.rs`: add `Op::Throw`, `Op::PushHandler(usize)`, `Op::PopHandler`. `validate`:
  `PushHandler(addr)` bounds-checks `addr` against the enclosing function's code length (the only one
  carrying an index; `Throw`/`PopHandler` carry none — no `validate` arm, like `MakeRange`/`Fault`).
- `src/vm.rs`: `enum VmError { Throw(Value), Fault(String) }` + `impl From<String>`; change `exec_op`,
  `run`, `run_until`, `call_closure_value` error types to `VmError` (the `From` makes existing `?` sites
  auto-wrap as `Fault`; convert the explicit `return Err(format!…)` sites — the compiler lists them).
  Add `handlers: Vec<Handler>` (`struct Handler { catch_ip: usize, frame_depth: usize, stack_height:
  usize }`) + `pending_throw: Option<Value>`.
  - `Op::PushHandler(addr)` ⇒ push `Handler { catch_ip: addr, frame_depth: self.frames.len(),
    stack_height: self.stack.len() }`. `Op::PopHandler` ⇒ pop the top handler.
  - `Op::Throw` ⇒ pop value, `return Err(VmError::Throw(v))`.
  - **Unwind (shared helper used by both `run` and `run_until`):** on `Err(VmError::Throw(v))`, if a
    handler exists *at or above* the loop's floor: pop frames to `handler.frame_depth`, truncate stack to
    `handler.stack_height`, pop the handler, push `v`, set the landed frame's `ip = handler.catch_ip`,
    continue. If the nearest handler is **below** `run_until`'s `target_depth` (the throw escapes a native
    closure), stash `v` in `pending_throw` and return the sentinel `Err(VmError::Fault(SENTINEL))` so the
    native propagates it; the `CallNative` op site rebuilds `VmError::Throw(pending_throw.take())`.
    A `Throw` with **no** handler anywhere is a defensive backstop (the checker's `E-UNCAUGHT-THROW`
    guarantees `main` can't leak one) — surface as an uncaught-fault `Diagnostic`.
  - `do_return`/normal frame exit must drop handlers whose `frame_depth >= frames.len()` after the pop.
- `src/compiler.rs` (`compile_stmt` + `stack_effect`):
  - `stack_effect`: `Throw` = `-1`, `PushHandler` = `0`, `PopHandler` = `0`. The catch landing pushes the
    thrown value (`+1`) — modeled at the landing-pad height, like a `match` scrutinee.
  - `Stmt::Throw(e)` ⇒ compile `e`, emit `Op::Throw`.
  - `Stmt::Try` ⇒ emit `PushHandler(catch_lp)`; compile `body`; emit `PopHandler`; **emit `finally`
    inline**; `Jump(end)`. At `catch_lp` (thrown value on stack): for each clause emit `IsInstance(ty)` (a
    union clause = OR-chain of `IsInstance` per member) → `JumpIfFalse(next_clause)` → bind (store to the
    clause's local slot) → compile clause body → **emit `finally` inline** → `Jump(end)`. After the last
    clause's no-match fall-through: **emit `finally` inline** → `Op::Throw` (re-throw the value still on
    stack). `end:` is past everything.
  - **finally-before-transfer:** maintain a compiler `finally_stack` (active enclosing finally blocks).
    `Stmt::Return`/`Break`/`Continue` emitted while inside a try-with-finally first emits the pending
    finally block(s) between the current point and the target, *then* the transfer op. (Tracks
    try-finally nesting relative to the target loop/fn.)
- First **differential** cases (now run ≡ runvm): caught exception value + path; `finally` ordering on
  normal/caught/rethrow; `return`/`break`/`continue` through a `finally`; throw-across-`map` caught
  outside; nested try; multiple + union catch dispatch; `panic` bypasses `catch`. Run gate. **Commit**
  (`feat: VM native unwinding — Op::Throw/PushHandler/PopHandler + finally codegen`).

**Task 2b.6 — transpile + PHP-oracle parity (run ≡ runvm ≡ real PHP).**
- `src/transpile.rs`: `Stmt::Throw` ⇒ `throw $e;`; `Stmt::Try` ⇒ PHP `try { } catch (\Type $e) { } …
  [finally { }]` 1:1 (multiple clauses 1:1; a union clause ⇒ PHP 8 `catch (A | B $e)`); `throws`
  declaration ⇒ erased (optionally a `@throws` docblock — skip for byte-identity simplicity). `?`-throws
  is already erased to the bare call by the checker (Task 2b.3) ⇒ nothing special here. `Error` mapping
  per Task 2b.2.
- Drive the **full differential under `PHORGE_REQUIRE_PHP=1`** so the PHP leg *fails* (not skips):
  every 2b.5 case must match **real PHP** too. Resolve any divergence (most likely the `\Exception`
  message wiring + catch-clause FQN `\` prefixing for `package main` global classes). Run gate.
  **Commit** (`feat: transpile throw/try/catch/finally -> PHP exceptions (3-way byte-identical)`).

**Task 2b.7 — example + docs.**
- `examples/guide/errors.phg`: a runnable program (must produce identical `Ok` output) exercising
  `throws`/`throw`, `try`/multiple `catch (X e) catch (Y e)`/`catch (A | B e)`/`finally`, and `?`-throws
  propagation — all caught so the program completes normally and prints deterministic lines. (Panics
  **cannot** be in a runnable example — documented in prose + KNOWN_ISSUES.)
- `examples/README.md` row + coverage-matrix line. `KNOWN_ISSUES.md`: panics uncatchable-by-design;
  cross-native throw **now supported**; multi-type catch supported; `finally`-returns-value not supported.
  `CHANGELOG.md` + `docs/MILESTONES.md` + `CLAUDE.md` M-faults paragraph + `error-model-slice2-progress`
  + `m-rt-progress` memory. `phg explain` covers all new codes (test like 2a's `explain_covers_*`).
  Full gate `PHORGE_REQUIRE_PHP=1`. **Commit** (`feat(examples): errors.phg guide + 2b docs`).

**Phase 2b acceptance:** `throws`/`throw`/`try`/`catch`/`finally`/`?`-throws byte-identical
run≡runvm≡real PHP (incl. throw-across-native, multiple + union catch, finally on every edge); all new
codes + `W-CATCH-UNREACHABLE` self-document via `phg explain`; totality engine handles `try`; full suite
green; clippy+fmt clean; **exactly 3 new `Op`s**, each with the three coupled matches. → review
checkpoint, then write the detailed 2c plan.

### PHASE 2c — cause-chain + imported-PHP catch bridge — OUTLINE
(`finally` moved **into 2b** per the plan-shaping Decisions Log.) Remaining 2c scope: exception
cause-chain (`A-fault-cause-chain`, hung off the `Error` base — a `cause: Error?` + `.cause()`,
transpiling to `\Exception`'s `$previous`/`getPrevious()`); catching PHP-thrown exceptions across the
interop boundary (the imported-PHP `catch` bridge). *Detailed task breakdown authored at the 2b→2c
checkpoint.*

## Self-review (plan vs spec)
- Spec §2 surface (`throws`/`try`/`catch`/`finally`/`?`/panics) → 2a covers `?`(Result)+panics; 2b
  covers `throws`/`throw`/`try`/`catch`/**`finally`**+`?`(throws); 2c covers cause-chain + PHP bridge.
  ✓ full coverage across phases (finally pulled forward into 2b per Decisions Log).
- Spec §3 enforcement + `Error` base → 2b.2/2b.3. §4 backends: 2a front-end/no-Op; §4.2 interpreter
  `Throw` split → 2b.4; §4.3 VM Ops (**pinned at 3**) → 2b.5; §4.4 PHP map → 2b.6; §4.5 totality → 2b.3.
  §5 testing/examples → per-task TDD + 2b.7 guide example. ✓
- Spec §7 non-goals: multi-type single catch is **adopted into 2b** (was "TBD at plan time" → developer
  chose Option 2 + multiple clauses; resolved). `finally`-returns-value still a non-goal. ✓
- Placeholder scan: 2b is fully detailed (concrete files/steps/tests/codes); 2c is an intentional outline
  (skill scope-check: one detailed plan per subsystem, written at its checkpoint). ✓
- Type/name consistency: `Expr::Propagate` (2a, reused), `Stmt::Throw`/`Stmt::Try`/`CatchClause`,
  `FunctionDecl.throws`/`cur_throws`, `VmError`/`Handler`/`pending_throw`, `Op::Throw`/`PushHandler`/
  `PopHandler`, the `E-*`/`W-CATCH-UNREACHABLE` codes — consistent across plan + spec + Decisions Log. ✓
- Op-coupling: 2b.5 names the three coupled matches (`exec_op`+`validate`+`stack_effect`) in one commit. ✓
