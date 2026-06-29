//! Bytecode compiler — expr (M-Decomp W4.1). See compiler/mod.rs for the struct,
//! emission/scope core, and the (kept-whole) `stack_effect`.

use super::*;

impl Compiler<'_> {
    /// Resolve a `parent`/super call to the baked target function index (M-RT super/parent). Uses the
    /// `methods` table (which already encodes dispatch origins) keyed by the named ancestor (qualified
    /// form) or, for the immediate form, the lexical class's direct parents — the unique resolved index
    /// (a diamond's shared base collapses to one; ≥2 distinct is checker-rejected as ambiguous, so a
    /// valid program always yields exactly one). Matches the interpreter's `resolve_parent_method`.
    fn resolve_parent_target(&self, ancestor: Option<&str>, method: &str) -> Result<usize, String> {
        let cur = self
            .cur_class
            .as_deref()
            .ok_or("`parent` used outside a method")?;
        let lookup = |class: &str| {
            self.methods
                .get(&(class.to_string(), method.to_string()))
                .copied()
        };
        let func = match ancestor {
            Some(a) => lookup(a),
            None => {
                let parents = self.parent_parents.and_then(|p| p.get(cur));
                let mut found: Vec<usize> = Vec::new();
                for p in parents.map(Vec::as_slice).unwrap_or(&[]) {
                    if let Some(idx) = lookup(p) {
                        if !found.contains(&idx) {
                            found.push(idx);
                        }
                    }
                }
                found.first().copied()
            }
        };
        func.ok_or_else(|| {
            format!(
                "internal: unresolved parent method `{method}` (checker should have caught this)"
            )
        })
    }

    /// The operand `CTy` of a `parent`/super call's result — the resolved target method's return type
    /// from `method_rets` (M-RT super/parent). `CTy::Other` when not resolvable (a non-operand context;
    /// the checker has already validated the call). Lets `parent.m(…) + 1` specialize on the VM.
    pub(super) fn parent_ret_cty(&self, ancestor: Option<&str>, method: &str) -> CTy {
        let Some(cur) = self.cur_class.as_deref() else {
            return CTy::Other;
        };
        let lookup = |class: &str| {
            self.method_rets
                .get(&(class.to_string(), method.to_string()))
                .cloned()
        };
        let cty = match ancestor {
            Some(a) => lookup(a),
            None => self
                .parent_parents
                .and_then(|p| p.get(cur))
                .and_then(|parents| parents.iter().find_map(|p| lookup(p))),
        };
        cty.unwrap_or(CTy::Other)
    }

    pub(super) fn expr(&mut self, e: &Expr) -> Result<(), String> {
        match e {
            Expr::Int(n, sp) => self.emit_const(Value::Int(*n), sp.line),
            Expr::Float(x, sp) => self.emit_const(Value::Float(*x), sp.line),
            // A `decimal` literal rides the constant pool like float/bytes — no new Op (M-NUM S1).
            Expr::Decimal {
                unscaled,
                scale,
                span,
            } => self.emit_const(
                Value::Decimal {
                    unscaled: *unscaled,
                    scale: *scale,
                },
                span.line,
            ),
            Expr::Bool(b, sp) => self.emit_const(Value::Bool(*b), sp.line),
            Expr::Str(parts, sp) => self.compile_str(parts, sp.line)?,
            Expr::Bytes(b, sp) => {
                self.emit_const(Value::Bytes(std::rc::Rc::new(b.clone())), sp.line)
            }
            Expr::Ident(name, sp) => {
                // Resolution order mirrors the interpreter's `eval_ident`: a `match`-arm binding
                // (re-extracted from `$match` along its payload path; P4-7) shadows a local/param,
                // which shadows a bare field of `this` (a method/ctor body, lowered to
                // `this.field`). An unresolved name is a compiler bug (the checker ran first).
                if let Some((slot, path)) = self.resolve_binding(name) {
                    self.emit(Op::GetLocal(slot), sp.line);
                    self.emit_path(&path, sp.line);
                } else if let Some(slot) = self.resolve_local(name) {
                    self.emit(Op::GetLocal(slot), sp.line);
                } else if let (Some(this), true) =
                    (self.this_slot, self.field_tags.contains_key(name))
                {
                    let idx = self.field_name_index(name)?;
                    self.emit(Op::GetLocal(this), sp.line);
                    self.emit(Op::GetField(idx), sp.line);
                } else if let Some(idx) = self.fns.get(name).map(|m| m.index) {
                    // Bare named-function reference in value position → a zero-capture closure.
                    // Read the index from the immutable `self.fns` borrow into a local before
                    // calling `self.emit` (which needs `&mut self`).
                    self.emit(Op::MakeClosure(idx), sp.line);
                } else {
                    return Err(format!("undefined variable `{name}`"));
                }
            }
            Expr::List(items, sp) => {
                for it in items {
                    self.expr(it)?;
                }
                self.emit(Op::MakeList(items.len()), sp.line);
            }
            Expr::Map(pairs, sp) => {
                // Push each key then its value (source order); `Op::MakeMap(n)` pops the 2n values and
                // builds the insertion-ordered map via the shared `build_map` kernel (M-RT S3).
                for (k, v) in pairs {
                    self.expr(k)?;
                    self.expr(v)?;
                }
                self.emit(Op::MakeMap(pairs.len()), sp.line);
            }
            Expr::Unary { op, expr, span } => {
                self.expr(expr)?;
                match op {
                    UnaryOp::Neg => self.emit(Op::Neg, span.line),
                    UnaryOp::Not => self.emit(Op::Not, span.line),
                    UnaryOp::BitNot => self.emit(Op::BitNot, span.line),
                }
            }
            Expr::Binary { op, lhs, rhs, span } => self.compile_binary(*op, lhs, rhs, span.line)?,
            Expr::InstanceOf {
                value,
                type_name,
                span,
            } => {
                // Push the value, then a single `IsInstance` op carrying the class name inline pops
                // it and pushes a `Bool` (M-RT S1). The class name lives in the op (like `Fault`), so
                // no name-pool entry is needed and the runtime predicate matches the interpreter.
                self.expr(value)?;
                self.emit(Op::IsInstance(type_name.clone()), span.line);
            }
            Expr::Cast {
                value,
                type_name,
                span,
            } => {
                // M4 as-matrix: a primitive `as` CONVERSION was rewritten to a native call before this
                // backend; the only primitive `Cast` reaching here is the **identity** (`T as T`) —
                // compile the value unchanged.
                if matches!(
                    type_name.as_str(),
                    "int" | "float" | "string" | "bool" | "decimal"
                ) {
                    self.expr(value)?;
                    return Ok(());
                }
                // Checked downcast (M4 casting axis 2): keep `value` if it `IsInstance` of `type_name`,
                // else replace it with `null` — result type `type_name?`. `value` is evaluated ONCE:
                // stash it in a scratch slot (the `??`/`$match` trick — its frame-relative top, NOT
                // `add_local`, since live transients may sit below), duplicate it to feed `IsInstance`,
                // then branch. Reuses `Op::IsInstance` — no new `Op` (decision S2-OPS).
                self.expr(value)?; // [v] — v lands in the scratch slot
                let slot = self.height - 1;
                self.emit(Op::GetLocal(slot), span.line); // [v, v]
                self.emit(Op::IsInstance(type_name.clone()), span.line); // [v, bool]
                let to_null = self.emit_jump(Op::JumpIfFalse(0), span.line); // [v]; jump if !instanceof
                let h_merge = self.height;
                // true path: `v` is already the result; jump past the null branch.
                let end_j = self.emit_jump(Op::Jump(0), span.line);
                self.patch_jump(to_null); // null path arrives with [v]
                self.height = h_merge;
                self.emit_const(Value::Null, span.line); // [v, null]
                self.emit(Op::SetLocal(slot), span.line); // [null] — overwrite the slot with null
                self.patch_jump(end_j); // both paths leave one value at `slot`
                self.height = h_merge;
            }
            Expr::Call { callee, args, span } => self.compile_call(callee, args, span.line)?,
            Expr::Null(sp) => self.emit_const(Value::Null, sp.line),
            Expr::This(sp) => match self.this_slot {
                // `this` is the receiver local: slot 0 in a method, the instance slot in a ctor.
                Some(slot) => self.emit(Op::GetLocal(slot), sp.line),
                // Checker-unreachable (`this` outside a method/ctor); mirrors the interpreter.
                None => return Err("`this` used outside a method".into()),
            },
            Expr::Member {
                object,
                name,
                safe,
                span,
            } => {
                // Field read: evaluate the object, then look its field up at runtime by name
                // (decision P4-5). Runtime lookup keeps the compiler untyped; the fault on a miss
                // is byte-identical to the interpreter's. `?.` (safe) short-circuits a null receiver.
                let line = span.line;
                // A `const` class constant (Feature A): inline the literal `Value` via `Op::Const` —
                // no runtime store. Checked before the static path (a const is never a static slot).
                if let Some(v) = self.const_value(object, name) {
                    self.emit_const(v, line);
                } else if let Some(idx) = self.static_slot(object, name) {
                    // Static read `ClassName.field` (M-mut.7): the head is a class name (not a local),
                    // so this is program-level state, not an instance field.
                    self.emit(Op::GetStatic(idx), line);
                } else if let Some(getm) = self.hook_get_method(object, name) {
                    // Property hook read `o.name` (M-mut.7b) → call the synthetic `<name>$get`
                    // 0-arg method, which leaves the computed value on the stack. `?.` short-circuits
                    // a null receiver before dispatch (the interpreter does the same).
                    if *safe {
                        self.compile_safe_access(object, line, |c| {
                            let idx = c.field_name_index(&getm)?;
                            c.emit(Op::CallMethod(idx, 0), line);
                            Ok(())
                        })?;
                    } else {
                        self.expr(object)?;
                        let idx = self.field_name_index(&getm)?;
                        self.emit(Op::CallMethod(idx, 0), line);
                    }
                } else if *safe {
                    self.compile_safe_access(object, line, |c| {
                        let idx = c.field_name_index(name)?;
                        c.emit(Op::GetField(idx), line);
                        Ok(())
                    })?;
                } else {
                    self.expr(object)?;
                    let idx = self.field_name_index(name)?;
                    self.emit(Op::GetField(idx), line);
                }
            }
            Expr::Index {
                object,
                index,
                span,
            } => {
                // Push the list, then the index; `Op::Index` pops index-then-list and pushes the
                // bounds-checked element clone (the same op `compile_for` already uses).
                self.expr(object)?;
                self.expr(index)?;
                self.emit(Op::Index, span.line);
            }
            Expr::Force { inner, span } => self.compile_force(inner, span.line)?,
            Expr::Propagate { inner, span } => self.compile_propagate(inner, span.line)?,
            Expr::CloneWith {
                object,
                fields,
                span,
            } => self.compile_clone_with(object, fields, span.line)?,
            Expr::Match {
                scrutinee,
                arms,
                span,
            } => self.compile_match(scrutinee, arms, span.line)?,
            Expr::Range {
                start,
                end,
                inclusive,
                span,
            } => {
                // Push start, then end; `MakeRange` pops end-then-start and materializes the list.
                self.expr(start)?;
                self.expr(end)?;
                self.emit(Op::MakeRange(*inclusive), span.line);
            }
            Expr::If {
                cond,
                then_expr,
                else_expr,
                span,
            } => {
                // Lower like `&&`/`||`: branch on the cond, each arm leaves exactly one value, and
                // the merge height is reset so both arms agree on the single result slot.
                self.expr(cond)?;
                let else_j = self.emit_jump(Op::JumpIfFalse(0), span.line); // pops cond
                let h_merge = self.height; // both arms converge to one value above this
                self.expr(then_expr)?;
                let end_j = self.emit_jump(Op::Jump(0), span.line);
                self.patch_jump(else_j);
                self.height = h_merge; // else path starts at the merge height
                self.expr(else_expr)?;
                self.patch_jump(end_j);
            }
            Expr::Lambda {
                params,
                body,
                ret,
                span,
            } => self.compile_lambda(params, body, ret.as_ref(), span.line)?,
            // `html"…"` literals are erased to `html.concat([…])` kernel calls by
            // `checker::resolve_html` before compilation; the compiler never sees one.
            Expr::Html(..) => unreachable!("html literal not resolved before compilation"),
            Expr::OverloadSelect { .. } => {
                unreachable!("overload selector resolved + rewritten before compilation (Slice C1)")
            }
            // `parent.m(args)` / `parent(A).m(args)` — super/parent dispatch (M-RT super/parent).
            Expr::ParentCall {
                ancestor,
                method,
                args,
                span,
            } => {
                let func = self.resolve_parent_target(ancestor.as_deref(), method)?;
                // Push the current receiver (`this`) as the call's slot 0, then the args.
                let this_slot = self
                    .this_slot
                    .ok_or_else(|| "`parent` used outside a method".to_string())?;
                self.emit(Op::GetLocal(this_slot), span.line);
                for a in args {
                    self.expr(a)?;
                }
                self.emit(Op::CallParent(func, args.len()), span.line);
            }
            Expr::New(..) => {
                unreachable!("Expr::New is unwrapped before compilation (checker::unwrap_new)")
            }
        }
        Ok(())
    }

    pub(super) fn compile_str(&mut self, parts: &[StrPart], line: u32) -> Result<(), String> {
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

    pub(super) fn compile_binary(
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
            Coalesce => {
                // `a ?? b`: keep `a` unless it is null, without re-evaluating it. Stash `a` in a
                // scratch local (the `$match`-scrutinee trick), test it against `null`; if null,
                // evaluate `b` and overwrite the slot with it. No new `Op` (decision S2-OPS).
                self.expr(lhs)?; // [a] — a lands in the scratch slot
                                 // The scratch slot is `a`'s frame-relative position (top of stack), NOT
                                 // `locals.len()`: live transients (e.g. earlier interpolation segments) may sit
                                 // below it, so `add_local`'s index would be wrong. Mirrors `compile_match`'s
                                 // `m_slot = self.height - 1`. Addressed numerically by Get/SetLocal — no `Local` entry.
                let slot = self.height - 1;
                self.emit(Op::GetLocal(slot), line); // [a, a]
                self.emit_const(Value::Null, line); // [a, a, null]
                self.emit(Op::Eq, line); // [a, bool]
                let keep = self.emit_jump(Op::JumpIfFalse(0), line); // [a]; jump if a != null
                let h_merge = self.height;
                self.expr(rhs)?; // [a, b]
                self.emit(Op::SetLocal(slot), line); // [b] — overwrite the slot with b
                self.patch_jump(keep); // keep-path arrives with [a]; both leave one value at `slot`
                self.height = h_merge;
                return Ok(());
            }
            _ => {}
        }
        // Strict ops: evaluate both, then emit.
        match op {
            // `string + string` → concatenation: reuse `Op::Concat(2)` (no new Op). The checker
            // guarantees both operands are `string`, and `ctype` resolves every string-producing
            // operand to `CTy::Str`, so this lowers byte-identically to the interpreter's `Str + Str`
            // (Phase 1 string slice).
            Add if matches!(self.ctype(lhs), Ok(CTy::Str)) => {
                self.expr(lhs)?;
                self.expr(rhs)?;
                self.emit(Op::Concat(2), line);
            }
            // `**` power has no dedicated `Op`: it lowers (type-directed) to a `Core.Math` native
            // call — `ipow` for `int**int`, `pow` for `float**float` — keeping the no-new-Op
            // invariant. The native dispatches into `value::int_pow`/`float_pow`, the *same* kernels
            // the interpreter's `**` arm uses, so `run`/`runvm` compute and fault identically. The
            // registry index is resolved at compile time, so no `import Core.Math` is required.
            Pow => {
                let leaf = match self.num_ty(lhs)? {
                    NumTy::Int => "ipow",
                    NumTy::Float => "pow",
                    // `decimal ** _` is rejected by the checker (decimal supports only `+ - *`).
                    NumTy::Decimal => unreachable!("decimal `**` rejected by checker"),
                };
                let idx = crate::native::index_of("Core.Math", leaf)
                    .expect("Core.Math.ipow/pow are registered natives");
                self.expr(lhs)?;
                self.expr(rhs)?;
                self.emit(Op::CallNative(idx, 2), line);
            }
            Add | Sub | Mul | Div | Rem => {
                // `decimal` arithmetic (M-NUM S1): emit `AddD/SubD/MulD` when EITHER operand is
                // decimal (`decimal ⊕ int` widens the int in the value kernel) — `num_ty(lhs)` alone
                // would mis-classify `int * decimal`. The checker allows all of decimal `+ - * % /`
                // (`/` is exact-or-fault), so any of them reaches the decimal path. Probe both operands; a probe that errs
                // (a genuinely unresolvable operand) falls through to the int/float path's error.
                let lhs_dec = matches!(self.ctype(lhs), Ok(CTy::Decimal));
                let rhs_dec = matches!(self.ctype(rhs), Ok(CTy::Decimal));
                let nt = if lhs_dec || rhs_dec {
                    NumTy::Decimal
                } else {
                    self.num_ty(lhs)?
                };
                self.expr(lhs)?;
                self.expr(rhs)?;
                let emit = match (op, nt) {
                    (Add, NumTy::Int) => Op::AddI,
                    (Add, NumTy::Float) => Op::AddF,
                    (Add, NumTy::Decimal) => Op::AddD,
                    (Sub, NumTy::Int) => Op::SubI,
                    (Sub, NumTy::Float) => Op::SubF,
                    (Sub, NumTy::Decimal) => Op::SubD,
                    (Mul, NumTy::Int) => Op::MulI,
                    (Mul, NumTy::Float) => Op::MulF,
                    (Mul, NumTy::Decimal) => Op::MulD,
                    (Div, NumTy::Int) => Op::DivI,
                    (Div, NumTy::Float) => Op::DivF,
                    (Rem, NumTy::Int) => Op::RemI,
                    (Rem, NumTy::Float) => Op::RemF,
                    // Exact decimal `%` and exact-or-fault `/` (2026-06-27).
                    (Rem, NumTy::Decimal) => Op::RemD,
                    (Div, NumTy::Decimal) => Op::DivD,
                    _ => unreachable!("arithmetic op set"),
                };
                self.emit(emit, line);
            }
            Eq => {
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
            // Bitwise binaries (primitives P2): int-only (checker-guaranteed), so the int Op is
            // emitted directly — no `NumTy` dispatch.
            BitAnd | BitOr | BitXor | Shl | Shr => {
                self.expr(lhs)?;
                self.expr(rhs)?;
                self.emit(
                    match op {
                        BitAnd => Op::BitAnd,
                        BitOr => Op::BitOr,
                        BitXor => Op::BitXor,
                        Shl => Op::Shl,
                        Shr => Op::Shr,
                        _ => unreachable!(),
                    },
                    line,
                );
            }
            Pipe => unreachable!("`|>` is lowered to a call in the parser"),
            And | Or | Coalesce => unreachable!("handled above"),
        }
        Ok(())
    }

    pub(super) fn compile_call(
        &mut self,
        callee: &Expr,
        args: &[Expr],
        line: u32,
    ) -> Result<(), String> {
        if let Expr::Ident(name, _) = callee {
            // Fault intrinsics (M-faults 2a) lower to `Op::Fault` — no user-function dispatch.
            if self.compile_intrinsic(name, args, line)? {
                return Ok(());
            }
            if let Some(meta) = self.fns.get(name) {
                let dispatch = meta.overload;
                let index = meta.index;
                for a in args {
                    self.expr(a)?;
                }
                // An overloaded name dispatches on the runtime argument types (M-RT); a single
                // overload is a direct call as before.
                match dispatch {
                    Some(set_id) => self.emit(Op::CallOverload(set_id, args.len()), line),
                    None => self.emit(Op::Call(index), line),
                }
                return Ok(());
            }
            // A local variable with a function type (lambda or named-fn ref): push the closure
            // first (by its local slot), then the args, then dispatch with `CallValue`.
            if let Some(slot) = self.resolve_local(name) {
                if matches!(self.locals[slot].ty, CTy::Fn { .. }) {
                    self.emit(Op::GetLocal(slot), line); // push the closure value
                    for a in args {
                        self.expr(a)?;
                    }
                    self.emit(Op::CallValue(args.len()), line);
                    return Ok(());
                }
            }
            // A match-arm binding with a function type (lambda passed as an argument).
            if let Some((slot, path)) = self.resolve_binding(name) {
                if matches!(
                    self.match_bindings
                        .iter()
                        .rev()
                        .find(|b| b.name == *name)
                        .map(|b| &b.ty),
                    Some(CTy::Fn { .. })
                ) {
                    // Re-extract the closure from its binding path.
                    self.emit(Op::GetLocal(slot), line);
                    self.emit_path(&path, line);
                    for a in args {
                        self.expr(a)?;
                    }
                    self.emit(Op::CallValue(args.len()), line);
                    return Ok(());
                }
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
            // A class constructor: `ClassName(args)` calls the synthetic `<Class>::new`, which
            // promotes its params into fields and returns the instance (decision P4-4).
            if let Some(&ctor_idx) = self.classes.get(name) {
                for a in args {
                    self.expr(a)?;
                }
                self.emit(Op::Call(ctor_idx), line);
                return Ok(());
            }
            // Unreachable for checker-validated programs; mirrors `interpreter::eval_call`'s wording.
            return Err(format!("`{name}` is not a function, variant, or class"));
        }
        // Method call `object.name(args)`: evaluate the receiver, then the args, and dispatch by
        // name at runtime off the receiver's class (decision P4-6).
        if let Expr::Member {
            object, name, safe, ..
        } = callee
        {
            // Namespaced native call: `console.println(x)` — a member call whose head is an imported
            // module qualifier rather than a value (M3 Wave 1). Locals-first: only an identifier that
            // is *not* a bound local/match-binding can be a qualifier, and the checker has already
            // enforced that it was imported and the native exists, so `index_of_by_leaf` is an
            // unambiguous lower (every stdlib leaf is distinct). Lowers to `Op::CallNative`, which
            // pushes the native's result — no separate `Const(Unit)` (the old `Print` path's pair).
            if !*safe {
                if let Expr::Ident(q, _) = &**object {
                    if self.resolve_local(q).is_none() && self.resolve_binding(q).is_none() {
                        if let Some(idx) = crate::native::index_of_by_leaf(q, name) {
                            for a in args {
                                self.expr(a)?;
                            }
                            self.emit(Op::CallNative(idx, args.len()), line);
                            return Ok(());
                        }
                    }
                }
            }
            // Static method call `ClassName.method(args)` (slice B0): the class is known at compile
            // time. Push a dummy receiver (slot 0 of the compiled method is `$this`, which a static
            // method never reads) then the args. A *non-overloaded* static lowers to a direct
            // `Op::Call` to the `(class, method)` function index; an *overloaded* static (Statics-B)
            // lowers to `Op::CallOverload(set_id, argc)` — the same runtime selector instance overloads
            // use, with the dummy receiver sitting below the `argc` args (so selection sees only the
            // args, and the selected body's arity pops the dummy + args). Resolved after the native
            // path (an explicit import wins a name collision), before instance dispatch.
            if !*safe {
                if let Expr::Ident(cls, _) = &**object {
                    if self.resolve_local(cls).is_none()
                        && self.resolve_binding(cls).is_none()
                        && self.classes.contains_key(cls)
                    {
                        let key = (cls.clone(), name.clone());
                        if let Some(&set_id) = self.method_overloads.get(&key) {
                            self.emit_const(Value::Unit, line); // dummy receiver in slot 0
                            for a in args {
                                self.expr(a)?;
                            }
                            self.emit(Op::CallStaticOverload(set_id, args.len()), line);
                            return Ok(());
                        }
                        if let Some(&fn_idx) = self.methods.get(&key) {
                            self.emit_const(Value::Unit, line); // dummy receiver in slot 0
                            for a in args {
                                self.expr(a)?;
                            }
                            self.emit(Op::Call(fn_idx), line);
                            return Ok(());
                        }
                    }
                }
            }
            // `o?.m(args)`: a null receiver short-circuits — the args are NOT evaluated and the
            // method is NOT dispatched (the null-skip lowering jumps over the whole `access`).
            if *safe {
                return self.compile_safe_access(object, line, |c| {
                    for a in args {
                        c.expr(a)?;
                    }
                    let idx = c.field_name_index(name)?;
                    c.emit(Op::CallMethod(idx, args.len()), line);
                    Ok(())
                });
            }
            self.expr(object)?;
            for a in args {
                self.expr(a)?;
            }
            let idx = self.field_name_index(name)?;
            self.emit(Op::CallMethod(idx, args.len()), line);
            return Ok(());
        }
        // Inline lambda call: `(fn(int x) => x+1)(3)` or (after pipe lowering) `3 |> fn(int v) =>
        // v+10`. Compile the lambda expression to push a closure, then push args, then dispatch.
        if let Expr::Lambda {
            params,
            body,
            ret,
            span,
        } = callee
        {
            self.compile_lambda(params, body, ret.as_ref(), span.line)?;
            for a in args {
                self.expr(a)?;
            }
            self.emit(Op::CallValue(args.len()), line);
            return Ok(());
        }
        // A general expression that evaluates to a function value — `adder()(x)` (call a returned
        // closure), `fns[i](x)`, `(c ? f : g)(x)`. The checker has verified the callee is
        // function-typed, so compile it (pushes the closure), then the args, then dispatch via
        // `CallValue` — the same path a lambda local takes. Mirrors `interpreter::eval_call`.
        self.expr(callee)?;
        for a in args {
            self.expr(a)?;
        }
        self.emit(Op::CallValue(args.len()), line);
        Ok(())
    }

    /// Lower a `?.` access (field read or method call): evaluate `object`; if it is `null`, the
    /// whole access short-circuits to `null`; otherwise run `access`, which transforms the receiver
    /// on top of the stack into the member result. No new `Op` (decision S2-OPS): a scratch local
    /// peeks the receiver for the null test (the `$coalesce` trick from `??`), then a
    /// `JumpIfFalse`/`Jump` pair selects the path. Both paths leave exactly one value at the
    /// receiver's slot, so the static height is the receiver's height throughout.
    pub(super) fn compile_safe_access(
        &mut self,
        object: &Expr,
        line: u32,
        access: impl FnOnce(&mut Self) -> Result<(), String>,
    ) -> Result<(), String> {
        self.expr(object)?; // [.., recv]
                            // `recv`'s frame-relative slot (top of stack), NOT `locals.len()`: live transients (earlier
                            // interpolation segments, an enclosing `??`'s lhs, …) may sit below it. Mirrors
                            // `compile_match`'s `m_slot = self.height - 1`; addressed numerically, no `Local` entry.
        let slot = self.height - 1;
        self.emit(Op::GetLocal(slot), line); // [.., recv, recv]
        self.emit_const(Value::Null, line); // [.., recv, recv, null]
        self.emit(Op::Eq, line); // [.., recv, bool]
        let do_access = self.emit_jump(Op::JumpIfFalse(0), line); // [.., recv]; recv != null → access
        let to_end = self.emit_jump(Op::Jump(0), line); // recv == null → keep recv (= null), skip access
        self.patch_jump(do_access);
        let h = self.height;
        access(self)?; // [.., recv] -> [.., member]
        self.patch_jump(to_end);
        self.height = h; // both paths converge here with one value at the receiver's slot
        Ok(())
    }

    /// `inner!` checked force-unwrap (M3 S2.5). Evaluate the inner; a non-consuming null-test keeps
    /// the value when present, else raises `Op::Fault(ForceUnwrapNull)` — byte-identical to the
    /// interpreter's `"force-unwrap of null"` fault. No new `Op` (the fault op is the generalized
    /// `MatchFail`). `o! + 1` still specializes because `ctype(Force)` resolves the result's type.
    pub(super) fn compile_force(&mut self, inner: &Expr, line: u32) -> Result<(), String> {
        self.expr(inner)?; // [opt] — stays as the result when non-null
                           // `opt`'s frame-relative slot (top of stack), NOT `locals.len()`: transients may sit below
                           // it (e.g. `"{a!} {b!}"`). Mirrors `compile_match`. `ctype(Force)` handles operand typing of
                           // the *result*, so the scratch needs no `CTy`. Addressed numerically, no `Local` entry.
        let slot = self.height - 1;
        self.emit(Op::GetLocal(slot), line); // [opt, opt]
        self.emit_const(Value::Null, line); // [opt, opt, null]
        self.emit(Op::Eq, line); // [opt, opt == null]
        let ok = self.emit_jump(Op::JumpIfFalse(0), line); // [opt]; non-null → keep, skip the fault
        self.emit(Op::Fault(FaultMsg::ForceUnwrapNull), line); // null → clean fault (terminal)
        self.patch_jump(ok);
        Ok(())
    }

    /// `expr?` — Result-error propagation (M-faults 2a). Evaluate the operand; if it is `Err(_)`,
    /// `Op::Return` the whole `Err` value (`do_return` truncates to the frame base, so this mid-expression
    /// early-return is clean even nested); otherwise unwrap the `Ok` payload. No new `Op` — reuses
    /// `MatchTag`/`GetEnumField`/`Return`. The checker restricts `?` to a let-initializer, so the result
    /// (the `Ok` payload) is what the binding receives.
    pub(super) fn compile_propagate(&mut self, inner: &Expr, line: u32) -> Result<(), String> {
        self.expr(inner)?; // [.., r]
        let slot = self.height - 1; // r's frame-relative slot (transients may sit below it)
        let err_idx = self
            .variants
            .get("Err")
            .ok_or_else(|| {
                "`?` requires a Result-shaped enum (no `Err` variant in scope)".to_string()
            })?
            .index;
        self.emit(Op::GetLocal(slot), line); // [.., r, r]
        self.emit(Op::MatchTag(err_idx), line); // [.., r, isErr]
        let not_err = self.emit_jump(Op::JumpIfFalse(0), line); // pops isErr -> [.., r]
        self.emit(Op::Return, line); // Err: return r (do_return truncates the frame stack)
        self.patch_jump(not_err); // Ok path: [.., r]
        self.height = slot + 1; // reassert post-branch height (the terminal Return desynced the tracker)
        self.emit(Op::GetEnumField(0), line); // [.., ok_payload]
        Ok(())
    }

    /// Lower a fault intrinsic (`panic`/`todo`/`unreachable`/`assert`) to `Op::Fault` (M-faults 2a).
    /// Returns `true` if `name` was an intrinsic (and was compiled), `false` otherwise. Messages are
    /// compile-time literals (the checker enforces this), so they bake straight into the `FaultMsg` —
    /// no new `Op`. `panic`/`todo`/`unreachable` are `never`-typed: the trailing `self.height += 1`
    /// keeps the expression's "produces one value" contract for the (dead) code after the terminal
    /// `Op::Fault`. `assert` produces `unit` on the true path and faults on the false path.
    pub(super) fn compile_intrinsic(
        &mut self,
        name: &str,
        args: &[Expr],
        line: u32,
    ) -> Result<bool, String> {
        let base = self.height;
        match name {
            "panic" => {
                let msg = str_literal(args.first());
                self.emit(Op::Fault(FaultMsg::Panic(msg)), line);
                self.height = base + 1;
            }
            "todo" => {
                self.emit(Op::Fault(FaultMsg::Todo), line);
                self.height = base + 1;
            }
            "unreachable" => {
                self.emit(Op::Fault(FaultMsg::Unreachable), line);
                self.height = base + 1;
            }
            "assert" => {
                let msg = args
                    .get(1)
                    .map_or_else(String::new, |m| str_literal(Some(m)));
                self.expr(&args[0])?; // [.., cond]
                let to_fault = self.emit_jump(Op::JumpIfFalse(0), line); // false → fault (pops cond)
                self.emit_const(Value::Unit, line); // true: [.., unit]
                let to_end = self.emit_jump(Op::Jump(0), line);
                self.patch_jump(to_fault);
                self.emit(Op::Fault(FaultMsg::Assert(msg)), line);
                self.patch_jump(to_end);
                self.height = base + 1; // unit result
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    /// `obj with { f = e, … }` (M-mut.4a). Reconstruct a fresh instance: evaluate `obj` into a
    /// scratch slot, then push each descriptor field in order — the override expr if named, else a
    /// `GetField` re-read of the source — and `MakeInstance` (which runs **no** constructor body,
    /// Fork 2 = B). Collapse the new instance over the scratch slot so the expression leaves one
    /// value. No new `Op`. The checker proved `obj` is a concrete class and the names are its fields.
    pub(super) fn compile_clone_with(
        &mut self,
        object: &Expr,
        fields: &[(String, Expr)],
        line: u32,
    ) -> Result<(), String> {
        let class = match self.ctype(object)? {
            CTy::Class(name) => name,
            _ => return Err("`with` requires a class instance".into()),
        };
        let desc_idx = self
            .class_descs
            .iter()
            .position(|d| d.class == class)
            .ok_or_else(|| format!("unknown class `{class}` in `with`"))?;
        let field_names = self.class_descs[desc_idx].fields.clone();
        self.expr(object)?; // [.., src]
        let src_slot = self.height - 1; // numeric scratch (transients may sit below), like `compile_match`
        for fname in &field_names {
            if let Some((_, e)) = fields.iter().find(|(n, _)| n == fname) {
                self.expr(e)?; // [.., override]
            } else {
                self.emit(Op::GetLocal(src_slot), line); // [.., src]
                let idx = self.field_name_index(fname)?;
                self.emit(Op::GetField(idx), line); // [.., src.field]
            }
        }
        self.emit(Op::MakeInstance(desc_idx), line); // pops the fields → [.., src, newInstance]
        self.emit(Op::SetLocal(src_slot), line); // collapse newInstance over the scratch → [.., newInstance]
        Ok(())
    }

    /// Compile a `fn(params) => body` expression-body lambda (M3 S3 Task 4).
    ///
    /// Layout:
    ///   - Compute the lambda's free variables (sorted, deterministic — invariant #8).
    ///   - Filter out names that resolve to top-level functions (not captures).
    ///   - For each capture: emit `GetLocal(slot)` to push it onto the stack.
    ///   - Build a sub-`Function` with layout `[captures.., params..]`.
    ///     * The sub-compiler's locals start with the captures (in free-var order),
    ///       then the declared params — matching the frame layout `CallValue` sets up.
    ///   - Append the sub-`Function` to `self.extra_functions` and record its `n_captures`.
    ///   - Emit `Op::MakeClosure(fn_idx)` which pops the captures and pushes a `Value::Closure`.
    pub(super) fn compile_lambda(
        &mut self,
        params: &[Param],
        body: &LambdaBody,
        _ret: Option<&Type>,
        line: u32,
    ) -> Result<(), String> {
        // 1. Compute free variables of the lambda body.
        let all_free = free_vars(params, body);
        // 2. Filter to only variables that resolve to a local in the *enclosing* scope
        //    (names that are top-level functions are resolved statically at call time and
        //    don't need to be captured — `compile_call` handles them via `Op::Call`).
        let captures: Vec<(usize, String)> = all_free
            .into_iter()
            .filter_map(|name| {
                // Only capture locals; top-level functions, variants, and classes are not.
                self.resolve_local(&name)
                    .filter(|_| !self.fns.contains_key(&name))
                    .map(|slot| (slot, name))
            })
            .collect();
        // `this`-capture (Phase 1 closures slice): when the body references `this` (directly or
        // through a nested lambda) and we have a receiver in scope, capture the enclosing `this` as
        // an extra, *first* capture so it lands at the sub-frame's slot 0 — exactly where the
        // sub-compiler's `this_slot` will point. The receiver value is the live `Rc` instance handle,
        // so a field write through it is visible to the closure, matching the interpreter + PHP.
        let uses_this = self.this_slot.is_some() && crate::ast::lambda_uses_this(body);
        let n_captures = captures.len() + usize::from(uses_this);

        // 3. Build the sub-function's index in the global table.
        //    `base_fn_idx` is the start of this compilation's slice of the trailing lambda block;
        //    each lambda this compiler emits takes the next slot, hence `base + len`.
        let fn_idx = self.base_fn_idx + self.extra_functions.len();

        // 4. Build a sub-compiler for the lambda body.
        //    This lambda occupies global slot `fn_idx`; step 8 appends its nested lambdas
        //    *immediately after* it, so they start at `fn_idx + 1`. The sub-compiler therefore
        //    treats `fn_idx + 1` as the start of its own (nested) lambda slice.
        let sub_base = fn_idx + 1;
        let empty_fields: HashMap<String, CTy> = HashMap::new();
        // A lambda body cannot reference `this` or bare fields (checker enforces E-LAMBDA-THIS),
        // so we create the sub-compiler without field scope or a class context.
        let mut sub = Compiler::new(
            self.fns,
            self.arities,
            self.variants,
            self.enum_descs,
            self.classes,
            self.statics_index,
            self.consts_index,
            self.class_descs,
            self.names_index,
            &empty_fields,
            self.class_field_ctys,
            self.method_rets,
            self.method_generic_ret_from_param,
            self.reified_operands,
            self.methods,
            self.method_overloads,
            sub_base,
        );

        // 5. Seed the sub-compiler's locals: [this?, captures.., params..] — matching the frame
        //    layout `Op::CallValue` builds (the receiver, if captured, is pushed first below).
        if uses_this {
            // The receiver's operand type is its class, so `this.x + 1` specializes in the lambda
            // (without `cur_class`, `ctype(This)` would fail — the documented CTy-operand trap).
            let this_cty = self.cur_class.clone().map_or(CTy::Other, CTy::Class);
            sub.add_local("$this", this_cty);
            sub.this_slot = Some(0);
            sub.cur_class = self.cur_class.clone();
        }
        for (_, cap_name) in &captures {
            // The capture's type comes from the enclosing scope's local.
            let slot = self
                .resolve_local(cap_name)
                .expect("capture must resolve in enclosing scope");
            let ty = self.locals[slot].ty.clone();
            sub.add_local(cap_name, ty);
        }
        for p in params {
            sub.add_local(&p.name, resolve_cty(&p.ty));
        }
        sub.height = sub.locals.len();

        // 6. Compile the body. Expression-body: evaluate + explicit Return.
        match body {
            LambdaBody::Expr(e) => {
                sub.expr(e)?;
                sub.emit(Op::Return, line);
            }
            LambdaBody::Block(stmts) => {
                for s in stmts {
                    sub.stmt(s)?;
                }
                sub.emit_const(Value::Unit, line);
                sub.emit(Op::Return, line);
            }
        }

        // 7. Collect any nested lambdas compiled inside the sub-compiler.
        let mut nested_extras = sub.extra_functions;

        // 8. Build the sub-function and append it to our own extra_functions.
        let lambda_fn = Function {
            name: format!("<lambda@{line}>"),
            arity: n_captures + params.len(),
            n_captures,
            chunk: sub.chunk,
        };
        self.extra_functions.push(lambda_fn);
        self.lambda_n_captures.push(n_captures);
        // Drain nested extras: their indices follow this lambda in the table.
        self.extra_functions.append(&mut nested_extras);

        // 9. Push capture values onto the stack (enclosing scope), then emit MakeClosure.
        //    The receiver is pushed first (→ sub slot 0), then the free-var captures — matching the
        //    sub-compiler's local order above and the frame `Op::CallValue` rebuilds.
        if uses_this {
            self.emit(
                Op::GetLocal(self.this_slot.expect("uses_this implies a receiver slot")),
                line,
            );
        }
        for (slot, _) in &captures {
            self.emit(Op::GetLocal(*slot), line);
        }
        self.emit(Op::MakeClosure(fn_idx), line);
        Ok(())
    }
}
