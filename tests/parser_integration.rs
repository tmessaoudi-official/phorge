use phorj::ast::{Expr, Pattern};
use phorj::lexer::lex;
use phorj::parser::Parser;

fn parse_expr(src: &str) -> Expr {
    let tokens = lex(src).expect("lex ok");
    let mut p = Parser::new(tokens);
    p.parse_expr().expect("parse ok")
}

#[test]
fn parses_spec_sample_match_body() {
    // The body of `area(Shape s)` from the spec's sample program.
    let src = "match s { Circle(r) => 3.14159 * r * r, Rect(w, h) => w * h, }";
    match parse_expr(src) {
        Expr::Match {
            scrutinee, arms, ..
        } => {
            assert!(matches!(*scrutinee, Expr::Ident(ref n, _) if n == "s"));
            assert_eq!(arms.len(), 2);
            // first arm: Circle(r) => 3.14159 * r * r
            match &arms[0].pattern {
                Pattern::Variant { name, fields, .. } => {
                    assert_eq!(name, "Circle");
                    assert_eq!(fields.len(), 1);
                }
                other => panic!("arm 0 pattern: {other:?}"),
            }
            assert!(matches!(arms[0].body, Expr::Binary { .. }));
            // second arm: Rect(w, h) => w * h
            match &arms[1].pattern {
                Pattern::Variant { name, fields, .. } => {
                    assert_eq!(name, "Rect");
                    assert_eq!(fields.len(), 2);
                }
                other => panic!("arm 1 pattern: {other:?}"),
            }
        }
        other => panic!("expected Match, got {other:?}"),
    }
}

#[test]
fn parses_interpolated_call_string() {
    // from the sample's loop body: "area = {area(s)}"
    match parse_expr("\"area = {area(s)}\"") {
        Expr::Str(parts, _) => assert_eq!(parts.len(), 2),
        other => panic!("expected Str, got {other:?}"),
    }
}
