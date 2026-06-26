# Developer Idea Backlog (running)

> A running log of ideas the developer pops, each with a hard-challenge verdict + recommendation +
> decision. The developer's standing process (2026-06-26): "I'll keep popping ideas till I have none —
> always include them in the roadmap, recommend actions, and discuss one-by-one via `AskUserQuestion`."
> Plan location = repo. Items move to a real milestone/slice plan once decided.

## Lens (constant)
Byte-identity Tier A (gated) vs case-by-case Tier B (impure, quarantined, fixture-tested, transpiles to
PHP). Philosophy: pragmatic, legible PHP upgrade (Phorge:PHP :: TS:JS); remove surprises, never
capability; one obvious way.

## Batch 1 — entry-point / module model + naming (2026-06-26)

### A. `main` not always required
**State [Verified]:** only `phg run`/`runvm` require `main` (`interpreter/mod.rs:235` "no `main` function";
`compiler/program.rs:92`). `check`/`transpile`/`build` do NOT — the transpiler emits the `main()`
bootstrap only `if funcs.contains("main")` — so **library files already work without `main` today**.
**Challenge:** PHP/Python-style top-level execution (no `main`, statements run) fights the deliberate
Go/Rust explicit-entry choice (legibility; no "which file runs first" ambiguity across a package).
**Rec:** formalize "library/web files need no `main`; only running needs an entry" (clearer error,
`phg check` happy with none); KEEP explicit `main` for CLI; allow top-level ONLY for `-e`/stdin quick
scripts (a scripting affordance, not project files). **Decision: TBD.**

### B. argv/argc on `main`
**State [Verified]:** argv already available via `Core.Process.args()` (Tier B); `main` is currently
called with zero args (`interpreter/mod.rs:238`, `vec![]`). **Challenge:** (1) drop `argc` (C-ism →
use `args.length`); (2) a `main` taking argv is argv-dependent → non-deterministic → **Tier B**
(quarantined like any `Core.Process.args()` program); the no-arg `main(): void` stays pure/gated.
**Rec:** add optional `main(args: List<string>): int` (Tier B when used; also gives exit codes), keep
`Core.Process.args()` as primary, no `argc`. **Decision: TBD.**

### C. `index.phg` / web entry
**State:** M6 W1 shipped the pure `handle(Request) -> Response` value model (byte-identity-gated).
**Challenge/answer:** web entry is **not `main`** — it's `handle(Request) -> Response`; `phg serve`
(Tier B socket loop) or the transpiled PHP **front-controller** (`index.php` from superglobals) invokes
it per request. `main` ⇄ CLI, `handle` ⇄ web (parallel conventions); a web file has no `main`
(reinforces A). **Rec:** formalize `handle(Request)->Response` as the reserved web-entry convention;
serving is Tier B, the handler stays gated. (Folds into M6.) **Decision: TBD.**

### D. `len` → `length` naming consistency
**State [Verified]:** 3 words for "how many" — `List.length`, `Bytes.len`/`Text.len`, `Map.size`/
`Set.size`. **Rec (north-star JS/TS):** `length` for ordered/indexed (List, Bytes, Text) + `size` for
keyed collections (Map, Set) — exactly `Array.length`/`String.length` vs `Map.size`/`Set.size`. Rename
`Bytes.len`/`Text.len` → `.length`; keep `Map`/`Set.size`. (Alt: unify everything to `length`.) Pre-1.0
single-dev → hard rename, no alias; ~14 call sites + a codemod. Small, do-able now. **Decision: TBD.**

## Decisions Log
- [2026-06-26] AGREED (Batch 1):
  - **A — ADOPT:** formalize "library/web files need no `main`; only running needs an entry"; keep
    explicit `main()` for CLI; top-level statements only for `-e`/stdin quick scripts. NO PHP-style
    top-level execution in project files.
  - **B — ADOPT:** add optional `main(args: List<string>): int` (Tier B when used; exit codes), keep
    `Core.Process.args()` as primary, **no `argc`**. **`phg run <file> <args…>` passes the actual CLI
    args to `main(args)`** (the post-`--`/post-script argv, via `cli::resolve_source`'s grammar).
  - **C — ADOPT:** reserve `handle(Request) -> Response` as the web entry convention (pure, gated);
    `phg serve` (Tier B) / the transpiled PHP front-controller (`index.php`) invoke it per request.
    Folds into M6. A web file has no `main`.
  - **D — ADOPT:** `length` for ordered/indexed (List, Bytes, Text) + `size` for keyed collections
    (Map, Set), per JS/TS. Rename `Bytes.len`/`Text.len` → `.length` (hard rename, no alias; ~14 sites
    + codemod); keep `Map`/`Set.size`.
