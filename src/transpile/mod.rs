//! Phorj → PHP transpiler. Walks the untyped AST (the same AST the evaluator walks)
//! and emits runnable PHP 8.x source. Entry point: [`emit`].
use crate::ast::*;
use crate::dispatch::ParamKind;
use std::collections::{BTreeSet, HashMap, HashSet};

/// Transpile a parsed program to PHP source. Returns the PHP text, or a
/// `transpile error: …` message for an unsupported construct.
pub fn emit(program: &Program) -> Result<String, String> {
    let mut t = Transpiler::new();
    t.class_implements = crate::ast::class_implements(program);
    t.class_tables = crate::native::ClassTables::from_program(program);
    t.consts = crate::ast::class_consts(program).into_keys().collect();
    t.decomposed = decomposed_classes(program);
    t.collect(program);
    t.emit_program(program)?;
    Ok(t.out)
}

/// A statically-resolved operand "kind" used by the transpiler's T6 specialization to pick a native
/// PHP operator over a runtime helper. Deliberately scalar-only — the cases where PHP's loose
/// semantics diverge from Phorj's (`+` concat-vs-add, `/` int-vs-float, interpolation display).
/// Anything the resolver cannot pin down is [`OpKind::Other`], which routes to the existing helper
/// (the safe fallback), so a wrong guess can never happen — only "known" or "fall back".
#[derive(Clone, PartialEq, Eq, Debug)]
enum OpKind {
    Str,
    Int,
    Float,
    /// `decimal` (M-NUM S1). A decimal operand routes `+ - *` to the `__phorj_dec_*` BCMath helpers
    /// (exact + i128-bounds-checked), and a decimal value erases to a PHP `string` for display.
    Decimal,
    Bool,
    /// A value of a user-defined class/enum/interface, carrying its name so a field read resolves
    /// through `class_field_kinds` (T6b). Never an arithmetic/display operand itself.
    Class(String),
    /// `List<E>` carrying its element kind, so `xs[i]` resolves to `E` (T6d) — `xs[i] + 1` / `"{xs[i]}"`.
    List(Box<OpKind>),
    /// `Map<K, V>` carrying key+value kinds, so `m[k]` resolves to `V` (T6d).
    Map(Box<OpKind>, Box<OpKind>),
    Other,
}

/// Map a checker [`Ty`] (a native's declared return type) to its [`OpKind`] (T6d), so a native-call
/// result (`Text.upper(s)`, `List.length(xs)`) resolves as an operand. Mirrors [`kind_of_type`] over
/// the `Ty` representation; anything non-scalar/non-container is `Other` (→ helper fallback).
fn opkind_of_ty(ty: &crate::types::Ty) -> OpKind {
    use crate::types::Ty;
    match ty {
        Ty::Int => OpKind::Int,
        Ty::Float => OpKind::Float,
        Ty::Decimal => OpKind::Decimal,
        Ty::String => OpKind::Str,
        Ty::Bool => OpKind::Bool,
        Ty::List(e) => OpKind::List(Box::new(opkind_of_ty(e))),
        Ty::Map(k, v) => OpKind::Map(Box::new(opkind_of_ty(k)), Box::new(opkind_of_ty(v))),
        Ty::Named(name, _) => OpKind::Class(name.clone()),
        _ => OpKind::Other,
    }
}

/// Map a (post-checker, resolved) type annotation to its scalar [`OpKind`]. Non-scalars (classes,
/// `void`, optionals, …) are `Other` — their values aren't the arithmetic/display operands T6
/// specializes, and the helper fallback covers any that slip through.
fn kind_of_type(ty: &Type) -> OpKind {
    match ty {
        Type::Named { name, args, .. } => match name.as_str() {
            "int" => OpKind::Int,
            "float" => OpKind::Float,
            "decimal" => OpKind::Decimal,
            "string" => OpKind::Str,
            "bool" => OpKind::Bool,
            // Containers carry their element kinds so an index read resolves as an operand (T6d).
            "List" => OpKind::List(Box::new(args.first().map_or(OpKind::Other, kind_of_type))),
            "Map" => OpKind::Map(
                Box::new(args.first().map_or(OpKind::Other, kind_of_type)),
                Box::new(args.get(1).map_or(OpKind::Other, kind_of_type)),
            ),
            // Non-arithmetic primitives — no native operand specialization.
            "void" | "never" | "empty" | "bytes" | "Set" => OpKind::Other,
            // A user-defined class/enum/interface name → `Class`, so field reads on a value of this
            // type resolve through `class_field_kinds` (T6b).
            other => OpKind::Class(other.to_string()),
        },
        _ => OpKind::Other,
    }
}

/// The set of classes that must lower to the interface+trait decomposition (M-RT S6b): every
/// transitive ancestor of any multi-parent (`extends A, B`) class. A multi-parent class itself is
/// emitted as a class that `implements`+`use`s (see [`Transpiler::emit_multi_class`]) and is *not*
/// in this set, unless it is also an ancestor of another multi-parent class.
fn decomposed_classes(program: &Program) -> BTreeSet<String> {
    let parents: HashMap<&str, &[String]> = program
        .items
        .iter()
        .filter_map(|it| match it {
            Item::Class(c) => Some((c.name.as_str(), c.extends.as_slice())),
            _ => None,
        })
        .collect();
    let mut out: BTreeSet<String> = BTreeSet::new();
    // Seed: the direct parents of every multi-parent class; then close upward over `extends`.
    let mut queue: Vec<String> = Vec::new();
    for it in &program.items {
        if let Item::Class(c) = it {
            if c.extends.len() >= 2 {
                queue.extend(c.extends.iter().cloned());
            }
        }
    }
    while let Some(name) = queue.pop() {
        if !out.insert(name.clone()) {
            continue;
        }
        if let Some(ps) = parents.get(name.as_str()) {
            queue.extend(ps.iter().cloned());
        }
    }
    out
}

struct Transpiler {
    funcs: HashSet<String>,
    /// Foreign PHP free functions declared via `declare function …;` (M8.5 interop). They are **not**
    /// emitted as PHP definitions (PHP already has them) and a call to one is emitted as the global form
    /// `\name(…)` (so it resolves to the PHP builtin even inside a namespace block). Kept separate from
    /// `funcs` so the emit loop skips them and `emit_call` routes them to the `\`-prefixed form.
    foreign_fns: HashSet<String>,
    /// Foreign PHP classes declared via `declare class … { … }` (M8.5 S2). Also kept in `classes` so
    /// construction and member-call resolution work; this set additionally routes construction to
    /// `new \Name(…)` and static calls to `\Name::s(…)` (global PHP), and suppresses the class
    /// definition (PHP already has it). Instance method/field access (`$o->m`, `$o->f`) needs no name.
    foreign_classes: HashSet<String>,
    classes: HashSet<String>,
    /// `(class, NAME)` pairs that name a `const` class constant (Feature A), inheritance/traits already
    /// flattened (the shared [`crate::ast::class_consts`] table). A `ClassName.NAME` access whose pair
    /// is in this set emits as `ClassName::NAME` (no `$`) — checked before the static-field `::$name`
    /// path. PHP resolves an inherited `Sub::MAX` itself, so only the keys are needed.
    consts: HashSet<(String, String)>,
    variants: HashSet<String>,
    variant_fields: HashMap<String, Vec<String>>,
    /// An enum variant's PHP namespace (`namespace_of` of the — possibly mangled — enum name), so a
    /// cross-package variant is constructed and `instanceof`-tested as a fully-qualified class
    /// (`new \Acme\Geometry\Circle(…)`). A `package Main` (bare) enum maps to `Main` ⇒ bare emission.
    variant_ns: HashMap<String, String>,
    out: String,
    indent: usize,
    locals: Vec<HashSet<String>>,
    /// Scoped operand-type environment (T6), parallel to `locals` (pushed/popped together). Maps a
    /// local/param/loop-var name to its scalar [`OpKind`] **where statically known** — so `+`, `/`,
    /// `%`, and interpolation can emit native PHP operators (`.`/`+`/`intdiv`/`fmod`/direct casts)
    /// instead of the `__phorj_add`/`_div`/`_rem`/`_str` runtime helpers. A name absent here resolves
    /// to [`OpKind::Other`] → the helper is emitted as a safe fallback (never a byte-identity risk).
    local_kinds: Vec<HashMap<String, OpKind>>,
    cur_class_fields: Option<HashSet<String>>,
    /// The class whose members are being emitted, for `this` operand-kind resolution (T6b). Set
    /// around `emit_class_members`, restored after.
    cur_class: Option<String>,
    /// B2 — active trait-alias map for `parent.m(…)` / `parent(A).m(…)` calls emitted inside an
    /// **MI class** or a **decomposed trait body**, where PHP has no native `parent::`/`A::` target
    /// (the ancestor lives in a `use`d trait). `Some` only while emitting such a body; keyed by the
    /// call's `(ancestor-as-written, method)`, valued by the `private` trait alias the `use` block
    /// declares (`T<dp>::m as private __super_<dp>_<m>` ⇒ `$this->__super_<dp>_<m>(…)`). A parent call
    /// absent from the map while this is `Some` targets a non-direct ancestor (a transitive MI jump) —
    /// not yet lowerable, surfaced as a transpile error rather than invalid PHP.
    parent_aliases: Option<std::collections::BTreeMap<(Option<String>, String), String>>,
    /// `class → (field/hook/promoted-ctor-param name → OpKind)` — operand kinds of a class's *own*
    /// members (T6b). Field reads (`p.x`, `this.x`) resolve through here + the parent chain
    /// (`class_parents`), so `p.x + 1` / `"{p.x}"` emit native PHP instead of a runtime helper.
    class_field_kinds: HashMap<String, HashMap<String, OpKind>>,
    /// `class → direct parents` (`extends`), for inherited-field kind lookup (T6b).
    class_parents: HashMap<String, Vec<String>>,
    /// `variant → payload field OpKinds` (positional), so a variant-payload match binding (`Pass(s)`)
    /// resolves `s`'s kind for native operand specialization (T6b).
    variant_field_kinds: HashMap<String, Vec<OpKind>>,
    /// `free-function name → return OpKind` (T6c), so a call result (`bulk(x)`, `"{f(x)}"`) resolves
    /// to a native operand. Overloads with differing return kinds collapse to `Other` (the fallback).
    fn_ret_kinds: HashMap<String, OpKind>,
    /// `(class, method) → return OpKind` (T6c), with `extends`-chain lookup, so a method-call result
    /// (`p.price()`, `c.get() + 1`) resolves. Differing overloads collapse to `Other`.
    method_ret_kinds: HashMap<(String, String), OpKind>,
    /// Active import map (leaf qualifier → full dotted module path) — how a namespaced native call
    /// `console.println(x)` is distinguished from a method call on a value (M3 Wave 1). The
    /// transpiler tracks no variable scope, so unlike the interpreter/compiler it cannot use a
    /// locals-first heuristic; the import map is the authority.
    imports: HashMap<String, String>,
    /// Set when `/`, `%`, an interpolation, or a range is emitted — each defines a once-per-file
    /// runtime helper (M7) that reproduces Phorj's type-driven semantics under PHP's looser rules:
    /// `__phorj_div` (int `/` ⇒ `intdiv`), `__phorj_rem` (float `%` ⇒ `fmod`), `__phorj_str`
    /// (bool ⇒ `"true"/"false"`), `__phorj_range` (empty/reversed ⇒ `[]`, never descending).
    uses_div: bool,
    uses_rem: bool,
    /// `__phorj_add` — `+` overloaded for string concat (`is_string` ⇒ `.`, else `+`).
    uses_add: bool,
    uses_str: bool,
    /// Set when an interpolation hole is statically a `float` and emits `__phorj_float` directly
    /// (T6) — so the shortest-round-trip float formatter is defined even when `__phorj_str` (its
    /// usual host) is never emitted because every other hole's kind was resolved natively.
    uses_float: bool,
    uses_range: bool,
    /// Set when `Reflect.kind(x)` is emitted — defines the `__phorj_kind` runtime helper once per
    /// file. A native's `php` closure can't set a `uses_*` flag (it has no `&mut self`), so
    /// `emit_member_call` special-cases this one native to set the flag before emitting (the
    /// established gated-helper pattern). The helper reproduces the coarse, erasure-stable type tag.
    uses_reflect_kind: bool,
    /// Set when `Reflect.className(x)` is emitted — defines the `__phorj_class_name` helper once per
    /// file (single-evaluates its argument; excludes closures). Same gated-helper rationale as
    /// `uses_reflect_kind`.
    uses_reflect_class_name: bool,
    /// True when the program carries mangled (`\`-bearing) names — a multi-package project (M5 S2c).
    /// Switches emission from the flat single-package form to one `namespace …{}` brace-block per
    /// package + a nameless bootstrap block, and forces fully-qualified (leading-`\`) call emission.
    namespaced: bool,
    /// The flattened `class_implements` oracle (M-RT overloading): used to order an overload set's
    /// PHP dispatch branches most-specific-first (subtypes before supertypes), so the emitted
    /// `if`-chain selects the same body the backends' `select_overload` does. Built once in `emit`.
    class_implements: std::collections::BTreeMap<String, Vec<String>>,
    /// Static class hierarchy for the reflection enumeration natives — emitted as the PHP
    /// `__phorj_reflect_of` static table when `uses_reflect_tables` is set, byte-identical to the
    /// `ClassTables` the Rust backends read (M-Reflect Tier-2).
    class_tables: crate::native::ClassTables,
    /// Set when a `Core.Reflect.interfaces`/`parents`/… call is emitted — defines the
    /// `__phorj_reflect_of($v, $kind)` helper + its static table once per file.
    uses_reflect_tables: bool,
    /// Set when `Core.Json.stringify` / `stringifyPretty` / `parse` is emitted — each defines its
    /// `__phorj_json_*` recursive helper once per file (the gated-helper pattern, set in
    /// `emit_member_call` because a native's `php` closure has no `&mut self`). The helpers walk the
    /// injected `Json` enum's PHP class hierarchy (mangled variant classes `Int_`/`Bool_`/…) so the
    /// PHP leg matches `run`/`runvm` byte-for-byte; floats route through `__phorj_float` (positional,
    /// not native json's scientific), so `uses_float` is implied by an encode.
    uses_json_encode: bool,
    uses_json_pretty: bool,
    uses_json_decode: bool,
    /// Set when `Core.Text.parseInt` is emitted — defines `__phorj_parse_int` once per file. The
    /// helper mirrors Rust's `i64::from_str` (optional sign, base-10 digits, i64 range, no surrounding
    /// whitespace) and returns `null` (Phorj `None`) otherwise — including on i64 overflow, which
    /// PHP's `(int)` cast would silently clamp.
    uses_text_parse_int: bool,
    /// Set when `Core.List.sort` / `sortWith` is emitted — defines the matching `__phorj_sort*`
    /// helper once per file. Both copy the list before `usort` (Phorj lists are immutable); `sort`
    /// uses a `<=>`/`strcmp` type-dispatched comparator (string by byte, NOT PHP's numeric-string
    /// `<=>`) to match Rust's natural order, `sortWith` defers to the user closure.
    uses_list_sort: bool,
    uses_list_sort_with: bool,
    /// Set when the matching `Core.List` breadth op is emitted — each defines a `__phorj_*` helper
    /// once per file (List breadth slice). They exist instead of inlining PHP `min`/`max`/`array_unique`
    /// because those juggle numeric strings, diverging from the Rust backends' byte-order; `find`/`any`/
    /// `all` short-circuit (`foreach` + early `return`) to match the Rust short-circuit on a
    /// side-effecting predicate.
    uses_list_unique: bool,
    uses_list_min: bool,
    uses_list_max: bool,
    uses_list_find: bool,
    uses_list_any: bool,
    uses_list_all: bool,
    /// Set when `Core.Map.set` / `remove` is emitted — defines the matching `__phorj_map_set` /
    /// `__phorj_map_remove` helper once per file. Both produce a NEW map (Phorj maps are immutable);
    /// PHP arrays are COW value types, so the helper's by-value `$m` is already a copy.
    uses_map_set: bool,
    uses_map_remove: bool,
    /// Set when `Core.List.indexOf` is emitted — defines `__phorj_index_of`, which maps PHP
    /// `array_search`'s `false`-on-miss to `null` (the `int?` return).
    uses_list_index_of: bool,
    /// Set when `Core.List.lastIndexOf` is emitted — defines `__phorj_last_index_of`, the LAST-match
    /// companion to `__phorj_index_of` (PHP `array_keys($xs, $needle, true)` → last key, or `null`).
    uses_list_last_index_of: bool,
    /// Set when `Core.Text.indexOf` is emitted — defines `__phorj_text_index_of`, mapping PHP
    /// `strpos`'s `false`-on-miss to `null` (the `int?` return).
    uses_text_index_of: bool,
    /// Set when `Core.Text.parseFloat` is emitted — defines `__phorj_parse_float`, which gates the
    /// float grammar (strict / permissive, rejecting inf/nan) then casts, mirroring the Rust kernel.
    uses_text_parse_float: bool,
    /// Set when a `decimal` `+`/`-`/`*` (or `Decimal.of`) is emitted — each defines its BCMath
    /// `__phorj_dec_*` helper once per file (M-NUM S1). The helpers derive operand scales at runtime,
    /// compute the result scale (add/sub = max, mul = sum), call `bcadd`/`bcsub`/`bcmul`, then
    /// bounds-check the result against i128 range and `throw` the same `decimal overflow` fault as the
    /// Rust kernels — so the PHP leg matches `run`/`runvm` byte-for-byte (incl. the overflow fault).
    uses_dec_add: bool,
    uses_dec_sub: bool,
    uses_dec_mul: bool,
    /// Set when bare `decimal % decimal` is emitted — defines `__phorj_dec_rem` (`bcmod` at
    /// `max(scales)`; a zero divisor throws, matching the Rust `decimal_rem` fault).
    uses_dec_rem: bool,
    /// Set when bare `decimal / decimal` is emitted — defines `__phorj_dec_div_exact` (bcdiv +
    /// exactness check + trailing-zero strip; non-terminating / zero divisor throws, matching the
    /// Rust `decimal_div_exact` fault boundary byte-for-byte).
    uses_dec_div_exact: bool,
    /// Set when `Decimal.of(s)` is emitted — defines `__phorj_dec_of`, validating the literal grammar
    /// (a tier-1 PCRE — NOT mbstring) + i128 range, returning the normalized decimal string or `null`.
    uses_dec_of: bool,
    /// Set when `Decimal.div`/`Decimal.round` are emitted (M-NUM S2) — define `__phorj_dec_div` /
    /// `__phorj_dec_round`, replicating the Rust `round_div` rounding kernel via BCMath
    /// (`bcdiv`/`bcmod`/`bccomp` truncate-toward-zero, dividend-signed remainder — verified identical
    /// to Rust i128 `/`/`%`), switching on the `RoundingMode` enum's PHP form, and reusing
    /// `__phorj_dec_check` for the i128 overflow fault. Both gate the shared `__phorj_round_div`.
    uses_dec_div: bool,
    uses_dec_round: bool,
    /// Set when `Convert.toInt(float)` is emitted (M-NUM S3) — defines `__phorj_float_to_int`,
    /// returning `null` on NaN/±∞/out-of-i64-range else the truncated int, with the edge-safe float
    /// bounds that agree with Rust `value::float_to_int` (avoids PHP's `(int)NAN == 0`).
    uses_float_to_int: bool,
    /// Set when `Convert.decimalToInt(decimal)` is emitted (M-NUM S3) — defines `__phorj_dec_to_int`,
    /// truncating the carrier string toward zero (split before the dot) and range-checking i64, else
    /// `null`. Mirrors Rust `value::decimal_to_int`.
    uses_dec_to_int: bool,
    /// Set when `Convert.floatToIntExact(float)` is emitted (M4 as-matrix `float as int`) — defines
    /// `__phorj_float_to_int_exact`: the integral-or-null kernel (`3.0→3`, `3.9→null`). Mirrors Rust
    /// `value::float_to_int_exact`.
    uses_float_to_int_exact: bool,
    /// Set when `Convert.decimalToIntExact(decimal)` is emitted (M4 as-matrix `decimal as int`) —
    /// defines `__phorj_dec_to_int_exact`: integral-or-null over the carrier string. Mirrors Rust
    /// `value::decimal_to_int_exact`.
    uses_dec_to_int_exact: bool,
    /// Set when `Math.gcd(int, int)` is emitted (M-NUM S4) — defines `__phorj_gcd` (Euclid over the
    /// magnitudes), since gmp is absent under `php -n`. Mirrors the Rust `math_gcd` native body.
    uses_math_gcd: bool,
    /// Set when `Math.lcm(int, int)` is emitted (M4) — defines `__phorj_lcm` (`x/gcd*y` over the
    /// magnitudes, inlining Euclid so it needs no `__phorj_gcd`). Mirrors the Rust `math_lcm` native.
    uses_math_lcm: bool,
    /// Set when `Math.numberFormat(float, int)` is emitted (M-NUM S4) — defines
    /// `__phorj_number_format`, assembling the grouped string byte-for-byte like `value::number_format`
    /// (so the PHP leg never relies on PHP's own `number_format` and its `-0`/locale quirks).
    uses_math_number_format: bool,
    /// Set when any `Core.Random` native is emitted (2026-06-27) — defines the `__phorj_rng_*`
    /// helpers: a process-global state plus a hand-rolled xorshift64 byte-identical to the Rust kernel
    /// (so a seeded sequence matches `run`/`runvm`). `>>` is masked for logical shift; `GOLDEN` is the
    /// signed-i64 reinterpretation of the unsigned constant.
    uses_rng: bool,
    /// Set when any `Core.Regex` native is emitted (Fork A, 2026-06-28) — defines the
    /// `__phorj_regex_*` helpers + the `__phorj_regex_delim` delimiter picker. The injected `Regex`
    /// class holds the bare pattern; each helper builds a collision-free `~…~u` PCRE form and calls the
    /// matching `preg_*`. Byte-identical to the `regex`-crate backends on the regular subset (the
    /// engine's no-backref/lookaround set ≡ what PCRE matches identically); `\d\w\s` Unicode-vs-ASCII
    /// is the one documented edge (KNOWN_ISSUES), so shipped examples keep ASCII subjects.
    uses_regex: bool,
    /// Set when any `Core.Time` native is emitted (M-TIME, 2026-06-28) — defines the `__phorj_now_*`
    /// helpers: a freezable process-global clock (`static $frozen`) hand-rolled to match the Rust kernel
    /// (`src/native/time.rs`). A *frozen* program is byte-identical on `run`/`runvm`/transpiled PHP; an
    /// unfrozen `nowMillis()` reads the wall clock on each backend and is documented non-gated.
    uses_clock: bool,
    /// Classes that must lower to the **interface + trait** decomposition (M-RT S6b): every transitive
    /// ancestor of a multi-parent (`extends A, B`) class. PHP has no multiple inheritance, so a
    /// multi-parent class `implements` its parents' interfaces and `use`s their traits; each ancestor
    /// therefore needs an `I<name>` interface + `T<name>` trait + a concrete `class <name>` form.
    /// Built once in `emit`. A class outside this set lowers as a plain class / single `extends`
    /// (byte-identical to pre-S6b output). The multi-parent classes themselves are emitted via
    /// `emit_multi_class` (a class that `implements`+`use`s), not listed here.
    decomposed: BTreeSet<String>,
    /// Monotonic counter for the hidden `$__phorj_d{N}` temporary that a let-destructuring spills its
    /// initializer into (Phase 1 slice 5). The name never collides with a user local (`$__phorj_` is
    /// not a writable Phorj identifier) and the value is immaterial to stdout, so any deterministic
    /// sequence is byte-identity-safe.
    tmp: usize,
}

/// A resolved method origin: `(declaring class, method name)` — mirrors `ast::class_method_origins`.
type Origin = (String, String);

/// Where a `match` expression's arm values flow: a `return` or an assignment to `$name`.
enum MatchTarget {
    Return,
    Assign(String),
}

/// The PHP namespace of a (possibly mangled) function name: the prefix before the last `\`
/// (`Acme\Util\compute` ⇒ `Acme\Util`), or `Main` for a bare name (the `main` package).
fn namespace_of(name: &str) -> String {
    match name.rfind('\\') {
        Some(i) => name[..i].to_string(),
        None => "Main".to_string(),
    }
}

/// The trailing segment of a mangled name (`Acme\Util\compute` ⇒ `compute`), used as the function's
/// declared name inside its `namespace` block. A bare name is returned unchanged.
fn last_segment(name: &str) -> &str {
    name.rsplit('\\').next().unwrap_or(name)
}

/// PHP reserves a fixed set of words as **class names** — `int`/`float`/`bool`/`null` etc. — and
/// rejects them as a class name *even inside a namespace* (verified vs PHP 8.5). An enum variant
/// transpiles to `final class <Variant> extends <Enum>`, so a variant named after one of these would
/// be a parse error. We mangle such a variant's PHP class name by appending `_` (`Int`→`Int_`). This
/// is **transpiler-only**: `run`/`runvm` address a variant by its Phorj name (`EnumVal.variant`),
/// never a PHP class name, so program stdout is unaffected. Comparison is case-insensitive (PHP class
/// names are). (`array`/`callable`/`list`/`enum` are NOT reserved as class names — left untouched.)
fn php_variant_name(variant: &str) -> String {
    // The trailing segment is the actual PHP class name (`\Ns\Int` ⇒ `Int`); mangle only that.
    let leaf = last_segment(variant);
    const RESERVED: &[&str] = &[
        "int", "float", "bool", "string", "true", "false", "null", "void", "iterable", "object",
        "mixed", "never", "self", "parent", "static",
    ];
    if RESERVED.contains(&leaf.to_ascii_lowercase().as_str()) {
        format!("{variant}_")
    } else {
        variant.to_string()
    }
}

/// Property names PHP's `\Exception` already declares (M-faults 2b). A Phorj `Error` subtype
/// transpiles to `extends \Exception`, so a promoted/declared field with one of these names would be
/// a typed redeclaration of an inherited untyped property — a PHP fatal — and must be emitted untyped.
fn exception_reserved(name: &str) -> bool {
    matches!(name, "message" | "code" | "file" | "line" | "previous")
}

/// Whether `ty` is the built-in marker `Error` (bare `Error` or optional `Error?`). Used by M-faults
/// 2c to recognize a conventional `cause` field whose value feeds PHP's native exception chain. A
/// type literally named `Error` in PHP would resolve to the unrelated *engine* `Error` class, so an
/// `Error`-typed cause must be emitted as `?\Throwable` (the type of `\Exception::$previous`), which
/// accepts every Phorj `Error` (each transpiles to `extends \Exception`).
fn is_error_marker_type(ty: &Type) -> bool {
    match ty {
        Type::Named { name, .. } => last_segment(name) == "Error",
        Type::Optional { inner, .. } => is_error_marker_type(inner),
        _ => false,
    }
}

/// A type *reference* in PHP: a mangled (`\`-bearing) cross-package name becomes an absolute FQN
/// (leading `\`, so it resolves regardless of the surrounding `namespace` block — uniform with
/// function de-mangling, no `use`); a bare same-/`Main`-namespace name stays bare (M-RT cross-package
/// types). Byte-identical to the pre-lift output for a single-package program (no `\` names).
fn php_type_ref(name: &str) -> String {
    if name.contains('\\') {
        let leaf = php_class_name(last_segment(name));
        let ns = &name[..name.len() - last_segment(name).len()];
        format!("\\{ns}{leaf}")
    } else {
        php_class_name(name)
    }
}

/// Mangle a PHP-reserved **class/enum** name that a Phorj type name would collide with. The only
/// case today is `RoundingMode`: PHP 8.4+ ships a built-in `enum RoundingMode`, so the injected M-NUM
/// S2 enum (`abstract class RoundingMode` + its variant subclasses) would otherwise fatal with
/// "cannot extend enum RoundingMode". Append `_` (`RoundingMode` → `RoundingMode_`), transpiler-only:
/// `run`/`runvm` address the enum by its Phorj name (`EnumVal.ty`), never a PHP class name, so
/// program stdout is unaffected. Distinct from [`php_variant_name`] (which mangles reserved *variant*
/// names like `Int`); the enum *type* name and its variant names mangle independently.
fn php_class_name(name: &str) -> String {
    if name.eq_ignore_ascii_case("RoundingMode") {
        format!("{name}_")
    } else {
        name.to_string()
    }
}

/// Whether a native's PHP erasure is a global function call (`strlen(...)`, `str_replace(...)`) — an
/// identifier immediately followed by `(`. Such calls need a leading `\` inside a namespace block so
/// they resolve to the global PHP builtin, not `CurrentNs\strlen`. A language construct like
/// `echo … . "\n"` (`console.println`) is not a function call and is left alone (M5-8).
fn looks_like_global_call(s: &str) -> bool {
    let mut chars = s.char_indices();
    match chars.next() {
        Some((_, c)) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    for (_, c) in chars {
        if c == '(' {
            return true;
        }
        if !(c.is_ascii_alphanumeric() || c == '_') {
            return false;
        }
    }
    false
}

// cohesion split (M-Decomp W4): program/types/stmt/expr/call/matches clusters.
mod call;
mod expr;
mod matches;
mod program;
mod stmt;
mod types;

impl Transpiler {
    fn new() -> Self {
        Transpiler {
            funcs: HashSet::new(),
            foreign_fns: HashSet::new(),
            foreign_classes: HashSet::new(),
            classes: HashSet::new(),
            consts: HashSet::new(),
            variants: HashSet::new(),
            variant_fields: HashMap::new(),
            variant_ns: HashMap::new(),
            out: String::new(),
            indent: 0,
            locals: Vec::new(),
            local_kinds: Vec::new(),
            cur_class: None,
            parent_aliases: None,
            class_field_kinds: HashMap::new(),
            class_parents: HashMap::new(),
            variant_field_kinds: HashMap::new(),
            fn_ret_kinds: HashMap::new(),
            method_ret_kinds: HashMap::new(),
            cur_class_fields: None,
            imports: HashMap::new(),
            uses_div: false,
            uses_rem: false,
            uses_add: false,
            uses_str: false,
            uses_float: false,
            uses_range: false,
            uses_reflect_kind: false,
            uses_reflect_class_name: false,
            namespaced: false,
            class_implements: std::collections::BTreeMap::new(),
            class_tables: crate::native::ClassTables::default(),
            uses_reflect_tables: false,
            uses_json_encode: false,
            uses_json_pretty: false,
            uses_json_decode: false,
            uses_text_parse_int: false,
            uses_list_sort: false,
            uses_list_sort_with: false,
            uses_list_unique: false,
            uses_list_min: false,
            uses_list_max: false,
            uses_list_find: false,
            uses_list_any: false,
            uses_list_all: false,
            uses_map_set: false,
            uses_map_remove: false,
            uses_list_index_of: false,
            uses_list_last_index_of: false,
            uses_text_index_of: false,
            uses_text_parse_float: false,
            uses_dec_add: false,
            uses_dec_rem: false,
            uses_dec_div_exact: false,
            uses_dec_sub: false,
            uses_dec_mul: false,
            uses_dec_of: false,
            uses_dec_div: false,
            uses_dec_round: false,
            uses_float_to_int: false,
            uses_dec_to_int: false,
            uses_float_to_int_exact: false,
            uses_dec_to_int_exact: false,
            uses_math_gcd: false,
            uses_math_lcm: false,
            uses_math_number_format: false,
            uses_rng: false,
            uses_regex: false,
            uses_clock: false,
            decomposed: BTreeSet::new(),
            tmp: 0,
        }
    }

    /// Indentation-aware line writer.
    fn line(&mut self, s: &str) {
        for _ in 0..self.indent {
            self.out.push_str("    ");
        }
        self.out.push_str(s);
        self.out.push('\n');
    }

    fn push_scope(&mut self) {
        self.locals.push(HashSet::new());
        self.local_kinds.push(HashMap::new());
    }
    fn pop_scope(&mut self) {
        self.locals.pop();
        self.local_kinds.pop();
    }
    fn declare(&mut self, name: &str) {
        if let Some(s) = self.locals.last_mut() {
            s.insert(name.to_string());
        }
    }
    fn is_local(&self, name: &str) -> bool {
        self.locals.iter().any(|s| s.contains(name))
    }
    /// Render a `catch` clause's type for PHP (M-faults 2b): a single class/interface via `php_type_ref`
    /// (FQN if cross-package), a union `A | B` as PHP 8's `A | B`. The built-in `Error` base maps to
    /// `\Exception` (a Phorj `Error` subtype transpiled to `extends \Exception`, and PHP's own `Error`
    /// is a *different* engine class — so `catch (Error e)` must catch `\Exception`, not PHP `\Error`).
    /// M8.5 S3a: a **foreign** exception class (`declare class … implements Error`) is caught by its own
    /// global PHP name (`\DivisionByZeroError`) — NOT the `Error`→`\Exception` mapping — so a foreign
    /// `\Error`-family class (a `\Throwable` that is not an `\Exception`) is caught correctly.
    fn php_catch_type(&self, ty: &Type) -> String {
        match ty {
            Type::Named { name, .. } if self.foreign_classes.contains(name) => {
                format!("\\{}", php_class_name(name))
            }
            Type::Named { name, .. } if last_segment(name) == "Error" => "\\Exception".to_string(),
            Type::Named { name, .. } => php_type_ref(name),
            Type::Union(members, _) => members
                .iter()
                .map(|m| self.php_catch_type(m))
                .collect::<Vec<_>>()
                .join(" | "),
            _ => "\\Exception".to_string(), // defensive — the checker requires an Error-typed catch
        }
    }
    /// Record a local/param/loop-var's scalar [`OpKind`] in the current scope (T6). Only called where
    /// the declared type is statically known; names without a kind resolve to `Other` (helper path).
    fn declare_kind(&mut self, name: &str, kind: OpKind) {
        if kind != OpKind::Other {
            if let Some(s) = self.local_kinds.last_mut() {
                s.insert(name.to_string(), kind);
            }
        }
    }
    /// Resolve a name's [`OpKind`] from the innermost scope outward; `Other` if unknown.
    fn local_kind(&self, name: &str) -> OpKind {
        self.local_kinds
            .iter()
            .rev()
            .find_map(|s| s.get(name).cloned())
            .unwrap_or(OpKind::Other)
    }

    /// The return [`OpKind`] of `class.method` — own method else walk `extends` parents (T6c).
    fn lookup_method_ret_kind(&self, class: &str, method: &str) -> OpKind {
        if let Some(k) = self
            .method_ret_kinds
            .get(&(class.to_string(), method.to_string()))
        {
            return k.clone();
        }
        if let Some(parents) = self.class_parents.get(class) {
            for p in parents {
                let k = self.lookup_method_ret_kind(p, method);
                if k != OpKind::Other {
                    return k;
                }
            }
        }
        OpKind::Other
    }

    /// The [`OpKind`] of `class.field` — the field's own kind, else walk `extends` parents (T6b).
    /// `Other` if the class/field is unknown (→ helper fallback).
    fn lookup_field_kind(&self, class: &str, field: &str) -> OpKind {
        if let Some(k) = self.class_field_kinds.get(class).and_then(|m| m.get(field)) {
            return k.clone();
        }
        if let Some(parents) = self.class_parents.get(class) {
            for p in parents {
                let k = self.lookup_field_kind(p, field);
                if k != OpKind::Other {
                    return k;
                }
            }
        }
        OpKind::Other
    }

    /// Statically resolve an expression's operand [`OpKind`] for native-operator selection (T6).
    /// Covers the scalar surface — literals, typed locals/params/loop-vars, nested arithmetic/unary,
    /// `instanceof` (bool), and `inner!` (the inner's kind). Field reads, indexing, method/function
    /// calls and `this` are deliberately `Other` (→ runtime helper), since pinning their types down
    /// would mean rebuilding the compiler's full type maps; the helper fallback keeps those correct.
    fn expr_kind(&self, e: &Expr) -> OpKind {
        match e {
            Expr::Int(..) => OpKind::Int,
            Expr::Float(..) => OpKind::Float,
            Expr::Decimal { .. } => OpKind::Decimal,
            Expr::Str(..) => OpKind::Str,
            Expr::Bool(..) => OpKind::Bool,
            Expr::Ident(name, _) => {
                // T6d: a bare class-name ident (only ever the object of a static/const access
                // `ClassName::FIELD`) resolves to that class, so the enclosing `Member` arm can look
                // up the const/static field's kind. A real local shadows (checked first).
                let k = self.local_kind(name);
                if k == OpKind::Other && self.classes.contains(name) {
                    OpKind::Class(name.clone())
                } else {
                    k
                }
            }
            Expr::Unary { op, expr, .. } => match op {
                UnaryOp::Neg => self.expr_kind(expr),
                UnaryOp::Not => OpKind::Bool,
                UnaryOp::BitNot => OpKind::Int,
            },
            Expr::Binary { op, lhs, rhs, .. } => match op {
                // Arithmetic: result kind follows the operands (the checker guarantees they agree).
                // `+` over strings is concatenation → `Str`; otherwise numeric (Float dominates Int).
                BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Rem => {
                    let (l, r) = (self.expr_kind(lhs), self.expr_kind(rhs));
                    if matches!(op, BinaryOp::Add) && (l == OpKind::Str || r == OpKind::Str) {
                        OpKind::Str
                    } else if l == OpKind::Decimal || r == OpKind::Decimal {
                        // `decimal ⊕ {decimal,int}` stays decimal (M-NUM S1); the PHP carrier is a
                        // string, but the operand *kind* is `Decimal` so a nested `(a * b) + c`
                        // routes every level through the `__phorj_dec_*` helpers.
                        OpKind::Decimal
                    } else if l == OpKind::Float || r == OpKind::Float {
                        OpKind::Float
                    } else if l == OpKind::Int || r == OpKind::Int {
                        OpKind::Int
                    } else {
                        OpKind::Other
                    }
                }
                // Comparisons / logical / bitwise-on-bool produce a bool.
                BinaryOp::Eq
                | BinaryOp::NotEq
                | BinaryOp::Lt
                | BinaryOp::Le
                | BinaryOp::Gt
                | BinaryOp::Ge
                | BinaryOp::And
                | BinaryOp::Or => OpKind::Bool,
                // Bitwise ops are int-only (primitives P2) → an int operand for any enclosing `+`.
                BinaryOp::BitAnd
                | BinaryOp::BitOr
                | BinaryOp::BitXor
                | BinaryOp::Shl
                | BinaryOp::Shr => OpKind::Int,
                _ => OpKind::Other,
            },
            Expr::InstanceOf { .. } => OpKind::Bool,
            Expr::Force { inner, .. } => self.expr_kind(inner),
            // T6d: `xs[i]` → element kind; `m[k]` → value kind.
            Expr::Index { object, .. } => match self.expr_kind(object) {
                OpKind::List(elem) => *elem,
                OpKind::Map(_, val) => *val,
                _ => OpKind::Other,
            },
            // A list/map literal carries its element kind from the first item, so `[1,2,3][0]`
            // resolves (M3 S1.1 analog).
            Expr::List(items, _) => OpKind::List(Box::new(
                items.first().map_or(OpKind::Other, |e| self.expr_kind(e)),
            )),
            Expr::Map(pairs, _) => OpKind::Map(
                Box::new(
                    pairs
                        .first()
                        .map_or(OpKind::Other, |(k, _)| self.expr_kind(k)),
                ),
                Box::new(
                    pairs
                        .first()
                        .map_or(OpKind::Other, |(_, v)| self.expr_kind(v)),
                ),
            ),
            // T6b: `this` is the enclosing class; a field read resolves through the class tables.
            Expr::This(_) => self
                .cur_class
                .as_ref()
                .map_or(OpKind::Other, |c| OpKind::Class(c.clone())),
            // A field read `obj.f` (instance or `this`): resolve `obj`'s class, then look up `f`.
            // A safe read `obj?.f` is `T?` (an optional) → not a scalar operand → `Other`.
            Expr::Member {
                object,
                name,
                safe: false,
                ..
            } => match self.expr_kind(object) {
                OpKind::Class(c) => self.lookup_field_kind(&c, name),
                _ => OpKind::Other,
            },
            // A call result (T6c): a constructor `ClassName(...)` (Phorj `new` is unwrapped to a
            // `Call`) yields an instance of that class (so `mk().x` resolves); a free-function call
            // resolves to its declared return kind; a method call `obj.m(...)` resolves to the
            // method's return kind on `obj`'s class (+ inherited).
            Expr::Call { callee, .. } => match &**callee {
                Expr::Ident(name, _) if self.classes.contains(name) => OpKind::Class(name.clone()),
                Expr::Ident(name, _) => self
                    .fn_ret_kinds
                    .get(name)
                    .cloned()
                    .unwrap_or(OpKind::Other),
                Expr::Member {
                    object,
                    name,
                    safe: false,
                    ..
                } => {
                    // T6d: a native call `Leaf.fn(...)` (Leaf an imported module qualifier, e.g.
                    // `Text.upper`) resolves to the native's declared return kind (mirrors the
                    // import-driven native resolution in `emit_call`).
                    if let Expr::Ident(leaf, _) = &**object {
                        if let Some(module) = self.imports.get(leaf) {
                            if let Some(idx) = crate::native::index_of(module, name) {
                                return opkind_of_ty(&crate::native::registry()[idx].ret);
                            }
                        }
                    }
                    // Otherwise a method call on a value — resolve its receiver's class.
                    match self.expr_kind(object) {
                        OpKind::Class(c) => self.lookup_method_ret_kind(&c, name),
                        _ => OpKind::Other,
                    }
                }
                _ => OpKind::Other,
            },
            _ => OpKind::Other,
        }
    }

    /// The kind of an `/` or `%` result for native-operator selection (T6): `Float` if either operand
    /// is float, `Int` if either is int, else `Other` (→ runtime helper). The checker guarantees both
    /// operands share a numeric type, so resolving either suffices.
    fn arith_kind(&self, lhs: &Expr, rhs: &Expr) -> OpKind {
        match (self.expr_kind(lhs), self.expr_kind(rhs)) {
            (OpKind::Float, _) | (_, OpKind::Float) => OpKind::Float,
            (OpKind::Int, _) | (_, OpKind::Int) => OpKind::Int,
            _ => OpKind::Other,
        }
    }
}

/// Escape a literal string chunk for embedding in a PHP double-quoted string.
/// `$` is escaped so PHP does not attempt its own interpolation on emitted literals.
/// The literal text of a fault intrinsic's string-literal message (M-faults 2a); empty if absent. The
/// checker guarantees the argument is a single `StrPart::Literal`.
fn lit_arg(e: Option<&Expr>) -> String {
    if let Some(Expr::Str(parts, _)) = e {
        if let [StrPart::Literal(s)] = &parts[..] {
            return s.clone();
        }
    }
    String::new()
}

fn php_escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('$', "\\$")
}

/// Escape a literal segment for emission *inside an interpolating* PHP double-quoted string (B-9).
/// Like [`php_escape`] for `\` and `"`, but escapes `$` **only where PHP would actually interpolate**
/// — i.e. when the next char is an identifier start (`[A-Za-z_]`), a `{`/`$` (the `${…}`/`$$`
/// complex-var forms), or the segment end (conservative: the following segment may begin with one of
/// those, incl. an emitted `{$…}` hole). `$5`, `$ `, a trailing-symbol `$` etc. stay bare — cleaner
/// PHP with identical output. Used only by [`emit_string`](expr); the other `php_escape` call sites
/// emit standalone/quoted contexts and keep the unconditional form.
fn php_escape_interp(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '$' => {
                let interpolates = match chars.peek() {
                    Some(n) => n.is_ascii_alphabetic() || *n == '_' || *n == '{' || *n == '$',
                    None => true, // trailing `$`: next segment might start a var / `{$…}` hole
                };
                out.push_str(if interpolates { "\\$" } else { "$" });
            }
            _ => out.push(c),
        }
    }
    out
}

/// Escape a `bytes` literal for a PHP double-quoted string. Printable ASCII is emitted verbatim (with
/// `\` `"` `$` escaped); every other octet becomes a two-digit `\xHH` (always two digits so PHP's
/// greedy `\x` escape can't merge with a following hex character). PHP strings are byte arrays, so the
/// round-trip is exact (M6 W0).
fn php_escape_bytes(bytes: &[u8]) -> String {
    let mut out = String::new();
    for &b in bytes {
        match b {
            b'\\' => out.push_str("\\\\"),
            b'"' => out.push_str("\\\""),
            b'$' => out.push_str("\\$"),
            0x20..=0x7E => out.push(b as char),
            _ => out.push_str(&format!("\\x{b:02x}")),
        }
    }
    out
}

/// A ctor param is promoted (becomes a field) iff it carries a visibility modifier —
/// matches the evaluator (EV-4) and the checker's `collect_class`.
fn is_promoted(mods: &[Modifier]) -> bool {
    mods.iter().any(|m| {
        matches!(
            m,
            Modifier::Public | Modifier::Private | Modifier::Protected
        )
    })
}

/// PHP visibility keyword for a member's modifiers (empty string = no keyword).
fn vis(mods: &[Modifier]) -> &'static str {
    if mods.iter().any(|m| matches!(m, Modifier::Private)) {
        "private"
    } else if mods.iter().any(|m| matches!(m, Modifier::Protected)) {
        "protected"
    } else if mods.iter().any(|m| matches!(m, Modifier::Public)) {
        "public"
    } else {
        ""
    }
}

#[cfg(test)]
mod tests;
