//! AST → bytecode compiler (M2 P1–P3). A dedicated pass over the type-checked AST,
//! emitting a `Chunk` the VM executes. Mirrors the tree-walker's semantics so
//! `runvm` output is byte-identical to `run` (the differential oracle).
//!
//! P2 scope: `main`-only programs — literals, arithmetic, comparison, logical
//! short-circuit, unary, interpolation, `println`, list literals, locals, `if`/`else`,
//! `for…in`, blocks. P3 added user function calls + call frames + recursion (multi-function
//! compile → `BytecodeProgram`). P4a adds single-payload enums (`Variant(args)` construction)
//! and exhaustive `match` (lowered to scrutinee-spill + per-arm tag/literal tests + payload
//! re-extraction; decision P4-7). Classes/methods/`this`/member (P4b/P4c) still raise a clean
//! compile error until implemented. Enums and lists are value-native `Value` (no heap; P4-1).

use crate::ast::{
    BinaryOp, Expr, FunctionDecl, Item, MatchArm, Pattern, Program, Stmt, StrPart, Type, UnaryOp,
};
use crate::chunk::{BytecodeProgram, Chunk, EnumDesc, Function, Op};
use crate::diagnostic::Diagnostic;
use crate::value::Value;
use std::collections::HashMap;

/// Numeric operand kind, inferred just enough to pick int- vs float-specialized
/// arithmetic ops (decision P2-6).
#[derive(Clone, Copy, PartialEq)]
enum NumTy {
    Int,
    Float,
}

/// The compiler's coarse view of a declared type — enough to pick int- vs float-specialized
/// arithmetic and give `num_ty` an *exhaustive* match (no stringly-typed compare). The checker has
/// already verified full types; threading its richer `types::Ty` here — so list-element and field
/// types are recoverable — is the deferred Wave 4 fix. That gap is exactly why `num_ty` can't yet
/// classify `Index`/`Member` operands (also surface-guarded until P4).
#[derive(Clone, Copy, PartialEq)]
enum TyTag {
    Int,
    Float,
    /// Any non-numeric or composite type (bool, string, unit, list, map, set, named, optional).
    /// The compiler only needs to *reject* these as arithmetic operands, not tell them apart.
    Other,
}

/// A declared local: its name, its coarse type tag (for `num_ty`), and the lexical depth it lives
/// at (for scope cleanup). Its stack slot is its index in `locals`.
struct Local {
    name: String,
    ty: TyTag,
    depth: u32,
}

/// Per-function metadata gathered in the pre-pass: its index in `BytecodeProgram.functions`
/// and its declared return-type tag (for `num_ty` of a call result — decision P3-6).
struct FnMeta {
    index: usize,
    ret: TyTag,
}

/// Per-variant metadata gathered in the pre-pass: its index into the `enum_descs` table (for
/// `MakeEnum`/`MatchTag`) and the coarse type tag of each payload field (so a payload binding
/// used in arithmetic resolves through `num_ty`). Decision P4-2.
struct VariantMeta {
    index: usize,
    field_tags: Vec<TyTag>,
}

/// A `match`-arm payload binding: the name, the slot of the hidden `$match` scrutinee local, and
/// the payload-index `path` from the scrutinee to the bound value. Bindings are *re-extracted* at
/// each use (`GetLocal $match` + `GetEnumField` per path step) rather than stored as stack locals,
/// which keeps arm bodies stack-neutral and sidesteps mid-expression slot bookkeeping (P4-7).
struct MatchBinding {
    name: String,
    match_slot: usize,
    path: Vec<usize>,
    ty: TyTag,
}

struct Compiler<'a> {
    chunk: Chunk,
    locals: Vec<Local>,
    scope_depth: u32,
    fns: &'a HashMap<String, FnMeta>,
    /// Function arities, indexed parallel to `BytecodeProgram.functions` — lets `stack_effect`
    /// account for `Op::Call` (which pops `arity` args and pushes one result).
    arities: &'a [usize],
    /// Variant name → its descriptor metadata (construction + pattern dispatch).
    variants: &'a HashMap<String, VariantMeta>,
    /// The shared enum-descriptor table — `stack_effect` reads `MakeEnum`'s payload arity from it.
    enum_descs: &'a [EnumDesc],
    /// Active `match`-arm bindings (a stack; innermost shadows). Populated while compiling an arm
    /// body, truncated after.
    match_bindings: Vec<MatchBinding>,
    /// Base-relative operand-stack height, tracked so `match` can spill its scrutinee to the
    /// correct slot even mid-expression. Reset to `locals.len()` at each statement boundary and
    /// fixed at `&&`/`||`/`match` control-flow merges; otherwise maintained by `emit`.
    height: usize,
}

/// Compile a whole program: a pre-pass indexes every top-level function (so calls — including
/// forward references and recursion — resolve to a static index), then each function body is
/// compiled into its own `Chunk`. Parameters occupy slots `0..arity` at the base of the frame
/// window; every function ends with an implicit `Unit` return (P3-7).
pub fn compile(program: &Program) -> Result<BytecodeProgram, Diagnostic> {
    // The compiler tracks no source position yet, so every fault becomes a position-less
    // compile-stage `Diagnostic` (renders `compile error: …`, unchanged from before).
    compile_program(program).map_err(Diagnostic::compile)
}

fn compile_program(program: &Program) -> Result<BytecodeProgram, String> {
    let mut order: Vec<&FunctionDecl> = Vec::new();
    let mut fns: HashMap<String, FnMeta> = HashMap::new();
    // Enum pre-pass: one `EnumDesc` per variant of every declared enum, plus the variant-name →
    // metadata map both construction and `match` resolve through (decision P4-2).
    let mut enum_descs: Vec<EnumDesc> = Vec::new();
    let mut variants: HashMap<String, VariantMeta> = HashMap::new();
    for it in &program.items {
        match it {
            Item::Function(f) => {
                fns.insert(
                    f.name.clone(),
                    FnMeta {
                        index: order.len(),
                        ret: f.ret.as_ref().map_or(TyTag::Other, type_tag),
                    },
                );
                order.push(f);
            }
            Item::Enum(e) => {
                for v in &e.variants {
                    variants.insert(
                        v.name.clone(),
                        VariantMeta {
                            index: enum_descs.len(),
                            field_tags: v.fields.iter().map(|p| type_tag(&p.ty)).collect(),
                        },
                    );
                    enum_descs.push(EnumDesc {
                        ty: e.name.clone(),
                        variant: v.name.clone(),
                        arity: v.fields.len(),
                    });
                }
            }
            Item::Import { .. } | Item::Class(_) => {}
        }
    }
    let main = fns
        .get("main")
        .map(|m| m.index)
        .ok_or_else(|| "no `main` function".to_string())?;
    let arities: Vec<usize> = order.iter().map(|f| f.params.len()).collect();

    let mut functions = Vec::with_capacity(order.len());
    for f in &order {
        let mut c = Compiler {
            chunk: Chunk::new(),
            locals: Vec::new(),
            scope_depth: 0,
            fns: &fns,
            arities: &arities,
            variants: &variants,
            enum_descs: &enum_descs,
            match_bindings: Vec::new(),
            height: 0,
        };
        for p in &f.params {
            c.add_local(&p.name, type_tag(&p.ty));
        }
        c.height = c.locals.len(); // params occupy slots `0..arity` (decision P3-1)
        let last_line = f.span.line;
        for s in &f.body {
            c.stmt(s)?;
        }
        c.emit_const(Value::Unit, last_line);
        c.emit(Op::Return, last_line);
        functions.push(Function {
            name: f.name.clone(),
            arity: f.params.len(),
            chunk: c.chunk,
        });
    }
    Ok(BytecodeProgram {
        functions,
        main,
        enum_descs,
    })
}

/// Classify a declared type annotation into the coarse `TyTag` the compiler reasons about. Only the
/// numeric head names matter; everything else — including generics like `List<int>`, whose element
/// type the compiler can't yet recover — collapses to `Other`.
fn type_tag(ty: &Type) -> TyTag {
    match ty {
        Type::Named { name, .. } => match name.as_str() {
            "int" => TyTag::Int,
            "float" => TyTag::Float,
            _ => TyTag::Other,
        },
        Type::Optional { .. } => TyTag::Other,
    }
}

impl<'a> Compiler<'a> {
    fn emit(&mut self, op: Op, line: u32) {
        // Maintain the operand-stack height (saturating: control flow after a `Return`/`MatchFail`
        // is dead code whose height is never read). Branch merges reset `height` explicitly.
        let eff = self.stack_effect(&op);
        self.height = self.height.saturating_add_signed(eff);
        self.chunk.emit(op, line);
    }

    /// Net operand-stack delta of one op (`pushes - pops`). Only consumed by `match` (to spill its
    /// scrutinee to the right slot); kept exhaustive so a new op can't silently skew the height.
    fn stack_effect(&self, op: &Op) -> isize {
        match op {
            Op::Const(_) | Op::GetLocal(_) => 1,
            Op::AddI | Op::SubI | Op::MulI | Op::DivI | Op::RemI => -1,
            Op::AddF | Op::SubF | Op::MulF | Op::DivF | Op::RemF => -1,
            Op::Eq | Op::Ne | Op::Lt | Op::Gt | Op::Le | Op::Ge => -1,
            Op::Pop | Op::SetLocal(_) | Op::JumpIfFalse(_) | Op::Index => -1,
            Op::Neg | Op::Not | Op::Len | Op::Jump(_) => 0,
            Op::MatchTag(_) | Op::GetEnumField(_) => 0, // pop one, push one
            Op::Concat(n) | Op::MakeList(n) => 1 - *n as isize,
            Op::Print(n) => -(*n as isize),
            Op::Call(idx) => 1 - self.arities[*idx] as isize,
            Op::MakeEnum(idx) => 1 - self.enum_descs[*idx].arity as isize,
            // Terminal (end/redirect the frame): height afterward is dead code, never read.
            Op::Return | Op::MatchFail => 0,
        }
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

    fn add_local(&mut self, name: &str, ty: TyTag) -> usize {
        self.locals.push(Local {
            name: name.to_string(),
            ty,
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
                // Mirror `expr`'s resolution order: a `match`-arm binding shadows a local.
                let tag =
                    if let Some(b) = self.match_bindings.iter().rev().find(|b| b.name == *name) {
                        b.ty
                    } else {
                        self.resolve_local(name)
                            .map(|s| self.locals[s].ty)
                            .ok_or_else(|| format!("undefined variable `{name}`"))?
                    };
                Self::as_num(tag).ok_or_else(|| format!("`{name}` is not numeric"))
            }
            Expr::Unary { expr, .. } => self.num_ty(expr),
            Expr::Binary { lhs, .. } => self.num_ty(lhs),
            Expr::Call { callee, .. } => {
                if let Expr::Ident(name, _) = &**callee {
                    if let Some(meta) = self.fns.get(name) {
                        return Self::as_num(meta.ret)
                            .ok_or_else(|| format!("`{name}` does not return a numeric type"));
                    }
                }
                Err(format!("cannot infer numeric type of {e:?}"))
            }
            other => Err(format!("cannot infer numeric type of {other:?}")),
        }
    }

    /// Numeric refinement of a stored `TyTag` — the bridge from "what type the operand is" to
    /// "which specialized arithmetic op." `None` for non-numeric tags (a defensive path: the
    /// checker already guarantees arithmetic operands are numeric).
    fn as_num(tag: TyTag) -> Option<NumTy> {
        match tag {
            TyTag::Int => Some(NumTy::Int),
            TyTag::Float => Some(NumTy::Float),
            TyTag::Other => None,
        }
    }

    fn stmt(&mut self, s: &Stmt) -> Result<(), String> {
        // Every statement begins with a clean operand stack (transients == 0), so the live operand
        // height equals the live-locals count. Anchoring here keeps `match`'s scrutinee slot exact
        // regardless of any height drift in preceding dead-code-after-`return`.
        self.height = self.locals.len();
        match s {
            Stmt::VarDecl { ty, name, init, .. } => {
                self.expr(init)?; // value stays on the stack as the new local's slot
                self.add_local(name, type_tag(ty));
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
            Stmt::If {
                cond,
                then_block,
                else_block,
                span,
            } => self.compile_if(cond, then_block, else_block.as_deref(), span.line),
            Stmt::For {
                ty,
                name,
                iter,
                body,
                span,
            } => self.compile_for(name, type_tag(ty), iter, body, span.line),
        }
    }

    fn expr(&mut self, e: &Expr) -> Result<(), String> {
        match e {
            Expr::Int(n, sp) => self.emit_const(Value::Int(*n), sp.line),
            Expr::Float(x, sp) => self.emit_const(Value::Float(*x), sp.line),
            Expr::Bool(b, sp) => self.emit_const(Value::Bool(*b), sp.line),
            Expr::Str(parts, sp) => self.compile_str(parts, sp.line)?,
            Expr::Ident(name, sp) => {
                // A `match`-arm binding shadows locals: re-extract it from `$match` along its
                // payload path (decision P4-7). Otherwise it's an ordinary local slot.
                if let Some((slot, path)) = self.resolve_binding(name) {
                    self.emit(Op::GetLocal(slot), sp.line);
                    for i in path {
                        self.emit(Op::GetEnumField(i), sp.line);
                    }
                } else {
                    let slot = self
                        .resolve_local(name)
                        .ok_or_else(|| format!("undefined variable `{name}`"))?;
                    self.emit(Op::GetLocal(slot), sp.line);
                }
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
            Expr::Match {
                scrutinee,
                arms,
                span,
            } => self.compile_match(scrutinee, arms, span.line)?,
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
                let l_false = self.emit_jump(Op::JumpIfFalse(0), line); // pops lhs
                let h_merge = self.height; // both branches converge to one bool above this
                self.expr(rhs)?;
                let l_end = self.emit_jump(Op::Jump(0), line);
                self.patch_jump(l_false);
                self.height = h_merge; // false-path: reset before pushing the literal `false`
                self.emit_const(Value::Bool(false), line);
                self.patch_jump(l_end);
                return Ok(());
            }
            Or => {
                self.expr(lhs)?;
                let l_rhs = self.emit_jump(Op::JumpIfFalse(0), line); // pops lhs
                let h_merge = self.height;
                self.emit_const(Value::Bool(true), line);
                let l_end = self.emit_jump(Op::Jump(0), line);
                self.patch_jump(l_rhs);
                self.height = h_merge; // rhs-path: reset before evaluating rhs
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
            // An enum variant constructor: `Variant(args)` (or a bare `Variant`, args empty).
            // The checker has already verified arity, so push the payload and tag it (P4-3).
            if let Some(meta) = self.variants.get(name) {
                let idx = meta.index;
                for a in args {
                    self.expr(a)?;
                }
                self.emit(Op::MakeEnum(idx), line);
                return Ok(());
            }
            // A non-function, non-variant identifier call is a class constructor — that lands at P4b.
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
        elem_ty: TyTag,
        iter: &Expr,
        body: &[Stmt],
        line: u32,
    ) -> Result<(), String> {
        self.begin_scope();
        self.expr(iter)?; // [list]
        let s_list = self.add_local("$for_list", TyTag::Other);
        self.emit_const(Value::Int(0), line); // [list, 0]
        let s_idx = self.add_local("$for_idx", TyTag::Int);

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

    /// Resolve a `match`-arm binding by name (innermost shadows). Returns the `$match` slot and the
    /// payload path to re-extract, cloned so the caller can emit without holding a borrow on `self`.
    fn resolve_binding(&self, name: &str) -> Option<(usize, Vec<usize>)> {
        self.match_bindings
            .iter()
            .rev()
            .find(|b| b.name == name)
            .map(|b| (b.match_slot, b.path.clone()))
    }

    /// `match scrutinee { pat => body, … }` as an expression (decision P4-7). The scrutinee is
    /// evaluated once and spilled to a hidden `$match` slot; each arm tests its pattern (skipping
    /// to the next arm on mismatch), binds payloads by re-extraction, then leaves its body's single
    /// value on the stack. A matched arm jumps past the rest to a collapse that overwrites the
    /// scrutinee slot with the result — so the whole `match` leaves exactly one value.
    fn compile_match(
        &mut self,
        scrutinee: &Expr,
        arms: &[MatchArm],
        line: u32,
    ) -> Result<(), String> {
        // Coarse type of the scrutinee, for a catch-all binding used arithmetically (best-effort:
        // non-numeric scrutinees collapse to `Other`, which `num_ty` rejects as an operand anyway).
        let scrut_tag = match self.num_ty(scrutinee) {
            Ok(NumTy::Int) => TyTag::Int,
            Ok(NumTy::Float) => TyTag::Float,
            Err(_) => TyTag::Other,
        };
        self.expr(scrutinee)?;
        let m_slot = self.height - 1; // scrutinee now on top: its base-relative slot
        let mut end_jumps = Vec::new();
        for arm in arms {
            self.height = m_slot + 1; // each arm dispatches with just the scrutinee live
            let mut skips = Vec::new();
            self.emit_pattern_test(&arm.pattern, m_slot, &[], &mut skips, line)?;
            let n_before = self.match_bindings.len();
            self.register_bindings(&arm.pattern, m_slot, &[], scrut_tag)?;
            self.expr(&arm.body)?; // -> [.., scrutinee, result]
            self.match_bindings.truncate(n_before);
            end_jumps.push(self.emit_jump(Op::Jump(0), line));
            for j in skips {
                self.patch_jump(j); // a mismatch lands at the next arm
            }
        }
        self.emit(Op::MatchFail, line); // checker-unreachable backstop (EV-7 parity)
        for j in end_jumps {
            self.patch_jump(j); // matched arms converge here: [.., scrutinee, result]
        }
        self.height = m_slot + 2;
        self.emit(Op::SetLocal(m_slot), line); // result overwrites scrutinee slot -> [.., result]
        Ok(())
    }

    /// Emit the test for `pat` against the `$match` sub-value reached by `path`. On a mismatch the
    /// emitted `JumpIfFalse`'s index is recorded in `skips` (the caller patches them to the next
    /// arm). Wildcard and binding patterns always match, so they emit no test.
    fn emit_pattern_test(
        &mut self,
        pat: &Pattern,
        m_slot: usize,
        path: &[usize],
        skips: &mut Vec<usize>,
        line: u32,
    ) -> Result<(), String> {
        match pat {
            Pattern::Wildcard(_) | Pattern::Binding { .. } => {}
            Pattern::Int(n, _) => self.emit_literal_test(m_slot, path, Value::Int(*n), skips, line),
            Pattern::Float(x, _) => {
                self.emit_literal_test(m_slot, path, Value::Float(*x), skips, line);
            }
            Pattern::Str(s, _) => {
                self.emit_literal_test(m_slot, path, Value::Str(s.clone()), skips, line);
            }
            Pattern::Bool(b, _) => {
                self.emit_literal_test(m_slot, path, Value::Bool(*b), skips, line);
            }
            Pattern::Null(_) => {
                // No null values exist in M1, so a null pattern never matches (interpreter
                // parity, `match_pattern`): an unconditional skip.
                self.emit_const(Value::Bool(false), line);
                skips.push(self.emit_jump(Op::JumpIfFalse(0), line));
            }
            Pattern::Variant { name, fields, .. } => {
                let idx = self
                    .variants
                    .get(name)
                    .ok_or_else(|| format!("unknown variant `{name}`"))?
                    .index;
                self.emit_load_path(m_slot, path, line);
                self.emit(Op::MatchTag(idx), line);
                skips.push(self.emit_jump(Op::JumpIfFalse(0), line));
                for (i, fp) in fields.iter().enumerate() {
                    let mut sub = path.to_vec();
                    sub.push(i);
                    self.emit_pattern_test(fp, m_slot, &sub, skips, line)?;
                }
            }
        }
        Ok(())
    }

    /// Load the `$match` sub-value at `path`, compare it to `lit`, and skip the arm on inequality.
    fn emit_literal_test(
        &mut self,
        m_slot: usize,
        path: &[usize],
        lit: Value,
        skips: &mut Vec<usize>,
        line: u32,
    ) {
        self.emit_load_path(m_slot, path, line);
        self.emit_const(lit, line);
        self.emit(Op::Eq, line);
        skips.push(self.emit_jump(Op::JumpIfFalse(0), line));
    }

    /// Push the sub-value of the `$match` scrutinee (slot `m_slot`) reached by `path`.
    fn emit_load_path(&mut self, m_slot: usize, path: &[usize], line: u32) {
        self.emit(Op::GetLocal(m_slot), line);
        for &i in path {
            self.emit(Op::GetEnumField(i), line);
        }
    }

    /// Register (emitting no code) every binding introduced by `pat`, so the arm body can
    /// re-extract them. `cur_ty` is the coarse type of the value `pat` matches (for `num_ty`).
    fn register_bindings(
        &mut self,
        pat: &Pattern,
        m_slot: usize,
        path: &[usize],
        cur_ty: TyTag,
    ) -> Result<(), String> {
        match pat {
            Pattern::Binding { name, .. } => self.match_bindings.push(MatchBinding {
                name: name.clone(),
                match_slot: m_slot,
                path: path.to_vec(),
                ty: cur_ty,
            }),
            Pattern::Variant { name, fields, .. } => {
                let field_tags = self
                    .variants
                    .get(name)
                    .ok_or_else(|| format!("unknown variant `{name}`"))?
                    .field_tags
                    .clone();
                for (i, fp) in fields.iter().enumerate() {
                    let mut sub = path.to_vec();
                    sub.push(i);
                    let ty = field_tags.get(i).copied().unwrap_or(TyTag::Other);
                    self.register_bindings(fp, m_slot, &sub, ty)?;
                }
            }
            _ => {} // wildcard / literals bind nothing
        }
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
        let program = compile(&prog).map_err(|d| d.to_string())?;
        Vm::new(&program).run().map_err(|d| d.to_string())
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
        assert_eq!(
            out(r#"function main() { println("{3.0 * 4.0}"); }"#),
            "12\n"
        );
    }

    #[test]
    fn comparison_and_short_circuit() {
        assert_eq!(
            out(r#"function main() { println("{1 < 2 && 3 >= 3}"); }"#),
            "true\n"
        );
        assert_eq!(
            out(r#"function main() { println("{1 > 2 || false}"); }"#),
            "false\n"
        );
    }

    #[test]
    fn unary_negation_and_not() {
        assert_eq!(
            out(r#"function main() { println("{-5}"); println("{!true}"); }"#),
            "-5\nfalse\n"
        );
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
        assert_eq!(
            out(r#"function main() { int x = 10; println("{x + 5}"); }"#),
            "15\n"
        );
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

    #[test]
    fn enum_construct_and_match_binds_payload() {
        let src = r#"enum Grade { Pass(int s), Fail(int s), }
            function d(Grade g) -> string { return match g { Pass(s) => "P{s}", Fail(s) => "F{s}", }; }
            function main() { println(d(Pass(9))); println(d(Fail(3))); }"#;
        assert_eq!(out(src), "P9\nF3\n");
    }

    #[test]
    fn match_literal_arms_and_catch_all_binding() {
        let src = r#"function f(int n) -> string { return match n { 0 => "z", 1 => "o", x => "m{x}", }; }
            function main() { println(f(0)); println(f(1)); println(f(9)); }"#;
        assert_eq!(out(src), "z\no\nm9\n");
    }

    #[test]
    fn match_as_binary_operand_tracks_scrutinee_slot() {
        // The lhs `1` is live on the operand stack when the `match` rhs compiles, so the scrutinee
        // must spill to a transient-aware slot (not `locals.len()`).
        let src = r#"function g(int n) -> int { return 1 + match n { 0 => 10, _ => 20 }; }
            function main() { println("{g(0)}"); println("{g(5)}"); }"#;
        assert_eq!(out(src), "11\n21\n");
    }

    #[test]
    fn nested_match_reextracts_outer_binding() {
        // Inner `match` compiles while the outer scrutinee occupies slot `locals.len()`; its own
        // scrutinee must land one slot higher (height tracking), and the inner arm re-extracts the
        // outer binding `b` from the outer scrutinee.
        let src = r#"enum Pair { P(int a, int b), }
            function f(Pair p) -> string {
                return match p { P(a, b) => match a { 0 => "z b={b}", _ => "a={a} b={b}", }, };
            }
            function main() { println(f(P(0, 9))); println(f(P(5, 2))); }"#;
        assert_eq!(out(src), "z b=9\na=5 b=2\n");
    }
}
