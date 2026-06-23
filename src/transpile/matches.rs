//! PHP transpiler — matches (M-Decomp W4). See mod.rs for the struct + core + entry points.

use super::*;

impl Transpiler {
    /// Emit a `match` as an ordered `instanceof` chain. Each arm yields its body either as
    /// `return …;` or `$target = …;` depending on `target`. Payload vars bind positionally
    /// from the subclass's promoted props. A non-exhaustive chain ends with a defensive
    /// `throw` (the checker already guarantees exhaustiveness).
    pub(super) fn emit_match(
        &mut self,
        scrutinee: &Expr,
        arms: &[MatchArm],
        target: MatchTarget,
    ) -> Result<(), String> {
        let subj = self.emit_expr(scrutinee)?;
        let yield_stmt = |t: &MatchTarget, body: &str| match t {
            MatchTarget::Return => format!("return {body};"),
            MatchTarget::Assign(v) => format!("${v} = {body};"),
        };
        // Emit one `if (…) {…} elseif (…) {…} … else {…}` chain so exactly one arm runs. Earlier
        // this was a sequence of independent `if`s, which only short-circuited in `Return` position
        // (the `return` exits before the next `if`). In `Assign` position the arms fall through and
        // every subsequent `if` — and the defensive `throw` — was reached unconditionally; chaining
        // with `elseif`/`else` is correct for both targets. A catch-all (`_` / bare binding) is the
        // terminal `else`; otherwise a defensive `else { throw }` closes the (checker-exhaustive) set.
        let mut first = true;
        let mut has_catch_all = false;
        for arm in arms {
            // `if` for the first conditional arm, `elseif` thereafter; a catch-all uses `else` (or a
            // bare block when it is itself the first/only arm, since a leading `else` is invalid PHP).
            let cond_kw = if first { "if" } else { "elseif" };
            match &arm.pattern {
                Pattern::Variant {
                    name: vname,
                    fields: pats,
                    ..
                } => {
                    let props = self.variant_fields.get(vname).cloned().unwrap_or_default();
                    self.push_scope();
                    let mut binds = String::new();
                    for (i, fp) in pats.iter().enumerate() {
                        let bind_name = match fp {
                            Pattern::Binding { name, .. } => name,
                            _ => return Err(
                                "transpile error: only simple variable patterns are supported in match payloads".into()),
                        };
                        let prop = props
                            .get(i)
                            .ok_or("transpile error: variant pattern arity mismatch")?;
                        binds.push_str(&format!("${bind_name} = {subj}->{prop}; "));
                        self.declare(bind_name);
                    }
                    let vref = self.variant_ref(vname);
                    let body = self.emit_expr(&arm.body)?;
                    self.line(&format!(
                        "{cond_kw} ({subj} instanceof {vref}) {{ {binds}{} }}",
                        yield_stmt(&target, &body)
                    ));
                    self.pop_scope();
                    first = false;
                }
                Pattern::Wildcard(_) => {
                    has_catch_all = true;
                    let body = self.emit_expr(&arm.body)?;
                    let else_kw = if first { "" } else { "else " };
                    self.line(&format!("{else_kw}{{ {} }}", yield_stmt(&target, &body)));
                    first = false;
                }
                Pattern::Binding { name, .. } => {
                    // bare identifier arm binds the whole scrutinee (catch-all)
                    has_catch_all = true;
                    self.push_scope();
                    self.declare(name);
                    let body = self.emit_expr(&arm.body)?;
                    let else_kw = if first { "" } else { "else " };
                    self.line(&format!(
                        "{else_kw}{{ ${name} = {subj}; {} }}",
                        yield_stmt(&target, &body)
                    ));
                    self.pop_scope();
                    first = false;
                }
                // `null` arm over an optional scrutinee (M3 S2.6) → a `=== null` guard.
                Pattern::Null(_) => {
                    let body = self.emit_expr(&arm.body)?;
                    self.line(&format!(
                        "{cond_kw} ({subj} === null) {{ {} }}",
                        yield_stmt(&target, &body)
                    ));
                    first = false;
                }
                // Literal patterns (M11) — a `=== <literal>` guard, mirroring the interpreter's
                // exact value match (`match_pattern`: `v == n` / `v == x` / `v == s` / `v == b`).
                // PHP `===` is strict (type + value), so the branch taken is byte-identical.
                Pattern::Int(n, _) => {
                    let body = self.emit_expr(&arm.body)?;
                    self.line(&format!(
                        "{cond_kw} ({subj} === {n}) {{ {} }}",
                        yield_stmt(&target, &body)
                    ));
                    first = false;
                }
                Pattern::Float(x, _) => {
                    let body = self.emit_expr(&arm.body)?;
                    self.line(&format!(
                        "{cond_kw} ({subj} === {x:?}) {{ {} }}",
                        yield_stmt(&target, &body)
                    ));
                    first = false;
                }
                Pattern::Str(s, _) => {
                    let body = self.emit_expr(&arm.body)?;
                    self.line(&format!(
                        "{cond_kw} ({subj} === \"{}\") {{ {} }}",
                        php_escape(s),
                        yield_stmt(&target, &body)
                    ));
                    first = false;
                }
                Pattern::Bool(b, _) => {
                    let body = self.emit_expr(&arm.body)?;
                    self.line(&format!(
                        "{cond_kw} ({subj} === {b}) {{ {} }}",
                        yield_stmt(&target, &body)
                    ));
                    first = false;
                }
                // M-RT S4 type pattern → a PHP `instanceof` guard, binding the narrowed value. The
                // type name uses `php_type_ref` (FQN if cross-package), mirroring `Expr::InstanceOf`.
                Pattern::Type {
                    type_name, binding, ..
                } => {
                    self.push_scope();
                    let bind = match binding {
                        Some(name) => {
                            self.declare(name);
                            format!("${name} = {subj}; ")
                        }
                        None => String::new(),
                    };
                    let body = self.emit_expr(&arm.body)?;
                    // M-RT S6c.3: a match type-pattern against a decomposed MI ancestor tests `I<name>`.
                    let tref = self.type_pos_ref(type_name);
                    self.line(&format!(
                        "{cond_kw} ({subj} instanceof {tref}) {{ {bind}{} }}",
                        yield_stmt(&target, &body)
                    ));
                    self.pop_scope();
                    first = false;
                }
            }
        }
        if !has_catch_all {
            // Defensive terminal arm: the checker guarantees exhaustiveness, so this is unreachable
            // in well-typed programs — but as the chain's `else` it must never fall through to the
            // assignment/return below it (the former independent-`if` form let it run unconditionally
            // in `Assign` position). `first` is only still true for an arm-less match (checker-forbidden).
            let else_kw = if first { "" } else { "else " };
            self.line(&format!(
                "{else_kw}{{ throw new \\UnhandledMatchError(); }}"
            ));
        }
        Ok(())
    }
}
