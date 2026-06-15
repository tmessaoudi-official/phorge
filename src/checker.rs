//! Sound static type-checker. Two sub-phases: `collect` (hoist decls + prelude),
//! then `check` (walk bodies). Returns all type errors at once.

use std::collections::HashMap;

use crate::ast::Program;
use crate::token::Span;
use crate::types::Ty;

/// A type error with source position. Mirrors `parser::ParseError`.
#[derive(Debug, Clone, PartialEq)]
pub struct TypeError {
    pub message: String,
    pub line: u32,
    pub col: u32,
}

struct FnSig {
    params: Vec<Ty>,
    ret: Ty,
}

struct EnumInfo {
    /// variant name -> field types (in declaration order)
    variants: HashMap<String, Vec<Ty>>,
}

struct ClassInfo {
    fields: HashMap<String, Ty>,
    methods: HashMap<String, FnSig>,
    /// constructor parameter types, for `ClassName(args)` calls
    ctor: Vec<Ty>,
}

pub struct Checker {
    funcs: HashMap<String, FnSig>,
    enums: HashMap<String, EnumInfo>,
    classes: HashMap<String, ClassInfo>,
    /// lexical block scopes; last is innermost
    scopes: Vec<HashMap<String, Ty>>,
    errors: Vec<TypeError>,
    /// return type of the function/method currently being checked
    cur_ret: Ty,
    /// class currently being checked (for `this` and bare field refs)
    cur_class: Option<String>,
}

impl Checker {
    fn new() -> Self {
        Checker {
            funcs: HashMap::new(),
            enums: HashMap::new(),
            classes: HashMap::new(),
            scopes: Vec::new(),
            errors: Vec::new(),
            cur_ret: Ty::Unit,
            cur_class: None,
        }
    }

    /// Record an error and return the poison type so callers can keep going.
    fn err(&mut self, span: Span, msg: impl Into<String>) -> Ty {
        self.errors.push(TypeError {
            message: msg.into(),
            line: span.line,
            col: span.col,
        });
        Ty::Error
    }

    /// Resolve an AST type annotation to an internal `Ty`. Records and poisons on
    /// unknown / deferred types.
    fn resolve_type(&mut self, ty: &crate::ast::Type) -> Ty {
        use crate::ast::Type;
        match ty {
            Type::Optional { span, .. } => {
                self.err(*span, "optional types are not yet supported in M1")
            }
            Type::Named { name, args, span } => match name.as_str() {
                "int" => self.no_args(name, args, *span, Ty::Int),
                "float" => self.no_args(name, args, *span, Ty::Float),
                "bool" => self.no_args(name, args, *span, Ty::Bool),
                "string" => self.no_args(name, args, *span, Ty::String),
                "List" => Ty::List(Box::new(self.one_arg(name, args, *span))),
                "Set" => Ty::Set(Box::new(self.one_arg(name, args, *span))),
                "Map" => {
                    if args.len() != 2 {
                        return self.err(*span, format!("Map expects 2 type arguments, got {}", args.len()));
                    }
                    let k = self.resolve_type(&args[0]);
                    let v = self.resolve_type(&args[1]);
                    Ty::Map(Box::new(k), Box::new(v))
                }
                "decimal" | "double" | "i8" | "i16" | "i32" | "i64" | "u8" | "u16" | "u32"
                | "u64" => self.err(
                    *span,
                    format!("the numeric type `{name}` is not yet supported in M1"),
                ),
                other => {
                    if self.enums.contains_key(other) || self.classes.contains_key(other) {
                        Ty::Named(other.to_string())
                    } else {
                        self.err(*span, format!("unknown type `{other}`"))
                    }
                }
            },
        }
    }

    fn no_args(&mut self, name: &str, args: &[crate::ast::Type], span: Span, ty: Ty) -> Ty {
        if args.is_empty() {
            ty
        } else {
            self.err(span, format!("type `{name}` takes no type arguments"))
        }
    }

    fn one_arg(&mut self, name: &str, args: &[crate::ast::Type], span: Span) -> Ty {
        if args.len() != 1 {
            self.err(span, format!("{name} expects 1 type argument, got {}", args.len()));
            Ty::Error
        } else {
            self.resolve_type(&args[0])
        }
    }

    /// Register builtin functions available without explicit user definition.
    fn register_prelude(&mut self) {
        self.funcs.insert(
            "println".into(),
            FnSig { params: vec![Ty::String], ret: Ty::Unit },
        );
    }

    /// Phase 1 — hoist all top-level declarations and the builtin prelude.
    fn collect(&mut self, program: &Program) {
        self.register_prelude();
        // user decl collection added in Task 4 & Task 5.
        let _ = program;
    }

    /// Phase 2 — check every function/method body.
    fn check_program(&mut self, _program: &Program) {
        // Body walking added in Task 3 onward.
    }
}

/// Type-check a whole program. `Ok(())` means it is well-typed; otherwise every
/// detected error is returned.
pub fn check(program: &Program) -> Result<(), Vec<TypeError>> {
    let mut c = Checker::new();
    c.collect(program);
    c.check_program(program);
    if c.errors.is_empty() {
        Ok(())
    } else {
        Err(c.errors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::lex;
    use crate::parser::Parser;

    /// Lex + parse `src` into a Program, panicking on lex/parse failure (tests here
    /// only care about type-checking).
    fn prog(src: &str) -> Program {
        let tokens = lex(src).expect("lex ok");
        Parser::new(tokens).parse_program().expect("parse ok")
    }

    /// Type-check `src` and return the errors (empty == well-typed).
    fn errors_of(src: &str) -> Vec<TypeError> {
        match check(&prog(src)) {
            Ok(()) => Vec::new(),
            Err(e) => e,
        }
    }

    #[test]
    fn empty_program_checks_ok() {
        assert!(errors_of("").is_empty());
    }

    #[test]
    fn resolve_maps_primitives_and_list() {
        use crate::ast::Type;
        use crate::token::Span;
        let sp = Span { start: 0, len: 1, line: 1, col: 1 };
        let mut c = Checker::new();
        assert_eq!(c.resolve_type(&Type::Named { name: "int".into(), args: vec![], span: sp }), Ty::Int);
        let list = Type::Named {
            name: "List".into(),
            args: vec![Type::Named { name: "int".into(), args: vec![], span: sp }],
            span: sp,
        };
        assert_eq!(c.resolve_type(&list), Ty::List(Box::new(Ty::Int)));
        assert_eq!(c.errors.len(), 0);
    }
}
