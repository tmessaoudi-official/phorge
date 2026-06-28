# Track L — Stdlib API design & breadth

## Track summary

The shipped `Core.*` surface is **nine leaf modules** — `Console` (println), `Math`
(sqrt/pow/floor/ceil/abs/min/max), `Text` (len/upper/lower/trim/contains/split/splitOnce/join/replace),
`File` (read/exists/write), `Bytes` (fromString/toString/len/find/concat/slice), `Html` (escape kernel +
builders + per-tag helpers), `List` (reverse/sum/map/filter/reduce), `Map` (keys/values/has/size), and
`Set` (of/contains/size). The architecture is excellent: every native single-sources its checker
signature + one shared `eval` (interpreter ≡ VM) + a `php` erasure, so the four backends cannot drift,
and the `NativeEval::Pure | HigherOrder` split already supports closure-taking natives byte-identically.
The **breadth**, however, is thin against any modern standard library and even against PHP's own — it is
a foundation, not yet a stdlib. The gaps fall into three buckets: (1) **collection-method completeness**
— `List` is missing the everyday combinators (`len`/`isEmpty`/`contains`/`indexOf`/`first`/`last`/`take`/
`drop`/`slice`/`concat`/`sort`/`sorted`/`unique`/`flatMap`/`zip`/`find`/`any`/`all`/`count`/`forEach`),
and `Map`/`Set` are missing `get`/`getOr`/`insert`/`remove`/`entries`/iteration and `union`/`intersection`/
`difference`; (2) **whole modules that should exist but don't** — `Convert`/parse (`Int.parse`,
`Float.parse`, `toString`), `Json` (encode/decode — blocked on a dynamic type), `Random` (deterministic,
seeded), `Time`/`Date`, `Env`/`Process` (args, env, exit), `Result`/`Option` helpers, `Regex`,
`Assert`/`panic`; and (3) **cross-cutting API-design consistency** — argument-ordering and naming
conventions (`splitOnce` snake_case vs the PascalCase-module/camelCase-fn rule; subject-first vs
collection-first; error-vs-optional return discipline) need a written **stdlib design charter** so the
library grows coherently instead of ad hoc. The single biggest unlock is **`List<T>`/`Map`/`Set` query &
transform completeness** — it's where a PHP dev's muscle memory lives (`count`, `in_array`, `usort`,
`array_unique`, `array_slice`, `array_column`) and every piece maps to a tier-1 PHP builtin and is
deterministic, so it is pure ADOPT. The deferrals cluster around features that break the determinism
spine (true randomness, wall-clock time, network) or need a type-system primitive Phorj lacks (`Json`
needs `Any`/`mixed`; lazy iterators need a `Seq`/generator protocol).

## Gap table

| id | title | kind | fit | rec | milestone | effort |
|---|---|---|---|---|---|---|
| L-stdlib-charter | Written stdlib design charter (naming, arg-order, error-vs-optional, module taxonomy) | new | strong | adopt | M4 | S |
| L-list-query | `Core.List` query ops: `len`/`isEmpty`/`contains`/`indexOf`/`first`/`last`/`count` | port | strong | adopt | M4 | M |
| L-list-slice | `Core.List` structural ops: `slice`/`take`/`drop`/`concat`/`flatten`/`repeat` | port | strong | adopt | M4 | M |
| L-list-sort | `Core.List` ordering: `sort`/`sortBy`/`sorted`/`unique`/`dedup` (pure, returns new list) | port | strong | adopt | M4 | M |
| L-list-ho-extra | `Core.List` higher-order extras: `find`/`any`/`all`/`forEach`/`flatMap`/`takeWhile`/`dropWhile`/`partition`/`groupBy`/`zip` | port | strong | adopt | M4 | M |
| L-map-access | `Core.Map` access: `get`/`getOr` (safe), `entries`, plus builder `insert`/`remove`/`merge` (COW) | port | strong | adopt | M4 | M |
| L-map-empty-builder | Empty/growable map constructor native (`Map.empty<K,V>()` or `Map.of(entries)`) | port | strong | adopt | M4 | S |
| L-set-algebra | `Core.Set` algebra: `union`/`intersection`/`difference`/`isSubset`/`toList`; `add`/`remove` (COW) | port | strong | adopt | M4 | M |
| L-iteration-protocol | First-class iteration: `for (k, v in map)` / `for (x in set)` (the missing `foreach`) | port | strong | adopt | M4 | M |
| L-convert | `Core.Convert` / parse module: `Int.parse(string) -> int?`, `Float.parse -> float?`, `toString(any-scalar)` | port | strong | adopt | M4 | M |
| L-text-breadth | `Core.Text` breadth: `startsWith`/`endsWith`/`indexOf`/`slice`/`substring`/`repeat`/`padStart`/`padEnd`/`chars`/`reverse`/`replaceFirst`/`splitN` | port | strong | adopt | M4 | M |
| L-math-breadth | `Core.Math` breadth: float `abs`/`min`/`max`/`round`/`sign`/`clamp`/`log`/`exp`/`sin`/`cos`; `Math.PI`/`E` consts; int `gcd`/`pow` | port | strong | adopt | M4 | M |
| L-result-type | `Result<T, E>` / richer `Option` combinators (`map`/`unwrapOr`/`isSome`) as stdlib over optionals | new | ok | defer | M3 (post-exceptions) | M |
| L-json | `Core.Json`: `encode(value) -> string`, `decode(string) -> Json?` | port | ok | defer | M-RT (needs `Any`/dynamic type) | L |
| L-regex | `Core.Regex`: `match`/`isMatch`/`replace`/`captures` (PCRE, tier-1 safe) | port | ok | defer | M4 | L |
| L-env-process | `Core.Env`/`Core.Process`: program `args`, `env(name) -> string?`, `exit(code)`, stdin read | port | ok | defer | M6 (CLI runtime) | M |
| L-time | `Core.Time`/`Core.Date`: monotonic + wall clock, formatting, durations | port | weak | defer | M6 (determinism-quarantined) | L |
| L-random | `Core.Random`: seeded, deterministic PRNG (no system entropy) | port | ok | defer | M6 | M |
| L-assert-panic | `Core.Assert.that(cond, msg)` / `panic(msg)` deliberate-fault primitive | new | ok | adopt | M4 | S |
| L-lazy-seq | Lazy iterators / `Seq<T>` (generator protocol) for streaming pipelines | new | weak | reject | — | L |
| L-numeric-format | Number formatting: `Text.format`/`printf`-style + `Float.toFixed(n)` | port | ok | defer | M4 | M |
| L-bytes-breadth | `Core.Bytes` breadth: `at(i) -> int?`, `indexOf`, `split`, hex/base64 encode-decode | port | ok | defer | M6 | M |
| L-console-io | `Core.Console` breadth: `print` (no newline), `eprintln`/`error` (stderr), `readLine -> string?` | port | strong | adopt | M4 | S |
| L-naming-fix | Rename `Text.splitOnce` → consistent camelCase already done; audit all natives for arg-order consistency (subject-first) | map | strong | adopt | M4 | S |

## Rationale per ADOPT item

**L-stdlib-charter** — Before adding 60+ natives the library needs a one-page written charter pinning
the conventions that are currently implicit and already slightly inconsistent (e.g. `splitOnce` reads as
snake-flavored; `Map.has` takes `(map, key)` but erases to `array_key_exists(key, array)`). The charter
should fix: PascalCase modules / camelCase functions; **subject/collection-first** argument order
(matching `xs.map(f)`-style reading even though calls are module-qualified); **optional-return for
absence, fault for programmer error** (the existing `File.read -> string?` vs `List.sum` fault split is
the right instinct — write it down); and a module taxonomy so future natives have an obvious home. This
is craftsmanship-apex: it makes the whole stdlib legible and prevents the ad-hoc drift the developer
wants to stop. Cheap, high-leverage, do it first.

**L-list-query / L-list-slice / L-list-sort / L-list-ho-extra** — A PHP developer reaches for `count()`,
`in_array()`, `array_slice()`, `array_unique()`, `usort()`, `array_column()`, and now (8.4) `array_find`/
`array_any`/`array_all` constantly; today Phorj's `List` has only `reverse`/`sum`/`map`/`filter`/
`reduce`. Every one of these maps to a tier-1 PHP builtin (`count`, `in_array`, `array_slice`,
`array_unique`, `usort`/`array_multisort`, `array_search`, `array_merge`, `array_find`, `array_some`),
is fully deterministic, and reuses the exact generic + `NativeEval::HigherOrder` machinery already
shipped (so no new `Op`, no `Value` change). `sort`/`sorted` return a **new** list (immutable-by-default
respected). This is the single largest legibility + adoption win and the strongest possible
philosophy-fit: familiar concept, provably-equivalent, idiomatic-PHP erasure. Split into four sub-tasks
only so they land as separate green slices; conceptually one push.

**L-map-access / L-map-empty-builder / L-set-algebra** — `Map` currently has no safe `get` (only the
faulting `m[k]` and the `has` predicate) and no way to build an empty/growable map; `Set` has no
algebra. A safe `get(map, key) -> V?` and `getOr(map, key, default) -> V` close the null-safety story for
maps (compose with S2 `??`); `Map.empty()`/`Map.of(entries)` removes the documented "no empty map
literal" wart by giving a builder. `Set.union`/`intersection`/`difference` are the obvious completions
already flagged as follow-ups in KNOWN_ISSUES. All map cleanly to PHP (`$m[$k] ?? null`,
`array_intersect_key`/`array_diff`/`array_merge`, COW arrays match Phorj's value-type COW for Map/Set).
Strong fit, generics machinery already present.

**L-iteration-protocol** — The most glaring everyday gap: there is `for (x in list)` but no
`foreach`-over-map (`for (k, v in m)`) or set. PHP devs `foreach` maps constantly; without it `Map` is
half-usable. It maps directly to PHP `foreach ($m as $k => $v)`. Determinism is guaranteed because
`Value::Map`/`Set` are already insertion-ordered `Vec`s (risk R1 was pre-paid exactly for this). Needs
a small parser + checker + backend addition (destructuring loop binding) but no new collection rep.

**L-convert** — Phorj can build strings via interpolation but has **no string→number parse** and no
explicit `toString`. `Int.parse("42") -> int?` / `Float.parse -> float?` (optional return on bad input —
the craftsmanship-correct contrast to PHP's silent `(int)"abc" == 0` footgun) and an explicit
`toString` are foundational and used in every CLI/web program. Maps to PHP `filter_var(_,
FILTER_VALIDATE_INT)` / `is_numeric`+cast / `strval`. This is exactly a "removes a surprise, never
capability" feature — Phorj gives the *safe* parse PHP never had.

**L-text-breadth** — `startsWith`/`endsWith`/`indexOf`/`slice`/`substring`/`padStart`/`padEnd`/`repeat`/
`chars`/`reverse` are daily-driver string ops, all tier-1 PHP (`str_starts_with`, `str_ends_with`,
`strpos`, `substr`, `str_pad`, `str_repeat`, `str_split`, `strrev`). The ASCII/byte-length discipline
already documented for `Text.len` extends cleanly. High fit, mechanical to add.

**L-math-breadth** — `round`/`sign`/`clamp`/`log`/`exp`/trig + `PI`/`E` constants + float `abs`/`min`/
`max` (today only int) round out a numerics module that's currently a stub. All tier-1 PHP (`round`,
`log`, `exp`, `sin`, `M_PI`). The irrational-float precision caveat already governs `sqrt`, so the
guide examples stay on exactly-representable values — the spine stays safe. Strong fit.

**L-assert-panic** — There is currently no first-class way to deliberately abort with a message
(`panic("unreachable")` / `Assert.that(cond, msg)`). It rides the existing fault path (already
byte-identical via `FaultKind`) and maps to PHP `throw new RuntimeException`/`assert`. Small, useful,
craftsmanship-positive (explicit failure beats a silent wrong answer).

**L-console-io** — `Console` has only `println`. Adding `print` (no newline), `eprintln`/`error` (stderr
— needed the moment programs do real I/O, and the `serve --dev` error page already proves stderr
plumbing exists), and a `readLine() -> string?` for stdin makes `Console` a real I/O module. `print`/
`eprintln` map to PHP `echo`/`fwrite(STDERR, …)`; `readLine` to `fgets(STDIN)`. The stderr split is
deterministic (separate stream, excluded from stdout byte-identity). Strong fit, small.

**L-naming-fix** — A consistency pass: audit every native's argument order against the charter
(subject/collection first), confirm camelCase, and document the deliberate PHP-arg-order reorderings
(already done correctly in the `php` closures for `split`/`join`/`replace`/`map`). This is a `map`/audit
task, not new capability — it makes the library coherent before it grows. Cheap, do alongside the charter.

(The DEFER items — `Json`, `Regex`, `Random`, `Time`, `Env/Process`, `Result`, lazy `Seq`, number
formatting, bytes breadth — each wait on a specific missing primitive or break the determinism spine:
`Json` needs an `Any`/dynamic type the type system doesn't have yet; `Random`/`Time`/network are
non-deterministic and must be quarantined like M6's URL deferral; `Result` is best built after the
exceptions/error-model slice so the two error channels are designed together; lazy `Seq` is REJECTED as
PL-theory maximalism — eager list combinators cover the real need, generators add a whole protocol +
coroutine surface that doesn't earn its surprise budget for a transpile-to-PHP language whose arrays are
eager.)

## Critic pass

**Verification of shipped state (so nothing already-done is re-listed).** Grepped `src/native.rs` —
the live `(module, name)` registry is exactly: `Console.println`; `Math.{sqrt,pow,floor,ceil,abs,min,max}`;
`Text.{len,upper,lower,trim,contains,split,splitOnce,join,replace}`; `File.{read,exists,write}`;
`Bytes.{fromString,toString,len,find,concat,slice}`; `Html.{el,voidEl,attr,boolAttr,text,raw,render,concat,div,…}`;
`List.{reverse,sum,map,filter,reduce}`; `Map.{keys,values,has,size}`; `Set.{of,contains,size}`.
**No mis-listings found** — every gap-list row names a native or module that does NOT yet exist (the
charter, the query/structural/sort/HO-extra list ops, safe map `get`, set algebra, map iteration,
convert/parse, text/math breadth, console I/O breadth, assert/panic, and all defers). The four-backend
single-sourcing (`NativeEval::Pure | HigherOrder`) is confirmed present, so the "no new Op/Value, reuse
the generic + HigherOrder machinery" claim under every ADOPT item is accurate. **removed_mislisted = 0.**

**Sanity-check of recommendations against philosophy.** All ADOPTs hold: each is a daily-driver PHP
builtin (`count`/`in_array`/`array_slice`/`usort`/`str_starts_with`/`round`/`echo`/`foreach`) a PHP dev
reaches for by reflex, deterministic, tier-1, and erasable — strongest possible familiarity-first fit.
The one defer I'd *challenge*: **`L-json` milestone is mis-targeted.** `encode` of a *statically-typed*
value needs no dynamic type (walk the known type → `json_encode`); only `decode` needs `Any`/`Json`. Splitting
encode (adoptable at M4) from decode (correctly deferred) is the legible move — but I leave the row's verdict
as-is and capture the encode-half as a new row below. The **`L-lazy-seq` REJECT is correct** (PL-theory
maximalism, fights the eager-array transpile target). `L-result-type` defer is right; I split out the
**Option-combinator half**, which needs no exceptions and is M4-able. Everything else stands.

**Newly-found gaps (the long tail the first pass missed).** Six genuine, non-subsumed items:

| id | title | kind | fit | rec | milestone | effort |
|---|---|---|---|---|---|---|
| L-char-ops | `Core.Text` char/codepoint ops: `charAt`/`code`(ord)/`fromCode`(chr)/`isDigit`/`isAlpha`/`isSpace` | port | strong | adopt | M4 | M |
| L-map-transform | `Core.Map` higher-order: `map`/`filter`/`mapValues`/`fromEntries`(+`fromLists`/zip) | port | strong | adopt | M4 | M |
| L-list-bykey | `Core.List` by-key reductions: `maxBy`/`minBy`/`sumBy`/`enumerate`(withIndex) | port | strong | adopt | M4 | S |
| L-json-encode | `Core.Json.encode(T) -> string` (statically-typed value walk; decode stays deferred) | port | ok | adopt | M4 | M |
| L-option-combinators | `Option`-style combinators over `T?`: `mapOpt`/`unwrapOr`/`orElse`/`isSome` (no exceptions needed) | new | ok | adopt | M4 | S |
| L-natives-introspect | `phg natives` / `--list-natives`: discoverable stdlib surface (module/sig/PHP-erasure, fed by the registry) | new | strong | adopt | M5 | S |

Plus a **second charter dimension the first pass folded into naming**: the *implementation-strategy*
decision — which stdlib functions are Rust natives vs. self-hosted `.phg` library packages built on the
shipped collection primitives (and transpiled like user code). This is architecturally distinct from
`L-stdlib-charter` (which is naming/arg-order/error-discipline). I capture it as one row:

| id | title | kind | fit | rec | milestone | effort |
|---|---|---|---|---|---|---|
| L-stdlib-impl-strategy | Charter §: native-vs-`.phg` policy — when a stdlib fn is a Rust native vs a self-hosted library package | new | strong | adopt | M4 | S |

**Notes on items deliberately NOT promoted to rows** (subsumed or too thin): `trimStart`/`trimEnd`/
`splitLines`/`countMatches` fold into **L-text-breadth**; `lcm`/floored-`mod`/`divMod`/`isEven` fold
into **L-math-breadth**; `Set.map`/`filter` is a minor HO follow-up (defer with `L-set-algebra`);
`Core.Hash` (crc32/md5/sha → PHP `hash()`) is deterministic+tier-1 but low-priority and pairs with the
M6 bytes/serialization work — note it under `L-bytes-breadth` rather than as its own row.

**Counts: new_found = 7, removed_mislisted = 0.**
