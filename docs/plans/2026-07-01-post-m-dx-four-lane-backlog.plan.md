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

## Progress
- [ ] Lane 1 — Naming-overhaul (NOT STARTED)
- [ ] Lane 2 — M-perf (NOT STARTED)
- [ ] Lane 3 — VM debug symbols (NOT STARTED)
- [ ] Lane 4 — Stdlib breadth (NOT STARTED)
