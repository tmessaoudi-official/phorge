# M — Bidirectional Gap Matrix: PHP 8.5 ↔ Phorj

> **Agent M output — full-audit fleet, Stage 2.** Inputs: `D-php-surface.md` (869 rows: 173 SYN,
> 631 FN, 20 RT, 45 DEF) and `E-phorj-surface.md` (code-verified at `ccb2403`). Where a verdict
> needed confirmation beyond E, it was checked against source or the built binary
> (`phg 0.5.1-alpha.1`) — those rows carry `[Verified: …]`. Roadmap citations refer to
> `ROADMAP.md` and `docs/specs/2026-06-21-php-parity-and-beyond.md` (the parity SSOT, "§3 rollup"
> / "§5 reject-list").
>
> **Verdict key:** `CB` COVERED-BETTER · `CE` COVERED-EQUAL · `P` PARTIAL · `GP` GAP-planned ·
> `GU` GAP-unplanned · `GD` GAP-by-design · `N/A` meaningless in Phorj's model.
>
> **Spot-verifications run for this matrix** (binary `target/release/phg`):
> lambda keyword is `function(int x) => e` (`fn` is retired — E's PJ-SYN-005 wording is stale);
> `String.length("héllo")` = **6 (bytes)**; trailing comma in calls accepted; top-level `const`
> rejected; `catch (A | B e)` parses (union multi-catch); `s[i]` string indexing rejected;
> `break 2` rejected; `int...` variadics rejected; float `==` type-checks (no lint yet).

---

## PASS 1 — PHP → Phorj (can Phorj do everything PHP does?)

### 1.1 SYN rows (173, one verdict each)

| ID | Verdict | Phorj counterpart / justification |
|---|---|---|
| SYN-001 | N/A | Phorj is not an HTML-embedded template language; typed `html"…"` (PJ-KW-004) is the deliberate alternative |
| SYN-002 | N/A | same |
| SYN-003 | N/A | same |
| SYN-004 | CE | `Output.print/printLine` (PJ-NAT-OUTPUT); interpolation replaces multi-arg echo |
| SYN-005 | CE | `;` + `{}` blocks (PJ-SYN-002) |
| SYN-006 | P | `//` and `/*…*/` [Inferred: fmt comment side-channel]; `#` line comment absent (bare `#` not a token, PJ-KW-003); `#[` attributes match PHP |
| SYN-007 | CB | always-strict; no per-file coercion switch (removes the DEF-019 config-changes-semantics class) |
| SYN-008 | N/A | no tick model |
| SYN-009 | N/A | UTF-8 source only |
| SYN-010 | CB | static `package`/`import` model (PJ-SYN-010, PJ-PROJ-002); no runtime file inclusion (fixes DEF-020) |
| SYN-011 | CB | same |
| SYN-012 | N/A | no include |
| SYN-013 | GD | closed no-eval language (§5: E-importer-stageC rationale — "dynamic PHP is un-importable into a closed no-`eval` language") |
| SYN-014 | P | `main(List<string> args) -> int` exit codes + `panic()` abort; no arbitrary mid-program `exit(status)` |
| SYN-015 | GD | no shell-exec operator (determinism spine + injection class) |
| SYN-016 | GU | `#` not a token; `phg build` binaries are the scripting answer (minor) |
| SYN-017 | N/A | data-after-code done properly: versioned CRC `.phorj` section container (PJ-CLI `build`) |
| SYN-018 | CB | sigil-free, uniformly case-sensitive + enforced casing `E-NAME-CASE`/`E-TYPE-CASE` (fixes DEF-018) |
| SYN-019 | GD | §5 reject A-compact-extract (`$$x`) |
| SYN-020 | CB | typed first-class function values (PJ-SYN-005) replace string dispatch; dynamic `$o->$m()` deliberately absent |
| SYN-021 | GD | static construction only (`E-NEW-REQUIRED`); Core.Reflection is read-only introspection |
| SYN-022 | GD | §5 reject A-func-static (fixes DEF-010) |
| SYN-023 | GD | same reject; class `static` fields cover persistent state |
| SYN-024 | P | class constants only (PJ-OOP-001) [Verified: top-level `const MAX = 10;` is a parse error] |
| SYN-025 | GD | constants/config must be compile-time (memory: config-must-be-compile-time; fixes DEF-019) |
| SYN-026 | GU | no `__LINE__`/`__FILE__` surface (spans exist internally only) |
| SYN-027 | CB | `Environment.get/all`, `Process.arguments`, Http `Request` object — read-only, typed (fixes DEF-010/DEF-045) |
| SYN-028 | N/A | no globals to restrict |
| SYN-029 | N/A | no http stream wrapper |
| SYN-030 | P | `Math.pi/e/infinity/nan`; no `PHP_INT_MAX`/`PHP_EOL`/OS-constant surface |
| SYN-031 | CB | `0x`/`0b`/`0o` + `_` (PJ-KW-005); legacy silent-octal `0755` removed |
| SYN-032 | CB | floats + exponents, plus `decimal` `19.99d`; int overflow is a checked fault, not silent float promotion (fixes DEF-022) |
| SYN-033 | CE | raw strings `r"…"`/`r#"…"#` (PJ-KW-004) |
| SYN-034 | CE | `\n \t \u{…}` escapes (PJ-KW-004) |
| SYN-035 | CB | one uniform `{expr}` interpolation, span-tracked; no unquoted-key quirks |
| SYN-036 | CE | `{expr}` |
| SYN-037 | N/A | never existed |
| SYN-038 | CE | `"""…"""` text blocks with JEP-378 auto-dedent (PJ-KW-004) |
| SYN-039 | CE | raw strings |
| SYN-040 | CB | `[…]` List, `[k => v]` Map, `Set` — distinct typed collections (fixes DEF-005) |
| SYN-041 | CE | case-sensitive keywords |
| SYN-042 | CB | one typed closure value; no string/array callables (PJ-SYN-005) |
| SYN-043 | CB | checked `x as T` → `T?` + explicit `Core.Conversion` matrix; `discard` = `(void)` cast (fixes DEF-041) |
| SYN-044 | CE | 17 built-in type words + unions/intersections/optionals/function types (PJ-TY-003); `mixed`/`callable` absent by design; `iterable` — see SYN-162 |
| SYN-045 | CB | no implicit coercion at all (fixes DEF-004) |
| SYN-046 | CB | explicit `String.parseInt/parseFloat -> T?` |
| SYN-047 | CB | typed Map keys, `"1"` ≠ `1` (`E-MAP-KEY`; fixes DEF-028) |
| SYN-048 | CB | typed handles (Channel/Task/Regex/Secret classes), no opaque resources (PJ-RT-001) |
| SYN-049 | CB | checked overflow/div-zero faults (PJ-RT-003); `**`; `Math.integerDivide` |
| SYN-050 | CE | statement-only `x++`/`x--` (PJ-SYN-002); no string-increment quirk (fixes DEF-029) |
| SYN-051 | CE | string concat via `+`/`Op::Concat` [Inferred]; no precedence trap |
| SYN-052 | P | `+= -= *= /= %= ??=`; no `**=`, no bitwise compounds `&= \|= ^= <<= >>=` |
| SYN-053 | CB | single typed `==` — no loose/strict split (fixes DEF-004); `<=>` absent (`List.sortWith` covers; J-spaceship in §3 M-RT rollup) |
| SYN-054 | CB | `&& \|\| !` only; low-precedence `and/or/xor` assignment trap removed |
| SYN-055 | CE | `& \| ^ ~ << >>` (int-only; `bytes` type replaces string byte-ops) |
| SYN-056 | CB | expression-`if` with mandatory `else`; no nested/short-ternary traps (fixes DEF-017) |
| SYN-057 | CE | `??`/`??=` on typed optionals |
| SYN-058 | CE | `?.` |
| SYN-059 | GD | §5/GA-M12 V-error-suppression-stance (fixes DEF-007) |
| SYN-060 | CB | `instanceof` with smart-cast flow narrowing (PJ-SYN-007); no dynamic string RHS |
| SYN-061 | CB | explicit `Map.merge`/`List.concat` semantics (fixes DEF-026) |
| SYN-062 | CE | `x \|> f` shipped in M3 S3 — pre-dates PHP 8.5's pipe |
| SYN-063 | GP | A-variadics/spread (§3 M-RT ergonomics) [Verified: `int...` parse error] |
| SYN-064 | P | `List.concat`/`flatten` cover; no literal `...` splat |
| SYN-065 | GD | §5 reject A-references (fixes DEF-006) |
| SYN-066 | CE | `obj with { f = v }` (PJ-SYN-004 CloneWith) — Phorj shipped 8.5's clone-with first |
| SYN-067 | P | `s[i]` rejected [Verified: "type `string` cannot be indexed"]; `String.substring` + for-in over string cover |
| SYN-068 | CE | `Index` on arbitrary expressions (PJ-SYN-004) [Inferred] |
| SYN-069 | CE | `if/else if/else`; no alternative endif syntax (by design) |
| SYN-070 | CE | `while` + `do…while` (parser/stmts.rs:659) |
| SYN-071 | CE | `CFor` `for(;;)` (PJ-SYN-002) |
| SYN-072 | CB | for-in/`foreach (e as x)` over list/range/string/Map two-binding; no by-ref dangling-reference pitfall (fixes DEF-006) |
| SYN-073 | GD | §5 reject A-switch (fall-through footgun); exhaustive `match` covers (fixes DEF-027) |
| SYN-074 | CB | compile-time exhaustiveness + payload/type/struct patterns + guards (PJ-SYN-006) vs PHP's value-only runtime `UnhandledMatchError` |
| SYN-075 | P | single-level only [Verified: `break 2` parse error]; B-labeled-break in §3 M-RT ergonomics → the labelled part is GAP-planned |
| SYN-076 | GD | no `goto` |
| SYN-077 | CB | totality-checked (`E-MISSING-RETURN`) |
| SYN-078 | P | `throw` is a statement (PJ-SYN-002); no throw-in-`??`/ternary expression form |
| SYN-079 | CB | compile-time checked `throws` + multi-catch via union type [Verified: `catch (E1 \| E1)` parses, dies on E-UNION-ARITY dup only] + `finally` |
| SYN-080 | GP | generators/`yield` — marathon A2 (memory: session-naming-and-b1 "NEXT=A2"); `yield` currently only PHP-reserved-guarded |
| SYN-081 | CB | uncolored green threads: `spawn`/`Channel`/`Task.join`, deterministic identical scheduling on both backends (PJ-SYN-009, PJ-RT-004) |
| SYN-082 | CB | case-sensitive + real overloading (PJ-OOP-002 — PHP has none); no conditional/nested declaration (by design) |
| SYN-083 | P | literal defaults on free functions only (`E-DEFAULT-PARAM-CONTEXT` bans method/ctor defaults; no `new` in defaults) |
| SYN-084 | CB | types mandatory and always enforced — no strict_types gate |
| SYN-085 | CB | `T?` with compile-time strictNullChecks (PJ-SYN-007); PHP nullable is runtime-only |
| SYN-086 | CB | unions + exhaustive match-over-union + narrowing (PJ-TY-006) — PHP unions have no narrowing |
| SYN-087 | CE | `A & B` (PJ-TY-006, transpiles to PHP 8.1 `A&B`) |
| SYN-088 | CE | `A \| B & C` — `&` binds tighter (PJ-TY-002), DNF expressible |
| SYN-089 | N/A | literal types are PL nicety; `bool`/`T?` cover the use |
| SYN-090 | P | `void` ✓, `never` ✓, `empty` extra (PJ-TY-004); `mixed` absent by design; `static`/`self` return types absent |
| SYN-091 | GP | A-variadics (§3) [Verified: parse error] |
| SYN-092 | GP | A-named-args (§3 M-RT ergonomics) |
| SYN-093 | GD | §5 reject A-references |
| SYN-094 | CE | `function(int x) -> T { … }` lambdas, by-value capture (simpler than `use()`) [Verified] |
| SYN-095 | CE | `function(int x) => e` expression body [Verified: runs; `fn` keyword retired — E PJ-SYN-005 wording stale] |
| SYN-096 | CB | a bare named-fn reference *is* a value — no `(...)` syntax needed |
| SYN-097 | GD | no `this`-rebinding (`bind`/`bindTo`/`call` are the dynamic-scope footgun class Phorj removes) |
| SYN-098 | CE | [Verified: `f(1, 2,)` runs] |
| SYN-099 | N/A | `T?` explicit from day one |
| SYN-100 | CB | must-use is the **default** (`E-UNUSED-VALUE`) + `discard`; PHP's `#[NoDiscard]` is opt-in per function |
| SYN-101 | CB | module loader resolves functions statically (fixes DEF-032) |
| SYN-102 | CE | mandatory `new` (`E-NEW-REQUIRED`); dynamic-expression class names absent by design |
| SYN-103 | CB | definite-assignment analysis (`E-FIELD-UNINITIALIZED`) — no runtime "uninitialized typed property" state |
| SYN-104 | CE | static fields/methods + visibility (PJ-OOP-001) |
| SYN-105 | CB | dynamic properties never existed; unknown field = compile error |
| SYN-106 | CB | immutable-by-default, `mutable` opt-in — the correct default; PHP had to bolt `readonly` on |
| SYN-107 | GP | A-asym-vis (§3 M-RT class surface) |
| SYN-108 | CE | property hooks `T name { get => e; set(v) {…} }` (PJ-OOP-001) |
| SYN-109 | CE | `constructor(public T x)` promotion |
| SYN-110 | CE | class consts: literal init, visibility, `E-CONST-*`; no `final const`/dynamic fetch (latter by design) |
| SYN-111 | P | explicit `parent.m()`/`parent(A).m()` (PJ-OOP-001); late static binding absent (A-lsb, §3) |
| SYN-112 | CB | **multiple** inheritance with explicit conflict errors (`E-MI-*`) + final-by-default `open` opt-in + `abstract` |
| SYN-113 | P | overrides checked but invariant (`E-OVERRIDE-SIG`); no co/contravariance |
| SYN-114 | CE | interfaces: multi-`extends`, nominal subtyping, `instanceof` RHS (PJ-OOP-003) |
| SYN-115 | CE | traits with methods/state/ctors/abstract-reqs/hooks + `use/rename/exclude`; conflicts are errors (arguably better than `insteadof`) |
| SYN-116 | CB | payload variants + generic enums (`Option<T>`/`Result<T,E>`) — PHP enums carry no payload; backed-enum sugar (`->value`/`cases()`) GAP-planned (A-backed-enums, §3) |
| SYN-117 | GU | no anonymous classes |
| SYN-118 | P | `#[Route]` only; no user-defined attributes (`E-UNKNOWN-ATTRIBUTE`) |
| SYN-119 | P | the *capabilities* are language defaults (Override→`E-OVERRIDE-SIG` always-on; NoDiscard→`E-UNUSED-VALUE` default; SensitiveParameter→`Secret<T>` typed); userland `#[Deprecated]` absent |
| SYN-120 | P | `Reflection.className/typeName` (PJ-NAT-REFLECTION) |
| SYN-121 | CB | explicit value(COW)/handle split + single structural `==` (mutation milestone; fixes DEF-024's invisibility) |
| SYN-122 | GU | lazy objects = DI-framework machinery |
| SYN-123 | GU | no weak refs (Rc model, no tracing GC) |
| SYN-124 | GP | A-magic-stringable (§3 M-RT class surface) |
| SYN-125 | CE | `constructor` |
| SYN-126 | GD | §5 reject A-destruct (Rc/Drop has no deterministic finalization; also fixes DEF-035) |
| SYN-127 | GD | §5 reject A-magic-dynamic |
| SYN-128 | GD | same |
| SYN-129 | GD | same; property hooks cover computed properties |
| SYN-130 | GD | same; `set` hooks |
| SYN-131 | GD | §5 reject A-isset-empty |
| SYN-132 | GD | same |
| SYN-133 | N/A | no native object serialization (legacy hook) |
| SYN-134 | N/A | same |
| SYN-135 | GD | no serialize mechanism at all (fixes DEF-013 RCE class); `Core.Json` is the data path |
| SYN-136 | GD | same |
| SYN-137 | GP | A-magic-stringable (§3) |
| SYN-138 | GP | A-magic-invoke (§3 M-RT class surface) |
| SYN-139 | N/A | no `var_export` |
| SYN-140 | N/A | `with { }` clone-update covers the use without a hook |
| SYN-141 | GU | `inspect::render` exists but no user hook |
| SYN-142 | CB | mandatory packages, folder=path enforced (`E-PKG-PATH`), PascalCase (`E-PKG-CASE`) |
| SYN-143 | CE | `import` + `as` + `import type`; no group-use/wildcard (by design — PHP has no `use A\*` either for the type case) |
| SYN-144 | CB | fully static resolution, no runtime global fallback (fixes DEF-033) |
| SYN-145 | N/A | no namespace operator needed |
| SYN-146 | CB | whole-project loader, functions included (fixes DEF-032) |
| SYN-147 | CE | contextual-keyword machinery + `E-RESERVED-NAME` PHP-interop guard (PJ-KW-006) |
| SYN-148 | CB | three deliberate tiers: checked `throws` / `Result` / uncatchable faults (PJ-SYN-008) |
| SYN-149 | CB | 12 fault classes, byte-identical traces (PJ-RT-003); engine errors are deliberately uncatchable bugs (the anti-Java decision) |
| SYN-150 | P | user-defined `Error` classes; no shipped standard exception taxonomy |
| SYN-151 | N/A | no warning/exception split brain to bridge (fixes DEF-008) |
| SYN-152 | CB | compile-time E-/W- codes + `phg explain`; nothing runtime-configurable |
| SYN-153 | GD | faults crash with traces; `serve --dev` renders errors; no global mutable handlers |
| SYN-154 | CE | `panic()`/`todo()`/`unreachable()` intrinsics |
| SYN-155 | CB | `assert()` **never stripped**, byte-identical across profiles (PJ-CLI-003); PHP compiles asserts out in prod |
| SYN-156 | P | stack traces ✓ run≡runvm; cause-chain GAP-planned (A-fault-cause-chain, §3 M-faults) |
| SYN-157 | GU | no atexit/shutdown hooks |
| SYN-158 | CB | escape-from-main is a **compile** error (`E-UNCAUGHT-THROW`); runtime faults exit nonzero with trace |
| SYN-159 | CB | asymmetry made explicit & documented (List/Map/Set COW values, Instance handle) + immutable default |
| SYN-160 | GD | §5 reject A-isset-empty; optionals + explicit `isEmpty` (fixes DEF-014) |
| SYN-161 | CB | typed struct/list destructuring with mandatory diverging `else` on refutable patterns (fixes DEF-036) |
| SYN-162 | GP | A-iterators / J-iter-protocol (§3 M11); generators = marathon A2 |
| SYN-163 | CB | typed keys (fixes DEF-028) |
| SYN-164 | CB | uniformly case-sensitive + enforced conventions (fixes DEF-018) |
| SYN-165 | CB | `0o` only |
| SYN-166 | CB | zero runtime config (config-must-be-compile-time; fixes DEF-019) |
| SYN-167 | CB | `Response` value object; serialization is explicit (fixes DEF-042) |
| SYN-168 | N/A | persistent-process model (`phg serve`) is the deliberate opposite; PHP itself is migrating there (see RT-005) |
| SYN-169 | CB | Rc+COW, acyclic by construction, deterministic reclamation; cycle collector deferred to v2 only if mutation needs it |
| SYN-170 | CE | 14 contextual keywords (PJ-KW-002) |
| SYN-171 | N/A | D's own n/a marker |
| SYN-172 | P | methods have `this`; lambdas cannot capture `this` (`E-LAMBDA-THIS`; this-capture a documented deferral) |
| SYN-173 | P | `W-DEPRECATED` side-table for stdlib + DEPRECATION.md policy; userland deprecation attribute absent |

**SYN tally (173):** CB 56 · CE 37 · PARTIAL 20 · GAP-planned 9 · GAP-unplanned 7 ·
GAP-by-design 24 · N/A 20.
**SYN coverage** (COVERED=1, PARTIAL=0.5; N/A+GD excluded): (93 + 10) / 129 = **79.8%**.

### 1.2 FN groups (631 rows — compressed where a family is uniform, itemized where verdicts differ)

Per-group verdict distribution `C/P/GP/GU/GD/NA` (counts sum to the group's row count).
"C" merges COVERED-BETTER and COVERED-EQUAL; better-than-PHP cases are named.

| Group (rows) | C | P | GP | GU | GD | NA | Notes — what's covered, what's named-missing |
|---|--|--|--|--|--|--|---|
| **FN-STR** (93) | 30 | 6 | 9 | 35 | 3 | 10 | Covered: length/contains/starts/ends/indexOf(**int?** — fixes DEF-009)/lastIndexOf/substring/count/replace/repeat/case/capitalize/trim×3/split/join/reverse/pad×2/lines/parseInt-Float-Bool, plus md5/sha1/crc32→`Core.Hash`, htmlspecialchars→**`Core.Html` typed XSS-safe (CB)**, str_getcsv→`Core.Csv`, bin2hex/hex2bin→`Core.Encoding`, number_format→`Math.numberFormat`, strcasecmp→equalsIgnoreCase, strval→`Conversion.toString`. GP: **sprintf family (7 rows, A-sprintf §3 M11)**, chr/ord (M-codepoint-int, M-text). GD: setlocale/nl_langinfo/strcoll (locale — §5 no-ICU). GU highlights: str_split(fixed-chunk), ucwords, wordwrap, strtr, similar_text/soundex/metaphone/levenshtein, strtok/strpbrk/strspn, str_increment/decrement, strip_tags. N/A: addslashes×5 (no magic-quotes/SQL-string model), utf8_encode/money_format/hebrevc (removed/deprecated), crypt (→ Cryptography) |
| **FN-ARR** (74) | 26 | 12 | 2 | 22 | 2 | 10 | Covered: map/filter/reduce (**uniform subject-first order — fixes DEF-003**), keys/values/has, append/concat/merge, reverse, indexOf/find/contains (**typed-strict — fixes DEF-027**), slice/take/drop, sum/max/min, unique, chunk, fill, count(length/size), range(`a..b` syntax), first/last, any/all, sort/sortWith (**pure, returns — fixes DEF-025**), array_find/any/all/first/last (8.4/8.5 parity), list()→destructuring. P: pop/shift/splice (immutable idiom via slice), diff/intersect (Set.difference/intersection for sets only), asort/ksort family (sortWith composes), array_walk (map covers), SORT_* flags. GP: remaining L-list-breadth (zip — deferred B3). GD: compact/extract (§5 reject). N/A: internal-pointer family (current/key/next/reset/end — no array cursor), each (removed), array_is_list (types make it meaningless) |
| **FN-MATH** (37) | 17 | 3 | 11 | 4 | 0 | 2 | Covered: abs/ceil/floor/round(+`Core.Decimal` RoundingMode — **CB, fixes DEF-023**)/fmod(`%` on floats)/intdiv/pow/sqrt/exp/log/log10/pi/sin-cos-tan/max-min/isNaN-isFinite-isInfinite + rand/mt_rand→`Core.Random` (seeded deterministic). GP: asin/acos/atan/atan2/hyperbolics/hypot/deg2rad/log2/log1p/expm1 (G-math-breadth §3 M11), BigInt/GMP (N-bigint, M-NUM-2). P: constants row (pi/e only), BCMath (decimal covers money, not arbitrary precision), base-conversions (hex literals + Encoding partial). **GAP: random_int/random_bytes CSPRNG** (security-relevant — no crypto-safe source; Random is deliberately deterministic). N/A: lcg_value (deprecated) |
| **FN-PCRE** (11) | 4 | 2 | 0 | 4 | 0 | 1 | Covered: preg_match(matches/find/findGroups), match_all(findAll), replace, split (`Core.Regex`, feature-gated). GU: **preg_replace_callback**(+_array), preg_filter, preg_quote. P: preg_grep (filter+matches composes), modifier surface (regex-crate subset of `i m s x u`). N/A: last_error (typed API) |
| **FN-JSON** (6) | 3 | 0 | 0 | 1 | 0 | 2 | encode/decode/validate via parse→`Json?` + stringify(Pretty) — decode is **CB: typed `Json` enum, `Json.Null` ≠ error (fixes DEF-044)**. GU: JsonSerializable protocol. N/A: last_error/JsonException (optional-return model) |
| **FN-DATE** (27) | 5 | 5 | 2 | 8 | 2 | 5 | Covered: DateTimeImmutable→`Core.Time` Instant/Date (**immutable-only — fixes DEF-011**), DateInterval→Duration, time/microtime/hrtime→nowMilliseconds/monotonicNanos. GP: DateTimeZone/IANA tz + default-timezone (N-tz-iana, M-TIME-2). GD: mutable DateTime (deliberately absent), **strtotime (DWIM parser — fixes DEF-039)**. P: format chars (toIso only), mktime/checkdate (factories partial), date(). GU: DatePeriod, getdate/localtime/gettimeofday, date_parse, sun_info, sleep/usleep. N/A: strftime/strptime/sunrise (deprecated), ~26 procedural aliases (OO-only by design) |
| **FN-FS** (55) | 8 | 2 | 7 | 34 | 0 | 4 | Covered: file_get/put_contents→read(**string?**)/write/append, exists, size, copy, rename, delete, basename/dirname/pathinfo→`Core.Path`. GP: mkdir/rmdir/opendir/scandir/glob/chdir-getcwd/touch (G-dir/G-file-more, M-Batteries §3). GU: **the entire fopen stream-handle family (16 rows)**, stat/perms/chmod/chown/umask, symlinks, realpath, tempnam/tmpfile, fnmatch, disk_*, parse_ini, popen, chroot. N/A: move_uploaded_file, fgetss (removed), include-path, clearstatcache |
| **FN-HASH** (8) | 0 | 1 | 4 | 3 | 0 | 0 | P: `hash()` breadth = only crc32/md5/sha1/sha256. GP: **hash_hmac, hash_equals (timing-safe!), hkdf, pbkdf2** (G-crypto digests, §3 M8). GU: hash_file, streaming HashContext, hash_algos |
| **FN-CRYPT** (8) | 2 | 0 | 0 | 5 | 0 | 1 | Covered: password_hash/verify → `Core.Cryptography` (**CB: Argon2id default, no bcrypt legacy**). GU: needs_rehash/get_info/algos, **sodium (~110 fns), openssl (~60 fns)** — both families entirely absent. N/A: crypt (legacy DES/MD5) |
| **FN-DB** (10) | 0 | 0 | 10 | 0 | 0 | 0 | **Entire database surface absent.** GAP-planned: ROADMAP M6 "Postgres connectivity". No PDO/mysqli/SQLite equivalent exists today — the single largest migration blocker |
| **FN-CURL** (13) | 0 | 0 | 13 | 0 | 0 | 0 | **No HTTP client.** GAP-planned: M6 deferral (determinism gate) + post-M-DX audited developer question (plan `2026-07-01-post-m-dx-four-lane-backlog`); `phg vendor` is the only network-touching code today |
| **FN-MB** (22) | 0 | 2 | 12 | 4 | 0 | 4 | GP: the M-text programme (M-codepoint-len, M-unicode-case, M-grapheme, M-normalization §3) replaces the mb_* second-family model with one correct API — **note: `String.length` is byte-based today [Verified: "héllo" → 6]**. P: case ops (ASCII-correct today). GU: convert_kana, mimeheader, numericentity, detect_encoding. N/A: mb_ereg legacy family (Core.Regex is the answer), http_input/output plumbing |
| **FN-ICONV** (6) | 0 | 0 | 2 | 2 | 0 | 2 | GP: charset conversion contract (M-encoding-contract, M-text). GU: mime encode/decode. N/A: get/set_encoding globals |
| **FN-SPL** (39) | 2 | 2 | 4 | 26 | 0 | 5 | Covered: SplFixedArray→`[T; N]` (**CB: static literal-index bounds**), class_implements/parents→`Reflection.interfaces/parents`. P: SplStack/SplQueue (List covers). GP: Iterator/IteratorAggregate/Traversable protocol (A-iterators §3 M11) + directory iterators (M-Batteries). GU: heaps, priority queue, SplObjectStorage (no object Map keys — HKey is int/bool/string), ArrayAccess protocol, the 12-row iterator-decorator zoo (eager higher-order List fns cover most uses), SplFileInfo/Object family. N/A: autoload fns (no autoloading — superior model), Serializable (deprecated) |
| **FN-CTYPE** (11) | 4 | 0 | 0 | 7 | 0 | 0 | Covered: alnum/alpha/digit/xdigit → `Validation.isAlnum/isAlpha/isInt/isHex`. GU: cntrl/graph/lower/print/punct/space/upper |
| **FN-FILTER** (9) | 0 | 2 | 0 | 3 | 0 | 4 | P: filter_var validate subset (`Core.Validation` int/number; email/URL absent). GU: sanitize filters, flags, filter_var_array. N/A: filter_input family (superglobal-driven) |
| **FN-SESS** (10) | 0 | 0 | 10 | 0 | 0 | 0 | **No session layer.** GAP-planned(deferred): K-auth-csrf-session (§3 M6 deferred web-security). Every stateful web app needs this |
| **FN-STREAM** (15) | 0 | 0 | 0 | 13 | 0 | 2 | No stream/context/wrapper/filter abstraction (the internal `Transport` trait is not user-facing). GU across the board; N/A: sapi_windows rows |
| **FN-SOCK** (10) | 0 | 0 | 0 | 10 | 0 | 0 | No raw socket API (serve owns the socket internally) |
| **FN-XML** (12) | 0 | 0 | 0 | 12 | 0 | 0 | **No XML/DOM/XPath/SimpleXML/XMLReader-Writer at all** (Core.Html is emission-only) |
| **FN-FINFO** (4) | 0 | 0 | 0 | 4 | 0 | 0 | No MIME sniffing |
| **FN-ZLIB** (7) | 0 | 0 | 0 | 7 | 0 | 0 | No compression |
| **FN-ZIP** (1) | 0 | 0 | 0 | 1 | 0 | 0 | No archives |
| **FN-PHAR** (1) | 0 | 0 | 1 | 0 | 0 | 0 | GP: P-phar (§3 M6); `phg build` already covers the self-contained-app use natively |
| **FN-INTL** (18) | 0 | 0 | 6 | 10 | 2 | 0 | GP: grapheme_*, Normalizer, IntlChar subset (M-text S2/S3) + number/date/message formatters (tier-3 extension policy, §3 defer). GD: Collator, Transliterator (§5 — need ICU data, can't honor zero-dep + `php -n` oracle). GU: calendars, timezones-intl, break iterators, Spoofchecker, UConverter, ResourceBundle, idn, ListFormatter |
| **FN-GD** (7) | 0 | 0 | 0 | 7 | 0 | 0 | No image processing (~110 fns family) |
| **FN-REFL** (15) | 0 | 5 | 0 | 5 | 2 | 3 | P: ReflectionClass/Object/Property/Enum introspection subset via `Core.Reflection` kind/className/typeName/fields/methods/parents/interfaces (names only, read-only). GD: ReflectionMethod/Function *invoke* (dynamic dispatch violates the static model). GU: parameter introspection, attributes reflection, ReflectionGenerator/Fiber, extensions. N/A: ReflectionType (types are compile-time), ReflectionReference, base classes |
| **FN-RAND** (4) | 1 | 1 | 0 | 0 | 0 | 2 | Covered: Randomizer→`Core.Random` (**CB for testing: seeded/deterministic by design — the O-deterministic-seam**). P: engines (one PRNG; **no Secure/CSPRNG engine**). N/A: interface/error rows |
| **FN-PROC** (16) | 1 | 1 | 1 | 6 | 6 | 1 | Covered: getenv→`Environment` (read-only — CB). GP: getopt (G-args, M-Batteries). GD: the pcntl fork/signal family (6 rows — green-threads single-threaded model replaces it). GU: **exec/system/proc_open (no subprocess API)**, posix family, getmypid, loadavg. P: set_time_limit (serve `--timeout`). N/A: escapeshell (no shell) |
| **FN-OB** (10) | 1 | 0 | 0 | 0 | 0 | 9 | Covered-better: header surface → Http `Response` value (fixes DEF-042). N/A: the whole ob_* buffer stack — no output-buffer model to control |
| **FN-VAR** (27) | 7 | 4 | 0 | 2 | 3 | 11 | Covered: gettype→Reflection.kind/typeName, intval-floatval-boolval→Conversion/parse (**T? — CB**), is_numeric→Validation.isNumber, get_class→className, method_exists→methods, is_a→instanceof, memory_get_*→`Core.Runtime`. P: var_dump/print_r (debugger + `--dump-on-fault`; no user dump fn — A-printf-debug §3), get_object_vars. GD: serialize/unserialize (fixes DEF-013), get_defined_vars, class_exists (compile-time knowledge). GU: var_export, get_declared_*. N/A: the is_int/is_string/... family (static types make runtime type-tests meaningless), settype, debug_zval_dump |
| **FN-FUNC** (8) | 2 | 0 | 0 | 2 | 1 | 3 | Covered: call_user_func→first-class fn values, Closure. GD: func_get_args (static signatures). GU: forward_static_call, register_shutdown_function. N/A: function_exists (compile-time), ticks, create_function (removed) |
| **FN-URL** (10) | 3 | 0 | 3 | 3 | 0 | 1 | Covered: urlencode/rawurlencode→`Url.encodeForm/encodeUriComponent`, base64→`Encoding`. GP: **parse_url + http_build_query + the 8.5 Uri objects** (G-url §3 M6 — should land spec-compliant, leapfrogging DEF-030). GU: parse_str, get_headers, get_meta_tags |
| **FN-NET** (9) | 0 | 0 | 1 | 8 | 0 | 0 | GP: syslog→G-log/Q-corelog (§3 M11). GU: DNS family, gethostby*, inet_*, fsockopen, **mail()** (DEF-031 — absent, unplanned), ftp family |
| **FN-MISC** (18) | 2 | 1 | 2 | 3 | 3 | 7 | Covered: phpversion→`--version`, token_get_all→`phg tokenize` (of Phorj itself — CB). GP: error_log (G-log), uniqid (G-uuid, deferred). P: debug_backtrace (traces exist; no user API). GD: ini_* (compile-time config), FFI (§5 E-php-ffi reject), extension model. GU: php_uname, cli process title, readline. N/A: phpinfo, gc_*, opcache_* (VM native — fixes DEF-043), highlight, get_defined_constants |

**FN tally (631):** COVERED 118 · PARTIAL 49 · GAP-planned 100 · GAP-unplanned 251 ·
GAP-by-design 24 · N/A 89.
**FN coverage** (N/A+GD excluded): (118 + 24.5) / 518 = **27.5%** (row-weighted; usage-weighted
figure in Pass 4).

### 1.3 RT rows (20)

| ID | Verdict | Justification |
|---|---|---|
| RT-001 | CB | zero-ini model: config is compile-time; build profiles baked into artifacts (PJ-CLI-003) — the DEF-019 fix generalized |
| RT-002 | P | limits are hardcoded (`src/limits.rs`) + `serve --timeout`; no memory_limit equivalent |
| RT-003 | P | one runtime (`phg run/serve/build`) + PHP front-controller transpile bridge; no FPM/mod_php equivalents (different model) |
| RT-004 | N/A | persistent-process model is the deliberate opposite contract |
| RT-005 | CB | `phg serve` is native long-running — what PHP needs FrankenPHP/Swoole to retrofit |
| RT-006 | CB | bytecode VM is native; no out-of-language cache required (fixes DEF-043) |
| RT-007 | GP | no JIT; AOT is v2 (I-aot, §3 v2) |
| RT-008 | P | `phorj.toml` + `phorj.lock` + `phg vendor` (exact-pin, offline, hash-verified — arguably safer); **no registry (by design, ADR-0005)**, **no transitive deps (documented deferral)** |
| RT-009 | CB | loader resolves functions and types alike — Composer's `files`-eager-include hack unnecessary (fixes DEF-032) |
| RT-010 | P | PSR-7/15 shape adopted at the value level (`handle(Request) -> Response`, Core.Http); PSR-3 logging GAP-planned (G-log); PSR-6/11/14 absent |
| RT-011 | P | `phg test` + `Core.Test` 8 assertions + deterministic Random/Time seam; no mocking, data providers, or coverage |
| RT-012 | CB | the checker **is** the language — real enforced generics vs docblock-PHPStan; `phg explain` codes |
| RT-013 | P | `phg debug` REPL + DAP server (interpreter-only, Dev-only); no profiler (F-profiler, §3 M13) |
| RT-014 | P | `serve --dev` error pages + faults-to-stderr; structured request logging GAP-planned (Q-serve-reqlog, §3 M6) |
| RT-015 | GD | no dynamic extension model (§5 rejects .so plugins + E-php-ffi); `.d.phg` declare-interop is the extension seam |
| RT-016 | CB | `phg build` native cross-OS static binaries (Linux glibc/musl, aarch64, Windows PE) — what static-php-cli/FrankenPHP-embed hack on |
| RT-017 | CE | `phg serve --dev` (127.0.0.1:8080, workers, timeout) |
| RT-018 | CE | SEMVER/STABILITY/DEPRECATION docs shipped (rock-3); pre-1.0 so cadence untested |
| RT-019 | CE | stdin `-`, `Process.arguments()`, `main -> int` exit codes |
| RT-020 | GU | no framework ecosystem (no Laravel/Symfony equivalent) — honest zero |

**RT tally (20):** CB 6 · CE 3 · PARTIAL 7 · GAP-planned 1 · GAP-unplanned 1 · GAP-by-design 1 · N/A 1.
**RT coverage:** (9 + 3.5) / 18 = **69.4%**.

### Pass 1 grand totals (824 verdict rows = 173 SYN + 631 FN + 20 RT)

| Verdict | SYN | FN | RT | Total |
|---|--|--|--|--|
| COVERED (better+equal) | 93 (56 CB + 37 CE) | 118 | 9 | **220** |
| PARTIAL | 20 | 49 | 7 | **76** |
| GAP-planned | 9 | 100 | 1 | **110** |
| GAP-unplanned | 7 | 251 | 1 | **259** |
| GAP-by-design | 24 | 24 | 1 | **49** |
| N/A | 20 | 89 | 1 | **110** |

---

## PASS 2 — Phorj → PHP (capabilities with no PHP counterpart)

The brag inventory: each row is a Phorj capability PHP cannot express, with the PHP pain it removes.

| # | Phorj capability | PHP pain removed |
|---|---|---|
| 1 | **Real enforced generics** — functions, methods, classes, enums (`Box<T>`, `Result<T,E>`), call-site inference, invariance checked (PJ-TY-005) | generics live only in docblocks policed by third-party PHPStan/Psalm (DEF-012) |
| 2 | **Compile-time null-safety** — non-optional `T` is never null; `??`/`?.`/`opt!`/if-let/smart-cast (PJ-SYN-007) | the billion-dollar `Call to a member function on null` production error |
| 3 | **Exhaustive `match` with patterns** — payload binding, type patterns, struct destructuring, guards, or-patterns, checked at compile time (PJ-SYN-006) | `UnhandledMatchError` at runtime; `switch` fall-through |
| 4 | **Totality checking** — `E-MISSING-RETURN`, `never`, `W-UNREACHABLE`, duplicate-arm detection | functions silently fall off the end returning null |
| 5 | **Three-tier error model** — checked `throws E` verified at every call site (`E-CALL-UNHANDLED`), `Result<T,E>`, uncatchable faults for bugs | `@throws` is an unenforced comment; warnings-vs-exceptions split brain (DEF-008) |
| 6 | **Flow narrowing** — `instanceof`/`!`/`&&`/`\|\|`/early-return narrow unions; match-over-union exhaustive | `if ($x instanceof A)` narrows nothing for the checker PHP doesn't have |
| 7 | **`decimal` primitive** (i128 fixed-point, `19.99d`, RoundingMode, exact-or-fault `/`; float×decimal mix is a compile error) | float-for-currency, the largest class of real-world PHP money bugs (DEF-023) |
| 8 | **`bytes` vs `string` split** + `b"…"` literals | one byte-string type pretending to be text (DEF-016 root) |
| 9 | **Typed XSS-safe HTML channel** — nominal `Html`/`Attr`, `html"…"`, auto-escaping holes (`E-HTML-HOLE`) | echo-a-string templating = XSS by default (DEF-020) |
| 10 | **`Secret<T>`** + `W-SECRET` sink lint | credentials are ordinary strings that leak into logs/traces |
| 11 | **Immutable-by-default** (`mutable` opt-in) + COW value collections | spooky aliasing, defensive cloning (DEF-006/DEF-024) |
| 12 | **Checked arithmetic** — overflow/div-zero/inexact are faults with identical traces on both backends | silent int→float overflow (DEF-022) |
| 13 | **Uncolored green threads** — `spawn f(x)` → `Task<T>`, `Channel<T>`, deterministic interleaving on both backends | Fibers are a low-level primitive with no scheduler; async is a third-party ecosystem fork |
| 14 | **Byte-identity dual backends + PHP oracle** — interpreter ≡ VM ≡ transpiled PHP under a real `php`, CI-gated (PJ-TOOL-005) | PHP has one implementation and no conformance corpus; BC is asserted, never proven |
| 15 | **`phg build`** — cross-OS standalone executables (glibc/musl/aarch64/Windows) with embedded program | deploy = interpreter + ini + opcache + vendor tree (DEF-043) |
| 16 | **PHP→Phorj lifter** (`phg lift`) + Phorj→PHP transpiler — a two-way migration bridge | one-way manual rewrites |
| 17 | **`.d.phg` declaration files** for foreign PHP (M8.5) — the TypeScript `.d.ts` model | no typed boundary to untyped code |
| 18 | **Toolchain in the box** — `phg test`/`benchmark`(+`--vs-php`+memory)/`format`/`lsp`/`debug --dap`/`explain`/`tokenize`/`disassemble` | PHPUnit/CS-Fixer/Xdebug/psalm are third-party assemblies |
| 19 | **Stable diagnostic codes + `phg explain`** — 196 codes, caret spans, did-you-mean, ratchet-tested explain coverage | error messages as unstructured prose |
| 20 | **Must-use by default** (`E-UNUSED-VALUE` + `discard`) | dropped return values (half the stdlib signals via return) — PHP got opt-in `#[NoDiscard]` only in 8.5 |
| 21 | **Method + return-type overloading** (`<Type>f()` selector) lowered to one dispatching PHP method | one symbol per name; userland `func_get_args` switchboards |
| 22 | **Multiple inheritance with compile-time conflict resolution** (`E-MI-CONFLICT`, `parent(A).m()`) | single inheritance + trait copy-paste conflicts (DEF-037) |
| 23 | **Enums with payloads** + generic enums | PHP enums are pure/backed constants only — no data |
| 24 | **`[T; N]` fixed lists** with static literal-index bounds checking | SplFixedArray checks at runtime |
| 25 | **Package hygiene as errors** — folder=path, one-public-type-per-file, casing (`E-PKG-*`, `E-FILE-*`, `E-NAME-CASE`) | PSR-4 is a convention enforced by no one |
| 26 | **Deterministic dependency model** — exact-pin lockfile, content-hash verify, offline-only builds, vendored source | Packagist supply chain + version ranges + install-time scripts |
| 27 | **Deterministic test seam** — seedable `Random`, `Time.freeze` | untestable time/rand without mockery |
| 28 | **Build profiles that cannot change behavior** — Dev/Release are observability-only; asserts never stripped (byte-identity across profiles) | `zend.assertions` makes prod semantically different from dev (DEF-019 family) |
| 29 | **Definite-assignment analysis** for fields (`E-FIELD-UNINITIALIZED`) | "typed property must not be accessed before initialization" at runtime |
| 30 | **UFCS** — `xs.map(f)` resolves to `List.map(xs, f)` with ambiguity errors | needle/haystack roulette (DEF-001) |
| 31 | **Expression-if with mandatory else**; expression-oriented `match` | statement/expression split; nested-ternary history (DEF-017) |
| 32 | **Text blocks with auto-dedent** + raw strings | heredoc indentation dance |
| 33 | **`with { }` record-update on instances** | shipped before PHP 8.5's `clone($o, [...])` |
| 34 | **WASM playground** sharing the real checker/VM | no official in-browser PHP |
| 35 | **Whole-project checking** — every `.phg` under the source root parsed/checked as one unit, cross-file diagnostics | file-at-a-time compile; cross-file breakage found at runtime |

---

## PASS 3 — DEF-045 defect scorecard

Adversarially checked: FIXED requires the mechanism to exist in E or be verified in code, not just
be plausible.

| DEF | Verdict | How |
|---|---|---|
| DEF-001 needle/haystack | **FIXED** | stdlib charter subject-first arg order + UFCS (`xs.contains(y)`) |
| DEF-002 naming chaos | **FIXED** | naming overhaul: full words, camelCase, charter-governed (`Core.String.startsWith`, no `strncasecmp`) |
| DEF-003 callback order | **FIXED** | uniform subject-first: `List.map(xs,f)`, `filter(xs,f)`, `reduce(xs,init,f)` |
| DEF-004 juggling / two equalities | **FIXED** | static types; single typed `==`. (Residual: float `==` compiles without lint [Verified] — IEEE issue, not PHP juggling; J-float-eq-lint pending) |
| DEF-005 array conflation | **FIXED** | List / Map / Set / `[T; N]` distinct types |
| DEF-006 spooky references | **FIXED** | no references; value/handle split explicit; foreach binds values |
| DEF-007 `@` suppression | **FIXED** | operator absent; errors are typed or fatal |
| DEF-008 errors-vs-exceptions split | **FIXED** | one coherent three-tier model, checker-enforced |
| DEF-009 `false` error returns | **FIXED** | `T?` optionals (`indexOf -> int?`); no `0\|false` trap |
| DEF-010 mutable global state | **FIXED** | no superglobals/`global`; `Environment` read-only; immutable default |
| DEF-011 mutable DateTime | **FIXED** | `Core.Time` classes are immutable-only |
| DEF-012 no generics | **FIXED** | erased generics across functions/methods/classes/enums, checker-enforced |
| DEF-013 unserialize RCE | **FIXED** | no native object (de)serialization exists; `Core.Json` is data-only |
| DEF-014 isset/empty conflation | **FIXED** | optionals + explicit `isEmpty`; no truthiness predicates (by-design reject) |
| DEF-015 locale-sensitive core | **FIXED** | no `setlocale` surface at all; behavior never varies by host locale (ICU features deferred to an explicit opt-in tier) |
| DEF-016 four string families | **PARTIALLY-FIXED — inherited sub-flaw** | ONE `Core.String` family + separate `bytes` type fixes the four-family chaos; **but `String.length` counts BYTES today [Verified: `"héllo"` → 6] — strlen's exact flaw inherited until M-text codepoint semantics land. High-priority finding.** |
| DEF-017 left-assoc ternary | **FIXED** | no ternary; expression-if with mandatory else |
| DEF-018 case-sensitivity mess | **FIXED** | uniformly case-sensitive + enforced casing conventions |
| DEF-019 foot-gun inis | **FIXED** | zero runtime config; profiles cannot change semantics (byte-identity across profiles) |
| DEF-020 include/template RCE | **FIXED** | no include/eval; typed auto-escaping `html"…"` |
| DEF-021 extract/variable-variables | **FIXED** | rejected (§5); scope is statically known |
| DEF-022 silent int→float overflow | **FIXED** | checked arithmetic → `IntOverflow` fault; float keys impossible (typed Map keys) |
| DEF-023 float display duality | **FIXED** | single-sourced Ryū round-trip float rendering on all backends; `decimal` for business data |
| DEF-024 value/object asymmetry | **PARTIALLY-FIXED** | asymmetry deliberately retained (List=COW value, Instance=handle) but made explicit, documented, and defanged by immutable-by-default. Adversarial note: it is still one `=` with two meanings at the call site |
| DEF-025 in-place bool sorts | **FIXED** | `List.sort/sortWith` are pure and return the list |
| DEF-026 three merge semantics | **FIXED** | explicit `concat` vs `merge`; no `+` on maps |
| DEF-027 loose in_array/switch | **FIXED** | typed `contains`; `switch` absent, `match` strict+exhaustive |
| DEF-028 key juggling | **FIXED** | `"1"` and `1` are distinct typed keys |
| DEF-029 string increment | **FIXED** | no `++` on strings |
| DEF-030 parse_url non-conformance | **PARTIALLY-FIXED** | nothing wrong shipped (encode/decode only) — but no URL *parser* exists yet; G-url (§3 M6) is planned to land spec-compliant. Absent ≠ fixed |
| DEF-031 mail() injection | **N/A** | no mail facility (unplanned) — nothing to inject |
| DEF-032 no fn autoloading | **FIXED** | loader resolves free functions statically; single-file PHP emission avoids PSR-4's function hole |
| DEF-033 namespace fallback | **FIXED** | explicit imports; zero runtime name fallback |
| DEF-034 named args froze param names | **N/A** | no named args yet; **when A-named-args lands, param names become API — decide renaming policy up front** |
| DEF-035 destructor fatality | **FIXED** | destructors deliberately absent (A-destruct reject) — the hazard class cannot exist |
| DEF-036 silent destructuring nulls | **FIXED** | refutable destructure requires diverging `else` (`E-DESTRUCTURE-NEEDS-ELSE`); fields type-checked |
| DEF-037 trait copy-paste | **PARTIALLY-FIXED** | conflicts are hard errors with explicit `rename`/`exclude` (vs silent `insteadof`); but composition is still copy-in — trait state duplicates per using class, same as PHP |
| DEF-038 LSB four-way confusion | **FIXED** | by simplification: explicit `parent.m()`/`parent(A).m()`; no `self::`/`static::` distinction exists (LSB itself is a planned feature — keep the four-way trap out when it lands) |
| DEF-039 strtotime DWIM | **FIXED** | deliberately absent; only explicit Time factories |
| DEF-040 resource\|false chains | **FIXED** | typed handles + `T?` returns |
| DEF-041 lossy silent casts | **FIXED** | `as` → `T?`; `Conversion.floatToIntExact -> int?`; no implicit narrowing |
| DEF-042 output/header coupling | **FIXED** | `Response` value serialized once; headers are data |
| DEF-043 per-request compile model | **FIXED** | compiled bytecode VM + standalone binaries; no out-of-language cache |
| DEF-044 json_decode null ambiguity | **FIXED** | `Json.parse -> Json?` — `Json.Null` value ≠ parse failure |
| DEF-045 superglobal request model | **FIXED** | `handle(Request) -> Response` at the value level; body/headers are typed data |

**Scorecard: 39 FIXED · 4 PARTIALLY-FIXED (DEF-016, -024, -030, -037) · 2 N/A (DEF-031, -034) ·
0 fully INHERITED.** The one adversarial red flag: **byte-based `String.length` (DEF-016) is a
genuinely inherited strlen flaw** until M-text ships codepoint semantics.

---

## PASS 4 — Completion percentage model

### 4.1 Method

Coverage per domain = (COVERED×1 + PARTIAL×0.5) / (rows − N/A − GAP-by-design). N/A and
GAP-by-design are excluded from the denominator: Phorj should not be penalized for deliberately
removing footguns or for rows meaningless in its model. All weights are judgment calls and are
flagged as such; the row counts and per-group arithmetic are mechanical, so the number is
recomputable after each milestone by re-running Pass 1 verdicts.

### 4.2 Raw row-parity (no weighting — the pessimistic floor)

- SYN: 103 / 129 = 79.8%
- FN: 142.5 / 518 = 27.5%
- RT: 12.5 / 18 = 69.4%
- **Pooled: (103 + 142.5 + 12.5) / (129 + 518 + 18) = 258 / 665 = 38.8%**

This treats `mysqli_stmt_bind_param` and `foreach` as equal-weight rows — useful as a floor, wrong
as a headline.

### 4.3 Usage-weighted stdlib (judgment: groups tiered by real-code frequency)

Tier ×3 (daily): STR ARR MATH JSON DATE FS PCRE URL VAR FUNC HASH CRYPT.
Tier ×2 (common): DB CURL SESS REFL RAND PROC MISC CTYPE FILTER MB SPL OB NET.
Tier ×1 (occasional): ICONV STREAM SOCK XML FINFO ZLIB ZIP PHAR INTL GD.

Per-tier score/denominator (from §1.2, N/A+GD excluded):
- T1: score 124.5 / den 303 → 41.1%
- T2: score 18.5 / den 140 → 13.2%
- T3: score 0 / den 75 → 0%

Weighted stdlib = (3×124.5 + 2×18.5 + 1×0) / (3×303 + 2×140 + 1×75) = 410.5 / 1264 = **32.5%**

### 4.4 Domain-weighted PHP-parity %

| Domain | Weight (judgment) | Coverage | Contribution |
|---|--|--|--|
| Language syntax, semantics, type system, error model (SYN) | 35 | 79.8% | 27.9 |
| Stdlib, usage-weighted (FN) | 40 | 32.5% | 13.0 |
| Runtime, deployment, ecosystem (RT) | 25 | 69.4% | 17.4 |
| **PHP-parity %** | 100 | | **≈ 58%** |

Weight rationale (stated, contestable): PHP's practical surface is stdlib-heavy (40); the language
core is what migrating code is *written in* (35); runtime/ecosystem determines deployability (25).
Moving stdlib weight ±10 points moves the headline ±4.7 points — the number is weight-sensitive
and should always be quoted with the weights.

### 4.5 Vision % (parity + the beyond-PHP programme)

Vision denominator = 70% PHP-parity + 30% roadmap-programme completion (the §3 rollup milestones,
judged against E/current state; v2 and post-1.0 M13 excluded as out-of-vision-scope):

M-RT 100 · M-faults 100 · M5-followups 25 · M2.5-P3 50 · M6 70 · M8/M8.5 60 · M9 70 ·
M11+M4 70 · M7/M12 70 · GA-M12 60 · M-NUM 80 · M-TIME 70 · M-text 40 · M-Test 85 ·
M-perf 30 · M-Batteries 50 → mean = 1030/16 = **64.4%** [judgment-graded per milestone]

**Vision % = 0.70 × 58.3 + 0.30 × 64.4 = 40.8 + 19.3 ≈ 60%**

Both headline numbers, honestly labeled: **PHP-parity ≈ 58% (row-parity floor 39%) · Vision ≈ 60%.**
The dominant drag on both is the same thing: stdlib breadth (FS/streams, DB, HTTP client,
sessions, XML, intl) — the language itself is at ~80% parity with substantial better-than coverage.

---

## TOP-20 highest-impact gaps (impact = frequency in real PHP code × migration blockage)

| # | Gap | Class | Evidence / plan |
|---|---|---|---|
| 1 | **Database access** (PDO/mysqli/SQLite — all 10 FN-DB rows) | GAP-planned | ROADMAP M6 "Postgres connectivity"; nothing exists today. Blocks essentially every real app |
| 2 | **HTTP client** (all 13 FN-CURL rows + url-capable file_get_contents) | GAP-planned | M6 determinism deferral; post-M-DX audited dev question. Second-most-universal capability |
| 3 | **Sessions / cookies / auth** (10 FN-SESS rows) | GAP-planned (deferred) | K-auth-csrf-session (§3 M6). Every stateful web app |
| 4 | **sprintf/printf format family** (7 FN-STR rows) | GAP-planned | A-sprintf (§3 M11). Ubiquitous in ported code; interpolation covers only simple cases |
| 5 | **Filesystem breadth** (mkdir/scandir/glob/stat/perms/temp/streams — ~40 FN-FS rows) | GAP-planned/unplanned | G-dir/G-file-more (M-Batteries) covers part; stream handles unplanned |
| 6 | **Unicode-correct strings** (byte `String.length` [Verified], mb_* replacement) | PARTIAL/inherited | M-text programme — the one *inherited* PHP defect (DEF-016); silently wrong today on non-ASCII |
| 7 | **Named arguments + variadics + spread** | GAP-planned | A-named-args/A-variadics (§3). Modern PHP idiom; also blocks the lifter on 8.0+ code |
| 8 | **Generators/`yield` + iterator protocol** | GAP-planned | marathon A2 + A-iterators (§3 M11). Lazy pipelines, large-data loops, Traversable interop |
| 9 | **Date/time breadth** (timezones, formatting, DatePeriod, parsing) | GAP-planned | N-tz-iana (M-TIME-2). Every business app touches tz |
| 10 | **CSPRNG** (random_int/random_bytes) + timing-safe compare (hash_equals) + HMAC | GAP-planned | G-crypto (§3 M8). Security-critical absences — tokens/nonces can't be generated safely today |
| 11 | **array_* long tail** (diff/intersect on lists, splice, column, combine, multisort, pad) | GAP mixed | L-list-breadth remainder. Muscle-memory blockers for line-by-line migration |
| 12 | **XML/DOM/XPath** (12 FN-XML rows) | GAP-unplanned | enterprise integration formats; no plan on record |
| 13 | **Subprocess execution** (exec/proc_open family) | GAP-unplanned | CLI tooling written in PHP shells out constantly; no Phorj process API |
| 14 | **Regex breadth** (preg_replace_callback, preg_quote, full modifier surface) | GAP-unplanned | callback-replace is the common non-trivial regex use |
| 15 | **Compression/archives** (zlib/zip; phar planned) | GAP-unplanned (P-phar GP) | deploy/packaging + data interchange |
| 16 | **User-defined attributes + reflection of them** | GAP mixed | only `#[Route]` exists; PHP frameworks are attribute-driven (8.0+) |
| 17 | **`__toString`/Stringable + `__invoke`** | GAP-planned | A-magic-stringable/A-magic-invoke (§3). Pervasive interop idioms in APIs being lifted |
| 18 | **Structured logging** (error_log/syslog → G-log/Q-corelog) | GAP-planned | §3 M11/M6; production apps need a log seam before serve is production-usable |
| 19 | **intl formatters** (NumberFormatter/IntlDateFormatter/MessageFormatter) | GAP-planned (tier-3 defer) | any localized app; deliberate ICU deferral needs an explicit extension story |
| 20 | **Math long tail + BigInt** (atan2/hypot/log2, base_convert; GMP/BCMath arbitrary precision) | GAP-planned | G-math-breadth (M11), N-bigint (M-NUM-2) |

Watch-item outside the ranking: **transitive dependencies** (RT-008, documented deferral) becomes
a top-10 blocker the moment a second-party package ecosystem appears.

---

## SUMMARY

```
PASS 1 (824 rows: 173 SYN + 631 FN + 20 RT)
  COVERED          220   (SYN 93 [56 better / 37 equal] · FN 118 · RT 9)
  PARTIAL           76   (SYN 20 · FN 49 · RT 7)
  GAP-planned      110   (SYN 9  · FN 100 · RT 1)
  GAP-unplanned    259   (SYN 7  · FN 251 · RT 1)
  GAP-by-design     49   (SYN 24 · FN 24 · RT 1)
  N/A              110   (SYN 20 · FN 89 · RT 1)

Coverage:  language 79.8% · stdlib 27.5% row-weighted / 32.5% usage-weighted · runtime 69.4%

PHP-parity %  ≈ 58   (domain-weighted; raw row-parity floor 38.8%)
Vision %      ≈ 60   (70% parity + 30% roadmap-programme at 64.4%)

PASS 2: 35 beyond-PHP capabilities (no PHP counterpart)

PASS 3 (45 DEF): 39 FIXED · 4 PARTIALLY-FIXED (DEF-016 string-bytes [inherited sub-flaw,
  verified], DEF-024 value/handle asymmetry, DEF-030 URL parser absent-not-fixed,
  DEF-037 trait state duplication) · 2 N/A · 0 fully INHERITED

Top-5 gaps: database access · HTTP client · sessions/auth · sprintf family · filesystem breadth
```
