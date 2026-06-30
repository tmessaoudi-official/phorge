# Naming Overhaul — clarity / no-shortcut / no-ambiguity (locked 2026-06-30)

> **SSOT for a breaking codemod.** The developer requested a full review of every reserved name,
> Core package, and native function to remove abbreviations, shortcuts, ambiguity, and unexpressive
> names — "no exceptions". The complete name space was enumerated from source and analyzed against the
> policy below; every decision was confirmed via `ask-human`. This file is the authoritative change
> list the codemod implements.

## Policy (locked)
1. **No abbreviations / shortcuts** in user-facing names — spell out (`recv`→`receive`, `args`→`arguments`).
2. **EXCEPT universal mathematical notation** — `sqrt` `abs` `pow` `sin` `cos` `tan` `exp` `log` `log10`
   `gcd` `lcm` `pi` `e` stay (those ARE the clear names).
3. **EXCEPT type-referencing names** — `toInt` / `parseFloat` / `nextInt` / `asBool` mirror the kept
   primitive type names `int`/`float`/`bool`, so they are consistent, not shortcuts. Keep.
4. **EXCEPT universal acronyms** — `Json` `Html` `Url` `Csv` `Regex` `Http`, hash `md5`/`sha256`/`crc32`. Keep.
5. **Packages are nouns** (`Validation`, not `Validate`).
6. **Familiarity-first** where it doesn't conflict (kept `Channel`/`Task`/`spawn`/`join`/`Some`/`None`).

## Decisions (all confirmed)

### Types
- **`Empty` → `empty`** — lowercase keyword (like `void`/`never`), the *holdable unit type* (one value,
  bindable, composes in unions). Lowercase ⇒ collision-proof (user classes are PascalCase). Coexists with
  `void`. NEW RULE: **`void` may NOT appear in a union** (`int|void` ✗, uninhabited); **`empty` MAY**
  (`int|string|empty` ✓, inhabited). Verify/enforce in `checker::resolve` union construction
  (`E-UNION-MEMBER` / a new `E-VOID-IN-UNION`).
- **Result variants `Ok`/`Err` → `Success`/`Failure`** (no abbreviation, symmetric; `Error` is reserved
  as the exception type so it can't be reused).
- KEEP: `int float bool string bytes decimal void never List Map Set Optional Error Channel Task`;
  Optional variants `Some`/`None`.

### Keywords
- **Lambda `fn` → `function`** — lambdas use the full word (`function(x) => e`); the `fn` keyword is
  retired (named functions already use `function`).
- KEEP all other keywords (full words already).

### Concurrency
- **`recv` → `receive`**. KEEP `spawn` `send` `join` `Channel` `Task` `Channel.create`.
  (`Task` not `Thread` — cooperative green tasks, not OS threads; `Channel` not `Observable` — CSP queue,
  not reactive streams. See concurrency rationale in [[m6-w4-green-threads]].)

### CLI subcommands
- **`fmt`→`format`** · **`bench`→`benchmark`** · **`disasm`→`disassemble`** · **`lex`→`tokenize`**.
- KEEP `run runvm build transpile check test serve vendor lift explain parse`.

### Packages
- **`Core.Console` → `Core.Output`** (output-only today: `print`/`printLine`; future stdin = `Core.Input`).
- **`Core.Validate` → `Core.Validation`**
- **`Core.Convert` → `Core.Conversion`**
- **`Core.Reflect` → `Core.Reflection`**
- **`Core.Crypto` → `Core.Cryptography`**
- **`Core.Text` → `Core.String`**
- **NEW `Core.Environment`** ← `Process.get`/`all` move here as `Environment.get`/`Environment.all`
  (a dedicated flat module, NOT a `Process.environment.get` object-path — that form is rejected, D-L9).
- KEEP `Math File Bytes Html List Map Set Json Time Http Regex Path Process Random Encoding Hash Url Csv Decimal Test`.

### Native functions
| Module | Rename |
|---|---|
| Output | `println` → `printLine` |
| String | `upper` → `uppercase`, `lower` → `lowercase` |
| Html | `el`→`element`, `voidEl`→`voidElement`, `attr`→`attribute`, `boolAttr`→`booleanAttribute` |
| Decimal | `div` → `divide` |
| Math | `ipow`→`integerPower`, `intdiv`→`integerDivide`, `negInfinity`→`negativeInfinity`, `isNan`→`isNaN` |
| Path | `basename`→`baseName`, `dirname`→`directoryName`, `stem`→`fileStem` |
| Process | `args` → `arguments` (and `get`/`all` move to `Core.Environment`) |
| Map | `getOr` → `getOrDefault` |
| Random | `next` → `nextInt` (+ add `nextFloat`) |
| Time | `nowMillis` → `nowMilliseconds`; Duration/Instant `millis`-family → `milliseconds` |
| Url | `urlEncode`→`encodeForm`, `rawUrlEncode`→`encodeUriComponent`, `urlDecode`→`decodeForm`, `rawUrlDecode`→`decodeUriComponent` |

### Kept (challenged but correct)
Math notation (§policy 2); type-referencing names (§3); universal acronyms (§4); `Some`/`None`;
`of` factory methods; Html `raw`/`render`/`text` builders; hash `md5`/`sha1`/`sha256`/`crc32`.

## Implementation plan (staged, each green + byte-identical run≡runvm≡PHP)
The PHP transpile target of each native is **unchanged** — only the Phorj-surface name changes — so
transpiled output stays byte-identical. Stage by category, full gate per commit, ALWAYS verify the `phg`
binary (not just the differential — see the A1 loader-path lesson):
1. Native-fn renames (registry `name:` + every `.phg`/inline-test caller), per module.
2. Package renames (module strings + import paths + transpiler namespace emission + `E-PKG-CASE` data).
3. `Core.Console`→`Core.Output`; new `Core.Environment`.
4. CLI subcommand renames (arg dispatch + help + skills/docs).
5. Keyword `fn`→`function` for lambdas (lexer token retire + parser).
6. `Empty`→`empty` (lowercase keyword) + `void`-not-in-union rule + `Ok`/`Err`→`Success`/`Failure`.
7. Examples/README, KNOWN_ISSUES, CHANGELOG, conformance corpus, `phg explain` codes.

A `tools/` codemod script per category (like `core_rename*.py`) is the safe vehicle; verify each with the
full gate before committing. Distributable coordinates (manifest `module`, vendor dirs) stay lowercase.

### Status — ✅ COMPLETE (2026-06-30)
All 7 stages landed green + byte-identical (`run≡runvm≡real PHP 8.5`), each a self-contained commit:
1–4 (natives / packages / `Core.Output`+`Core.Environment` / CLI subcommands) — earlier commits.
5. `4eec4f3` — lambda `fn`→`function` (retired the `Fn` token; PHP arrow-fn `fn($x)=>` unaffected).
   `21bb2c2` — `Channel.recv`→`Channel.receive`.
   `e8bfcc8` — Time `millis`-family → `milliseconds` (PHP `__phorj_now_millis` helper unchanged).
6. `6ac717a` — type `Empty`→lowercase `empty` + new `E-VOID-IN-UNION` rule (void rejected, empty allowed
   in a union); `5c17351` — Result variants `Ok`/`Err`→`Success`/`Failure` (structural detection across
   all four backends). New explain code `E-VOID-IN-UNION`; tests `union_rejects_void_member` /
   `union_allows_empty_member`.
7. Living-docs sweep (examples/README, KNOWN_ISSUES, README, FEATURES, INVARIANTS) — collision-safe
   token codemod. CHANGELOG historical entries left as-is (they record what shipped at the time).

## Decisions Log
- [2026-06-30] AGREED: full naming overhaul per the table above; `Task`/`Channel` kept (rejected
  Thread/Observable); lambdas use `function`; `Empty`→lowercase `empty` (void-not-in-union, empty-in-union);
  Result `Success`/`Failure`; `Core.Output` (not Console/Out); `Core.Environment` split; spell out all
  non-math abbreviations; packages as nouns. Breaking codemod approved, staged + byte-identity-gated.
