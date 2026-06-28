# M-faults Slice 2 — Error Model (design)

**Status:** Designed — not yet implemented. Supersedes the bare "Result-first" framing of
`A-exceptions`/`B-result` in `docs/specs/2026-06-21-php-parity-and-beyond.md` §2.1 (which locked the
high-level shape on 2026-06-22). Plan: `docs/plans/2026-06-22-error-model-slice2.plan.md`.

## 1. Goal & principle

Phorj gets a **three-tier error model** under one enforced-failure principle — *a value-carrying
failure path is always visible in the type system, and a "bug" failure always crashes loudly*. The three
tiers, from most-PHP-familiar to most-functional to uncatchable:

1. **`throws E`** — enforced, *typed* exceptions. The fix to PHP's unenforced `@throws` docblock:
   checker-enforced at the call site, `?`-propagable, **a specific error type required** (no bare
   `throws Exception` swallow). Transpiles to **idiomatic PHP exceptions**. The PHP-familiar default.
2. **`Result<T, E>`** — error-as-value (functional, `match` / `?`). Already expressible: it *is* the
   generic enum shipped in M-RT (`enum Result<T, E> { Ok(T value), Err(E error) }`). This slice adds the
   `?` propagation ergonomics (and, later, stdlib combinators).
3. **Unchecked faults / panics** — programmer bugs / invariant violations (`panic`, `todo`,
   `unreachable`, failed `assert`, plus the existing index-OOB / force-unwrap-null). They *crash* with a
   Slice-1 stack trace and are **never declared up the call chain** and **never caught** by user
   `try/catch` — the explicit fix to Java's "everything is checked" mistake.

Both checked tiers (1, 2) are typed, checker-enforced, and `?`-composable. **Hard invariant:**
`run ≡ runvm ≡ real PHP`, byte-identical, preserved at every commit (`PHORJ_REQUIRE_PHP=1`).

`throw` / `panic` / `todo` / `unreachable` are **`never`-typed** — they complete the totality story
(a function ending in `throw`/`panic` satisfies return-on-all-paths for free).

## 2. Surface syntax (Section A)

```phorj
package Main;
import Core.Console;

// Tier 1 — a typed, enforced exception. `ParseError` is a subtype of the core `Error` base.
class ParseError implements Error {
    constructor(public string message) {}
}

function parsePort(string s) -> int throws ParseError {
    if (!allDigits(s)) { throw ParseError("not a number: {s}"); }  // `throw` is `never`-typed
    return toInt(s);
}

function main() {
    // (a) handle with try/catch
    try {
        int p = parsePort("8080");
        Console.println("port {p}");
    } catch (ParseError e) {
        Console.println("bad: {e.message}");
    } finally {
        Console.println("done");
    }
}

// (b) propagate: this fn declares it throws too
function openConfig(string s) -> Config throws ParseError {
    int p = parsePort(s)?;   // `?` — unwrap or re-throw (caller's `throws` set must cover ParseError)
    return Config(p);
}

// Tier 2 — error-as-value
function lookup(Map<string, int> m, string k) -> Result<int, string> {
    if (m.has(k)) { return Ok(m[k]); }
    return Err("missing {k}");
}
function total(Map<string, int> m) -> Result<int, string> {
    int a = lookup(m, "a")?;   // `?` on a Result — unwrap Ok, or early-return Err
    int b = lookup(m, "b")?;
    return Ok(a + b);
}

// Tier 3 — panics (uncatchable bugs)
function head(List<int> xs) -> int {
    if (xs.size() == 0) { panic("head of empty list"); }  // `never`
    return xs[0];
}
```

**The `?` propagation operator** is postfix (locked by §2.1 lines 41/43). It collides syntactically with
`?.` (safe-nav) and `??` (coalesce), resolved by a **one-char lexer lookahead**: a `?` is the
propagation operator **only when not immediately followed by `.` or `?`** (`x?;`, `x?)`, `x? + 1` =
propagation; `x?.f` = safe-nav; `x ?? y` = coalesce). `?` is **type-directed**: on a `throws`-call it
propagates the throw; on a `Result` value it unwraps `Ok`/early-returns `Err`.

## 3. Type model & enforcement (Section B)

### 3.1 The `Error` base type
A thrown value must be a subtype of a **built-in `core` `Error`** type (an interface — `class X implements
Error`). It transpiles to a PHP class extending `\Exception`, which gives: a real `\Throwable` in PHP, a
home for `.message()` and the (Slice-2c) cause chain, and a clean upper bound for `throws` / `catch`.

### 3.2 Enforcement (checker, front-end-only — the `throws` *declaration* erases before any backend)
- A function carries a **`throws` set** = the union of its declared `throws` types.
- Every potential throw must be **discharged**, one of three ways:
  1. **Propagate** — the enclosing fn declares `throws E'` with `E <: E'`.
  2. **`?`-propagate** — same requirement as propagation, at an expression.
  3. **Catch** — inside a `try` whose `catch` clauses cover `E`.
- `throw e` (`e: E`) requires `E` discharged in the enclosing context (else `E-THROW-UNDECLARED`).
- Calling a `throws E` function requires `E` discharged (else `E-CALL-UNHANDLED`).
- **`main()` may not declare `throws`** → any throw reaching it uncaught is `E-UNCAUGHT-THROW`. This is
  the enforcement teeth: total handling is verified at compile time (PHP would only fatal at runtime).
- **Declare specific, catch broad:** a `throws` declaration must name a *specific* subtype — naming the
  bare `Error` root is `E-THROWS-TOO-BROAD` (the "no swallow" rule). A `catch (Error e)` *may* catch
  broadly. `throws A | B` reuses the S4 union machinery.

### 3.3 `?` typing
The operand's type selects the mode:
- operand is a `throws`-call → unwrap to `T`, re-throw `E` (caller's `throws` set must cover `E`).
- operand is a `Result<T, E>` → unwrap `Ok` to `T`, early-`return Err(e)` (enclosing fn must return
  `Result<_, E'>` with `E <: E'`).
A `?` whose context can satisfy neither is a checker error (`E-PROPAGATE-CONTEXT`).

## 4. Backends & PHP mapping (Section C)

### 4.1 AST / lexer
New keywords `throw` `try` `catch` `finally` `throws`; `Stmt::Throw(Expr)`,
`Stmt::Try { body, catches: Vec<CatchClause>, finally: Option<Block> }`, `Expr::Propagate(Box<Expr>)`
(the postfix `?`), `FunctionDecl.throws: Vec<Type>`, classes carry `implements Error` (existing
`implements` machinery). `panic` / `todo` / `unreachable` / `assert` are **front-end intrinsics** (not
`core` natives — a native signature cannot express `never`): the checker recognizes them, types them
`never` (`assert` → `unit`, but `assert(false, …)` faults), and lowers them to the existing `Op::Fault`.

### 4.2 Interpreter — the error channel becomes two-variant
Today the error channel is `Err(String)` (an uncatchable fault). It becomes a **signal** with two
variants:
- **`Throw(Value)`** — a catchable thrown instance.
- **`Fault(msg)`** — an uncatchable panic / runtime fault.

A `try` boundary catches `Throw(v)` when a `catch (E e)` matches by `instanceof`, binds `e`, runs the
handler; `finally` always runs (normal *and* exceptional exit); a `Fault` propagates straight through
every user `catch` (panics are uncatchable, by design).

### 4.3 VM — native unwinding (≈2 new Ops, exact count pinned at plan time)
- `Op::Throw` — pops the thrown value and unwinds, carrying it (generalizes the existing `Op::Fault`
  `Err`-propagation, but value-carrying and *catchable*).
- A handler mechanism (`Op::PushHandler(catch_addr)` / `Op::PopHandler`, counted as one mechanism):
  `try` installs a landing pad (catch address + the operand stack height + the `instanceof` tests);
  normal exit pops it; a thrown value unwinds frames to the nearest handler and jumps to its catch
  address with the value on the stack. `finally` is compiler-emitted on both the normal and the
  unwinding path.
- The re-entrant `run_until` (higher-order natives) must respect handler frames.
- **`Op`-coupling discipline:** every new `Op` extends the three coupled matches in one commit —
  `src/vm.rs` `exec_op`, `src/chunk.rs` `BytecodeProgram::validate`, `src/compiler.rs` `stack_effect`.

### 4.4 PHP transpile
- `throws E` declaration → **erased** (PHP has no checked exceptions; optionally a `@throws` docblock).
- `throw e` → `throw $e;`. `try { } catch (E e) { } finally { }` → PHP `try/catch/finally`, 1:1.
- core `Error` → a PHP class `extends \Exception`; `.message()` → `getMessage()`.
- **`?` in a `throws` context → just the bare call** (PHP propagates exceptions automatically:
  `parsePort(s)?` ⇒ `parsePort($s)`). `?` in a `Result` context → a generated early-return-on-`Err`
  helper (`__phorj_try`), analogous to the existing `__phorj_div`/`__phorj_str` runtime helpers.
- `panic` / `todo` / `unreachable` → `throw new \RuntimeException(…)` / `\LogicException(…)`; `assert` →
  a guarded throw (tier-1-only PHP, no ini-loaded extensions per the transpile policy).

### 4.5 Totality integration
`throw` / `panic*` are `never`-typed, so the existing `block_terminates` / `stmt_terminates` engine
already treats them as diverging. Extend the engine for `try`: a `try` terminates iff its body **and**
every `catch` terminate (and `finally` doesn't fall through).

## 5. Testing (Section D)
- **Checker tests:** `E-UNCAUGHT-THROW`, `E-THROWS-TOO-BROAD`, `E-THROW-UNDECLARED`,
  `E-CALL-UNHANDLED`, `E-PROPAGATE-CONTEXT`; `throws A | B` subtyping; `?` typing in both contexts;
  `catch` binding + smart-cast + coverage. Each code self-documents via `phg explain`.
- **Differential (the spine):** caught-exception **control flow** byte-identical on run/runvm/real-PHP
  (the value after a catch, the path taken, `finally` ordering); `?` propagation (throws + Result);
  panics via `agree_err` + a new `FaultKind::Panic`. Run under `PHORJ_REQUIRE_PHP=1`.
- **Examples** (examples-ship-with-features): caught exceptions / `Result` produce normal `Ok` output, so
  they're runnable + byte-identity-gated — `examples/guide/errors.phg` (`throws`/`try`/`catch`/`finally`
  + `?`) and `examples/guide/result.phg` (`Result` + `?`). **Panics can't be runnable examples** (every
  example must produce identical `Ok` output) — documented in `examples/README.md` + KNOWN_ISSUES.

## 6. Sub-slice ordering (cadence decided at plan time)
The full model is designed here; whether it lands in one plan or these sub-slices is decided when the
implementation plan is written (developer leans one-shot; author leans incremental to isolate the
try/catch runtime risk):
- **2a — value tier + panics** *(front-end only, no new `Op`)*: `Result` `?` propagation +
  `panic`/`todo`/`unreachable`/`assert` over `Op::Fault`. Completes `never`. Smallest, safest.
- **2b — exceptions** *(control-flow core, ≈2 new `Op`s)*: core `Error`, `throws E` + enforcement,
  `throw`, `try/catch`, `?` in throws-context, PHP exception mapping. The headline landing.
- **2c — `finally` + cause-chain + imported-PHP catch bridge** (`A-fault-cause-chain` folds in).

## 7. Non-goals / deferred
- No multi-catch type *unions in one clause* beyond reusing `throws A | B` (a single `catch (A | B e)`
  may land in 2b or defer — TBD at plan time, called out so it isn't assumed).
- No `Result` stdlib combinators (`map`/`unwrapOr`/…) this slice — they need the higher-order-native
  path and ride a later stdlib slice.
- No retry/`finally`-returns-value exotica; `finally` is side-effect-only (matches PHP's common form).
- The same-head generic-invariance gap (KNOWN_ISSUES, shared with generic classes) is unrelated and
  untouched here.
