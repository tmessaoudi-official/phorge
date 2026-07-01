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

## OPEN QUESTION (developer, 2026-07-01) — answer at session end, with challenge
Dynamic reflection surface — the developer asks:
1. Can we **instantiate a class from a string at runtime** dynamically, WITHOUT importing it? (PHP `new $className()`)
2. Can we **call a function dynamically at runtime from a string**? (PHP `$fn()` / `call_user_func`)
3. What **callable formats** do we currently have? (first-class fn values, closures, method refs?)
> To answer: audit `Core.Reflection` (`src/native/reflect.rs`), `Value::Closure`, first-class fn values,
> the resolution model (imports resolved at compile-time via the loader mangle pass). CHALLENGE angle:
> dynamic-from-string fights the whole design — compile-time namespacing, "nothing in the wind",
> byte-identity spine, and the type system's non-null/exhaustiveness guarantees. A string→class/fn
> lookup is un-typeable and un-erasable. Frame the tension explicitly before recommending.

## Progress
- [ ] Lane 1 — Naming-overhaul (NOT STARTED) — W1..W7
- [ ] Lane 2 — M-perf (NOT STARTED) — W1..W7
- [ ] Lane 3 — VM debug symbols (NOT STARTED) — W1..W5
- [ ] Lane 4 — Stdlib breadth (NOT STARTED) — W1..W8
