# Track E — PHP interop & migration (roadmap-completeness audit)

## Track summary

Phorj already owns one direction of the interop bridge superbly — **Phorj → PHP transpile** is
first-class, byte-identity-gated against real PHP, and deploys onto any PHP host (so transpiled output
can already sit alongside Composer/Symfony/Laravel). That is the *deploy* half of "the relationship
TypeScript has to JavaScript." The **adoption** half — the half that actually won TypeScript — is
almost entirely unbuilt and only loosely roadmapped as a single line item: "M8 — PHP → Phorj migration
tool (the inverse of the transpiler)." TypeScript did not win because it transpiled *to* JS; it won
because of `allowJs` (mix `.ts` and `.js` in one project from day one), `.d.ts` declaration files +
DefinitelyTyped (type the entire untyped npm ecosystem without touching it), `// @ts-check` + JSDoc
(opt into checking one file at a time), and a codemod/`ts-migrate`-style automated importer. Phorj's
*equivalent* of each of these is a distinct gap, and the current single "M8 importer" line collapses a
half-dozen separable adoption mechanisms into one deferred milestone.

The good news: a lot of the foundation is *already designed*. The M8 importer has a locked staged
A→B→C scope (`docs/plans/2026-06-18-m8-php-import-design.md`) and an exhaustive PHP-feature
importability inventory (`docs/research/m8/raw/`). The parity spec already resolved try/catch as a
"thin PHP-interop bridge" and attributes as full runtime reflection (both interop-relevant). So this
track is less about *discovering* the importer and more about (a) **decomposing the monolithic "M8"**
into the genuine adoption-killer sub-capabilities, and (b) surfacing the interop mechanisms that aren't
captured anywhere yet — the **declaration-file equivalent** (Phorj's `.d.ts`/`.stub`), **mixed
`.php`+`.phg` projects**, **calling a real Composer package from Phorj source**, and a **raw-PHP escape
hatch**. The philosophy lens is decisive here and prunes hard: Phorj's spine is `run ≡ runvm ≡ real
PHP` byte-identity over a *closed, statically-typed, no-`eval`* program. Any interop mechanism that
requires executing arbitrary dynamic PHP *inside* the Phorj VM (live FFI, embedding the PHP engine,
runtime `require` of a Composer lib) is fundamentally incompatible with that spine and is a clean
**reject** — the ecosystem spec already rejected these for the right reason. What survives is the
TypeScript-shaped set: erasure-style declaration files, an offline codemod, transpile-time interop, and
a transpile-only escape hatch. Those are the high-ROI adoption levers and most are **adopt** (as
roadmap decomposition) or **defer** (real but post-GA).

## Gaps

| id | title | kind | fit | rec | milestone | effort |
|---|---|---|---|---|---|---|
| E-importer-stageA | PHP→Phorj importer Stage A (round-trip of our own emitted PHP) | port | strong | adopt | M8 | L |
| E-importer-stageB | PHP→Phorj importer Stage B (idiomatic typed PHP 8) | port | strong | adopt | M8 | L |
| E-decl-files | Declaration-file equivalent (`.d.phg` / stub) for untyped PHP deps | new | strong | adopt | new milestone M8.5 (interop) | L |
| E-mixed-project | Mixed `.php` + `.phg` in one project (`allowJs` analogue) | new | ok | defer | M8.5 | L |
| E-call-composer | Call a Composer/PHP library from Phorj source (transpile-time) | new | strong | adopt | M8.5 | M |
| E-raw-php-escape | Raw-PHP escape hatch (`php"…"` / `extern php fn`), transpile-only | new | ok | defer | M8.5 | M |
| E-gradual-checkjs | Per-file gradual checking / `@phorj-check` opt-in on imported PHP | map | weak | reject | — | — |
| E-importer-stageC | Importer of general/dynamic PHP (eval, var-vars, `__call`) | omit | weak | reject | — | — |
| E-php-ffi | Live PHP-engine FFI / embed PHP into the VM | omit | weak | reject | — | — |
| E-named-args | Named arguments (importer prerequisite + own-right feature) | port | strong | adopt | M-RT / M8-prereq | M |
| E-variadics | Variadic / rest parameters `...$args` (importer prerequisite) | port | strong | adopt | M-RT / M8-prereq | M |
| E-union-to-enum | PHP `T\|U` → tagged-enum mapping on import (better idiom) | map | strong | defer | M8 | M |
| E-strict-types-gate | `declare(strict_types=1)` as the import-eligibility gate | map | strong | adopt | M8 | S |
| E-trycatch-bridge | try/catch/throw as the PHP-interop error bridge | port | ok | defer | M3 (error-model S2) | L |
| E-migration-report | Importer migration report (per-construct BETTER/SAME/REJECT verdict + un-importable list) | new | strong | adopt | M8 | M |
| E-incremental-codemod | Directory-at-a-time codemod CLI (`phg import ./legacy`) with idempotent re-run | new | strong | adopt | M8 | M |
| E-composer-manifest-bridge | `phorj.toml` ↔ `composer.json` interop (consume/emit) | map | ok | defer | M8.5 | M |
| E-php-version-target | `--php-target=8.1\|8.2\|8.3\|8.4` transpile floor (deploy-interop) | port | strong | adopt | M9/M12 | S |
| E-phpunit-bridge | Emit/consume PHPUnit for transpiled output (test interop) | new | ok | defer | M7-tooling | M |
| E-namespace-fqn-interop | Map PHP `namespace`/`use` ↔ Phorj package on import | map | strong | adopt | M8 | M |

## Rationale for ADOPT items

**E-importer-stageA / E-importer-stageB (M8, L each).** These are the heart of the migration story and
are already design-locked (`docs/plans/2026-06-18-m8-php-import-design.md`). They are listed as
*separate* gaps here deliberately: Stage A (import exactly the PHP our own transpiler emits, giving a
`phg → php → phg' ≡ phg` behavioral round-trip) is the bootstrap that proves the PHP parser is correct,
while Stage B (idiomatic typed PHP 8 — typed signatures, classes, enums, `match`, readonly) is where
the *product value* lives. The philosophy fit is maximal: this is literally the JS→TS port story, the
"closing the loop with its PHP heritage" line in VISION.md. The standing quality bar — every mapped
construct is BETTER / SAME+syntax / SAME / WORSE(reject) — keeps it from importing PHP's unsoundness.
Adopt; build after M6 per the existing lock.

**E-decl-files (new milestone M8.5, L).** The single biggest *uncaptured* gap and arguably the true
adoption-killer. TypeScript's `.d.ts` + DefinitelyTyped let a TS program use the entire untyped npm
ecosystem without porting it; PHP's de-facto equivalent already exists as **PHPStan/Psalm `.stub`
files** (PHPDoc signatures over `vendor/` — [Verified: PHPStan Stub Files docs] ) and
**phpstorm-stubs** for builtins. Phorj needs the same thing: a way to *declare* the typed surface of a
Composer package (or a builtin) — a `.d.phg` of erased signatures, no bodies — so Phorj code can call
into it (through the transpile backend) while the checker type-checks the call. This is pure
erasure-discipline (signatures only, erased before any backend), it leans on existing
mechanisms Phorj already runs (`expand-before-backends`, `import type`, the `(module,name)` native
registry), and it is what makes "use Phorj as a typed layer over an existing PHP codebase" *real*
rather than aspirational. It is genuinely beyond plain PHP (PHP has no native declaration-file format —
it's a tooling convention), so `kind=new`. Adopt as the headline of a dedicated interop milestone
(M8.5) sitting beside the importer; it is independent of and complementary to Stage B.

**E-call-composer (M8.5, M).** The concrete payoff of declaration files: `import php Vendor\Package;`
resolves a `.d.phg`/stub, the checker types the calls, and the **transpile backend** emits `\Vendor\…`
calls into idiomatic PHP that runs against the real Composer autoloader. Crucially this is
**transpile-time only** — it never tries to run Composer code inside the Phorj VM (that would shatter
the byte-identity spine and is the rejected `E-php-ffi`). On the native VM these calls are simply
unavailable (a clean compile error stating the symbol is PHP-backend-only), exactly as a TS program
using a Node-only API can't run in a browser. Strong fit, modest effort once E-decl-files exists; it is
the single most compelling "you can adopt Phorj incrementally on your existing Laravel app today" demo.

**E-named-args + E-variadics (M-RT / M8-prereq, M each).** Already flagged as M8 prerequisites in the
import design plan, and both are own-right parity features (named args is PHP 8.0; variadics is
ancient). They are import-blocking: idiomatic PHP 8 uses both pervasively, so an importer that can't
represent them can't be ≥ PHP. The parity spec also flags array unpacking `...` as needing a variadic
design first, so these unblock multiple downstream items. Both map to clean idiomatic PHP. Adopt; pull
forward into the M-RT/parity track so they're ready before the importer build starts.

**E-strict-types-gate (M8, S).** The cheap, decisive eligibility rule: only `declare(strict_types=1)`
PHP files are import-eligible (the inventory already marks this IMPORTANT — only strict-typed PHP is
cleanly importable). Weak-typed/coercing PHP is rejected cleanly at the front door rather than
mis-imported into something that violates Phorj's strict-types promise. Small, high-leverage, sets
honest expectations. Adopt as part of the importer's front-end.

**E-migration-report (M8, M).** Migration is never 100% automatic — the adoption-killer is *trust*, and
trust comes from a clear report: per-construct BETTER/SAME+syntax/SAME verdicts plus an explicit
un-importable list with reasons (this exact verdict vocabulary is already a locked quality bar in the
M8 plan). `ts-migrate` and Psalm/Rector all succeed partly on their reporting. Strong fit; it's the
human-review surface that makes a one-way port palatable. Adopt.

**E-incremental-codemod (M8, M).** The importer needs a real CLI ergonomics layer: `phg import
./legacy` over a directory, idempotent on re-run (don't clobber hand-edited output), file-by-file so a
team migrates incrementally rather than big-bang. This is how every successful migration tool
(`ts-migrate`, Rector, `2to3`) actually gets used. Strong fit; modest effort layered on the Stage
A/B engine. Adopt.

**E-namespace-fqn-interop (M8, M).** Concrete, load-bearing mapping work: PHP `namespace Acme\Util;` +
`use` statements must map onto Phorj's Go-shaped package/`import` model (and back, since the transpiler
already emits `namespace Acme\Util { … }` brace blocks — the inverse exists). Without this the importer
can't place imported symbols into the right packages or resolve cross-file references. Strong fit (it's
a direct structural correspondence), and it reuses the existing loader name-mangling/de-mangling pass.
Adopt as core importer plumbing.

**E-php-version-target (M9/M12, S).** A small but real *deploy-interop* gap: the transpiler currently
targets a single recent PHP. Real adopters deploy on a fixed PHP (8.1 LTS-ish, 8.2, 8.3, 8.4), and some
emitted features (8.1 intersections, 8.4 property hooks, 8.0 unions) are version-gated. A
`--php-target=8.x` floor that refuses to emit a feature the target can't run (or lowers it) makes the
"deploy on your existing PHP infra" promise honest across versions. Small, high-trust. Adopt around the
release-hardening milestones (M9/M12) where the emit surface is being audited anyway.

## Notes on the DEFER / REJECT calls

- **E-mixed-project (defer):** mixing `.php` and `.phg` in one *built* project is the literal `allowJs`
  analogue and is powerful, but it only makes sense once both the importer and declaration files exist,
  and it complicates the loader (two front-ends, one merged program). Real and on-philosophy, but
  sequenced after the M8/M8.5 core. Fit is "ok" not "strong" because the cleaner Phorj story is
  *import once* (codemod) rather than *coexist forever*.
- **E-raw-php-escape (defer):** a `php"…"`/`extern php fn` block that passes a literal PHP fragment
  straight through the transpiler (and is *unavailable* on the VM/interpreter) is a legitimate, bounded
  escape hatch — the TS `// @ts-ignore` / `declare` equivalent. It must be transpile-only and clearly
  marked unsafe, so it doesn't touch the byte-identity spine. Real but explicitly a power-user corner;
  defer until demand is proven.
- **E-union-to-enum (defer):** the import design already locked "raw `T|U` stays rejected on import; map
  to a tagged enum (the better idiom)." Now that Phorj *has* shipped real union types `A|B` (M-RT S4),
  this mapping choice should be revisited at build time — it may be that importing PHP `A|B` to Phorj
  `A|B` is now the right move for class/interface unions, with the enum mapping reserved for
  scalar-discriminated unions. Defer the final decision to the importer build.
- **E-trycatch-bridge (defer):** the parity spec resolved recoverable errors **Result-first**, with
  `try`/`catch`/`throw` as the *thin PHP-interop bridge*. It's interop-relevant (imported PHP throws),
  so it belongs in this track's awareness, but it's owned by the M3 error-model slice 2, not M8. Defer
  to that slice.
- **E-composer-manifest-bridge (defer) / E-phpunit-bridge (defer):** both are real ecosystem-interop
  conveniences (consume `composer.json` deps; emit PHPUnit so transpiled tests run on the existing PHP
  test infra — the ecosystem spec's bootstrap-lever idea). Useful, on-philosophy, but second-order; let
  them follow the core importer/decl-file work.
- **E-importer-stageC (reject):** general/dynamic PHP (`eval`, variable-variables, `__call`/`__get`
  magic, runtime `new $class`) is fundamentally un-importable into a static, closed, no-`eval` language
  — the M8 plan already marks Stage C rejected-by-design and the inventory flags these UN-IMPORTABLE.
  Clean reject.
- **E-php-ffi (reject):** live PHP-engine FFI / embedding PHP into the VM was already rejected by the
  ecosystem spec — it drags the whole dynamic PHP runtime in and shatters the clean static break and the
  byte-identity spine. The sanctioned path is the transpile backend (Composer is free there) or native
  connectors. Clean reject; this is the one place the philosophy hard-stops a tempting feature.
- **E-gradual-checkjs (reject):** per-file *gradual/optional* typing (a `mixed`/`any` hole so untyped
  PHP can be checked loosely in-place) is the one TS mechanism Phorj should NOT copy — the parity spec
  already rejected gradual typing ("punches a hole in the static + byte-identity story; PHP already IS
  the gradual target"). The Phorj answer to "I have untyped PHP" is *declaration files* (E-decl-files)
  + *import* (E-importer), not loosening the type system. Reject.

Sources: [PHPStan Stub Files](https://phpstan.org/user-guide/stub-files), [php-stubs/wordpress-stubs](https://github.com/php-stubs/wordpress-stubs), [phpstan/php-8-stubs](https://github.com/phpstan/php-8-stubs)

## Critic pass

I read the shipped state (FEATURES.md, KNOWN_ISSUES.md, docs/MILESTONES.md, ROADMAP.md, VISION.md, the
project CLAUDE.md milestone log) and the M8 design + raw inventory before judging.

### Mis-listings (already shipped) — none

Every original item is genuinely unbuilt. The **Phorj→PHP transpiler** ships and is byte-identity-gated,
but that is the *deploy* direction; the entire *import* direction (Stage A/B, decl-files, codemod,
migration report) and all interop tooling (version-target, Composer bridge, PHPUnit bridge) are
un-built. `E-named-args`/`E-variadics` are confirmed absent from FEATURES.md and KNOWN_ISSUES.md.
`E-php-version-target` is not shipped — the transpiler targets a single recent PHP (8.4 hooks / 8.1
intersections / 8.0 unions are emitted unconditionally, no `--php-target` floor). **removed_mislisted = 0.**

### Newly-found items (in this track's domain, missed by the first pass)

The original list decomposes the *importer mechanics* well but under-covers two long-tail surfaces: (a)
specific **per-construct import mappings** the M8 raw inventory explicitly flags as importable
(attributes-as-erased-annotations, heredoc/nowdoc, first-class-callable, PHPDoc harvest), and (b) the
**mirror of `E-php-version-target`** — making the *deploy* (transpile) direction honest is as much an
interop concern as making the *import* direction honest, and the transpile-hazard linter is currently
absent (the hazards live only as prose caveats in KNOWN_ISSUES).

| id | title | kind | fit | rec | milestone | effort |
|---|---|---|---|---|---|---|
| E-phpdoc-harvest | Harvest PHPDoc `@param`/`@return`/`@var`/`@template` type hints on import | port | strong | adopt | M8 (Stage B) | M |
| E-attributes-import | Import `#[Attr(...)]` attributes as erased annotations (drop or map to lint) | map | ok | defer | M8 (Stage B) | S |
| E-firstclass-callable-import | Map PHP first-class callable `f(...)` / `$o->m(...)` → Phorj fn value | map | strong | adopt | M8 (Stage B) | S |
| E-heredoc-nowdoc-import | Map heredoc `<<<EOT` → interpolated string, nowdoc `<<<'EOT'` → raw string | map | strong | adopt | M8 (Stage A/B) | S |
| E-psr4-autoload-bridge | Consume `composer.json` `autoload.psr-4` to drive package placement on import | map | ok | defer | M8.5 | M |
| E-transpile-hazard-lint | Transpile-interop hazard linter (`W-PHP-BUILTIN-NAME`, external-`private`-field, non-finite-float) | new | strong | adopt | M9/M12 | S |
| E-stub-distribution | Stubs distribution for Composer ecosystem (DefinitelyTyped-analogue repo + `phg stub get`) | new | ok | defer | post-M8.5 | L |

**Rationale (new ADOPT items):**

- **E-phpdoc-harvest (M8 Stage B, M).** The first pass adopts `declare(strict_types=1)` as the import
  *gate* (`E-strict-types-gate`) but stops there. A huge fraction of real PHP is *weakly typed in the
  signature but richly typed in PHPDoc* (`@param int[] $xs`, `@return Foo|null`, `@template T`) —
  PHPStan/Psalm/PhpStorm get nearly all their type information this way. An importer that reads only
  native type hints leaves most legacy PHP un-importable; harvesting PHPDoc is how the importer reaches
  *"≥ PHP"* on the largest body of real code, and it is the natural source for the `T|null` → `T?`,
  `Foo|Bar` → union, and `@template` → generic mappings the importer needs anyway. Strong fit, directly
  on the JS→TS-via-JSDoc precedent. — [Inferred: M8 raw inventory marks generics as docblock-`@template`
  only and weak-typing as the dominant gap; PhpStorm/PHPStan PHPDoc reliance is well established.]

- **E-firstclass-callable-import (M8 Stage B, S).** The inventory explicitly marks first-class callable
  `f(...)` / `$obj->m(...)` / `C::m(...)` as **importable** ("they carry a type") and core. Phorj
  *already ships* first-class function values + `(T)->T` types (M3 S3), so this is a near-free 1:1
  mapping — the receiving feature exists, only the import arm is missing. Strong fit, small effort.

- **E-heredoc-nowdoc-import (M8 Stage A/B, S).** Inventory verdict is SAME / SAME+syntax: heredoc → a
  Phorj interpolated multi-line string (Phorj strings are already multi-line, lexer.rs:180), nowdoc →
  a raw string. A common-enough literal form that an importer must handle to round-trip real files;
  trivial mapping onto existing string machinery. Adopt as a small Stage-A/B mapping rule.

- **E-transpile-hazard-lint (M9/M12, S).** The exact mirror of `E-php-version-target` and the strongest
  newly-found item by leverage. The *deploy* direction already has documented foot-guns that only exist
  on the PHP leg: a `package Main` function named `serialize`/`strlen`/`header` collides with a PHP
  builtin; an externally-read `private` promoted field throws under PHP; a non-finite float diverges
  (all three are KNOWN_ISSUES prose, caught today only by the example author or the PHP oracle). A
  `phg transpile --lint` / checker warning channel (`W-PHP-BUILTIN-NAME`, `W-PHP-PRIVATE-EXTERNAL`)
  turns "deploy on your PHP infra honestly" from a prose convention into an enforced gate — the deploy
  half of the interop story, currently unowned. Strong fit (it makes an existing promise honest, removes
  a surprise), small effort (the warning channel exists since S2). Adopt at the release-hardening
  milestone where the emit surface is audited.

**Rationale (new DEFER items):**

- **E-attributes-import (defer):** the inventory marks most attributes erasable/diagnostics-only and a
  few (`#[\Override]`, `#[\Deprecated]`, `#[\NoDiscard]`) as mappable to a Phorj lint. Importing them
  means *dropping* them (safe — they're metadata) or mapping the handful with a Phorj-lint analogue.
  Real but low-value and dependent on whether Phorj grows a native attribute/annotation surface; defer
  behind the core Stage B mappings.
- **E-psr4-autoload-bridge (defer):** `E-namespace-fqn-interop` covers the `namespace`/`use` → package
  mapping; the *additional* signal is `composer.json`'s `autoload.psr-4` map (dir → namespace prefix),
  which tells the importer where to place files under Phorj's folder=path model. A useful refinement of
  the namespace mapping but second-order; pair it with `E-composer-manifest-bridge` at M8.5.
- **E-stub-distribution (defer, L):** `E-decl-files` builds the *format*; this is the *ecosystem* — a
  shared repository of community stubs for popular Composer packages (the DefinitelyTyped analogue) plus
  a `phg stub get vendor/pkg` fetcher. This is what actually made `.d.ts` win, but it is a
  social/infrastructure effort that only pays off post-1.0 once the decl-file format and a user base
  exist; defer well past M8.5.

### Sanity-check of original recommendations against the philosophy — all hold

The three rejects are correct and philosophy-decisive: `E-php-ffi` (drags the dynamic runtime in,
shatters the `run≡runvm≡php` spine), `E-importer-stageC` (eval/var-vars/magic are un-importable into a
closed no-`eval` language), and `E-gradual-checkjs` (gradual typing punches a hole in the static spine;
PHP *is* the gradual target — the Phorj answer is decl-files + import, not `mixed` holes). One nuance
worth flagging at build time (already noted by the first pass under `E-union-to-enum`): now that real
unions `A|B` ship, the import mapping should send PHP class/interface unions to Phorj `A|B` 1:1 and
reserve the enum mapping for scalar-discriminated unions — the original DEFER is right to leave the final
call to the importer build.

**new_found = 7 · removed_mislisted = 0.**
