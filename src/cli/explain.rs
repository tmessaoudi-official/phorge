/// The prose explanation for a diagnostic `code`, or `None` if the code is unknown. The codes are
/// the stable identifiers carried by [`crate::diagnostic::Diagnostic::code`] and shown in `[…]`
/// beneath a rendered error.
pub fn explain_text(code: &str) -> Option<String> {
    let body = match code {
        "E-UNKNOWN-IDENT" => {
            "E-UNKNOWN-IDENT — a name was used that is not in scope.\n\n\
             Phorge resolves identifiers lexically: block-scope locals (including `var` bindings\n\
             and `for` loop variables), parameters, top-level functions, and — inside a method —\n\
             the current class's fields. A typo or an out-of-scope reference triggers this; the\n\
             diagnostic suggests the nearest in-scope name when one is close.\n"
        }
        "E-UNKNOWN-TYPE" => {
            "E-UNKNOWN-TYPE — a type name was used that is not defined.\n\n\
             Built-in types are `int`, `float`, `bool`, `string`, `List<T>`, `Map<K,V>`, `Set<T>`.\n\
             User types come from `class`, `enum`, and `type` alias declarations. Check the\n\
             spelling and that the declaration is present.\n"
        }
        "E-INFER-NULL" => {
            "E-INFER-NULL — `var` cannot infer a type from `null` alone.\n\n\
             `null` has no element type on its own, so `var x = null;` is rejected. Annotate the\n\
             optional instead, e.g. `int? x = null;`.\n"
        }
        "E-ALIAS-CYCLE" => {
            "E-ALIAS-CYCLE — a `type` alias refers to itself.\n\n\
             `type A = B; type B = A;` has no underlying type. Break the cycle so every alias\n\
             bottoms out at a built-in, class, or enum type.\n"
        }
        "E-RANGE-TYPE" => {
            "E-RANGE-TYPE — a range bound is not an `int`.\n\n\
             Both bounds of `a..b` / `a..=b` must be `int`; the range materializes to a\n\
             `List<int>` (its role this slice is `for (int i in 0..n)`). Use integer bounds, or\n\
             build a `List` explicitly if you need other element types.\n"
        }
        "E-MAP-KEY" => {
            "E-MAP-KEY — a map's key type is not hashable.\n\n\
             A `Map<K, V>` key must be `int`, `bool`, or `string` (the hashable subset) — a\n\
             `float`, list, instance, or other composite can't be a key. Change the key type, or\n\
             model the lookup differently (e.g. key by a `string` id).\n"
        }
        "E-NO-PACKAGE" => {
            "E-NO-PACKAGE — a file has no `package` declaration.\n\n\
             Everything is namespaced (\"nothing in the wind\"): every file must declare its package\n\
             as its first line, never inferred. A runnable program declares `package Main;` (the\n\
             reserved entry); library code declares a dotted path like `package app.util;`.\n"
        }
        "E-RESERVED-PACKAGE" => {
            "E-RESERVED-PACKAGE — a user file claimed a `core` package root.\n\n\
             The `core.` root is reserved for the standard library (`Core.Console`, `Core.Math`,\n\
             `Core.File`, …), like a built-in type name. Root your own packages elsewhere, e.g.\n\
             `package app;` or `package app.util;`.\n"
        }
        "E-RESERVED-NAME" => {
            "E-RESERVED-NAME — a function / class / enum / interface / trait / type was named `var`.\n\n\
             `var` is a *contextual* keyword in Phorge: it is the inference-binding keyword only at a\n\
             declaration start (`var x = …`), and an ordinary identifier everywhere else — so it is\n\
             fine as a variable, parameter, field, property, or method name (it maps to a legal PHP\n\
             `$var` / `->var` / `->var()`). But PHP reserves `var` for a *symbol* name: a free\n\
             `function var()` or `class var {}` is a PHP parse error, so Phorge rejects `var` there.\n\
             Rename the function/class/type (the value/parameter/field/method name can stay `var`).\n"
        }
        "E-PKG-PATH" => {
            "E-PKG-PATH — a file's `package` does not match its location.\n\n\
             In a project, the directory under the source root IS the package (folder = path, Go's\n\
             model): `src/app/util/*.phg` must declare `package app.util;`. `package Main;` is exempt\n\
             (runnable anywhere). Move the file, or fix its package to match the directory.\n"
        }
        "E-PKG-TYPE" => {
            "E-PKG-TYPE — a class/enum was declared in a library (non-`main`) package.\n\n\
             M5 S2c namespaces *functions* across packages; cross-package types are a later slice.\n\
             A library package may export functions only — move the `class`/`enum` to `package Main;`\n\
             for now, or await the M5 type-namespacing follow-up.\n"
        }
        "E-SHADOW-IMPORT" => {
            "E-SHADOW-IMPORT — a local binding shadows an imported module qualifier.\n\n\
             Everything is namespaced (\"nothing in the wind\"): after `import Core.Console;` the\n\
             name `console` is a module qualifier, so a value binding (variable, parameter, loop or\n\
             match binding) of the same name would make `Console.x()` ambiguous — the run backends\n\
             would read a method call, the transpiler a native. Rename the binding, or drop the\n\
             matching import.\n"
        }
        "E-SHADOW-FN" => {
            "E-SHADOW-FN — a local binding shadows a top-level function name.\n\n\
             Functions are first-class values, so a bare `f` resolves to the function and a bare\n\
             `f(…)` calls it. A local binding (variable, parameter, loop or match binding) of the\n\
             same name would be ambiguous — the run backends dispatch functions-first while the\n\
             transpiler emits the local, a silent divergence. Rename the binding so a local never\n\
             shares a name with a function.\n"
        }
        "E-OPT-ASSIGN" => {
            "E-OPT-ASSIGN — an optional `T?` was used where a non-optional `T` is required.\n\n\
             A non-optional value can never be `null`, so a `T?` cannot flow into a `T` binding,\n\
             parameter, field, or return without handling absence first. Unwrap it with `??`\n\
             (default), `?.` (safe access), `if (var x = opt) { … }`, or `opt!` (checked).\n"
        }
        "E-ASSIGN-IMMUTABLE" => {
            "E-ASSIGN-IMMUTABLE — a reassignment targeted an immutable binding.\n\n\
             Bindings are immutable by default. Only a binding declared `mutable` may be reassigned\n\
             with `x = …;`. Declare it `mutable int x = …;` (or `mutable var x = …;`) — or, if it\n\
             never changes, keep it immutable and introduce a new binding instead.\n"
        }
        "E-ASSIGN-TYPE" => {
            "E-ASSIGN-TYPE — a reassigned value's type does not match the binding's type.\n\n\
             Reassignment keeps the binding's declared type; the new value must be assignable to it\n\
             (the same rule as the original declaration). Convert the value, or change the binding's\n\
             declared type.\n"
        }
        "E-ASSIGN-UNKNOWN" => {
            "E-ASSIGN-UNKNOWN — a reassignment targeted a name that is not an in-scope local.\n\n\
             `x = …;` reassigns an existing local variable; the name must already be declared in\n\
             scope. Declare it first (`mutable int x = …;`), or check for a typo.\n"
        }
        "E-ASSIGN-TARGET" => {
            "E-ASSIGN-TARGET — an assignment target is not a simple variable.\n\n\
             Only `name = expr;` (reassigning a local) is supported in this slice. Field assignment\n\
             (`obj.field = …`) and element assignment (`xs[i] = …`) land in a later mutation slice.\n"
        }
        "E-HOOK-NO-GET" => {
            "E-HOOK-NO-GET — a property hook with no `get` was read.\n\n\
             A property hook may be read-only, write-only, or both. Reading one that declares only a\n\
             `set` is not allowed. Add a `get => …;` clause, or do not read this property.\n"
        }
        "E-HOOK-NO-SET" => {
            "E-HOOK-NO-SET — a property hook with no `set` was assigned.\n\n\
             A read-only computed property (only a `get`) cannot be assigned. Add a `set(T v) { … }`\n\
             clause to make it writable, or do not assign this property.\n"
        }
        "E-HOOK-TYPE" => {
            "E-HOOK-TYPE — a property hook's `get` result or `set` parameter does not match its type.\n\n\
             A hook `T name { … }` reads as `T`, so its `get` expression must yield `T`; its `set`\n\
             parameter must be declared `set(T v)`. Align the get expression / set parameter with the\n\
             hook's declared type.\n"
        }
        "E-HOOK-DUP" => {
            "E-HOOK-DUP — a property hook collides with another member.\n\n\
             A hook is virtual (it has no storage), so its name must be distinct from every field,\n\
             static, method, and other hook in the class. Rename the hook or the colliding member.\n"
        }
        "E-VIS-PRIVATE" => {
            "E-VIS-PRIVATE — a `private` declaration was referenced from another file.\n\n\
             A declaration marked `private` (visibility modifiers) is visible only within its own\n\
             `.phg` file. Referencing it from any other file — even one in the same package — fails.\n\
             Mark it `internal` (visible package-wide) or `public` (visible everywhere) to widen it.\n"
        }
        "E-VIS-INTERNAL" => {
            "E-VIS-INTERNAL — an `internal` declaration was referenced from another package.\n\n\
             A declaration marked `internal` is visible only within its own package (all its files),\n\
             not from other packages. A cross-package reference (an `import type`, or a qualified\n\
             `pkg.fn()` call) fails. Mark it `public` to export it across packages.\n"
        }
        "E-OPT-USE" => {
            "E-OPT-USE — a plain `.field` / `.method()` was used on an optional `T?` receiver.\n\n\
             The receiver could be `null`, so a plain member access risks a null dereference. Use\n\
             `?.` for null-safe access (the whole access yields `null` when the receiver is null),\n\
             or first narrow the optional with `if (var x = opt) { … }` or `opt!` (checked).\n"
        }
        "E-IF-LET-TYPE" => {
            "E-IF-LET-TYPE — `if (var x = …)` was given a non-optional scrutinee.\n\n\
             The if-let form narrows an optional `T?` to its non-null inner `T`, binding it inside\n\
             the then-block. A scrutinee that is already non-optional has nothing to narrow — use a\n\
             plain `if (cond)` for a boolean test, or make the scrutinee a `T?`.\n"
        }
        "E-OPT-UNWRAP" => {
            "E-OPT-UNWRAP — force-unwrap `!` was applied to a non-optional value.\n\n\
             `opt!` asserts that an optional `T?` is non-null and unwraps it to `T` (faulting at\n\
             runtime if it is null). A value that is already a non-optional `T` has nothing to\n\
             unwrap — remove the `!`.\n"
        }
        "W-FORCE-UNWRAP" => {
            "W-FORCE-UNWRAP — a force-unwrap `!` may fault at runtime (lint).\n\n\
             `opt!` aborts the program if the optional is null. This is a deliberate guardrail: it\n\
             flags every `!` so you can prefer a total alternative — `??` (default value), `?.`\n\
             (safe access), or `if (var x = opt) { … }` (narrow) — where null is a real possibility.\n"
        }
        "E-LAMBDA-THIS" => {
            "E-LAMBDA-THIS — a field-initializer lambda captures `this`.\n\n\
             A method-body lambda MAY capture `this` (it is captured live, by the instance handle). The\n\
             one place it is rejected is a field or static initializer: that code runs while the\n\
             instance is only partially built, so capturing the receiver would expose half-initialized\n\
             fields. Move the closure into the constructor body, or capture a specific value\n\
             (`var v = this.x;`) before building the closure.\n"
        }
        "E-VENDOR-MISSING" => {
            "E-VENDOR-MISSING — a `[require]` dependency is declared but not vendored.\n\n\
             Dependencies resolve offline from the committed `vendor/` tree — Phorge never fetches on\n\
             `run`/`check`/`transpile`. Run `phg vendor` to clone each `[require]` dependency at its\n\
             pinned tag/rev into `vendor/` and write `phorge.lock`, then commit both.\n"
        }
        "E-VENDOR-MAIN" => {
            "E-VENDOR-MAIN — a vendored dependency declared `package Main`.\n\n\
             A dependency is a library: it exports dotted packages (e.g. `package acme.strutil;`),\n\
             never the reserved `package Main` (which would collide with the consuming program's\n\
             entry). Fix the dependency to use a dotted package, or remove the stray `main` File.\n"
        }
        "E-DUP-DEF" => {
            "E-DUP-DEF — two functions share a name within one package.\n\n\
             After the project + its vendored dependencies are merged, every function is keyed by\n\
             `(package, name)` and must be unique. Two files declaring the same `package` cannot both\n\
             define a function of the same name — rename one, or move it to a different package.\n"
        }
        "E-HTML-HOLE" => {
            "E-HTML-HOLE — a value of an un-renderable type was interpolated into `html\"…\"`.\n\n\
             An `html\"…\"` hole `{e}` accepts an `Html` fragment (embedded as-is), a `string`, or a\n\
             primitive (`int`/`float`/`bool`, escaped). Anything else — a class, enum, list, optional\n\
             — has no safe HTML rendering. Render it first: build it with the html builders\n\
             (`Html.el(…)`), produce a `string` and let the hole escape it, or wrap audited markup in\n\
             `Html.raw(…)`.\n"
        }
        "E-HTML-IMPORT" => {
            "E-HTML-IMPORT — `html\"…\"` was used without importing Core.Html.\n\n\
             The `html\"…\"` literal desugars to `Html.raw`/`Html.text`/`Html.concat` kernel calls, so\n\
             the module must be in scope. Add `import Core.Html;` (or `import Core.Html as h;`) to the\n\
             File.\n"
        }
        "E-NAME-CASE" => {
            "E-NAME-CASE — a value identifier is not camelCase.\n\n\
             Functions, methods, parameters, fields, variable bindings, and lambda parameters must be\n\
             camelCase: a lowercase first letter and no underscores (e.g. `splitOnce`, `cToF`, `area`).\n\
             This is the value half of Phorge's casing rule (types/enums/variants are PascalCase via\n\
             E-TYPE-CASE); both are front-end-only, so they never change the generated PHP. Rename the\n\
             identifier — the diagnostic suggests the converted form (`split_once` → `splitOnce`).\n"
        }
        "E-TYPE-CASE" => {
            "E-TYPE-CASE — a type identifier is not PascalCase.\n\n\
             Class names, enum names, enum variant names, and `type` alias names must be PascalCase: an\n\
             uppercase first letter and no underscores (e.g. `Shape`, `Circle`, `HttpRequest`). This is\n\
             the type half of Phorge's casing rule (functions/variables/params are camelCase via\n\
             E-NAME-CASE); both are front-end-only, so they never change the generated PHP. Rename the\n\
             type — the diagnostic suggests the converted form (`shape` → `Shape`).\n"
        }
        "E-PKG-CASE" => {
            "E-PKG-CASE — a package or import segment is not PascalCase.\n\n\
             Every package/folder segment is PascalCase (e.g. `package Acme.StringUtil;` lives in\n\
             `src/Acme/StringUtil/`), and so are import path segments and an import `as` alias\n\
             (`import Acme.StringUtil as Strutil;`). This makes the source-to-PHP namespace mapping 1:1\n\
             with no casing transform (`Acme.StringUtil` ⇒ `Acme\\StringUtil`). The reserved roots\n\
             `Main` (the runnable entry) and `Core` (the standard library) are already PascalCase. It is\n\
             front-end-only, so it never changes the generated PHP — rename the segment to the suggested\n\
             form (`acme` → `Acme`).\n"
        }
        "E-INSTANCEOF-TYPE" => {
            "E-INSTANCEOF-TYPE — an `instanceof` operand is not valid.\n\n\
             `value instanceof T` tests whether a class instance is of class/interface `T`. The right\n\
             operand must name a declared **class or interface** (M-RT S2 added interfaces); the left\n\
             operand must be a class instance. The result is `bool`, and inside `if (x instanceof T)`\n\
             the operand `x` is smart-cast to `T` in the then-block.\n"
        }
        "E-IFACE-IMPL" => {
            "E-IFACE-IMPL — a name in `implements`/`extends` is not an interface.\n\n\
             A class `implements` declared interfaces, and an interface `extends` other interfaces. A\n\
             name that resolves to a class, enum, or nothing cannot appear there. Declare the missing\n\
             `interface`, or remove the name.\n"
        }
        "E-IFACE-UNIMPL" => {
            "E-IFACE-UNIMPL — a class does not implement an interface method.\n\n\
             A class that `implements I` must provide every method of `I` and its `extends` chain. PHP\n\
             would fatal at class-declaration time, so Phorge rejects it up front. Add the missing\n\
             method (matching the interface's signature) to the class.\n"
        }
        "E-IFACE-SIG" => {
            "E-IFACE-SIG — a class method's signature does not match the interface's.\n\n\
             An implementing method must match the interface method's parameter types and return type\n\
             exactly (no variance this slice). Align the class method's signature with the interface\n\
             declaration.\n"
        }
        "E-IFACE-CYCLE" => {
            "E-IFACE-CYCLE — interfaces form an `extends` cycle.\n\n\
             `interface A extends B` while `B extends A` (directly or transitively) has no well-founded\n\
             method set. Break the cycle so every interface's `extends` chain bottoms out.\n"
        }
        "E-EXTEND-FINAL" => {
            "E-EXTEND-FINAL — a class extends a non-`open` class.\n\n\
             Phorge is final-by-default (M-RT S6): a class can only be a parent if it is declared\n\
             `open class`. Mark the parent `open` to allow extension, or remove the `extends`. (This is\n\
             the inheritance dual of the `mutable` opt-in — safe by default, opt into the power.)\n"
        }
        "E-EXTEND-UNKNOWN" => {
            "E-EXTEND-UNKNOWN — a class extends a name that is not a class.\n\n\
             `extends` lists parent *classes*; the name resolved to an interface, enum, or nothing.\n\
             Use `implements` for interfaces, or declare the missing parent class.\n"
        }
        "E-MI-CYCLE" => {
            "E-MI-CYCLE — classes form an `extends` cycle.\n\n\
             `class A extends B` while `B extends A` (directly or transitively) has no well-founded\n\
             member set. Break the cycle so every class's `extends` chain bottoms out at a root class.\n"
        }
        "E-MI-CONFLICT" => {
            "E-MI-CONFLICT — a method is inherited from more than one parent.\n\n\
             Under multiple inheritance (`class C extends A, B`, M-RT S6b), if two parents each supply a\n\
             method of the same name Phorge will not silently pick one. Resolve it in C's body with a\n\
             clause: `use P.m` (pick parent P's `m`), `rename P.m as n` (keep both under a new name),\n\
             `exclude P.m` (drop one), or override by declaring `function m(…)` in C. A diamond where\n\
             both arms reach the *same* declaring method auto-merges and is never a conflict.\n"
        }
        "E-USE-UNKNOWN" => {
            "E-USE-UNKNOWN — a `use` clause names something that is not a declared trait.\n\n\
             A class composes a trait with `use T;` (M-RT S8). The name must resolve to a `trait`, not a\n\
             class, interface, or undeclared name. If you meant to inherit a class, use `extends` (a\n\
             class is an *is-a* supertype); `use` is for *has-the-behavior-of* horizontal reuse. Declare\n\
             the trait with `trait T { … }`.\n"
        }
        "E-USE-AS-TYPE" => {
            "E-USE-AS-TYPE — a trait was used where a type is expected.\n\n\
             A trait (M-RT S8) is horizontal reuse, NOT a type: you cannot type a variable/parameter/\n\
             field as a trait, and `instanceof T` on a trait is rejected. Compose it into a class with\n\
             `use T;` and type values by the class (or by an interface the class implements).\n"
        }
        "E-TRAIT-CTOR-COLLISION" => {
            "E-TRAIT-CTOR-COLLISION — a class composes constructors from two or more traits.\n\n\
             A `use`d trait's constructor becomes the class's constructor (M-RT S8). A class can adopt at\n\
             most one — two trait constructors would collide (PHP fatals on this). Resolve it by giving\n\
             the class its own `constructor(…)` (which wins and runs the trait initializers explicitly),\n\
             or by composing only one ctor-bearing trait.\n"
        }
        "W-TRAIT-CTOR-SHADOWED" => {
            "W-TRAIT-CTOR-SHADOWED — a class's own constructor shadows a `use`d trait's constructor.\n\n\
             When a class declares its own `constructor` AND composes a trait that also has one, the\n\
             class's ctor wins and the trait's never runs (PHP P1). This is a warning, not an error —\n\
             intentional if you meant to override. If the trait's initializer must run, call it from the\n\
             class ctor or drop the class ctor.\n"
        }
        "W-TRAIT-CTOR-PARENT-SKIPPED" => {
            "W-TRAIT-CTOR-PARENT-SKIPPED — a trait constructor runs instead of the parent's.\n\n\
             When a class `extends` a parent that has a constructor AND composes a trait that also has\n\
             one (with no class ctor of its own), the trait's constructor wins and the parent's is NOT\n\
             auto-run (PHP P2). A warning so the silent skip is visible: give the class its own ctor that\n\
             initializes the parent if that matters.\n"
        }
        "E-MI-FIELD-CONFLICT" => {
            "E-MI-FIELD-CONFLICT — a field is inherited from more than one parent.\n\n\
             Under multiple inheritance (`class C extends A, B`, M-RT S6c), if two parents each declare\n\
             an instance field of the same name Phorge will not silently pick one. Unlike a method\n\
             collision there are no `use`/`rename`/`exclude` clauses — PHP has no `insteadof` for\n\
             properties. Resolve it by redeclaring the field in C (or renaming it in a parent). A\n\
             diamond where both arms reach the *same* declaring field auto-merges and is never a\n\
             conflict.\n"
        }
        "E-ABSTRACT-INSTANTIATE" => {
            "E-ABSTRACT-INSTANTIATE — an abstract class cannot be instantiated.\n\n\
             An `abstract class` (M-RT S6b) may have bodyless `abstract function` methods, so it has no\n\
             complete behavior to construct. Instantiate a concrete subclass that implements every\n\
             abstract method instead.\n"
        }
        "E-ABSTRACT-UNIMPL" => {
            "E-ABSTRACT-UNIMPL — a concrete class leaves an abstract method unimplemented.\n\n\
             A non-`abstract` class must provide a body for every `abstract` method it declares or\n\
             inherits. Implement the method (`function name(…) -> void { … }`), or declare the class itself\n\
             `abstract` so a further subclass implements it.\n"
        }
        "E-OPEN-STATIC" => {
            "E-OPEN-STATIC — a method is both `open` and `static`.\n\n\
             Static methods are resolved by name, not by an instance's runtime class, so they are not\n\
             virtual and cannot be overridden. Drop `open` (the method stays callable) or drop `static`\n\
             (the method becomes a normal, overridable instance method).\n"
        }
        "E-OVERRIDE-FINAL" => {
            "E-OVERRIDE-FINAL — a method overrides a non-`open` ancestor method.\n\n\
             Methods are final-by-default (M-RT S6): a subclass may only redefine a parent method that\n\
             the parent declared `open function`. Mark the parent method `open` to allow the override,\n\
             or rename the subclass method so it does not shadow the inherited one.\n"
        }
        "E-UNION-MEMBER" => {
            "E-UNION-MEMBER — a union member is not an allowed type.\n\n\
             A union `A | B` (M-RT S4) may combine classes, interfaces, and primitives\n\
             (`int | string`). Enum members, optional `T?` members, and function-typed members are not\n\
             supported this slice — an enum is already a closed sum (match its variants directly), and\n\
             optional/function members complicate the PHP `A|B` emission. Replace the member, or model\n\
             the case differently.\n"
        }
        "E-UNION-ARITY" => {
            "E-UNION-ARITY — a union needs two or more distinct types.\n\n\
             `A | A` (or any union whose members are all the same after normalization) collapses to a\n\
             single type, so it is not a union. Give the union at least two distinct members, or use the\n\
             single type directly.\n"
        }
        "E-MATCH-TYPE" => {
            "E-MATCH-TYPE — a `match` type pattern is invalid.\n\n\
             A type pattern (`Circle c => …`, M-RT S4) matches when the scrutinee is an instance of the\n\
             named **class or interface** — the same runtime test as `instanceof`. The name must be a\n\
             declared class or interface (not an enum — match an enum's variants directly), and a type\n\
             pattern is allowed only at the **top level** of a match arm, not nested inside a variant\n\
             pattern. Use it to match over a union scrutinee.\n"
        }
        "E-MATCH-GUARD-EXHAUST" => {
            "E-MATCH-GUARD-EXHAUST — a shape is covered only by guarded arms.\n\n\
             A match arm guard (`pat when <cond> => …`, pattern cluster) is an optional boolean\n\
             condition; a false guard falls through to the next arm. Because the guard might be false,\n\
             a guarded arm does NOT discharge its shape for exhaustiveness. If every arm matching a\n\
             given variant/type is guarded, the match can fall through with no arm — so add an\n\
             **unguarded** arm (or `_`) covering that shape as a fallback.\n"
        }
        "E-FIXEDLIST-LEN" => {
            "E-FIXEDLIST-LEN — a fixed-length list literal has the wrong length.\n\n\
             A `[T; N]` fixed-length list (Phase 1 types slice) has a compile-time length `N`. When a\n\
             list literal initializes one, the literal must have exactly `N` elements: `[int; 3] rgb =\n\
             [255, 128, 0];` (ok) but `[int; 2] p = [1, 2, 3];` is this error. Adjust the literal or the\n\
             declared length.\n"
        }
        "E-FIXEDLIST-BOUNDS" => {
            "E-FIXEDLIST-BOUNDS — a literal index is out of bounds for a fixed-length list.\n\n\
             Indexing a `[T; N]` with a *constant* index is bounds-checked at compile time: valid\n\
             indices are `0..N`, so `pair[5]` on a `[int; 2]` is this error. A non-literal index\n\
             (`pair[i]`) is left to the runtime bounds check, exactly like a `List<T>`.\n"
        }
        "E-OR-PATTERN-BIND" => {
            "E-OR-PATTERN-BIND — an or-pattern alternative binds a variable.\n\n\
             An or-pattern groups alternatives that share one arm body: `match n { 1 | 2 | 3 => \"low\",\n\
             _ => \"hi\" }`. Because any alternative can match, the shared body cannot know which one\n\
             did — so no alternative may be a catch-all (`_` or a bare name) or introduce a binding\n\
             (`Some(n)`, `Circle c`, a struct-field binder). Concrete patterns and `_` *sub*-patterns\n\
             are fine (`Some(_) | None()`). If you need to bind, write separate arms instead.\n"
        }
        "E-GUARD-TYPE" => {
            "E-GUARD-TYPE — a match arm guard is not boolean.\n\n\
             The condition after `when` in a match arm (`pat when <cond> => …`) is a boolean test,\n\
             evaluated with the arm's pattern bindings in scope. It must have type `bool` — wrap a\n\
             non-boolean value in a comparison (`when n > 0`) rather than relying on truthiness.\n"
        }
        "E-STRUCT-PAT-TYPE" => {
            "E-STRUCT-PAT-TYPE — a struct pattern's head is not a class.\n\n\
             A struct pattern (`Point { x, y } => …`, pattern cluster S5.2) destructures a class\n\
             instance's named fields — its head must be a declared **class**. An interface has no\n\
             fields (use a type pattern `Iface x` to bind it); an enum is matched by its variants\n\
             (`Some(v)`), not by fields.\n"
        }
        "E-STRUCT-FIELD-UNKNOWN" => {
            "E-STRUCT-FIELD-UNKNOWN — a struct pattern names a field the class does not declare.\n\n\
             Each `field` (or `field: sub-pattern`) in a struct pattern (`Point { x, y }`) must be a\n\
             field declared on the class (including inherited fields). Destructure only declared\n\
             fields — check for a typo or a field on a different class.\n"
        }
        "E-PATTERN-DUP-BIND" => {
            "E-PATTERN-DUP-BIND — a pattern binds the same name twice.\n\n\
             A struct pattern (`Point { x, y: x }`) or any nested pattern must give each destructured\n\
             binding a distinct name — two bindings of `x` would have one silently shadow the other.\n\
             Rename one (`Point { x, y: y2 }`).\n"
        }
        "E-INTERSECT-MEMBER" => {
            "E-INTERSECT-MEMBER — an intersection member is not an allowed type.\n\n\
             An intersection `A & B` (M-RT S5) combines interfaces, plus *at most one* concrete class\n\
             (`Cls & I & J`). Primitives, enums, optional `T?` members, and function-typed members are\n\
             not allowed — a value satisfies an intersection by being a single instance that conforms to\n\
             every member, which only interfaces (and one class) express. Replace the member.\n"
        }
        "E-INTERSECT-MULTI-CLASS" => {
            "E-INTERSECT-MULTI-CLASS — an intersection names two or more concrete classes.\n\n\
             A value has exactly one class, so it can never be an instance of two distinct classes at\n\
             once — `Cat & Dog` is uninhabited. Name at most one class and compose the rest with\n\
             interfaces. (A second class becomes meaningful only once class `extends` lands in S6.)\n"
        }
        "E-INTERSECT-ARITY" => {
            "E-INTERSECT-ARITY — an intersection needs two or more distinct types.\n\n\
             `A & A` (or any intersection whose members are all the same after normalization) collapses\n\
             to a single type, so it is not an intersection. Give it at least two distinct members, or\n\
             use the single type directly.\n"
        }
        "E-INTERSECT-SIG" => {
            "E-INTERSECT-SIG — intersection members share a method with conflicting signatures.\n\n\
             Two members of `A & B` declare the same method with different parameter or return types.\n\
             A class satisfying the intersection would need that one method to conform to both — which\n\
             the current overload-agnostic intersection check cannot verify — so the intersection is\n\
             rejected. Align the shared method's signature across the members (or drop one).\n"
        }
        "E-INTERSECT-NO-MEMBER" => {
            "E-INTERSECT-NO-MEMBER — a member access on an intersection resolves to nothing.\n\n\
             A method/field call on an `A & B` value searches every member (each interface, plus the\n\
             lone class for fields). None of them declares the named method or field. Check the name, or\n\
             add it to one of the intersection's members.\n"
        }
        "E-OVERLOAD-RETURN" => {
            "E-OVERLOAD-RETURN — overloads of one name must share a return type.\n\n\
             Phorge overloading is dynamic multiple dispatch: the runtime argument types choose the\n\
             overload, so the compiler cannot know which one fires at a polymorphic call. Requiring a\n\
             single return type keeps every overloaded call statically typed. Overloads model one\n\
             operation over different argument types; if the return must vary with the input, use a\n\
             generic function (`f<T>(T) -> T`) or separate names.\n"
        }
        "E-OVERLOAD-DUPLICATE" => {
            "E-OVERLOAD-DUPLICATE — two overloads have identical parameter types.\n\n\
             Each overload of a name must be distinguishable by its parameter signature (arity or\n\
             parameter types). Two declarations with the same parameters are redundant and could never\n\
             be told apart at a call. Remove one, or change its parameters.\n"
        }
        "E-OVERLOAD-GENERIC" => {
            "E-OVERLOAD-GENERIC — a generic function/method cannot be overloaded.\n\n\
             A generic declaration (`f<T>(…)`) must be the only one with its name. Generic overloading\n\
             (mixing `<T>` overloads with concrete ones) is not supported. Remove the type parameters\n\
             and write concrete overloads, or rename one declaration.\n"
        }
        "E-OVERLOAD-NO-MATCH" => {
            "E-OVERLOAD-NO-MATCH — no overload accepts the call's argument types.\n\n\
             The call's static argument types match no overload's parameter types (by arity or\n\
             assignability). Check the argument types against the available overloads; an argument\n\
             whose static type is a supertype of every overload's parameter cannot be dispatched.\n"
        }
        "E-OVERLOAD-FN-VALUE" => {
            "E-OVERLOAD-FN-VALUE — an overloaded function has no single first-class value.\n\n\
             A bare reference to an overloaded function (`var g = f;`) is ambiguous — there is no one\n\
             signature to give the value. Call the function directly, or wrap the intended overload in\n\
             a lambda (`var g = fn(int x) => f(x);`).\n"
        }
        "E-MISSING-RETURN" => {
            "E-MISSING-RETURN — a function does not return a value on every path.\n\n\
             A function whose declared return type carries a value (`-> int`, `-> Shape`, …) must\n\
             `return` (or diverge) on *every* control-flow path. The classic leak is an `if` with no\n\
             `else`: the false branch falls through to the end. Add a trailing `return`, give the `if`\n\
             an `else` that also returns, or diverge (an infinite loop / a `-> never` call). A `-> void`\n\
             or `-> Empty` function carries no value and is exempt.\n"
        }
        "E-MISSING-RETURN-TYPE" => {
            "E-MISSING-RETURN-TYPE — a function or method declares no return type.\n\n\
             Every function and method must declare its return type — including `main`. Add `-> void`\n\
             for a side-effecting function that returns nothing, `-> Empty` to return the holdable\n\
             empty value, or the concrete type it returns (`-> int`, `-> Shape`, …). Constructors have\n\
             no return slot and property hooks are typed by their property, so neither needs one;\n\
             expression-body lambdas (`fn(x) => e`) infer their return from the expression.\n"
        }
        "E-VOID-CAPTURE" => {
            "E-VOID-CAPTURE — a `void` value cannot be captured.\n\n\
             `void` is the type of an expression that produces *nothing* (a side-effecting call like\n\
             `Console.println(…)`), so there is nothing to bind: `var x = note(\"hi\");` is rejected.\n\
             Call it as a statement instead (drop the binding). If you genuinely need to hold the\n\
             empty value — e.g. to satisfy a generic slot — annotate it `Empty` (`Empty x = note(…);`):\n\
             `void` widens to the holdable `Empty`.\n"
        }
        "E-NEVER-RETURN" => {
            "E-NEVER-RETURN — a `-> never` function can return normally.\n\n\
             `never` is the bottom type: a function annotated `-> never` promises it never returns —\n\
             it must diverge on every path (today, an infinite loop or a call to another `never`\n\
             function; once `throw` lands, also by throwing). This body can fall through and return.\n\
             Make it diverge, or drop the `never` return type.\n"
        }
        "W-UNREACHABLE" => {
            "W-UNREACHABLE — a statement can never be reached (warning).\n\n\
             A preceding statement always returns or diverges (a `return`, an infinite loop, or a call\n\
             to a `-> never` function), so the flagged statement is dead code. This is a non-fatal\n\
             lint — remove the unreachable statements. It never blocks the build.\n"
        }
        "W-MATCH-UNREACHABLE" => {
            "W-MATCH-UNREACHABLE — a `match` arm can never be reached (warning).\n\n\
             Either an earlier arm is a catch-all (`_` or a bare identifier binding, which matches\n\
             everything) so later arms are dead, or this arm duplicates an earlier literal/variant/type\n\
             pattern. Reorder so the catch-all is last, or remove the duplicate. Non-fatal lint.\n"
        }
        "E-PROPAGATE-POSITION" => {
            "E-PROPAGATE-POSITION — `?` used outside a let-initializer.\n\n\
             The `?` error-propagation operator is allowed only as the *whole* initializer of a binding\n\
             (`int a = mayFail()?;`). It is not allowed nested in a larger expression (`g(f()?)`) or in a\n\
             `return` — PHP, the transpile target, cannot return from the caller inside an expression.\n\
             Bind the call's result to a local first, then handle it (M-faults).\n"
        }
        "E-PROPAGATE-CONTEXT" => {
            "E-PROPAGATE-CONTEXT — `?` in a function that can't propagate the error.\n\n\
             `?` unwraps an `Ok` or early-returns the `Err`, so it requires a `Result`-shaped operand\n\
             (an enum with `Ok`/`Err` variants) AND an enclosing function that returns that same\n\
             `Result`. Declare the function to return `Result<…>`, or handle the value with a `match`.\n"
        }
        "E-PROPAGATE-ERR" => {
            "E-PROPAGATE-ERR — `?` propagates an incompatible error type.\n\n\
             The operand's `Err` payload type must be assignable to the enclosing function's `Err`\n\
             payload type (it is the value `?` early-returns). Widen the function's error type, or map\n\
             the error before propagating.\n"
        }
        "E-RESERVED-INTRINSIC" => {
            "E-RESERVED-INTRINSIC — a reserved built-in name was redefined.\n\n\
             `panic`, `todo`, `unreachable`, and `assert` are built-in fault intrinsics (M-faults) and\n\
             cannot be declared as user functions. Rename your function.\n"
        }
        "E-INTRINSIC-LITERAL" => {
            "E-INTRINSIC-LITERAL — a fault intrinsic's message must be a string literal.\n\n\
             `panic(\"…\")` and `assert(cond, \"…\")` bake their message at compile time, so it must be a\n\
             plain string literal — no interpolation or computed expression (yet). Use a literal, or\n\
             compute the message into a local for a future dynamic form.\n"
        }
        "E-THROW-TYPE" => {
            "E-THROW-TYPE — only an `Error` value may be thrown or declared.\n\n\
             `throw e` requires `e` to be a value whose type implements the built-in `Error` marker\n\
             (`class Foo implements Error { … }`), and a `throws T` declaration requires the same of\n\
             `T`. You cannot throw a primitive, enum, or arbitrary object.\n"
        }
        "E-THROW-UNDECLARED" => {
            "E-THROW-UNDECLARED — a thrown exception is neither caught nor declared.\n\n\
             A checked exception must be discharged: wrap the `throw` (or the throwing call) in a\n\
             `try { … } catch (T e) { … }`, or add `throws T` to the enclosing function so callers\n\
             handle it. Phorge enforces this at compile time — nothing leaks silently.\n"
        }
        "E-CALL-UNHANDLED" => {
            "E-CALL-UNHANDLED — a call can throw a checked exception that isn't handled.\n\n\
             Calling a `throws T` function obliges the caller to handle `T`: catch it in an enclosing\n\
             `try`/`catch`, or propagate it with `?` AND declare `throws T` on the enclosing function.\n\
             A bare call may not silently let the exception escape.\n"
        }
        "E-UNCAUGHT-THROW" => {
            "E-UNCAUGHT-THROW — an exception escapes `main`.\n\n\
             `main` is the program entry point: it may not declare `throws`, and every exception it\n\
             (or anything it calls) can raise must be caught before it escapes. Wrap the throwing code\n\
             in a `try { … } catch (T e) { … }` inside `main`.\n"
        }
        "E-THROWS-TOO-BROAD" => {
            "E-THROWS-TOO-BROAD — `throws Error` is too broad.\n\n\
             Declare the *specific* exception type(s) a function throws (`throws BadInput`), not the\n\
             bare `Error` root, so callers know exactly what to catch. A `catch (Error e)` is still\n\
             allowed — catching broad is fine; declaring broad is not.\n"
        }
        "E-CATCH-TYPE" => {
            "E-CATCH-TYPE — a `catch` clause names a non-`Error` type.\n\n\
             A `catch (T e)` requires `T` (or every member of a union `catch (A | B e)`) to implement\n\
             the built-in `Error` marker — you can only catch what can be thrown. Catching the `Error`\n\
             base itself is allowed (it matches every exception).\n"
        }
        "W-CATCH-UNREACHABLE" => {
            "W-CATCH-UNREACHABLE — a `catch` clause can never run (warning).\n\n\
             An earlier clause in the same `try` already catches this type (it is the same as, or a\n\
             supertype of, this one), so control never reaches it. Remove the dead clause, or reorder\n\
             so the more specific type comes first. This is a lint — it never fails the build.\n"
        }
        "E-CONST-NO-INIT" => {
            "E-CONST-NO-INIT — a `const` class constant has no initializer.\n\n\
             A constant is fixed at declaration, so it must be assigned a value: `const int MAX = 100;`.\n"
        }
        "E-CONST-NOT-LITERAL" => {
            "E-CONST-NOT-LITERAL — a `const` initializer is not a compile-time literal.\n\n\
             A class constant must be a literal (int/float/bool/string/null) this slice — not a call,\n\
             method, or another expression. For a computed class-level value, use a `static` field (or,\n\
             once available, an expression field initializer).\n"
        }
        "E-CONST-MUTABLE" => {
            "E-CONST-MUTABLE — a `const` was also declared `mutable`.\n\n\
             A constant is immutable by definition; `const mutable` is contradictory. Drop `mutable`, or\n\
             use a `static mutable` field for class-level mutable state.\n"
        }
        "E-CONST-INIT-TYPE" => {
            "E-CONST-INIT-TYPE — a `const` initializer's type does not match its declared type.\n\n\
             The literal must be assignable to the constant's type — e.g. `const int MAX = 100;`, not\n\
             `const int MAX = \"x\";`.\n"
        }
        "E-CONST-CASE" => {
            "E-CONST-CASE — a `const` name is not SCREAMING_SNAKE_CASE.\n\n\
             Class constants follow the PHP/C/Java convention: uppercase letters, digits, and `_`\n\
             (`MAX`, `MAX_SIZE`, `HTTP_2`). Rename `maxVal` to `MAX_VAL`.\n"
        }
        "E-CONST-VISIBILITY" => {
            "E-CONST-VISIBILITY — a `private`/`protected` constant was read from outside its class.\n\n\
             A `private const` is readable only inside the declaring class; a `protected const` only\n\
             inside that class and its subclasses. Make it `public` (the default) to read it elsewhere,\n\
             or access it from within the class hierarchy.\n"
        }
        "E-FIELD-VISIBILITY" => {
            "E-FIELD-VISIBILITY — a `private`/`protected` field was read or written from outside its scope.\n\n\
             A `private` field is reachable only inside the declaring class; a `protected` field only\n\
             inside that class and its subclasses (an un-annotated field is `public`). The check runs in\n\
             the type-checker so every backend agrees — without it a `private` read would pass on the\n\
             Phorge interpreter/VM but throw in the transpiled PHP. Add a public accessor method (e.g.\n\
             `function valueOf() -> int { return this.value; }`), or declare the field `public`.\n"
        }
        "E-METHOD-VISIBILITY" => {
            "E-METHOD-VISIBILITY — a `private`/`protected` method was called from outside its scope.\n\n\
             A `private` method is callable only inside the declaring class; a `protected` method only\n\
             inside that class and its subclasses (an un-annotated method is `public`). Enforced in the\n\
             type-checker so the interpreter, VM, and transpiled PHP all reject the same call. Call it\n\
             through a public method of the class, or make the method `public`.\n"
        }
        "E-CONST-INSTANCE-ACCESS" => {
            "E-CONST-INSTANCE-ACCESS — a constant was read through an instance.\n\n\
             A `const` lives on the class, not the instance: read it as `ClassName.NAME`, never\n\
             `instance.NAME` (the same class-name-only rule as a `static` field).\n"
        }
        "E-CONST-REASSIGN" => {
            "E-CONST-REASSIGN — a `const` class constant was assigned to.\n\n\
             Constants are fixed at declaration and can never be reassigned. For class-level state that\n\
             changes, use a `static mutable` field instead.\n"
        }
        "E-FIELD-INIT-FORWARD-REF" => {
            "E-FIELD-INIT-FORWARD-REF — a field initializer reads a not-yet-initialized field.\n\n\
             Expression field initializers run per-instance at construction, in declaration order, after\n\
             the promoted constructor params are bound. An initializer may read `this` and any\n\
             EARLIER-declared field (or a promoted param) — but not a later field, nor itself. Declare\n\
             the field it depends on first, or set this one in the constructor.\n"
        }
        "E-FIELD-INIT-TYPE" => {
            "E-FIELD-INIT-TYPE — a field initializer's type does not match the field's declared type.\n\n\
             The initializer expression must be assignable to the field's type — e.g. `int weight =\n\
             compute(3);`, not `int weight = \"x\";`.\n"
        }
        "E-DESTRUCTURE-TYPE" => {
            "E-DESTRUCTURE-TYPE — a struct destructuring's value is not the named class.\n\n\
             `var Point { x, y } = p;` (Phase 1 slice 5) requires `p` to be a `Point` (or a subtype) so\n\
             the binding always succeeds. Destructure the value at its own type, or `match` on it if it\n\
             is a union/interface whose concrete type isn't statically known.\n"
        }
        "E-DESTRUCTURE-NOT-CLASS" => {
            "E-DESTRUCTURE-NOT-CLASS — a struct destructuring's head is not a class.\n\n\
             `var Name { … } = e;` destructures a class instance's fields, so `Name` must be a declared\n\
             class. To destructure a list, use the list form `var [a, b] = e else { … };`.\n"
        }
        "E-DESTRUCTURE-FIELD-UNKNOWN" => {
            "E-DESTRUCTURE-FIELD-UNKNOWN — a struct destructuring names a field the class does not have.\n\n\
             Each `field` (or `field: binding`) in `var Point { x, y } = p;` must be a field declared on\n\
             the class (including inherited fields). Bind only declared fields.\n"
        }
        "E-DESTRUCTURE-NOT-LIST" => {
            "E-DESTRUCTURE-NOT-LIST — a list destructuring's value is not a list.\n\n\
             `var [a, b] = e else { … };` requires `e` to be a `List<T>` or a fixed-length `[T; N]`. To\n\
             destructure a class instance, use the struct form `var Type { … } = e;`.\n"
        }
        "E-DESTRUCTURE-NEEDS-ELSE" => {
            "E-DESTRUCTURE-NEEDS-ELSE — a refutable list destructuring has no `else`.\n\n\
             A `List<T>` carries no static length, so `var [a, b] = xs;` can fail at runtime. It must\n\
             carry an `else { … }` that bails out (returns / throws / breaks / continues) when the\n\
             length doesn't match — the Swift `guard let` model. (A fixed-length `[T; N]` whose length\n\
             matches the binder count is irrefutable and takes no `else`.)\n"
        }
        "E-DESTRUCTURE-ELSE-IRREFUTABLE" => {
            "E-DESTRUCTURE-ELSE-IRREFUTABLE — an irrefutable destructuring has an `else`.\n\n\
             A struct destructuring, and a list destructuring over a length-matching `[T; N]`, always\n\
             succeed — so they cannot have an `else`. Remove it; the binding is unconditional.\n"
        }
        "E-DESTRUCTURE-ELSE-FALLTHROUGH" => {
            "E-DESTRUCTURE-ELSE-FALLTHROUGH — a destructuring `else` can fall through.\n\n\
             When the refutable destructuring fails, its binders are never created, so control must not\n\
             continue past the `else`. End every path of the `else` with `return` / `throw` / `break` /\n\
             `continue` (it is a bail-out block, like a `guard let … else`).\n"
        }
        "E-DESTRUCTURE-DUP-BIND" => {
            "E-DESTRUCTURE-DUP-BIND — a destructuring binds the same name twice.\n\n\
             Each binder in a destructuring must be distinct: `var [a, a] = xs` and `var Point { x, x }\n\
             = p` are errors. Rename one binding (`var Point { x, y: x2 } = p`).\n"
        }
        "E-FIXEDLIST-DESTRUCTURE-LEN" => {
            "E-FIXEDLIST-DESTRUCTURE-LEN — a list destructuring's arity differs from the fixed length.\n\n\
             Destructuring a fixed-length `[T; N]` is irrefutable only when the pattern binds exactly\n\
             `N` elements: `var [a, b] = pair;` needs `pair: [T; 2]`. Bind exactly `N` elements, or\n\
             destructure a `List<T>` with an `else` if the length is not statically known.\n"
        }
        _ => return None,
    };
    Some(body.to_string())
}

/// `explain <code>`: print the explanation for a diagnostic code, or error on an unknown one.
pub fn cmd_explain(code: &str) -> Result<String, String> {
    explain_text(code).ok_or_else(|| {
        format!(
            "unknown diagnostic code `{code}` \
             (known: E-NO-PACKAGE, E-RESERVED-PACKAGE, E-PKG-PATH, E-PKG-TYPE, E-VENDOR-MISSING, E-VENDOR-MAIN, E-DUP-DEF, E-UNKNOWN-IDENT, E-UNKNOWN-TYPE, E-INFER-NULL, E-ALIAS-CYCLE, E-RANGE-TYPE, E-OPT-ASSIGN, E-OPT-USE, E-IF-LET-TYPE, E-OPT-UNWRAP, W-FORCE-UNWRAP, E-LAMBDA-THIS, E-SHADOW-FN, E-NAME-CASE, E-TYPE-CASE, E-PKG-CASE, E-INSTANCEOF-TYPE, E-IFACE-IMPL, E-IFACE-UNIMPL, E-IFACE-SIG, E-IFACE-CYCLE, E-MAP-KEY, E-UNION-MEMBER, E-UNION-ARITY, E-MATCH-TYPE, E-INTERSECT-MEMBER, E-INTERSECT-MULTI-CLASS, E-INTERSECT-ARITY, E-INTERSECT-SIG, E-INTERSECT-NO-MEMBER, E-HOOK-NO-GET, E-HOOK-NO-SET, E-HOOK-TYPE, E-HOOK-DUP, E-FIELD-VISIBILITY, E-METHOD-VISIBILITY, E-VIS-PRIVATE, E-VIS-INTERNAL, E-PROPAGATE-POSITION, E-PROPAGATE-CONTEXT, E-PROPAGATE-ERR, E-RESERVED-INTRINSIC, E-INTRINSIC-LITERAL, E-THROW-TYPE, E-THROW-UNDECLARED, E-CALL-UNHANDLED, E-UNCAUGHT-THROW, E-THROWS-TOO-BROAD, E-CATCH-TYPE, W-CATCH-UNREACHABLE, E-STRUCT-PAT-TYPE, E-STRUCT-FIELD-UNKNOWN, E-PATTERN-DUP-BIND, E-OR-PATTERN-BIND, E-FIXEDLIST-LEN, E-FIXEDLIST-BOUNDS, E-DESTRUCTURE-TYPE, E-DESTRUCTURE-NOT-CLASS, E-DESTRUCTURE-FIELD-UNKNOWN, E-DESTRUCTURE-NOT-LIST, E-DESTRUCTURE-NEEDS-ELSE, E-DESTRUCTURE-ELSE-IRREFUTABLE, E-DESTRUCTURE-ELSE-FALLTHROUGH, E-DESTRUCTURE-DUP-BIND, E-FIXEDLIST-DESTRUCTURE-LEN)"
        )
    })
}
