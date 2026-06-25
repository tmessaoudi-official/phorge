//! Phorge → PHP transpiler. Walks the untyped AST (the same AST the evaluator walks)
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
/// semantics diverge from Phorge's (`+` concat-vs-add, `/` int-vs-float, interpolation display).
/// Anything the resolver cannot pin down is [`OpKind::Other`], which routes to the existing helper
/// (the safe fallback), so a wrong guess can never happen — only "known" or "fall back".
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum OpKind {
    Str,
    Int,
    Float,
    Bool,
    Other,
}

/// Map a (post-checker, resolved) type annotation to its scalar [`OpKind`]. Non-scalars (`List`,
/// `Map`, classes, `void`, optionals, …) are `Other` — their values aren't the arithmetic/display
/// operands T6 specializes, and the helper fallback covers any that slip through.
fn kind_of_type(ty: &Type) -> OpKind {
    match ty {
        Type::Named { name, .. } => match name.as_str() {
            "int" => OpKind::Int,
            "float" => OpKind::Float,
            "string" => OpKind::Str,
            "bool" => OpKind::Bool,
            _ => OpKind::Other,
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
    /// instead of the `__phorge_add`/`_div`/`_rem`/`_str` runtime helpers. A name absent here resolves
    /// to [`OpKind::Other`] → the helper is emitted as a safe fallback (never a byte-identity risk).
    local_kinds: Vec<HashMap<String, OpKind>>,
    cur_class_fields: Option<HashSet<String>>,
    /// Active import map (leaf qualifier → full dotted module path) — how a namespaced native call
    /// `console.println(x)` is distinguished from a method call on a value (M3 Wave 1). The
    /// transpiler tracks no variable scope, so unlike the interpreter/compiler it cannot use a
    /// locals-first heuristic; the import map is the authority.
    imports: HashMap<String, String>,
    /// Set when `/`, `%`, an interpolation, or a range is emitted — each defines a once-per-file
    /// runtime helper (M7) that reproduces Phorge's type-driven semantics under PHP's looser rules:
    /// `__phorge_div` (int `/` ⇒ `intdiv`), `__phorge_rem` (float `%` ⇒ `fmod`), `__phorge_str`
    /// (bool ⇒ `"true"/"false"`), `__phorge_range` (empty/reversed ⇒ `[]`, never descending).
    uses_div: bool,
    uses_rem: bool,
    /// `__phorge_add` — `+` overloaded for string concat (`is_string` ⇒ `.`, else `+`).
    uses_add: bool,
    uses_str: bool,
    /// Set when an interpolation hole is statically a `float` and emits `__phorge_float` directly
    /// (T6) — so the shortest-round-trip float formatter is defined even when `__phorge_str` (its
    /// usual host) is never emitted because every other hole's kind was resolved natively.
    uses_float: bool,
    uses_range: bool,
    /// Set when `Reflect.kind(x)` is emitted — defines the `__phorge_kind` runtime helper once per
    /// file. A native's `php` closure can't set a `uses_*` flag (it has no `&mut self`), so
    /// `emit_member_call` special-cases this one native to set the flag before emitting (the
    /// established gated-helper pattern). The helper reproduces the coarse, erasure-stable type tag.
    uses_reflect_kind: bool,
    /// Set when `Reflect.className(x)` is emitted — defines the `__phorge_class_name` helper once per
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
    /// `__phorge_reflect_of` static table when `uses_reflect_tables` is set, byte-identical to the
    /// `ClassTables` the Rust backends read (M-Reflect Tier-2).
    class_tables: crate::native::ClassTables,
    /// Set when a `Core.Reflect.interfaces`/`parents`/… call is emitted — defines the
    /// `__phorge_reflect_of($v, $kind)` helper + its static table once per file.
    uses_reflect_tables: bool,
    /// Classes that must lower to the **interface + trait** decomposition (M-RT S6b): every transitive
    /// ancestor of a multi-parent (`extends A, B`) class. PHP has no multiple inheritance, so a
    /// multi-parent class `implements` its parents' interfaces and `use`s their traits; each ancestor
    /// therefore needs an `I<name>` interface + `T<name>` trait + a concrete `class <name>` form.
    /// Built once in `emit`. A class outside this set lowers as a plain class / single `extends`
    /// (byte-identical to pre-S6b output). The multi-parent classes themselves are emitted via
    /// `emit_multi_class` (a class that `implements`+`use`s), not listed here.
    decomposed: BTreeSet<String>,
    /// Monotonic counter for the hidden `$__phorge_d{N}` temporary that a let-destructuring spills its
    /// initializer into (Phase 1 slice 5). The name never collides with a user local (`$__phorge_` is
    /// not a writable Phorge identifier) and the value is immaterial to stdout, so any deterministic
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

/// Property names PHP's `\Exception` already declares (M-faults 2b). A Phorge `Error` subtype
/// transpiles to `extends \Exception`, so a promoted/declared field with one of these names would be
/// a typed redeclaration of an inherited untyped property — a PHP fatal — and must be emitted untyped.
fn exception_reserved(name: &str) -> bool {
    matches!(name, "message" | "code" | "file" | "line" | "previous")
}

/// Whether `ty` is the built-in marker `Error` (bare `Error` or optional `Error?`). Used by M-faults
/// 2c to recognize a conventional `cause` field whose value feeds PHP's native exception chain. A
/// type literally named `Error` in PHP would resolve to the unrelated *engine* `Error` class, so an
/// `Error`-typed cause must be emitted as `?\Throwable` (the type of `\Exception::$previous`), which
/// accepts every Phorge `Error` (each transpiles to `extends \Exception`).
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
        format!("\\{name}")
    } else {
        name.to_string()
    }
}

/// Render a `catch` clause's type for PHP (M-faults 2b): a single class/interface via `php_type_ref`
/// (FQN if cross-package), a union `A | B` as PHP 8's `A | B`. The built-in `Error` base maps to
/// `\Exception` (a Phorge `Error` subtype transpiled to `extends \Exception`, and PHP's own `Error`
/// is a *different* engine class — so `catch (Error e)` must catch `\Exception`, not PHP `\Error`).
fn php_catch_type(ty: &Type) -> String {
    match ty {
        Type::Named { name, .. } if last_segment(name) == "Error" => "\\Exception".to_string(),
        Type::Named { name, .. } => php_type_ref(name),
        Type::Union(members, _) => members
            .iter()
            .map(php_catch_type)
            .collect::<Vec<_>>()
            .join(" | "),
        _ => "\\Exception".to_string(), // defensive — the checker requires an Error-typed catch
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
            classes: HashSet::new(),
            consts: HashSet::new(),
            variants: HashSet::new(),
            variant_fields: HashMap::new(),
            variant_ns: HashMap::new(),
            out: String::new(),
            indent: 0,
            locals: Vec::new(),
            local_kinds: Vec::new(),
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
    /// Record a local/param/loop-var's scalar [`OpKind`] in the current scope (T6). Only called where
    /// the declared type is statically known; names without a kind resolve to `Other` (helper path).
    fn declare_kind(&mut self, name: &str, kind: OpKind) {
        if kind != OpKind::Other {
            if let Some(s) = self.local_kinds.last_mut() {
                s.insert(name.to_string(), kind);
            }
        }
    }
    /// Resolve a name's scalar [`OpKind`] from the innermost scope outward; `Other` if unknown.
    fn local_kind(&self, name: &str) -> OpKind {
        self.local_kinds
            .iter()
            .rev()
            .find_map(|s| s.get(name).copied())
            .unwrap_or(OpKind::Other)
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
            Expr::Str(..) => OpKind::Str,
            Expr::Bool(..) => OpKind::Bool,
            Expr::Ident(name, _) => self.local_kind(name),
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
