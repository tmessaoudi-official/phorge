use phorj::lexer::lex;
use phorj::token::TokenKind;

#[test]
fn tokenizes_sample_without_error() {
    let src = std::fs::read_to_string("tests/fixtures/sample.phg").unwrap();
    let toks = lex(&src).expect("sample must lex cleanly");
    // last token is always Eof
    assert!(matches!(toks.last().unwrap().kind, TokenKind::Eof));
    // sanity: contains the function keyword and the fat-arrow match syntax
    assert!(toks.iter().any(|t| t.kind == TokenKind::Function));
    assert!(toks.iter().any(|t| t.kind == TokenKind::FatArrow));
    assert!(toks.iter().any(|t| t.kind == TokenKind::Match));
}
