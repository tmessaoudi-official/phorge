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
    /// `var` — placeholder for an inferred local binding type (resolved by the checker from the
    /// initializer, erased everywhere else). Only valid as a `Stmt::VarDecl` type.
    Infer(Span),
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
    Is,
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
    Final,
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
    /// `Type name = expr;`
    VarDecl {
        ty: Type,
        name: String,
        init: Expr,
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
    /// `{ .. }`
    Block(Vec<Stmt>, Span),
    /// `expr;`
    Expr(Expr, Span),
}

/// A function or method declaration. `modifiers` is empty for a free (top-level) function.
#[derive(Debug, Clone, PartialEq)]
pub struct FunctionDecl {
    pub modifiers: Vec<Modifier>,
    pub name: String,
    pub params: Vec<Param>,
    pub ret: Option<Type>,
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
    pub name: String,
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
        span: Span,
    },
    Constructor {
        params: Vec<CtorParam>,
        body: Vec<Stmt>,
        span: Span,
    },
    Method(FunctionDecl),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClassDecl {
    pub name: String,
    pub members: Vec<ClassMember>,
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
        span: Span,
    },
    Function(FunctionDecl),
    Enum(EnumDecl),
    Class(ClassDecl),
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
    /// The file's package path (`package app.util;` ⇒ `["app", "util"]`). Empty only for a
    /// malformed file with no declaration — the checker rejects that as `E-NO-PACKAGE` (M5: every
    /// file is packaged, never inferred). The reserved `["main"]` is the runnable entry (M5 S1).
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
            name: "area".into(),
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
}
