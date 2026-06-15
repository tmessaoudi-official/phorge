use crate::token::Span;

/// Type annotations (e.g. `int`, `List<Shape>`, `T?`).
#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    /// `int`, `List<Shape>`, `Map<string, int>` — `args` empty for non-generic.
    Named { name: String, args: Vec<Type>, span: Span },
    /// `T?`
    Optional { inner: Box<Type>, span: Span },
}

/// Patterns in `match` arms.
#[derive(Debug, Clone, PartialEq)]
pub enum Pattern {
    /// `_`
    Wildcard(Span),
    /// bare identifier — binds the scrutinee (catch-all)
    Binding { name: String, span: Span },
    Int(i64, Span),
    Float(f64, Span),
    Str(String, Span),
    Bool(bool, Span),
    Null(Span),
    /// `Circle(r)`, `Rect(w, h)` — destructure an enum variant
    Variant { name: String, fields: Vec<Pattern>, span: Span },
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
    /// `object.name`
    Member {
        object: Box<Expr>,
        name: String,
        span: Span,
    },
    /// `object[index]`
    Index {
        object: Box<Expr>,
        index: Box<Expr>,
        span: Span,
    },
    Match {
        scrutinee: Box<Expr>,
        arms: Vec<MatchArm>,
        span: Span,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::token::Span;

    fn sp() -> Span {
        Span { start: 0, len: 1, line: 1, col: 1 }
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
            fields: vec![Pattern::Binding { name: "r".into(), span: sp() }],
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
}
