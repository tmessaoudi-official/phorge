//! Bytecode compiler — matches (M-Decomp W4.1). See compiler/mod.rs for the struct,
//! emission/scope core, and the (kept-whole) `stack_effect`.

use super::*;

impl Compiler<'_> {
    /// `match scrutinee { pat => body, … }` as an expression (decision P4-7). The scrutinee is
    /// evaluated once and spilled to a hidden `$match` slot; each arm tests its pattern (skipping
    /// to the next arm on mismatch), binds payloads by re-extraction, then leaves its body's single
    /// value on the stack. A matched arm jumps past the rest to a collapse that overwrites the
    /// scrutinee slot with the result — so the whole `match` leaves exactly one value.
    pub(super) fn compile_match(
        &mut self,
        scrutinee: &Expr,
        arms: &[MatchArm],
        line: u32,
    ) -> Result<(), String> {
        // Class-aware type of the scrutinee, for a catch-all binding's type (best-effort: an
        // unresolvable scrutinee collapses to `Other`, which `as_num` rejects as an operand anyway).
        // A class-typed scrutinee's catch-all binding keeps its class, so `x.field` resolves (Wave 4).
        let scrut_cty = self.ctype(scrutinee).unwrap_or(CTy::Other);
        self.expr(scrutinee)?;
        let m_slot = self.height - 1; // scrutinee now on top: its base-relative slot
        let mut end_jumps = Vec::new();
        for arm in arms {
            self.height = m_slot + 1; // each arm dispatches with just the scrutinee live
            let mut skips = Vec::new();
            self.emit_pattern_test(&arm.pattern, m_slot, &[], &mut skips, line)?;
            let n_before = self.match_bindings.len();
            self.register_bindings(&arm.pattern, m_slot, &[], scrut_cty.clone())?;
            // An arm guard runs after binding (its bindings are live); a false guard skips to the
            // next arm exactly like a pattern mismatch — its `JumpIfFalse` joins `skips`. `JumpIfFalse`
            // pops the bool, so `emit`'s stack_effect leaves height at the arm-relative base.
            if let Some(g) = &arm.guard {
                self.expr(g)?; // -> [.., scrutinee, bool]
                skips.push(self.emit_jump(Op::JumpIfFalse(0), line));
            }
            self.expr(&arm.body)?; // -> [.., scrutinee, result]
            self.match_bindings.truncate(n_before);
            end_jumps.push(self.emit_jump(Op::Jump(0), line));
            for j in skips {
                self.patch_jump(j); // a mismatch lands at the next arm
            }
        }
        self.emit(Op::Fault(FaultMsg::NonExhaustiveMatch), line); // checker-unreachable backstop (EV-7 parity)
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
    pub(super) fn emit_pattern_test(
        &mut self,
        pat: &Pattern,
        m_slot: usize,
        path: &[PathSeg],
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
                // M3 S2.6: the arm matches iff the scrutinee is `null` — a literal `Eq null` test
                // (interpreter parity, `match_pattern`). `eq_val` defines `(Null, Null) => true`.
                self.emit_literal_test(m_slot, path, Value::Null, skips, line);
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
                    sub.push(PathSeg::Enum(i));
                    self.emit_pattern_test(fp, m_slot, &sub, skips, line)?;
                }
            }
            // M-RT S4 type pattern: the arm matches iff the sub-value is an instance of `type_name`
            // — the SAME runtime test as `instanceof`, reusing `Op::IsInstance` (no new op). Mirrors
            // the Variant flow: load the value, test, skip on a false result.
            Pattern::Type { type_name, .. } => {
                self.emit_load_path(m_slot, path, line);
                self.emit(Op::IsInstance(type_name.clone()), line);
                skips.push(self.emit_jump(Op::JumpIfFalse(0), line));
            }
            // S5.2 struct pattern: `instanceof` test (reusing `Op::IsInstance`, no new op), then each
            // field's sub-pattern tests against `path + Field(field)`. A binding sub-pattern emits no
            // test here — the value is re-extracted lazily on use (so an unused binding never reads
            // the field); a literal/nested-struct sub-pattern reads the field to compare/recurse.
            Pattern::Struct {
                type_name, fields, ..
            } => {
                self.emit_load_path(m_slot, path, line);
                self.emit(Op::IsInstance(type_name.clone()), line);
                skips.push(self.emit_jump(Op::JumpIfFalse(0), line));
                for fp in fields {
                    let idx = self.field_name_index(&fp.field)?;
                    let mut sub = path.to_vec();
                    sub.push(PathSeg::Field(idx));
                    self.emit_pattern_test(&fp.pat, m_slot, &sub, skips, line)?;
                }
            }
        }
        Ok(())
    }

    /// Load the `$match` sub-value at `path`, compare it to `lit`, and skip the arm on inequality.
    pub(super) fn emit_literal_test(
        &mut self,
        m_slot: usize,
        path: &[PathSeg],
        lit: Value,
        skips: &mut Vec<usize>,
        line: u32,
    ) {
        self.emit_load_path(m_slot, path, line);
        self.emit_const(lit, line);
        self.emit(Op::Eq, line);
        skips.push(self.emit_jump(Op::JumpIfFalse(0), line));
    }

    /// Register (emitting no code) every binding introduced by `pat`, so the arm body can
    /// re-extract them. `cur_ty` is the class-aware type of the value `pat` matches (for `ctype`) —
    /// a class-typed payload keeps its class, so `binding.field` resolves (Wave 4).
    pub(super) fn register_bindings(
        &mut self,
        pat: &Pattern,
        m_slot: usize,
        path: &[PathSeg],
        cur_ty: CTy,
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
                    sub.push(PathSeg::Enum(i));
                    let ty = field_tags.get(i).cloned().unwrap_or(CTy::Other);
                    self.register_bindings(fp, m_slot, &sub, ty)?;
                }
            }
            // S5.2 struct pattern: each field binds (or sub-binds) at `path + Field(field)`. The
            // field's CTy comes from the program-wide class-field table so a struct-bound int is a
            // first-class arithmetic operand on the VM (`Point { x } => x + 1`), the operand trap.
            Pattern::Struct {
                type_name, fields, ..
            } => {
                for fp in fields {
                    let idx = self.field_name_index(&fp.field)?;
                    let mut sub = path.to_vec();
                    sub.push(PathSeg::Field(idx));
                    let ty = self
                        .class_field_ctys
                        .get(type_name)
                        .and_then(|m| m.get(&fp.field))
                        .cloned()
                        .unwrap_or(CTy::Other);
                    self.register_bindings(&fp.pat, m_slot, &sub, ty)?;
                }
            }
            // M-RT S4 type pattern: bind the matched value (the whole sub-value at `path`) as the
            // narrowed class, so `binding.field` resolves (the Wave-4 class-aware operand path).
            Pattern::Type {
                type_name,
                binding: Some(name),
                ..
            } => self.match_bindings.push(MatchBinding {
                name: name.clone(),
                match_slot: m_slot,
                path: path.to_vec(),
                ty: CTy::Class(type_name.clone()),
            }),
            _ => {} // wildcard / literals / `Type _` bind nothing
        }
        Ok(())
    }
}
