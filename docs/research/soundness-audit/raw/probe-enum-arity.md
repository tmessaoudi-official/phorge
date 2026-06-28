# Probe: enum variant constructed/matched with correct arity

**Rule:** An enum variant must be *constructed* and *matched* with exactly the number of
payload arguments/fields declared in its definition. Over- or under-applying a variant
(at `new V(...)` or in a `match` pattern) must be rejected.

**Verdict: ENFORCED (no gap).** Every wrong-arity probe — construction and match,
too-few / too-many / zero-payload-with-arg — is rejected at `check` time for the *right*
reason, with a precise "expects N … found M" diagnostic. Correct-arity programs run cleanly.

- enforced: **true**
- is_gap: **false**
- severity: **none**

---

## Probes & evidence

Binary: `/stack/projects/phorj/target/release/phg` (pre-built release, not rebuilt).
Probes under the scratchpad audit dir.

### A. Constructor arity (`new V(...)`)

```
=== ctor-too-few CHECK ===   ( new Percent()  ; Percent declares 1 field )
type error at 10:18: `Percent` expects 1 argument(s), found 0
    Discount d = new Percent();
                 ^
exit=1

=== ctor-too-many CHECK ===  ( new Percent(10, 20) )
type error at 10:18: `Percent` expects 1 argument(s), found 2
    Discount d = new Percent(10, 20);
                 ^
exit=1

=== ctor-zero-with-arg CHECK ===  ( new None(5) ; None is zero-payload )
type error at 10:18: `None` expects 0 argument(s), found 1
    Discount d = new None(5);
                 ^
exit=1
```

All three rejected (exit=1) for the correct reason — variant constructor arity mismatch.

### B. Match-pattern arity

```
=== match-too-many CHECK ===   ( Percent(a, b) over Percent(int) )
type error at 12:9: variant `Percent` expects 1 field(s), found 2
        Percent(a, b) => a + b,
        ^
... (+ E-UNKNOWN-IDENT for the spurious bindings a, b)
exit=1

=== match-too-few CHECK ===    ( Percent() over Percent(int) )
type error at 12:9: variant `Percent` expects 1 field(s), found 0
        Percent() => 99,
        ^
exit=1

=== match-zero-with-bind CHECK ===  ( None(x) over zero-payload None )
type error at 11:9: variant `None` expects 0 field(s), found 1
        None(x) => x,
        ^
... (+ E-UNKNOWN-IDENT for x)
exit=1

=== match-multi-too-few CHECK ===  ( Flat(x) over Flat(int a, int b) )
type error at 10:9: variant `Flat` expects 2 field(s), found 1
        Flat(x) => x,
        ^
exit=1
```

`run` produces the same rejection as `check` for every match probe (checked before
execution), so the bad program never reaches the interpreter/VM. All exit=1.

### C. Sanity baseline (correct arity must run)

```
=== correct RUN ===   ( None()/Percent(p)/Flat(a,b) all matched at correct arity )
7
exit=0
```

A correctly-arity'd program type-checks and runs (`new Flat(3,4)` → `3 + 4` → `7`),
confirming the rejections above are arity-specific, not a probe syntax artifact.

---

## Where it is enforced (for reference)

- Variant constructor arity: `src/checker/calls.rs:587`
  — `"variant `{name}` expects {} argument(s), found {}"`.
- Match-pattern field arity: `src/checker/matches.rs:269`
  — `"variant `{name}` expects {} field(s), found {}"`.

Both run during `check()` (the front end), so the guarantee holds uniformly across all
backends (interpreter, VM, transpiler) — wrong-arity code never reaches any backend.

## Note (out of scope, not a gap for this probe)

The documented zero-payload footgun — a bare `None =>` (no parens) in a `match` is a
*silent catch-all binding*, not a constructor of `None` (see CLAUDE.md / memory
[[zero-payload-variant-call-form]]) — is a *pattern-grammar* quirk, not an arity-enforcement
hole: it is not "wrong arity accepted", it is a different (binding) interpretation of the
syntax, and is already a known, documented design point. This probe's rule (correct arity
when the call/match form `V(...)` is used) is fully enforced.

## Fix recommendation

None required — rule is enforced. No change to `src/checker/calls.rs` or
`src/checker/matches.rs`.
