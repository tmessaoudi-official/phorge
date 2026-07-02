# Agent E — Phorj Surface Inventory (Side B)

> Full-audit raw output. Code-verified inventory of everything Phorj HAS, as of commit `ccb2403`
> (2026-07-02). Scaffold: `docs/specs/2026-07-01-no-wind-namespace-and-language-surface-design.md`
> (the "nothing in the wind" SSOT) — verified claim-by-claim; mismatches in §DRIFT.
> Every item carries a file reference. IDs are stable for gap-matrix referencing.

---

## PJ-KW — Keywords & Tokens (`src/lexer/mod.rs`, `src/token.rs`)

### PJ-KW-001 — Reserved keywords (42) — lexer keyword table `src/lexer/mod.rs:828-878`
`function class enum constructor trait const open abstract public private protected internal
return if else for while do break continue in match import package this true false null new
instanceof interface implements extends mutable static with type throw try catch finally throws`

### PJ-KW-002 — Contextual keywords (14) — lex as `Ident`, recognized positionally
| Word | Role | Ref |
|---|---|---|
| `var` | inferred binding / destructure head | `src/parser/mod.rs:157`, `stmts.rs:34` |
| `foreach` | `foreach (expr as x)` loop | `stmts.rs:40,393` |
| `as` | foreach separator · cast `x as T` · import alias | `exprs.rs:100`, `items.rs:174` |
| `when` | match-arm / if-let / while-let guard | `exprs.rs:679`, `stmts.rs:295,610` |
| `discard` | `discard <expr>;` must-use escape | `mod.rs:160` |
| `spawn` | green-task prefix `spawn f(x)` | `mod.rs:182`, `exprs.rs:217` |
| `parent` | super-dispatch `parent.m()` / `parent(A).m()` | `mod.rs:174`, `exprs.rs:256` |
| `test` | `test "name" { … }` item | `items.rs:15` |
| `declare` | foreign PHP interop item (M8.5) | `items.rs:24` |
| `use` / `rename` / `exclude` | trait composition + conflict resolution | `items.rs:568-676` |
| `get` / `set` | property hooks | `items.rs:858,868` |

### PJ-KW-003 — TokenKind: 98 variants (`src/token.rs`)
7 literal kinds (`Int(i64)`, `Float(f64)`, `Decimal(i128,u8)` — `19.99d` suffix, `Str(Vec<StrSeg>)`
pre-split interpolation, `Bytes(Vec<u8>)` — `b"…"`, `Html(String)` — `html"…"`, `Ident`), 42 keywords,
49 punctuation/operators incl.: `.. ..= ?? ?. ??= ?. -> => |> | & ^ ~ << #[ ** += -= *= /= %= ++ --`.
Notable: **`>>` is NOT a token** — two adjacent `Gt` handled in `parse_binary` so `List<List<int>>`
closes (`token.rs` Shl doc). `#[` opens PHP-8-style attributes; bare `#` is not a token.

### PJ-KW-004 — String-literal family (lexer)
- `"…"` with interpolation `{expr}` (lexer-split `StrSeg::Lit/Interp` with absolute-offset spans),
  escapes `\n \t \u{…} \{ …`
- `r"…"` / `r#"…"#` raw strings (no escapes, no interpolation) — `lexer/mod.rs:531,1014`
- `"""…"""` multi-line **text blocks**, JEP-378 auto-dedent (A-62) — `lexer/mod.rs:401-412,866+`
- `b"…"` bytes literal (`\xHH`), `html"…"` typed-HTML literal (interpolation desugared in parser)

### PJ-KW-005 — Number literals
Decimal ints; floats incl. exponent; base prefixes `0x`/`0b`/`0o` (`lexer/mod.rs:60-70`); `_`
separators; `19.99d` decimal literal (scale-carrying, `1e3d` rejected — `token.rs` Decimal doc).

### PJ-KW-006 — PHP-reserved-name guard
Kind-aware list of words legal in Phorj but PHP-reserved in symbol position (`switch`, `namespace`,
`yield`, `goto`, `insteadof`, `endforeach`, …) → `E-RESERVED-NAME` (`checker/common.rs:336-380`).

---

## PJ-SYN — Syntax Constructs (`src/parser/`, `src/ast/mod.rs`)

### PJ-SYN-001 — Items (8 AST variants, `ast/mod.rs:871`)
`Import` (incl. `import type`, `as` alias) · `Function` · `Enum` · `Class` · `Interface` · `Trait` ·
`TypeAlias` (`type X = Y;`) · `Test` (`test "name" { … }`). Plus: `package A.B;` declaration
(Program-level, mandatory — `E-NO-PACKAGE`), foreign `declare function …;` / `declare class … { … }`
(M8.5 — `foreign: bool` flag on Function/Class, `items.rs:24,285`), `#[Route("GET","/p")]` attributes
on free functions (only `Route` recognized — `checker/program.rs:626`).

### PJ-SYN-002 — Statements (14 Stmt variants, `ast/mod.rs:452`)
`VarDecl` (typed / `var` inferred) · `Assign` (incl. index/path targets) · `Return` · `If` (incl.
if-let `if (var x = opt)` + `when` guard) · `For` (for-in over list/range/string/Map two-binding —
also spelled `foreach (e as x)`) · `While` (incl. while-let + `when` guard) · `CFor` (C-style
`for(;;)`) · `Break` · `Continue` · `Block` · `Expr` · `Discard` · `Throw` · `Try` (catch/finally) ·
`Destructure`. **do-while** exists (`parser/stmts.rs:13,659`). Parser-desugared: `+= -= *= /= %=`,
`x++`/`x--` (statement-only), `??=` (all → `Stmt::Assign`).

### PJ-SYN-003 — Destructuring statement (`ast/mod.rs:562`, `DestructurePat`)
`var Point { x, y: px } = p;` (struct, irrefutable) and `var [a, b] = xs;` (list — refutable on
`List<T>` with mandatory diverging `else`, irrefutable on length-matching `[T; N]`).

### PJ-SYN-004 — Expressions (30 Expr variants, `ast/mod.rs:185`)
Literals (`Int Float Decimal Bool Null Str Bytes Html List Map`) · `Ident` · `This` · `Unary`
(`- ! ~`) · `Binary` (21 ops: arith `+ - * / % **`, compare, `&& ||`, pipe `|>` — parser-lowered to
Call, `??`, bitwise `& | ^ << >>`) · `InstanceOf` · `Cast` (`x as T` → `T?`, single-eval) · `Call` ·
`Member` · `Index` (`xs[i]`, `m[k]`) · `Force` (`opt!`) · `ParentCall` · `OverloadSelect`
(`<Type>f(args)` return-overload selector — `parser/exprs.rs:204`) · `Propagate` (`expr?`) · `Match` ·
`Range` (`a..b` / `a..=b`) · `If` (expression-if, parens + mandatory else) · `Lambda` · `CloneWith`
(`obj with { f = v, … }` — `parser/exprs.rs:340`) · `New` (mandatory `new` for class/variant
construction — `E-NEW-REQUIRED`) · `Spawn` · `Html`.

### PJ-SYN-005 — Lambdas & first-class functions (M3 S3)
Expression body `fn(int x) => e` and statement body `fn(int x) -> int { … }` (explicit return type);
capture by value; `E-LAMBDA-THIS`; named functions as values; UFCS method-style calls on natives
(`xs.map(f)` → `List.map(xs,f)`, span-keyed erasure `checker/rewrite_ufcs.rs`); method references
as values are design-adopted, NOT implemented (SSOT Q1).

### PJ-SYN-006 — Match & patterns (11 Pattern variants, `ast/mod.rs:68`)
`Wildcard _` · `Binding` · `Int/Float/Decimal/Str/Bool/Null` literals · `Variant` (`Some(n)`, nested
payload patterns) · `Type` (`Circle c` over unions) · `Struct` (`Point { x, y: px }`, shorthand/
rename/full nesting). Plus: **or-patterns** `p1 | p2 =>` (parser desugars to one arm per alternative,
binding-free — `parser/exprs.rs:672-700`, `E-OR-PATTERN-BIND`); **guards** `pat when cond =>`
(guarded arm never discharges exhaustiveness). Match is expression-only (no statement-match).

### PJ-SYN-007 — Null-safety suite (M3 S2)
`T?` optionals; `??`; `?.`; `opt!` (fault `force-unwrap of null`, `W-FORCE-UNWRAP`); if-let +
smart-cast; match-over-`T?` null-arm narrowing; flow narrowing from `instanceof`/`!`/`&&`/`||`/
early-return (`checker` narrow_from_condition).

### PJ-SYN-008 — Errors & exceptions (M-faults)
`throws E1 | E2` clauses; `throw`; `try/catch/finally`; `?` propagate (let-init position only,
`E-PROPAGATE-*`); checked at call sites (`E-CALL-UNHANDLED`, `E-THROW-UNDECLARED`,
`E-THROWS-TOO-BROAD` bans `throws Error`); built-in `Error` marker interface root; unchecked
faults/panics separate tier. Fault intrinsics `panic("m") todo() unreachable() assert(c[, "m"])`
are currently **import-free call syntax** (`checker/common.rs:10 is_intrinsic_name`; string-literal
message enforced `E-INTRINSIC-LITERAL`; SSOT decision moves them behind `import Core;` — not yet
implemented, no `E-UNIMPORTED` code exists yet).

### PJ-SYN-009 — Concurrency (M6 W4, green threads)
`spawn f(args)` → `Task<T>` (`E-SPAWN-NOT-CALL`, `E-SPAWN-VOID`); `Channel<int> ch =
Channel.create();` (annotation-required — `E-CHANNEL-ANNOTATION`); `ch.send(v)`, `ch.receive()`,
`task.join()` (built-in handle-method dispatch `checker/calls.rs:1097-1135`). Cooperative,
single-threaded (Rc heap); **not transpilable** (`E-CONCURRENCY-NO-PHP`). No generators/`yield`
(reserved word only, marathon A2 pending).

### PJ-SYN-010 — Imports
`import Core.X;` (stdlib, leaf-qualified calls `X.f()`); user packages `import A.B;` (path =
package dirs, arbitrary depth, PascalCase `E-PKG-CASE`); `import a.b as c;` aliasing;
`import type Pkg.Path.Type [as T];` selective type import (`E-TYPE-IMPORT-{UNKNOWN,CONFLICT,
BUILTIN,SHADOW}`); `E-SHADOW-IMPORT` guard. **No deep stdlib imports** (`import Core.List.map` —
SSOT decision 2, unimplemented), no wildcards.

---

## PJ-TY — Type System (`src/types.rs`, `src/checker/`)

### PJ-TY-001 — Checker `Ty`: 23 variants (`types.rs:6`)
`Int Float Decimal Bool String Bytes Html Attr Void Empty Never Named(String,Vec<Ty>)
List FixedList(T,N) Map Set Optional Null Param Union Intersection Error Function`

### PJ-TY-002 — AST `Type`: 8 forms (`ast/mod.rs:17`)
`Named{name,args}` · `Optional T?` · `Union A|B` · `Intersection A&B` (binds tighter than `|`) ·
`Infer` (`var`) · `Function (int,string)->bool` · `FixedList [T; N]` · `Erased` (post-checker only).

### PJ-TY-003 — Built-in type words (`checker/common.rs:297-330`)
Active (17): `int float bool string bytes never void empty decimal Html Attr List Map Set Error
Channel Task`. **Reserved-but-rejected (9):** `double i8 i16 i32 i64 u8 u16 u32 u64`
(`checker/resolve.rs:224` errors on use — future numeric widths).

### PJ-TY-004 — Two-nothing model
`void` (uncapturable, `E-VOID-CAPTURE`, not a union member `E-VOID-IN-UNION`) <: `empty` (holdable
nothing); `never` bottom type (`-> never` verified diverging, `E-NEVER-RETURN`); totality:
`E-MISSING-RETURN`, `W-UNREACHABLE`, `W-MATCH-UNREACHABLE`.

### PJ-TY-005 — Generics (erased, TypeScript model)
`<T>` on free functions, methods, classes (`Box<T>`, invariant, inference-only construction),
enums (`Option<T>`/`Result<T,E>`-shaped); call-site `unify`; `checker::erase_generics` rewrites to
`Type::Erased` pre-backend; native sigs use registry-only `Ty::Param`. No bounds, no variance
annotations. `E-GENERIC-PARAM`, `E-TYPE-ARG-COUNT`.

### PJ-TY-006 — Unions & intersections
`A|B` (classes/interfaces/primitives; `E-UNION-MEMBER/-ARITY`; match-over-union type patterns,
exhaustive) · `A&B` (interfaces + ≤1 class; `E-INTERSECT-MEMBER/-MULTI-CLASS/-ARITY/-SIG/
-NO-MEMBER`; PHP 8.1 `A&B`).

### PJ-TY-007 — Misc type features
`type` aliases (cycle-checked, expanded pre-backend) · `[T; N]` fixed lists (static literal-index
bounds `E-FIXEDLIST-*`) · `decimal` exact fixed-point primitive (i128 carrier, no float mix
`E-DECIMAL-FLOAT-MIX`, exact-or-fault `/`) · `bytes` · nominal `Html`/`Attr` (XSS-safe channel) ·
`Secret<T>` (injected on `import Core.Secret`, `W-SECRET` lint) · checked cast `x as T` → `T?`
(`W-REDUNDANT-CAST`) · `instanceof` with smart-cast.

### PJ-OOP-001 — Classes (`ast` ClassDecl: `classes.rs`/parser)
**Multiple inheritance** `extends A, B` (`extends: Vec<String>`; `E-MI-CONFLICT/-CYCLE/
-FIELD-CONFLICT`, `E-PARENT-AMBIGUOUS`); final-by-default (`open` opt-in, `E-EXTEND-FINAL`,
`E-OVERRIDE-FINAL/-SIG`); `abstract` classes + methods (`E-ABSTRACT-*`); constructor promotion
(`constructor(public T x)`); field initializers (`E-FIELD-INIT-*`, definite-assignment
`E-FIELD-UNINITIALIZED`); member visibility `public/private/protected` on fields/methods/ctors/
consts (six access surfaces, `E-*-VISIBILITY`); declaration visibility `public/internal/private`
(`E-VIS-INTERNAL/-PRIVATE`); `static` fields/methods (`E-STATIC-*`, `E-OPEN-STATIC`); class
`const` (SCREAMING_SNAKE_CASE, literal init — `E-CONST-*`); property hooks `T name { get => e;
set(v) { … } }` (`E-HOOK-*`); `this.field` mandatory (`E-BARE-FIELD`); `parent.m()` /
`parent(A).m()` / `parent.constructor(…)`; entry points: free `main` or static class `main`
(`E-MAIN-SIGNATURE`, `E-MULTIPLE-MAIN`), `main(List<string> args) -> int` exit codes.

### PJ-OOP-002 — Overloading (M-RT)
Parameter-type overloading (`E-OVERLOAD-DUPLICATE/-NO-MATCH/-ERASE/-STATIC-MIX/-GENERIC`) +
return-type overloading with `<Type>f()` selector (`E-OVERLOAD-RETURN/-NO-CONTEXT/
-AMBIGUOUS-RETURN/-SELECT-*`, `E-OVERLOAD-FN-VALUE`). Transpiles to one dispatching PHP method.

### PJ-OOP-003 — Interfaces & traits
Interfaces: multi-`extends`, nominal subtyping, `instanceof` RHS, `E-IFACE-IMPL/-UNIMPL/-SIG/
-CYCLE`. Traits: `trait T { … }` + `use T;` with methods/state/constructors/abstract
requirements/hooks; conflict resolution `use P.m` / `rename P.m as n` / `exclude`
(`ast/mod.rs:829` Resolution; `E-TRAIT-CTOR-COLLISION`, `W-TRAIT-CTOR-*`, `E-USE-UNKNOWN`,
`E-USE-AS-TYPE`). PHP native `trait`/`use` emission.

### PJ-OOP-004 — Enums
Payload variants (`enum Shape { Circle(float r), … }`); generic enums `Option<T>`; construction
`new Some(7)` (`E-NEW-REQUIRED`); match with payload binding + nested patterns; zero-payload
construct `new V()`, match with `V()` call form (bare `V =>` is catch-all binding — documented
footgun). `E-DUP-VARIANT`.

### PJ-OOP-005 — File & naming rules
One public type per file, file named after it (`E-FILE-MULTI-PUBLIC/-MIXED-PUBLIC/-NAME`);
camelCase values (`E-NAME-CASE`), PascalCase types (`E-TYPE-CASE`), PascalCase packages
(`E-PKG-CASE`); folder=path (`E-PKG-PATH`); `package Main` = runnable entry; `Core`/`core` roots
reserved (`E-RESERVED-PACKAGE`).

---

## PJ-NAT — Native Stdlib (`src/native/*.rs`)

**270 natives, 27 modules** (Core.Regex feature-gated on the `regex` crate; wasm playground builds
without it). Registry keyed `(module, name)` (`native/mod.rs:368 registry()`); `NativeEval::Pure |
HigherOrder | Reflective`; per-native `php` erasure + `pure` quarantine flag. Naming-overhaul names
are live (Output/String/Conversion/Cryptography/Reflection/Validation). Only trailing default:
`Core.String.parseFloat(s, bool permissive = false)` (`native/mod.rs:397 native_defaults`).
Deprecation side-table `deprecation_of` → `W-DEPRECATED` (empty in shipping build).

### PJ-NAT-OUTPUT (2) — `mod.rs:299,316`
`print(string)->void` · `printLine(string)->void` (requires `import Core.Output;` — bare `println`
retired).

### PJ-NAT-MATH (31) — `math.rs`
`abs(int) ceil(float) clamp(int,int,int) cos e() exp floor gcd infinity() integerDivide
integerPower isEven isFinite isInfinite isNaN isOdd lcm log log10 max(int,int) min(int,int) nan()
negativeInfinity() numberFormat(float,int)->string pi() pow(float,float) round(float)->int
sign(int) sin sqrt tan` (float ops `(float)->float` unless shown).

### PJ-NAT-STRING (31) — `text.rs` (module `Core.String`)
`capitalize contains containsIgnoreCase count endsWith equalsIgnoreCase indexOf->int? isEmpty
join(List<string>,s) lastIndexOf->int? length lines->List<string> lowercase padLeft padRight
parseBool->bool? parseFloat(s,bool=false)->float? parseInt->int? removePrefix removeSuffix
repeat(s,int) replace(s,s,s) reverse split->List<string> splitOnce->List<string> startsWith
substring(s,int,int) trim trimEnd trimStart uppercase`

### PJ-NAT-LIST (30) — `list.rs` (generic `Ty::Param`; map/filter/reduce are HigherOrder)
`all any append chunk concat contains count drop enumerate->Map<int,T> fill(T,int) filter find->T?
first->T? flatten indexOf->int? isEmpty last->T? lastIndexOf->int? length map<T,U> max->T? min->T?
reduce(list,U,fn(U,T)->U) reverse slice sort sortWith(cmp) sum(List<int>) take unique`

### PJ-NAT-MAP (12) — `map.rs`
`filter get->V? getOrDefault has isEmpty keys->List<K> map merge remove set size values->List<V>`

### PJ-NAT-SET (11) — `set.rs`
`add contains difference intersection isEmpty isSubset of(List<T>)->Set<T> remove size toList union`

### PJ-NAT-FILE (8) — `file.rs` (impure module, quarantined; `tests/filesystem.rs`)
`append(s,s) copy(s,s)->int delete(s) exists(s)->bool read(s)->string? rename(s,s) size(s)->int?
write(s,s)`

### PJ-NAT-PATH (5) — `path.rs`
`baseName directoryName extension fileStem join(s,s)`

### PJ-NAT-BYTES (6) — `bytes.rs`
`concat find(b,b)->int? fromString(s)->bytes length slice(b,int,int) toString(b)->string?`

### PJ-NAT-HTML (51) — `html.rs`
Core 8: `text(s)->Html raw(s)->Html render(Html)->string attribute(s,s)->Attr
booleanAttribute(s)->Attr element(s,List<Attr>,List<Html>)->Html voidElement(s,List<Attr>)->Html
concat(List<Html>)->Html`. Plus 37 `tag_el!` content-element helpers
(`div span p a ul ol li h1..h6 section article header footer nav main aside button label form
table thead tbody tr td th em strong b i small code pre blockquote`) + 6 `tag_void!`
(`br hr img input meta link`) — `html.rs:303-346`.

### PJ-NAT-JSON (3) — `json.rs`
`parse(string)->Json? stringify(Json)->string stringifyPretty(Json)->string` (Json = injected enum,
PJ-NAT-INJ).

### PJ-NAT-CONVERSION (20) — `convert.rs` (`as`-cast matrix backing)
`asBool<T>->bool? asFloat<T>->float? asInt<T>->int? boolToDecimal boolToFloat boolToInt
decimalToBool decimalToFloat decimalToInt->int? decimalToIntExact->int? floatToBool
floatToDecimal->decimal? floatToIntExact->int? intToBool intToDecimal round(float)->int
toFloat(int) toInt(float)->int? toString<T> truncate(float)->int`

### PJ-NAT-DECIMAL (3) — `decimal.rs`
`of(string)->decimal? divide(d,d,int,RoundingMode)->decimal round(d,int,RoundingMode)->decimal`

### PJ-NAT-ENCODING (4) — `encoding.rs`
`base64Decode(s)->bytes? base64Encode(b)->string hexDecode(s)->bytes? hexEncode(b)->string`

### PJ-NAT-HASH (4) — `hash.rs:392` (all `(bytes)->string` hex)
`crc32 md5 sha1 sha256`

### PJ-NAT-CRYPTOGRAPHY (2) — `crypto.rs` (argon2 crate, feature-gated)
`hashPassword(string)->string` (impure: random salt) · `verifyPassword(string,string)->bool`

### PJ-NAT-REGEX (7) — `regex.rs` (feature `regex`)
`compile(s)->Regex find(Regex,s)->string? findAll->List<string> findGroups->Map? matches->bool
replace(Regex,s,s)->string split->List<string>`

### PJ-NAT-URL (4) — `url.rs`
`decodeForm->string? decodeUriComponent->string? encodeForm encodeUriComponent`

### PJ-NAT-VALIDATION (5) — `validate.rs:76` (all `(string)->bool`)
`isInt isNumber isAlpha isAlnum isHex`

### PJ-NAT-CSV (2) — `csv.rs`
`format(List<string>)->string parse(string)->List<string>`

### PJ-NAT-RANDOM (4) — `random.rs` (seeded/deterministic → pure)
`intBetween(int,int) nextFloat() nextInt() seed(int)`

### PJ-NAT-TIME (3) — `time.rs` (impure)
`nowMilliseconds()->int freeze(int) unfreeze()` (+ injected Duration/Date/Instant classes,
PJ-NAT-INJ).

### PJ-NAT-RUNTIME (4) — `runtime.rs` (impure; manual benching)
`memoryBytes monotonicNanos peakMemoryBytes resetPeakMemory`

### PJ-NAT-PROCESS (1) + PJ-NAT-ENVIRONMENT (2) — `process.rs` (impure, quarantined)
`Process.arguments()->List<string>` · `Environment.get(s)->string?` · `Environment.all()->Map<string,string>`

### PJ-NAT-REFLECTION (7) — `reflect.rs` (Reflective eval; static-type pass erases `typeName`)
`className<T>->string? fields->List<string> interfaces->List<string> kind<T>->string
methods->List<string> parents->List<string> typeName<T>->string`

### PJ-NAT-TEST (8) — `test.rs` (for `phg test` blocks)
`assert(bool,string) assertEquals<T>(T,T) assertFalse assertNotEquals<T> assertNotNull<T>
assertNull<T> assertTrue assertFaults`

### PJ-NAT-INJ — Injected pure-Phorj preludes (`src/cli/mod.rs:346-930`), import-gated
| Import | Injected types |
|---|---|
| `Core.Json` | `enum Json { Null() Bool Int Float Str List Map … }` (`:338`) |
| `Core.Decimal` | `enum RoundingMode { HalfUp HalfDown HalfEven Up Down Ceiling Floor }` (`:382`) |
| `Core.Http` | classes `Request Response Route Router` + `respond` bridge over user `handle(Request)->Response` (`:427,625`) |
| `Core.Regex` | `class Regex { public string pattern }` (`:705`) |
| `Core.Secret` | `class Secret<T> { constructor(private T value); expose()->T }` (`:736`) |
| `Core.Time` | classes `Duration Date Instant` (static factories, plus/minus, toIso, …) (`:784`) |

---

## PJ-DIAG — Diagnostics (196 codes: 187 E-, 9 W-)

Registry: `src/cli/explain.rs` (`phg explain <CODE>`); construction via `Diagnostic::new`
(`src/diagnostic.rs`) with caret spans + did-you-mean. A ratchet test enforces every emitted code
has an explain entry — verified: zero emitted-but-unexplained codes (only test fixtures `E-FOO`/
`E-NOPE`/`E-TYPE`). Warnings ride the non-gating stderr channel.

<details><summary>Full code list (one-liners from the explain registry)</summary>

| Code | Meaning |
|---|---|
| E-ABSTRACT-INSTANTIATE | abstract class cannot be instantiated |
| E-ABSTRACT-UNIMPL | concrete class leaves an abstract method unimplemented |
| E-ALIAS-CYCLE | `type` alias refers to itself |
| E-ASSIGN-IMMUTABLE | reassignment targeted an immutable binding |
| E-ASSIGN-TARGET | assignment target is not a simple variable |
| E-ASSIGN-TYPE | reassigned value's type mismatches the binding |
| E-ASSIGN-UNKNOWN | reassignment targeted a non-local name |
| E-ATTR-TARGET | attribute attached to a non-free-function |
| E-BARE-FIELD | instance field referenced without `this.` |
| E-BREAK-OUTSIDE-LOOP | `break` outside a loop |
| E-CALL-UNHANDLED | call can throw an unhandled checked exception |
| E-CAST-TYPE | invalid `as` cast operand |
| E-CATCH-TYPE | `catch` names a non-`Error` type |
| E-CHANNEL-ANNOTATION | `Channel.create()` needs `Channel<T>` annotation |
| E-CHANNEL-NEW-ARITY | `Channel.create()` given arguments |
| E-CHANNEL-NEW-TYPE | `Channel.create()` bound to non-`Channel` type |
| E-CONCURRENCY-ARITY | wrong arg count on a concurrency-handle method |
| E-CONCURRENCY-METHOD | unknown method on a concurrency handle |
| E-CONCURRENCY-NO-PHP | green threads cannot transpile to PHP |
| E-CONST-CASE | `const` not SCREAMING_SNAKE_CASE |
| E-CONST-INIT-TYPE | `const` initializer type mismatch |
| E-CONST-INSTANCE-ACCESS | constant read through an instance |
| E-CONST-MUTABLE | `const` also `mutable` |
| E-CONST-NO-INIT | class constant lacks initializer |
| E-CONST-NOT-LITERAL | `const` initializer not a literal |
| E-CONST-REASSIGN | class constant assigned to |
| E-CONST-VISIBILITY | non-public constant read outside its class |
| E-CONTINUE-OUTSIDE-LOOP | `continue` outside a loop |
| E-CTOR-MODIFIER | non-visibility modifier on a constructor |
| E-CTOR-VISIBILITY | non-public constructor called outside scope |
| E-DECIMAL-DIV | decimal division semantics (informational; retired as error) |
| E-DECIMAL-FLOAT-MIX | `decimal` and `float` mixed |
| E-DECIMAL-LITERAL | malformed/out-of-range `decimal` literal |
| E-DECL-NONFOREIGN | `.d.phg` contains a non-`declare` item |
| E-DECL-PACKAGE | `.d.phg` declares a `package` |
| E-DEFAULT-PARAM-CONTEXT | default on a method/ctor parameter |
| E-DEFAULT-PARAM-EXPR | default not a literal constant |
| E-DEFAULT-PARAM-ORDER | required param after defaulted one |
| E-DEFAULT-PARAM-TYPE | default value type mismatch |
| E-DESTRUCTURE-DUP-BIND | destructuring binds a name twice |
| E-DESTRUCTURE-ELSE-FALLTHROUGH | destructuring `else` can fall through |
| E-DESTRUCTURE-ELSE-IRREFUTABLE | irrefutable destructuring has `else` |
| E-DESTRUCTURE-FIELD-UNKNOWN | struct destructuring names unknown field |
| E-DESTRUCTURE-NEEDS-ELSE | refutable list destructuring lacks `else` |
| E-DESTRUCTURE-NOT-CLASS | struct destructuring head not a class |
| E-DESTRUCTURE-NOT-LIST | list destructuring value not a list |
| E-DESTRUCTURE-TYPE | struct destructuring value wrong class |
| E-DUP-CONST | class declares a `const` twice |
| E-DUP-DEF | two functions share a name in one package |
| E-DUP-FIELD | instance field declared twice |
| E-DUP-PARAM | two parameters share a name |
| E-DUP-STATIC | `static` field declared twice |
| E-DUP-TYPE | type name declared twice |
| E-DUP-VARIANT | enum variant declared twice |
| E-EXTEND-FINAL | extends a non-`open` class |
| E-EXTEND-UNKNOWN | extends a non-class name |
| E-FIELD-INIT-FORWARD-REF | field init reads later field |
| E-FIELD-INIT-TYPE | field initializer type mismatch |
| E-FIELD-UNINITIALIZED | non-optional field never definitely assigned |
| E-FIELD-VISIBILITY | non-public field accessed outside scope |
| E-FILE-MIXED-PUBLIC | file mixes public type + public functions |
| E-FILE-MULTI-PUBLIC | file declares >1 public type |
| E-FILE-NAME | public type in wrongly-named file |
| E-FIXEDLIST-BOUNDS | literal index out of fixed-list bounds |
| E-FIXEDLIST-DESTRUCTURE-LEN | destructure arity ≠ fixed length |
| E-FIXEDLIST-LEN | fixed-list literal wrong length |
| E-FOREIGN-RUNTIME | foreign `declare` symbols run on a Rust backend |
| E-GENERIC-PARAM | invalid generic type parameter |
| E-GUARD-TYPE | match guard not boolean |
| E-HOOK-DUP | property hook collides with a member |
| E-HOOK-NO-GET | write-only hook was read |
| E-HOOK-NO-SET | read-only hook was assigned |
| E-HOOK-TYPE | hook get/set type mismatch |
| E-HTML-HOLE | un-renderable type interpolated into `html"…"` |
| E-HTML-IMPORT | `html"…"` without `import Core.Html;` |
| E-IF-LET-TYPE | if-let scrutinee not optional |
| E-IFACE-CYCLE | interface `extends` cycle |
| E-IFACE-IMPL | `implements` names a non-interface |
| E-IFACE-SIG | method signature ≠ interface's |
| E-IFACE-UNIMPL | interface method not implemented |
| E-INFER-NULL | `var` can't infer from `null` |
| E-INSTANCEOF-TYPE | invalid `instanceof` operand |
| E-INTERSECT-ARITY | intersection needs ≥2 distinct types |
| E-INTERSECT-MEMBER | disallowed intersection member |
| E-INTERSECT-MULTI-CLASS | ≥2 concrete classes in intersection |
| E-INTERSECT-NO-MEMBER | member access resolves to nothing |
| E-INTERSECT-SIG | members share method w/ conflicting sigs |
| E-INTRINSIC-LITERAL | intrinsic message must be string literal |
| E-LAMBDA-THIS | field-init lambda captures `this` |
| E-MAIN-SIGNATURE | unsupported `main` signature |
| E-MAP-KEY | map key type not hashable |
| E-MATCH-GUARD-EXHAUST | shape covered only by guarded arms |
| E-MATCH-TYPE | invalid match type pattern |
| E-METHOD-VISIBILITY | non-public method called outside scope |
| E-MI-CONFLICT | method inherited from >1 parent |
| E-MI-CYCLE | class `extends` cycle |
| E-MI-FIELD-CONFLICT | field inherited from >1 parent |
| E-MISSING-RETURN | not all paths return |
| E-MISSING-RETURN-TYPE | no declared return type |
| E-MULTIPLE-MAIN | >1 `main` entry point |
| E-NAME-CASE | value identifier not camelCase |
| E-NEVER-RETURN | `-> never` fn can return |
| E-NEW-ON-NONCONSTRUCT | `new` on non-constructible |
| E-NEW-REQUIRED | construction missing `new` |
| E-NO-PACKAGE | file lacks `package` |
| E-OPEN-STATIC | method both `open` and `static` |
| E-OPT-ASSIGN | `T?` used where `T` required |
| E-OPT-UNWRAP | `!` on non-optional |
| E-OPT-USE | plain access on optional receiver |
| E-OR-PATTERN-BIND | or-pattern alternative binds |
| E-OVERLOAD-AMBIGUOUS-RETURN | selector matches >1 overload |
| E-OVERLOAD-DUPLICATE | overloads w/ identical param types |
| E-OVERLOAD-ERASE | overloads indistinguishable in PHP |
| E-OVERLOAD-FN-VALUE | overloaded fn has no single value |
| E-OVERLOAD-GENERIC | generic fn can't be overloaded |
| E-OVERLOAD-NO-CONTEXT | return-overload call lacks type context |
| E-OVERLOAD-NO-MATCH | no overload accepts the args |
| E-OVERLOAD-RETURN | mixes param- and return-type overloading |
| E-OVERLOAD-SELECT-CONFLICT | `<Type>` selector vs surrounding type |
| E-OVERLOAD-SELECT-UNKNOWN | selector names no overload return |
| E-OVERLOAD-STATIC-MIX | overloads mix static + instance |
| E-OVERRIDE-FINAL | overrides non-`open` method |
| E-OVERRIDE-SIG | incompatible override return type |
| E-PARENT-AMBIGUOUS | bare `parent.m()` ambiguous under MI |
| E-PARENT-CTOR-MI | `parent.constructor` under MI |
| E-PARENT-CTOR-OUTSIDE | `parent.constructor` outside a ctor |
| E-PARENT-CTOR-STMT | `parent.constructor` as a value |
| E-PARENT-NO-METHOD | no ancestor declares the method |
| E-PARENT-NO-PARENT | `parent` in parentless class |
| E-PARENT-NOT-ANCESTOR | `parent(A)`: A not an ancestor |
| E-PARENT-OUTSIDE-METHOD | `parent` outside instance method/ctor |
| E-PATTERN-DUP-BIND | pattern binds a name twice |
| E-PKG-CASE | package/import segment not PascalCase |
| E-PKG-PATH | `package` ≠ file location |
| E-PKG-TYPE | (RETIRED gate — see DRIFT-08) type in library package |
| E-PROPAGATE-CONTEXT | `?` where propagation impossible |
| E-PROPAGATE-ERR | `?` propagates incompatible error |
| E-PROPAGATE-POSITION | `?` outside let-initializer |
| E-RANGE-TYPE | range bound not `int` |
| E-RESERVED-INTRINSIC | reserved built-in redefined |
| E-RESERVED-NAME | PHP-reserved symbol name |
| E-RESERVED-PACKAGE | user file claims `core` root |
| E-ROUTE-ARGS | `#[Route]` wrong arguments |
| E-ROUTE-HANDLER | `#[Route]` handler wrong shape |
| E-ROUTE-METHOD-STATIC | `#[Route]` method not static |
| E-ROUTE-SPEC | `#[Route]` method/path malformed |
| E-SHADOW-FN | local shadows top-level function |
| E-SHADOW-IMPORT | local shadows import qualifier |
| E-SPAWN-NOT-CALL | `spawn` on a non-call |
| E-SPAWN-VOID | spawned call returns no value |
| E-STATIC-CALL | `ClassName.method()` not a static method |
| E-STATIC-INIT-TYPE | static field init type mismatch |
| E-STATIC-NO-INIT | static field lacks initializer |
| E-STATIC-THIS | static method touches instance state |
| E-STATIC-UNKNOWN | unknown static field |
| E-STRUCT-FIELD-UNKNOWN | struct pattern names unknown field |
| E-STRUCT-PAT-TYPE | struct pattern head not a class |
| E-TEST-OUTSIDE-TESTS | `test` block outside `tests/` |
| E-THROW-TYPE | only `Error` values thrown/declared |
| E-THROW-UNDECLARED | throw neither caught nor declared |
| E-THROWS-TOO-BROAD | `throws Error` too broad |
| E-TRAIT-CTOR-COLLISION | ctors composed from ≥2 traits |
| E-TYPE-ARG-COUNT | wrong number of type args |
| E-TYPE-CASE | type identifier not PascalCase |
| E-TYPE-IMPORT-BUILTIN | `import type` of a built-in |
| E-TYPE-IMPORT-CONFLICT | two `import type` bind one name |
| E-TYPE-IMPORT-SHADOW | `import type` collides locally |
| E-TYPE-IMPORT-UNKNOWN | package doesn't export the type |
| E-UFCS-AMBIGUOUS | UFCS call matches >1 native |
| E-UNCAUGHT-THROW | exception escapes `main` |
| E-UNION-ARITY | union needs ≥2 distinct types |
| E-UNION-MEMBER | disallowed union member |
| E-UNKNOWN-ATTRIBUTE | unrecognized attribute |
| E-UNKNOWN-IDENT | name not in scope |
| E-UNKNOWN-TYPE | type not defined |
| E-UNUSED-VALUE | non-void result dropped (use `discard`) |
| E-USE-AS-TYPE | trait used as a type |
| E-USE-UNKNOWN | `use` names a non-trait |
| E-VENDOR-MAIN | vendored dep declares `package Main` |
| E-VENDOR-MISSING | required dep not vendored |
| E-VIS-INTERNAL | `internal` referenced cross-package |
| E-VIS-PRIVATE | `private` referenced cross-file |
| E-VOID-CAPTURE | `void` value captured |
| E-VOID-IN-UNION | `void` as union member |
| E-WITH-FIELD | `with` sets unknown field |
| E-WITH-NONCLASS | `with` receiver not class instance |
| E-WITH-TYPE | `with` field value wrong type |
| W-CATCH-UNREACHABLE | `catch` can never run |
| W-DEPRECATED | deprecated stdlib symbol used |
| W-FORCE-UNWRAP | `!` may fault at runtime |
| W-MATCH-UNREACHABLE | unreachable match arm |
| W-REDUNDANT-CAST | cast to same type |
| W-SECRET | Secret plaintext reaches a sink |
| W-TRAIT-CTOR-PARENT-SKIPPED | trait ctor runs instead of parent's |
| W-TRAIT-CTOR-SHADOWED | own ctor shadows trait's |
| W-UNREACHABLE | unreachable statement |

</details>

---

## PJ-CLI — CLI Surface (`src/main.rs`, `src/cli/`)

### PJ-CLI-001 — 17 public subcommands + 1 internal (`main.rs:73-75`)
Post-naming-overhaul, **full words** (no `fmt`/`bench`/`disasm`/`lex` aliases):

| Command | Purpose / flags |
|---|---|
| `run` | interpreter; project-aware; `--dump-on-fault`; `-- args…` → Process.arguments; `main` int → exit code; Dev profile |
| `runvm` | bytecode VM; same flags/semantics |
| `check` | type-check; `--json` (LSP foothold); project mode prints scope summary |
| `transpile` | Phorj→PHP |
| `lift` | PHP→Phorj |
| `parse` / `tokenize` | AST / token dump (single-file string path) |
| `disassemble` | bytecode listing |
| `benchmark` | median-of-N 2-backend timing + memory; `--vs-php`, `--json` |
| `build` | standalone executable; `-o`, `--target <triple>` \| `--all` (zigbuild cross: linux musl/aarch64, windows-gnu; apple rejected), `--dev` profile, `--sign` reserved |
| `vendor` | fetch git deps (ONLY network command) |
| `serve` | HTTP server; `--addr` (default 127.0.0.1:8080), `--timeout` (30s), `--workers N` (auto=cores), `--dev` error pages |
| `lsp` | stdio JSON-RPC language server |
| `debug` | interpreter-only REPL debugger; `--dap` for DAP server (M-DX S5) |
| `test` | discover+run `test` blocks (`*.phg` under `tests/`) |
| `format` | formatter; `--check`, `-` stdin |
| `explain <CODE>` | diagnostic explainer |
| `rewrite-new <file>` | internal migration codemod (not in USAGE) |

### PJ-CLI-002 — Global surface
`-h/--help`, `-v/--version`; per-command `--help`; source forms `<file> | - (stdin) | -e/--eval
<code> | -- <path>` (`cli::resolve_source`); self-executing built artifacts bypass the CLI
(`bundle::embedded_program`, `main.rs:16`).

### PJ-CLI-003 — Build profiles (M-DX S0, `src/profile.rs`)
`Dev` / `Release`; observability-only (byte-identity across profiles; assertions NEVER stripped).
run/runvm/test = Dev; build/serve = Release default, `--dev` opt-in; baked into artifacts (env
can't flip a Release binary).

---

## PJ-PROJ — Project Model (`src/manifest.rs`, `src/loader/`, `src/lock.rs`, `src/vendor.rs`)

- **PJ-PROJ-001** `phorj.toml`: `[package]` → `module` (lowercase distributable coordinate),
  `version`, `source` (source root); `[require]` / `[require-dev]` with `{ git, tag|rev }` or
  `"url@tag"` shorthand, exact-pin only (`manifest.rs:38-61`). Detection = walk-up
  (`Project::detect`).
- **PJ-PROJ-002** Packages: mandatory `package A.B;` everywhere (even `-e`/stdin — loose mode is
  `Main`-only); `package Main;` = runnable entry; folder=path; PascalCase; `core` reserved;
  loader-side mangle/resolve pass (backends see rewritten flat AST; transpiler de-mangles into PHP
  brace namespaces).
- **PJ-PROJ-003** Lockfile `phorj.lock`: `[[package]]` → `name git rev hash` (FNV-1a-64 content
  hash; 40-hex SHA pin) — `lock.rs:17-30`.
- **PJ-PROJ-004** `phg vendor`: clone→checkout→copy under `vendor/<vendor>/<package>/`, atomic
  staged swap; run/check/transpile are offline-only (`E-VENDOR-MISSING`, `E-VENDOR-MAIN`).
  Transitive deps NOT supported (documented deferral); `phg build` stays single-file.
- **PJ-PROJ-005** Interop (M8.5): `declare function/class` foreign PHP surfaces, `.d.phg`
  declaration files (`E-DECL-NONFOREIGN`, `E-DECL-PACKAGE`, `E-FOREIGN-RUNTIME` on Rust backends).

---

## PJ-RT — Runtime Model

- **PJ-RT-001** `Value`: 16 variants (`src/value.rs`) — `Int Float Decimal Bool Str Bytes Unit
  Null List Map Set Instance Enum Closure Channel Task`. List/Map/Set = `Rc` COW
  (`make_mut`-in-slot index assign); Instance = shared-mutable; no tracing GC (immutable+acyclic +
  Rc/Drop).
- **PJ-RT-002** `Op`: **73 variants** (`src/chunk.rs`) — typed arith (I/F/D × add/sub/mul/div/rem),
  bitwise 5, cmp 6, `Neg Not BitNot`, stack/locals, jumps, `Concat MakeList MakeMap Index SetIndex
  SetIndexLocal SetPathLocal Len IterElems MakeRange`, concurrency 6 (`Spawn SpawnCall ChannelNew
  ChannelSend ChannelRecv Join`), calls 5 (`CallNative Call CallOverload CallStaticOverload
  Return`), enums/match 4 (`MakeEnum MatchTag GetEnumField Fault`), objects 7 (`MakeInstance
  GetField SetField GetStatic SetStatic CallMethod CallParent`), closures 3 (`MakeClosure CallValue
  IsInstance`), exceptions 3 (`Throw PushHandler PopHandler`). `Fault(FaultMsg)` carries
  `Panic/Todo/Unreachable/Assert(msg)/…` — there is no separate `Op::Assert`.
- **PJ-RT-003** Fault classes (differential harness `tests/differential.rs:64-107`): IntOverflow,
  DivZero, ModZero, StackOverflow, Unsupported, NoField, IndexOob, ForceUnwrap, RangeTooLarge,
  Panic, DecimalInexact, Concurrency (+Other). Byte-identical `run≡runvm` incl. stack traces.
- **PJ-RT-004** Green threads (`src/green/`): single-sourced scheduler kernel (`sched`) driven by
  BOTH backends (identical interleaving); native executor = corosensei stackful coroutines
  (feature `green`, non-wasm); wasm = VM frame-swap. Cooperative, single-threaded, no parallelism.
- **PJ-RT-005** Limits (`src/limits.rs`): call depth 4096, nest depth 512, expr depth 10 000;
  256 MB deep-stack worker (`cli::on_deep_stack`); i64/f64 checked semantics.
- **PJ-RT-006** External deps (`Cargo.toml:44-70`): 4, all optional/gated — `argon2` (Crypto),
  `regex` (Regex), `ctrlc` (serve shutdown), `corosensei` (green threads). Everything else std-only;
  `#![forbid(unsafe_code)]`.

---

## PJ-TOOL — Tooling Beyond the CLI

- **PJ-TOOL-001** LSP (`src/lsp/`, `phg lsp`): full-doc sync, hover, definition, completion
  (trigger `.`), documentSymbol, references, documentHighlight, rename, formatting
  (`lsp/mod.rs:436`) + push diagnostics; cross-file/project-aware.
- **PJ-TOOL-002** DAP debugger (`src/dap.rs`, `phg debug --dap`) + terminal REPL
  (`src/cli/debug_repl.rs`); interpreter-only, Dev-only.
- **PJ-TOOL-003** Editor extensions (`editors/`): VS Code (TextMate grammar + LSP client, packaged
  `.vsix`) and PhpStorm.
- **PJ-TOOL-004** Playground (`playground/` — separate crate + `web/`): WASM build (regex/argon2/
  corosensei gated off; VM frame-swap concurrency; bypasses `on_deep_stack`).
- **PJ-TOOL-005** Quality infra: differential harness (`tests/differential.rs` — run≡runvm≡real-PHP
  oracle, `PHORJ_REQUIRE_PHP=1`, floor PHP 8.5); `conformance/` suite (collections/ddd/diagnostics/
  errors/lang/stdlib/types/web); `selftest/`; `scripts/perf-gate.sh` + `bench/baseline.json`
  (CI perf-regression gate); `phg fmt` comment-preserving formatter (`src/fmt/`); PHP→Phorj lifter
  (`src/lift/` — PHP lexer/parser/AST + printer).

---

## Totals

| Section | Count |
|---|---|
| Reserved keywords | 42 |
| Contextual keywords | 14 |
| TokenKind variants | 98 |
| Item / Stmt / Expr / Pattern variants | 8 / 14 / 30 / 11 (+or-patterns, +guards) |
| Binary / Unary operators | 21 / 3 |
| Checker `Ty` variants | 23 |
| Built-in type words | 17 active + 9 reserved-rejected |
| Native functions / modules | **270 / 27** (+6 injected preludes, 11 injected types) |
| Diagnostic codes | **196** (187 E-, 9 W-) |
| CLI subcommands | 17 public + 1 internal |
| VM Op variants | 73 |
| Value variants | 16 |
| Fault classes | 12 (+Other) |
| LSP capabilities | 9 |

---

## DRIFT — SSOT/doc vs code mismatches (9 findings)

| ID | Finding | Evidence |
|---|---|---|
| DRIFT-01 | SSOT §1 cites "`Op::Assert`" — no such Op exists; assertion is `Op::Fault(FaultMsg::Assert(String))` | `src/chunk.rs:60` vs SSOT line 21 |
| DRIFT-02 | SSOT §4's built-in-type de-reservation inventory omits 9 reserved numeric words (`double`, `i8`–`i64`, `u8`–`u64`) that sit in `is_builtin_type_name` and are rejected at use — any "de-reserve" plan must account for them | `checker/common.rs:314-322`, `checker/resolve.rs:224` |
| DRIFT-03 | SSOT §2 "Today: flat two-level `import`" is accurate only for stdlib `Core.*`; user-package import paths are already arbitrary-depth (folder=path) and `import type` takes ≥3 segments | `loader/mod.rs:479-495`, `parser/items.rs:164-199` |
| DRIFT-04 | `native/mod.rs` doc comments say "~166 registry literals" and "~50 NativeFn literals" — actual registry is **270** | `native/mod.rs` Deprecated + NativeDefault docs vs counted registry |
| DRIFT-05 | Stale pre-rename comments: `main.rs:22` says `Core.Process.args()` (native is `arguments`); `main.rs:428` comment says "parse, lex, disasm, and bench" (commands are `tokenize/disassemble/benchmark`); `checker/calls.rs:1062` doc says `Channel.new()` (API is `Channel.create()`) | cited lines |
| DRIFT-06 | `checker/casing.rs:398` comment claims "Cross-package types do not exist yet (E-PKG-TYPE)" — the gate is retired and cross-package types shipped | `loader/mod.rs:261`, `transpile/expr.rs:163` (same staleness) |
| DRIFT-07 | Project docs (`CLAUDE.md` header, possibly README) still describe the implementation as "std-only, no external crates" — Cargo.toml carries 4 vetted optional deps (argon2, regex, ctrlc, corosensei) | `Cargo.toml:44-70` |
| DRIFT-08 | `explain.rs` retains retired codes (`E-PKG-TYPE`, `E-DECIMAL-DIV`) whose explain text describes lifted restrictions; only E-DECIMAL-DIV self-labels as historical | `cli/explain.rs:92`, explain titles |
| DRIFT-09 | SSOT §3 "EXISTS (verified)" claims for aliasing hold (`loader/mod.rs` `user_import_map`, alias.or(last)) — confirmed, but cited lines 477–492 have shifted slightly (~475–495); everything else in SSOT marked "not implemented yet" is indeed absent (no `E-UNIMPORTED`, no deep imports, no `Core.Async`, intrinsics still import-free) | verified by grep |

*Verification basis: every count and list above extracted from source by grep/AST-block extraction
in this session (commit `ccb2403` tree); no runtime execution of the registry (270 = static count
incl. 43 macro-generated Html tag natives + 9 factory-generated Hash/Validation natives).*
