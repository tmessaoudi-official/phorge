# Agent F — Cross-Language High-ROI Feature Scan

> Full-audit fleet, Agent F. Date: 2026-07-02. Mission: features from **other languages that PHP
> does NOT have** and that would be high-ROI for Phorj — including pure-DX wins.
>
> **Calibration basis [Verified]:** read `CLAUDE.md`, `FEATURES.md`, `ROADMAP.md`,
> `docs/specs/2026-07-01-no-wind-namespace-and-language-surface-design.md`, the prior 555-candidate
> triage SSOT (`docs/specs/2026-06-21-php-parity-and-beyond.md` — verdicts cited as "prior: adopt/
> defer/reject"), the philosophy memory, `examples/guide/` inventory, and spot-checks against
> `src/native/` + `examples/guide/pattern-matching.phg` + `src/value.rs`.
>
> **Not re-proposed because already shipped [Verified against FEATURES.md + guide examples]:**
> generics (fns/methods/classes/enums, erased), unions, intersections, optionals/`??`/`?.`/`!`,
> `never`, match + guards + struct destructuring + type patterns + **or-patterns** (`1 | 2 | 3 =>`
> is live in `pattern-matching.phg:140`), flow narrowing, lambdas/pipe, traits + MI + overloading
> (incl. return-type), property hooks, green threads (`spawn`/`Channel`/`Task`), `bytes`, `decimal`,
> three-tier error model + cause chains, ranges, if-expr, if-let, interpolation, UFCS, default
> params, `with`-expressions, `must-use`/`discard`, text blocks, numeric literals/separators,
> `Secret<T>`, packages/vendoring/lockfile, LSP/DAP/fmt/test/bench/playground/debugger.
>
> **Evidence grading:** repo-state claims are graded inline. ROI grades and recommendations are
> design judgment — [Speculative] unless a prior developer-locked triage verdict exists, in which
> case the verdict is cited [Verified: SSOT line refs per the grep in this audit's session].
> "PHP lacks it" claims are from PHP 8.5-era knowledge [Inferred: training knowledge, spot-checked
> against the transpile floor docs]; each entry states honestly when PHP has a partial form.

**Philosophy filter applied throughout** (per the philosophy memory): apex criterion is
craftsmanship (SOLID/best-practice), not familiarity and not purism; additive power must coexist
with existing strengths; no implicit/action-at-a-distance magic; every runtime feature must map to
idiomatic PHP (compile-time-only features may erase); nothing may threaten the
`run ≡ runvm ≡ real PHP` byte-identity spine.

---

## 1. Master table — 66 candidates, ranked

Ranked by ROI within recommendation tier: **ADOPT-NOW** (13) → **ADOPT-LATER** (27) → **REJECT** (26).

### Tier 1 — ADOPT-NOW

| ID | Feature | Source | What it is | Why PHP lacks it | ROI | Tradeoff | Phorj fit | Transpilability | Recommendation |
|---|---|---|---|---|---|---|---|---|---|
| XL-001 | **Interpolation format specifiers** `"{price:.2f}"`, `"{n:04d}"`, `"{x:,}"` | Python f-strings, C#, Rust `format!` | Format directives inside string interpolation holes: precision, padding, thousands separators, hex | PHP interpolation `"{$x}"` takes no format spec; you must break out to `sprintf`/`number_format` | **DX-pure-win** | Grammar inside strings grows; mitigated by a small closed spec grammar checked at compile time | Front-end: lexer extends `StrSeg::Interp` with a spec; checker validates spec-vs-type; desugars to existing native calls — **no new Op** | `sprintf('%.2f', $x)` / `number_format` / `str_pad` — fully idiomatic, deterministic | **ADOPT-NOW** — the single highest-frequency DX gap left in daily code |
| XL-002 | **Closed derive channel** `#[derive(Equals, Show, Hash, Ord, Default)]` | Rust derive, Kotlin `data class`, Swift `Codable`, Scala case class | Compiler-generated structural equality, string rendering, hashing, ordering for a class/enum | PHP has none — object `==` is property-recursive-loose, no derived hash/ord/show | **High** | Generated code is invisible; mitigated: closed fixed set (never user macros — prior reject of open macros stands), `phg disassemble`/`inspect` show the synthesis | Checker/front-end synthesizes methods pre-backend (like `erase_generics` discipline); attributes surface already exists (`#[Route]`) [Verified: m6-w2 attributes] | Emits the real PHP methods in the transpiled class (`equals()`, `__toString`, …) — plain readable PHP | **ADOPT-NOW** — prior triage: B-derive adopt-shape/defer M11 [Verified: SSOT:156]; generic enums + attributes now make it cheap |
| XL-003 | **Sealed hierarchies + exhaustive match over subtypes** `sealed interface Shape` | Kotlin, Java 17, Scala 3 | A closed set of implementors known at compile time; `match` over the interface type is exhaustiveness-checked with no `_` needed | PHP has only `final`; no closed-hierarchy construct, no exhaustive switch | **High** | None beyond a keyword; it *removes* a surprise (silent non-exhaustive default) | Front-end only: checker collects implementors (whole-program compilation makes "sealed" almost free — `class_implements` table already exists [Verified: M-RT S2]); type patterns + `Op::IsInstance` already do the runtime part | Erases: PHP gets plain interface + classes; exhaustiveness is compile-time-only (legitimate erasure) | **ADOPT-NOW** — prior triage: B-sealed adopt [Verified: SSOT:88]; completes the union/type-pattern story |
| XL-004 | **Machine-applicable fixes** `phg fix [--apply]` | Rust `cargo fix`/rustc suggestions, TS quickfixes, Go `gofix` | Diagnostics carry a structured replacement span; a command applies them (unused import removal, rename-to-suggestion, add missing `import`) | PHP tooling has nothing first-party | **DX-pure-win** | Only risk is a bad auto-edit; mitigated by `--check` default + fmt-stable output + span infra already precise | Rides the existing `Diagnostic` type (spans + hints already there [Verified: S0 diagnostics]); LSP code-actions get it for free | n/a — tooling, no runtime surface | **ADOPT-NOW** — hints already computed; shipping them as edits is the cheap second half |
| XL-005 | **Doc-tests** (runnable ``` examples in `///` doc comments) | Rust, Python doctest, Elixir | Code blocks in API docs are extracted and run by the test runner; docs can never rot | PHP docblocks are inert; no first-party doc-test runner | **High** | Slightly slower test runs; none semantically | `phg test` runner + the differential harness exist; doc-test extraction is a lexer-level pass; **synergy: Phorj's examples-ship-with-features culture is doc-tests by hand today** | n/a — tooling; extracted programs run through the normal 3-way oracle | **ADOPT-NOW** — prior triage deferred (C-doctest [Verified: SSOT:323]) but `phg test` + `phg doc` plans have since landed/firmed; revisit now |
| XL-006 | **Opaque newtypes / branded types** `newtype UserId = int;` | Gleam, Kotlin value class, Haskell newtype, TS brands | A distinct nominal type over a representation; `UserId` ≠ `int` at the boundary, zero runtime cost | PHP has no zero-cost nominal wrapper (a class costs allocation + ceremony) | **High** | Users must convert explicitly at boundaries — that's the point | Checker-only: a nominal name resolving to a rep type, erased pre-backend exactly like `type` aliases but *without* assignment-compatibility; smart constructor = a plain function | Erases to the rep type (`int`) in PHP — compile-time-only, legitimate | **ADOPT-NOW** — prior triage: B-newtype adopt [Verified: SSOT:89]; also the sanctioned answer to rejected refinement types |
| XL-007 | **Optional/Result combinators** `opt.map(f)`, `opt.getOr(x)`, `res.mapError(f)`, `res.andThen(f)`, `opt.okOr(e)` | Rust Option/Result, Kotlin, Swift, Gleam | Higher-order stdlib methods on `T?` and `Result<T,E>` for pipeline-style handling | PHP has no Option/Result at all | **High** | None — additive stdlib | Higher-order native machinery is fully built (`NativeEval::HigherOrder`, re-entrant VM [Verified: S7b-3]); no `Core.Option`/`Core.Result` module exists today [Verified: grep of `src/native/` module inventory this session]; UFCS makes them read as methods | `$x === null ? null : $f($x)` / match-based expansion via the native `php` mapping — mechanical | **ADOPT-NOW** — completes the shipped null-safety + error-model story; pure stdlib recipe work |
| XL-008 | **Compile-time-validated literals** (regex + format strings checked at compile time) | Rust (`regex!`-style, `format!` checking), C# interpolated-string handlers | A literal passed to `Regex.compile("…")` / a format spec is parsed and validated by the *checker*; a bad pattern is a compile error, not a runtime fault | PHP `preg_match` with a bad pattern is a runtime warning/false | **DX-pure-win** | Only literal arguments can be checked (dynamic strings still runtime-checked) — no capability removed | Front-end only: checker special-cases a literal arg to known natives (`Core.Regex`, XL-001 specs); zero backend/Op/Value change; the regex engine already exists in-tree [Verified: core-regex design] | No PHP change — runtime behavior identical; validation is compile-time-only | **ADOPT-NOW** — cheap, removes a whole class of runtime surprises, pure craftsmanship |
| XL-009 | **Auto-import quickfix / import organizer** | Go goimports, TS/IntelliJ auto-import | Unknown-name diagnostic offers "add `import Core.List;`"; a save-action sorts/dedupes imports | PHP IDEs do it; the *language toolchain* doesn't | **DX-pure-win** | None | LSP + `phg fix` synergy; the native registry knows every `(module, name)` pair [Verified: src/native keyed registry], so candidate imports are a lookup | n/a — tooling | **ADOPT-NOW** — **load-bearing for "nothing in the wind"**: mandatory-explicit-imports is only pleasant if the tooling writes them for you; ship alongside namespace-v2 |
| XL-010 | **Tuples + multiple return** `(int, string) f()`, `var (a, b) = f();` | Go, Python, Rust, Swift, C# | Lightweight anonymous product type with positional destructuring | PHP has no tuple type — arrays + `list()` are untyped positional conventions | **High** | One new composite type threads type-checker + CTy; scope-controlled (no 1-tuples, positional only — named fields stay classes) | Prior adopt (A-named-tuples [Verified: SSOT:92]); needs `Ty::Tuple`, a `Value` rep (or reuse List with a checker-level arity/type overlay), destructuring rides the shipped pattern machinery | PHP array `[$a, $b]` + `[$a, $b] = f()` — exactly the idiom PHP devs already write, now typed | **ADOPT-NOW** — unlocks map iteration `(k, v)` ergonomics and multi-return without ceremony classes |
| XL-011 | **Display protocol for user types in interpolation** `interface Printable { function toText(): string }` | Rust `Display`, Kotlin/Java `toString`, Python `__str__` | A user class opts into interpolation/`println` by implementing one interface; checked at compile time | PHP has `__toString` but it is *implicit magic* (invoked by coercion, fatal if absent in string context) | **High** | None — today interpolating an instance is simply unavailable (render is primitive-only [Verified: src/value.rs:461 "Render a *primitive* value… None"]) | Checker: interpolation hole of class type requires `implements Printable`; interpreter/VM call the method (existing dispatch); no new Op | `__toString()` — the one place PHP's magic becomes an *explicit, checked* contract; idiomatic on both sides | **ADOPT-NOW** — upgrade-not-removal of a PHP concept, textbook philosophy fit |
| XL-012 | **let-else / bind-or-diverge** `var x = opt else { return 0; }` | Rust let-else, Swift `guard let` | Bind an optional (or refutable pattern); the `else` block must diverge, and the binding is in scope *flat* for the rest of the block | PHP has no optionals, so no equivalent | **High** | None — removes if-let rightward drift | Parser desugar over shipped if-let + the shipped totality engine (`block_terminates` verifies the else diverges [Verified: totality cluster]); front-end only | `if (($x = e) === null) { return 0; }` — plain PHP | **ADOPT-NOW** — prior adopt (B-let-else, strong [Verified: SSOT:64]); every ingredient already shipped, assembly-only |
| XL-013 | **Labeled loops** `outer: for … { break outer; }` | Java, Kotlin, Rust, Go | Break/continue a named enclosing loop instead of counting levels | PHP has numeric `break 2;` — the footgun form (silently wrong after refactors); labels don't exist | **DX-pure-win** | None; strictly clearer than the count | Parser + compiler jump-target bookkeeping; the VM's jump ops already exist — no new Op, front-end + codegen only | `break 2;` with the depth *computed by the compiler* — emits the exact PHP idiom, but the source is refactor-safe | **ADOPT-NOW** — prior adopt (B-labeled-break [Verified: SSOT:63]); the "upgrade the footgun form" pattern in miniature |

### Tier 2 — ADOPT-LATER

| ID | Feature | Source | What it is | Why PHP lacks it | ROI | Tradeoff | Phorj fit | Transpilability | Recommendation |
|---|---|---|---|---|---|---|---|---|---|
| XL-014 | **Structured concurrency scopes** `scope { spawn …; spawn …; }` — all children joined/cancelled at scope exit | Kotlin coroutineScope, Swift TaskGroup, Python Trio | Tasks cannot leak past their lexical scope; errors propagate to the scope | PHP fibers have no supervision structure | **High** | Constrains fire-and-forget patterns (that's the feature) | Green-thread engine has spawn/join [Verified: marathon A1]; a scope is a compiler-managed join-set; belongs in the pending M-Parallel spec | Concurrency already sits on the quarantine seam outside the PHP oracle; PHP leg = sequential execution or Fiber shim | **ADOPT-LATER** — fold into the M-Parallel plan the developer already commissioned [Verified: no-wind spec §5] |
| XL-015 | **`select` over channels** (wait on multiple, first-ready wins) | Go select, Erlang selective receive | Multiplex several channels/timeouts in one blocking construct | No PHP equivalent | **High** | Nondeterminism must be tamed (deterministic policy, e.g. declared priority order, to protect the spine) | Scheduler already parks/wakes tasks; select = park-on-many; likely one new Op or a `Core.Async` native | Quarantined with the rest of concurrency | **ADOPT-LATER** — the standard companion Phorj's channels currently lack; M-Parallel |
| XL-016 | **Deadlines/timeouts on channel ops** `receive(ch, timeout)` | Go context, Kotlin withTimeout | Bounded blocking receive/join | No PHP equivalent | Medium | Introduces time into semantics → determinism policy needed (same seam as `now()`) | `Core.Async` native on the scheduler | Quarantined | **ADOPT-LATER** — M-Parallel |
| XL-017 | **Actor-model parallelism** (per-heap threads + owned-value message passing) | Erlang/Elixir, Swift actors | True multicore without shared memory: each actor owns an isolated heap; messages move owned values | PHP is shared-nothing per *request*, not in-process | **High** | Major engineering; API design must keep single-threaded code untouched | Already identified as the best structural fit for the `Rc` heap in the no-wind spec's brainstorm table [Verified: spec §5]; the *only* multicore path that keeps the 2.4× Rc win | Quarantined; PHP leg degrades to sequential | **ADOPT-LATER** — this IS the M-Parallel headline; deep-plan commissioned, don't pre-empt it here |
| XL-018 | **`defer` statement** `defer file.close();` | Go, Zig, Swift | Schedule cleanup at scope exit, written next to the acquisition | PHP has only `finally` (cleanup far from acquisition) and destructor timing (nondeterministic-feeling) | Medium | Honest tradeoff vs `finally`: two cleanup idioms in one language; LIFO ordering must be taught | Compiler lowers to try/finally over the remainder of the scope — front-end + existing exception ops | `try { … } finally { … }` nesting — mechanical, readable | **ADOPT-LATER** — value is real but low until stateful handle types (XL-019) exist |
| XL-019 | **Resource blocks / `using`** `using (var f = File.open(p)) { … }` | C# using, Java try-with-resources, Python with | Scoped acquisition with guaranteed checked release via a `Closeable` interface | PHP relies on destructors/manual `fclose` | Medium | Needs handle-based resources; `Core.File` is deliberately stateless today [Verified: no-wind spec Q2] | Interface + desugar to try/finally; pairs with any future handle-based IO (sockets, DB in M6+) | `try/finally` + explicit `->close()` | **ADOPT-LATER** — land with the first real handle type (DB/socket), not before |
| XL-020 | **try-expression → Result bridge** `var r = try parse(s);` where `r: Result<T, E>` | Swift `try?`, Zig `catch`, Scala `Try{}` | Capture a `throws`-world call into the value world as `Result` in one keyword | PHP exceptions have no value-world bridge | Medium-High | None semantically — makes the three-tier model *composable* across tiers | Front-end desugar to try/catch building `Success`/`Failure`; existing exception Ops; checker knows the throws-set → the `E` type | `try { $r = Success(f()); } catch (E $e) { $r = Failure($e); }` — plain PHP | **ADOPT-LATER** — natural once XL-007 combinators exist; the pair completes the error model |
| XL-021 | **Semver enforcement** `phg semver-check` (API diff against the last release) | Elm (enforced!), cargo-semver-checks | Tool diffs the public API surface and *blocks* an under-bumped version | Nothing in the PHP world does this | **High** | Requires a machine-readable API snapshot format | `SEMVER.md`/`STABILITY.md` policies exist [Verified: repo root]; the checker owns the full typed surface — exporting a surface manifest is cheap; whole-program compilation makes diffing exact | n/a — tooling | **ADOPT-LATER** — flip to NOW at first tagged public release; pre-GA it gates nothing |
| XL-022 | **Inline snapshot tests** `assertSnapshot(actual)` with in-source auto-updated expected blocks | Rust insta, Jest | Runner writes/updates the expected literal into the test source under a flag | No first-party PHP equivalent | Medium | Blessed-update workflows can rubber-stamp regressions; mitigate with review-diff discipline | `phg test` + `phg format` (canonical rewriting exists) make source-patching safe | n/a — tooling | **ADOPT-LATER** — prior defer (O-snapshot → M-Test follow-up [Verified: SSOT:335]); stands |
| XL-023 | **Property-based testing** `Core.Test.forAll(gen, prop)` | Haskell QuickCheck, Rust proptest, Elixir StreamData | Randomized-but-seeded input generation + shrinking | No first-party PHP equivalent | Medium | Shrinker engineering; determinism needs the seeded-Random seam (exists [Verified: M-Test design]) | Generators = closures; higher-order natives already run closures | n/a — tooling | **ADOPT-LATER** — prior defer stands; differential harness already covers the highest-value ground |
| XL-024 | **Deprecation with replacement + codemod** `#[deprecated(use = "X")]` + `phg fix` applies the rename | Rust, Elm messaging, Go fix | Deprecations carry the machine-readable replacement; the fixer migrates callers | PHP `@deprecated` is prose | Medium-High | None | Rides XL-004 fix infra + the adopted deprecation policy (S-deprecation-policy [Verified: SSOT:403]); the naming-overhaul codemods are this tool done by hand today | n/a — tooling | **ADOPT-LATER** — after XL-004 lands; pre-1.0 renames (already frequent) become one command |
| XL-025 | **REPL** `phg repl` | Python, Elixir iex, Kotlin | Interactive incremental evaluation with persistent bindings | `php -a` is famously weak | Medium | Incremental redefinition vs immutable-by-default needs design (shadowing model) | The M-DX debugger REPL [Verified: memory M-DX S5] is 70% of the machinery; interpreter-only is fine (no parity burden — a REPL is a dev surface, not a program) | n/a — tooling | **ADOPT-LATER** — already on M12's list; debugger REPL is the seed |
| XL-026 | **Compile-time asset embedding** `embed "logo.png" as bytes` | Go //go:embed, Rust include_bytes! | A file's content becomes a `bytes`/`string` constant at compile time | PHP always reads at runtime | Medium | Binary size; path resolution must be project-root-relative and deterministic | Loader reads at compile time → `Op::Const` bytes; deterministic (content is fixed at build) so it stays *inside* the differential spine — unusual for IO; huge for `phg build` single-file executables + `phg serve` static assets | Emit the content as a PHP literal (deterministic) — or `file_get_contents` + committed file; literal keeps byte-identity | **ADOPT-LATER** — natural M2.5/M6 companion |
| XL-027 | **Workspaces / monorepo** (multi-package roots, shared lockfile) | Cargo workspaces, Go modules, pnpm | Several first-party packages developed together with one lock/vendor | Composer's answer (path repos) is clunky | Medium | Loader/manifest complexity | `phorj.toml` walk-up + source-root machinery exists [Verified: M5 S2a-b]; transitive deps are already the named next gap | n/a — build system | **ADOPT-LATER** — with M5's transitive-deps follow-up |
| XL-028 | **Typed JSON (de)serialization derive** `#[derive(Json)]` | Rust serde, Swift Codable, Kotlin kotlinx.serialization | Compiler-generated encode/decode between a class/enum and JSON, type-checked | PHP `json_decode` returns untyped soup | **High** | Schema-evolution policy (missing/extra fields) must be decided | Rides XL-002 derive + `Core.Json` + generic enums (`Result`) — all shipped; prior defer B-derive-json M11 [Verified: SSOT:157] | Generated PHP encode/decode methods — plain readable PHP | **ADOPT-LATER** — first derive wave (XL-002) ships equality/show; Json is wave 2 |
| XL-029 | **Slice syntax** `xs[1..3]`, `xs[..n]` | Python, Rust, Kotlin | Range-indexed sublist, bounds-clamped or checked | PHP needs `array_slice`/`substr` calls | Medium | Clamp-vs-fault semantics must match indexing discipline (Phorj faults on OOB index) | Ranges + `Op::Index` exist; checker types `List<T>[Range] → List<T>`; likely reuses runtime-polymorphic Index like the Map arm did [Verified: M-RT S3 pattern] | `array_slice($xs, 1, 2)` / `substr` — idiomatic | **ADOPT-LATER** — nice compression of shipped parts; not urgent |
| XL-030 | **Range patterns in match** `1..=5 => …` | Rust, Swift | Match an int against an inclusive range | PHP match has no ranges | Medium | None | Pattern kind lowering to two comparisons; pattern machinery is rich already | `$x >= 1 && $x <= 5` guard in the dispatch chain | **ADOPT-LATER** — small win; batch with the next pattern slice |
| XL-031 | **@-bindings** `Circle c @ Circle { r } =>` (bind whole + parts) | Rust, Haskell | Bind the matched value while also destructuring it | No PHP analog | Medium | None | Prior adopt (B-at-bind [Verified: SSOT:114]); pattern plumbing exists | Compiles like existing patterns + one extra local | **ADOPT-LATER** — batch with XL-030 |
| XL-032 | **List patterns** `[first, ..rest] => …` | Elixir, Rust, JS/TS | Destructure head/tail or fixed shape of a list in match | PHP `[$a, $b] = $arr` exists for *assignment* but not as a guarded match pattern | Medium | O(n) tail copy under COW — must be stated | Pattern kind + `Op::Index`/slice; needs XL-029's slice for `..rest` | `count()` guard + `array_slice` | **ADOPT-LATER** — after XL-029 |
| XL-033 | **Trailing closure syntax** `items.each(function(x) { … })` → `items.each { x -> … }` | Kotlin, Swift, Ruby blocks | A final closure argument moves outside the parens | PHP closures are always in-args noise | Medium | Two ways to write a call; Kotlin-scale familiarity is decent | Parser-only sugar to the existing call form; zero backend | Emits the normal closure-arg call | **ADOPT-LATER** — real DX for the higher-order stdlib, but wait for usage data from XL-007; reject implicit `it` regardless (XL-053) |
| XL-034 | **Pipe topic/placeholder** `x \|> f(_, 10)` | Hack `$$`, Elixir capture, F# | Pipe into a non-first argument | n/a (Hack has it; PHP doesn't) | Medium | A placeholder token is mild magic; first-arg-only is teachable | Parser lowering, like the shipped pipe | Plain call | **ADOPT-LATER** — collect real friction first; UFCS already covers most non-first-arg cases |
| XL-035 | **`nameof(x)`** | C#, Swift #keyPath-lite | Compile-time constant string of an identifier — refactor-safe strings for logs/errors/reflection keys | PHP has `::class` only for classes | Medium | None | Front-end intrinsic → `Op::Const` string; must live under `Core` per nothing-in-the-wind (`Core.nameof`) | Emits the string literal | **ADOPT-LATER** — small, clean; batch with a diagnostics/reflection slice |
| XL-036 | **WASM build target** `phg build --target wasm32` | Rust, Go | Ship a program as a wasm module | PHP-in-wasm is exotic | Medium | Toolchain surface only | The playground already runs the engine in wasm [Verified: playground-wasm memory]; this is packaging, not language | n/a | **ADOPT-LATER** — M2.5 Phase 3 adjacency |
| XL-037 | **Purity/effect annotation** `pure function f(…)` | Haskell (types), D pure, Koka effects | Checker-verified freedom from IO/impure natives; safe to reorder/memoize/parallelize | PHP has no effect tracking | Medium | Split-world risk (effect-polymorphism pain) if made mandatory; keep opt-in | The stdlib already has the determinism/quarantine tier split [Verified: M4 charter + differential exclusion]; a `pure` fn = "calls only tier-1 natives + pure fns" — a reachability check | Erases (compile-time-only) | **ADOPT-LATER** — becomes valuable as the data-parallel `List.map` story (M-Parallel table) needs provable purity |
| XL-038 | **Contracts** `requires n > 0; ensures result >= 0;` | Eiffel, D, Ada 2012 | Declared pre/postconditions checked at runtime (dev profile) and stated in docs | PHP has asserts only | Medium | Runtime cost (profile-gated — Dev/Release profiles exist [Verified: M-DX S0]); risk of overlapping with rejected refinement types — keep runtime-checked, never solver-backed | Desugars to `Core.assert` at entry/exit; front-end only | `assert()` calls, or stripped in release | **ADOPT-LATER** — honest middle ground the prior refinement-type reject pointed toward |
| XL-039 | **Raw string literals** `r"\d+"` | Rust r"", Python r'', C# @"" | No escape processing — regex/Windows-path ergonomics | PHP single-quotes still process `\\` and `\'`; nowdoc is heavyweight | Medium (narrow) | Third string form (after `"…"`, text blocks) | Lexer-only | Emits an escaped PHP single-quoted string | **ADOPT-LATER** — pairs naturally with XL-008 regex work |
| XL-040 | **Generators / lazy sequences** `yield` | Python, Kotlin sequence, C#, JS | Lazily-produced streams | PHP *has* generators — included only to place it: prior triage rejected lazy-seq for transpile-divergence risk [Verified: SSOT:590], but the coroutine engine changed the calculus and **A2 generators is already the named next marathon step** [Verified: memory marathon-a1 "NEXT = A2 generators"] | Medium | Laziness vs byte-identity needs the same care as green threads | Green-thread coroutine engine is the substrate | PHP generators exist as a target | **ADOPT-LATER** — already in-flight as marathon A2; noted here only so this report doesn't contradict the plan |

### Tier 3 — REJECT (with the philosophy-grounded reason)

| ID | Feature | Source | ROI | Recommendation + reason |
|---|---|---|---|---|
| XL-041 | async/await (colored functions) | JS/TS, C#, Rust, Python | — | **REJECT** — prior developer-locked reject [Verified: SSOT:281]; green threads are the chosen uncolored model; two concurrency colors is a capability *loss* in legibility |
| XL-042 | Open macros (proc/hygenic) | Rust, Elixir, Scala | — | **REJECT** — prior reject stands [Verified: SSOT:589]: open metaprogramming breaks "refuses to lie"/legibility; the closed derive channel (XL-002) is the sanctioned answer |
| XL-043 | `comptime` compile-time execution | Zig | — | **REJECT** — arbitrary compile-time execution is untranspilable and illegible to the audience; XL-008's *closed* compile-time validation captures the safe 20% |
| XL-044 | Decorators (runtime wrappers) | Python, TS legacy | — | **REJECT** — action-at-a-distance call rewriting; Phorj's closed attribute surface (`#[Route]`, derive) covers declarative uses without hidden control flow |
| XL-045 | User operator overloading | C++, Rust, Swift, Kotlin | — | **REJECT** — prior reject (`$a->__add($b)` hidden dispatch [Verified: SSOT:575]); `decimal` shows the native-type path for genuinely arithmetic domains |
| XL-046 | Extension functions on foreign types | Kotlin, C#, Swift | — | **REJECT** — UFCS already gives call-site fluency [Verified: shipped]; *declaring* members on types you don't own adds orphan/resolution ambiguity for marginal gain |
| XL-047 | Scope functions `let/run/apply/also/with` | Kotlin | — | **REJECT** — a five-way near-synonym zoo is the opposite of legibility; pipe + `with` + if-let cover the use cases |
| XL-048 | Cascades `obj..a()..b()` | Dart | — | **REJECT** — `with`-expressions + method chaining cover it; a second statement-sequencing syntax earns nothing |
| XL-049 | Collection comprehensions `[x*2 for x in xs if p(x)]` | Python, Haskell | — | **REJECT** — map/filter with lambdas is the already-shipped, PHP-dev-teachable spelling; comprehensions add a parallel grammar with zero new capability |
| XL-050 | LINQ query syntax | C# | — | **REJECT** — an embedded sub-language; method-chain form (shipped) is the same power without the second grammar |
| XL-051 | Implicit lambda parameter `it` / `$0` | Kotlin, Swift | — | **REJECT** — implicit bindings are anti-legibility; a named param costs 4 characters |
| XL-052 | Placeholder partial application `_ * 2` | Scala | — | **REJECT** — same implicitness objection; lambdas are one token longer |
| XL-053 | Structural records/shapes | TS, Elm, OCaml | — | **REJECT** — prior reject [Verified: SSOT:428]; Phorj is deliberately nominal (instanceof, sealed, interfaces all lean on it) |
| XL-054 | Refinement/liquid types | LiquidHaskell, F* | — | **REJECT** — prior reject (solver-backed [Verified: SSOT:587]); XL-006 newtypes + XL-038 contracts are the pragmatic slice |
| XL-055 | Typestate / linear types | Rust (affine), ATS | — | **REJECT** — prior reject; type-system weight far beyond the audience |
| XL-056 | Higher-kinded types / typeclasses | Haskell, Scala | — | **REJECT** — interfaces + traits + erased generics cover the practical ground; HKT is illegible to the audience and un-erasable to PHP idiom |
| XL-057 | Variance annotations `in`/`out` | Kotlin, Scala, C# | — | **REJECT** — prior reject; erased generics are invariant by design (recently *enforced* — soundness batch B) |
| XL-058 | Const generics `Array<T, N>` | Rust, C++, Zig | — | **REJECT** — no PHP mapping, no use case without sized arrays |
| XL-059 | GADTs | Haskell, OCaml | — | **REJECT** — prior reject; expert-only power |
| XL-060 | Units of measure `float<m/s>` | F# | — | **REJECT** (as a type-system feature) — XL-006 newtypes give the checked-boundary 80%; unit *arithmetic* needs operator overloading (rejected) |
| XL-061 | Guaranteed TCO | Scheme, Elixir | — | **REJECT** — prior reject [Verified: SSOT:577]: a program that lives under TCO dies under transpiled PHP — a direct spine violation |
| XL-062 | `method_missing` / dynamic dispatch by name | Ruby, PHP `__call` | — | **REJECT** — already litigated as Q1 [Verified: no-wind spec]: un-typeable; method-references + typed registries are the adopted answer |
| XL-063 | Implicits / givens / context parameters | Scala | — | **REJECT** — invisible argument flow is the canonical action-at-a-distance |
| XL-064 | do-notation / Gleam `use` callback flattening | Haskell, Gleam | — | **REJECT** — monadic sugar is illegible to PHP devs; `?`-propagation + if-let + XL-020 cover the practical chains |
| XL-065 | Chained comparisons `a < b < c` | Python | — | **REJECT** — silently means something else in every C-family language incl. PHP; a portability *surprise*, the exact thing Phorj removes |
| XL-066 | Hot code reload | Dart, Erlang | — | **REJECT** — `phg serve --dev` + sub-second compiles cover the dev loop; runtime code-swap breaks the compile-time-config invariant [Verified: config-must-be-compile-time memory] |

Also considered and **excluded as "PHP already has it"** (out of the brief's scope): named
arguments, variadics/spread, backed enums + enum methods, `Iterator`/`foreach`-over-objects,
anonymous classes, `readonly`, first-class callables, heredoc/nowdoc, `match` comma-lists,
list-destructuring assignment, fibers, attributes.

---

## 2. TOP-15 deep dives (ranked by ROI)

### 1. XL-001 — Interpolation format specifiers *(DX-pure-win, ADOPT-NOW)*
Phorj already won the interpolation war (`"{expr}"` with full expressions and absolute spans), but
every real program eventually prints money, percentages, padded columns, and hex — and today that
means leaving the string for `Core.String`/`Core.Math` call chains, exactly the ergonomic cliff PHP
devs know from `sprintf`. A closed spec mini-grammar (`{x:.2f}`, `{n:>8}`, `{n:04}`, `{x:,}`,
`{b:x}`) is checked against the hole's *static type* at compile time (a `:.2f` on a `string` is a
compile error — something Python cannot do), desugars in the checker to existing natives, and adds
no Op, no Value, no PHP-side novelty (`sprintf` is the idiomatic target). It also composes with
XL-011: format specs for primitives, `Printable` for classes. The one design constraint: the spec
grammar must be closed and versioned, never user-extensible, to stay legible. Estimated at roughly
the size of the shipped `html"…"` sugar. Highest daily-touch frequency of anything in this report —
[Speculative] on grade, but every corpus of guide examples in the repo already fakes it with
`Math.round` + concatenation.

### 2. XL-002 — Closed derive channel *(High, ADOPT-NOW)*
The single biggest boilerplate deleter available. Phorj classes today get reference-free structural
`eq_val` internally, but *users* cannot ask for value equality, ordering, hashing, or a debug
rendering on their own types without hand-writing them. A **fixed, compiler-owned set** —
`#[derive(Equals, Show, Hash, Ord, Default)]` — is the craftsmanship-respecting subset of macros:
the prior audit already rejected open macros and pointed at exactly this closed channel as the
answer [Verified: SSOT:589]. Implementation is a front-end synthesis pass in the
`erase_generics`/`expand_aliases` chokepoint family (synthesize method ASTs before any backend), so
`run≡runvm≡PHP` holds by construction, and the transpiled PHP contains the *real, readable* derived
methods — no runtime reflection, no magic. It unlocks XL-028 (`derive(Json)`) as wave 2 and gives
`Map` keys/`Set` membership a story for user types (currently `HKey` is primitives-only). Risk:
generated-code invisibility — mitigated because `phg transpile` and `inspect` literally show it.

### 3. XL-003 — Sealed hierarchies *(High, ADOPT-NOW)*
Phorj already has the *hard* parts: exhaustive match over enums and over closed union types, type
patterns, and a whole-program `class_implements` table. What's missing is letting a *nominal
hierarchy* be the closed set: `sealed interface Payment` + `match p { Card c => …, Cash c => … }`
with no `_` arm and compile-time exhaustiveness. Kotlin/Java 17 proved this is the mainstream way
teams model domain sums when variants carry rich behavior (enums with payloads cover data-shaped
sums; sealed covers behavior-shaped ones). Because Phorj compiles whole programs (no separate
compilation), "sealed" is nearly free — the implementor set is already computed. Front-end only, no
new Op (type patterns reuse `Op::IsInstance`), erases to plain PHP interfaces (exhaustiveness is
compile-time-only, a legitimate erasure like generics). Prior triage: adopt [Verified: SSOT:88].
This also future-proofs XL-014/17 message-type modeling.

### 4. XL-004 — `phg fix`: machine-applicable diagnostics *(DX-pure-win, ADOPT-NOW)*
The diagnostics system already computes caret spans, did-you-mean suggestions, and stable codes
[Verified: S0 + M-DX S1]. Today those suggestions die as prose. Attaching a structured
`(span, replacement)` to the diagnostics that already know the answer — unknown name with one
candidate, unused import, deprecated name, missing `import Core.X;` — and shipping `phg fix`
(check-only by default, `--apply` to write) converts the compiler from a critic into a collaborator.
This is the Rust lesson: suggestion-quality diagnostics changed the language's reputation more than
any feature. Zero language-surface change, zero parity risk, and it becomes the delivery vehicle
for every future rename (the naming-overhaul codemods were exactly this tool, hand-rolled in
Python). LSP code-actions fall out of the same data. Prerequisite for XL-009 and XL-024.

### 5. XL-005 — Doc-tests *(High, ADOPT-NOW)*
Phorj has a developer-enforced "examples ship with features" rule and a differential harness that
globs every example — the project already *believes* in executable documentation; doc-tests are
that belief applied to API docs. Extract fenced code blocks from `///` comments, run each through
the same 3-way oracle as examples, fail the build when a doc rots. The prior audit deferred this
[Verified: SSOT:323], but two things changed: `phg test` shipped, and a `phg doc` generator is on
the M7/M12 slate — doc-tests should be designed *into* the doc comment format now, not retrofitted.
Rust's experience: doc-tests are the single reason its stdlib docs are never wrong. Pure tooling,
no language surface, no transpile question. The stdlib natives (now ~30 `Core.*` modules
[Verified: grep this session]) are the immediate beneficiary — their contracts currently live only
in tests.

### 6. XL-006 — Opaque newtypes *(High, ADOPT-NOW)*
`newtype UserId = int;` — a nominal type that is *not* assignment-compatible with its
representation, so `takesUserId(orderId)` is a compile error at zero runtime cost. This is the
highest-leverage type-safety feature per line of implementation in existence (Gleam/Kotlin value
classes/TS brands all converged on it), and it's the sanctioned pragmatic slice of the rejected
refinement-types idea: pair it with a smart constructor (`UserId.of(int) -> UserId?`) and you get
"validated at the boundary, trusted everywhere else" without a solver. Implementation rides the
`type` alias machinery (parse, resolve, erase) with one behavioral flip: *no* implicit
compatibility. Erases to the rep type in PHP — compile-time-only, exactly like generics. Prior
triage: adopt [Verified: SSOT:89]. Interactions to interrogate (per philosophy tenet 5): overload
resolution treats `UserId` ≠ `int` (good — that's the point); interpolation/format uses the rep.

### 7. XL-007 — Optional/Result combinators *(High, ADOPT-NOW)*
The null-safety surface (`??`, `?.`, `!`, if-let) covers *consuming* an optional, but not
*transforming* one; the error model has `Success`/`Failure` and `?` but no value-level algebra. A
small combinator set — `Optional`: `map`, `flatMap`, `getOr`, `filter`, `okOr`; `Result`: `map`,
`mapError`, `andThen`, `getOr`, `toOptional` — turns both into pipeline-friendly citizens
(`parse(s) |> r -> r.map(f).getOr(0)` style, reading naturally under UFCS). Every mechanism needed
already shipped: generic natives with `Ty::Param` sigs, `NativeEval::HigherOrder` with the
re-entrant VM closure invoker, UFCS. No `Core.Option`/`Core.Result` module exists today
[Verified: native module grep this session]. PHP mapping is mechanical per-native (`$x === null ?
null : $f($x)` etc.). This is a stdlib recipe exercise (the "collection-native recipe" memory
applies verbatim) — days, not weeks, and it makes the error model feel finished.

### 8. XL-008 — Compile-time-validated literals *(DX-pure-win, ADOPT-NOW)*
Phorj owns its regex engine and its checker, and literals are statically known — so
`Regex.compile("(\d+")` failing at *compile time* with a caret under the unbalanced paren is
almost embarrassingly cheap: the checker special-cases literal arguments to a known-validatable
native and runs the existing parser over them. Same trick powers XL-001's format specs and any
future `Csv`/`Url` literal contracts. Dynamic strings keep today's runtime behavior — no capability
removed, no semantics changed, no backend touched, no PHP delta (validation is purely front-end).
This is the *safe* fraction of Zig's comptime, extracted without the metaprogramming. The pattern
generalizes into a small internal API ("literal validators") that future natives can register into
— worth designing once, generically.

### 9. XL-009 — Auto-import quickfix *(DX-pure-win, ADOPT-NOW — sequenced with namespace-v2)*
"Nothing in the wind" is the right principle and a real typing tax: every `Console.println` needs
its `import Core.Output;` (post-rename [Verified: Core.Output in native registry]). Go proved the
resolution: mandatory explicitness is *loved* when goimports writes the line for you. Phorj's
native registry is keyed `(module, name)` and the loader owns the user-package symbol tables — so
"unknown name `println`, exactly one candidate module" is a lookup, emitted as an XL-004 structured
fix and an LSP code-action, plus an import-sorting canonical form in `phg format`. This should ship
*in the same milestone* as the namespace-v2/deep-imports plan the developer commissioned — it is
the tooling half that makes the language half humane. Without it, deep imports + intrinsics-under-
`Core` will read as bureaucracy; with it, they read as clarity.

### 10. XL-010 — Tuples + multiple return *(High, ADOPT-NOW)*
Prior-adopted (A-named-tuples [Verified: SSOT:92]) and increasingly load-bearing: map iteration
wants `(k, v)` (FEATURES lists tuples/map-iteration as the open M-RT follow-up [Verified:
FEATURES.md:38]), `divmod`-style natives want two results without a ceremony class, and XL-032 list
patterns want a positional product to bind into. Scope discipline keeps it craftsmanship-clean:
positional only (named fields = use a class), no 1-tuples, destructure with `var (a, b) = f();` and
in match patterns. Implementation is the one genuinely non-trivial ADOPT-NOW: `Ty::Tuple(Vec<Ty>)`
in the checker, a value representation (candidate: reuse the `List` runtime rep with checker-level
arity — zero new `Value` variant, zero new Op — the Map precedent shows runtime-polymorphic reuse
works [Verified: M-RT S3]), transpiling to PHP `[$a, $b]` + `[$a, $b] = f()` which is *exactly*
what PHP devs already write, now typed.

### 11. XL-011 — `Printable` display protocol *(High, ADOPT-NOW)*
Today interpolation renders primitives only — `src/value.rs` documents the render as primitive-
scoped [Verified: value.rs:461] — so user types can't be printed without hand-built `describe()`
functions. The upgrade-not-removal move (philosophy tenet: keep the familiar concept, fix the
unsoundness): PHP's `__toString` concept, made explicit and checked. `interface Printable {
function toText(): string }` in `Core`; the checker requires it for a class-typed interpolation
hole (a *compile* error otherwise — today's behavior, but with a did-you-mean-implement hint);
backends call the method through existing dispatch. Transpiles to `__toString()` — the rare case
where PHP's magic surface is the perfectly idiomatic target for Phorj's explicit contract.
Composes with XL-002 (`derive(Show)` auto-implements it) and XL-001 (`{order}` calls `toText`,
`{order:>20}` pads the result).

### 12. XL-012 — let-else *(High, ADOPT-NOW)*
`var user = findUser(id) else { return Failure(NotFound()); }` — the flat early-exit spelling of
if-let, and the fix for its one weakness (rightward drift when a function unwraps three optionals
in sequence). Every ingredient is shipped: if-let binding, smart casts, and — critically — the
totality engine's `block_terminates`, which is exactly the check that the `else` block diverges
[Verified: totality cluster]. Parser desugar, front-end only, zero backend delta, trivial PHP
(`if (($x = e) === null) { …diverge…; }`). Prior-adopted with "strong" evidence
[Verified: SSOT:64]. Smallest item in the top-15; listed this high because cost is near-zero and
it multiplies the value of every optional-returning API in XL-007's wave.

### 13. XL-013 — Labeled loops *(DX-pure-win, ADOPT-NOW)*
PHP's `break 2;` is a live footgun: refactor a loop level and the count silently targets the wrong
loop. Labels (`search: for … { for … { break search; } }`) are the craftsmanship upgrade of the
*same concept* — and the compiler emits `break N` with N *computed*, so the transpiled PHP is
exactly the familiar idiom, minus the human error. Prior-adopted [Verified: SSOT:63]. Parser +
compiler jump bookkeeping (both loops' jump patching already exists); no new Op. The niche is
real-but-narrow (nested search/matrix code), which is why it ranks last of the ADOPT-NOWs, but the
cost is a day-scale slice.

### 14. XL-014 — Structured concurrency *(High, ADOPT-LATER — into M-Parallel)*
Green threads shipped the primitives (spawn/receive/join); structured concurrency is the discipline
that makes them safe at scale: a `scope { … }` block that joins all children at exit and propagates
the first error — tasks can't leak, lifetimes are lexical, cancellation has a shape. Kotlin, Swift,
and Trio all converged here after watching unstructured task-spawning rot codebases; adopting it
*before* Phorj accumulates unstructured `spawn` idioms is cheaper than retrofitting (the exact
lesson of Kotlin's GlobalScope regret). It belongs inside the M-Parallel deep plan the developer
already commissioned [Verified: no-wind spec §5 handoff] — flagged here so the plan treats scoping
as a first-class requirement, not an afterthought: the actor-model decision changes *how* scopes
nest, but every candidate model needs them. Concurrency sits on the quarantine seam outside the
PHP oracle, so spine risk is contained by construction.

### 15. XL-015 — `select` over channels *(High, ADOPT-LATER — into M-Parallel)*
The known missing half of the CSP model Phorj chose: with only blocking `receive`, any task that
must watch two channels (request + shutdown; data + timeout) can't be written without busy-polling.
Go's `select` is the canonical answer and its absence is the first wall real server code (M6's own
domain) will hit. The scheduler already parks and wakes tasks; select is "park on many, wake on
first" plus a *determinism policy* — and that policy is the actual design work: Go randomizes
ready-case choice, which Phorj must NOT copy (spine + refuses-to-lie); declared priority order
(first-listed ready case wins) is deterministic, teachable, and sufficient. Pairs with XL-016
deadlines, which then subsume ad-hoc timeout arguments across the async API. Sequence into
M-Parallel behind XL-014's scopes.

---

## 3. Summary counts

| Tier | Count |
|---|---|
| ADOPT-NOW | 13 (XL-001 … XL-013) |
| ADOPT-LATER | 27 (XL-014 … XL-040) |
| REJECT | 26 (XL-041 … XL-066) |
| **Total candidates** | **66** (+11 excluded as "PHP already has it") |

**Cross-cutting observations:**
1. **The ADOPT-NOW set is almost entirely front-end.** 11 of 13 need no new Op and no Value change
   (tuples may need a rep decision; format specs desugar to natives). The architecture's
   "expand-before-backends" discipline is the enabler — Phorj is unusually cheap to grow right now.
2. **Three items form a mutually-reinforcing DX cluster** — XL-004 fix + XL-009 auto-import +
   XL-024 deprecation-codemod — and together they de-risk the developer's own pending breaking
   changes (namespace-v2, future renames). Tooling that applies migrations is the difference
   between "breaking codemod milestone" being a dread or a command.
3. **The derive channel (XL-002) is the keystone**: Show feeds XL-011, Equals/Hash feed user-typed
   Map/Set keys, Json feeds XL-028 — one closed mechanism, four payoffs.
4. **Concurrency candidates all funnel into M-Parallel** rather than standing alone — this report
   deliberately feeds requirements into that commissioned plan instead of pre-empting it.
