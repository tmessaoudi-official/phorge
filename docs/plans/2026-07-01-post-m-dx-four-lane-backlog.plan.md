# Post-M-DX Four-Lane Backlog

> M-DX (Error Experience & Build Profiles) is **COMPLETE** (6 slices, `ffb2265..e72d3ba`). The
> developer approved building all four lanes below, in this confirmed order. Each is its own focused
> session (fresh context → the M-DX quality bar). All work stays byte-identical `run ≡ runvm ≡ real
> PHP` at the PHP-8.5 floor, clippy + fmt clean, committed per-slice (push is manual).

## Decisions Log
- [2026-07-01] AGREED: after M-DX, build all four lanes — Naming-overhaul, M-perf, VM-debug-symbols,
  Stdlib-breadth.
- [2026-07-01] AGREED: order = **Naming → M-perf → VM-symbols → Stdlib** (rework-minimization + gate
  -early + isolated-quality + additive-last).
- [2026-07-01] AGREED: fold five ADD candidates into this autonomous run — `phg repl`, `phg doc`,
  parser multi-error recovery, A2 generators/`yield`, plus opportunistic wins (doc-comments `///` with
  `phg doc`, `phg new`, `defer`, VM tail-call-opt inside M-perf, format specifiers with `sprintf`).
  Start = Lane 1 W1. Autonomy = full (30/8, per-slice commit, stop only on genuine forks / push).

## NEW DECISIONS (developer, 2026-07-01, after research re-ask) — three new work items
- [2026-07-01] AGREED **Q1**: add **method-references-as-values** (`obj.method` → a first-class typed
  closure) — the one genuine callable gap — + a guide example for the `Map<string, () => T>` typed-
  registry pattern. NO string-dispatch primitive (un-typeable/un-erasable).
- [2026-07-01] AGREED **Q2**: **stateless filesystem namespace** — expand `Core.File`
  (+ a `Core.Directory`): `append`/`delete`/`rename`/`copy`/`mkdir`/`listDir`/`size`/`isDir`/`metadata`.
  All `pure:false` → quarantined from the byte-identity oracle (the `Core.Process` recipe). NO stateful
  handles (that needs `Value::Resource` + `defer` + lifecycle → a separate mini-milestone if ever).
- [2026-07-01] AGREED **Q3**: build a **full Guzzle-style HTTP client incl. HTTPS** — amend the
  dependency policy to admit **`rustls`** under the crypto clause (wasm-gated, like argon2/ctrlc/
  corosensei). HTTP/1.1 over `std::net`, reuse the M6 `Request`/`Response` value model (client-side
  extensions), middleware as closures, pooling via the EXISTING green threads (no tokio), transpiles to
  PHP Guzzle/curl; the socket send is quarantined behind a `Transport` trait (like `src/serve.rs`).
  This is a real milestone (**M-HTTP-Client**) — design-spec first.
- Build order this session-family: **Q2 (filesystem, self-contained) → Q1 (method-refs) → Q3 design
  spec → Q3 build**, interleaved with the locked Lanes 2–4 as budget allows.

## Lane order + scope

### 1. Naming-overhaul codemod (FIRST — breaking; do before adding surface)
- SSOT: `docs/specs/2026-06-30-naming-overhaul-design.md` (locked). **Partially done** already
  (prior sessions: `fn→function`, `recv→receive`, `millis→milliseconds`, `Empty→empty`+`E-VOID-IN-UNION`,
  `Ok/Err→Success/Failure`, docs). **Remaining (the codemod):** native-fn renames (~20: `println→printLine`
  already?, `upper→uppercase`/`lower→lowercase`, Html `el→element`/…, `Decimal.div→divide`, Math
  `ipow→integerPower`/`intdiv→integerDivide`/`negInfinity`/`isNan→isNaN`, Path `basename→baseName`/…,
  `Process.args→arguments`, `Map.getOr→getOrDefault`, `Random.next→nextInt`+add `nextFloat`, Time
  `nowMillis→nowMilliseconds`, Url `urlEncode→encodeForm`/…); package renames (`Core.Text→Core.String`,
  `Core.Validate→Core.Validation`, `Core.Convert→Core.Conversion`, `Core.Reflect→Core.Reflection`,
  `Core.Crypto→Core.Cryptography`; NEW `Core.Environment` ← `Process.get`/`all`); CLI (`fmt→format`,
  `bench→benchmark`, `disasm→disassemble`, `lex→tokenize`).
- **Phase 0 MUST re-verify what's already shipped** (memory is ambiguous — some items done). Staged per
  the spec §"Implementation plan"; each stage green + byte-identical. Care: substring collisions,
  PHP-target names, update every `.phg`/inline-test caller + registry `name:` + transpiler namespace
  emission + `E-PKG-CASE` data. **The project memory flags this "fresh context."**

### 2. M-perf — perf-regression gate + VM wins (SECOND — gate-early)
- Establish a **CI perf-regression gate** (`phg bench` median-of-N, output-identity-gated) FIRST so it
  guards all later work. Then VM wins: `Rc`-share `Value::Str`, intern `IsInstance`, faster dispatch,
  const-fold, peephole, lazy `for`-range. Defers superinstructions / inline caches. Each win: a
  before/after `phg bench` number + byte-identity preserved.

### 3. VM debug symbols — close the S3/S5 deviation (THIRD)
- **Verified need:** the compiler recycles local slots across sibling blocks (`locals.pop()` /
  truncation), so a static slot→name table is ambiguous. Emit **per-local scope IP ranges** in
  `chunk::Function` (name, slot, start_ip, end_ip) from the compiler; at a VM fault/pause, filter to
  live locals → name→value. Then: byte-identical VM value-dump (`runvm --dump-on-fault` gains named
  locals) AND VM stepping becomes possible (a per-line hook in the VM loop, mirroring the interpreter's
  `exec_stmt` hook). Extends the M-DX debugger (`src/debug.rs`/`src/dap.rs`) to the VM backend.

### 4. Stdlib breadth (M11 on the M4 charter) (LAST — additive; uses new names + perf gate)
- Charter: `docs/specs/2026-06-29-m4-stdlib-charter.md`. Breadth: collections, `core.json` encode +
  safe parse, `core.regex` (PCRE `/u`), `sprintf`, hash/encoding/path/url/log, iterators. Each module
  ships a byte-identity-gated guide example (per the examples-ship-with-features rule).

## Wave breakdown (turnkey for a big autonomous session — each wave: green + byte-identical + commit)

### Lane 1 — Naming-overhaul (7 waves)
- **W1** Phase-0 delta: grep the codebase for every OLD name (memory is ambiguous re what shipped) →
  produce the authoritative remaining-renames list. No code change; just the verified delta.
- **W2** Native-fn renames, per module (registry `name:` + every `.phg`/inline-test caller):
  Output(`println→printLine`?), String(`upper→uppercase`/`lower→lowercase`), Html(`el→element`/
  `voidEl→voidElement`/`attr→attribute`/`boolAttr→booleanAttribute`), Decimal(`div→divide`),
  Math(`ipow→integerPower`/`intdiv→integerDivide`/`negInfinity`/`isNan→isNaN`), Path(`basename→baseName`/
  `dirname→directoryName`/`stem→fileStem`), Map(`getOr→getOrDefault`), Random(`next→nextInt` + add
  `nextFloat`), Time(`nowMillis→nowMilliseconds`), Url(`urlEncode→encodeForm`/…). One commit per module.
- **W3** Package renames: `Core.Text→Core.String`, `Core.Validate→Core.Validation`,
  `Core.Convert→Core.Conversion`, `Core.Reflect→Core.Reflection`, `Core.Crypto→Core.Cryptography`
  (module strings + import paths + transpiler namespace emission + `E-PKG-CASE` data + UFCS leaf tables).
- **W4** NEW `Core.Environment` ← `Process.get`/`all` move as `Environment.get`/`all`.
- **W5** CLI subcommands: `bench→benchmark`, `disasm→disassemble`, `lex→tokenize` (verify `fmt→format`
  shipped); update USAGE + `help_for` + every test/skill/doc invoking them.
- **W6** Migrate all `examples/**/*.phg` + fixtures + guide READMEs + CHANGELOG to the new names.
- **W7** Confirm keyword/type renames complete (`Empty→empty`+`E-VOID-IN-UNION`, `Ok/Err→Success/Failure`,
  `fn→function`, `recv→receive` — memory says done; verify none regressed).

### Lane 2 — M-perf (7 waves)
- **W1** Establish the **CI perf-regression gate** FIRST (a `phg bench` baseline JSON + a gate that
  fails on a >X% median regression, output-identity-gated). Guards all later waves.
- **W2** `Rc`-share `Value::Str` (clone = refcount bump). **W3** intern `IsInstance` (class name→id).
  **W4** faster opcode dispatch. **W5** compiler const-fold. **W6** peephole. **W7** lazy `for`-range
  (don't materialize `List<int>`). Each: before/after `phg bench` number + byte-identity preserved.

### Lane 3 — VM debug symbols (5 waves) — closes the S3/S5 deviation
- **W1** Compiler emits per-local scope IP ranges into `chunk::Function`
  (`Vec<LocalDebug{name, slot, start_ip, end_ip}>`) — solves the slot-recycling ambiguity.
- **W2** VM maps live slots→names at fault → `runvm --dump-on-fault` gains named locals, byte-identical
  to the interpreter dump (closes the S3 deviation). **W3** VM per-line pause hook (mirror the
  interpreter's `exec_stmt` hook) → VM stepping. **W4** wire the VM into the debug engine
  (`src/debug.rs`) so REPL + DAP work over `runvm`; tests. **W5** examples/docs (`examples/debug/`).

### Lane 4 — Stdlib breadth (M11, ~8 waves; each module ships a byte-identity-gated guide example)
- **W1** `core.json` encode + safe parse. **W2** `core.regex` (PCRE `/u`). **W3** `sprintf`/format.
  **W4** hash/encoding breadth. **W5** path/url breadth. **W6** log facility. **W7** iterators.
  **W8** collections breadth (audit gaps vs the M4 charter). Charter:
  `docs/specs/2026-06-29-m4-stdlib-charter.md`.

## Lock assessment (are we ready for a big autonomous run?)
- **LOCKED** for Lanes 1–3: scope + waves + verified constraints are concrete; a fresh session can
  execute top-to-bottom autonomously (project bypass sentinel armed).
- **Lane 4 needs one decision per module** (native-vs-`.phg`, optional-vs-fault, determinism tier) —
  the M4 charter answers most; the fresh session should read the charter at Lane-4 Phase 0 and only
  pause if a module's policy is genuinely ambiguous. Not a blocker to starting.

## DEVELOPER OPEN QUESTIONS (2026-07-01) — audited this session; feed the NEXT big auto session

### Q1 — Dynamic reflection (instantiate/call from a string). AUDITED — answer: NO, by design.
- **Instantiate a class from a runtime string / without importing?** NO. Classes compile to
  `Op::MakeInstance(index)` — a compile-time index into `class_descs`, no runtime string→class table;
  and resolution *requires* the type in scope (the loader mangle pass). No `new $className()`.
- **Call a function from a runtime string?** NO. Calls resolve at compile time (loader mangle → FQN →
  direct / `Op::CallNative(idx)`) or dispatch a closure VALUE via `Op::CallValue`. No `call_user_func`.
- **Callable formats we DO have:** exactly ONE value — `Value::Closure(Rc<ClosureData>)`, three internal
  variants: `Tree` (interpreter lambda; has `this_capture` — lambdas CAN reference `this` now),
  `Named(String)` (a bare named fn is a first-class value), `Byte{func,captures}` (VM bytecode closure).
  Invoked by direct call or pipe `|>`; higher-order natives (map/filter/reduce/sort) consume it via
  `ClosureInvoker`. **No method-reference-as-value** (`obj.method` un-called) — deferred (KNOWN_ISSUES).
- **Reflection is read-only, name-level:** `Core.Reflection` = kind/className/typeName/interfaces/
  parents/methods/fields — it TELLS you names, gives NO way to ACT on them (no invoke/instantiate).
- **CHALLENGE (locked):** dynamic-from-string is *un-typeable* (result is `mixed` → kills non-null +
  exhaustiveness + arg-checking), *un-erasable* (inherently runtime, breaks the erase-before-backend
  discipline), and re-introduces the global string registry the namespace design removed. Almost every
  PHP use has a typed equivalent already: pass a **closure** (we have first-class fns), dispatch through
  an **interface/union + match** (see `examples/web/router.phg` — handlers are first-class values, not
  name strings), model closed sets as **enums**. The idiomatic escape hatch for the real ~5% (construct-
  by-name from data) is a **developer-owned typed registry**: `Map<string, () -> T>` of constructor
  closures, or a `match name { … }` — string→behavior as *data you own*, not a hole in the checker.

### Q2 — Filesystem beyond read: create/delete/rename/overwrite/append, in a good OOP way.
- **Current `Core.File`:** only `read` (→`string?`), `exists`, `write` — function-style, minimal.
  Missing: delete/rename/copy/append/mkdir/metadata/list-dir, and any OOP handle.
- **Direction (design next session):** a richer `Core.File` (`append`/`delete`/`rename`/`copy`/`size`/
  `isDir`/`listDir`/`mkdir`) AND/OR an OOP `File`/`Path` value-object surface (`file_get_contents`/
  `file_put_contents` peers, but methods on a typed handle). All `pure: false` → **quarantined** from
  the byte-identity oracle (like `Core.Process`/`Core.Environment`), tested in `tests/`, walkthrough
  (not gated) examples. **CHALLENGE:** OOP file *handles* (an open fd you read/write/seek) fight the
  value-native, immutable-by-default model — a handle is mutable stateful identity. Decide: (a) stateless
  static-method OOP (`File.readText(path)` — a namespace, not a handle; keeps determinism-quarantine
  simple) vs (b) real stateful `File` objects (opens the door to resource lifecycle, `defer`-close,
  the mutation/GC story). Recommend (a) first; (b) only if a real streaming need appears.

### Q3 — Guzzle-style HTTP CLIENT (rich, well-structured) to consume/get APIs.
- **Current state:** NONE. All HTTP is server-side (`handle(Request)->Response`, `phg serve`). No client,
  no `TcpStream`, no network native anywhere — network was deliberately **deferred to M6/beyond** because
  (1) Rust std has no HTTP client → breaks zero-dep, and (2) network is non-deterministic → breaks the
  byte-identical spine (the real gate, per the M6 design).
- **CHALLENGE (the hard one):** a Guzzle-rich client (PSR-7/18, middleware, retries, pooling) is a
  genuinely large surface AND its results can NEVER be byte-identity-gated (a live API is
  non-deterministic + external). Options, in ascending cost: (a) reuse the **existing PSR-7-style
  `Request`/`Response` VALUE model** from M6 W1 as the client's data types (the portable unit already
  exists — only the transport is new), keep transport quarantined behind a `Transport` trait like
  `src/serve.rs`; (b) admit a vetted HTTP dependency (breaks zero-dep — needs a dependency-policy
  decision like the `argon2` precedent) OR hand-roll HTTP/1.1 over `std::net::TcpStream` (zero-dep, no
  TLS → HTTPS needs a TLS crate anyway). **The unavoidable tension:** the client's *value surface*
  (build a request, read a response) can be pure, typed, and even transpile to PHP Guzzle/`curl`; but
  the *send* is I/O — quarantined, tested against a local fixture server (like `tests/vendor.rs`'s
  `file://` git fixture), never in `differential.rs`. Frame it as **"rich typed value API + thin
  quarantined transport,"** mirroring the M6 server split. TLS/HTTPS is the real dependency fork.

## Session 2026-07-01 (this session) — progress
- Lane 1 (Naming) COMPLETE (`88082a8`/`4f539f0`/`b8679e7`). Lane 2 W1 (perf gate) DONE (`df00f4d`).
- Lane 2 W2 (`Rc`-share `Value::Str`) SCOPED, deliberately DEFERRED: 164 sites / 34 files — the headline
  win, but a full-differential-reverified mechanical sweep best done in its own worktree-isolated session
  rather than half-landed under budget. NEXT concrete perf step.

## Progress
- [x] Lane 1 — Naming-overhaul (**COMPLETE** 2026-07-01, `88082a8`/`4f539f0`/`b8679e7`) — W1 discovery
      found ~95% already shipped; delta was Path (baseName/directoryName/fileStem) + new
      Random.nextFloat (dyadic, byte-identical) + doc/comment drift cleanup (module headers,
      example READMEs, explain text, INVARIANTS/ARCHITECTURE). CHANGELOG history left intact.
- [~] Lane 2 — M-perf (IN PROGRESS) — **W1 DONE** (CI perf-regression gate: `scripts/perf-gate.sh`
      gates best-of-N `vm_speedup` from `phg benchmark --json` vs `bench/baseline.json`, machine-
      independent ratio + one-directional-noise → best-of-N + generous floor; wired as gating CI job).
      W2..W7 = VM wins (Rc-share Value::Str, intern IsInstance, dispatch, const-fold, peephole, lazy
      for-range) — each guarded by the new gate.
- [ ] Lane 3 — VM debug symbols (NOT STARTED) — W1..W5
- [ ] Lane 4 — Stdlib breadth (NOT STARTED) — W1..W8
