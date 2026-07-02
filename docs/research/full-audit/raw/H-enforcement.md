# Agent H — Enforcement Audit (adversarial static-guarantee verification)

> Binary: `target/release/phg` 0.5.1-alpha.1 (built 2026-07-01 22:01, one commit behind HEAD;
> the only later code commit a23ca00 adds Core.File natives, no diagnostic codes — valid for
> enforcement probes). ~300 probes total: 197-code trigger sweep + 45 targeted checker probes +
> 12 runtime-parity probes (run vs runvm) + 12 missing-rule probes + 20 project-mode probes +
> ~15 corrective re-probes. Probe files:
> `/tmp/claude-1000/-stack-projects-phorj/45a2fdbc-30f8-494e-83a1-ece45517336b/scratchpad/enforcement/`
> (`pass1-results.txt`, `pass2-results.txt`, `pass2rt-results.txt`, `pass3-results.txt`,
> `projects-results.txt`, `fix/`).

## Summary matrix

| Family | Verdict | Detail |
|---|---|---|
| E/W-code trigger sweep (197 codes) | **193 alive, 2 CLI-dead, 1 uncoded, 1 documented-future** | dead: E-RESERVED-PACKAGE, E-PKG-CASE(package-decl arm); uncoded: E-ALIAS-CYCLE; future: E-OVERLOAD-SELECT-CONFLICT |
| Visibility — instance fields/methods | **ENFORCED** | private/protected, from subclass, sibling, lambda, write path — all compile errors |
| Visibility — static METHODS + consts | **ENFORCED** | E-METHOD-VISIBILITY, E-CONST-VISIBILITY |
| Visibility — static FIELDS | **HOLE (P0)** | private/protected static field read AND write from outside: accepted, runs on both backends, PHP leg fatals |
| Static-vs-instance discipline | **PARTIAL** | instance-via-class: enforced both ways for fields+methods; static-via-instance: fields enforced, **methods accepted (hole vs developer intent)** |
| Unknown-symbol positions | **ENFORCED** (10/12) | class/fn/method/field/variant/extend/instanceof/match-type all error; **unknown IMPORT silently accepted** (both loose + project mode) |
| throws discharge | **ENFORCED** | undeclared throw, unhandled call, `?` misuse, main-throws all compile errors; catch-of-never-thrown silently accepted (P2) |
| OOP contracts | **ENFORCED** | abstract, final-by-default, iface impl+sig, MI conflict/cycle, self-extend, trait conflicts, dup defs (file + cross-file), const discipline, immutability |
| Type soundness spot-probes | **ENFORCED** | union leak, generic invariance, arity, optional-into-plain, non-exhaustive match (enum/union/optional), totality |
| Runtime fault parity run≡runvm | **MESSAGE PARITY; LINE DIVERGENCE (P1)** | faults inside `"{…}"` interpolation report the true line on `run`, line 1 on `runvm`; stack-trace frames likewise |
| Project-mode loader rules | **PARTIAL** | E-PKG-PATH, E-FILE-*, E-VENDOR-*, E-DUP-DEF, E-TYPE-IMPORT-*, E-VIS-* all enforced; **reserved-`Core` + package-decl casing NOT checked in project mode** |
| Missing-rules (should-flag) | 7 recommendations | see §7 |

---

## 1. E-code trigger sweep (197 codes)

Method: enumerated every code from `phg explain` registry + `rg '"E-[A-Z0-9-]+"' src/`; wrote one
minimal probe per code; expected `check` (or `run`/`transpile` where the code is a runtime/transpile
gate) to emit that code. [Verified: `pass1-results.txt` + `fix/` re-probes]

- **182/197 triggered on the first pass** (`pass1-results.txt`, all `PASS rc=1` / `W-* rc=0`).
- The 15 first-pass failures resolved as follows (all re-probed, `fix/`):

| Code | Resolution |
|---|---|
| E-CONCURRENCY-NO-PHP | ALIVE — raised on `transpile` of a `spawn` program (pass-1 probed `check`). [Verified: `fix/conc-no-php3.phg` → `transpile` rc=1 E-CONCURRENCY-NO-PHP] |
| E-FOREIGN-RUNTIME | ALIVE — raised on `run` of a `declare` program. [Verified: `fix/foreign-run.phg` → rc=1] |
| E-MATCH-TYPE | ALIVE. [Verified: pass2 `e2-match-unknown.phg`] |
| E-PROPAGATE-POSITION | ALIVE — Result-mode `?` outside a let-initializer (`discard f()?;`). [Verified: `fix/propagate-position3.phg`] |
| E-SHADOW-IMPORT | ALIVE — fires alongside E-NAME-CASE when a local shadows an imported qualifier. [Verified: `fix/shadow-import.phg` emits both codes] |
| E-SPAWN-NOT-CALL | ALIVE — `spawn w` (bare ident; `spawn 42` is a parse error before it). [Verified: `fix/spawn-not-call3.phg`] |
| E-UFCS-AMBIGUOUS | ALIVE — `Core.Math.round` and `Core.Conversion.round` share leaf name AND first-param type (`Float`); importing both + `x.round()` triggers it. [Verified: `fix/ufcs-ambig.phg`] Note: this is the only shipped collision pair; the naming discipline ([[ufcs-generic-subject-leaf-collision]]) otherwise keeps the guard latent. |
| E-WITH-FIELD / E-WITH-NONCLASS / E-WITH-TYPE | ALIVE — pass-1 probes used wrong syntax (`x: 2`; correct is `x = 2`). [Verified: `fix/with-*2.phg`] |
| W-SECRET | ALIVE — `new Secret("k")` + `Output.printLine(s.expose())`. [Verified: `fix/w-secret2.phg`] |
| **E-ALIAS-CYCLE** | **FINDING (P2): rule alive but the diagnostic lost its code tag** — `type A = B; type B = A;` + use → rc=1 `"type alias cycle through 'A'"` with **no `E-` code** (so `phg explain E-ALIAS-CYCLE` documents a code the compiler never attaches). Additionally an **unused** alias cycle passes `check` clean (lazy resolution). [Verified: `fix/alias-cycle.phg` rc=0; `fix/alias-cycle2.phg` rc=1 uncoded] |
| **E-RESERVED-PACKAGE** | **FINDING (P1): CLI-unreachable (dead rule)** — see §2.3. |
| **E-PKG-CASE (package-decl arm)** | **FINDING (P1): decl arm CLI-unreachable** — the import-segment + alias arms are alive (`pkgcase2` project probe); the package-declaration arm is not reachable from any CLI path. See §2.3. |
| **E-OVERLOAD-SELECT-CONFLICT** | Registered in `explain`, **never raised anywhere in src/** — its own explain text says "(Raised once the inferable sinks land)". The scenario it describes IS caught today via the generic assign-type error (`string x = <int>parse("7")` → `expected string, found int`), so no soundness gap — registry cosmetics only. [Verified: `fix/ovl-select-conflict.phg` + `rg` no raise site] |

Project-scoped codes verified separately in project mode (§2.3 table): E-PKG-PATH, E-FILE-NAME,
E-FILE-MULTI-PUBLIC, E-FILE-MIXED-PUBLIC, E-DUP-DEF, E-VENDOR-MAIN, E-VENDOR-MISSING,
E-TYPE-IMPORT-{UNKNOWN,CONFLICT,BUILTIN,SHADOW}, E-VIS-PRIVATE, E-VIS-INTERNAL, E-DECL-PACKAGE,
E-DECL-NONFOREIGN — all ENFORCED. [Verified: `projects-results.txt`]

## 2. Developer's four named cases

### 2.1 Visibility — "private can't be called outside"

| Angle | Result |
|---|---|
| private field read outside | ENFORCED — E-FIELD-VISIBILITY [Verified: v1..v6 probes] |
| private field from subclass | ENFORCED |
| protected field outside / write | ENFORCED |
| private access from a lambda inside main | ENFORCED (lexical position honored) |
| private access from sibling class | ENFORCED |
| private instance METHOD call outside | ENFORCED — E-METHOD-VISIBILITY |
| private STATIC method call outside | ENFORCED — E-METHOD-VISIBILITY [Verified: v7c] |
| private/protected class CONSTANT read | ENFORCED — E-CONST-VISIBILITY |
| **private STATIC FIELD read outside** | **HOLE — accepted, prints the value** [Verified: `v7run.phg` — `run`/`runvm` both print `3`] |
| **private STATIC FIELD write outside** | **HOLE — accepted** [Verified: `v7b-priv-static-write.phg` rc=0, runs clean] |
| **protected STATIC FIELD read outside** | **HOLE — accepted** [Verified: `v7d`] |
| method as first-class value (`var f = a.m`) | rejected — but with the misleading message `type A has no field m` even for a **public** method (method refs on instances aren't values; the diagnostic never says so). P3 diagnostics finding. [Verified: v3 + v3b] |

**P0 — the static-field hole is also a three-way divergence.** The transpiled PHP emits a real
`private static` property, so the SAME program prints `3` on `run`/`runvm` and dies on the PHP leg:
`Fatal error: Uncaught Error: Cannot access private property A::$s` [Verified: transpiled
`v7run.php` under php-8.5.7]. This breaks the run≡runvm≡PHP spine for any program that touches the
hole — and it is the exact guarantee the developer names first ("private can't be called outside").

**Root cause** [Verified: read `src/checker/calls.rs` ~1501, `src/checker/assign.rs` ~209]:
`classes[cls].statics` is a bare `name → Ty` map with **no visibility metadata**. Class constants
carry `entry.vis` and enforce E-CONST-VISIBILITY on the same code path; static fields drop their
`private`/`protected` modifier at collection time, so neither the read path (calls.rs) nor the
write path (assign.rs) can check it. Fix shape: store vis alongside the static's type (mirroring
the consts entry) and gate both paths — same owner/subclass logic E-CONST-VISIBILITY already uses.

### 2.2 Static-vs-instance discipline

| Angle | Result |
|---|---|
| instance field via class name (`A.x`) | ENFORCED — `A has no static field x` (E-STATIC-UNKNOWN) |
| instance method via class name (`A.m()`) | ENFORCED — `m is an instance method of A, not a static one` [Verified: `fix/inst-via-class.phg`] |
| static field via instance (`a.s`) | ENFORCED — `type A has no field s` |
| **static method via instance (`a.m()`)** | **ACCEPTED — checks clean, runs on run/runvm/PHP (prints 7 on all three)** [Verified: s2 probe] |
| `this` in static method | ENFORCED — E-STATIC-THIS |

The static-method-via-instance case is not a runtime divergence (PHP happily allows
`$a->staticMethod()`), but it contradicts the developer's stated rule ("static not via instance")
AND is inconsistent with the field case one row up. Recommendation: ERROR (§7).

### 2.3 Unknown symbols in every position

Instantiate, extend, implement, type-position, `instanceof` RHS, match type-pattern, enum-variant
construct + match, method/field/function reference — **all compile errors** with did-you-mean hints
where applicable. [Verified: e1–e9 probes]

**Exception — unknown IMPORT is silently accepted** in both loose and project mode:
`import Core.Bogus;` / `import Acme.Nothing;` → `OK` while unused; using the qualifier gives the
generic `unknown identifier Bogus` (E-UNKNOWN-IDENT), not an import-site error. Go errors on the
import line; PHP `use` of a bogus symbol is also silent until use — this is a place Phorj should
beat PHP and currently doesn't. [Verified: e6/e6b/e6c + `projects/unkimport`]

**Project-mode gap (P1): the reserved-`Core` and package-decl-casing rules are dead on every CLI
path.** In project mode a user file may declare `package Core;`, `package Core.Output;` — even one
defining `printLine` — or lowercase `package acme;`, and the whole project checks clean.
[Verified: `projects/{corehijack,reserved,pkgcase}` all rc=0]. In single-file mode the loose-script
rule (`only package Main runs as a loose script`) fires first, so E-RESERVED-PACKAGE /
E-PKG-CASE-on-decl are raised **only from unit tests** that call `check()` directly.
Root cause [Verified: `src/checker/program.rs:97-126` + loader behavior]: the checks read
`program.package`, but the project loader flat-merges all files (mangling non-Main defs) **before**
`check()` — per-file package decls never reach the checker; the loader's own per-file validation
(E-PKG-PATH, E-FILE-*) doesn't include them. Mitigation observed: no actual hijack — for a known
native name the registry wins (`Output.printLine` prints `hello`, the user's 666 version is
unreachable); for an unknown name (`Output.sneak()`) the call fails `unknown identifier Output`.
The user's `Core.*` files are accepted then **silently dead** — confusing, and the documented
"reserved `Core.` root" guarantee is unenforced where it matters.

### 2.4 `throws` discharge

| Angle | Result |
|---|---|
| call a throwing fn with no try/`?`/declare | ENFORCED — E-CALL-UNHANDLED |
| `throw` undeclared type | ENFORCED — E-THROW-UNDECLARED (also on re-throw out of a catch) |
| `main` declaring `throws` | ENFORCED — E-UNCAUGHT-THROW ("main may not declare throws") — so an unhandled checked throw is **statically impossible to reach runtime**; no run/runvm fault-text comparison applies (the differential's fault parity covers panics/faults, §5) |
| `?` on a non-Result/non-throwing operand | ENFORCED — E-PROPAGATE-CONTEXT |
| `?` in non-initializer position | ENFORCED — E-PROPAGATE-POSITION |
| over-broad `throws` declaration | ENFORCED — E-THROWS-TOO-BROAD |
| catch of a type the try-block never throws | **ACCEPTED silently** (W-CATCH-UNREACHABLE covers only arm-shadowing) — P2, recommend warning [Verified: t1 rc=0, no W-*] |

## 3. OOP contract probes — all ENFORCED

Instantiate abstract (E-ABSTRACT-INSTANTIATE), unimplemented abstract member (E-ABSTRACT-UNIMPL),
override non-open (E-OVERRIDE-FINAL — final-by-default holds), extend final (E-EXTEND-FINAL),
override signature mismatch (E-OVERRIDE-SIG), unimplemented/mis-signed interface method
(E-IFACE-UNIMPL/E-IFACE-SIG), MI diamond method conflict (E-MI-CONFLICT), MI field conflict
(E-MI-FIELD-CONFLICT), extends cycle incl. self-extend (E-MI-CYCLE), trait method conflict
(E-MI-CONFLICT via trait use), trait ctor collision (E-TRAIT-CTOR-COLLISION), duplicate
fn/field/static/variant/type/const (E-DUP-*; cross-file same-package = E-DUP-DEF in project mode),
const reassignment (E-CONST-REASSIGN), immutable field assignment (E-ASSIGN-IMMUTABLE).
[Verified: pass1 + o1–o3 + dupdef project probe]

Note (DX, P3): a non-`mutable` field can only be initialized at its declaration or via constructor
promotion — a ctor-body assignment to it is E-ASSIGN-IMMUTABLE (g10). Sound (rules out
read-before-init by construction) but the error message doesn't point at the init-at-declaration
rule.

## 4. Type-soundness spot-probes — all ENFORCED

`A|B` into `A` unnarrowed → error; `Box<string>` into `Box<int>` → error (invariance holds,
post-Soundness-Batch-B); arity mismatch → error; `T?` into `T` → E-OPT-ASSIGN; non-exhaustive match
over enum/union/optional → error (m1–m3); bare-variant catch-all footgun now at least warns
(W-MATCH-UNREACHABLE on the dead arm, m4); E-MISSING-RETURN control-flow: `while(true)` no-break
correctly diverges, `while(cond)`+break correctly errors, `?`-in-try half-return correctly errors
first on the propagate context. `match` arms are expression-only (a `{ return 1; }` arm body is a
parse error), so "statement-match where all arms return" is unrepresentable — totality is safe by
construction there. [Verified: y1–y7]

## 5. Runtime fault parity (run vs runvm)

Fault **messages and exit codes are byte-identical** across all 12 probes (div/mod zero, float div
zero, list OOB, negative index, map key missing, force-unwrap null, int overflow, decimal
non-exact, deep recursion, statement-position OOB). [Verified: `pass2rt-results.txt`]

**P1 DIVERGENCE — line attribution:** a fault raised inside a string interpolation (`"{xs[5]}"`)
reports the true line on `run` and **line 1 on `runvm`**; stack-trace frames show the same skew
(`main line 4` vs `main line 1` in the recursion trace; statement-position faults like r12 agree at
the true line). The differential harness compares by FaultKind body substring ([[error-parity-faultkind]]),
so this never fails CI — by design, but it means the "byte-identical" claim excludes fault line
numbers today. Feeds the VM-debug-symbols backlog lane. [Verified: r1 `run: error at 3` vs
`runvm: error at 1`; r6, r11 diffs]

**NaN inconsistency (P2):** `0.0/0.0` faults (`division by zero`) — but `Math.sqrt(-1.0)` returns
NaN on both backends (`n == n` → `false`). So the language faults on one NaN source and
manufactures NaN from another. Both backends agree, so no spine break, but the no-NaN story is
half-enforced. [Verified: r3 + `fix/nan-sqrt.phg` → `false NaN` on both]

## 6. Missing rules (accepted today; probed + verified)

| # | Behavior today | PHP behavior | Recommendation |
|---|---|---|---|
| M1 | **Lambda assignment to a by-value captured `mutable` local compiles; the write silently vanishes** (`x=1; f=function(){x=5;…}; f(); x` prints 1 on both backends) | PHP requires explicit `use ($x)` making the copy visible; `use (&$x)` opts into sharing | **ERROR** (assignment to a by-value capture) — Phorj's capture is implicit, so the footgun is invisible [Verified: g11] |
| M2 | Unknown import accepted while unused (loose + project) | `use` also silent until use | **ERROR at import site** (E-IMPORT-UNKNOWN) — Go model; beats PHP |
| M3 | No unused-local / unused-param / unused-import warnings at all | none in PHP | **WARNING** family (W-UNUSED-*) |
| M4 | Local silently shadows a param / outer local (g4, g5) | PHP has no block scope to shadow | **WARNING** (E-SHADOW-IMPORT/E-SHADOW-FN already exist for the divergence-critical shadows; this is the DX tier) |
| M5 | catch of never-thrown type silent (t1) | PHP silent | **WARNING** (W-CATCH-NEVER-THROWN) |
| M6 | Property hook trivially reading itself (`get => this.p`) compiles, stack-overflows at runtime (both backends, line skew again: run=3 vm=2) | PHP same (fatal at runtime) | **WARNING** on syntactic self-reference [Verified: g9] |
| M7 | `Math.sqrt(-1.0)` → NaN while float division by zero faults | PHP: sqrt(-1)=NAN, 1/0 throws | fault (decimal-division precedent) or return `float?` — align the no-NaN stance |
| — | `==` across unrelated primitive types | PHP `==` juggles | already ENFORCED (brag) [Verified: g6–g8] |

## 7. Recommendations table

| P | Finding | Suggested fix |
|---|---|---|
| **P0** | Private/protected **static field** visibility unenforced; run/runvm≠PHP on the same program | Add vis to the `statics` map entries (mirror consts); gate read (calls.rs ~1501) + write (assign.rs ~209) with the E-CONST-VISIBILITY owner/subclass logic; new/extended code E-FIELD-VISIBILITY |
| **P1** | Reserved `Core.` root + package-decl casing dead on all CLI paths (project mode accepts `package Core;` / `package acme;`) | Run the two decl checks per-file in the loader **before** the flat merge (where E-PKG-PATH already lives) |
| **P1** | Static method callable via instance (`a.m()`) | ERROR (mirror of the enforced field case; developer's stated rule) |
| **P1** | VM fault line = 1 inside interpolation; trace frames skewed | VM-debug-symbols lane; until fixed, document that fault parity excludes line numbers |
| **P1** | Unknown import silent | E-IMPORT-UNKNOWN at the import line (loose + project + vendored) |
| **P1** | Lambda capture-write silently lost | ERROR on assignment to a by-value capture |
| **P2** | E-ALIAS-CYCLE detected but diagnostic uncoded; unused alias cycle passes | re-attach the code; resolve alias graph eagerly |
| **P2** | catch-of-never-thrown silent | W-CATCH-NEVER-THROWN |
| **P2** | No W-UNUSED-* family; silent shadowing | warning channel additions |
| **P2** | NaN producible via sqrt while `/0.0` faults | pick one story |
| **P3** | `var f = a.m` says "no field m" even for public methods | dedicated "method references aren't values (yet)" diagnostic |
| **P3** | E-OVERLOAD-SELECT-CONFLICT registered but never raised | raise it or drop the explain entry until the sinks land |

## 8. Brag list — verified-enforced rules PHP lacks (showcase feed)

All [Verified] via the probes above; PHP comparison = stock PHP 8.5 semantics.

1. **Checked exceptions that actually check** — undeclared `throw`, unhandled throwing call, and a
   throws-declaring `main` are all compile errors; an unhandled checked exception is statically
   unreachable. PHP's `@throws` is a docblock comment.
2. **Compile-time visibility** on instance fields/methods, static methods, and class constants —
   including from lambdas and sibling classes. PHP discovers all of this at runtime.
3. **No type juggling**: `"a" + 1` and `1 == "1"` / `1.0 == 1` are compile errors with explicit-
   conversion hints. PHP's `==` table is the language's most famous footgun.
4. **Integer overflow faults deterministically** on both engines. PHP silently promotes to float.
5. **Non-exhaustive `match` is a compile error** (enums, unions, optionals — with guard-aware
   exhaustiveness E-MATCH-GUARD-EXHAUST). PHP throws UnhandledMatchError at runtime.
6. **Totality**: a value-returning function that can fall off the end is a compile error
   (E-MISSING-RETURN), with `never` verified to diverge (E-NEVER-RETURN). PHP returns null.
7. **Unknown anything is a compile error in every position** — class, function, method, field,
   enum variant (construct AND pattern), extends, implements, instanceof, type pattern — with
   did-you-mean hints. PHP fatals at runtime, and only when the line executes.
8. **Immutable-by-default fields + const discipline** (E-ASSIGN-IMMUTABLE, E-CONST-REASSIGN,
   E-CONST-VISIBILITY). PHP `readonly` is opt-in per property.
9. **final-by-default inheritance** (E-OVERRIDE-FINAL/E-EXTEND-FINAL) + override-signature
   checking (E-OVERRIDE-SIG). PHP allows any override, checks LSP loosely at load.
10. **Null-safety as a type property** — `T?` into `T` rejected, `!` audited (W-FORCE-UNWRAP),
    `?.`/`??` flow-narrowing. PHP nullability is a runtime TypeError.
11. **Invariant generics with real argument checking** (`Box<string>` ≠ `Box<int>`) — erased for
    PHP yet enforced at compile time; PHP has no generics at all.
12. **Whole-project + vendored-dependency type-check** (`OK — whole project type-checks clean:
    N files, M packages … every file + vendored deps`) with package-path (E-PKG-PATH), public-
    surface-per-file (E-FILE-*), duplicate-definition (E-DUP-DEF), cross-package visibility
    (E-VIS-PRIVATE/E-VIS-INTERNAL) and vendor-integrity (E-VENDOR-*) rules. Composer checks none
    of this.
13. **Secret taint-lite** (W-SECRET: an `expose()` flowing straight into a print/write sink warns).
    No PHP equivalent.
14. **Four-backend divergence guards as language rules** (E-SHADOW-IMPORT, E-SHADOW-FN) — the
    compiler forbids programs that would mean different things to different engines.
15. **UFCS ambiguity is an error, not a resolution order** (E-UFCS-AMBIGUOUS). PHP method
    resolution never asks.

— End of report. Probes + raw outputs preserved in the scratchpad path above.
