# Phorge GA Direction & Autonomy Contract — Plan + Resume Point

> Status: **PAUSED mid-discussion (2026-06-21)** — developer needed to restart the computer.
> This file is the authoritative resume point. Plan-location sentinel: `repo`.
> Baseline commit at pause: see `git log` (S5 = `e73cab9`, parity-review methodology = `a3f487a`,
> plus this session's review-deliverable + philosophy commits).

## ⏸️ EXACT RESUME POINT (read this first)

We were locking the **mutation / modifier model**. State:
- Immutable-by-default: **ACCEPTED** by the developer (after I challenged hard and gave code examples).
- Keyword: **`mutable`** (NOT `mut` — developer chose the spelled-out word).
- **PENDING — the one open question we paused on:** confirm the **modifier model** I proposed:
  four orthogonal axes, one keyword each, eliminate redundant PHP modifiers:

  | Axis | Question | Default | Opt-in keyword |
  |---|---|---|---|
  | Mutability | reassignable after init? | immutable | `mutable` |
  | Compile-time const | named compile-time constant? | — (decl form) | `const NAME = <const-expr>` |
  | Association | instance vs class-level (fields/methods)? | instance | `static` |
  | Extensibility | can class/method be extended/overridden? | final/closed | `open` |

  → **ELIMINATE `final`/`readonly` as value modifiers** (subsumed by immutable-default); `final`-for-
  inheritance becomes the default with `open` as opt-in. `static mutable` = shared mutable class state
  (gated on the mutation+GC milestone; syntax/rules lockable now, runtime later).

  **The unanswered AskUserQuestion was:** "Confirm this modifier model?" (options: confirm / keep
  final+readonly too / refine). **On resume: re-present and get that confirmation, then continue.**

After the modifier model is confirmed, the remaining to-dos before full autonomous GA work begin:
1. Produce the **"gates to bypass for full autonomy"** summary the developer explicitly asked for
   (see Autonomy Contract below — most is already decided; just needs stating + any setup).
2. Reconcile the parity-review matrix verdicts to the craftsmanship-apex lens (see below).
3. Begin **Wave A slice 1** (function ergonomics) design→build.

## Philosophy (LOCKED — governs everything)

Full text in `~/.claude/projects/-stack-projects-phorge/memory/philosophy-of-phorge.md` and the new
**VISION.md "The philosophy" section**. Essence:

> *Phorge starts FROM PHP and is bound only by CRAFTSMANSHIP and effort — it keeps what respects
> SOLID/best-practice/design-patterns, changes what doesn't (familiarity never excuses a compromise),
> adds power that COEXISTS with existing strengths, and the PHP bridge exists to make migration easy,
> not to cap the language.*

- **Apex filter = software craftsmanship** (SOLID / design patterns / best practice). NOT familiarity,
  NOT minimalism/purism.
- **PHP is the floor, not the ceiling, not the identity.** No ceiling — only effort.
- **Familiarity is conceptual (what it DOES) + lightly syntactic**, never a license to keep unsound forms.
- **Transpile (both directions) = migration bridge**, not the soul. Byte-identity spine = honesty enforcer.
- **Additive power: coexistence, never replacement** (multi-inheritance AND traits; overloading AND
  nullable/variadic).
- **Interrogate every feature** for interactions + what must be enforced.
- I (Claude) have a documented bias toward PL-theory maximalism/purism AND toward syntax-preserving
  familiarity — BOTH are wrong. Default question before any proposal: *"most craftsmanship-sound,
  shippable form? familiar concept preserved? coexists with existing strengths? interactions enforced?"*

## Autonomy Contract (decided this session)

- **Autonomy level: TOTAL — no checkpoints.** Design + build + commit everything autonomously, including
  the big architectural milestones (mutation+GC, exceptions, Json/Any, concurrency). Developer reviews via
  commits + specs after the fact.
- **EXCEPTION — stop and wait on a *genuine fork*:** if a real decision has no clear craftsmanship-best
  answer, STOP and ask (do not guess). This overrides "no checkpoints" — autonomy by default, pause only
  at true ambiguity.
- **Always pause regardless:** deny-listed/destructive ops, force-push, data loss (per global safety).
- **Git: auto-commit each green, byte-identical, lint-clean slice. Do NOT push** (developer pushes / asks).
  Force-push never.
- **Engine: use multi-agent workflows where they clearly raise quality** (research/design/review/sweeps);
  inline for ordinary slices. Cost-mindful, quality-first.
- **Gates to bypass (already/− to set):** the per-turn ask-human gate is bypassed via
  `~/.claude/projects/-stack-projects-phorge/ask-human-gate-bypass` (file present — KEEP it). Run in
  `_AUTONOMOUS_3C=1` mode (skip the 3C/6C convergence + phase plan-gates). No per-slice approval gate.
- **Mid-flight forks with a clear craftsmanship answer:** decide + document in this plan's Decision Log +
  continue (the "decide+document+continue" half) — but a *genuine* fork (no clear answer) → STOP (per
  above). [These two answers together = decide-when-clear, stop-when-ambiguous.]

## GA acceptance bar (decided)

**Feature-complete vs PHP + differentiators.** GA = every "adopt" feature shipped + the prerequisite
milestones (mutation+tracing-GC, exceptions + Result, Json/Any, runtime attributes, concurrency M6,
PHP→Phorge migration M8), each **byte-identical `run≡runvm≡real PHP`**, documented, example-gated. Nothing
missing vs PHP, plus the beyond-PHP wins.

## Locked design decisions (this session, post-review)

- **Mutation: immutable-by-default + explicit `mutable`** (ACCEPTED). Keyword `mutable`. Modifier model
  PENDING final confirmation (see Resume Point).
- **Json/Any: sealed `Json` ADT (primary) + a `mixed` escape hatch for rare cases** (developer chose
  "Option 1 AND 2"). `Json = null|bool|int|float|string|List<Json>|Map<string,Json>`, exhaustively matched;
  `mixed` available but must be explicitly narrowed (no implicit use → stays legible/no-surprise);
  transpiles to PHP arrays / json_decode / mixed.
- **Overloading: compile-time unambiguous, most-specific-wins.** Resolved statically by arity + param
  types; `T?` is DISTINCT from `T` for resolution; variadic/nullable overloads allowed only if the set
  stays unambiguous (E-error on any call matching two). COEXISTS with — never replaces — nullable +
  variadic args. Lowers to one dispatching PHP method.
- **Multi-inheritance (flagged by developer as a "real game changer"):** to be added WITHOUT removing
  traits (traits serve other purposes). Revisit mechanism at the traits/inheritance slices. Coexistence.

## Build order (decided in the batched review — see the review spec Decision Log, Batches 1–7)

Review deliverable + full Decision Log: **`docs/specs/2026-06-21-php-parity-and-beyond.md`**
(646 features triaged; 7 themed batches decided). Sequence:

1. **Wave A ergonomics** (interleaved with / before method overloading): function ergonomics (variadics +
   default+named args + spread/destructuring, ONE slice) → sprintf/`Core.Text.format` → pattern cluster
   (guards + payload/struct destructuring + or-patterns + range/list patterns + @-bindings, no new Op) →
   operators (spaceship + bitwise + exponent) → literal-forms lexer batch → let-else + break/continue →
   constants (module + class) → opaque newtypes/refinement → pipeline-first stdlib reshape (data-last).
2. **Stdlib breadth** (∥): sort/sortBy + array/Map breadth + first/last→`T?` + foreach-over-Map/Set;
   `Core.Debug.dump` (canonical) + derive set (Eq/Show/Ord/Default, all four).
3. **Regex**: hand-rolled std-only **subset first** (`Core.Regex`), full PCRE later.
4. **OOP slices (locked order, informed by default-args):** method overloading → `extends`
   (final/closed-by-default + `open`) → traits.
5. **Attributes milestone (AFTER OOP):** FULL PHP-parity runtime reflection (decorate-and-read; Route/ORM/
   Validate/DI). Deterministic via closed-program + canonical iteration order. Inert passthrough + closed
   derive are the cheap sub-channels.
6. **Prioritized: Json/Any dynamic-type design** (unblocks core.json + derive(Json)).
7. **Deferred milestones (build autonomously per the contract, but they reshape the runtime):**
   mutation+tracing-GC (unblocks compound-assign, `++`/`--`, `??=`, while/do-while/C-for, static mutable,
   while-let, clone-with, property set-hooks); exceptions (try/catch/finally/throw/Throwable) — **Result+?
   is the PRIMARY recoverable-error channel, exceptions a PHP-interop bridge**; concurrency (M6
   green-threads); PHP→Phorge migration (M8).

## Reject re-categorization (PENDING matrix reconciliation)

Developer overruled the original ~56-item reject bucket. Authoritative version is the review spec's
Decision Log "Batch 7 → Reject re-categorization" (three groups: KEEP-upgraded / DEFER-to-milestone /
GENUINELY-REMOVED ~12 with documented why + preserved capability). **TODO on resume:** reconcile the inline
matrix "reject" verdicts to those groups, and **temper the "maximal familiarity" entry to the
craftsmanship-apex framing** (familiarity is the on-ramp, NOT a reason to keep unsound syntax — e.g. lossy
`(int)` casts change to checked conversions; do not preserve the footgun syntax).

## Standing constraints (unchanged)

- GRDF org rules: only C1/C2 data to Claude; Phorge is OSS (fine). No sensitive/strategic/RGPD data.
- Phorge git autonomy overrides global Rule 10 (auto-commit; push needs explicit request).
