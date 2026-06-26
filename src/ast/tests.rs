use super::walk::collect_free_stmt;
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
            default: None,
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
        default: None,
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
