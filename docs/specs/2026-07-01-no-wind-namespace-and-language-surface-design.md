# "Nothing in the Wind" — Namespace & Language-Surface Design

> Status: **design-locked with the developer, 2026-07-01** (extended design session). Feeds a
> Fable-led deep audit + implementation plan. This is the SSOT for the decisions below; nothing here
> is implemented yet except where noted "EXISTS".

## The governing principle (developer's definition — authoritative)

**"In the wind" = a name (function/value/type) usable WITHOUT an explicit `import`.** The rule:
*nothing is usable without an explicit import*, with the single, closed exception of **the language
grammar itself** (keywords + built-in type words) — which cannot be imported because it is syntax.

Corollary the developer stressed: a name **imported to a bare call site is NOT in the wind** (it was
explicitly imported). So `import Core.List.doThis;` → `doThis(...)` is fine; the sin is the *absence*
of an import, not a bare call site.

## Decisions

### 1. Fault-intrinsics → `import Core;` then qualified `Core.assert(...)` — CONVERGED
- `panic` / `todo` / `unreachable` / `assert` are currently import-free call-syntax (checker
  `is_intrinsic_name`, `Op::Assert`/`Op::Fault`). This violates the rule.
- **Decision:** they move behind a mandatory **`import Core;`** and are called **qualified**:
  `Core.assert(cond[, "msg"])`, `Core.panic("msg")`, `Core.todo()`, `Core.unreachable()`. Using any of
  them without `import Core;` is **`E-UNIMPORTED`**.
- This is the synthesis: **qualified (attributed to the core) AND imported (nothing in the wind)**,
  while preserving their special semantics (`never`-typing for panic/todo/unreachable; compile-time
  literal message for the `--dump-on-fault` frame; guaranteed-not-stripped; lowers to PHP `throw`).
- Resolves the collision with the real native `Core.Test.assert` (test-runner assertion) — distinct
  surfaces, both now qualified.
- `Core` becomes a **reserved, importable language-core module** (the intrinsics live under it).

### 2. Deep / arbitrary-depth imports + dual call form
- Today: flat two-level `import Core.List;` → `List.foo(...)` (Go-style). PHP namespaces are
  arbitrary-depth, so this transpiles cleanly at any depth.
- **Decision:** support `import Core.A.B.C…` to **any depth**, selecting a **sub-module, function, or
  type** at the leaf. A selective deep import (`import Core.List.doThis;`) binds **BOTH**:
  (a) the **bare leaf** `doThis(...)` (legal because imported — not in the wind), AND
  (b) the **parent-qualified** path `List.doThis(...)` (the old way stays available).
- **No wildcards** (PHP `use` has none).
- Open sub-questions for Fable: ambiguity/shadowing across multiple deep imports; whether importing a
  deep element implicitly makes intermediate qualifiers available; interaction with `E-SHADOW-IMPORT`.

### 3. Import aliasing — EXISTS (extend)
- `import a.b as c;` and `import type a.b.C as D;` already implemented (M5 S2c, loader/mod.rs:477–492:
  `qualifier = alias.or(path.last())`). **Verified.**
- **Decision:** extend the same mechanism to **stdlib** (`import Core.List as MyList;`) and to **deep**
  imports (`import Core.List.doThis as ListDo;`) and **user** deep paths (`import MyApp.List as MyList;`).
  Small extension, no new machinery.

### 4. De-reserve built-in TYPE names that belong to importable modules
- Built-in reserved type set today (checker `is_builtin_type_name`): primitives (KEEP), `List`/`Map`/
  `Set` (KEEP — literal syntax justifies them), and the questionable ones below.
- **Decisions (developer-selected):**
  - **`Attr` → `Core.Html`** (require `import Core.Html;`). No literal-syntax justification; pure
    `Core.Html` territory. (`Html` STAYS built-in — it backs the `html"…"` typed literal, like
    `bytes`↔`b"…"`.)
  - **`Error` → `Core.Error`** (the `throws`-hierarchy root marker interface becomes importable).
  - **`Channel` / `Task` → `Core.Async`** (NOT `Core.Concurrent`). **The developer correctly rejected
    "Concurrent" as a misnomer:** Phorj green threads are **cooperative + single-threaded** (`Value`
    is `Rc`, not `Send`) — a `Task` is never parallel. `Core.Async` names what it actually is.

### 5. Real parallelism / async / concurrency — ON HOLD (Fable to deep-plan)
- **Current (shipped):** cooperative green threads (`spawn`/`Channel`/`Task`), single-threaded,
  **forced** by the `Rc`-shared heap (the 2.4× object win). No parallelism.
- **The insight:** the `Rc` memory model is a *commitment* that selects the concurrency model — shared-
  memory threading is off the table unless the fast path is abandoned.
- **Models brainstormed** (for Fable's spec):
  | Model | Cores | Fit | Precedent |
  |---|---|---|---|
  | Async-I/O reactor | 1 | near-term; green threads park on epoll (needs a reactor — `unsafe`/vetted crate) | Node.js |
  | **Actor / message-passing** | N | best structural fit; per-heap threads + owned-value channels; no data races by construction | Erlang/Elixir |
  | Data-parallel (`List.map` only) | N | rides existing immutability; shippable soonest; restricted | rayon |
  | Shared-memory `Send`/`Sync` | N | worst fit; kills the `Rc` win; huge type lift | Rust/Java |
- **Developer decision:** put on hold; **Fable to audit and produce a complete, deep M-Parallel plan**
  (100%-rich version + review + fix-plan).

## Earlier same-session feature decisions (context)
- **Q1 dynamic dispatch:** NO string-instantiate/string-call primitive (un-typeable/un-erasable). ADD
  **method-references-as-values** (`obj.method` → typed closure) + a typed-registry guide
  (`Map<string, () => T>`). (Recorded in the four-lane plan.)
- **Q2 filesystem:** stateless namespace — DONE this session (`Core.File` append/delete/rename/copy +
  size; module now impure/quarantined; `tests/filesystem.rs`). Commit `a23ca00`.
- **Q3 HTTP client:** full Guzzle-style incl. HTTPS; **admit `rustls`** under the crypto clause
  (wasm-gated); reuse M6 `Request`/`Response`; pooling via green threads; PHP-Guzzle transpile;
  socket quarantined behind a `Transport` trait. Milestone **M-HTTP-Client** (design-spec first).

## Handoff to Fable
Audit the whole language surface against the "nothing in the wind" rule, produce a complete deep
implementation plan (namespace-v2: intrinsics-under-Core, deep imports, aliasing, type de-reservation)
AND the M-Parallel plan, review for problems, and give a fix-plan. This document is the decision SSOT.
