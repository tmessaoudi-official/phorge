//! PHP transpiler — matches (M-Decomp W4). See mod.rs for the struct + core + entry points.

use super::*;

/// A pattern's lowering: the boolean `tests` to conjoin (empty = unconditional) and the
/// `(php-var, access-expr)` bindings it introduces. Returned by `classify_pattern`.
type Classified = (Vec<String>, Vec<(String, String)>);

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
            // `if` for the first conditional arm, `elseif` thereafter; an unguarded catch-all uses
            // `else` (or a bare block when first, since a leading `else` is invalid PHP). A `when`
            // guard turns even a `_`/binding arm conditional (it may fall through).
            let cond_kw = if first { "if" } else { "elseif" };
            self.push_scope();
            // Classify the pattern (recursively) into a list of boolean `tests` (joined by `&&`; an
            // empty list = matches unconditionally, i.e. a catch-all) and the bindings it introduces,
            // as `(var, access-expr)` pairs.
            let (tests, binds) = self.classify_pattern(&arm.pattern, &subj)?;
            // Declare the bindings so the guard and body can reference them.
            for (name, _) in &binds {
                self.declare(name);
            }
            let guard = match &arm.guard {
                Some(g) => Some(self.emit_expr(g)?),
                None => None,
            };
            let body = self.emit_expr(&arm.body)?;
            let yield_s = yield_stmt(&target, &body);
            match guard {
                // Guarded arm: never a catch-all. The guard needs its bindings live, so the binds
                // move into the condition as always-true assignment conjuncts (`(($x = E) || true)`,
                // safe for any value incl. falsy), ahead of the parenthesized guard. The body carries
                // no binds — the condition already assigned them (PHP vars are function-scoped).
                Some(g) => {
                    let mut parts: Vec<String> = tests.clone();
                    for (name, access) in &binds {
                        let lhs = format!("${name}");
                        parts.push(format!("(({lhs} = {access}) || true)"));
                    }
                    parts.push(format!("({g})"));
                    self.line(&format!(
                        "{cond_kw} ({}) {{ {yield_s} }}",
                        parts.join(" && ")
                    ));
                }
                // Unguarded arm: byte-identical to the pre-guard emission.
                None => {
                    let body_binds: String = binds
                        .iter()
                        .map(|(name, access)| format!("${name} = {access}; "))
                        .collect();
                    if tests.is_empty() {
                        has_catch_all = true;
                        let else_kw = if first { "" } else { "else " };
                        self.line(&format!("{else_kw}{{ {body_binds}{yield_s} }}"));
                    } else {
                        self.line(&format!(
                            "{cond_kw} ({}) {{ {body_binds}{yield_s} }}",
                            tests.join(" && ")
                        ));
                    }
                }
            }
            self.pop_scope();
            first = false;
        }
        // (classify_pattern is defined below.)
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

    /// Recursively lower a pattern against a PHP subject expression `subj` into a list of boolean
    /// `tests` (conjoined with `&&` at the call site; empty = unconditional) and `(var, access)`
    /// bindings. Nesting composes: a struct/variant field recurses with `subj->field` as its subject,
    /// so `Line { from: Point { x, y }, to }` yields an `instanceof` per struct level plus a bind per
    /// leaf — mirroring the interpreter's recursive `match_pattern` and the compiler's path walk.
    fn classify_pattern(&mut self, pat: &Pattern, subj: &str) -> Result<Classified, String> {
        Ok(match pat {
            Pattern::Wildcard(_) => (Vec::new(), Vec::new()),
            Pattern::Binding { name, .. } => (Vec::new(), vec![(name.clone(), subj.to_string())]),
            // `null` arm over an optional scrutinee (M3 S2.6) → a strict `=== null` test.
            Pattern::Null(_) => (vec![format!("{subj} === null")], Vec::new()),
            // Literal patterns — a strict `=== <literal>` test (type + value), mirroring the
            // interpreter's exact value match so the branch taken is byte-identical.
            Pattern::Int(n, _) => (vec![format!("{subj} === {n}")], Vec::new()),
            Pattern::Float(x, _) => (vec![format!("{subj} === {x:?}")], Vec::new()),
            Pattern::Str(s, _) => (
                vec![format!("{subj} === \"{}\"", php_escape(s))],
                Vec::new(),
            ),
            Pattern::Bool(b, _) => (vec![format!("{subj} === {b}")], Vec::new()),
            // M-RT S4 type pattern → an `instanceof` test, binding the narrowed value. M-RT S6c.3: a
            // decomposed-MI ancestor tests `I<name>` via `type_pos_ref`.
            Pattern::Type {
                type_name, binding, ..
            } => {
                let tref = self.type_pos_ref(type_name);
                let binds = match binding {
                    Some(name) => vec![(name.clone(), subj.to_string())],
                    None => Vec::new(),
                };
                (vec![format!("{subj} instanceof {tref}")], binds)
            }
            Pattern::Variant {
                name: vname,
                fields: pats,
                ..
            } => {
                let props = self.variant_fields.get(vname).cloned().unwrap_or_default();
                let mut tests = vec![format!("{subj} instanceof {}", self.variant_ref(vname))];
                let mut binds = Vec::new();
                for (i, fp) in pats.iter().enumerate() {
                    let prop = props
                        .get(i)
                        .ok_or("transpile error: variant pattern arity mismatch")?;
                    let (t, b) = self.classify_pattern(fp, &format!("{subj}->{prop}"))?;
                    tests.extend(t);
                    binds.extend(b);
                }
                (tests, binds)
            }
            // S5.2 struct pattern → an `instanceof` test plus each field's sub-pattern against
            // `subj->field` (the promoted property keeps the field's name).
            Pattern::Struct {
                type_name, fields, ..
            } => {
                let tref = self.type_pos_ref(type_name);
                let mut tests = vec![format!("{subj} instanceof {tref}")];
                let mut binds = Vec::new();
                for fp in fields {
                    let (t, b) =
                        self.classify_pattern(&fp.pat, &format!("{subj}->{}", fp.field))?;
                    tests.extend(t);
                    binds.extend(b);
                }
                (tests, binds)
            }
        })
    }
}
