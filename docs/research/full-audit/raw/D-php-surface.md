# D — PHP 8.5 Reference Surface (Side A of bidirectional gap analysis)

> **Agent D output — full-audit fleet.** Pinned reference: **PHP 8.5** (released 2025-11), with
> 8.0–8.5 features version-tagged inline. Produced from model knowledge (cutoff 2026-01), no
> network, no Phorj-side inspection (independence requirement). Stable item IDs (`SYN-*`,
> `FN-<GRP>-*`, `RT-*`, `DEF-*`) for machine-diffing against the Phorj inventory (side B).
> Confidence: core surface [Verified against training knowledge of php.net docs]; a handful of
> 8.5-specific items are tagged `(8.5)` and should be spot-checked against the 8.5 changelog.

---

## PART 1 — LANGUAGE SYNTAX & SEMANTICS

### 1.1 Program structure & top-level

- **SYN-001** `<?php … ?>` open/close tags; text outside tags emitted verbatim (native template mode)
- **SYN-002** Short echo tag `<?= expr ?>` (always available)
- **SYN-003** Short open tag `<?` (ini `short_open_tag`, discouraged)
- **SYN-004** `echo e1, e2, …` and `print expr` (print is an expression returning 1)
- **SYN-005** Statement terminator `;`; blocks `{}`; closing `?>` implies `;`
- **SYN-006** Comments: `//`, `#` (line), `/* … */` (block); `#[` starts an attribute, not a comment (8.0)
- **SYN-007** `declare(strict_types=1)` — per-file scalar type coercion switch
- **SYN-008** `declare(ticks=N)` + `register_tick_function`
- **SYN-009** `declare(encoding=…)`
- **SYN-010** `include` / `include_once` (warning on failure)
- **SYN-011** `require` / `require_once` (fatal on failure)
- **SYN-012** Included file may `return` a value to the includer
- **SYN-013** `eval(string)` — compile & run code at runtime
- **SYN-014** `exit` / `die` with int status or string message (proper function with default arg since 8.4)
- **SYN-015** Backtick execution operator `` `cmd` `` (≡ `shell_exec`)
- **SYN-016** CLI shebang `#!/usr/bin/env php` support
- **SYN-017** `__halt_compiler()` + `__COMPILER_HALT_OFFSET__` (data-after-code, Phar stubs)

### 1.2 Variables & constants

- **SYN-018** `$name` sigil; variable names case-SENSITIVE (function/class names case-insensitive)
- **SYN-019** Variable variables `$$a`, `${expr}` (as variable, not interpolation)
- **SYN-020** Variable functions `$f()`, variable methods `$o->$m()`, dynamic props `$o->{$e}`
- **SYN-021** Dynamic class names `new $cls(…)`, `$cls::$prop`, `$cls::m()`
- **SYN-022** `global $x` keyword (binds local name to global scope)
- **SYN-023** `static $x = init` inside functions (persistent locals; per-class not per-bind in methods since 8.1)
- **SYN-024** `const NAME = expr` at namespace scope (compile-time constants)
- **SYN-025** `define('NAME', value)` runtime constants; `defined()`, `constant()`
- **SYN-026** Magic constants: `__LINE__`, `__FILE__`, `__DIR__`, `__FUNCTION__`, `__CLASS__`, `__TRAIT__`, `__METHOD__`, `__NAMESPACE__`
- **SYN-027** Superglobals: `$GLOBALS`, `$_SERVER`, `$_GET`, `$_POST`, `$_FILES`, `$_COOKIE`, `$_SESSION`, `$_REQUEST`, `$_ENV` — auto-visible in every scope, mutable
- **SYN-028** `$GLOBALS` semantics restricted 8.1 (read-only copy; no whole-array write)
- **SYN-029** Magic local `$http_response_header` (populated by http:// stream reads)
- **SYN-030** Predefined constant families: `PHP_VERSION*`, `PHP_INT_MAX/MIN/SIZE`, `PHP_FLOAT_*`, `PHP_EOL`, `PHP_OS(_FAMILY)`, `DIRECTORY_SEPARATOR`, `PATH_SEPARATOR`, `PHP_BINARY`, `PHP_SAPI`, `PHP_BUILD_DATE` (8.5)

### 1.3 Literals & core types

- **SYN-031** Integer literals: decimal, hex `0x`, binary `0b`, legacy octal `0755`, explicit octal `0o755` (8.1), numeric separator `1_000_000` (7.4)
- **SYN-032** Float literals: `1.5`, `.5`, `1e3`, `1E-3`; IEEE-754 double; overflow int→float silently
- **SYN-033** Single-quoted strings (only `\\` and `\'` escapes)
- **SYN-034** Double-quoted strings: escapes `\n \r \t \v \e \f \\ \$ \" \0..\777 \xHH \u{HHHH}` (unicode escape 7.0)
- **SYN-035** Simple interpolation: `"$var"`, `"$arr[key]"` (unquoted key), `"$obj->prop"`
- **SYN-036** Complex interpolation `"{$expr}"` (any dereferencable expression)
- **SYN-037** `"${var}"` / `"${expr}"` interpolation DEPRECATED 8.2
- **SYN-038** Heredoc `<<<ID … ID;` (interpolating; flexible closing-marker indentation 7.3)
- **SYN-039** Nowdoc `<<<'ID'` (non-interpolating)
- **SYN-040** Array literals `[…]` / `array(…)`, `k => v` pairs, mixed int/string keys, nested, trailing comma
- **SYN-041** `null`, `true`, `false` (keywords, case-insensitive)
- **SYN-042** Callable value forms: `'fname'`, `[$obj,'m']`, `['Cls','m']`, `'Cls::m'`, Closure, invokable object, first-class-callable
- **SYN-043** Casts: `(int)/(integer)`, `(float)/(double)`, `(string)`, `(bool)/(boolean)`, `(array)`, `(object)`; binary string prefix `b"…"` (no-op); `(unset)` removed 8.0; `(void)` discard cast (8.5)
- **SYN-044** Type set: scalar `int|float|string|bool`; compound `array|object|callable|iterable`; special `null|resource`; type-syntax-only `void|never|mixed|static|self|parent|false|true`
- **SYN-045** Type juggling / implicit coercion rules; saner string↔number comparison (8.0: `0 == "foo"` is false)
- **SYN-046** Numeric string semantics unified 8.0 (trailing whitespace allowed, leading already); non-numeric string arithmetic → `TypeError`/warning
- **SYN-047** Array keys coerced: `"1"`→1, `1.9`→1 (deprecated float-key truncation warning 8.1), `true`→1, `null`→`""`
- **SYN-048** Resources (opaque handles) — progressively replaced by final opaque classes: `GdImage`, `CurlHandle`, `Socket`, `XMLParser`, `OpenSSLCertificate`, `PgSql\Connection` … (8.0/8.1)

### 1.4 Operators

- **SYN-049** Arithmetic `+ - * / % **`; `/` yields float unless evenly divisible ints; `%` int-only (coerces); `intdiv()`/`fdiv()` complements
- **SYN-050** Unary `+ -`; increment/decrement `++ --` pre/post (alphanumeric string increment quirk `"a"++ → "b"`, `"z"++ → "aa"`; decrement on strings no-op; ++/-- on null asymmetric)
- **SYN-051** String concatenation `.` and `.=` (precedence lowered below `+`/`-` in 8.0)
- **SYN-052** Assignment `=` and compound `+= -= *= /= %= **= .= &= |= ^= <<= >>= ??=`
- **SYN-053** Comparison `== != <> === !== < > <= >=` and spaceship `<=>`
- **SYN-054** Logical `! && ||` and low-precedence `and or xor` (assignment-precedence trap)
- **SYN-055** Bitwise `& | ^ ~ << >>` (on ints; on strings: byte-wise)
- **SYN-056** Ternary `a ? b : c`; short ternary `a ?: c`; nested ternary requires parens (error since 8.0)
- **SYN-057** Null coalescing `??` (isset-semantics, no notice) and `??=`
- **SYN-058** Nullsafe chain `?->` (8.0) — short-circuits entire chain on null
- **SYN-059** Error-suppression operator `@` (8.0: no longer hides fatal errors)
- **SYN-060** `instanceof` (no error on non-object LHS; accepts dynamic class-string RHS)
- **SYN-061** Array union operator `+` (left-biased merge by key — ≠ `array_merge`)
- **SYN-062** **Pipe operator `|>` (8.5)**: `$x |> callable` ≡ `callable($x)`; chains left-to-right; RHS is any single-parameter callable (incl. first-class callable syntax, closures)
- **SYN-063** Spread in calls `f(...$args)` (5.6) — positional; string-keyed spread = named args (8.1)
- **SYN-064** Spread in array literals `[...$a, ...$b]` (7.4); string keys allowed 8.1
- **SYN-065** References: `$b =& $a`; `function &f()`; `function f(&$p)`; `foreach ($a as &$v)`; `[&$x]` in arrays; reference sets & unset() detaching
- **SYN-066** `clone $obj` shallow copy; **`clone($obj, [...properties])` clone-with property updates (8.5)**
- **SYN-067** String offsets `$s[0]`, negative offsets `$s[-1]` (7.1), offset write extends with space-padding
- **SYN-068** Array/string dereference on arbitrary expressions: `f()[0]`, `[1,2][0]`, `"ab"[1]`, constant deref

### 1.5 Control flow

- **SYN-069** `if / elseif / else`; alternative syntax `if(): … endif;`
- **SYN-070** `while` / `do … while`; alternative `while(): endwhile;`
- **SYN-071** `for (init; cond; step)` — comma-separated expression lists in each clause
- **SYN-072** `foreach ($it as $v)` / `as $k => $v` / `as &$v` (by-ref; dangling-ref pitfall) / destructuring `as [$a,$b]` or `as list(…)`
- **SYN-073** `switch` (LOOSE `==` comparison per case) + alternative syntax; fallthrough by default
- **SYN-074** `match (expr) { c1, c2 => r, default => r }` (8.0): expression, STRICT `===`, no fallthrough, multi-condition arms, exhaustive → `UnhandledMatchError`
- **SYN-075** `break N;` / `continue N;` (multi-level)
- **SYN-076** `goto label;` (cannot jump into loops/switch)
- **SYN-077** `return expr;`
- **SYN-078** `throw` is an EXPRESSION (8.0) — usable in `??`, ternary, arrow fns
- **SYN-079** `try / catch (T1|T2 $e) / finally`; multi-catch (7.1); non-capturing `catch (T)` (8.0); finally-overrides-return semantics
- **SYN-080** Generators: `yield`, `yield $k => $v`, `yield from` (delegation, incl. return-value plumbing), `Generator::send/throw/getReturn/current/key/next/rewind/valid`
- **SYN-081** Fibers (8.1): `Fiber` class — `start/suspend/resume/throw/getReturn/isStarted/isRunning/isSuspended/isTerminated`, `Fiber::getCurrent()`; full-stack coroutines (no async syntax)

### 1.6 Functions

- **SYN-082** `function name(params): ret {}` — conditional & nested declaration allowed; names case-insensitive; NO overloading (one symbol per name)
- **SYN-083** Default parameter values (constant expressions; `new Expr()` in initializers 8.1; closures/FCC in const expressions 8.5)
- **SYN-084** Parameter & return type declarations; `strict_types` gating coercion
- **SYN-085** Nullable `?T`
- **SYN-086** Union types `A|B` (8.0), incl. `|null`, literal-type members `false`/`true`
- **SYN-087** Intersection types `A&B` (8.1) — class/interface members only
- **SYN-088** DNF types `(A&B)|C` (8.2)
- **SYN-089** Standalone `true` / `false` / `null` types (8.2)
- **SYN-090** `void`; `never` (8.1, bottom type — throw/exit/infinite); `mixed` (8.0, top type); `static` return (8.0); `self`/`parent` types
- **SYN-091** Variadics `...$args` (typed, optionally by-ref)
- **SYN-092** Named arguments `f(x: 1, y: 2)` (8.0); mixing positional-then-named; parameter NAMES are public API
- **SYN-093** By-reference parameters `&$p`; by-reference returns `function &f()`
- **SYN-094** Anonymous functions (Closure): `function () use ($a, &$b) {}`; `static function` (no `$this` bind)
- **SYN-095** Arrow functions `fn(...) => expr` (7.4): implicit BY-VALUE capture of everything, single expression
- **SYN-096** First-class callable syntax `strlen(...)`, `$obj->m(...)`, `Cls::m(...)` (8.1) → Closure
- **SYN-097** `Closure` API: `bind`, `bindTo`, `call`, `fromCallable`
- **SYN-098** Trailing commas: call args (7.3), parameter lists & closure use lists (8.0)
- **SYN-099** Implicit nullable `T $x = null` DEPRECATED (8.4) — write `?T $x = null`
- **SYN-100** `#[\NoDiscard]` attribute (8.5) — warn when a call's return value is unused; `(void)` cast silences
- **SYN-101** No function autoloading (autoload is class-like only); unqualified fn/const names fall back to global namespace

### 1.7 OOP — classes, members, modifiers

- **SYN-102** `class`, `new C(…)`; `new C()->method()` chaining WITHOUT parens (8.4); `new` with arbitrary expressions `new ($f())(…)` (8.0)
- **SYN-103** Typed properties (7.4) with defaults; uninitialized-typed-property (unset) state
- **SYN-104** Static properties/methods; `::` access; property/method visibility `public/protected/private`
- **SYN-105** Dynamic (undeclared) properties DEPRECATED 8.2; opt back in via `#[\AllowDynamicProperties]`; `stdClass` exempt
- **SYN-106** `readonly` properties (8.1): write-once from declaring scope; `readonly class` (8.2, all props readonly, no dynamic); readonly props re-initializable inside `__clone` (8.3)
- **SYN-107** Asymmetric visibility (8.4): `public private(set) T $p;`, `protected(set)`; implies write barriers, works with hooks
- **SYN-108** Property hooks (8.4): `public T $p { get => …; set(T $v) { … } }`; virtual (backing-less) properties; hooks in interfaces; `parent::$p::get()`
- **SYN-109** Constructor property promotion (8.0): `__construct(private int $x)`; combinable with readonly, hooks (8.4), asymmetric visibility; `final` on promoted props (8.5)
- **SYN-110** Class constants: visibility modifiers (7.1), `final` (8.1), TYPED class constants (8.3), dynamic fetch `C::{$expr}` (8.3); interface constants (overridable until final)
- **SYN-111** `self::` vs `static::` (late static binding) vs `parent::`; `new static`; `static` in return position
- **SYN-112** Single inheritance `extends`; abstract classes & `abstract` methods; `final` classes/methods
- **SYN-113** Covariant return types & contravariant parameter types in overrides (7.4); LSP violations are fatal
- **SYN-114** Interfaces: `implements` many, interfaces `extends` many, constants, (8.4) property declarations via hooks
- **SYN-115** Traits: `use T;`, conflict resolution `insteadof`, aliasing/visibility `as`, abstract trait methods (enforced sigs 8.0), static members, properties, trait CONSTANTS (8.2)
- **SYN-116** Enums (8.1): `enum E { case A; }` pure; backed `enum E: int|string { case A = 1; }`; auto `->name`/`->value`; `E::cases()`, `E::from()`, `E::tryFrom()`; methods, static methods, interfaces, constants, attributes; no state/instantiation
- **SYN-117** Anonymous classes `new class (…) extends/implements … {}` (7.0); readonly anonymous classes (8.3)
- **SYN-118** Attributes `#[Attr(args)]` (8.0): on classes/methods/props/params/consts/functions/enums; grouped `#[A, B]`; `#[Attribute(Attribute::TARGET_*|IS_REPEATABLE)]` meta; reflection `getAttributes()->newInstance()`; constants allowed in attribute args
- **SYN-119** Built-in attributes: `#[Attribute]`, `#[ReturnTypeWillChange]` (8.1), `#[AllowDynamicProperties]` (8.2), `#[SensitiveParameter]` (8.2, redacts traces), `#[Override]` (8.3, override check), `#[Deprecated]` (8.4, userland deprecations), `#[NoDiscard]` (8.5)
- **SYN-120** `::class` constant on names AND objects (`$obj::class`, 8.0)
- **SYN-121** Object model: assignment copies HANDLE (objects by reference-semantics), `==` structural vs `===` identity comparison
- **SYN-122** Lazy objects (8.4): `ReflectionClass::newLazyGhost()` / `newLazyProxy()` + initializer control API
- **SYN-123** Weak references: `WeakReference` (7.4), `WeakMap` (8.0)
- **SYN-124** `Stringable` interface (8.0) — auto-implemented by any class with `__toString`

### 1.8 Magic methods (each its own surface item)

- **SYN-125** `__construct(…)`
- **SYN-126** `__destruct()`
- **SYN-127** `__call($name, $args)` — inaccessible instance method
- **SYN-128** `__callStatic($name, $args)`
- **SYN-129** `__get($name)` — inaccessible/undefined property read
- **SYN-130** `__set($name, $value)`
- **SYN-131** `__isset($name)` (isset/empty triggers)
- **SYN-132** `__unset($name)`
- **SYN-133** `__sleep()` (legacy serialize filter)
- **SYN-134** `__wakeup()` (legacy unserialize hook)
- **SYN-135** `__serialize(): array` (7.4, preferred)
- **SYN-136** `__unserialize(array $data)` (7.4)
- **SYN-137** `__toString()`
- **SYN-138** `__invoke(…)` — object as callable
- **SYN-139** `__set_state(array)` — var_export round-trip
- **SYN-140** `__clone()` — post-clone hook (readonly re-init allowed 8.3)
- **SYN-141** `__debugInfo()` — var_dump output control

### 1.9 Namespaces, imports, autoloading

- **SYN-142** `namespace A\B;` (file-level) and braced `namespace A { … }` (multiple per file legal)
- **SYN-143** `use A\B\C;` / `use function f;` / `use const K;`; aliasing `as`; group use `use A\{B, C as D, function f};` (7.0)
- **SYN-144** Name resolution: fully-qualified `\A\B`, qualified `A\B`, unqualified; functions/constants fall back to global namespace; classes never do
- **SYN-145** `namespace` operator & `__NAMESPACE__`
- **SYN-146** Autoloading model: `spl_autoload_register` stack (classes/interfaces/traits/enums only)
- **SYN-147** Reserved words can be namespace segments? NO — reserved-word rules; `enum`, `readonly` etc. contextual/soft-reserved constraints

### 1.10 Error model

- **SYN-148** `Throwable` root interface; two branches: `Error` (engine) and `Exception` (userland); both uncatchable-by-`Exception` design (7.0)
- **SYN-149** `Error` subclasses: `TypeError`, `ValueError` (8.0), `ArgumentCountError`, `ArithmeticError`, `DivisionByZeroError`, `AssertionError`, `ParseError`, `CompileError`, `UnhandledMatchError` (8.0), `FiberError` (8.1)
- **SYN-150** SPL `Exception` hierarchy: `LogicException` → `BadFunctionCallException` → `BadMethodCallException`, `DomainException`, `InvalidArgumentException`, `LengthException`, `OutOfRangeException`; `RuntimeException` → `OutOfBoundsException`, `OverflowException`, `RangeException`, `UnderflowException`, `UnexpectedValueException`; plus `JsonException`, `RandomException` (8.2), `DateException` family (8.3)
- **SYN-151** `ErrorException` — bridge from error handler to exceptions
- **SYN-152** Non-exception diagnostics: error levels `E_ERROR, E_WARNING, E_NOTICE, E_DEPRECATED, E_USER_*, E_ALL` (E_STRICT deprecated 8.4); `error_reporting`, `display_errors`, `log_errors` inis
- **SYN-153** Handlers: `set_error_handler`, `set_exception_handler`, `restore_*`; `get_error_handler()`/`get_exception_handler()` (8.5)
- **SYN-154** `trigger_error(msg, E_USER_*)`
- **SYN-155** `assert()` + `zend.assertions` (compiled out in prod) + `AssertionError`
- **SYN-156** Exception chaining `new E(msg, code, $previous)`; `getTrace/getTraceAsString/getFile/getLine/getMessage/getCode/getPrevious`
- **SYN-157** Fatal-error path: `register_shutdown_function` + `error_get_last`; fatal errors get backtraces via `fatal_error_backtraces` ini (8.5)
- **SYN-158** Uncaught exception → fatal error with trace; exit code 255

### 1.11 Cross-cutting semantics

- **SYN-159** Arrays are VALUE types with copy-on-write; objects are handle types — the core asymmetry
- **SYN-160** `isset()` (multi-arg), `empty()`, `unset()` — language constructs, not functions; isset false for null
- **SYN-161** Destructuring: `[$a, $b] = $arr;`, `list()`, keyed `['x' => $x] = …`, nested, by-ref `[&$a]`, swap idiom, in foreach
- **SYN-162** `iterable` pseudo-type = `array|Traversable`; `Traversable`/`Iterator`/`IteratorAggregate` protocol; `yield` inside any function makes it a Generator
- **SYN-163** String↔number juggling in array keys: `$a["1"]` and `$a[1]` are the SAME slot
- **SYN-164** Case sensitivity matrix: variables/properties/constants sensitive; functions/methods/classes insensitive
- **SYN-165** Legacy octal pitfall: `0755` vs `755`; `0o` form fixes (8.1)
- **SYN-166** INI-dependent language behavior: `short_open_tag`, `precision`/`serialize_precision`, `zend.assertions`, `error_reporting`, `memory_limit`, `max_execution_time`
- **SYN-167** Output as language feature: anything outside `<?php` + echo goes to output buffer/SAPI stream; `header()` must precede body output
- **SYN-168** Process model: shared-nothing per-request lifecycle (web SAPIs); persistent process in CLI/workers
- **SYN-169** Garbage collection: refcounting + cycle collector (`gc_collect_cycles`, `gc_status`); destructors non-deterministic on cycles
- **SYN-170** `match`/`readonly`/`enum`/`fn` are (semi-)reserved; contextual keyword handling
- **SYN-171** `goto`-free structured alternative idioms — n/a marker (PHP retains goto)
- **SYN-172** Closures over `$this` auto-bind in class scope; `static fn` opts out
- **SYN-173** Deprecation lifecycle: `E_DEPRECATED` → removal at next major; userland `#[\Deprecated]` (8.4)

---

## PART 2 — BUILTIN FUNCTION SURFACE (by extension group)

> Mandatory symbol-by-symbol groups: string, array, math, PCRE, JSON, date/time, filesystem,
> SPL, reflection. Huge groups (intl, gd, sodium, openssl, posix, ftp, expat) are listed as
> families with counts, per brief.

### 2.1 FN-STR — String functions (ext/standard)

- FN-STR-001 strlen
- FN-STR-002 str_contains (8.0)
- FN-STR-003 str_starts_with (8.0)
- FN-STR-004 str_ends_with (8.0)
- FN-STR-005 strpos
- FN-STR-006 stripos
- FN-STR-007 strrpos
- FN-STR-008 strripos
- FN-STR-009 strstr
- FN-STR-010 stristr
- FN-STR-011 strrchr
- FN-STR-012 substr
- FN-STR-013 substr_count
- FN-STR-014 substr_replace
- FN-STR-015 substr_compare
- FN-STR-016 str_replace
- FN-STR-017 str_ireplace
- FN-STR-018 strtr
- FN-STR-019 str_repeat
- FN-STR-020 str_pad
- FN-STR-021 str_split
- FN-STR-022 str_word_count
- FN-STR-023 strrev
- FN-STR-024 strtolower (locale-free since 8.2)
- FN-STR-025 strtoupper
- FN-STR-026 ucfirst
- FN-STR-027 lcfirst
- FN-STR-028 ucwords
- FN-STR-029 trim
- FN-STR-030 ltrim
- FN-STR-031 rtrim / chop (alias)
- FN-STR-032 wordwrap
- FN-STR-033 nl2br
- FN-STR-034 chunk_split
- FN-STR-035 explode
- FN-STR-036 implode / join (alias)
- FN-STR-037 str_getcsv
- FN-STR-038 htmlspecialchars (ENT_QUOTES|ENT_SUBSTITUTE|ENT_HTML401 default since 8.1)
- FN-STR-039 htmlspecialchars_decode
- FN-STR-040 htmlentities
- FN-STR-041 html_entity_decode
- FN-STR-042 get_html_translation_table
- FN-STR-043 strip_tags (array of allowed tags 7.4)
- FN-STR-044 addslashes
- FN-STR-045 stripslashes
- FN-STR-046 addcslashes
- FN-STR-047 stripcslashes
- FN-STR-048 quotemeta
- FN-STR-049 chr
- FN-STR-050 ord
- FN-STR-051 bin2hex
- FN-STR-052 hex2bin
- FN-STR-053 sprintf
- FN-STR-054 printf
- FN-STR-055 vsprintf
- FN-STR-056 vprintf
- FN-STR-057 fprintf
- FN-STR-058 vfprintf
- FN-STR-059 sscanf
- FN-STR-060 number_format
- FN-STR-061 similar_text
- FN-STR-062 soundex
- FN-STR-063 metaphone
- FN-STR-064 levenshtein
- FN-STR-065 strcmp
- FN-STR-066 strcasecmp
- FN-STR-067 strncmp
- FN-STR-068 strncasecmp
- FN-STR-069 strnatcmp
- FN-STR-070 strnatcasecmp
- FN-STR-071 strcoll
- FN-STR-072 strtok
- FN-STR-073 strpbrk
- FN-STR-074 strspn
- FN-STR-075 strcspn
- FN-STR-076 count_chars
- FN-STR-077 crc32
- FN-STR-078 md5 / md5_file
- FN-STR-079 sha1 / sha1_file
- FN-STR-080 crypt
- FN-STR-081 str_rot13
- FN-STR-082 str_shuffle
- FN-STR-083 nl_langinfo
- FN-STR-084 setlocale / localeconv
- FN-STR-085 quoted_printable_encode / quoted_printable_decode
- FN-STR-086 convert_uuencode / convert_uudecode
- FN-STR-087 str_increment (8.3)
- FN-STR-088 str_decrement (8.3)
- FN-STR-089 strval
- FN-STR-090 utf8_encode / utf8_decode — DEPRECATED 8.2 (Latin-1 misnomer)
- FN-STR-091 money_format — REMOVED 8.0 (→ intl NumberFormatter)
- FN-STR-092 hebrev — kept; hebrevc REMOVED 8.0
- FN-STR-093 sprintf %-directive surface (b c d e E f F g G h H o s u x X, width/precision/padding/argnum `%1$s`)

### 2.2 FN-ARR — Array functions (ext/standard)

- FN-ARR-001 array (constructor construct)
- FN-ARR-002 array_change_key_case
- FN-ARR-003 array_chunk
- FN-ARR-004 array_column
- FN-ARR-005 array_combine
- FN-ARR-006 array_count_values
- FN-ARR-007 array_diff
- FN-ARR-008 array_diff_assoc
- FN-ARR-009 array_diff_key
- FN-ARR-010 array_diff_uassoc
- FN-ARR-011 array_diff_ukey
- FN-ARR-012 array_fill
- FN-ARR-013 array_fill_keys
- FN-ARR-014 array_filter (ARRAY_FILTER_USE_KEY / USE_BOTH)
- FN-ARR-015 array_flip
- FN-ARR-016 array_intersect
- FN-ARR-017 array_intersect_assoc
- FN-ARR-018 array_intersect_key
- FN-ARR-019 array_intersect_uassoc
- FN-ARR-020 array_intersect_ukey
- FN-ARR-021 array_is_list (8.1)
- FN-ARR-022 array_key_exists / key_exists (alias)
- FN-ARR-023 array_key_first (7.3)
- FN-ARR-024 array_key_last (7.3)
- FN-ARR-025 array_keys
- FN-ARR-026 array_map (multi-array zip mode; null callback)
- FN-ARR-027 array_merge
- FN-ARR-028 array_merge_recursive
- FN-ARR-029 array_multisort
- FN-ARR-030 array_pad
- FN-ARR-031 array_pop
- FN-ARR-032 array_product
- FN-ARR-033 array_push
- FN-ARR-034 array_rand
- FN-ARR-035 array_reduce
- FN-ARR-036 array_replace
- FN-ARR-037 array_replace_recursive
- FN-ARR-038 array_reverse
- FN-ARR-039 array_search
- FN-ARR-040 array_shift
- FN-ARR-041 array_slice
- FN-ARR-042 array_splice
- FN-ARR-043 array_sum
- FN-ARR-044 array_udiff / array_udiff_assoc / array_udiff_uassoc
- FN-ARR-045 array_uintersect / array_uintersect_assoc / array_uintersect_uassoc
- FN-ARR-046 array_unique
- FN-ARR-047 array_unshift
- FN-ARR-048 array_values
- FN-ARR-049 array_walk
- FN-ARR-050 array_walk_recursive
- FN-ARR-051 array_find (8.4)
- FN-ARR-052 array_find_key (8.4)
- FN-ARR-053 array_any (8.4)
- FN-ARR-054 array_all (8.4)
- FN-ARR-055 array_first (8.5)
- FN-ARR-056 array_last (8.5)
- FN-ARR-057 count / sizeof (alias) — COUNT_RECURSIVE mode; TypeError on non-countable (8.0)
- FN-ARR-058 in_array (loose by default; strict flag)
- FN-ARR-059 range (int/float/char sequences; stricter 8.3)
- FN-ARR-060 compact
- FN-ARR-061 extract (variable injection; EXTR_* flags)
- FN-ARR-062 shuffle (mutates)
- FN-ARR-063 sort / rsort (mutate, reindex)
- FN-ARR-064 asort / arsort (mutate, keep keys)
- FN-ARR-065 ksort / krsort
- FN-ARR-066 usort / uasort / uksort (callback sorts; stable since 8.0)
- FN-ARR-067 natsort / natcasesort
- FN-ARR-068 SORT_* flag surface (REGULAR/NUMERIC/STRING/LOCALE_STRING/NATURAL/FLAG_CASE)
- FN-ARR-069 current / pos (alias)
- FN-ARR-070 key
- FN-ARR-071 next / prev
- FN-ARR-072 reset / end
- FN-ARR-073 each — REMOVED 8.0
- FN-ARR-074 list() — see SYN-161 (construct)

### 2.3 FN-MATH — Math (ext/standard) + arbitrary precision

- FN-MATH-001 abs
- FN-MATH-002 ceil
- FN-MATH-003 floor
- FN-MATH-004 round (+ `RoundingMode` enum 8.4: HalfAwayFromZero/HalfTowardsZero/HalfEven/HalfOdd/TowardsZero/AwayFromZero/NegativeInfinity/PositiveInfinity)
- FN-MATH-005 fmod
- FN-MATH-006 fdiv (8.0 — IEEE division, no DivisionByZeroError)
- FN-MATH-007 intdiv (7.0)
- FN-MATH-008 pow (and `**`)
- FN-MATH-009 sqrt
- FN-MATH-010 exp
- FN-MATH-011 expm1
- FN-MATH-012 log (arbitrary base)
- FN-MATH-013 log10
- FN-MATH-014 log2
- FN-MATH-015 log1p
- FN-MATH-016 pi (M_PI)
- FN-MATH-017 sin / cos / tan
- FN-MATH-018 asin / acos / atan
- FN-MATH-019 atan2
- FN-MATH-020 sinh / cosh / tanh
- FN-MATH-021 asinh / acosh / atanh
- FN-MATH-022 hypot
- FN-MATH-023 deg2rad / rad2deg
- FN-MATH-024 max / min (variadic or array)
- FN-MATH-025 is_nan / is_finite / is_infinite
- FN-MATH-026 base_convert
- FN-MATH-027 bindec / decbin
- FN-MATH-028 hexdec / dechex
- FN-MATH-029 octdec / decoct
- FN-MATH-030 rand / srand / getrandmax (aliases onto Mt19937 since 7.1)
- FN-MATH-031 mt_rand / mt_srand / mt_getrandmax
- FN-MATH-032 random_int (CSPRNG, 7.0)
- FN-MATH-033 random_bytes (CSPRNG, 7.0)
- FN-MATH-034 lcg_value (deprecated 8.4)
- FN-MATH-035 Math constant surface: M_PI, M_E, M_LOG2E, M_LOG10E, M_LN2, M_LN10, M_PI_2, M_PI_4, M_1_PI, M_2_PI, M_SQRTPI, M_2_SQRTPI, M_SQRT2, M_SQRT3, M_SQRT1_2, M_EULER, NAN, INF, PHP_ROUND_HALF_* 
- FN-MATH-036 BCMath family: bcadd, bcsub, bcmul, bcdiv, bcmod, bcpow, bcpowmod, bcsqrt, bccomp, bcscale, bcdivmod (8.4), bcfloor/bcceil/bcround (8.4) — ~14 fns + **`BcMath\Number` object API with operator overloading (8.4)**
- FN-MATH-037 GMP family: ~50 gmp_* functions (arith, bitwise, number theory: gmp_gcd, gmp_nextprime, gmp_prob_prime, gmp_powm …) + GMP object with operator overloading

### 2.4 FN-PCRE — Regex (PCRE2)

- FN-PCRE-001 preg_match (capture groups, named groups, PREG_OFFSET_CAPTURE/UNMATCHED_AS_NULL)
- FN-PCRE-002 preg_match_all (PREG_PATTERN_ORDER/SET_ORDER)
- FN-PCRE-003 preg_replace (arrays of patterns/replacements, `$1`/`\1`/`${1}` refs)
- FN-PCRE-004 preg_replace_callback
- FN-PCRE-005 preg_replace_callback_array
- FN-PCRE-006 preg_filter
- FN-PCRE-007 preg_split (PREG_SPLIT_NO_EMPTY/DELIM_CAPTURE/OFFSET_CAPTURE)
- FN-PCRE-008 preg_grep (PREG_GREP_INVERT)
- FN-PCRE-009 preg_quote
- FN-PCRE-010 preg_last_error / preg_last_error_msg (8.0)
- FN-PCRE-011 Pattern modifier surface: i m s x u (UTF-8) A D S U X J n (8.2 no-auto-capture); delimiters; (?<name>…), lookaround, backtrack limits inis

### 2.5 FN-JSON — JSON

- FN-JSON-001 json_encode (flags: PRETTY_PRINT, UNESCAPED_SLASHES, UNESCAPED_UNICODE, THROW_ON_ERROR, NUMERIC_CHECK, PRESERVE_ZERO_FRACTION, INVALID_UTF8_IGNORE/SUBSTITUTE, PARTIAL_OUTPUT_ON_ERROR, HEX_*)
- FN-JSON-002 json_decode (assoc flag, depth, BIGINT_AS_STRING, OBJECT_AS_ARRAY, THROW_ON_ERROR)
- FN-JSON-003 json_validate (8.3 — validate without decoding)
- FN-JSON-004 json_last_error / json_last_error_msg
- FN-JSON-005 JsonSerializable interface (jsonSerialize())
- FN-JSON-006 JsonException (with JSON_THROW_ON_ERROR)

### 2.6 FN-DATE — Date & time

Classes:
- FN-DATE-001 DateTimeInterface (format/getTimestamp/getTimezone/diff/getOffset/getMicrosecond 8.4)
- FN-DATE-002 DateTime (MUTABLE: modify/add/sub/setDate/setTime/setTimestamp/setTimezone/setISODate/createFromFormat/createFromImmutable/createFromInterface/createFromTimestamp 8.4)
- FN-DATE-003 DateTimeImmutable (same API, returns new instances)
- FN-DATE-004 DateTimeZone (getName/getOffset/getTransitions/listIdentifiers/listAbbreviations/getLocation)
- FN-DATE-005 DateInterval (y m d h i s f invert days; createFromDateString; format())
- FN-DATE-006 DatePeriod (start/interval/end or recurrences; ISO8601 string; createFromISO8601String 8.3)
- FN-DATE-007 DateError/DateException hierarchy (8.3): DateMalformedStringException, DateMalformedIntervalStringException, DateInvalidTimeZoneException, DateInvalidOperationException, …
- FN-DATE-008 Format-character surface: d D j l N S w z W F m M n t L o X x Y y a A B g G h H i s u v e I O P p T Z c r U (+ DATE_ATOM/RFC3339(_EXTENDED)/RFC2822/COOKIE/W3C constants)

Functions:
- FN-DATE-010 date / gmdate
- FN-DATE-011 idate
- FN-DATE-012 time
- FN-DATE-013 mktime / gmmktime
- FN-DATE-014 checkdate
- FN-DATE-015 strtotime (relative-format English parser)
- FN-DATE-016 microtime
- FN-DATE-017 hrtime (7.3 — monotonic nanoseconds)
- FN-DATE-018 gettimeofday
- FN-DATE-019 getdate
- FN-DATE-020 localtime
- FN-DATE-021 date_default_timezone_get / date_default_timezone_set
- FN-DATE-022 date_parse / date_parse_from_format
- FN-DATE-023 date_sun_info
- FN-DATE-024 date_sunrise / date_sunset — DEPRECATED 8.1
- FN-DATE-025 strftime / gmstrftime — DEPRECATED 8.1 (→ IntlDateFormatter)
- FN-DATE-026 strptime — DEPRECATED 8.1
- FN-DATE-027 Procedural OO-aliases family: date_create(_immutable)(_from_format), date_add, date_sub, date_modify, date_diff, date_format, date_timestamp_get/set, date_timezone_get/set, date_offset_get, date_date_set, date_time_set, date_isodate_set, date_get_last_errors, date_interval_create_from_date_string, date_interval_format, timezone_open, timezone_name_get, timezone_offset_get, timezone_transitions_get, timezone_identifiers_list, timezone_abbreviations_list, timezone_location_get, timezone_name_from_abbr, timezone_version_get (~26 aliases)
- FN-DATE-028 sleep / usleep / time_nanosleep / time_sleep_until

### 2.7 FN-FS — Filesystem & directories

- FN-FS-001 fopen (mode surface r r+ w w+ a a+ x x+ c c+ e; b/t flags)
- FN-FS-002 fclose
- FN-FS-003 fread
- FN-FS-004 fwrite / fputs (alias)
- FN-FS-005 fgets
- FN-FS-006 fgetc
- FN-FS-007 fgetcsv / fputcsv (escape param required-ish 8.4 deprecation)
- FN-FS-008 fscanf
- FN-FS-009 feof
- FN-FS-010 fflush
- FN-FS-011 fsync / fdatasync (8.1)
- FN-FS-012 fseek / ftell / rewind
- FN-FS-013 fstat
- FN-FS-014 ftruncate
- FN-FS-015 fpassthru
- FN-FS-016 flock (LOCK_SH/EX/UN/NB)
- FN-FS-017 file (read to array; FILE_IGNORE_NEW_LINES/SKIP_EMPTY_LINES)
- FN-FS-018 file_get_contents (offset/maxlen/context; URL-capable via wrappers)
- FN-FS-019 file_put_contents (FILE_APPEND, LOCK_EX)
- FN-FS-020 readfile
- FN-FS-021 file_exists
- FN-FS-022 is_file / is_dir / is_link
- FN-FS-023 is_readable / is_writable (is_writeable alias) / is_executable
- FN-FS-024 filesize
- FN-FS-025 filetype
- FN-FS-026 filemtime / fileatime / filectime
- FN-FS-027 fileowner / filegroup / fileperms / fileinode
- FN-FS-028 stat / lstat
- FN-FS-029 clearstatcache
- FN-FS-030 touch
- FN-FS-031 chmod / chown / chgrp
- FN-FS-032 lchown / lchgrp
- FN-FS-033 umask
- FN-FS-034 copy
- FN-FS-035 rename
- FN-FS-036 unlink
- FN-FS-037 mkdir (recursive flag)
- FN-FS-038 rmdir
- FN-FS-039 link / symlink / readlink / linkinfo
- FN-FS-040 realpath / realpath_cache_get / realpath_cache_size
- FN-FS-041 basename / dirname (levels param) / pathinfo
- FN-FS-042 tempnam / tmpfile / sys_get_temp_dir
- FN-FS-043 glob (GLOB_BRACE/ONLYDIR/NOSORT…)
- FN-FS-044 fnmatch
- FN-FS-045 disk_free_space (diskfreespace alias) / disk_total_space
- FN-FS-046 parse_ini_file / parse_ini_string (INI_SCANNER_TYPED)
- FN-FS-047 move_uploaded_file / is_uploaded_file
- FN-FS-048 set_file_buffer (≡ stream_set_write_buffer)
- FN-FS-049 popen / pclose (process pipes as streams)
- FN-FS-050 chdir / getcwd
- FN-FS-051 chroot
- FN-FS-052 opendir / readdir / rewinddir / closedir / dir (Directory class)
- FN-FS-053 scandir (sort flags)
- FN-FS-054 fgetss — REMOVED 8.0
- FN-FS-055 include-path helpers: get_include_path / set_include_path / restore_include_path

### 2.8 FN-HASH / FN-CRYPT — Hashing, passwords, crypto

- FN-HASH-001 hash (algo, data, binary, options) — algos incl. md5, sha1, sha2 family, sha3 family, blake2b? (via sodium), xxh32/xxh64/xxh3/xxh128 (8.1), murmur3 (8.1), crc32/b/c, fnv1a32/64, adler32, ripemd, whirlpool, tiger…
- FN-HASH-002 hash_file
- FN-HASH-003 hash_hmac / hash_hmac_file / hash_hmac_algos
- FN-HASH-004 hash_init / hash_update / hash_update_file / hash_update_stream / hash_final / hash_copy (HashContext object, serializable 8.1)
- FN-HASH-005 hash_algos
- FN-HASH-006 hash_equals (timing-safe compare)
- FN-HASH-007 hash_hkdf
- FN-HASH-008 hash_pbkdf2
- FN-CRYPT-001 password_hash (PASSWORD_BCRYPT / PASSWORD_ARGON2I / PASSWORD_ARGON2ID; cost/memory/threads options)
- FN-CRYPT-002 password_verify
- FN-CRYPT-003 password_needs_rehash
- FN-CRYPT-004 password_get_info
- FN-CRYPT-005 password_algos
- FN-CRYPT-006 crypt (legacy DES/MD5/SHA-256/SHA-512/blowfish salts)
- FN-CRYPT-007 **sodium_* family (~110 fns)** — libsodium: crypto_secretbox(_open), crypto_box family (keypair/seal/open), crypto_sign family (detached/ed25519↔curve25519 conversion), crypto_aead_{chacha20poly1305,xchacha20poly1305_ietf,aes256gcm,aegis128l,aegis256 (8.4)}, crypto_pwhash(_str)(_verify) Argon2id, crypto_generichash (BLAKE2b), crypto_shorthash, crypto_kdf, crypto_kx, crypto_stream(_xor), crypto_scalarmult, randombytes_*, memory: sodium_memzero/increment/compare/pad/unpad, hex2bin/bin2hex/base642bin/bin2base64
- FN-CRYPT-008 **openssl_* family (~60 fns)**: openssl_encrypt/decrypt (AEAD tag params), openssl_cipher_iv_length/openssl_cipher_key_length (8.2), openssl_digest, openssl_sign/verify, openssl_seal/open, openssl_pkey_new/get_private/get_public/export/get_details, openssl_public/private_encrypt/decrypt, openssl_x509_* (~10: parse/verify/checkpurpose/fingerprint/export…), openssl_csr_* (new/sign/export…), openssl_pkcs7_*, openssl_pkcs12_*, openssl_cms_* (8.0), openssl_random_pseudo_bytes, openssl_error_string, openssl_get_cipher_methods/curve_names/md_methods, openssl_dh_compute_key/pbkdf2/spki_*

### 2.9 FN-DB — Database bindings

- FN-DB-001 PDO class: __construct(dsn), connect() factory (8.4), prepare, query, exec, lastInsertId, beginTransaction/commit/rollBack/inTransaction, quote, getAttribute/setAttribute, errorCode/errorInfo, getAvailableDrivers
- FN-DB-002 PDOStatement: execute, fetch (FETCH_ASSOC/NUM/BOTH/OBJ/CLASS/INTO/COLUMN/KEY_PAIR/GROUP/UNIQUE/FUNC/NAMED/LAZY), fetchAll, fetchObject, fetchColumn, bindParam/bindValue/bindColumn, rowCount, columnCount, getColumnMeta, nextRowset, setFetchMode, closeCursor, debugDumpParams, getIterator (8.0)
- FN-DB-003 PDOException (SQLSTATE codes); PDO::ERRMODE_EXCEPTION default since 8.0
- FN-DB-004 PDO driver subclasses (8.4): Pdo\Mysql, Pdo\Pgsql, Pdo\Sqlite, Pdo\Firebird, Pdo\Odbc (driver-specific methods e.g. Pdo\Sqlite::createFunction)
- FN-DB-005 mysqli class: connect/real_connect, query, real_query, prepare, multi_query, begin_transaction/commit/rollback, autocommit, select_db, set_charset, escape_string/real_escape_string, insert_id, affected_rows, error/errno/error_list, ping (dep 8.4), stat, ssl_set, options, execute_query (8.2)
- FN-DB-006 mysqli_stmt (prepare/bind_param/bind_result/execute/fetch/get_result/store_result…), mysqli_result (fetch_assoc/fetch_array/fetch_object/fetch_row/fetch_all/fetch_column 8.1…), mysqli_driver (report_mode — exceptions default 8.1), mysqli_warning
- FN-DB-007 mysqli procedural family (~100 mysqli_* function aliases)
- FN-DB-008 SQLite3 / SQLite3Stmt / SQLite3Result classes (exceptions mode 8.3; createFunction/createAggregate/createCollation, openBlob)
- FN-DB-009 pgsql extension family (~90 pg_* fns; resources→objects 8.1)
- FN-DB-010 dba, odbc families (counts: dba_* ~15, odbc_* ~40)

### 2.10 FN-CURL — cURL

- FN-CURL-001 curl_init (returns CurlHandle object 8.0)
- FN-CURL-002 curl_setopt / curl_setopt_array (CURLOPT_* surface ~200 constants: URL, RETURNTRANSFER, POSTFIELDS, HTTPHEADER, FOLLOWLOCATION, SSL_*, TIMEOUT(_MS), PROXY, WRITEFUNCTION/HEADERFUNCTION…)
- FN-CURL-003 curl_exec
- FN-CURL-004 curl_getinfo (CURLINFO_* surface)
- FN-CURL-005 curl_error / curl_errno / curl_strerror
- FN-CURL-006 curl_reset / curl_pause / curl_copy_handle
- FN-CURL-007 curl_escape / curl_unescape
- FN-CURL-008 curl_close (no-op with objects) / curl_version
- FN-CURL-009 curl_upkeep (8.2)
- FN-CURL-010 curl_multi_init/add_handle/remove_handle/exec/select/getcontent/info_read/close/errno/strerror/setopt + **curl_multi_get_handles (8.5)**
- FN-CURL-011 curl_share_init/setopt/close/errno/strerror + **curl_share_init_persistent (8.5, persistent share handles)**
- FN-CURL-012 CURLFile / CURLStringFile (8.1) upload objects
- FN-CURL-013 CurlHandle / CurlMultiHandle / CurlShareHandle opaque classes (8.0)

### 2.11 FN-MB — mbstring (multibyte)

- FN-MB-001 mb_strlen
- FN-MB-002 mb_substr / mb_strcut / mb_strimwidth / mb_strwidth
- FN-MB-003 mb_strpos / mb_stripos / mb_strrpos / mb_strripos
- FN-MB-004 mb_strstr / mb_stristr / mb_strrchr / mb_strrichr
- FN-MB-005 mb_substr_count
- FN-MB-006 mb_str_split (7.4)
- FN-MB-007 mb_strtolower / mb_strtoupper / mb_convert_case (MB_CASE_* incl. TITLE/FOLD)
- FN-MB-008 mb_ucfirst / mb_lcfirst (8.4)
- FN-MB-009 mb_trim / mb_ltrim / mb_rtrim (8.4)
- FN-MB-010 mb_str_pad (8.3)
- FN-MB-011 mb_convert_encoding
- FN-MB-012 mb_detect_encoding / mb_detect_order
- FN-MB-013 mb_check_encoding
- FN-MB-014 mb_scrub (7.2 — invalid byte replacement)
- FN-MB-015 mb_internal_encoding / mb_substitute_character / mb_preferred_mime_name
- FN-MB-016 mb_ord / mb_chr (7.2)
- FN-MB-017 mb_convert_kana
- FN-MB-018 mb_convert_variables
- FN-MB-019 mb_encode_mimeheader / mb_decode_mimeheader
- FN-MB-020 mb_encode_numericentity / mb_decode_numericentity
- FN-MB-021 mb_parse_str / mb_http_input / mb_http_output / mb_output_handler / mb_language / mb_send_mail / mb_get_info / mb_list_encodings / mb_encoding_aliases
- FN-MB-022 mb_ereg family (~11 legacy Oniguruma regex fns: mb_ereg, mb_eregi, mb_ereg_replace, mb_ereg_match, mb_ereg_search*, mb_split, mb_regex_encoding)

### 2.12 FN-ICONV — iconv

- FN-ICONV-001 iconv (charset conversion; //TRANSLIT //IGNORE)
- FN-ICONV-002 iconv_strlen
- FN-ICONV-003 iconv_substr
- FN-ICONV-004 iconv_strpos / iconv_strrpos
- FN-ICONV-005 iconv_mime_encode / iconv_mime_decode / iconv_mime_decode_headers
- FN-ICONV-006 iconv_get_encoding / iconv_set_encoding

### 2.13 FN-SPL — Standard PHP Library

Interfaces:
- FN-SPL-001 Traversable (engine marker)
- FN-SPL-002 Iterator (current/key/next/rewind/valid)
- FN-SPL-003 IteratorAggregate (getIterator)
- FN-SPL-004 ArrayAccess (offsetExists/offsetGet/offsetSet/offsetUnset)
- FN-SPL-005 Countable (count)
- FN-SPL-006 Serializable (DEPRECATED 8.1 → __serialize/__unserialize)
- FN-SPL-007 OuterIterator / RecursiveIterator / SeekableIterator
- FN-SPL-008 SplObserver / SplSubject (observer pattern)

Data structures:
- FN-SPL-010 SplDoublyLinkedList
- FN-SPL-011 SplStack
- FN-SPL-012 SplQueue
- FN-SPL-013 SplHeap / SplMinHeap / SplMaxHeap
- FN-SPL-014 SplPriorityQueue
- FN-SPL-015 SplFixedArray (int-indexed, fixed size)
- FN-SPL-016 SplObjectStorage (object set/map, ArrayAccess)
- FN-SPL-017 ArrayObject / ArrayIterator / RecursiveArrayIterator

File handling:
- FN-SPL-020 SplFileInfo (~30 methods: getPathname/getSize/getMTime/isFile/openFile…)
- FN-SPL-021 SplFileObject (line iteration, CSV, flags)
- FN-SPL-022 SplTempFileObject
- FN-SPL-023 DirectoryIterator / FilesystemIterator
- FN-SPL-024 RecursiveDirectoryIterator
- FN-SPL-025 GlobIterator

Iterators:
- FN-SPL-030 IteratorIterator
- FN-SPL-031 AppendIterator
- FN-SPL-032 CachingIterator / RecursiveCachingIterator
- FN-SPL-033 FilterIterator / RecursiveFilterIterator
- FN-SPL-034 CallbackFilterIterator / RecursiveCallbackFilterIterator
- FN-SPL-035 LimitIterator
- FN-SPL-036 InfiniteIterator / NoRewindIterator / EmptyIterator
- FN-SPL-037 MultipleIterator (parallel iteration)
- FN-SPL-038 ParentIterator
- FN-SPL-039 RecursiveIteratorIterator (SELF_FIRST/CHILD_FIRST/LEAVES_ONLY)
- FN-SPL-040 RegexIterator / RecursiveRegexIterator
- FN-SPL-041 RecursiveTreeIterator

Functions:
- FN-SPL-050 spl_autoload_register / spl_autoload_unregister / spl_autoload_functions
- FN-SPL-051 spl_autoload / spl_autoload_call / spl_autoload_extensions
- FN-SPL-052 spl_object_hash / spl_object_id
- FN-SPL-053 iterator_to_array / iterator_count / iterator_apply
- FN-SPL-054 class_implements / class_parents / class_uses

### 2.14 FN-CTYPE — Character type checks

- FN-CTYPE-001 ctype_alnum
- FN-CTYPE-002 ctype_alpha
- FN-CTYPE-003 ctype_cntrl
- FN-CTYPE-004 ctype_digit
- FN-CTYPE-005 ctype_graph
- FN-CTYPE-006 ctype_lower
- FN-CTYPE-007 ctype_print
- FN-CTYPE-008 ctype_punct
- FN-CTYPE-009 ctype_space
- FN-CTYPE-010 ctype_upper
- FN-CTYPE-011 ctype_xdigit (all: non-string args deprecated 8.1)

### 2.15 FN-FILTER — filter extension

- FN-FILTER-001 filter_var
- FN-FILTER-002 filter_var_array
- FN-FILTER-003 filter_input (INPUT_GET/POST/COOKIE/SERVER/ENV)
- FN-FILTER-004 filter_input_array
- FN-FILTER-005 filter_has_var
- FN-FILTER-006 filter_id / filter_list
- FN-FILTER-007 Validate filters: FILTER_VALIDATE_INT/FLOAT/BOOL/EMAIL/URL/IP/MAC/DOMAIN/REGEXP
- FN-FILTER-008 Sanitize filters: FILTER_SANITIZE_EMAIL/URL/NUMBER_INT/NUMBER_FLOAT/SPECIAL_CHARS/FULL_SPECIAL_CHARS/ADD_SLASHES (FILTER_SANITIZE_STRING removed 8.1→dep)
- FN-FILTER-009 Flags: FILTER_FLAG_IPV4/IPV6/NO_PRIV_RANGE/NO_RES_RANGE/GLOBAL_RANGE (8.2)/ALLOW_FRACTION/…; FILTER_NULL_ON_FAILURE; options min_range/max_range/default

### 2.16 FN-SESS — Sessions

- FN-SESS-001 session_start (options array)
- FN-SESS-002 session_id / session_name / session_create_id
- FN-SESS-003 session_regenerate_id
- FN-SESS-004 session_destroy / session_unset / session_abort / session_reset
- FN-SESS-005 session_write_close / session_commit (alias)
- FN-SESS-006 session_status (DISABLED/NONE/ACTIVE)
- FN-SESS-007 session_get_cookie_params / session_set_cookie_params (SameSite support 7.3)
- FN-SESS-008 session_set_save_handler + SessionHandler / SessionHandlerInterface / SessionIdInterface / SessionUpdateTimestampHandlerInterface
- FN-SESS-009 session_save_path / session_module_name / session_cache_limiter / session_cache_expire / session_gc / session_encode / session_decode
- FN-SESS-010 $_SESSION superglobal binding; ini surface (session.cookie_httponly, cookie_secure, use_strict_mode, sid_length…)

### 2.17 FN-STREAM — Streams, contexts, wrappers, filters

- FN-STREAM-001 stream_context_create / get_default / set_default
- FN-STREAM-002 stream_context_get_options / set_option / set_options / get_params / set_params
- FN-STREAM-003 stream_copy_to_stream
- FN-STREAM-004 stream_get_contents / stream_get_line / stream_get_meta_data
- FN-STREAM-005 stream_select (multiplexing)
- FN-STREAM-006 stream_set_blocking / stream_set_timeout / stream_set_chunk_size / stream_set_read_buffer / stream_set_write_buffer
- FN-STREAM-007 stream_socket_client / stream_socket_server / stream_socket_accept
- FN-STREAM-008 stream_socket_pair / stream_socket_get_name / stream_socket_recvfrom / stream_socket_sendto / stream_socket_shutdown
- FN-STREAM-009 stream_socket_enable_crypto (TLS upgrade)
- FN-STREAM-010 stream_filter_append / prepend / remove; stream_filter_register (php_user_filter); stream_get_filters
- FN-STREAM-011 stream_wrapper_register / unregister / restore (userland protocol classes: streamWrapper method contract ~15 methods)
- FN-STREAM-012 stream_get_wrappers / stream_get_transports / stream_resolve_include_path / stream_is_local / stream_isatty / stream_supports_lock / sapi_windows_vt100_support
- FN-STREAM-013 Built-in wrappers: file://, http://, https://, ftp://, ftps://, php:// (stdin/stdout/stderr/input/output/fd/memory/temp/filter), data://, glob://, phar://, zlib://, compress.zlib://, compress.bzip2://
- FN-STREAM-014 Context option namespaces: http (method/header/content/timeout/proxy/follow_location), ssl (verify_peer/cafile/peer_fingerprint/SNI), ftp, phar, zip, socket (bindto/backlog/so_reuseport)
- FN-STREAM-015 Built-in filters: string.rot13, string.toupper/tolower, convert.base64-*, convert.quoted-printable-*, zlib.deflate/inflate, bzip2.*, dechunk, consumed

### 2.18 FN-SOCK — Sockets (ext/sockets)

- FN-SOCK-001 socket_create / socket_create_pair / socket_create_listen (Socket object 8.0)
- FN-SOCK-002 socket_bind / socket_listen / socket_accept / socket_connect
- FN-SOCK-003 socket_read / socket_write / socket_recv / socket_send / socket_recvfrom / socket_sendto / socket_recvmsg / socket_sendmsg
- FN-SOCK-004 socket_select
- FN-SOCK-005 socket_set_option / socket_get_option (SO_* surface) / socket_set_block / socket_set_nonblock
- FN-SOCK-006 socket_last_error / socket_strerror / socket_clear_error
- FN-SOCK-007 socket_shutdown / socket_close
- FN-SOCK-008 socket_getpeername / socket_getsockname
- FN-SOCK-009 socket_import_stream / socket_export_stream
- FN-SOCK-010 socket_atmark / socket_cmsg_space / socket_wsaprotocol_info_* (Windows)

### 2.19 FN-XML — XML processing

- FN-XML-001 DOMDocument (loadXML/loadHTML/save/saveXML/saveHTML/createElement/importNode/getElementById/schemaValidate/xinclude…)
- FN-XML-002 DOM node classes: DOMNode, DOMElement, DOMAttr, DOMText, DOMComment, DOMCdataSection, DOMDocumentFragment, DOMDocumentType, DOMEntity, DOMEntityReference, DOMProcessingInstruction, DOMNotation, DOMNameSpaceNode
- FN-XML-003 DOMNodeList / DOMNamedNodeMap (Traversable)
- FN-XML-004 DOMXPath (query/evaluate/registerNamespace/registerPhpFunctions + callable registration 8.4)
- FN-XML-005 DOMImplementation; modern DOM living-standard props (parentElement, childElementCount, append/prepend/replaceWith… 8.0+)
- FN-XML-006 **New DOM API (8.4): `Dom\HTMLDocument` (HTML5 parser!), `Dom\XMLDocument`, `Dom\Node`… namespace — spec-compliant, querySelector/querySelectorAll**
- FN-XML-007 simplexml_load_string / simplexml_load_file / simplexml_import_dom; SimpleXMLElement (attributes/children/xpath/addChild/asXML; ArrayAccess/iteration), SimpleXMLIterator
- FN-XML-008 XMLReader (pull parser: read/next/moveToAttribute/expand/open/xml/isValid; fromStream/fromString 8.4)
- FN-XML-009 XMLWriter (streaming writer: openMemory/openUri/startElement/writeAttribute/…; toStream/toMemory 8.4)
- FN-XML-010 Expat family: xml_parser_create(_ns), xml_parse, xml_parse_into_struct, xml_set_element_handler + 8 more handler setters, xml_get_error_code/error_string/current_*_number, xml_parser_get/set_option (~20 fns; XMLParser object 8.0)
- FN-XML-011 libxml_* : use_internal_errors, get_errors, get_last_error, clear_errors, set_streams_context, set_external_entity_loader/get_external_entity_loader (8.4); entity-loader hardening (LIBXML_NOENT etc.; libxml_disable_entity_loader deprecated 8.0 — XXE safe-by-default with libxml≥2.9)
- FN-XML-012 XSLTProcessor (ext/xsl) — importStylesheet/transformToXml/registerPHPFunctions (callable array 8.4)

### 2.20 FN-FINFO — fileinfo

- FN-FINFO-001 finfo class (finfo_open) — FILEINFO_MIME_TYPE/MIME_ENCODING/…
- FN-FINFO-002 finfo_file / finfo_buffer (+ OO file()/buffer())
- FN-FINFO-003 finfo_close / finfo_set_flags
- FN-FINFO-004 mime_content_type

### 2.21 FN-ZLIB / FN-ZIP / FN-PHAR — Compression & archives

- FN-ZLIB-001 gzencode / gzdecode (gzip format)
- FN-ZLIB-002 gzcompress / gzuncompress (zlib format)
- FN-ZLIB-003 gzdeflate / gzinflate (raw deflate)
- FN-ZLIB-004 zlib_encode / zlib_decode / zlib_get_coding_type
- FN-ZLIB-005 gz-file family: gzopen/gzread/gzwrite/gzgets/gzgetc/gzeof/gzclose/gzseek/gztell/gzrewind/gzpassthru/gzfile/readgzfile (~13)
- FN-ZLIB-006 Incremental: deflate_init/deflate_add, inflate_init/inflate_add, inflate_get_status/inflate_get_read_len
- FN-ZLIB-007 bzip2 family: bzopen/bzread/bzwrite/bzclose/bzcompress/bzdecompress/bzerrno/bzerrstr/bzerror/bzflush (~10)
- FN-ZIP-001 ZipArchive class (~50 methods: open/close/addFile/addFromString/addGlob/extractTo/getFromName/getStream/setPassword/setEncryptionName/setCompressionName/deleteName/renameName/registerProgressCallback/registerCancelCallback…) — procedural zip_* REMOVED-deprecated 8.0
- FN-PHAR-001 Phar / PharData / PharFileInfo classes (build/extract/compress/setStub/setSignatureAlgorithm/webPhar; phar.readonly ini); phar:// wrapper; unserialization-hardened metadata (8.0)

### 2.22 FN-INTL — intl (ICU) — families with counts

- FN-INTL-001 Collator (~15 methods) + collator_* aliases
- FN-INTL-002 NumberFormatter (~15; DECIMAL/CURRENCY/PERCENT/SPELLOUT…) + numfmt_*
- FN-INTL-003 MessageFormatter (ICU MessageFormat, plural/select) + msgfmt_*
- FN-INTL-004 IntlDateFormatter (~15) + datefmt_*; IntlDatePatternGenerator (8.1)
- FN-INTL-005 Locale static class (~20: getDefault/lookup/filterMatches/parseLocale/getDisplayName…; **Locale::isRightToLeft / locale_is_right_to_left (8.5)**)
- FN-INTL-006 Normalizer (NFC/NFD/NFKC/NFKD; isNormalized; getRawDecomposition)
- FN-INTL-007 IntlCalendar / IntlGregorianCalendar (~50 methods)
- FN-INTL-008 IntlTimeZone (~30)
- FN-INTL-009 IntlChar (~60 static: charName/charType/isU*/ord/chr/foldCase/charAge…)
- FN-INTL-010 IntlBreakIterator / IntlRuleBasedBreakIterator / IntlCodePointBreakIterator (word/sentence/line segmentation)
- FN-INTL-011 Transliterator (any-to-any script transforms, ~6 methods)
- FN-INTL-012 Spoofchecker (confusable detection)
- FN-INTL-013 UConverter (~20)
- FN-INTL-014 ResourceBundle
- FN-INTL-015 grapheme_* functions (~10: grapheme_strlen/substr/strpos/stripos/strrpos/extract/strstr/stristr + **grapheme_str_split (8.4)**, grapheme_levenshtein (8.5?) — cluster-aware string ops)
- FN-INTL-016 idn_to_ascii / idn_to_utf8 (IDNA2008)
- FN-INTL-017 intl_get_error_code / intl_get_error_message / intl_is_failure / intl_error_name
- FN-INTL-018 **IntlListFormatter (8.5)** — "a, b, and c" locale list formatting

### 2.23 FN-GD — Image processing (families with counts)

- FN-GD-001 GdImage object (8.0), GdFont (8.1); imagecreate/imagecreatetruecolor + imagecreatefrom{png,jpeg,gif,webp,bmp,wbmp,xbm,xpm,tga,avif(8.1),gd,gd2,string} (~14 constructors)
- FN-GD-002 Output: imagepng/imagejpeg/imagegif/imagewebp/imageavif (8.1)/imagebmp/imagewbmp/imagegd/imagegd2 (~9)
- FN-GD-003 Drawing family (~25): imagesetpixel, imageline, imagerectangle, imagefilledrectangle, imageellipse, imagefilledellipse, imagearc, imagefilledarc, imagepolygon, imagefilledpolygon, imagechar, imagestring, imagettftext/imagefttext, imagefill, imagefilltoborder…
- FN-GD-004 Color family (~15): imagecolorallocate(alpha), imagecolorat, imagecolorclosest, imagecolorexact, imagecolorset, imagecolortransparent, imagecolorstotal, imagepalettetotruecolor…
- FN-GD-005 Transform family (~20): imagecopy, imagecopyresized, imagecopyresampled, imagecopymerge, imagerotate, imagescale, imagecrop, imagecropauto, imageflip, imagesetinterpolation, imageaffine…
- FN-GD-006 Filter/misc (~20): imagefilter (IMG_FILTER_*), imageconvolution, imagegammacorrect, imagesx/imagesy, imageistruecolor, imagesetthickness, imagesetstyle, imagealphablending, imagesavealpha, imageinterlace, getimagesize, getimagesizefromstring, image_type_to_extension/mime_type, imageresolution
- FN-GD-007 Total surface ≈ 110 functions; ext/exif companions: exif_read_data, exif_imagetype, exif_thumbnail, exif_tagname

### 2.24 FN-REFL — Reflection

- FN-REFL-001 ReflectionClass (~60 methods: newInstance(Args), getMethods/getProperties/getConstants, getAttributes, isInterface/isEnum/isReadOnly (8.2), newLazyGhost/newLazyProxy (8.4)…)
- FN-REFL-002 ReflectionObject
- FN-REFL-003 ReflectionMethod (invoke/getModifiers/createFromMethodName 8.3)
- FN-REFL-004 ReflectionFunction / ReflectionFunctionAbstract (invoke/getClosure/isAnonymous 8.2)
- FN-REFL-005 ReflectionParameter (getType/isOptional/getDefaultValue/isPromoted 8.0)
- FN-REFL-006 ReflectionProperty (getValue/setValue/getType/isReadOnly/isVirtual+hooks introspection 8.4/setRawValueWithoutLazyInitialization 8.4)
- FN-REFL-007 ReflectionClassConstant (getType 8.3 / isEnumCase)
- FN-REFL-008 **ReflectionConstant (8.5) — global/namespace constants**
- FN-REFL-009 ReflectionEnum / ReflectionEnumUnitCase / ReflectionEnumBackedCase (8.1)
- FN-REFL-010 ReflectionType hierarchy: ReflectionNamedType, ReflectionUnionType (8.0), ReflectionIntersectionType (8.1)
- FN-REFL-011 ReflectionAttribute (8.0: getName/getArguments/newInstance/IS_INSTANCEOF)
- FN-REFL-012 ReflectionGenerator; ReflectionFiber (8.1)
- FN-REFL-013 ReflectionReference (7.4)
- FN-REFL-014 ReflectionExtension / ReflectionZendExtension
- FN-REFL-015 Reflection / Reflector / ReflectionException

### 2.25 FN-RAND — Random extension (8.2)

- FN-RAND-001 Random\Randomizer: getInt, nextInt, getBytes, shuffleArray, shuffleBytes, pickArrayKeys, getBytesFromString (8.3), getFloat/nextFloat (8.3, IntervalBoundary enum)
- FN-RAND-002 Random\Engine / Random\CryptoSafeEngine interfaces
- FN-RAND-003 Engines: Random\Engine\Mt19937, Random\Engine\PcgOneseq128XslRr64, Random\Engine\Xoshiro256StarStar, Random\Engine\Secure
- FN-RAND-004 Random\RandomError / Random\BrokenRandomEngineError / Random\RandomException

### 2.26 FN-PROC — Processes, signals, program execution

- FN-PROC-001 exec / system / passthru / shell_exec
- FN-PROC-002 escapeshellarg / escapeshellcmd
- FN-PROC-003 proc_open (descriptor spec, pipes, env, cwd; array-command bypass-shell form) / proc_close / proc_terminate / proc_get_status / proc_nice
- FN-PROC-004 pcntl_fork / pcntl_exec
- FN-PROC-005 pcntl_signal / pcntl_signal_dispatch / pcntl_async_signals / pcntl_signal_get_handler
- FN-PROC-006 pcntl_alarm
- FN-PROC-007 pcntl_wait / pcntl_waitpid + status macros pcntl_wifexited/wifstopped/wifsignaled/wexitstatus/wtermsig/wstopsig
- FN-PROC-008 pcntl_sigprocmask / pcntl_sigwaitinfo / pcntl_sigtimedwait
- FN-PROC-009 pcntl_getpriority / pcntl_setpriority; pcntl_unshare (8.1); pcntl_getcpu*/setcpu* affinity (8.3)
- FN-PROC-010 posix_* family (~40: getpid, getppid, getuid/geteuid/setuid, getgid/…, kill, getpwnam/getpwuid, getgrnam/getgrgid, uname, times, getrlimit/setrlimit, isatty, ttyname, mkfifo, mknod, access, errno/strerror, eaccess (8.3)…)
- FN-PROC-011 getmypid / getmyuid / getmygid / getmyinode / get_current_user
- FN-PROC-012 getenv / putenv / $_ENV
- FN-PROC-013 getopt (CLI argument parsing)
- FN-PROC-014 sys_getloadavg
- FN-PROC-015 ignore_user_abort / connection_aborted / connection_status
- FN-PROC-016 set_time_limit / max_execution_time model

### 2.27 FN-OB — Output buffering & control

- FN-OB-001 ob_start (callback, chunk size, flags)
- FN-OB-002 ob_get_contents / ob_get_length / ob_get_level / ob_get_status
- FN-OB-003 ob_get_clean / ob_get_flush
- FN-OB-004 ob_end_clean / ob_end_flush / ob_clean / ob_flush
- FN-OB-005 flush (SAPI-level)
- FN-OB-006 ob_implicit_flush
- FN-OB-007 ob_gzhandler
- FN-OB-008 ob_list_handlers
- FN-OB-009 output_add_rewrite_var / output_reset_rewrite_vars
- FN-OB-010 HTTP header surface: header, header_remove, headers_sent, headers_list, http_response_code, setcookie, setrawcookie; **http_get_last_response_headers / http_clear_last_response_headers (8.4, for http-wrapper calls)**; request_parse_body (8.4)

### 2.28 FN-VAR — Variable handling & type inspection

- FN-VAR-001 var_dump
- FN-VAR-002 var_export (round-trippable PHP literal; __set_state)
- FN-VAR-003 print_r (return mode)
- FN-VAR-004 serialize / unserialize (allowed_classes option; max_depth) — object (de)serialization incl. __serialize/__unserialize
- FN-VAR-005 gettype / get_debug_type (8.0, canonical names)
- FN-VAR-006 settype
- FN-VAR-007 intval (base param) / floatval / doubleval / boolval / strval
- FN-VAR-008 is_int / is_integer / is_long
- FN-VAR-009 is_float / is_double
- FN-VAR-010 is_string
- FN-VAR-011 is_bool
- FN-VAR-012 is_array
- FN-VAR-013 is_object
- FN-VAR-014 is_null
- FN-VAR-015 is_numeric
- FN-VAR-016 is_callable / is_iterable (7.1) / is_countable (7.3)
- FN-VAR-017 is_scalar
- FN-VAR-018 is_resource
- FN-VAR-019 get_defined_vars
- FN-VAR-020 get_object_vars / get_mangled_object_vars (7.4)
- FN-VAR-021 get_class ($this-less deprecated 8.3) / get_parent_class / get_called_class
- FN-VAR-022 get_class_methods / method_exists / property_exists
- FN-VAR-023 class_exists / interface_exists / trait_exists / enum_exists (8.1)
- FN-VAR-024 is_a / is_subclass_of
- FN-VAR-025 get_declared_classes / get_declared_interfaces / get_declared_traits
- FN-VAR-026 memory_get_usage / memory_get_peak_usage / memory_reset_peak_usage (8.2)
- FN-VAR-027 debug_zval_dump

### 2.29 FN-FUNC — Function handling

- FN-FUNC-001 call_user_func / call_user_func_array
- FN-FUNC-002 forward_static_call / forward_static_call_array
- FN-FUNC-003 func_get_args / func_num_args / func_get_arg
- FN-FUNC-004 function_exists / get_defined_functions
- FN-FUNC-005 register_shutdown_function
- FN-FUNC-006 register_tick_function / unregister_tick_function
- FN-FUNC-007 create_function — REMOVED 8.0
- FN-FUNC-008 Closure class (see SYN-097); usort-style callable params throughout stdlib

### 2.30 FN-URL — URL & encoding functions

- FN-URL-001 parse_url (component extraction; known corner-case laxness)
- FN-URL-002 parse_str (query-string → variables/array)
- FN-URL-003 http_build_query (RFC1738/RFC3986 enc types)
- FN-URL-004 urlencode / urldecode (form-encoding, + for space)
- FN-URL-005 rawurlencode / rawurldecode (RFC 3986 %20)
- FN-URL-006 base64_encode / base64_decode (strict mode)
- FN-URL-007 get_headers
- FN-URL-008 get_meta_tags
- FN-URL-009 **New URI extension (8.5): `Uri\Rfc3986\Uri` and `Uri\WhatWg\Url` — spec-compliant, immutable, withers; resolution, normalization, component getters** — first correct built-in URL parser
- FN-URL-010 quoted-printable / uu — see FN-STR-085/086

### 2.31 FN-NET — Network, DNS, mail

- FN-NET-001 dns_get_record / checkdnsrr (dns_check_record) / dns_get_mx (getmxrr)
- FN-NET-002 gethostbyname / gethostbynamel / gethostbyaddr / gethostname
- FN-NET-003 inet_pton / inet_ntop / ip2long / long2ip
- FN-NET-004 getprotobyname / getprotobynumber / getservbyname / getservbyport
- FN-NET-005 fsockopen / pfsockopen
- FN-NET-006 mail (sendmail/SMTP-ini based; header-injection-prone)
- FN-NET-007 syslog / openlog / closelog
- FN-NET-008 ftp_* family (~35: connect/ssl_connect/login/get/put/fget/fput/nlist/rawlist/mlsd/mkdir/rename/delete/chmod/pasv/…; FTP\Connection object 8.1)
- FN-NET-009 net_get_interfaces (7.3)

### 2.32 FN-MISC — Core misc / runtime info / tokens

- FN-MISC-001 phpversion / PHP_VERSION / version_compare
- FN-MISC-002 php_uname / php_sapi_name / PHP_OS_FAMILY
- FN-MISC-003 phpinfo / phpcredits
- FN-MISC-004 ini_get / ini_set / ini_restore / ini_get_all / get_cfg_var / ini_parse_quantity (8.2)
- FN-MISC-005 php_ini_loaded_file / php_ini_scanned_files
- FN-MISC-006 error_log / error_get_last / error_clear_last
- FN-MISC-007 debug_backtrace / debug_print_backtrace (DEBUG_BACKTRACE_IGNORE_ARGS)
- FN-MISC-008 uniqid (non-crypto) 
- FN-MISC-009 gc_collect_cycles / gc_enable / gc_disable / gc_enabled / gc_status / gc_mem_caches
- FN-MISC-010 highlight_file / highlight_string / php_strip_whitespace
- FN-MISC-011 token_get_all + PhpToken class (8.0) — tokenizer of PHP itself
- FN-MISC-012 get_extension_funcs / extension_loaded / get_loaded_extensions / dl (CLI)
- FN-MISC-013 constant surfaces: get_defined_constants
- FN-MISC-014 cli_set_process_title / cli_get_process_title
- FN-MISC-015 readline_* family (~12, CLI)
- FN-MISC-016 FFI class (7.4/8.0): cdef/load/new/cast/type — C interop without extensions
- FN-MISC-017 opcache_* userland: opcache_reset, opcache_invalidate, opcache_get_status, opcache_get_configuration, opcache_compile_file, opcache_is_script_cached
- FN-MISC-018 tick/declare & sapi_windows_* families

---

## PART 3 — RUNTIME & ECOSYSTEM SURFACE

- **RT-001** php.ini model: system ini → scanned conf.d → SAPI ini; per-dir `.user.ini` (fpm/cgi) and `php_admin_value` (fpm pool) overrides; `ini_set` runtime layer; INI_SYSTEM/INI_PERDIR/INI_USER/INI_ALL changeability tiers
- **RT-002** Key ini surface: memory_limit, max_execution_time, error_reporting, display_errors, upload_max_filesize, post_max_size, date.timezone, opcache.*, session.*, disable_functions, open_basedir
- **RT-003** SAPI model: **cli** (php file.php, -r, -a REPL, -l lint, -S dev server with router script), **php-fpm** (FastCGI pool manager — the production default), **apache2handler** (mod_php), **cgi/fcgi**, **embed** (libphp), **phpdbg**; litespeed
- **RT-004** Shared-nothing request lifecycle: each web request boots and tears down all state; persistence only via opcache/APCu/sessions/DB — the defining runtime contract
- **RT-005** Long-running alt runtimes (ecosystem): FrankenPHP, RoadRunner, Swoole/OpenSwoole, ReactPHP/amphp (event loops on Fibers) — worker mode keeps state across requests
- **RT-006** OPcache: shared-memory bytecode cache (production mandatory); file cache; `opcache.preload` (7.4) — preloaded always-available classes/functions
- **RT-007** JIT (8.0): tracing/function JIT inside opcache (opcache.jit=1255, jit_buffer_size); big numeric gains, modest web gains
- **RT-008** Composer: `composer.json` (require/require-dev/autoload/scripts/config), `composer.lock`, semver constraints (`^ ~ * ||`), Packagist registry, `vendor/autoload.php`, platform packages (php, ext-*), private repos (vcs/path/artifact)
- **RT-009** Composer autoload modes: PSR-4 (canonical), PSR-0 (legacy), classmap, files (the only way to autoload FUNCTIONS — eager, not lazy)
- **RT-010** PSR standards that matter: PSR-1/PSR-12/PER-CS (style), PSR-3 (LoggerInterface), PSR-4 (autoloading), PSR-6/PSR-16 (caches), PSR-7 (HTTP message), PSR-11 (container), PSR-13 (links), PSR-14 (events), PSR-15 (middleware/RequestHandler), PSR-17 (HTTP factories), PSR-18 (HTTP client), PSR-20 (clock)
- **RT-011** PHPUnit — de-facto testing: TestCase, assertions (~100), data providers, `#[Test]/#[DataProvider]` attributes (10+), mocking (createMock/createStub), coverage via xdebug/pcov, phpunit.xml
- **RT-012** Companion QA ecosystem: PHPStan/Psalm (static analysis — where "generics by docblock" actually live), PHP-CS-Fixer/PHPCS, Rector (codemods), Infection (mutation testing)
- **RT-013** Xdebug: step debugging (DBGp protocol, IDE integration), profiling (cachegrind), code coverage, develop-mode overloads of var_dump; alternative: phpdbg
- **RT-014** Error/exception production model: display_errors=Off + log_errors=On + error_log target; fpm catch_workers_output; fatal_error_backtraces (8.5)
- **RT-015** Extension model: bundled vs PECL; `extension=…` ini loading; `dl()` in CLI; FFI as extension-free interop; API/ABI break every minor version
- **RT-016** Deployment forms: git+composer, phar self-contained apps (box), Docker official images (php:8.5-fpm/cli/apache), static-binary ecosystem (static-php-cli, FrankenPHP embed)
- **RT-017** php -S localhost:8000 built-in dev server (single-threaded (multi-worker via PHP_CLI_SERVER_WORKERS), router script convention)
- **RT-018** Version/support policy: yearly minor (x.y in Nov), 2y active + 2y security (3y+1 revised policy 8.1+); deprecation → next-major removal
- **RT-019** stdin/stdout/stderr constants STDIN/STDOUT/STDERR (CLI); argc/argv; exit-code contract
- **RT-020** Framework gravity (ecosystem context for parity targets): Laravel (Eloquent/Blade/artisan), Symfony (components/DI/console), Slim/Laminas/CakePHP; WordPress as the legacy mass; PSR-15 middleware stacks

---

## PART 4 — PHP'S KNOWN DEFECTS ("PHP does it wrong → Phorj MUST do better")

- **DEF-001** Needle/haystack argument-order inconsistency — `strpos($haystack, $needle)` vs `in_array($needle, $haystack)` vs `array_search($needle, $haystack)`; no rule, pure memorization.
- **DEF-002** Function naming chaos — `strlen`/`str_replace`/`strcmp`/`ucwords`/`nl2br`/`htmlspecialchars`: underscore rules, abbreviations, prefixes all inconsistent; API reads as accreted, not designed.
- **DEF-003** Callback parameter-order inconsistency — `array_map($cb, $arr)` but `array_filter($arr, $cb)` and `usort($arr, $cb)`; same concept, three shapes.
- **DEF-004** Weak-typing juggling & `==` pitfalls — even after the 8.0 fix, `"1" == "01"`, `"10" == "1e1"`, `0 == "0"` surprise; two equality operators is itself the defect (safe one should be default).
- **DEF-005** `array` conflates list, map, set, tuple — one ordered-hash type; `array_is_list` (8.1) is an apology, not a fix; JSON encode ambiguity ([] vs {}) is a direct symptom.
- **DEF-006** Reference semantics are spooky — `foreach ($a as &$v)` leaves a live dangling reference corrupting the next loop; references infect arrays invisibly; `&` return/param semantics are folklore.
- **DEF-007** `@` error suppression operator — silences diagnostics at call site, encourages ignoring failure, costs performance, hides real bugs; still needed because stdlib signals via warnings.
- **DEF-008** Errors-vs-exceptions split brain — half the stdlib returns `false` + raises E_WARNING, half throws; caller must know which; `ErrorException` bridging is userland duct tape.
- **DEF-009** `false` as the universal error return — `strpos` returning `0|false` forces `!==` everywhere; the classic `if (strpos(...))` bug.
- **DEF-010** Mutable global state — superglobals are writable anywhere ($_GET can be reassigned), `global` keyword, static mutable state; testing/injection hostile.
- **DEF-011** DateTime is mutable — `$d->modify()` mutates in place; `DateTimeImmutable` added later as opt-in; wrong default preserved forever.
- **DEF-012** No generics in the language — `array<int, User>` exists only in docblocks enforced by third-party static analysis (PHPStan/Psalm); collections are untyped at runtime.
- **DEF-013** `unserialize()` is an RCE primitive — object injection / POP gadget chains; `allowed_classes` opt-in arrived late; `Phar` metadata made it reachable via file ops (pre-8.0).
- **DEF-014** `isset`/`empty` conflation — `empty("0")` is true (string zero is "empty"); `isset($x)` false for null-valued set variable; three subtly different existence notions.
- **DEF-015** Locale-sensitive core functions — historical `strtolower` et al. behavior varied with `setlocale` (Turkish-I bug); fixed only in 8.2 by de-localizing; `localeconv` still leaks into number parsing.
- **DEF-016** Two parallel string worlds — byte functions (`strlen`) vs `mb_*` vs `grapheme_*` vs `iconv_*`; correctness requires knowing which of four families to call; Unicode was bolted on.
- **DEF-017** Left-associative ternary was the default — `a ? b : c ? d : e` parsed WRONG for 25 years (fixed: deprecated 7.4, fatal 8.0).
- **DEF-018** Case-sensitivity inconsistency — functions/classes case-insensitive, variables/constants case-sensitive; `define(…, case_insensitive)` existed until 8.0.
- **DEF-019** Historic foot-gun inis — register_globals, magic_quotes, safe_mode, short_open_tag: language semantics varying per-server; the lesson (config must not change semantics) still violated by strict_types being per-file and error_reporting being runtime.
- **DEF-020** `include`/template duality — any included file is executable code; template injection = RCE; no sandboxed template tier in the language.
- **DEF-021** `extract()` / variable variables — runtime injection of arbitrary variable names into scope; kills static reasoning and enables register_globals-class bugs voluntarily.
- **DEF-022** Silent int→float overflow & float keys — `PHP_INT_MAX + 1` silently becomes float; `$a[1.9]` truncates to `$a[1]` (deprecated 8.1, still works).
- **DEF-023** `0.1 + 0.2` display/precision duality — `precision` vs `serialize_precision` inis make the same float print differently in echo vs var_dump/json_encode; no decimal type in core.
- **DEF-024** Array value-semantics vs object handle-semantics asymmetry — `$b = $a` copies an array but aliases an object; one assignment operator, two meanings, invisible at the call site.
- **DEF-025** In-place sorts returning bool — `sort($a)` mutates and returns true; can't chain; contrast `array_reverse` which returns; mutation/pure API split is unprincipled.
- **DEF-026** `array_merge` vs `+` vs `array_replace` — three merge semantics (renumber vs left-bias vs right-bias) with non-obvious names.
- **DEF-027** Loose `in_array`/`switch`/`array_search` defaults — strict comparison is opt-in (`true` third arg); `switch` cannot opt in at all (pre-match).
- **DEF-028** String-to-number array key juggling — `$a["1"]` === `$a[1]` but `$a["01"]` differs; keys silently transformed at insert.
- **DEF-029** Alphanumeric string increment — `"a9"++` → `"b0"`, `"z"++` → `"aa"`, but `--` is a no-op on strings; `str_increment` (8.3) added because the operator can't be fixed.
- **DEF-030** `parse_url`/`filter_var` are non-conformant — historic parser accepts/mangles invalid URLs, FILTER_VALIDATE_URL diverges from WHATWG; only fixed by the 8.5 Uri extension (two more APIs now coexist).
- **DEF-031** `mail()` header injection & sendmail coupling — raw string interface, no escaping, ini-configured transport.
- **DEF-032** No function/const autoloading — classes autoload lazily, functions can't (Composer `files` = eager include per request); pushes everything into static classes.
- **DEF-033** Namespace fallback resolution for functions — unqualified `strlen()` checks current namespace then global at runtime; perf + shadowing hazard; opcache special-cases a compiled list.
- **DEF-034** Named arguments made parameter names API — 8.0 retroactively froze 25 years of inconsistent parameter names as BC surface; renaming a param is now a break.
- **DEF-035** Exceptions in destructors/`__toString` historically fatal-prone; destructor timing nondeterministic under cycles — RAII unusable.
- **DEF-036** `list()`/destructuring silently yields null on missing keys — no error, no default syntax.
- **DEF-037** Traits copy-paste semantics — conflicts resolved textually (`insteadof`), no linearization; state in traits duplicates per class; `static` in traits per-using-class surprises.
- **DEF-038** Late static binding complexity — `self::` vs `static::` vs `$this::` vs `parent::` four-way distinction is a recurring bug source; wrong (`self`) is shorter and looks right.
- **DEF-039** `strtotime`'s DWIM parsing — accepts almost anything, guesses formats, returns false or wrong dates silently ("0000-00-00", day-first vs month-first ambiguity by separator).
- **DEF-040** Resource-to-false APIs & mixed handle types — pre-8.0 `fopen` returns resource|false, feeding a false into the next call yields unrelated warnings; object handles (8.0+) fixed only some extensions.
- **DEF-041** `settype`/casts lose information silently — `(int)"12abc"` → 12 with (only since 7.1) a notice; `(bool)"0.0"` → true vs `(bool)"0"` → false.
- **DEF-042** Output/headers temporal coupling — `header()` after any byte of output is a warning-and-broken-response; whitespace before `<?php` is a classic production bug; BOM breaks it invisibly.
- **DEF-043** Per-request compile without opcache is the semantic model — language pretends scripts are interpreted fresh; real deployments require an out-of-language cache for viability.
- **DEF-044** `json_decode` returning null for both "null" input and errors (pre-THROW_ON_ERROR discipline) — in-band error signaling again.
- **DEF-045** Superglobal-driven request model — $_GET/$_POST/$_FILES pre-parsed by SAPI with ini-bounded limits (max_input_vars) that silently truncate; raw body one-shot on php://input.

---

## ITEM COUNT SUMMARY

| Part | ID space | Unique items |
|---|---|---|
| Part 1 — Language syntax & semantics | SYN-001…SYN-173 | **173** |
| Part 2 — Builtin function surface (35 groups) | FN-<GRP>-NNN | **631** |
| Part 3 — Runtime & ecosystem | RT-001…RT-020 | **20** |
| Part 4 — Known defects | DEF-001…DEF-045 | **45** |
| **Grand total** | | **869** |

Part 2 per-group row counts: STR 93 · ARR 74 · FS 55 · SPL 39 · MATH 37 · DATE 27 · VAR 27 ·
MB 22 · INTL 18 · MISC 18 · PROC 16 · REFL 15 · STREAM 15 · CURL 13 · XML 12 · CTYPE 11 ·
PCRE 11 · DB 10 · OB 10 · SESS 10 · SOCK 10 · URL 10 · FILTER 9 · NET 9 · CRYPT 8 · FUNC 8 ·
HASH 8 · GD 7 · ZLIB 7 · ICONV 6 · JSON 6 · FINFO 4 · RAND 4 · PHAR 1 · ZIP 1.

**Deliberate compression** (per brief): sodium (~110 fns), openssl (~60), intl classes
(method-level), gd (~110), posix (~40), ftp (~35), mysqli procedural aliases (~100),
pgsql (~90), expat (~20), date OO-aliases (~26), BCMath/GMP, readline, CURLOPT_* constants —
rolled into family rows with counts. Fully-expanded symbol count implied by those families is
roughly **1,600–1,800 additional symbols** beyond the 869 itemized rows.

*End of Agent D surface. Diff key: any SYN/FN/RT row with no Phorj-side counterpart is a gap
candidate; every DEF row is a "must do better" test the Phorj design should explicitly pass.*





