# Phorge examples

What Phorge can do **today**. Every `.phg` here runs byte-identically on both backends
(`phg run` and `phg runvm`) — enforced by `tests/differential.rs`, which globs this directory,
so a new example is auto-gated the moment it lands. This page is updated as examples are added.

## Index

| Example | What it shows |
|---|---|
| `hello.phg` | the minimal program — `package Main;` + `import Core.Console;` + `Console.println` |
| `fib.phg` | recursion, `for…in`, `List<int>` |
| `grades.phg` | enums + `match`, a class with a method, `List`, `for…in` |
| `realworld/ledger.phg` | bank accounts: classes + methods + `this`, payload enum + `match`, recursion (compound interest), integer-cents arithmetic, immutability (`apply` returns a fresh `Account`) |
| `realworld/library.phg` | catalogue: zero-payload + payload variants, `match`, a class, `List` + `for`, float arithmetic |
| `realworld/shop.phg` | cart + discounts: enum + `match`, class composition, recursion (bulk pricing), integer arithmetic |
| `realworld/rpg.phg` | turn-based combat: enum + `match`, class + methods + `this`, `List` + `for`, immutable state evolution |
| `guide/operators.phg` | arithmetic, comparison, logical, unary operators; `bool` |
| `guide/control-flow.phg` | `if`/`else`, `for…in`, recursion, mutual recursion |
| `guide/functions.phg` | functions: typed params, return types, a no-return function, composition, a `List<int>`-returning function |
| `guide/collections.phg` | `List<T>` literals, nested `List<List<int>>`, nested `for`, list of instances, `List.length` |
| `guide/classes.phg` | constructor promotion, methods, `this`, composition, a method call on a field |
| `guide/enums-match.phg` | payload + zero-payload variants; literal, binding, and variant patterns |
| `guide/match-expr.phg` | `match` in expression position (operand / call argument) + literal patterns; transpiles to an IIFE (M11) |
| `guide/strings.phg` | string interpolation |
| `guide/inference.phg` | `var` local type inference + `type` aliases (M3 S0) |
| `guide/ergonomics.phg` | indexing `xs[i]`, integer ranges `0..n` / `0..=n`, expression `if` (M3 S1) |
| `guide/mutable.phg` | the `mutable` binding modifier + variable reassignment (`x = e;`) — immutable-by-default, `mutable`/`mutable var` opt-in, reassignment as a loop accumulator, a two-binding scalar-copy case; reassignment reuses `Op::SetLocal` (no new Op), `mutable` erased in PHP output (mutation milestone M-mut.1) |
| `guide/compound-assign.phg` | compound assignment `+= -= *= /= %=`, statement `++`/`--`, and `??=` — all pure desugar into M-mut.1 reassignment (`x op= e` ⟶ `x = x op e`); integer `/=` truncates, `%=` follows the dividend's sign, `??=` assigns only when null; a two-binding scalar-copy observe; no new `Op`, no GC (mutation milestone M-mut.2) |
| `guide/loops.phg` | condition loops — `while`, `do-while`, C-style `for (init; cond; step)`, while-let `while (var x = opt)`, plus `break`/`continue`; nested-loop inner-`break`; every form lowers to existing `Jump`/`JumpIfFalse` back-edges (no new loop opcode, F5); while-let is parser sugar over if-let + `break` (mutation milestone M-mut.3) |
| `guide/clone-with.phg` | `obj with { field = value, … }` — a functional update producing a fresh instance with named fields overridden, **bypassing the constructor** and leaving the source untouched (Fork 2 = B); methods work on the result; lowers to the existing `Op::MakeInstance` (no new `Op`), transpiles to PHP `clone($obj, ['f' => …])` (mutation milestone M-mut.4a) |
| `guide/element-set.phg` | value-type element set `xs[i] = e` (list) and `m[k] = e` (map), incl. compound `xs[i] += e` and filling a list in a loop; **copy-on-write** value semantics (a copied binding is independent — the F13 aliasing catcher); one new `Op::SetIndex` with COW via `Rc::make_mut`, still GC-free; transpiles to PHP `$xs[$i] = $e` (mutation milestone M-mut.5) |
| `guide/mutable-fields.phg` | shared-mutable instance fields `o.f = e` — instances are **handles** (two bindings share one cell, a write through one is visible through the other — the F13 handle catcher, opposite of value-type COW); fields are immutable-by-default, `mutable` opt-in; `this.f = e` in a method/ctor body; one new `Op::SetField`, `eq_val` made cycle-safe (F4); transpiles to PHP `$o->f = $e` (mutation milestone M-mut.6) |
| `guide/static-fields.phg` | `static` class fields — program-lifetime state on the class, accessed as `ClassName.field` (dot, not `::`); `static mutable` opts into reassignment (immutable static = a class constant); literal-const initializers evaluated once at load; one new `Op::GetStatic`/`SetStatic`, transpiles to PHP `Class::$field` (mutation milestone M-mut.7) |
| `guide/property-hooks.phg` | property hooks `T name { get => …; set(T v) { … } }` — a computed-read and/or intercepted-write member that looks like a field but runs code (a virtual property; the motivating Celsius↔Fahrenheit case); get-only = read-only, set-only = write-only; lowers on the VM to synthetic `<name>$get`/`$set` methods dispatched via the existing `Op::CallMethod` (**no new `Op`**); transpiles 1:1 to a PHP 8.4 property hook (mutation milestone M-mut.7b) |
| `guide/null-safety.phg` | optionals `T?`, `??`, `?.`, `if (var x = opt)`, `opt!`, `match` over `T?` (M3 S2) |
| `guide/instanceof.phg` | the `instanceof` runtime type test (`value instanceof ClassName` → `bool`) + smart-cast narrowing inside `if`; transpiles to PHP `instanceof` (Rich Types M-RT S1) |
| `guide/interfaces.phg` | `interface` + `class … implements …` + `interface … extends …`; nominal subtyping (a class instance flows into an interface-typed slot), polymorphic calls through an interface-typed parameter, and `instanceof` against an interface (with smart-cast narrowing); transpiles to a PHP `interface`/`implements`/`extends` (Rich Types M-RT S2) |
| `guide/maps.phg` | `Map<K, V>` literals `[k => v]` + indexing `m[k]` (string- and int-keyed; a map-index result as an arithmetic operand); keys are `int`/`bool`/`string`, insertion-ordered, transpiles to a PHP `[k => v]` array (Rich Types M-RT S3) |
| `guide/generics.phg` | erased generics — `<T>` type parameters on free functions, inferred at the call site; reuse at many concrete types, a `List<T>` parameter, a `(T) -> T` function-typed parameter; no monomorphization (type params erase to PHP `mixed`/`array`/`\Closure`) (Rich Types M-RT S7) |
| `guide/generic-methods.phg` | erased generics on **methods** — `<T>` declared on a method of a (non-generic) class, inferred from the call's arguments (`u.id(7)`, `u.firstOr(xs, -1)`, `u.applyTwice(5, fn …)`); reuses the free-function machinery, erases the same way (PHP `mixed`/`array`/`\Closure`), zero backend changes (Rich Types M-RT generics-all) |
| `guide/generic-types.phg` | erased generics on **classes** — `class Box<T>` / `class Pair<A, B>`; the type parameter is inferred at construction (`Box(7)` ⇒ `Box<int>`) and recovered at every use site (`Box(7).get()` is `int`); a method taking a `T`; no monomorphization (a `T` field erases to PHP `mixed`, an instance carries no runtime type argument) (Rich Types M-RT generics-all) |
| `guide/generic-enums.phg` | erased generics on **enums** — `enum Option<T>` / `enum Result<T, E>`; the parameter is inferred at the variant constructor (`Some(7)` ⇒ `Option<int>`) and recovered at every `match` (so `Some(n)` binds `n: int`); a variant that mentions no parameter (`None`) is fixed by annotating the binding (`Option<int> n = None();`); no monomorphization (a `T` payload erases to PHP `mixed`) (Rich Types M-RT generic enums) |
| `guide/collections-query.phg` | the first **generic stdlib natives** — `Core.List` `reverse`/`sum` and `Core.Map` `keys`/`values`/`has`/`size`; type parameters (`reverse(List<T>) -> List<T>`, `keys(Map<K,V>) -> List<K>`) inferred at the call site by the same unifier as a generic free function, erasing to PHP array builtins (Rich Types M-RT S7b) |
| `guide/sets.phg` | **`Set<T>`** via `Core.Set` — `of(List<T>) -> Set<T>` (dedupe, insertion-ordered), `contains(Set<T>, T) -> bool`, `size(Set<T>) -> int`; generic, erases to a deduped PHP array (`array_unique`/`in_array`/`count`) (Rich Types M-RT S7b) |
| `guide/higher-order.phg` | **higher-order `Core.List` natives** — `map`/`filter`/`reduce` taking a closure argument (run once per element on either backend via one shared native body); inline lambdas, a captured local, and a composed filter→map→reduce pipeline; generic, erases to PHP `array_map`/`array_values(array_filter(…))`/`array_reduce` (Rich Types M-RT S7b-3) |
| `guide/unions.phg` | union types `A \| B \| C` (classes, interfaces, primitives); a value of any member flows into a union-typed slot; reach a member via **match-over-union** type patterns (`match s { Circle c => … }`, exhaustive) or `instanceof` narrowing; a primitive `int \| string` union matched by literal value; transpiles to PHP 8.0 `A\|B` (Rich Types M-RT S4) |
| `guide/intersections.phg` | intersection types `A & B` (interfaces plus at most one class); a value satisfying all members flows in, and **every member's methods are in scope** without narrowing; an `A & B` value also flows out to a single member; `&` binds tighter than `\|`; transpiles to PHP 8.1 `A&B` (Rich Types M-RT S5) |
| `guide/totality.phg` | **return-on-all-paths** — a typed function must `return`/diverge on every path (else `E-MISSING-RETURN`); the **`never`** bottom type (a `-> never` function provably diverges, → PHP 8.1 `never`); dead code after `return` (`W-UNREACHABLE`) and a `match` arm after a catch-all (`W-MATCH-UNREACHABLE`) are warned (Rich Types M-RT totality cluster) |
| `guide/lambdas-pipe.phg` | lambdas (expression + statement body), higher-order functions, first-class named-fn references, the pipe operator `\|>` (M3 S3 Track A) |
| `guide/overloading.phg` | **method & function overloading** — DYNAMIC multiple dispatch: several functions/methods of one name with distinct parameter signatures; the runtime argument types select the most-specific overload (free-fn type + arity overloads; class/interface most-specific dispatch; method overloads), identically on `run`/`runvm`/real PHP (one `is_*`/`instanceof` dispatcher); all overloads of a name share a return type (M-RT) |
| `guide/inheritance.phg` | **single inheritance** — `open class … { … }` + `class Sub extends Base`; **final-by-default** (a class must be `open` to be extended, a method must be `open` to be overridden — else `E-EXTEND-FINAL`/`E-OVERRIDE-FINAL`, the inheritance sibling of immutable-by-default); a subclass inherits non-overridden methods, overrides the `open` ones, and is a subtype of its parent; dynamic dispatch (an inherited method runs the subclass's override); transpiles to PHP `final class`/`class … extends …` (Rich Types M-RT S6a) |
| `guide/inheritance-multi.phg` | **multiple inheritance** — `class C extends A, B`; cross-parent method collisions are resolved explicitly with `use P.m` / `rename P.m as n` / `exclude P.m` (else `E-MI-CONFLICT`); a **diamond** shared base auto-merges (a method reached identically through two arms is never a conflict); also `abstract class`/`abstract function` (`E-ABSTRACT-INSTANTIATE`/`-UNIMPL`). PHP has no MI, so it lowers to per-parent `interface I…` + `trait T…` and the subclass `implements I…, I… { use T…, T… { … insteadof/as … } }` (Rich Types M-RT S6b) |
| `guide/inheritance-lattice.phg` | **subtyping & `instanceof` across the lattice** — a subclass is a subtype of *every* ancestor (single + multiple parent classes and their interfaces); `instanceof` against any ancestor class is true, a value flows into an ancestor-typed binding/parameter, and a `match` type-pattern narrows on an ancestor. A multi-parent class lowers to PHP `implements I<Parent>`, so the transpiler tests the interface form (`instanceof IFish`); the interpreter + VM share one subtype oracle (parent classes **and** interfaces — `ast::instanceof_table`), so all three backends agree (Rich Types M-RT S6c.3) |
| `guide/inheritance-state.phg` | **multiple inheritance with state** — a multi-parent class with no own constructor gets a **synthesized orchestrating constructor**: its params are the parents' ctor params concatenated in `extends` order, and constructing it runs each parent's constructor (its arg slice) on the one instance, initializing every inherited field (a parent ctor may have a body, not just promoted params). Lowers to per-parent `interface I…`/`trait T…` (the trait holds the fields as plain properties — two trait `__construct`s would collide) + a single explicit-assignment `__construct` on the subclass; also shows ancestor-typed bindings + `instanceof` across the lattice (Rich Types M-RT S6c.2/.3) |
| `guide/traits.phg` | **traits** — horizontal code reuse with `trait` / `use`; a trait is reuse, **NOT a type** (`use` = has-the-behavior-of, vs `extends` = is-a; `instanceof Trait` is rejected). Composes methods (any visibility incl. a `private` helper), `mutable` instance state, a trait **constructor** (promotion + body — adopted by a using class with no ctor of its own; wins over a parent ctor, `W-TRAIT-CTOR-*`), a `static` field (per-using-class copy), an **abstract requirement** the using class must satisfy, and a **property hook** reading it; multi-trait composition. Trait members flatten into the using class before any backend; the transpiler emits a native PHP `trait`/`use` (Rich Types M-RT S8) |
| `guide/numeric.phg` | **numeric primitives** — literal formats (`0xFF`/`0b1010`/`0o17` Rust-style with no implicit-octal footgun, `_` separators, `1e3` scientific) and **bitwise operators** `& \| ^ ~ << >>` (int-only, PHP-identical; shift-right is two `Gt` so nested generics are untouched; transpile 1:1 to PHP). Literal *value* (not surface form) reaches the AST, so formats are pure-lexer byte-identical (primitives P1+P2) |
| `guide/pattern-matching.phg` | **match-arm guards** — an optional `when <cond>` on a match arm (the arm matches only when the pattern matches AND the guard holds; a false guard falls through to the next arm, so several arms can share a shape). The guard reads the arm's bindings; contextual `when` (a normal identifier elsewhere); a guarded arm doesn't discharge its shape for exhaustiveness (`E-MATCH-GUARD-EXHAUST`), non-bool guard is `E-GUARD-TYPE`. Shown over an enum (numeric ranges, fall-through, unguarded fallback) and a union type-pattern (field access in the guard); lowers to existing branch ops (no new `Op`), folds into the PHP if/elseif chain (Rich Types pattern cluster S5.1). Also **struct (named-field) destructuring** (S5.2): shorthand `Circle { r }`, rename `Point { x: fx }`, and nesting `Line { from: Point { x, y }, to }` — the same `instanceof` test (reuses `Op::IsInstance`), then field reads; bound fields are usable as typed operands (`fx + fy`). Errors `E-STRUCT-PAT-TYPE`/`E-STRUCT-FIELD-UNKNOWN`/`E-PATTERN-DUP-BIND` |
| `guide/result.phg` | error-as-value `Result<T, E>` + the **`?` propagation operator** (in a let-initializer: unwrap `Ok` or early-return `Err`); the unchecked-fault tier — `panic`/`todo`/`unreachable` (`never`-typed) and `assert(cond[, "msg"])` — shown via a passing `assert` (faults can't be runnable examples) (M-faults Slice 2a) |
| `guide/errors.phg` | **checked exceptions** — `throws A \| B` declarations, `throw`, `try`/multiple `catch (X e)`/union `catch (A \| B e)`/`finally`, and `?`-throws propagation (a throwing call's exceptions flow to the enclosing `throws`); a thrown type implements the built-in `Error` marker → PHP class `extends \Exception`; native unwinding on both backends (3 new `Op`s: `Throw`/`PushHandler`/`PopHandler`), transpiles to PHP `try`/`catch`/`finally` (M-faults Slice 2b) |
| `guide/cause-chain.phg` | **exception cause chains** — a conventional `cause` field of type `Error?` on an `Error` subtype preserves the lower-level error that triggered a higher-level one; it is routed into PHP's native exception chain (`parent::__construct($message, 0, $cause)` → `getPrevious()`), so the transpiled PHP reports an idiomatic "caused by" while the Phorge backends read it back as a plain field (M-faults Slice 2c) |
| `guide/math.phg` | the `Core.Math` stdlib module — `sqrt`/`pow`/`floor`/`ceil`/`abs`/`min`/`max`/`round` (M3 Track B Wave 2; `round` added P3.2) |
| `guide/floats.phg` | `float` stringification — shortest-round-trip, always-positional, byte-identical across `run`/`runvm`/PHP for every finite magnitude (irrational, large, small) via the `__phorge_float` transpile helper |
| `guide/text.phg` | the `Core.Text` stdlib module — `len`/`upper`/`lower`/`trim`/`contains`/`split`/`join`/`replace`/`startsWith`/`endsWith`/`repeat` (M3 Track B Wave 2; last three added P3.2) |
| `guide/file.phg` | the `Core.File` stdlib module — `read` (→ `string?`), `exists`; reads a committed fixture, composes with S2 `??` / if-let (M3 Track B Wave 2) |
| `guide/bytes.phg` | the `bytes` type + `b"…"` literals (`\xHH`) + `Core.Bytes` interop — `fromString`/`toString` (→ `string?`)/`len`/`concat`/`slice` (M6 W0) |
| `guide/html.phg` | `Core.Html` — the escape **kernel** (`text`/`raw`/`render`), the typed element **builders** (`el`/`voidEl`/`attr`/`boolAttr`/`concat`), **named per-tag helpers** (`div`/`p`/`a`/`ul`/`li`/`br`/…), and the **`html"<h1>{name}</h1>"` literal sugar** (holes escape by type unless already `Html`); `Html`/`Attr` are distinct from `string`, XSS-safe by construction (Core.Html Waves 1–3) |
| `bench/workload.phg` | a **profiling** workload (CPU recursion + heap allocation) for `phg bench`/`disasm` — see `bench/README.md` |
| `transpile/demo.phg` | the **Phorge → PHP** bridge — see `transpile/README.md` |
| `build/app.phg` | **standalone executables** — `phg build` — see `build/README.md` |
| `cli/demo.phg` | the **`phg` CLI** — source forms, `check`/`parse`/`lex`, diagnostics, `explain` — see `cli/README.md` |
| `web/handler.phg` | the **M6 W1 HTTP handler model** — `Request`/`Response` classes, `parseRequest`/`serializeResponse` in pure Phorge, `handle(Request) -> Response`; `bytes` bodies, `req.header(name)` lookup, `bytes.find` + `text.splitOnce`. No socket yet (that's W3's `phg serve`) |
| `web/router.phg` | the **M6 W2 static router** — a data-driven `List<Route>` table, linear exact-match `(method, path)` scan → a `Handler` enum tag, dispatched by exhaustive `match` to named handler functions; method-sensitive 404 fallback. Pure Phorge (no new feature); path params + middleware deferred (Track A / generics) |
| `web/server.phg` | the **M6 W4 served app** — W1 parse/serialize + W2 routing + the single entry `respond(bytes) -> bytes` that **`phg serve`** runs over a real socket. `web/server.php` is the **`php -S`** front-controller bridge (both call the same `handle(Request) -> Response`) — see `web/README.md` |
| `project/tempconv/` | a **multi-file project** (M5) — mandatory packages, folder = path, cross-package qualified calls + import aliasing, namespaced PHP — see `project/README.md` |
| `project/withdeps/` | a project with a **vendored git dependency** (M5 S3) — `[require]`, `phg vendor`, `phorge.lock`, offline `vendor/` — see `project/withdeps/README.md` |
| `project/shapes/` | **cross-package types** (M-RT) — a library package (`Acme.Geometry`) exports a `class` + `interface` + `enum`, consumed from `package Main` via `import type Acme.Geometry.Rect;`; nominal subtyping, `instanceof`, and enum `match` all cross-package; erases to namespaced PHP (`new \Acme\Geometry\Rect(…)`) |
| `project/visibility/` | **declaration visibility** (visibility modifiers) — `public` / `internal` / `private` on top-level declarations; a `public` class crosses packages, an `internal` helper crosses files within its package, a `private` helper stays file-local; loader-enforced, erased from PHP — see `project/visibility/README.md` |

## Coverage matrix (the runnable surface)

| Feature | Examples |
|---|---|
| `int`/`float` arithmetic, `%`, comparison, logical, unary, overflow-checked | `guide/operators`, all `realworld/*` |
| immutable typed bindings | every example |
| functions, recursion, mutual recursion | `guide/functions`, `guide/control-flow`, `fib`, `ledger`, `shop` |
| `if`/`else`, `for…in` | `guide/control-flow`, `fib`, all `realworld/*` |
| `List<T>` literals, nesting, iteration | `guide/collections`, all `realworld/*` |
| classes: ctor promotion, fields, methods, `this`, field reads, composition | `guide/classes`, `ledger`, `rpg`, `grades` |
| enums (payload **and** zero-payload via `V()`) + exhaustive `match` | `guide/enums-match`, all `realworld/*`, `grades` |
| `match` literal patterns + expression-position `match` (transpiles, oracle-gated) | `guide/enums-match`, `guide/match-expr` |
| string interpolation `"{expr}"` | `guide/strings`, every example |
| indexing `xs[i]`, ranges `0..n` / `0..=n`, expression `if` | `guide/ergonomics` |
| null safety: `T?`, `??`, `?.`, `if (var x = opt)`, `opt!`, `match` over `T?` | `guide/null-safety` |
| type test `instanceof` (class operand) + `if`-narrowing, transpiles to PHP `instanceof` | `guide/instanceof` |
| interfaces + `implements`/`extends`, nominal subtyping, polymorphism, `instanceof` against an interface | `guide/interfaces` |
| traits: `trait`/`use` reuse (not a type), methods+visibility, state (`mutable`/`static`), trait ctor, abstract requirement, property hooks → native PHP `trait`/`use` | `guide/traits` |
| lambdas (expr + stmt body), higher-order fns, first-class named-fn refs, pipe `\|>` | `guide/lambdas-pipe` |
| erased generics `<T>` on free functions, call-site inference (incl. `List<T>` + `(T) -> T` params) | `guide/generics` |
| erased generics `<T>` on methods, then classes (`Box<T>`/`Pair<A, B>`, inferred at construction) | `guide/generic-methods`, `guide/generic-types` |
| erased generics `<T>` on enums (`Option<T>`/`Result<T, E>`, inferred at the variant ctor, substituted at `match`) | `guide/generic-enums` |
| generic stdlib natives: `Core.List` `reverse`/`sum`, `Core.Map` `keys`/`values`/`has`/`size` | `guide/collections-query` |
| `Set<T>`: `Core.Set` `of`/`contains`/`size` (insertion-ordered, deduped) | `guide/sets` |
| totality: return-on-all-paths (`E-MISSING-RETURN`), the `never` bottom type, dead-code lints (`W-UNREACHABLE`/`W-MATCH-UNREACHABLE`) | `guide/totality` |
| error model: `Result<T,E>` + `?` propagation, fault intrinsics (`panic`/`todo`/`unreachable`/`assert`) | `guide/result` |
| `var` local type inference, `type` aliases | `guide/inference` |
| `Console.println(string)` (after `import Core.Console;`) | every example |
| `Core.Math` stdlib: `sqrt`/`pow`/`floor`/`ceil`/`abs`/`min`/`max`/`round` | `guide/math` |
| `float` shortest-round-trip rendering, byte-identical across backends + PHP | `guide/floats` |
| `Core.Text` stdlib: `len`/`upper`/`lower`/`trim`/`contains`/`split`/`join`/`replace`/`startsWith`/`endsWith`/`repeat` | `guide/text` |
| `Core.File` stdlib: `read` (→ `string?`), `exists` (fixture-gated) | `guide/file` |
| `Core.Html` kernel (`text`/`raw`/`render`) + builders (`el`/`voidEl`/`attr`/`boolAttr`/`concat`) + named per-tag helpers (`div`/`p`/`a`/`ul`/`li`/`br`/…) + `html"…"` literal sugar (type-directed hole escaping); `Html`/`Attr` ≠ `string` (XSS-safe by construction) | `guide/html` |
| `Core.Bytes`: `find` (→ `int?`); `Core.Text`: `splitOnce` (→ `List<string>`) | `web/handler` |
| HTTP handler model: `Request`/`Response`, `parseRequest`/`serializeResponse`, `handle()` | `web/handler` |
| static HTTP router: `List<Route>` table, exact `(method, path)` match → `Handler` enum + exhaustive dispatch | `web/router` |
| HTTP serve runtime: `phg serve` (native socket) + `php -S` front-controller, one `respond(bytes) -> bytes` entry | `web/server` |
| Phorge → PHP transpile | `transpile/demo` |
| standalone executable (`phg build`) | `build/app` |
| CLI: source forms, inspection (`check`/`parse`/`lex`), diagnostics, `explain` | `cli/demo` |
| multi-file projects: packages, folder = path, cross-package imports + aliasing, namespaced PHP | `project/tempconv` |
| git dependencies: `[require]`, `phg vendor`, `phorge.lock`, offline `vendor/` | `project/withdeps` |
| declaration visibility: `public`/`internal`/`private` (file ⊂ package ⊂ public), loader-enforced | `project/visibility` |
| runtime stack traces + fault reporting (CLI + `phg serve --dev` web page) | `errors/` (walkthrough) |

## Three sharp edges

- **Every file declares a package (M5 S1) — `package Main;` is the runnable entry.** Nothing lives
  "in the wind": each file's first line is a `package` declaration, never inferred. A runnable program
  uses the reserved `package Main;` (every example here starts with it); `Core` is reserved for the
  stdlib. Dotted library packages (`package Acme.Convert;`) + strict folder=path + cross-package
  imports are now **shipped** — see `project/tempconv/` and `project/README.md`. Package and
  folder segments are **PascalCase** (`E-PKG-CASE`), mapping 1:1 to PHP namespaces (`Acme.Convert`
  ⇒ `Acme\Convert`); types are PascalCase, functions/variables camelCase.
- **Zero-payload enum variants use call form `V()` everywhere** — to construct (`Defend()`) *and* in
  a `match` arm (`Defend() =>`). A bare `Defend =>` arm is a catch-all *binding*, not a variant
  pattern, so it silently swallows every case.
- **`import Core.Console;` is load-bearing (M3 Wave 1).** Everything is namespaced — "nothing in the
  wind" — so there is no free global `println`: a program must `import Core.Console;` and call
  `Console.println(...)`. Stdlib modules are reserved under `core.*`; the root lives in the import and
  the leaf qualifies the call (Go's `import "fmt"` → `fmt.Println`). The same leaf-qualified `import`
  resolves user `.phg` packages in a project (M5) — see `project/tempconv/`.

## Not yet supported (intentionally absent here)

These are designed but not implemented; they will arrive in later milestones, and examples will be
added as each lands: sized ints, `decimal`, and **transitive** git dependencies (a dependency's own
`[require]`; direct git deps are shipped — see `project/withdeps/`).
