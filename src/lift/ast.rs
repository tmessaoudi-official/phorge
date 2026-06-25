//! M-Lift L2 — the **PHP AST** (Tier-1 subset) produced by [`super::parser`].
//!
//! Deliberately kept close to PHP semantics, NOT pre-lifted: `array` stays [`PhpExpr::Array`]
//! (its List/Map/Set role is undecided here), `?T` stays [`PhpType::Nullable`]. The lossy mapping to
//! Phorge's typed world (`array` → `List`/`Map`/`Set`, `?T` → `T?`, `??`/`?->` → Phorge equivalents)
//! is **L4's** job (the lifter), not the parser's — separation of concerns keeps each stage honest.
//!
//! Tier boundary: anything outside this AST (closures, references, union types, casts, heredoc,
//! interpolated strings, `try`/`switch`/`namespace`/…) is rejected *loudly* by the parser rather than
//! represented and guessed at. Classes and enums land in L2b (added to [`PhpItem`] then).

/// A parsed PHP source file: a flat sequence of top-level items. PHP interleaves declarations with
/// file-level statements, so [`PhpItem::Stmt`] carries the latter.
#[derive(Debug, Clone, PartialEq)]
pub struct PhpProgram {
    pub items: Vec<PhpItem>,
}

/// A top-level item. L2a covers functions and file-level statements; `Class`/`Enum` arrive in L2b.
#[derive(Debug, Clone, PartialEq)]
pub enum PhpItem {
    Function(PhpFunction),
    Stmt(PhpStmt),
}

/// A typed top-level function: `function name(params): ret { body }`.
#[derive(Debug, Clone, PartialEq)]
pub struct PhpFunction {
    pub name: String,
    pub params: Vec<PhpParam>,
    /// Declared return type, if any (`: int`). `None` = no return hint.
    pub ret: Option<PhpType>,
    pub body: Vec<PhpStmt>,
    /// 1-based source line of the `function` keyword (for lift diagnostics).
    pub line: usize,
}

/// A function/method parameter. The leading `$` is stripped from `name`.
#[derive(Debug, Clone, PartialEq)]
pub struct PhpParam {
    /// Declared type hint, if any.
    pub ty: Option<PhpType>,
    pub name: String,
    /// Default value (`= expr`), if any. Tier-1: a literal or simple constant expression.
    pub default: Option<PhpExpr>,
}

/// A PHP type hint. Tier-1 = a single name or a nullable single name. Union types (`A|B`) can't even
/// be lexed (the lexer has no bare `|`), so they're excluded at the token level by construction.
#[derive(Debug, Clone, PartialEq)]
pub enum PhpType {
    /// `int`, `float`, `string`, `bool`, `void`, `array`, `mixed`, or a class/enum name.
    Named(String),
    /// `?T` — a nullable type.
    Nullable(Box<PhpType>),
}

/// A PHP statement.
#[derive(Debug, Clone, PartialEq)]
pub enum PhpStmt {
    /// `return;` or `return expr;`.
    Return(Option<PhpExpr>),
    /// An expression used for effect: `foo();`, `$x = 1;`, `$i++;`.
    Expr(PhpExpr),
    /// `if (cond) { then } elseif (c) { … } else { els }`. Bodies are statement lists (a single
    /// brace-less statement is parsed into a one-element list).
    If {
        cond: PhpExpr,
        then: Vec<PhpStmt>,
        /// Zero or more `elseif`/`else if` clauses, in source order.
        elifs: Vec<(PhpExpr, Vec<PhpStmt>)>,
        els: Option<Vec<PhpStmt>>,
    },
    /// `while (cond) { body }`.
    While {
        cond: PhpExpr,
        body: Vec<PhpStmt>,
    },
    /// `for (init; cond; step) { body }`. Each clause is optional (`for (;;)`).
    For {
        init: Option<PhpExpr>,
        cond: Option<PhpExpr>,
        step: Option<PhpExpr>,
        body: Vec<PhpStmt>,
    },
    /// `foreach ($array as $value)` or `foreach ($array as $key => $value)`. Names are `$`-stripped.
    Foreach {
        array: PhpExpr,
        key: Option<String>,
        value: String,
        body: Vec<PhpStmt>,
    },
    /// `echo a, b, c;`.
    Echo(Vec<PhpExpr>),
    Break,
    Continue,
    /// A brace block `{ … }` used as a statement.
    Block(Vec<PhpStmt>),
}

/// A PHP expression.
#[derive(Debug, Clone, PartialEq)]
pub enum PhpExpr {
    Int(i64),
    Float(f64),
    /// A safe (non-interpolating) string literal, escapes decoded.
    Str(String),
    Bool(bool),
    Null,
    /// `$name` — a variable (without the `$`). `$this` arrives as `Var("this")`.
    Var(String),
    /// A bare identifier: a global constant or a function name (when followed by `(`, postfix turns
    /// it into a [`PhpExpr::Call`]).
    Name(String),
    /// `[a, b, k => v]` (and the `array(…)` long form, which parses as a `Call` to `Name("array")`).
    Array(Vec<PhpArrayElem>),
    Unary {
        op: PhpUnOp,
        expr: Box<PhpExpr>,
    },
    Binary {
        op: PhpBinOp,
        left: Box<PhpExpr>,
        right: Box<PhpExpr>,
    },
    /// `target = value` (right-associative). `target` is a validated lvalue.
    Assign {
        target: Box<PhpExpr>,
        value: Box<PhpExpr>,
    },
    /// `target op= value` (`+=`, `.=`, `??=`, …). Kept distinct from `Assign` so it round-trips to
    /// Phorge's own compound assignment.
    CompoundAssign {
        target: Box<PhpExpr>,
        op: PhpBinOp,
        value: Box<PhpExpr>,
    },
    /// `++x` / `x++` / `--x` / `x--`.
    IncDec {
        target: Box<PhpExpr>,
        inc: bool,
        prefix: bool,
    },
    /// `cond ? then : els`. `then == None` encodes the elvis form `cond ?: els`.
    Ternary {
        cond: Box<PhpExpr>,
        then: Option<Box<PhpExpr>>,
        els: Box<PhpExpr>,
    },
    /// `callee(args)` — `callee` is typically a `Name` (free function) but may be any expression.
    Call {
        callee: Box<PhpExpr>,
        args: Vec<PhpExpr>,
    },
    /// `recv->name(args)` / `recv?->name(args)`.
    MethodCall {
        recv: Box<PhpExpr>,
        name: String,
        args: Vec<PhpExpr>,
        nullsafe: bool,
    },
    /// `recv->name` / `recv?->name` (property access, no call).
    Member {
        recv: Box<PhpExpr>,
        name: String,
        nullsafe: bool,
    },
    /// `Class::method(args)`.
    StaticCall {
        class: String,
        name: String,
        args: Vec<PhpExpr>,
    },
    /// `Class::CONST`.
    ClassConst {
        class: String,
        name: String,
    },
    /// `Class::$prop`.
    StaticProp {
        class: String,
        name: String,
    },
    /// `base[index]`.
    Index {
        base: Box<PhpExpr>,
        index: Box<PhpExpr>,
    },
    /// `new Class(args)` / `new Class`.
    New {
        class: String,
        args: Vec<PhpExpr>,
    },
    /// `match (subject) { conds => body, …, default => body }`.
    Match {
        subject: Box<PhpExpr>,
        arms: Vec<PhpMatchArm>,
    },
}

/// One element of an array literal: `value` or `key => value`.
#[derive(Debug, Clone, PartialEq)]
pub struct PhpArrayElem {
    pub key: Option<PhpExpr>,
    pub value: PhpExpr,
}

/// One arm of a `match`: `conds => body`, where `conds == None` is the `default` arm.
#[derive(Debug, Clone, PartialEq)]
pub struct PhpMatchArm {
    /// `None` = the `default` arm; `Some(list)` = one or more comma-separated conditions.
    pub conds: Option<Vec<PhpExpr>>,
    pub body: PhpExpr,
}

/// Binary operators (Tier-1 subset).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhpBinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    /// String concatenation `.`.
    Concat,
    /// Loose equality `==`.
    Eq,
    /// Strict equality `===`.
    Identical,
    /// Loose inequality `!=`.
    NotEq,
    /// Strict inequality `!==`.
    NotIdentical,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
    /// Null-coalesce `??`.
    Coalesce,
}

/// Prefix unary operators (Tier-1 subset).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhpUnOp {
    /// `!`.
    Not,
    /// `-` (negation).
    Neg,
}
