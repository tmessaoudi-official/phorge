# Big Marathon: Cross-pkg lift → Soundness → Stdlib charter → Concurrency Plan

> Started 2026-06-29 from `e9d95a6`. Fully autonomous (`_AUTONOMOUS_3C=1`, full 30/8).
> Byte-identical `run ≡ runvm ≡ real PHP 8.5` spine; examples-ship-with-features; commit green
> self-contained changes as we go (project git autonomy). Gate:
> `PHORJ_PHP=/stack/tools/phpbrew/php/php-8.5.7/bin/php PHORJ_REQUIRE_PHP=1 cargo test --workspace`
> + `cargo clippy --all-targets` + `cargo fmt --check`.

## Decisions Log
- [2026-06-29] AGREED: Marathon = **all four spines**, in the recommended dependency order, **fully autonomous** (full 30/8).
- [2026-06-29] AGREED: Order = (1) Cross-package M-RT lift → (2) Soundness long-tail close → (3) Stdlib charter + breadth → (4) Concurrency + server (M6 W4). Rationale: #1 unifies type system ↔ modules and unblocks core.json multi-package + cross-package stdlib; #2 cleans the now-unified base; #3 writes the charter then breadth (multi-package core.json now possible); #4 capstone capability on a solid foundation.
- [2026-06-29] AGREED (session 3, "new big thing + marathon"): developer chose **"all of 1 and 2 and 4 in the recommended order autonomously"** = full Spine-2 soundness long-tail → Spine-4 M6 W4 concurrency capstone, with Spine-3 breadth interleaved as low-risk warm-ups. Pacing: **one heavy slice per context window, commit green, let compaction carry the marathon.** Immediate next = S2.2 method return-overloading (design recorded checkpoint #4).
- [2026-06-29] AGREED (session 2, post-breadth): developer pushed the 13 marathon commits; directive = **do all the rest**, in this **confirmed order** — **Spine 2 soundness first (tractable→heaviest): S2.4 while-let guards → S2.2 method return-overloading → S2.1 generic-result VM operand → S2.3 must-use B/C; then Spine 4 W4 concurrency (capstone) on the cleaned base; Spine-3 breadth interleaved as low-risk warm-ups.** Rationale: don't build the concurrency layer atop known run↔runvm parity gaps; ramp difficulty up rather than opening on the heaviest item.

## Progress
- [2026-06-29] S1.4 cross-package generic library types — DONE `718fa3d` (example-only, already worked).
- [2026-06-29] S1.1 cross-package traits — DONE `cc711b9` (loader symbol-table + resolve `Item::Trait`/`uses` rewrite + transpiler namespace bucketing).
- [2026-06-29] S1.2 lambdas/fn-values in library packages — DONE `5d7beb9` (loader `Expr::Ident` value-resolution arm; Main no-op).
- [2026-06-29] S1.3 core.json multi-package + cross-package map literals — DONE `d63cb9d` (JSON helper `\Main\` prefix + loader `Expr::Map` arm).
- [2026-06-29] S1.5 cross-package single inheritance + parent dispatch — DONE `41fa646` (loader `c.extends` resolution + `Expr::ParentCall` arm). **SPINE 1 COMPLETE.**
- [2026-06-29] **Spine 2 DEFERRED to a dedicated session** (recorded autonomously; reorder, not drop). Rationale: every Spine-2 slice is architecturally heavy and each has a clean documented workaround, so rushing one under context pressure risks the byte-identity spine.
  - **S2.1 generic-result VM operand (`id(7)+1`)** — **PARTIAL DONE (narrow)**: shipped the `generic_ret_from_param: Option<usize>` field on `FunctionDecl` (set in `erase_generics` from the pre-erasure signature when the return is *exactly* an own parameter), copied into the compiler's `FnMeta`, consumed in `ctype`'s `Call`/`Ident` arm (recurse into the echoed argument). Closes `identity(7)+1` / `firstOr(xs,-1)*2` byte-identically (`examples/guide/generics.phg`). **Still deferred** (needs the full span-keyed reified-result side-table threaded into `compile_program`): generic *methods*/*fields* (`box.get()+1`), `List<T>`-element/container returns, multi-param-derived returns. The narrow field rides the AST into the compiler — no `compile()` signature change, no span table (avoids the staleness-across-rewrites trap).
  - **S2.2 method return-overloading** — ✅ **DONE (session 3, this context)**: instance methods may now
    return-overload (identical params, distinct returns), resolved by a `<Type>receiver.m(args)` selector
    and mangled per return (`read__ret_int`) before any backend — **zero backend changes, no new `Op`/
    `Value`**. The free-fn pipeline was already fully parameterized: flipped `validate_new_overload`'s
    `allow_return_overload` to `!sig.is_static` for methods (instance-only; statics keep the classic
    shared-return rule via `E-OVERLOAD-RETURN` — they have no selector call-site path); added
    `finalize_method_overloads` (classify `(class,method)` sets, reuse `ret_overload_mangle` +
    `overload_def_renames`); `check_overload_select` gained a `Member`-callee arm →
    `resolve_method_return_overload` (resolve receiver class, substitute class type args, pick member by
    substituted return, record a mangled *method*-call rewrite into the shared `overload_resolutions`);
    `check_method_call` rejects a bare set with `E-OVERLOAD-NO-CONTEXT`; `rename_overload_defs` gained an
    `Item::Class` arm renaming method members. Byte-identical run≡runvm≡**real PHP 8.5**
    (`examples/guide/method-return-overloading.phg`); new tests: differential `agree_out_php` ×2 (incl.
    `this`-receiver + interpolation), typecheck `bare_…needs_selector`/`selector_picks`/
    `selector_unknown`/`static_methods_cannot_return_overload`. Scope: `package Main` instance methods,
    single declaring class, selector-only (no C2 sink yet); deferred (KNOWN_ISSUES): C2 sink for methods,
    return-overload override across an inheritance/interface hierarchy, generic-class bare-param-return
    member. **Commit pending gate-green.**
  - **S2.2 method return-overloading [original design, now implemented above]** — extend C1's `OverloadSelect`/per-return mangle from free fns to methods. **FULL DESIGN (mapped, pick-up-ready):** per-class method overload sets already exist (`checker::classes[cls].methods[name]: Vec<MethodSig>`), so mirror the free-fn machinery: (1) a `finalize_method_overloads` classifying each `(class, method)` with ≥2 sigs / shared params / distinct returns into a method analog of `return_overload_sets`; (2) `check_overload_select` — currently *rejects* a `Member` callee (calls.rs ~1095) — gains a method arm: resolve the receiver's static class (`check_expr(object)` → `Ty::Named(cls,_)`), pick the member by selector/expected return, mangle (`m__ret_int`); (3) a sink path in `check_method_call` (calls.rs:1012) mirroring `try_resolve_sink_overload`; (4) a method-def mangle pass (extend `rename_overload_defs`, overloads.rs:305 — currently skips methods) renaming the `ClassMember::Method`; (5) the call-site rewrite produces a **method** call to the mangled name (`obj.m__ret_int(args)` — a `Call` with a `Member` callee, preserving the receiver) — NOT a free `Call`. **4-backend dispatch:** interpreter + VM key methods on `(class, name)`; both def-rename and call-rewrite to the mangled name keeps dispatch consistent; transpiler emits `$obj->m__ret_int(...)` (the class must define it). **Scope it C1-equivalent: single declaring class, no override of an overload member across the hierarchy** (defer the inheritance/polymorphic-dispatch interaction — a base-typed receiver resolving the mangled name needs every implementer to rename consistently). Irreducibly multi-commit + byte-identity-critical across all 4 backends — **do in a fresh context.**
  - **S2.3 must-use B/C** — bidirectional must-use propagation (flagged a real arch change in the 4th marathon).
  - ~~**S2.4 while-let guards**~~ — **DONE** (session 2): `while (var x = opt when g)` — a pure parser desugar mirroring the if-let guard (wrap BODY in `if (g) { BODY } else { break }`, so a false guard exits the loop). No `Stmt::If.guard` field, no backend change; byte-identical run≡runvm≡real PHP. Tractable-first pick paid off. `examples/guide/loops.phg`, KNOWN_ISSUES updated (both if-let + while-let guards now ship).
- [2026-06-29] S3.1 stdlib charter — DONE `3a6d2ea` (`docs/specs/2026-06-29-m4-stdlib-charter.md`, ROADMAP M4 adopted).
- [2026-06-29] S3.2 `Core.List.chunk` — DONE `ddfabc4` (charter-compliant; `List<List<T>>`, `array_chunk`, size<1 faults).
- [2026-06-29] S3.3 `Core.Text.lines` — DONE `8ea0b67` (split on `\n`, `explode` semantics).
- [2026-06-29] S3.4 **`Core.Path`** (new module) — DONE (basename/dirname/extension/stem/join; pure path-string manipulation, Tier 1; PHP `basename`/`dirname`/`pathinfo`; `src/native/path.rs` + `path_tests.rs`, `examples/guide/paths.phg`). Algorithms derived from PHP 8.5 ground truth, oracle-verified byte-identical run≡runvm≡real PHP.
- [2026-06-29] S3.3 **`Core.Text` ergonomic breadth** — DONE (`lastIndexOf` → `int?`/`strrpos`; `removePrefix`/`removeSuffix` → Kotlin-style affix trim, `str_starts_with`/`str_ends_with`+`substr` single-eval arrow-IIFE). Extended `examples/guide/text-ops.phg`; oracle-verified byte-identical.
- [2026-06-29] S3.5 **`Core.List.fill`** — DONE (generic `fill(value, count) -> List<T>`; `array_fill(0, n, value)`; element type inferred at the call site; `count < 0` faults, EV-7). **Named `fill`, not `repeat`** — a generic-subject native (bare `Ty::Param` first param) unifies with *every* receiver under UFCS, so sharing the `repeat` leaf with `Text.repeat` made `x.repeat(n)` `E-UFCS-AMBIGUOUS` (caught by the differential `ufcs.phg`). `fill` is unique-leafed → no clash; resolver semantics untouched (the principled "exclude bare-Param-first from UFCS" alternative was rejected — it would break the intentionally-UFCS-eligible `Convert.toString`/`Reflect.kind`/`className`). Extended `examples/guide/list-breadth.phg`; byte-identical run≡runvm≡real PHP. **LESSON: a new generic-subject native must use a leaf name unique across all UFCS-eligible natives.**
- **Spine 3 has a charter + a new module + breadth natives** (`Core.Path` new; `Text.lastIndexOf`/`removePrefix`/`removeSuffix`; `List.chunk`/`fill`; `Text.lines`/`Text.capitalize`; core.json encode/safe-parse earlier; sprintf — genuine design fork (variadic vs list / `%` vs `{}`), deferred for an explicit design call).
- **Spine 4 (M6 W4 concurrency/server) NOT started** — a large milestone (keep-alive, graceful shutdown, then uncolored `spawn`+channels green threads on the VM's reified frames, Tier-3 quarantined per the charter). Start fresh.
- **Marathon checkpoint (8 commits): Spine 1 complete, Spine 3 charter+2 natives; Spine 2 deferred (architectural), Spine 4 pending.**
- **Marathon checkpoint #4 (session 2 cont.): Spine 2 — S2.4 (`33f4d0d`) + S2.1 narrow (`1163e47`) DONE.** Next, in order: **S2.2 method return-overloading** (full design recorded below — fresh context), then **S2.3 must-use B/C**, then **Spine 4 W4 concurrency** (capstone). Session 2 total: 8 commits (3 stdlib breadth + 2 soundness + 3 checkpoints), all green, pushed through `1163e47`-ish (developer pushes).
- **Marathon checkpoint #3 (session 2 cont.): Spine 2 OPENED — `33f4d0d` S2.4 while-let `when` guards DONE** (tractable-first, pure parser desugar, green). **Remaining Spine-2 items are all heavier/architectural and best done in a fresh context (one per session for quality):** S2.2 method return-overloading (multi-site: overload sets are free-fn/bare-name keyed in `overloads.rs`; `check_overload_select` rejects method-call selectors — needs `(class,method)` keying + a `<Type>obj.m(args)` selector grammar + per-return method mangle + method dispatch/sink resolution), S2.1 generic-result VM operand (checker→compiler type side-table threaded through `compile_program`), S2.3 must-use B/C. Then Spine 4 W4 concurrency (capstone). **Pace: one heavy slice per fresh context — do NOT batch them under accumulated context pressure (byte-identity risk).**
- **Marathon checkpoint #2 (13 commits, all green, NOT pushed): + Spine-3 breadth this session** — `48a8f03` Core.Path (new module, 5 fns), `c59bf51` Core.Text `lastIndexOf`/`removePrefix`/`removeSuffix`, `5954a2f` Core.List.fill. Each byte-identical run≡runvm≡real-PHP-8.5, 1259 lib + workspace green, clippy+fmt clean. **Spine 2 (soundness) + Spine 4 (M6 W4 concurrency) still pending — both deliberately deferred to a fresh context (Spine 4 is milestone-scale; the handoff says start it fresh).**

## Formal Plan

### Spine 1 — Cross-package M-RT lift
Lift the `package Main`-only wall. Loader mangle-pass + transpiler namespacing are the heavy machinery.
- S1.1 Cross-package **traits** (`trait` in a library package + cross-package `use`).
- S1.2 Lambdas / first-class fn-values **inside library packages** (loader rewrites lambda bodies + bare fn-value refs to mangled targets).
- S1.3 **core.json multi-package** (injected `Json` enum emitted namespaced, not flat).
- S1.4 Cross-package **generic library types** (`Box<T>` in a library package).
- S1.5 Cross-package **parent calls** (`parent.m()` across package boundary).

### Spine 2 — Soundness long-tail close
- S2.1 **Generic-result VM operand fix** (`id(7)+1` / `box.get()+1` on the VM) — thread reified generic result types into the compiler `CTy`.
- S2.2 **Method return-type overloading** (extend C1 from free-fns to methods).
- S2.3 **must-use Slice B/C** (bidirectional propagation of must-use).
- S2.4 **Pattern-cluster refinements** (while-let guards, same-binding or-patterns where provable).
- S2.5 **Late-static-binding alternative** ergonomics (or document as permanent non-feature with a clean path).

### Spine 3 — Stdlib charter + breadth (M4 / M-Batteries)
- S3.1 Write **`docs/specs/…-m4-stdlib-charter.md`** (naming, subject-first arg order, optional-vs-fault discipline, determinism tiers, native-vs-`.phg` policy).
- S3.2 **core.json encode + safe parse** breadth (now multi-package, post S1.3).
- S3.3 **sprintf / string-format** + more `Core.Text`.
- S3.4 **path / url** breadth on the determinism seam.

### Spine 4 — Concurrency + server (M6 W4)
- S4.1 Server hardening: HTTP **keep-alive**, **graceful shutdown/join**, per-worker metrics.
- S4.2 Uncolored **`spawn`** + **channels** (green threads on the VM's reified call frames), quarantined behind the determinism seam, tested outside `differential.rs`.
- S4.3 `phg serve` CLI + docs + example.

> Each slice: design-check → TDD → implement → full gate green → example + KNOWN_ISSUES/README → commit.
> Scope/deferrals captured in KNOWN_ISSUES as we go. Adjust slice boundaries as discovery dictates.
