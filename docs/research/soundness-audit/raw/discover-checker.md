# Soundness Audit — Stage 1 DISCOVER (checker / types)

Area: checker + types (`src/checker/**`, `src/types.rs`, `src/parser/items.rs`)
Binary probed: `/stack/projects/phorj/target/release/phg` (pre-built, not rebuilt)
Probes written under the scratchpad audit dir; nothing written into the repo except this report.

## Method

Enumerated every declared rule via `grep -rho 'E-[A-Z-]*\|W-[A-Z-]*' src/checker/` (≈120 codes),
read the parser's class-member path (the seed bug), then wrote one minimal probe per suspected hole and
ran `phg check` / `phg run`, distinguishing "rejected for the right reason" from "accepted = GAP".

## CONFIRMED GAPS (pasted evidence)

### GAP-1 (P0) — constructor visibility modifier parsed and DROPPED
`src/parser/items.rs:511` comment: "Modifiers preceding `constructor` are consumed and dropped". The
`modifiers` parsed in `parse_class_member` are never threaded into `ClassMember::Constructor`.

Probe `p1_privctor.phg`:
```
package Main;
import Core.Console;
class Secret { private constructor(public int x) {} }
function main() -> void { Secret s = new Secret(42); Console.println("{s.x}"); }
```
```
$ phg check p1_privctor.phg
OK (type-checks clean)        [exit 0]
$ phg run p1_privctor.phg
42                            [exit 0]
```
External `new Secret(42)` on a `private constructor` runs and prints 42 — no error.

Broader than `private`: `protected constructor` is ALSO bypassed (probe `p20.phg`):
```
open class Base { protected constructor(public int x) {} }
function main() -> void { Base b = new Base(7); Console.println("{b.x}"); }
```
```
$ phg check p20.phg → OK (type-checks clean)   [exit 0]
$ phg run p20.phg   → 7                          [exit 0]
```
Verdict: P0. Unsound — a declared access rule (the 7th, missing, access site = construction) is silently
ignored. Every OTHER visibility site is enforced (`E-FIELD-VISIBILITY` / `E-METHOD-VISIBILITY`, verified
in p14 + field/method probes), making this the lone hole in an otherwise-complete matrix.

### GAP-2 (P0) — non-optional instance field need not be definitely assigned
A field declared with a non-optional type and NO initializer is NOT required to be assigned by the
constructor. The checker accepts it (a non-optional `T` "can never be null" per S2), but at runtime the
slot does not exist and a read faults. The S2 non-null guarantee is violated.

Probe `p4_defassign.phg`:
```
package Main; import Core.Console;
class C { int n; constructor() {} function getN() -> int { return this.n; } }
function main() -> void { C c = new C(); Console.println("{c.getN()}"); }
```
```
$ phg check p4_defassign.phg → OK (type-checks clean)   [exit 0]
$ phg run p4_defassign.phg   → runtime error at 6: no field `n` on `C`   [exit 1]
```
Also fires on a CONDITIONALLY-assigned field (no definite-assignment flow analysis) — probe `p4c.phg`
(`constructor(bool flag){ if(flag){ this.n=7; } }`, called with `false`): check clean, run faults
"no field `n` on `C`".

Asymmetry that proves this is an oversight: STATIC fields DO require an initializer
(`E-STATIC-NO-INIT`, probe p22 — "static field `count` needs an initializer"), but instance fields do
not. Control probe p4b (field assigned unconditionally in ctor body) runs fine, confirming the field
mechanism itself works.

Verdict: P0. Unsound — lets provably-wrong code (read of a guaranteed-non-null field that is actually
absent) type-check clean.

### GAP-3 (P1, facet of GAP-2) — optional field never assigned faults instead of yielding null
An `int? n` field with no initializer, never assigned, faults at runtime on read rather than reading as
`null`. The checker accepts the optional read; the runtime has no slot. Probe `p21.phg`:
```
class C { int? n; constructor() {} function getN() -> int? { return this.n; } }
… new C(); c.getN() ?? -1
```
```
$ phg check p21.phg → OK (type-checks clean)   [exit 0]
$ phg run p21.phg   → runtime error at 6: no field `n` on `C`   [exit 1]
```
Expected (sound) behavior: an unassigned optional field reads as `null`. Either fix is acceptable
(require init for optional too, or default unassigned optional fields to null). Filed P1 because it is a
declared-rule/contract violation but the "wrong-code" surface is narrower than GAP-2 (the value is at
least typed optional). Same root cause as GAP-2 (no field definite-assignment / default model).

### GAP-4 (P0) — same-head generic types are not invariant at an assignment boundary
Known/logged in CLAUDE.md + memory, re-verified here. `Box<int> b = new Box("hello")` and passing a
`Box<string>` where `Box<int>` is expected both type-check clean.

Probe `p2_invariance.phg`:
```
class Box<T> { constructor(public T v) {} function get() -> T { return this.v; } }
function main() -> void { Box<int> b = new Box("hello"); Console.println("done"); }
```
```
$ phg check p2_invariance.phg → OK (type-checks clean)   [exit 0]
```
Probe `p3_invariance2.phg` (pass `Box<string>` to a `Box<int>` parameter): also `OK (type-checks clean)`.
Root cause (per memory `m-rt-progress` / CLAUDE.md): the nominal assignability check short-circuits on
the reflexive name edge before the invariant type-argument compare in the shared subtype oracle.
Verdict: P0 type hole.

## RULES VERIFIED ENFORCED (negative results — NOT gaps, listed for coverage / bidirectionality)

| Rule | Probe | Result |
|---|---|---|
| External private field read | (existing tests + p14) | `E-FIELD-VISIBILITY` |
| Write non-mutable public field externally | p14 | `E-ASSIGN-IMMUTABLE` |
| Interface method wrong return type | p5 | `E-IFACE-SIG` |
| Interface method missing | p6 | `E-IFACE-UNIMPL` |
| Non-exhaustive enum match | p7 | "non-exhaustive match: missing Blue" |
| Enum variant arity | p8 | "`A` expects 1 argument(s), found 2" |
| Duplicate function decl | p9 | `E-OVERLOAD-DUPLICATE` |
| Extend final-by-default class | p10 | `E-EXTEND-FINAL` |
| Instantiate abstract class | p11 | `E-ABSTRACT-INSTANTIATE` |
| Concrete subclass missing abstract impl | p17 | `E-ABSTRACT-UNIMPL` |
| Override non-open method | p19 | `E-OVERRIDE-FINAL` |
| Read private const externally | p12 | `E-CONST-VISIBILITY` |
| const via instance | p13 | `E-CONST-INSTANCE-ACCESS` |
| static field via instance | p15 | "type `C` has no field `count`" |
| Assign null to non-optional | p16 | `E-OPT-ASSIGN` |
| static field no initializer | p22 | `E-STATIC-NO-INIT` |
| return-on-all-paths (missing else) | p18 | `E-MISSING-RETURN` |

## Suspected-but-not-confirmed / handoff for deeper probing
- Constructor-modifier drop may also swallow a future `static`/`final` ctor modifier — same code path;
  worth a guard regardless of M1 scope.
- Definite-assignment (GAP-2/3) should be re-probed for: assignment only inside a helper method called
  by the ctor; assignment via a loop; trait-provided constructors; inherited fields. The flow engine
  `block_terminates`/`stmt_terminates` exists for returns but there is no analogous field-init flow pass.
- Generic invariance (GAP-4): also check `List<int> = List<string>` (container head), Map key/value
  variance, and Optional/Function type-arg variance — same reflexive-edge short-circuit likely applies.
