//! Shared parser-test helpers (M-Decomp W3.1b). Re-exports parser internals so each
//! by-construct test file needs only `use super::support::*;`.

pub(super) use super::super::*;
pub(super) use crate::ast::{
    ClassMember, Expr, Item, Modifier, Pattern, Stmt, StrPart, Type, Visibility,
};
pub(super) use crate::lexer::lex;

/// Helper: lex `src` and build a parser over the tokens.
pub(super) fn parser(src: &str) -> Parser {
    Parser::new(lex(src).expect("lex ok"))
}

/// Helper: parse a whole program, panicking on a parse error.
pub(super) fn prog(src: &str) -> Program {
    parser(src).parse_program().expect("parse ok")
}

/// Helper: parse a whole program expecting a parse error, returning its rendered message.
pub(super) fn prog_err(src: &str) -> String {
    parser(src).parse_program().unwrap_err().render(src)
}

/// Helper: parse `src` as a single expression.
pub(super) fn expr(src: &str) -> Expr {
    parser(src).parse_expr().expect("parse ok")
}

pub(super) fn ty(src: &str) -> Type {
    parser(src).parse_type().expect("parse ok")
}

pub(super) fn pat(src: &str) -> Pattern {
    parser(src).parse_pattern().expect("parse ok")
}

/// Helper: parse `src` as a single statement.
pub(super) fn stmt(src: &str) -> Stmt {
    parser(src).parse_stmt().expect("parse ok")
}

/// Helper: parse `src` as a top-level item.
pub(super) fn item(src: &str) -> Item {
    parser(src).parse_item().expect("parse ok")
}

/// Render an expression to a fully-parenthesized string so precedence is visible.
pub(super) fn sexpr(e: &Expr) -> String {
    match e {
        Expr::Int(n, _) => n.to_string(),
        Expr::Float(f, _) => format!("{f}"),
        Expr::Bool(b, _) => b.to_string(),
        Expr::Null(_) => "null".into(),
        Expr::Ident(s, _) => s.clone(),
        Expr::This(_) => "this".into(),
        Expr::Unary { op, expr, .. } => {
            let o = match op {
                UnaryOp::Neg => "-",
                UnaryOp::Not => "!",
                UnaryOp::BitNot => "~",
            };
            format!("({o} {})", sexpr(expr))
        }
        Expr::Binary { op, lhs, rhs, .. } => {
            let o = match op {
                BinaryOp::Add => "+",
                BinaryOp::Sub => "-",
                BinaryOp::Mul => "*",
                BinaryOp::Div => "/",
                BinaryOp::Rem => "%",
                BinaryOp::Eq => "==",
                BinaryOp::NotEq => "!=",
                BinaryOp::Lt => "<",
                BinaryOp::Gt => ">",
                BinaryOp::Le => "<=",
                BinaryOp::Ge => ">=",
                BinaryOp::And => "&&",
                BinaryOp::Or => "||",
                BinaryOp::Pipe => "|>",
                BinaryOp::Coalesce => "??",
                BinaryOp::BitAnd => "&",
                BinaryOp::BitOr => "|",
                BinaryOp::BitXor => "^",
                BinaryOp::Shl => "<<",
                BinaryOp::Shr => ">>",
            };
            format!("({o} {} {})", sexpr(lhs), sexpr(rhs))
        }
        Expr::Member {
            object, name, safe, ..
        } => format!(
            "{}{}{}",
            sexpr(object),
            if *safe { "?." } else { "." },
            name
        ),
        Expr::Call { callee, args, .. } => {
            let a: Vec<String> = args.iter().map(sexpr).collect();
            format!("{}({})", sexpr(callee), a.join(", "))
        }
        Expr::Index { object, index, .. } => format!("{}[{}]", sexpr(object), sexpr(index)),
        Expr::Lambda { params, body, .. } => {
            let ps: Vec<&str> = params.iter().map(|p| p.name.as_str()).collect();
            let body_str = match body {
                LambdaBody::Expr(e) => sexpr(e),
                LambdaBody::Block(_) => "<block>".into(),
            };
            format!("(lambda ({}) {})", ps.join(" "), body_str)
        }
        Expr::InstanceOf {
            value, type_name, ..
        } => format!("(instanceof {} {type_name})", sexpr(value)),
        other => format!("{other:?}"),
    }
}
