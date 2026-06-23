//! Abstract syntax tree: the parser's output and the shared input to every backend (checker,
//! tree-walking interpreter, bytecode compiler, PHP transpiler). Nodes are **untyped** — the
//! checker validates without annotating, so each backend re-derives the types it needs (see
//! `compiler::CTy`). `token::Span` is carried on nodes for diagnostics.

use crate::token::Span;

/// Type annotations (e.g. `int`, `List<Shape>`, `T?`).
#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    /// `int`, `List<Shape>`, `Map<string, int>` — `args` empty for non-generic.
    Named {
        name: String,
        args: Vec<Type>,
        span: Span,
    },
    /// `T?`
    Optional { inner: Box<Type>, span: Span },
    /// `A | B | C` — a union type (M-RT S4): a value that is *one of* several nominal/primitive types,
    /// the open-composition counterpart to a closed `enum`. Members are in source order here; the
    /// checker normalizes (flatten/dedupe/canonical-sort) into `Ty::Union`. Members are restricted to
    /// classes, interfaces, and primitives (`E-UNION-MEMBER`); transpiles to PHP 8.0 `A|B`.
    Union(Vec<Type>, Span),
    /// `A & B & C` — an intersection type (M-RT S5): a value that satisfies *all* members
    /// simultaneously, the narrowing dual of a union. Members are in source order here; the checker
    /// normalizes (flatten/dedupe/canonical-sort) into `Ty::Intersection`. Members are restricted to
    /// interfaces, plus at most one concrete class (`E-INTERSECT-MEMBER`/`E-INTERSECT-MULTI-CLASS`) —
    /// a value has exactly one class, so two distinct classes are uninhabited. Transpiles to PHP 8.1
    /// `A&B`. `&` binds tighter than `|` in `parse_type`.
    Intersection(Vec<Type>, Span),
    /// `var` — placeholder for an inferred local binding type (resolved by the checker from the
    /// initializer, erased everywhere else). Only valid as a `Stmt::VarDecl` type.
    Infer(Span),
    /// `(int, string) -> bool` — a first-class function type (M3 S3).
    Function {
        params: Vec<Type>,
        ret: Box<Type>,
        span: Span,
    },
    /// An **erased** generic type parameter (M-RT S7). Produced *only* by `checker::erase_generics`,
    /// which rewrites every `Type::Named` that refers to an in-scope type parameter (`T`) into this
    /// after type-checking. No parser ever emits it and no checker pass before erasure sees it; the
    /// backends consume it as the erasure target — the compiler treats it as `CTy::Other`, the
    /// transpiler emits PHP `mixed`. This is the same "compile-time-only, expanded out before any
    /// backend" discipline as `type` aliases (which become their target) and `html"…"`.
    Erased(Span),
}

/// Patterns in `match` arms.
#[derive(Debug, Clone, PartialEq)]
pub enum Pattern {
    /// `_`
    Wildcard(Span),
    /// bare identifier — binds the scrutinee (catch-all)
    Binding {
        name: String,
        span: Span,
    },
    Int(i64, Span),
    Float(f64, Span),
    Str(String, Span),
    Bool(bool, Span),
    Null(Span),
    /// `Circle(r)`, `Rect(w, h)` — destructure an enum variant
    Variant {
        name: String,
        fields: Vec<Pattern>,
        span: Span,
    },
    /// `Circle c` / `Square _` — a **type pattern** for match-over-union (M-RT S4): matches when the
    /// scrutinee is an instance of `type_name` (a class or interface — the same runtime test as
    /// `instanceof`, reusing `Op::IsInstance`), binding it (narrowed to `type_name`) as `binding` for
    /// the arm body. `binding` is `None` for `Type _`. Parsed as two identifiers in pattern position
    /// (`PascalCaseHead lowercaseBinder`); a lone `Circle =>` stays a catch-all `Binding`.
    Type {
        type_name: String,
        binding: Option<String>,
        span: Span,
    },
}

/// One segment of a (possibly interpolated) string literal.
#[derive(Debug, Clone, PartialEq)]
pub enum StrPart {
    Literal(String),
    Expr(Box<Expr>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub body: Expr,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Not,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Eq,
    NotEq,
    Lt,
    Gt,
    Le,
    Ge,
    And,
    Or,
    Pipe,
    /// `??` null-coalesce (M3 S2).
    Coalesce,
}

/// Expressions.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Int(i64, Span),
    Float(f64, Span),
    Bool(bool, Span),
    Null(Span),
    /// String literal as interpolation parts; a plain string is a single `Literal` part.
    Str(Vec<StrPart>, Span),
    /// `b"…"` raw byte-string literal — a flat octet sequence, no interpolation.
    Bytes(Vec<u8>, Span),
    Ident(String, Span),
    This(Span),
    /// `[a, b, c]`
    List(Vec<Expr>, Span),
    /// `[k => v, k2 => v2]` — a map literal (M-RT S3). Distinguished from `List` by the `=>` after the
    /// first element; at least one pair (an empty map literal is deferred — `[]` is the empty *list*).
    /// Keys must be `int`/`bool`/`string` (`E-MAP-KEY`); transpiles to a PHP `[k => v]` array.
    Map(Vec<(Expr, Expr)>, Span),
    Unary {
        op: UnaryOp,
        expr: Box<Expr>,
        span: Span,
    },
    Binary {
        op: BinaryOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
        span: Span,
    },
    /// `value instanceof TypeName` — a runtime type test (M-RT S1). The right operand is a class
    /// *type name* parsed as a type (not an expression), so this is a dedicated variant rather than a
    /// `BinaryOp`. It evaluates to `bool`; in `if (x instanceof C) { … }` the checker smart-casts `x`
    /// to `C` inside the then-block. Transpiles to PHP `$value instanceof TypeName`. (Replaces the
    /// retired value-equality `is` stub.)
    InstanceOf {
        value: Box<Expr>,
        type_name: String,
        span: Span,
    },
    /// `callee(args)` — also covers `Circle(2.0)` constructor calls
    Call {
        callee: Box<Expr>,
        args: Vec<Expr>,
        span: Span,
    },
    /// `object.name` (`safe == false`) or `object?.name` (`safe == true`, nullsafe access:
    /// a `null` receiver short-circuits the whole access to `null` instead of faulting). A
    /// safe *method* call is a `Call` whose `callee` is a `Member { safe: true, .. }` (M3 S2).
    Member {
        object: Box<Expr>,
        name: String,
        safe: bool,
        span: Span,
    },
    /// `object[index]`
    Index {
        object: Box<Expr>,
        index: Box<Expr>,
        span: Span,
    },
    /// `inner!` — checked force-unwrap of an optional `T?` to `T` (M3 S2.5). The checker requires
    /// `inner: T?` and lints every use (`W-FORCE-UNWRAP`); at runtime a `null` inner is a clean,
    /// byte-identical fault on both backends rather than a crash.
    Force {
        inner: Box<Expr>,
        span: Span,
    },
    /// `inner?` — error propagation (M-faults Slice 2a). On a `Result<T, E>` operand it unwraps an
    /// `Ok(v)` to `v`, or early-`return`s the `Err(e)` from the enclosing function (which the checker
    /// requires to return `Result<_, E'>` with `E <: E'`). Lowers on both backends to the existing
    /// variant-tag test + `return` (no new `Op`); the `throws`-call mode is added in Slice 2b. Note the
    /// lexer munches `??`/`?.` into their own tokens, so a lone `Question` in postfix position is
    /// unambiguously this operator.
    Propagate {
        inner: Box<Expr>,
        span: Span,
    },
    Match {
        scrutinee: Box<Expr>,
        arms: Vec<MatchArm>,
        span: Span,
    },
    /// `start..end` (exclusive) or `start..=end` (inclusive) — an integer range, materialized to a
    /// `List<int>` by both backends (decision S1-R). Its only role this slice is `for … in`.
    Range {
        start: Box<Expr>,
        end: Box<Expr>,
        inclusive: bool,
        span: Span,
    },
    /// `if (cond) { then } else { else }` in **expression** position: both arms are single
    /// expressions and `else` is mandatory (the value flows out). Distinct from the statement
    /// `Stmt::If`; the parser picks expr-vs-stmt by position (M3 S1.3).
    If {
        cond: Box<Expr>,
        then_expr: Box<Expr>,
        else_expr: Box<Expr>,
        span: Span,
    },
    /// `fn(Type param, …) [-> RetType] => expr` — an expression-body lambda (M3 S3, Task 3).
    /// Block-body lambdas (`fn(…) { … }`) are Task 6.
    Lambda {
        params: Vec<Param>,
        ret: Option<Type>,
        body: LambdaBody,
        span: Span,
    },
    /// `obj with { field = expr, … }` — a functional update (M-mut.4a, Fork 2 = B): a fresh instance
    /// copying `object`'s fields with the named ones overridden, **bypassing the constructor**.
    /// `object` must be a concrete class; `fields` names a subset of its (promoted) fields. Lowers to
    /// the existing `Op::MakeInstance` (no new `Op`); transpiles to PHP `clone($obj, ['f' => …])`.
    CloneWith {
        object: Box<Expr>,
        fields: Vec<(String, Expr)>,
        span: Span,
    },
    /// `html"<h1>{name}</h1>"` — a typed HTML literal (core.html Wave 3). The parser captures it as
    /// interpolation `parts` (literal chunks + `{expr}` holes, exactly like [`Expr::Str`]); the
    /// **checker** resolves each hole by type (an `Html` hole embeds as-is, a `string`/primitive hole
    /// is auto-escaped via `html.text`, anything else is `E-HTML-HOLE`) and rewrites the whole node
    /// into `html.concat([html.raw(chunk), …])` kernel calls, so no backend ever sees this variant —
    /// it is erased to ordinary native calls before the interpreter/compiler/transpiler run, the same
    /// "compile-time sugar, expanded out" treatment as `type` aliases.
    Html(Vec<StrPart>, Span),
}

/// The body of a lambda: either a single expression (`=> expr`) or a block of statements
/// (`{ stmts… }`). Only `Expr` is constructed in Task 3; `Block` is added in Task 6.
#[derive(Debug, Clone, PartialEq)]
pub enum LambdaBody {
    Expr(Box<Expr>),
    Block(Vec<Stmt>),
}

/// Compute the **sorted** free variables of a lambda: identifiers referenced in `body`
/// that are NOT the lambda's own params, NOT locals bound inside the body (`var`,
/// `if (var …)`, `for (T x in …)`, match-arm bindings, nested-lambda params), and NOT
/// `this`.
///
/// The result is sorted (invariant #8: deterministic capture order for all backends).
///
/// **Note:** over-reporting is acceptable — a global function name may appear in the
/// result if it is also used as an identifier reference. Call-site consumers (the
/// interpreter, compiler) filter it out by checking whether the name resolves to a
/// function or a local. Under-reporting (missing a real capture) is a correctness bug.
pub fn free_vars(params: &[Param], body: &LambdaBody) -> Vec<String> {
    let mut bound: std::collections::HashSet<String> =
        params.iter().map(|p| p.name.clone()).collect();
    let mut found: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    match body {
        LambdaBody::Expr(e) => collect_free_expr(e, &mut bound, &mut found),
        LambdaBody::Block(stmts) => collect_free_block(stmts, &mut bound, &mut found),
    }
    found.into_iter().collect()
}

/// The transitively-flattened interface set each concrete class implements, keyed by class name.
///
/// `class Dog implements Speaker` where `interface Speaker extends Named` ⇒ `Dog → [Named, Speaker]`
/// (every interface in the `implements` set *and* the `extends` closure of each). This is the single
/// runtime table behind `instanceof` against an interface: `x instanceof I` is true iff `I` is in
/// `class_implements[class_of(x)]`. It is computed **once** by this shared function and consumed
/// identically by the checker (subtyping + conformance), the interpreter, and the compiler/VM — one
/// algorithm, so the three backends can never diverge (the same discipline as [`free_vars`]).
///
/// The per-class list is **sorted** (invariant #8: deterministic order for all backends) and the
/// `extends` walk is **cycle-safe** via a visited set, so a malformed cyclic interface graph (which
/// the checker rejects as `E-IFACE-CYCLE` before any backend runs) can never make this loop forever.
/// Names are whatever the (already loader-mangled, if multi-package) AST carries — consistent across
/// every consumer.
pub fn class_implements(program: &Program) -> std::collections::BTreeMap<String, Vec<String>> {
    use std::collections::{BTreeMap, BTreeSet};
    // Direct `extends` edges for interfaces and for classes (M-RT S6), plus each class's own
    // `implements` list. A class inherits the interfaces of all its ancestor classes.
    let mut iface_extends: BTreeMap<&str, &[String]> = BTreeMap::new();
    let mut class_extends: BTreeMap<&str, &[String]> = BTreeMap::new();
    let mut own_implements: BTreeMap<&str, &[String]> = BTreeMap::new();
    for item in &program.items {
        match item {
            Item::Interface(i) => {
                iface_extends.insert(i.name.as_str(), &i.extends);
            }
            Item::Class(c) => {
                class_extends.insert(c.name.as_str(), &c.extends);
                own_implements.insert(c.name.as_str(), &c.implements);
            }
            _ => {}
        }
    }
    // Transitive closure of a name's `extends` chain (the name itself included), visited-guarded
    // against cycles. Used for both the interface graph and the class graph.
    fn closure<'a>(
        name: &'a str,
        edges: &BTreeMap<&'a str, &'a [String]>,
        acc: &mut BTreeSet<String>,
    ) {
        if !acc.insert(name.to_string()) {
            return; // already visited — also breaks any cycle
        }
        if let Some(parents) = edges.get(name) {
            for p in parents.iter() {
                closure(p, edges, acc);
            }
        }
    }
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for item in &program.items {
        if let Item::Class(c) = item {
            // The class itself plus every ancestor class (so inherited interfaces flow down, M-RT S6).
            let mut family: BTreeSet<String> = BTreeSet::new();
            closure(c.name.as_str(), &class_extends, &mut family);
            let mut ifaces: BTreeSet<String> = BTreeSet::new();
            for cls in &family {
                if let Some(impls) = own_implements.get(cls.as_str()) {
                    for i in impls.iter() {
                        closure(i, &iface_extends, &mut ifaces);
                    }
                }
            }
            out.insert(c.name.clone(), ifaces.into_iter().collect());
        }
    }
    out
}

/// Transitive parent-class closure for every class: `class_supertypes[c]` is the sorted set of all
/// ancestor class names reachable through `extends` — **not** including `c` itself, except when `c`
/// is part of an `extends` cycle (then `c` appears in its own set, which the checker uses to report
/// `E-MI-CYCLE`). Mirrors [`class_implements`]; the `extends` walk is cycle-safe via a visited set.
/// Consumed by the checker's nominal-subtype oracle (so `Dog <: Animal`) and (S6b+) the backends for
/// `instanceof` against a parent class — one algorithm, so the three backends can never diverge.
pub fn class_supertypes(program: &Program) -> std::collections::BTreeMap<String, Vec<String>> {
    use std::collections::{BTreeMap, BTreeSet};
    let mut class_extends: BTreeMap<&str, &[String]> = BTreeMap::new();
    for item in &program.items {
        if let Item::Class(c) = item {
            class_extends.insert(c.name.as_str(), &c.extends);
        }
    }
    // Accumulate the ancestors of `name` (parents, grandparents, …) — `name` itself is added only if
    // a cycle leads back to it.
    fn ancestors<'a>(
        name: &'a str,
        edges: &BTreeMap<&'a str, &'a [String]>,
        acc: &mut BTreeSet<String>,
    ) {
        if let Some(parents) = edges.get(name) {
            for p in parents.iter() {
                if acc.insert(p.clone()) {
                    ancestors(p, edges, acc);
                }
            }
        }
    }
    let mut out: std::collections::BTreeMap<String, Vec<String>> = BTreeMap::new();
    for item in &program.items {
        if let Item::Class(c) = item {
            let mut anc: BTreeSet<String> = BTreeSet::new();
            ancestors(c.name.as_str(), &class_extends, &mut anc);
            out.insert(c.name.clone(), anc.into_iter().collect());
        }
    }
    out
}

/// The **runtime subtype oracle** (M-RT S6c.3): for each class, every type name it is an instance of —
/// its transitive parent classes ([`class_supertypes`]) **and** its transitive interfaces
/// ([`class_implements`]). This is the single source consumed by `instanceof`, match type-patterns, and
/// overload subtyping on **both** backends, so a `Dog instanceof Animal` / `Duck instanceof Swimmer`
/// (a *class* ancestor, not just an interface) is true and can never diverge between `run` and `runvm`.
/// (The checker keeps a separate interfaces-only `class_implements` for interface *conformance*; its
/// `is_subtype` already consults `class_supertypes` independently.)
pub fn instanceof_table(program: &Program) -> std::collections::BTreeMap<String, Vec<String>> {
    let mut table = class_implements(program);
    for (cls, sups) in class_supertypes(program) {
        let entry = table.entry(cls).or_default();
        for s in sups {
            if !entry.contains(&s) {
                entry.push(s);
            }
        }
    }
    for v in table.values_mut() {
        v.sort();
        v.dedup();
    }
    table
}

/// Method-resolution order for every class: `class_mro[c]` is `c`'s ancestor classes in
/// **nearest-first breadth-first** order (direct parents in `extends` order, then their parents, …),
/// excluding `c` itself. Cycle-safe via a visited set. This is the **single source of dispatch
/// precedence** consumed by both the interpreter's `call_method` parent walk and the compiler's
/// method-table pre-flatten (M-RT S6b), so the two backends can never disagree on *which* ancestor a
/// method is inherited from. A method is resolved by scanning `[c] ++ class_mro[c]` and taking the
/// first class that declares it (so a nearer declaration overrides a farther one); a diamond shared
/// base is visited once, auto-merging when both arms reach the same declaring method.
pub fn class_mro(program: &Program) -> std::collections::BTreeMap<String, Vec<String>> {
    use std::collections::{BTreeMap, HashSet};
    let parents: BTreeMap<&str, &[String]> = program
        .items
        .iter()
        .filter_map(|it| match it {
            Item::Class(c) => Some((c.name.as_str(), c.extends.as_slice())),
            _ => None,
        })
        .collect();
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for item in &program.items {
        if let Item::Class(c) = item {
            let mut order = Vec::new();
            let mut seen: HashSet<String> = HashSet::new();
            let mut queue: Vec<String> = c.extends.clone();
            let mut i = 0;
            while i < queue.len() {
                let p = queue[i].clone();
                i += 1;
                if !seen.insert(p.clone()) {
                    continue;
                }
                order.push(p.clone());
                if let Some(gps) = parents.get(p.as_str()) {
                    queue.extend(gps.iter().cloned());
                }
            }
            out.insert(c.name.clone(), order);
        }
    }
    out
}

/// The fully-resolved method-dispatch table for every class (M-RT S6b): for each `(class, name)` it
/// gives the `(declaring_class, declaring_method)` a call of `name` on an instance of `class` runs.
/// This is the **single source of dispatch** for *both* backends — the interpreter looks up the
/// origin and the compiler aliases the bytecode method-table entry to it — so multi-parent dispatch
/// (including resolution clauses and renamed aliases) can never diverge between `run` and `runvm`.
///
/// Composition: a class's own methods map to itself (override); each direct parent contributes its
/// own resolved table; a diamond shared base auto-merges (both arms reach the *same* declaring
/// method). Resolution clauses (`use`/`rename`/`exclude`) are applied before finalizing. The second
/// return value lists every **unresolved** cross-parent collision as `(class, name, class_span)` —
/// the checker reports each as `E-MI-CONFLICT`. On a conflict the table still records a deterministic
/// pick so a backend never panics (the checker fails the build first).
#[allow(clippy::type_complexity)]
pub fn class_method_origins(
    program: &Program,
) -> (
    std::collections::BTreeMap<(String, String), (String, String)>,
    Vec<(String, String, Span)>,
) {
    use std::collections::{BTreeMap, BTreeSet};

    struct Ctx {
        decl: BTreeMap<String, BTreeSet<String>>,
        extends: BTreeMap<String, Vec<String>>,
        resolutions: BTreeMap<String, Vec<Resolution>>,
        spans: BTreeMap<String, Span>,
        memo: BTreeMap<String, BTreeMap<String, (String, String)>>,
        conflicts: Vec<(String, String, Span)>,
        in_progress: BTreeSet<String>,
    }

    impl Ctx {
        fn resolve(&mut self, c: &str) -> BTreeMap<String, (String, String)> {
            if let Some(m) = self.memo.get(c) {
                return m.clone();
            }
            if !self.in_progress.insert(c.to_string()) {
                // `extends` cycle — reported as `E-MI-CYCLE` elsewhere; break to avoid infinite loop.
                return BTreeMap::new();
            }
            let mut map: BTreeMap<String, (String, String)> = BTreeMap::new();
            // Own methods win over anything inherited (override).
            if let Some(ms) = self.decl.get(c).cloned() {
                for m in ms {
                    map.insert(m.clone(), (c.to_string(), m));
                }
            }
            // Gather each direct parent's resolved contributions, tracking the direct parent the
            // method arrives through (so a `use/rename/exclude P.m` clause can target it) and the true
            // origin (so a diamond dedups by origin).
            let mut contrib: BTreeMap<String, Vec<(String, (String, String))>> = BTreeMap::new();
            for p in self.extends.get(c).cloned().unwrap_or_default() {
                let p_map = self.resolve(&p);
                for (name, origin) in p_map {
                    if map.contains_key(&name) {
                        continue; // overridden by C itself
                    }
                    contrib.entry(name).or_default().push((p.clone(), origin));
                }
            }
            // Apply resolution clauses in source order.
            for r in self.resolutions.get(c).cloned().unwrap_or_default() {
                match r {
                    Resolution::Use { parent, method, .. } => {
                        if let Some(v) = contrib.get_mut(&method) {
                            v.retain(|(pn, _)| pn == &parent);
                        }
                    }
                    Resolution::Exclude { parent, method, .. } => {
                        if let Some(v) = contrib.get_mut(&method) {
                            v.retain(|(pn, _)| pn != &parent);
                        }
                    }
                    Resolution::Rename {
                        parent,
                        method,
                        as_name,
                        ..
                    } => {
                        let moved: Vec<(String, (String, String))> =
                            if let Some(v) = contrib.get_mut(&method) {
                                let (keep, take): (Vec<_>, Vec<_>) =
                                    v.drain(..).partition(|(pn, _)| pn != &parent);
                                *v = keep;
                                take
                            } else {
                                Vec::new()
                            };
                        if !moved.is_empty() {
                            contrib.entry(as_name).or_default().extend(moved);
                        }
                    }
                }
            }
            // Finalize each inherited name: dedup by origin (diamond), else conflict.
            for (name, v) in contrib {
                if map.contains_key(&name) {
                    continue;
                }
                let distinct: BTreeSet<(String, String)> = v.into_iter().map(|(_, o)| o).collect();
                let mut it = distinct.into_iter();
                match it.next() {
                    None => {}
                    Some(first) => {
                        if it.next().is_some() {
                            let span = self.spans.get(c).copied().unwrap_or(Span {
                                start: 0,
                                len: 0,
                                line: 1,
                                col: 1,
                            });
                            self.conflicts.push((c.to_string(), name.clone(), span));
                        }
                        map.insert(name, first); // deterministic pick (sorted-first)
                    }
                }
            }
            self.in_progress.remove(c);
            self.memo.insert(c.to_string(), map.clone());
            map
        }
    }

    let mut ctx = Ctx {
        decl: BTreeMap::new(),
        extends: BTreeMap::new(),
        resolutions: BTreeMap::new(),
        spans: BTreeMap::new(),
        memo: BTreeMap::new(),
        conflicts: Vec::new(),
        in_progress: BTreeSet::new(),
    };
    for item in &program.items {
        if let Item::Class(c) = item {
            let mut ms = BTreeSet::new();
            for m in &c.members {
                if let ClassMember::Method(f) = m {
                    ms.insert(f.name.clone());
                }
            }
            ctx.decl.insert(c.name.clone(), ms);
            ctx.extends.insert(c.name.clone(), c.extends.clone());
            ctx.resolutions
                .insert(c.name.clone(), c.resolutions.clone());
            ctx.spans.insert(c.name.clone(), c.span);
        }
    }
    let names: Vec<String> = ctx.extends.keys().cloned().collect();
    for n in &names {
        ctx.resolve(n);
    }
    let mut out: BTreeMap<(String, String), (String, String)> = BTreeMap::new();
    for (c, m) in &ctx.memo {
        for (name, origin) in m {
            out.insert((c.clone(), name.clone()), origin.clone());
        }
    }
    (out, ctx.conflicts)
}

/// M-RT S6c — instance-field collision detection, the field analog of [`class_method_origins`].
/// Returns every `(class, field, class_span)` where a class inherits a same-named instance field from
/// **two or more distinct declaring origins** without redeclaring it — the checker reports each as
/// `E-MI-FIELD-CONFLICT`. Unlike methods there are no resolution clauses (PHP has no `insteadof` for
/// properties), so a child can resolve a collision only by redeclaring the field itself.
///
/// "Instance field" = an explicit non-`static` `Field` member plus every promoted constructor
/// parameter (one carrying a `public`/`private`/`protected` modifier — these become fields, EV-4).
/// A diamond auto-merges exactly like a shared method: a field reached through two arms that resolve
/// to the *same* declaring class dedups (no conflict). Static fields are out of scope this slice.
pub fn class_field_conflicts(program: &Program) -> Vec<(String, String, Span)> {
    use std::collections::{BTreeMap, BTreeSet};

    struct Ctx {
        decl: BTreeMap<String, BTreeSet<String>>,
        extends: BTreeMap<String, Vec<String>>,
        spans: BTreeMap<String, Span>,
        memo: BTreeMap<String, BTreeMap<String, String>>,
        conflicts: Vec<(String, String, Span)>,
        in_progress: BTreeSet<String>,
    }

    impl Ctx {
        /// Resolve `c`'s flat instance-field table: each field name → its single declaring origin
        /// class. Own fields win (redeclare); a name arriving from ≥2 distinct origins is recorded as
        /// a conflict (a deterministic pick still lands in the table so the build can continue).
        fn resolve(&mut self, c: &str) -> BTreeMap<String, String> {
            if let Some(m) = self.memo.get(c) {
                return m.clone();
            }
            if !self.in_progress.insert(c.to_string()) {
                return BTreeMap::new(); // `extends` cycle — `E-MI-CYCLE` reported elsewhere
            }
            let mut map: BTreeMap<String, String> = BTreeMap::new();
            // Own fields win over anything inherited (the child redeclaring resolves a collision).
            if let Some(fs) = self.decl.get(c).cloned() {
                for f in fs {
                    map.insert(f, c.to_string());
                }
            }
            // Gather each direct parent's resolved fields, tracking the true declaring origin so a
            // diamond dedups by origin.
            let mut contrib: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
            for p in self.extends.get(c).cloned().unwrap_or_default() {
                for (name, origin) in self.resolve(&p) {
                    if map.contains_key(&name) {
                        continue; // redeclared by C itself
                    }
                    contrib.entry(name).or_default().insert(origin);
                }
            }
            for (name, origins) in contrib {
                if map.contains_key(&name) {
                    continue;
                }
                let mut it = origins.into_iter();
                if let Some(first) = it.next() {
                    if it.next().is_some() {
                        let span = self.spans.get(c).copied().unwrap_or(Span {
                            start: 0,
                            len: 0,
                            line: 1,
                            col: 1,
                        });
                        self.conflicts.push((c.to_string(), name.clone(), span));
                    }
                    map.insert(name, first); // deterministic pick (sorted-first)
                }
            }
            self.in_progress.remove(c);
            self.memo.insert(c.to_string(), map.clone());
            map
        }
    }

    let mut ctx = Ctx {
        decl: BTreeMap::new(),
        extends: BTreeMap::new(),
        spans: BTreeMap::new(),
        memo: BTreeMap::new(),
        conflicts: Vec::new(),
        in_progress: BTreeSet::new(),
    };
    for item in &program.items {
        if let Item::Class(c) = item {
            let mut fs = BTreeSet::new();
            for m in &c.members {
                match m {
                    ClassMember::Field {
                        name, modifiers, ..
                    } if !modifiers.contains(&Modifier::Static) => {
                        fs.insert(name.clone());
                    }
                    ClassMember::Constructor { params, .. } => {
                        for p in params {
                            if p.modifiers.iter().any(|m| {
                                matches!(
                                    m,
                                    Modifier::Public | Modifier::Private | Modifier::Protected
                                )
                            }) {
                                fs.insert(p.name.clone());
                            }
                        }
                    }
                    _ => {}
                }
            }
            ctx.decl.insert(c.name.clone(), fs);
            ctx.extends.insert(c.name.clone(), c.extends.clone());
            ctx.spans.insert(c.name.clone(), c.span);
        }
    }
    let names: Vec<String> = ctx.extends.keys().cloned().collect();
    for n in &names {
        ctx.resolve(n);
    }
    ctx.conflicts
}

/// The ordered list of constructors a `ClassName(args)` call runs (M-RT S6c.2). Each entry is one
/// `(params, body)` to execute, in order, on the single instance being built; the call's full argument
/// list is the entries' params concatenated in this order, sliced per entry.
///
/// - A class with its **own** constructor → just that one (`[own]`).
/// - **Single** inheritance, no own ctor → the parent's plan (the nearest ancestor's ctor, transitively
///   chained — S6c.2a).
/// - **Multiple** inheritance, no own ctor → each parent's plan concatenated in `extends` order, so
///   every parent's constructor runs and initializes its fields (S6c.2b). A diamond-shared base's ctor
///   runs once per arm — identically on all three backends, so byte-identity holds.
/// - No ctor anywhere → `[]` (a zero-arg `ClassName()` builds an empty instance).
///
/// Single source of the construction decision: checker (signature = concatenated param types),
/// compiler (instance descriptor + synthetic ctor body + arity), interpreter (run each entry with its
/// arg slice). A child that declares its *own* ctor under inheritance returns just its own — initializing
/// inherited state then needs the deferred `super`-replacement (KNOWN_ISSUES).
pub fn ctor_plan(program: &Program, class: &str) -> Vec<(Vec<CtorParam>, Vec<Stmt>)> {
    let Some(decl) = program.items.iter().find_map(|it| match it {
        Item::Class(c) if c.name == class => Some(c),
        _ => None,
    }) else {
        return Vec::new();
    };
    if let Some((p, b)) = decl.members.iter().find_map(|m| match m {
        ClassMember::Constructor { params, body, .. } => Some((params.clone(), body.clone())),
        _ => None,
    }) {
        return vec![(p, b)];
    }
    match decl.extends.len() {
        0 => Vec::new(),
        1 => ctor_plan(program, &decl.extends[0]),
        _ => decl
            .extends
            .iter()
            .flat_map(|p| ctor_plan(program, p))
            .collect(),
    }
}

fn collect_free_expr(
    e: &Expr,
    bound: &mut std::collections::HashSet<String>,
    found: &mut std::collections::BTreeSet<String>,
) {
    match e {
        Expr::Ident(name, _) => {
            if !bound.contains(name) {
                found.insert(name.clone());
            }
        }
        Expr::This(_) => {} // `this` is never captured (E-LAMBDA-THIS rejects it at check time)
        Expr::Int(..) | Expr::Float(..) | Expr::Bool(..) | Expr::Null(..) | Expr::Bytes(..) => {}
        Expr::Str(parts, _) | Expr::Html(parts, _) => {
            for part in parts {
                if let StrPart::Expr(inner) = part {
                    collect_free_expr(inner, bound, found);
                }
            }
        }
        Expr::List(items, _) => {
            for it in items {
                collect_free_expr(it, bound, found);
            }
        }
        Expr::Map(pairs, _) => {
            for (k, v) in pairs {
                collect_free_expr(k, bound, found);
                collect_free_expr(v, bound, found);
            }
        }
        Expr::Unary { expr, .. } => collect_free_expr(expr, bound, found),
        Expr::Binary { lhs, rhs, .. } => {
            collect_free_expr(lhs, bound, found);
            collect_free_expr(rhs, bound, found);
        }
        Expr::InstanceOf { value, .. } => collect_free_expr(value, bound, found),
        Expr::Call { callee, args, .. } => {
            collect_free_expr(callee, bound, found);
            for a in args {
                collect_free_expr(a, bound, found);
            }
        }
        Expr::Member { object, .. } => collect_free_expr(object, bound, found),
        Expr::Index { object, index, .. } => {
            collect_free_expr(object, bound, found);
            collect_free_expr(index, bound, found);
        }
        Expr::Force { inner, .. } => collect_free_expr(inner, bound, found),
        Expr::Propagate { inner, .. } => collect_free_expr(inner, bound, found),
        Expr::CloneWith { object, fields, .. } => {
            collect_free_expr(object, bound, found);
            for (_, e) in fields {
                collect_free_expr(e, bound, found);
            }
        }
        Expr::Match {
            scrutinee, arms, ..
        } => {
            collect_free_expr(scrutinee, bound, found);
            for arm in arms {
                // arm-pattern bindings are in scope for the arm body only
                let mut arm_bound = bound.clone();
                collect_pattern_bindings(&arm.pattern, &mut arm_bound);
                collect_free_expr(&arm.body, &mut arm_bound, found);
            }
        }
        Expr::Range { start, end, .. } => {
            collect_free_expr(start, bound, found);
            collect_free_expr(end, bound, found);
        }
        Expr::If {
            cond,
            then_expr,
            else_expr,
            ..
        } => {
            collect_free_expr(cond, bound, found);
            collect_free_expr(then_expr, bound, found);
            collect_free_expr(else_expr, bound, found);
        }
        Expr::Lambda { params, body, .. } => {
            // Nested lambda: its params shadow outer names; walk the body with an extended
            // bound set (but do NOT add its params to the outer `bound` set).
            let mut inner_bound = bound.clone();
            for p in params {
                inner_bound.insert(p.name.clone());
            }
            match body {
                LambdaBody::Expr(inner_e) => collect_free_expr(inner_e, &mut inner_bound, found),
                LambdaBody::Block(stmts) => collect_free_block(stmts, &mut inner_bound, found),
            }
        }
    }
}

fn collect_free_block(
    stmts: &[Stmt],
    bound: &mut std::collections::HashSet<String>,
    found: &mut std::collections::BTreeSet<String>,
) {
    for s in stmts {
        collect_free_stmt(s, bound, found);
    }
}

fn collect_free_stmt(
    s: &Stmt,
    bound: &mut std::collections::HashSet<String>,
    found: &mut std::collections::BTreeSet<String>,
) {
    match s {
        Stmt::VarDecl { name, init, .. } => {
            // The initializer is evaluated before the name enters scope
            collect_free_expr(init, bound, found);
            bound.insert(name.clone());
        }
        Stmt::Return { value, .. } => {
            if let Some(e) = value {
                collect_free_expr(e, bound, found);
            }
        }
        Stmt::If {
            cond,
            bind,
            then_block,
            else_block,
            ..
        } => {
            collect_free_expr(cond, bound, found);
            let mut then_bound = bound.clone();
            if let Some(name) = bind {
                then_bound.insert(name.clone());
            }
            collect_free_block(then_block, &mut then_bound, found);
            if let Some(eb) = else_block {
                let mut else_bound = bound.clone();
                collect_free_block(eb, &mut else_bound, found);
            }
        }
        Stmt::For {
            name, iter, body, ..
        } => {
            collect_free_expr(iter, bound, found);
            let mut loop_bound = bound.clone();
            loop_bound.insert(name.clone());
            collect_free_block(body, &mut loop_bound, found);
        }
        Stmt::While { cond, body, .. } => {
            collect_free_expr(cond, bound, found);
            let mut loop_bound = bound.clone();
            collect_free_block(body, &mut loop_bound, found);
        }
        Stmt::CFor {
            init,
            cond,
            step,
            body,
            ..
        } => {
            // `init` declares into the loop's own scope; `cond`/`step`/`body` see those bindings.
            let mut loop_bound = bound.clone();
            if let Some(s) = init {
                collect_free_stmt(s, &mut loop_bound, found);
            }
            if let Some(c) = cond {
                collect_free_expr(c, &mut loop_bound, found);
            }
            if let Some(s) = step {
                collect_free_stmt(s, &mut loop_bound, found);
            }
            collect_free_block(body, &mut loop_bound, found);
        }
        Stmt::Break(_) | Stmt::Continue(_) => {}
        Stmt::Block(stmts, _) => {
            let mut inner = bound.clone();
            collect_free_block(stmts, &mut inner, found);
        }
        Stmt::Assign { target, value, .. } => {
            // Reassignment: the target names an existing binding (a use, not a new binding),
            // and the value is evaluated against the current scope.
            collect_free_expr(target, bound, found);
            collect_free_expr(value, bound, found);
        }
        Stmt::Expr(e, _) => collect_free_expr(e, bound, found),
        Stmt::Throw { value, .. } => collect_free_expr(value, bound, found),
        Stmt::Try {
            body,
            catches,
            finally_block,
            ..
        } => {
            let mut try_bound = bound.clone();
            collect_free_block(body, &mut try_bound, found);
            for c in catches {
                // The catch binding is in scope only inside its own clause body.
                let mut catch_bound = bound.clone();
                catch_bound.insert(c.name.clone());
                collect_free_block(&c.body, &mut catch_bound, found);
            }
            if let Some(fb) = finally_block {
                let mut fin_bound = bound.clone();
                collect_free_block(fb, &mut fin_bound, found);
            }
        }
    }
}

fn collect_pattern_bindings(pat: &Pattern, bound: &mut std::collections::HashSet<String>) {
    match pat {
        Pattern::Binding { name, .. } => {
            bound.insert(name.clone());
        }
        Pattern::Variant { fields, .. } => {
            for f in fields {
                collect_pattern_bindings(f, bound);
            }
        }
        // A type pattern (`Circle c`, M-RT S4) binds its `binding` (if any) for the arm body.
        Pattern::Type {
            binding: Some(name),
            ..
        } => {
            bound.insert(name.clone());
        }
        _ => {}
    }
}

/// A function/method parameter: `Type name`.
#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub ty: Type,
    pub name: String,
    pub span: Span,
}

/// Visibility / binding modifiers on class members and promoted constructor params.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Modifier {
    Public,
    Private,
    Protected,
    Const,
    /// `open` on a class or method (M-RT S6) — opts into extensibility/overridability. Phorge is
    /// **final-by-default** (a non-`open` class can't be `extend`ed; a non-`open` method can't be
    /// overridden), so the `final` keyword is retired. Checker-enforced (`E-EXTEND-FINAL`/
    /// `E-OVERRIDE-FINAL`); the transpiler emits PHP `final` for the *absence* of `open`. The
    /// extensibility axis of the modifier model, orthogonal to `mutable` (mutation) and `static`.
    Open,
    /// `mutable` on a class field or promoted ctor param (M-mut.6) — the field may be reassigned via
    /// `o.f = e`. Immutable by default (a property of the place, not the type); erased in PHP output
    /// (PHP properties are always mutable unless `readonly`). The binding analog of `VarDecl.mutable`.
    Mutable,
    /// `static` on a class field (M-mut.7) — class-level (not per-instance), program-lifetime state
    /// accessed as `ClassName.field`. The Association axis of the modifier model. Transpiles to a PHP
    /// `static` property.
    Static,
    /// `abstract` on a method (M-RT S6b) — a bodyless signature a concrete subclass must implement.
    /// Implicitly `open` (overridable). Legal only in an `abstract class`; the transpiler emits a PHP
    /// `abstract function …;`.
    Abstract,
}

/// Declaration-level visibility on a top-level item (visibility modifiers). A NEW axis, distinct from
/// the member-level `Modifier::{Public,Private,Protected}`. Ordered so `vis >= Visibility::Internal`
/// reads as "at least package-visible": `Private` (this file only) < `Internal` (this package) <
/// `Public` (cross-package; the default). Enforced entirely in the loader; never read by a backend
/// (PHP has no file/package-private declarations), so it is "erased" simply by being ignored
/// downstream — the byte-identity spine is safe by construction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Visibility {
    Private,
    Internal,
    Public,
}

/// A constructor parameter, which may carry promotion modifiers
/// (`constructor(private string name)`).
#[derive(Debug, Clone, PartialEq)]
pub struct CtorParam {
    pub modifiers: Vec<Modifier>,
    pub ty: Type,
    pub name: String,
    pub span: Span,
}

/// Statements — appear inside function/method bodies.
#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    /// `Type name = expr;` or `mutable Type name = expr;` (M-mut.1). `mutable` is a *binding*
    /// modifier (a property of the place, not the type) — immutable by default; only a `mutable`
    /// binding may be reassigned via `Stmt::Assign`. Erased in PHP output (PHP locals are always
    /// mutable); checker-only.
    VarDecl {
        ty: Type,
        name: String,
        init: Expr,
        mutable: bool,
        span: Span,
    },
    /// `<lvalue> = expr;` — reassignment (M-mut.1). `target` is an lvalue expression; this slice
    /// accepts only `Expr::Ident` (field/index targets land in M-mut.5/6 and extend this same
    /// statement). The checker enforces the target is `mutable` (`E-ASSIGN-IMMUTABLE`).
    Assign {
        target: Expr,
        value: Expr,
        span: Span,
    },
    /// `return;` or `return expr;`
    Return { value: Option<Expr>, span: Span },
    /// `if (cond) { .. } [else { .. } | else if ..]` — else-branch is a block (an
    /// `else if` chain is stored as a single-statement block wrapping a nested `If`).
    ///
    /// `bind` is `Some(name)` for the `if (var name = cond)` form (M3 S2.4): `cond` is the optional
    /// scrutinee, and `name` is smart-cast to the non-optional inner `T` inside `then_block` only.
    If {
        cond: Expr,
        bind: Option<String>,
        then_block: Vec<Stmt>,
        else_block: Option<Vec<Stmt>>,
        span: Span,
    },
    /// `for (Type name in iter) { .. }`
    For {
        ty: Type,
        name: String,
        iter: Expr,
        body: Vec<Stmt>,
        span: Span,
    },
    /// A condition loop (M-mut.3): `while (cond) { .. }` (`post_cond = false`) or
    /// `do { .. } while (cond);` (`post_cond = true` — the body runs once before the first test).
    /// Lowers to existing `Jump`/`JumpIfFalse` back-edges (F5) — no new loop opcode. while-let
    /// (`while (var x = opt) { .. }`) is desugared by the parser into `while (true) { if (var x = opt)
    /// { .. } else { break; } }`, reusing the if-let lowering, so it needs no representation here.
    While {
        cond: Expr,
        body: Vec<Stmt>,
        post_cond: bool,
        span: Span,
    },
    /// C-style `for (init; cond; step) { .. }` (M-mut.3). Each clause is optional (`for (;;) {}` is
    /// an infinite loop); `init`/`step` are statements (a `VarDecl`/`Assign`/`Expr`), `cond` an
    /// expression. Lowers to the same jump back-edge as `While` with `step` at the continue target.
    CFor {
        init: Option<Box<Stmt>>,
        cond: Option<Expr>,
        step: Option<Box<Stmt>>,
        body: Vec<Stmt>,
        span: Span,
    },
    /// `break;` — exit the innermost enclosing loop (M-mut.3).
    Break(Span),
    /// `continue;` — skip to the next iteration of the innermost enclosing loop (M-mut.3).
    Continue(Span),
    /// `{ .. }`
    Block(Vec<Stmt>, Span),
    /// `expr;`
    Expr(Expr, Span),
    /// `throw expr;` (M-faults 2b). `value` is `never`-typed at the statement level (a `throw`
    /// diverges — it satisfies return-on-all-paths); the thrown value must be `<: Error`.
    Throw { value: Expr, span: Span },
    /// `try { .. } catch (Type name) { .. } [catch …] [finally { .. }]` (M-faults 2b). At least one
    /// `catch` **or** a `finally` is present (parser-enforced). Catches are tried in source order; a
    /// thrown value matches the first clause whose `ty` it is an `instanceof`. `finally_block` runs on
    /// every exit edge (normal, caught, re-propagated, and a `return`/`break`/`continue` escaping the
    /// try). An uncatchable fault (panic) passes straight through every `catch`.
    Try {
        body: Vec<Stmt>,
        catches: Vec<CatchClause>,
        finally_block: Option<Vec<Stmt>>,
        span: Span,
    },
}

/// One `catch (Type name) { .. }` clause of a [`Stmt::Try`] (M-faults 2b). `ty` may be a union
/// (`catch (A | B e)`) — `name` is then bound at the union type. Each clause has its own binding,
/// scope, and body.
#[derive(Debug, Clone, PartialEq)]
pub struct CatchClause {
    pub ty: Type,
    pub name: String,
    pub body: Vec<Stmt>,
    pub span: Span,
}

/// A function or method declaration. `modifiers` is empty for a free (top-level) function.
#[derive(Debug, Clone, PartialEq)]
pub struct FunctionDecl {
    pub modifiers: Vec<Modifier>,
    /// Declaration-level visibility. Meaningful only for a free (top-level) function; a method or an
    /// interface method signature carries `Visibility::Public` and the loader never checks it.
    pub vis: Visibility,
    pub name: String,
    /// Generic type parameters, in declaration order — `["T", "U"]` for
    /// `function pair<T, U>(T a, U b) -> …` (M-RT S7). Empty for a non-generic function. A type
    /// annotation naming one of these (e.g. `T`) resolves to `Ty::Param("T")` while checking this
    /// function, and is erased to `Type::Erased` before any backend runs.
    pub type_params: Vec<String>,
    pub params: Vec<Param>,
    pub ret: Option<Type>,
    /// Declared checked-exception set: the `throws T (| T)*` clause (M-faults 2b). Empty for a
    /// function that throws nothing. Each member must be a specific subtype of the built-in `Error`
    /// (the bare root is `E-THROWS-TOO-BROAD`). Erased before any backend — the `throws` declaration
    /// is checker-only (PHP has no checked exceptions).
    pub throws: Vec<Type>,
    pub body: Vec<Stmt>,
    pub span: Span,
}

/// One variant of an enum, with optional associated data fields (`Circle(float radius)`).
#[derive(Debug, Clone, PartialEq)]
pub struct EnumVariant {
    pub name: String,
    pub fields: Vec<Param>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnumDecl {
    /// Declaration-level visibility (default `Public`). Loader-enforced; see [`Visibility`].
    pub vis: Visibility,
    pub name: String,
    /// Generic type parameters, in declaration order — `["T"]` for `enum Option<T>`, `["T", "E"]` for
    /// `enum Result<T, E>` (M-RT generic enums). Empty for a non-generic enum — the common case. While
    /// checking the enum, a bare type name in this set resolves to `Ty::Param` in a variant's field
    /// types; a generic value's arguments are inferred at the variant constructor and these parameters
    /// are **erased** (rewritten to `Type::Erased` across every variant) before any backend runs —
    /// the same compile-time-only discipline as generic classes (`Box<T>`).
    pub type_params: Vec<String>,
    pub variants: Vec<EnumVariant>,
    pub span: Span,
}

/// A member of a class.
#[derive(Debug, Clone, PartialEq)]
pub enum ClassMember {
    Field {
        modifiers: Vec<Modifier>,
        ty: Type,
        name: String,
        /// A field-level initializer (`static mutable int total = 0;`). Required for `static` fields
        /// (class-level state has no constructor to set it); must be `None` for an instance field
        /// (those are set via the constructor). Restricted to a literal constant this slice (M-mut.7).
        init: Option<Expr>,
        span: Span,
    },
    Constructor {
        params: Vec<CtorParam>,
        body: Vec<Stmt>,
        span: Span,
    },
    Method(FunctionDecl),
    /// A **property hook** (M-mut.7b) — a member that looks like a field but computes on read and/or
    /// intercepts writes: `T name { get => expr; set(T v) { stmts } }`. v1 is *virtual-only*: it
    /// declares no storage and takes no initializer, so it is never an instance field (no slot in the
    /// instance map, never promoted, invisible to `clone with`). A `get` is an expression evaluated
    /// with `this` in scope (a read-only computed property); a `set` is a block with the assigned
    /// value bound to its parameter `v`, run with `this` in scope (typically writing other `mutable`
    /// fields). A hook may have get-only, set-only, or both. Reading a get-less hook is
    /// `E-HOOK-NO-GET`; writing a set-less one is `E-HOOK-NO-SET`. Lowers on the VM to synthetic
    /// methods `<Class>::<name>$get`/`$set` dispatched via `Op::CallMethod` (no new `Op`);
    /// transpiles 1:1 to a PHP 8.4 property hook.
    Hook {
        ty: Type,
        name: String,
        /// `get => <expr>` — the computed-read body; `None` for a write-only hook.
        get: Option<Expr>,
        /// `set(T v) { <stmts> }` — the intercepted-write body; the `Param` carries `v`'s name+type.
        /// `None` for a read-only computed hook.
        set: Option<(Param, Vec<Stmt>)>,
        span: Span,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClassDecl {
    /// Declaration-level visibility (default `Public`). Loader-enforced; see [`Visibility`].
    pub vis: Visibility,
    pub name: String,
    /// Generic type parameters, in declaration order — `["T"]` for `class Box<T>`, `["A", "B"]` for
    /// `class Pair<A, B>` (M-RT generics-all). Empty for a non-generic class — the common case. While
    /// checking the class, a bare type name in this set resolves to `Ty::Param`; a generic instance's
    /// arguments are inferred at construction and these parameters are **erased** (rewritten to
    /// `Type::Erased` across every member) before any backend runs.
    pub type_params: Vec<String>,
    /// Parent classes this class `extends` (M-RT S6). Empty for a root class; one entry for single
    /// inheritance (`class Dog extends Animal`); two or more for multiple inheritance
    /// (`class Duck extends Swimmer, Flyer`). Each parent must be an `open` class
    /// (`E-EXTEND-FINAL` otherwise); a cycle is `E-MI-CYCLE`. The checker flattens the transitive
    /// supertype set (`ast::class_supertypes`) for subtyping/`instanceof`, and inherits the parents'
    /// fields and methods into this class. Multi-parent collisions must be explicitly resolved (S6b).
    pub extends: Vec<String>,
    /// Interfaces this class declares it implements (`class Dog implements Speaker, Named`). The
    /// checker (`E-IFACE-IMPL`/`E-IFACE-UNIMPL`/`E-IFACE-SIG`) validates each name resolves to an
    /// interface and the class provides every method of it and its `extends` chain (M-RT S2).
    pub implements: Vec<String>,
    /// `open class` — whether this class may be `extend`ed (M-RT S6). **Final-by-default**: a
    /// non-`open` class is a leaf (`E-EXTEND-FINAL` if a subclass names it). The transpiler emits a
    /// PHP `final class` for a non-`open` class. The extensibility opt-in, orthogonal to `vis`.
    pub open: bool,
    /// `abstract class` (M-RT S6b) — cannot be instantiated (`E-ABSTRACT-INSTANTIATE`); may declare
    /// `abstract` (bodyless) methods that a concrete subclass must implement (`E-ABSTRACT-UNIMPL`).
    /// Abstract implies extensible, so the parser also sets `open` for an abstract class.
    pub is_abstract: bool,
    /// Explicit multi-inheritance resolution clauses (M-RT S6b), declared in the class body before/among
    /// members: `use P.m` (pick `P`'s `m` for the colliding name), `rename P.m as n` (rebind `P`'s `m`
    /// under a fresh name `n`, removing it from the collision), `exclude P.m` (drop `P`'s `m`). Empty
    /// for a single-parent or collision-free class. Consumed by `ast::class_method_origins` (dispatch)
    /// and the transpiler (`insteadof`/`as` emission). An unresolved cross-parent method collision is
    /// `E-MI-CONFLICT`.
    pub resolutions: Vec<Resolution>,
    pub members: Vec<ClassMember>,
    pub span: Span,
}

/// A multi-inheritance conflict-resolution clause (M-RT S6b) — see [`ClassDecl::resolutions`]. Each
/// names a **direct parent** and one of its methods; the checker validates the parent/method exist and
/// that every cross-parent collision is resolved (`E-MI-CONFLICT`).
#[derive(Debug, Clone, PartialEq)]
pub enum Resolution {
    /// `use P.m` — pick parent `P`'s `m` as the winner for the method name `m`; other parents' `m` drop.
    Use {
        parent: String,
        method: String,
        span: Span,
    },
    /// `rename P.m as n` — bind parent `P`'s `m` under the new name `n` (and remove it from the `m`
    /// collision, so a single remaining source resolves `m`).
    Rename {
        parent: String,
        method: String,
        as_name: String,
        span: Span,
    },
    /// `exclude P.m` — drop parent `P`'s contribution to the method name `m`.
    Exclude {
        parent: String,
        method: String,
        span: Span,
    },
}

/// An interface declaration (`interface Speaker { method-sigs } [extends A, B]`). Methods are
/// signatures only — a `FunctionDecl` with an empty body (M-RT S2). Interfaces are nominal types
/// usable as a variable/parameter type; a class that `implements` one is a subtype of it. PHP-absent
/// at runtime: there are no interface instances, so the backends only use interfaces for the
/// `instanceof` table and (the transpiler) for emitting a PHP `interface`.
#[derive(Debug, Clone, PartialEq)]
pub struct InterfaceDecl {
    /// Declaration-level visibility (default `Public`). Loader-enforced; see [`Visibility`].
    pub vis: Visibility,
    pub name: String,
    /// Parent interfaces (`interface Animal extends Speaker, Named`) — flattened transitively.
    pub extends: Vec<String>,
    /// Method signatures (each a `FunctionDecl` with an empty body).
    pub methods: Vec<FunctionDecl>,
    pub span: Span,
}

/// A top-level item in a program.
#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    /// `import a.b.c;` or `import a.b.c as leaf;` — `alias`, when present, overrides the call-site
    /// qualifier (the bound leaf) so colliding leaves from different packages can coexist (M5 S2c,
    /// design O-9). `None` ⇒ the qualifier is `path`'s last segment.
    Import {
        path: Vec<String>,
        alias: Option<String>,
        /// `import type a.b.C [as D];` — a *terminal type* import: the leaf (`C`) is a user/library
        /// **type**, bound bare (or as `D`). Resolved + erased before any backend by the loader's
        /// cross-package type pass (M-RT generics-all / cross-package types). A plain module import
        /// (`import a.b;`, for Go-qualified `b.fn()` calls) has `type_only = false`.
        type_only: bool,
        span: Span,
    },
    Function(FunctionDecl),
    Enum(EnumDecl),
    Class(ClassDecl),
    Interface(InterfaceDecl),
    /// `type Name = Type;` — a compile-time alias, erased after checking (resolved by the checker
    /// and expanded out of the AST before any backend runs).
    TypeAlias {
        name: String,
        ty: Type,
        span: Span,
    },
}

/// A whole parsed program.
#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    /// The file's package path (`package App.Util;` ⇒ `["App", "Util"]`). Empty only for a
    /// malformed file with no declaration — the checker rejects that as `E-NO-PACKAGE` (M5: every
    /// file is packaged, never inferred). The reserved `["Main"]` is the runnable entry (M5 S1).
    pub package: Vec<String>,
    pub items: Vec<Item>,
    pub span: Span,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::token::Span;

    fn sp() -> Span {
        Span {
            start: 0,
            len: 1,
            line: 1,
            col: 1,
        }
    }

    #[test]
    fn builds_binary_expr() {
        let e = Expr::Binary {
            op: BinaryOp::Add,
            lhs: Box::new(Expr::Int(1, sp())),
            rhs: Box::new(Expr::Int(2, sp())),
            span: sp(),
        };
        match e {
            Expr::Binary { op, .. } => assert_eq!(op, BinaryOp::Add),
            _ => panic!("expected Binary"),
        }
    }

    #[test]
    fn builds_variant_pattern() {
        let p = Pattern::Variant {
            name: "Circle".into(),
            fields: vec![Pattern::Binding {
                name: "r".into(),
                span: sp(),
            }],
            span: sp(),
        };
        match p {
            Pattern::Variant { name, fields, .. } => {
                assert_eq!(name, "Circle");
                assert_eq!(fields.len(), 1);
            }
            _ => panic!("expected Variant"),
        }
    }

    #[test]
    fn builds_var_decl_stmt() {
        let s = Stmt::VarDecl {
            ty: Type::Named {
                name: "int".into(),
                args: vec![],
                span: sp(),
            },
            name: "n".into(),
            init: Expr::Int(5, sp()),
            mutable: false,
            span: sp(),
        };
        match s {
            Stmt::VarDecl { name, .. } => assert_eq!(name, "n"),
            _ => panic!("expected VarDecl"),
        }
    }

    #[test]
    fn builds_function_item() {
        let f = FunctionDecl {
            modifiers: vec![Modifier::Private],
            vis: Visibility::Public,
            name: "area".into(),
            type_params: vec![],
            params: vec![Param {
                ty: Type::Named {
                    name: "Shape".into(),
                    args: vec![],
                    span: sp(),
                },
                name: "s".into(),
                span: sp(),
            }],
            ret: Some(Type::Named {
                name: "float".into(),
                args: vec![],
                span: sp(),
            }),
            throws: vec![],
            body: vec![],
            span: sp(),
        };
        match Item::Function(f) {
            Item::Function(d) => {
                assert_eq!(d.name, "area");
                assert_eq!(d.params.len(), 1);
                assert!(d.ret.is_some());
            }
            _ => panic!("expected Function item"),
        }
    }

    // --- F1: free_vars unit tests (M3 S3 Task 4) ---

    /// Build a bare `Expr::Ident` (no span needed beyond a dummy one).
    fn ident(name: &str) -> Expr {
        Expr::Ident(name.to_string(), sp())
    }

    /// Build a `Param` with a dummy int type.
    fn int_param(name: &str) -> Param {
        Param {
            ty: Type::Named {
                name: "int".into(),
                args: vec![],
                span: sp(),
            },
            name: name.to_string(),
            span: sp(),
        }
    }

    #[test]
    fn free_vars_no_captures() {
        // `fn(int x) => x` — `x` is a param, no free vars.
        let body = LambdaBody::Expr(Box::new(ident("x")));
        assert_eq!(free_vars(&[int_param("x")], &body), Vec::<String>::new());
    }

    #[test]
    fn free_vars_simple_capture() {
        // `fn(int x) => x + a` — `a` is free.
        let body = LambdaBody::Expr(Box::new(Expr::Binary {
            op: BinaryOp::Add,
            lhs: Box::new(ident("x")),
            rhs: Box::new(ident("a")),
            span: sp(),
        }));
        assert_eq!(free_vars(&[int_param("x")], &body), vec!["a".to_string()]);
    }

    #[test]
    fn free_vars_two_captures_sorted() {
        // `fn(int x) => x + a + b` — `a` and `b` are free; result is sorted.
        let inner = Expr::Binary {
            op: BinaryOp::Add,
            lhs: Box::new(ident("x")),
            rhs: Box::new(ident("a")),
            span: sp(),
        };
        let body = LambdaBody::Expr(Box::new(Expr::Binary {
            op: BinaryOp::Add,
            lhs: Box::new(inner),
            rhs: Box::new(ident("b")),
            span: sp(),
        }));
        let got = free_vars(&[int_param("x")], &body);
        assert_eq!(got, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn free_vars_inner_var_not_captured() {
        // `fn(int x) { var y = x; return y; }` — `y` is bound inside, `x` is a param.
        let body = LambdaBody::Block(vec![
            Stmt::VarDecl {
                ty: Type::Infer(sp()),
                name: "y".to_string(),
                init: ident("x"),
                mutable: false,
                span: sp(),
            },
            Stmt::Return {
                value: Some(ident("y")),
                span: sp(),
            },
        ]);
        assert_eq!(free_vars(&[int_param("x")], &body), Vec::<String>::new());
    }

    #[test]
    fn assign_free_vars_includes_target_and_value() {
        // `x = y;` — both the target binding and the value are free-variable uses.
        let s = Stmt::Assign {
            target: ident("x"),
            value: ident("y"),
            span: sp(),
        };
        let mut found = std::collections::BTreeSet::new();
        let mut bound = std::collections::HashSet::new();
        collect_free_stmt(&s, &mut bound, &mut found);
        assert!(found.contains("x") && found.contains("y"));
    }
}
