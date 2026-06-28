# Extension Policy & Required-Extensions Manifest — Design Spec

> **Status:** 🔲 Designed — not yet implemented (the *core=tier-1* principle is already enforced in
> practice; the tier-3 mechanism below is proposed). The triggering fix (`0bb620b`, `core.bytes.to_string`
> mbstring → PCRE) and the policy statement are landed; this spec captures the rule and the forward
> mechanism so future stdlib growth (`core.image`, `core.http`, …) has a defined path.
> **Milestone:** M9 hardening follow-up (post-CI-green); a prerequisite shape for any extension-bound
> stdlib module.
> **Trigger:** CI run #3 — `examples/guide/bytes.phg` fataled under the PHP oracle because the
> transpiled `mb_check_encoding` (mbstring) is undefined when the oracle runs `php -n`. See memory
> `transpile-no-ini-extensions`.
> **Code state at spec time:** master `414b54a`, tree clean, CI fully green (gate + cross-build).

---

## 1. Problem

Phorj's transpile contract is **Phorj : PHP :: TypeScript : JavaScript** — every feature maps to
**idiomatic PHP that runs anywhere**. "Anywhere" is the load-bearing word: TypeScript output runs in
any conformant JS engine; it doesn't silently assume a Node-only or browser-only API. Phorj's
emitted PHP made exactly that mistake.

`core.bytes.to_string` emitted `mb_check_encoding($s, 'UTF-8')`. `mb_check_encoding` lives in the
**mbstring** extension, which is *usually present but not guaranteed*:

- The correctness oracle (`tests/differential.rs`) runs transpiled PHP with **`php -n`** (no
  `php.ini`) — so any extension loaded via `conf.d/*.ini` (the normal Linux packaging of mbstring)
  is **absent**. Only statically-compiled-in extensions survive.
- Minimal real-world PHP (Alpine `php:8.4-cli-alpine`, hardened containers) ships without mbstring.

The result: the example passed **locally** (dev PHP 8.6 has mbstring compiled *static* → survives
`-n`) and **fataled on CI** (setup-php loads mbstring as a shared module → stripped by `-n` →
`Call to undefined function`). A statically-linked local extension **masks** the portability gap
entirely; only `php -n` on a shared-extension build exposes it.

The deeper issue: **there was no policy** for which PHP functions the transpiler may emit, and **no
mechanism** for a module that legitimately needs an extension to declare and guard it.

## 2. Goals / Non-Goals

**Goals**
- A written, enforceable rule for which PHP functions transpiled output may use.
- A defined path for a future stdlib module that *genuinely* needs an extension (image, HTTP,
  intl) to declare it and fail clearly — never with an undefined-function fatal.
- An automated guard so a tier-violating emit can't ship silently (mirrors how the oracle now
  guards value-divergence).

**Non-Goals**
- **Not** building a Cargo-feature matrix for Phorj's *own* (Rust) build — its std-only stdlib has
  no per-module cost; that's YAGNI until a module pulls a heavy/platform dep (revisit at M6 sockets
  or a wasm target).
- **Not** vendoring or bundling PHP extensions — Phorj emits PHP source, it does not ship a runtime.
- **Not** changing the `run ≡ runvm` spine — this is transpile-output-only.

## 3. The extension tiers

| Tier | Examples | Availability | Phorj stance |
|------|----------|--------------|---------------|
| **1 — always-compiled** | `Core`/`standard` (`strlen`, `substr`, `str_*`, `intdiv`, `fmod`, `range`, `strpos`, `explode`), **PCRE** (`preg_*`), `json_*` (always-on since 8.0) | Present on **every** PHP, survives `php -n` | **Allowed in core stdlib.** |
| **2 — default-but-removable** | **mbstring**, `ctype`, `tokenizer`, `fileinfo` | Usually present; **absent under `php -n` and on minimal builds** | **Forbidden in core stdlib** — pick a tier-1 equivalent. |
| **3 — genuinely optional** | `gd`, `curl`, `intl`, `pdo_*` | Installed deliberately | **Allowed only in an extension-bound module that declares + guards it** (§5). |

Tier-2 is the trap: "it works on my machine" is precisely tier-2's failure mode. The rule collapses
it away — core code targets tier-1; anything beyond tier-1 is tier-3 and must be explicit.

## 4. Policy (in force now)

**Core stdlib transpiled output uses tier-1 functions only.** Concretely:

- UTF-8 validity → `preg_match('//u', $s) === 1` (PCRE), **not** `mb_check_encoding` (✅ done, `0bb620b`).
- String length / slice → `strlen` / `substr` (byte semantics, already tier-1).
- Any future core function picks a tier-1 PHP target or it does not ship in core.

**Enforcement (proposed, cheap):** a test that transpiles every `examples/**/*.phg` and asserts the
output contains **no** tier-2/3 function from a denylist (`mb_*`, `ctype_*`, `gd_*`, `curl_*`, `intl`/
`Collator`, …). This is the static analogue of the value oracle: today's ad-hoc `grep mb_` made a
regression test. One scan, no PHP needed, runs in the `gate` job.

## 5. Tier-3 mechanism (proposed — for `core.image`/`core.http`/… later)

When a module *must* use a tier-3 extension, three coordinated pieces make it honest:

1. **Declare** in `phorj.toml` `[require]` using Composer's own vocabulary — `ext-gd = "*"`. We
   already adopted Composer vocabulary for deps; `ext-*` is its native idiom. The manifest parser
   (`src/manifest.rs`) gains an `ext` requirement kind (no network, no resolution — purely declared).
2. **Preflight guard** in emitted PHP — the transpiler prepends, once per required extension:
   ```php
   if (!extension_loaded('gd')) { fwrite(STDERR, "phorj: this program requires the PHP 'gd' extension\n"); exit(1); }
   ```
   A clean, diagnosable exit — never an undefined-function fatal mid-run.
3. **Transpile-time manifest + gate** — the transpiler already knows every imported `core.*` module,
   so it can: (a) emit a `// requires: ext-gd` header, and (b) honor a `--php-target=baseline|full`
   flag that **rejects at transpile time** any tier-3 use under `baseline` (CI/default), surfacing
   the dependency before runtime. `baseline` is the default; `full` opts into tier-3.

This keeps the common case (tier-1) invisible and makes the rare case (tier-3) loudly explicit at
declare-, compile-, and run-time.

## 6. What's in force vs proposed

| Piece | State |
|-------|-------|
| Core = tier-1 only (the rule) | ✅ in force; `bytes.to_string` migrated (`0bb620b`) |
| `transpile-no-ini-extensions` memory + this spec | ✅ landed |
| Denylist transpile-scan regression test | 🔲 proposed (small, no PHP) |
| `phorj.toml` `ext-*` requirement kind | 🔲 proposed (lands with first tier-3 module) |
| Preflight `extension_loaded` guard emit | 🔲 proposed (same) |
| `--php-target=baseline\|full` gate + `// requires:` header | 🔲 proposed (same) |

## 7. Implementation sketch (when a tier-3 module first lands)

- `src/manifest.rs` — parse `ext-<name>` entries in `[require]` into a `Vec<String>` of required
  extensions on `Manifest` (no `Dependency`/`Pin` — declared-only).
- `src/native.rs` — each `NativeFn` optionally names a `requires_ext: Option<&'static str>`; the
  registry can then enumerate required extensions for the imported modules.
- `src/transpile.rs` — (a) collect `requires_ext` over imported natives; (b) emit the preflight guard
  block + `// requires:` header; (c) under `--php-target=baseline`, error on any tier-3 native.
- `src/cli.rs` — `--php-target` flag (default `baseline`), threaded into `cmd_transpile`.
- Tests: the denylist scan (§4) now; per-module guard-emit + baseline-rejection tests with the first
  tier-3 module.

## 8. Open questions / deferrals

- **`core.text` Unicode semantics.** `text.len` is currently *byte* length (`strlen`, tier-1). True
  *codepoint* length wants mbstring (`mb_strlen`) — a tier-1 PCRE workaround
  (`preg_match_all('//u', …)`) exists but is awkward. Decision deferred: either keep byte semantics
  (document it) or make a future `core.text.chars`/`graphemes` an explicit tier-3 (`ext-mbstring`)
  module. Not blocking; no current example needs codepoint length.
- **`--php-target` default.** Proposed `baseline`. If a user base reliably has mbstring, a project
  could set `full` in `phorj.toml`; out of scope until tier-3 exists.
- **JSON.** `json_*` is tier-1 (always-on since 8.0); `core.json` (deferred for needing a dynamic
  `Any` type) is unaffected by this policy.
