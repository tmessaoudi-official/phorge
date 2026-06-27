//! Abstract syntax tree: the parser's output and the shared input to every backend (checker,
//! tree-walking interpreter, bytecode compiler, PHP transpiler). Nodes are **untyped** — the
//! checker validates without annotating, so each backend re-derives the types it needs (see
//! `compiler::CTy`). `token::Span` is carried on nodes for diagnostics.

use crate::token::Span;

// AST analyses live in sibling files (M-Decomp W3.3); re-exported so callers keep
// using `crate::ast::{free_vars, class_implements, ...}` unchanged.
mod classes;
mod walk;
pub use classes::*;
pub use walk::*;

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
    /// `[T; N]` — a **fixed-length list** (Phase 1 types slice): a `List<T>` whose length is a
    /// compile-time constant `N`. Distinct from `List<T>` only in the checker (length tracking +
    /// static literal-index bounds + assignability `[T; N] → List<T>`). At runtime it *is* a list
    /// (`Value::List`, erases to a PHP array) — no new `Value`/`Op`; the length is a compile-time-only
    /// guarantee. The backends treat it exactly as `List<T>` (compiler `CTy::List`, transpiler `array`).
    FixedList {
        elem: Box<Type>,
        len: usize,
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
    /// A `decimal` literal pattern — `19.99d` in a `match` arm (M-NUM S1). Matches numerically
    /// (scale-insensitive, like `==`): `1.5d` matches a scrutinee of `1.50d`.
    Decimal {
        unscaled: i128,
        scale: u8,
        span: Span,
    },
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
    /// `Point { x, y }` / `Point { x: px }` / `Line { from: Point { x, y }, to }` — a **struct
    /// pattern** (M-RT pattern cluster S5.2): matches when the scrutinee is an instance of
    /// `type_name` (a class — the same `instanceof` runtime test as a [`Pattern::Type`], reusing
    /// `Op::IsInstance`), then matches each named field's sub-pattern against that field's value.
    /// Each [`FieldPat`] carries the field name and a sub-pattern; shorthand `x` and rename `x: px`
    /// both desugar to a [`Pattern::Binding`] sub-pattern, so all the existing per-backend pattern
    /// recursion (bind / literal / nested struct) is reused without a new field-target enum.
    Struct {
        type_name: String,
        fields: Vec<FieldPat>,
        span: Span,
    },
}

/// One `field: sub-pattern` entry of a [`Pattern::Struct`]. Shorthand `Point { x }` is sugar for
/// `Point { x: x }` — the parser fills `pat` with `Pattern::Binding { name: field }` — so a field
/// target is always a full [`Pattern`] (bind, literal, wildcard, nested struct, …).
#[derive(Debug, Clone, PartialEq)]
pub struct FieldPat {
    pub field: String,
    pub pat: Pattern,
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
    /// Optional arm guard (`pattern when <cond> => …`). The arm matches only when the pattern
    /// matches AND the guard evaluates true; a false guard falls through to the next arm. `None`
    /// for an unguarded arm. A guarded arm does not discharge its shape for exhaustiveness.
    pub guard: Option<Expr>,
    pub body: Expr,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Not,
    /// `~` — bitwise NOT on an `int` operand (primitives P2).
    BitNot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    /// `**` power (Phase 1 operators slice). Type-directed (`int**int→int`, `float**float→float`);
    /// no dedicated `Op` — the compiler lowers it to `Op::CallNative` (`Core.Math.ipow`/`pow`).
    Pow,
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
    /// Bitwise operators on `int` operands (primitives P2). PHP-identical integer semantics; shifts
    /// fault on a negative count and yield 0 / sign-fill for a count ≥ 64.
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
}

/// Expressions.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Int(i64, Span),
    Float(f64, Span),
    /// A `decimal` literal — `19.99d` (M-NUM S1). `unscaled`/`scale` are parsed from the literal
    /// **text** so trailing zeros set the scale (`1.50d` ⇒ `{1999? no — 150}, scale 2`; `1.500d` ⇒
    /// scale 3; `100d` ⇒ scale 0). Value = `unscaled × 10^(-scale)`. A literal whose digits overflow
    /// i128 is a lex/parse error (`E-DECIMAL-LITERAL`), known at compile time.
    Decimal {
        unscaled: i128,
        scale: u8,
        span: Span,
    },
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
    /// `value as TypeName` — a **checked** downcast (M4 casting axis 2). Result type is `TypeName?`:
    /// `value` itself when `value instanceof TypeName` at runtime, else `null` (the Kotlin/Swift `as?`
    /// model — the honest, surprise-free form of TS's unchecked `<T>v`). The RHS is a class/interface
    /// *type name* (parsed as a type, like `InstanceOf`), so this is its own variant. `value` is
    /// evaluated exactly once. Lowers (no new `Op`) to the `IsInstance` predicate + a branch on the
    /// backends; transpiles to a PHP arrow-IIFE `(fn($x) => $x instanceof T ? $x : null)($value)`.
    /// Composes with `??` / if-let smart-cast. (Value *conversion* — `int→float` etc. — is the
    /// separate `Core.Convert` axis; `as` only reinterprets an existing value.)
    Cast {
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
    /// `new <call>` — the mandatory construction keyword (Feature C). Wraps the inner construction
    /// `Expr::Call` (a class instantiation `new Counter()` or an enum-variant construction `new
    /// Some(7)`). The checker validates that the inner callee really is a class/enum variant
    /// (`E-NEW-ON-NONCONSTRUCT`) and that every construction is `new`-wrapped (`E-NEW-REQUIRED`), then
    /// **unwraps** this node to its inner `Call` (`checker::unwrap_new`, alongside `expand_aliases`/
    /// `erase_generics`) **before any backend runs** — so the interpreter/compiler/transpiler never see
    /// it and construction semantics + the byte-identity spine are unchanged. A bare `new` not followed
    /// by a call is a parse error.
    New(Box<Expr>, Span),
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

/// A function/method parameter: `Type name`.
#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub ty: Type,
    pub name: String,
    /// An optional **default value** (M4 default parameters): `bool b = false`. Restricted to a
    /// literal constant by the checker (`E-DEFAULT-PARAM-EXPR`). A parameter with a default is
    /// optional at the call site; the post-check `fill_defaults` pass appends the default expression
    /// to any under-filled call, so the backends only ever see full-arity calls (byte-identity safe).
    /// Defaults must be trailing (`E-DEFAULT-PARAM-ORDER`). **Boxed** so the rare-and-large default
    /// expression does not bloat every `Param` (which is embedded in `ClassMember::Hook`).
    pub default: Option<Box<Expr>>,
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
    /// Let-destructuring (Phase 1 slice 5): `var Point { x, y } = p;` (struct, irrefutable) or
    /// `var [a, b] = xs else { … };` (list, refutable). The binders enter the **enclosing** scope (a
    /// binding statement, not a nested block), so they are live for the rest of the block. `else_block`
    /// is present only for the refutable list form and must diverge (Swift `guard let` model — checked
    /// via the totality engine); a present `else` on an irrefutable pattern is a compile error. No new
    /// `Op`/`Value`: the struct form lowers to field reads, the list form to a length-check + indexed
    /// reads (the same ops as an `if`).
    Destructure {
        pat: DestructurePat,
        init: Expr,
        else_block: Option<Vec<Stmt>>,
        span: Span,
    },
}

/// The target of a [`Stmt::Destructure`] (Phase 1 slice 5). A dedicated, flat (no nested sub-patterns)
/// representation — deliberately *not* the match [`Pattern`] enum: a list target is not a match pattern
/// (adding `Pattern::List` would force match-side handling + exhaustiveness), and let-destructuring
/// needs eager binding into the enclosing scope, which the lazy match-binding path does not model.
#[derive(Debug, Clone, PartialEq)]
pub enum DestructurePat {
    /// `Point { x, y }` / `Point { x: px }` — `type_name` is a concrete class; each field binds (with
    /// optional rename). Irrefutable: the init's static type must be assignable to `type_name`.
    Struct {
        type_name: String,
        fields: Vec<DestructureField>,
        span: Span,
    },
    /// `[a, b]` — bind list elements positionally. Refutable on a `List<T>` (mandatory diverging
    /// `else`), irrefutable on a length-matching `[T; N]`.
    List {
        binders: Vec<(String, Span)>,
        span: Span,
    },
}

/// One `field` / `field: binding` entry of a struct [`DestructurePat`] (Phase 1 slice 5). Shorthand
/// `Point { x }` fills `binding` with `field`; rename `Point { x: px }` sets `binding = "px"`.
#[derive(Debug, Clone, PartialEq)]
pub struct DestructureField {
    pub field: String,
    pub binding: String,
    pub span: Span,
}

impl DestructurePat {
    /// The variable names this pattern binds, each with its span, in source order. Used by every pass
    /// that introduces the binders into scope (free-var analysis, checker, casing).
    pub fn binders(&self) -> Vec<(String, Span)> {
        match self {
            DestructurePat::Struct { fields, .. } => {
                fields.iter().map(|f| (f.binding.clone(), f.span)).collect()
            }
            DestructurePat::List { binders, .. } => binders.clone(),
        }
    }

    pub fn span(&self) -> Span {
        match self {
            DestructurePat::Struct { span, .. } | DestructurePat::List { span, .. } => *span,
        }
    }
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
        /// Modifiers on the `constructor` keyword itself — its *own* visibility
        /// (`private`/`protected`/`public`), distinct from the per-param promotion modifiers in
        /// `params`. Enforced at the construction site (`E-CTOR-VISIBILITY`); non-visibility
        /// modifiers here are rejected (`E-CTOR-MODIFIER`). Previously parsed and dropped.
        modifiers: Vec<Modifier>,
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
    /// Traits this class composes via `use T;` (M-RT S8). Each names a `trait` whose members are
    /// flattened into this class (methods registered for dispatch, fields/const/static/hooks/ctor
    /// folded in) **before any backend runs** — a trait is reuse, not a supertype, so it never enters
    /// the `instanceof`/subtype tables. Trait-vs-trait collisions reuse the same `resolutions` clauses
    /// as multi-parent collisions (a clause's "parent" may name a `use`d trait). The transpiler emits a
    /// native PHP `trait`/`use`. Empty for a class that composes no traits.
    pub uses: Vec<UseTrait>,
    pub members: Vec<ClassMember>,
    pub span: Span,
}

/// A `use T;` trait-composition clause in a class body (M-RT S8) — see [`ClassDecl::uses`]. Named by
/// the trait's bare name (`package Main`-only this slice). Distinguished at parse time from an S6b
/// resolution clause (`use P.m`) by dot-lookahead: a `.` after the name is a resolution clause, a
/// `,`/`;` is trait composition.
#[derive(Debug, Clone, PartialEq)]
pub struct UseTrait {
    pub name: String,
    pub span: Span,
}

/// A trait declaration (`trait T { members }`, M-RT S8) — horizontal code reuse that is **not a type**
/// (a variable can never be typed `T`; `instanceof T` is rejected). Its members use the exact same
/// grammar as class members (methods with any visibility, instance fields with `mutable`/immutable,
/// `const`, `static`, property hooks, a constructor, and `abstract` requirements). A class composes a
/// trait with `use T;`; the trait's members are flattened into the using class before any backend, so
/// the interpreter/VM see ordinary class members. The transpiler emits a native PHP `trait`.
#[derive(Debug, Clone, PartialEq)]
pub struct TraitDecl {
    pub name: String,
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
    /// `trait T { members }` — horizontal reuse composed by a class via `use T;` (M-RT S8). Not a type.
    Trait(TraitDecl),
    /// `type Name = Type;` — a compile-time alias, erased after checking (resolved by the checker
    /// and expanded out of the AST before any backend runs).
    TypeAlias {
        name: String,
        ty: Type,
        span: Span,
    },
    /// `test "name" { stmts }` — a unit test (M-Test T1). `test` is a *contextual* keyword (special
    /// only at item position when immediately followed by a string literal), so it stays usable as an
    /// identifier elsewhere. The body is checked like a `-> void` function body with no `this`. A test
    /// item is valid only under `phg test` (test mode); in a normal build the checker rejects it as
    /// `E-TEST-OUTSIDE-TESTS`. It is never reached by a backend in a normal compile — the `phg test`
    /// runner executes test bodies directly on the interpreter (M-Test T3).
    Test {
        name: String,
        body: Vec<Stmt>,
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
mod tests;
