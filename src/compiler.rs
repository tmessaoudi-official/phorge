//! AST → bytecode compiler (M2 P2). A dedicated pass over the type-checked AST,
//! emitting a `Chunk` the VM executes. Mirrors the tree-walker's semantics so
//! `runvm` output is byte-identical to `run` (the differential oracle).
//!
//! P2 scope: `main`-only programs — literals, arithmetic, comparison, logical
//! short-circuit, unary, interpolation, `println`, list literals, locals, `if`/`else`,
//! `for…in`, blocks. User calls (P3), classes/enums/`match`/`this`/member (P4) raise a
//! clean compile error until implemented. Lists are inline `Value::List` in P2; they
//! migrate to the arena heap at P4.

use crate::ast::{BinaryOp, Expr, FunctionDecl, Item, Program, Stmt, StrPart, Type, UnaryOp};
use crate::chunk::{BytecodeProgram, Chunk, Function, Op};
use crate::value::Value;
use std::collections::HashMap;

/// Numeric operand kind, inferred just enough to pick int- vs float-specialized
/// arithmetic ops (decision P2-6).
#[derive(Clone, Copy, PartialEq)]
enum NumTy {
    Int,
    Float,
}

/// A declared local: its name, the declared type name (for `num_ty`), and the lexical
/// depth it lives at (for scope cleanup). Its stack slot is its index in `locals`.
struct Local {
    name: String,
    ty: String,
    depth: u32,
}

/// Per-function metadata gathered in the pre-pass: its index in `BytecodeProgram.functions`
/// and its declared return-type name (for `num_ty` of a call result — decision P3-6).
struct FnMeta {
    index: usize,
    ret: String,
}

struct Compiler<'a> {
    chunk: Chunk,
    locals: Vec<Local>,
    scope_depth: u32,
    fns: &'a HashMap<String, FnMeta>,
}

/// Compile a whole program: a pre-pass indexes every top-level function (so calls — including
/// forward references and recursion — resolve to a static index), then each function body is
/// compiled into its own `Chunk`. Parameters occupy slots `0..arity` at the base of the frame
/// window; every function ends with an implicit `Unit` return (P3-7).
pub fn compile(program: &Program) -> Result<BytecodeProgram, String> {
    let mut order: Vec<&FunctionDecl> = Vec::new();
    let mut fns: HashMap<String, FnMeta> = HashMap::new();
    for it in &program.items {
        if let Item::Function(f) = it {
            fns.insert(
                f.name.clone(),
                FnMeta {
                    index: order.len(),
                    ret: f.ret.as_ref().map_or_else(|| "unit".to_string(), type_name),
                },
            );
            order.push(f);
        }
    }
    let main = fns
        .get("main")
        .map(|m| m.index)
        .ok_or_else(|| "no `main` function".to_string())?;

    let mut functions = Vec::with_capacity(order.len());
    for f in &order {
        let mut c = Compiler { chunk: Chunk::new(), locals: Vec::new(), scope_depth: 0, fns: &fns };
        for p in &f.params {
            c.add_local(&p.name, &type_name(&p.ty));
        }
        let last_line = f.span.line;
        for s in &f.body {
            c.stmt(s)?;
        }
        c.emit_const(Value::Unit, last_line);
        c.emit(Op::Return, last_line);
        functions.push(Function { name: f.name.clone(), arity: f.params.len(), chunk: c.chunk });
    }
    Ok(BytecodeProgram { functions, main })
}

/// The declared type name of a `Type` annotation (the head identifier).
fn type_name(ty: &Type) -> String {
    match ty {
        Type::Named { name, .. } => name.clone(),
        Type::Optional { .. } => "optional".to_string(),
    }
}

impl<'a> Compiler<'a> {
    fn emit(&mut self, op: Op, line: u32) {
        self.chunk.emit(op, line);
    }

    fn emit_const(&mut self, v: Value, line: u32) {
        let k = self.chunk.add_const(v);
        self.emit(Op::Const(k), line);
    }

    fn here(&self) -> usize {
        self.chunk.code.len()
    }

    /// Emit a jump placeholder (target 0); returns its code index for `patch_jump`.
    fn emit_jump(&mut self, op: Op, line: u32) -> usize {
        let idx = self.here();
        self.emit(op, line);
        idx
    }

    /// Patch a previously-emitted forward jump to point at the current code position.
    fn patch_jump(&mut self, idx: usize) {
        let target = self.here();
        self.chunk.code[idx] = match self.chunk.code[idx] {
            Op::Jump(_) => Op::Jump(target),
            Op::JumpIfFalse(_) => Op::JumpIfFalse(target),
            ref other => unreachable!("patch_jump on {other:?}"),
        };
    }

    fn begin_scope(&mut self) {
        self.scope_depth += 1;
    }

    fn end_scope(&mut self, line: u32) {
        self.scope_depth -= 1;
        while matches!(self.locals.last(), Some(l) if l.depth > self.scope_depth) {
            self.emit(Op::Pop, line);
            self.locals.pop();
        }
    }

    fn add_local(&mut self, name: &str, ty: &str) -> usize {
        self.locals.push(Local {
            name: name.to_string(),
            ty: ty.to_string(),
            depth: self.scope_depth,
        });
        self.locals.len() - 1
    }

    fn resolve_local(&self, name: &str) -> Option<usize> {
        self.locals.iter().rposition(|l| l.name == name)
    }

    /// Infer whether an arithmetic expression is int- or float-typed (decision P2-6).
    /// Only reached for operands of `+ - * / %`, which the checker guarantees are numeric.
    fn num_ty(&self, e: &Expr) -> Result<NumTy, String> {
        match e {
            Expr::Int(..) => Ok(NumTy::Int),
            Expr::Float(..) => Ok(NumTy::Float),
            Expr::Ident(name, _) => {
                let l = self
                    .resolve_local(name)
                    .map(|s| self.locals[s].ty.as_str())
                    .ok_or_else(|| format!("undefined variable `{name}`"))?;
                match l {
                    "int" => Ok(NumTy::Int),
                    "float" => Ok(NumTy::Float),
                    other => Err(format!("`{name}` is `{other}`, not numeric")),
                }
            }
            Expr::Unary { expr, .. } => self.num_ty(expr),
            Expr::Binary { lhs, .. } => self.num_ty(lhs),
            Expr::Call { callee, .. } => {
                if let Expr::Ident(name, _) = &**callee {
                    if let Some(meta) = self.fns.get(name) {
                        return match meta.ret.as_str() {
                            "int" => Ok(NumTy::Int),
                            "float" => Ok(NumTy::Float),
                            other => Err(format!("`{name}` returns `{other}`, not numeric")),
                        };
                    }
                }
                Err(format!("cannot infer numeric type of {e:?}"))
            }
            other => Err(format!("cannot infer numeric type of {other:?}")),
        }
    }

    fn stmt(&mut self, s: &Stmt) -> Result<(), String> {
        match s {
            Stmt::VarDecl { ty, name, init, .. } => {
                self.expr(init)?; // value stays on the stack as the new local's slot
                let tyname = type_name(ty);
                self.add_local(name, &tyname);
                Ok(())
            }
            Stmt::Expr(e, span) => {
                self.expr(e)?;
                self.emit(Op::Pop, span.line);
                Ok(())
            }
            Stmt::Return { value, span } => {
                match value {
                    Some(e) => self.expr(e)?,
                    None => self.emit_const(Value::Unit, span.line),
                }
                self.emit(Op::Return, span.line);
                Ok(())
            }
            Stmt::Block(stmts, span) => {
                self.begin_scope();
                for st in stmts {
                    self.stmt(st)?;
                }
                self.end_scope(span.line);
                Ok(())
            }
            Stmt::If { cond, then_block, else_block, span } => {
                self.compile_if(cond, then_block, else_block.as_deref(), span.line)
            }
            Stmt::For { ty, name, iter, body, span } => {
                let elem_ty = type_name(ty);
                self.compile_for(name, &elem_ty, iter, body, span.line)
            }
        }
    }

    fn expr(&mut self, e: &Expr) -> Result<(), String> {
        match e {
            Expr::Int(n, sp) => self.emit_const(Value::Int(*n), sp.line),
            Expr::Float(x, sp) => self.emit_const(Value::Float(*x), sp.line),
            Expr::Bool(b, sp) => self.emit_const(Value::Bool(*b), sp.line),
            Expr::Str(parts, sp) => self.compile_str(parts, sp.line)?,
            Expr::Ident(name, sp) => {
                let slot = self
                    .resolve_local(name)
                    .ok_or_else(|| format!("undefined variable `{name}`"))?;
                self.emit(Op::GetLocal(slot), sp.line);
            }
            Expr::List(items, sp) => {
                for it in items {
                    self.expr(it)?;
                }
                self.emit(Op::MakeList(items.len()), sp.line);
            }
            Expr::Unary { op, expr, span } => {
                self.expr(expr)?;
                match op {
                    UnaryOp::Neg => self.emit(Op::Neg, span.line),
                    UnaryOp::Not => self.emit(Op::Not, span.line),
                }
            }
            Expr::Binary { op, lhs, rhs, span } => self.compile_binary(*op, lhs, rhs, span.line)?,
            Expr::Call { callee, args, span } => self.compile_call(callee, args, span.line)?,
            Expr::Null(_) => return Err("null is not supported (M1 surface)".into()),
            Expr::This(_) => {
                return Err("`this` is not supported by the VM compiler yet (M2 P4)".into())
            }
            Expr::Member { .. } => {
                return Err("member access is not supported by the VM compiler yet (M2 P4)".into())
            }
            Expr::Index { .. } => return Err("indexing is not supported (M1 surface)".into()),
            Expr::Match { .. } => {
                return Err("`match` is not supported by the VM compiler yet (M2 P4)".into())
            }
        }
        Ok(())
    }

    fn compile_str(&mut self, parts: &[StrPart], line: u32) -> Result<(), String> {
        // A single literal segment (or empty) is just a string constant.
        if let [StrPart::Literal(s)] = parts {
            self.emit_const(Value::Str(s.clone()), line);
            return Ok(());
        }
        if parts.is_empty() {
            self.emit_const(Value::Str(String::new()), line);
            return Ok(());
        }
        for part in parts {
            match part {
                StrPart::Literal(s) => self.emit_const(Value::Str(s.clone()), line),
                StrPart::Expr(e) => self.expr(e)?,
            }
        }
        self.emit(Op::Concat(parts.len()), line);
        Ok(())
    }

    fn compile_binary(
        &mut self,
        op: BinaryOp,
        lhs: &Expr,
        rhs: &Expr,
        line: u32,
    ) -> Result<(), String> {
        use BinaryOp::*;
        // Short-circuit logical ops desugar to jumps (decision P2-5).
        match op {
            And => {
                self.expr(lhs)?;
                let l_false = self.emit_jump(Op::JumpIfFalse(0), line);
                self.expr(rhs)?;
                let l_end = self.emit_jump(Op::Jump(0), line);
                self.patch_jump(l_false);
                self.emit_const(Value::Bool(false), line);
                self.patch_jump(l_end);
                return Ok(());
            }
            Or => {
                self.expr(lhs)?;
                let l_rhs = self.emit_jump(Op::JumpIfFalse(0), line);
                self.emit_const(Value::Bool(true), line);
                let l_end = self.emit_jump(Op::Jump(0), line);
                self.patch_jump(l_rhs);
                self.expr(rhs)?;
                self.patch_jump(l_end);
                return Ok(());
            }
            _ => {}
        }
        // Strict ops: evaluate both, then emit.
        match op {
            Add | Sub | Mul | Div | Rem => {
                let nt = self.num_ty(lhs)?;
                self.expr(lhs)?;
                self.expr(rhs)?;
                let emit = match (op, nt) {
                    (Add, NumTy::Int) => Op::AddI,
                    (Add, NumTy::Float) => Op::AddF,
                    (Sub, NumTy::Int) => Op::SubI,
                    (Sub, NumTy::Float) => Op::SubF,
                    (Mul, NumTy::Int) => Op::MulI,
                    (Mul, NumTy::Float) => Op::MulF,
                    (Div, NumTy::Int) => Op::DivI,
                    (Div, NumTy::Float) => Op::DivF,
                    (Rem, NumTy::Int) => Op::RemI,
                    (Rem, NumTy::Float) => Op::RemF,
                    _ => unreachable!("arithmetic op set"),
                };
                self.emit(emit, line);
            }
            Eq | Is => {
                self.expr(lhs)?;
                self.expr(rhs)?;
                self.emit(Op::Eq, line);
            }
            NotEq => {
                self.expr(lhs)?;
                self.expr(rhs)?;
                self.emit(Op::Ne, line);
            }
            Lt | Gt | Le | Ge => {
                self.expr(lhs)?;
                self.expr(rhs)?;
                self.emit(
                    match op {
                        Lt => Op::Lt,
                        Gt => Op::Gt,
                        Le => Op::Le,
                        Ge => Op::Ge,
                        _ => unreachable!(),
                    },
                    line,
                );
            }
            Pipe => return Err("the `|>` pipe operator is not supported (M1 surface)".into()),
            And | Or => unreachable!("handled above"),
        }
        Ok(())
    }

    fn compile_call(&mut self, callee: &Expr, args: &[Expr], line: u32) -> Result<(), String> {
        if let Expr::Ident(name, _) = callee {
            if name == "println" {
                for a in args {
                    self.expr(a)?;
                }
                self.emit(Op::Print(args.len()), line);
                // `println` evaluates to `Unit` (interpreter parity): leave a value so the
                // enclosing expression-statement's `Pop` is balanced.
                self.emit_const(Value::Unit, line);
                return Ok(());
            }
            if let Some(meta) = self.fns.get(name) {
                for a in args {
                    self.expr(a)?;
                }
                self.emit(Op::Call(meta.index), line);
                return Ok(());
            }
            // A non-function, non-`println` identifier call is an enum variant or class
            // constructor — those land at P4.
            return Err(format!(
                "calling `{name}` is not supported by the VM compiler yet (M2 P4)"
            ));
        }
        Err("method calls are not supported by the VM compiler yet (M2 P4)".into())
    }

    fn compile_if(
        &mut self,
        cond: &Expr,
        then_block: &[Stmt],
        else_block: Option<&[Stmt]>,
        line: u32,
    ) -> Result<(), String> {
        self.expr(cond)?;
        let else_jump = self.emit_jump(Op::JumpIfFalse(0), line); // pops cond
        self.begin_scope();
        for s in then_block {
            self.stmt(s)?;
        }
        self.end_scope(line);
        let end_jump = self.emit_jump(Op::Jump(0), line);
        self.patch_jump(else_jump);
        if let Some(eb) = else_block {
            self.begin_scope();
            for s in eb {
                self.stmt(s)?;
            }
            self.end_scope(line);
        }
        self.patch_jump(end_jump);
        Ok(())
    }

    /// `for (T name in iter)` desugars to a counter loop over an inline list
    /// (decision P2-7). Hidden locals `$for_list` and `$for_idx` bracket `name`.
    fn compile_for(
        &mut self,
        name: &str,
        elem_ty: &str,
        iter: &Expr,
        body: &[Stmt],
        line: u32,
    ) -> Result<(), String> {
        self.begin_scope();
        self.expr(iter)?; // [list]
        let s_list = self.add_local("$for_list", "list");
        self.emit_const(Value::Int(0), line); // [list, 0]
        let s_idx = self.add_local("$for_idx", "int");

        let loop_start = self.here();
        self.emit(Op::GetLocal(s_idx), line);
        self.emit(Op::GetLocal(s_list), line);
        self.emit(Op::Len, line); // [idx, len]
        self.emit(Op::Lt, line); // [idx < len]
        let exit_jump = self.emit_jump(Op::JumpIfFalse(0), line);

        self.emit(Op::GetLocal(s_list), line);
        self.emit(Op::GetLocal(s_idx), line);
        self.emit(Op::Index, line); // [elem]
        self.add_local(name, elem_ty); // elem becomes the loop variable

        self.begin_scope(); // body's own locals get cleaned each iteration
        for s in body {
            self.stmt(s)?;
        }
        self.end_scope(line);

        self.emit(Op::Pop, line); // drop the loop variable
        self.locals.pop(); // unregister `name`

        // idx = idx + 1
        self.emit(Op::GetLocal(s_idx), line);
        self.emit_const(Value::Int(1), line);
        self.emit(Op::AddI, line);
        self.emit(Op::SetLocal(s_idx), line);
        self.emit(Op::Jump(loop_start), line);

        self.patch_jump(exit_jump);
        self.end_scope(line); // pops $for_idx, $for_list
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::lex;
    use crate::parser::Parser;
    use crate::vm::Vm;

    /// Compile + run a program on the VM, returning captured output.
    fn run(src: &str) -> Result<String, String> {
        let tokens = lex(src).expect("lex ok");
        let prog = Parser::new(tokens).parse_program().expect("parse ok");
        let program = compile(&prog)?;
        Vm::new(&program).run()
    }

    fn out(src: &str) -> String {
        run(src).expect("run ok")
    }

    #[test]
    fn prints_a_literal_string() {
        assert_eq!(out(r#"function main() { println("hi"); }"#), "hi\n");
    }

    #[test]
    fn integer_arithmetic_in_interpolation() {
        assert_eq!(out(r#"function main() { println("{1 + 2 * 3}"); }"#), "7\n");
    }

    #[test]
    fn float_arithmetic_formats_like_interpreter() {
        assert_eq!(out(r#"function main() { println("{3.0 * 4.0}"); }"#), "12\n");
    }

    #[test]
    fn comparison_and_short_circuit() {
        assert_eq!(out(r#"function main() { println("{1 < 2 && 3 >= 3}"); }"#), "true\n");
        assert_eq!(out(r#"function main() { println("{1 > 2 || false}"); }"#), "false\n");
    }

    #[test]
    fn unary_negation_and_not() {
        assert_eq!(out(r#"function main() { println("{-5}"); println("{!true}"); }"#), "-5\nfalse\n");
    }

    #[test]
    fn division_by_zero_is_runtime_error() {
        let e = run(r#"function main() { println("{1 / 0}"); }"#).unwrap_err();
        assert!(e.contains("division by zero"), "{e}");
    }

    #[test]
    fn missing_main_is_compile_error() {
        let e = run(r#"function other() {}"#).unwrap_err();
        assert!(e.contains("main"), "{e}");
    }

    #[test]
    fn user_function_call_runs() {
        let src = r#"function inc(int n) -> int { return n + 1; } function main() { println("{inc(4)}"); }"#;
        assert_eq!(out(src), "5\n");
    }

    #[test]
    fn recursion_runs() {
        let src = r#"function fib(int n) -> int {
            if (n < 2) { return n; }
            return fib(n - 1) + fib(n - 2);
        } function main() { println("{fib(10)}"); }"#;
        assert_eq!(out(src), "55\n");
    }

    #[test]
    fn class_call_still_rejected_as_p4() {
        // a name that is neither a function nor `println` is a variant/class → P4.
        let src = r#"function main() { println("{Circle(2.0)}"); }"#;
        let e = run(src).unwrap_err();
        assert!(e.contains("M2 P4"), "{e}");
    }

    #[test]
    fn var_decl_and_use() {
        assert_eq!(out(r#"function main() { int x = 10; println("{x + 5}"); }"#), "15\n");
    }

    #[test]
    fn multiple_locals_resolve_to_distinct_slots() {
        let src = r#"function main() { int a = 1; int b = 2; println("{a + b}"); }"#;
        assert_eq!(out(src), "3\n");
    }

    #[test]
    fn float_local_uses_float_arithmetic() {
        let src = r#"function main() { float r = 2.0; println("{r * r}"); }"#;
        assert_eq!(out(src), "4\n");
    }

    #[test]
    fn if_else_picks_branch() {
        let src = r#"function main() { if (1 < 2) { println("yes"); } else { println("no"); } }"#;
        assert_eq!(out(src), "yes\n");
    }

    #[test]
    fn if_without_else() {
        let src = r#"function main() { if (1 > 2) { println("never"); } println("after"); }"#;
        assert_eq!(out(src), "after\n");
    }

    #[test]
    fn for_loop_over_list() {
        let src = r#"function main() { List<int> xs = [1, 2, 3]; for (int x in xs) { println("{x}"); } }"#;
        assert_eq!(out(src), "1\n2\n3\n");
    }

    #[test]
    fn for_loop_body_locals_do_not_leak() {
        // A body-local must be cleaned each iteration (stack stays balanced).
        let src = r#"function main() {
            List<int> xs = [1, 2];
            for (int x in xs) { int y = x + 10; println("{y}"); }
            println("done");
        }"#;
        assert_eq!(out(src), "11\n12\ndone\n");
    }
}
