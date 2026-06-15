//! Phorge → PHP transpiler. Walks the untyped AST (the same AST the evaluator walks)
//! and emits runnable PHP 8.x source. Entry point: [`emit`].
use crate::ast::*;
use std::collections::{HashMap, HashSet};

/// Transpile a parsed program to PHP source. Returns the PHP text, or a
/// `transpile error: …` message for an unsupported construct.
pub fn emit(program: &Program) -> Result<String, String> {
    let mut t = Transpiler::new();
    t.collect(program);
    t.emit_program(program)?;
    Ok(t.out)
}

// `#[allow(dead_code)]` is temporary scaffolding: the full state is declared up front but
// only `out` is consumed in Task 1. Removed in Task 2 once functions/stmts/exprs land.
#[allow(dead_code)]
struct Transpiler {
    funcs: HashSet<String>,
    classes: HashSet<String>,
    variants: HashSet<String>,
    variant_fields: HashMap<String, Vec<String>>,
    out: String,
    indent: usize,
    locals: Vec<HashSet<String>>,
    cur_class_fields: Option<HashSet<String>>,
}

impl Transpiler {
    fn new() -> Self {
        Transpiler {
            funcs: HashSet::new(),
            classes: HashSet::new(),
            variants: HashSet::new(),
            variant_fields: HashMap::new(),
            out: String::new(),
            indent: 0,
            locals: Vec::new(),
            cur_class_fields: None,
        }
    }

    /// Pass 1 — index top-level names so call dispatch and match binding can resolve them.
    fn collect(&mut self, program: &Program) {
        for item in &program.items {
            match item {
                Item::Function(f) => {
                    self.funcs.insert(f.name.clone());
                }
                Item::Class(c) => {
                    self.classes.insert(c.name.clone());
                }
                Item::Enum(e) => {
                    for v in &e.variants {
                        self.variants.insert(v.name.clone());
                        self.variant_fields
                            .insert(v.name.clone(), v.fields.iter().map(|p| p.name.clone()).collect());
                    }
                }
                Item::Import { .. } => {}
            }
        }
    }

    fn emit_program(&mut self, _program: &Program) -> Result<(), String> {
        self.out.push_str("<?php\n");
        Ok(())
    }

    /// Indentation-aware line writer (consumed from Task 2 onward).
    #[allow(dead_code)]
    fn line(&mut self, s: &str) {
        for _ in 0..self.indent {
            self.out.push_str("    ");
        }
        self.out.push_str(s);
        self.out.push('\n');
    }
}

#[cfg(test)]
mod tests {
    use super::emit;
    use crate::lexer::lex;
    use crate::parser::Parser;

    fn php(src: &str) -> String {
        let tokens = lex(src).expect("lex");
        let prog = Parser::new(tokens).parse_program().expect("parse");
        emit(&prog).expect("emit")
    }

    #[test]
    fn empty_program_emits_php_open_tag() {
        assert_eq!(php(""), "<?php\n");
    }
}
