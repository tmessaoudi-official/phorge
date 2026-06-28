//! PHP transpiler — matches (M-Decomp W4). See mod.rs for the struct + core + entry points.

use super::*;

/// A pattern's lowering: the boolean `tests` to conjoin (empty = unconditional) and the
/// `(php-var, access-expr)` bindings it introduces. Returned by `classify_pattern`.
type Classified = (Vec<String>, Vec<(String, String)>);

impl Transpiler {
    /// T1: a *value* `match` whose arms are all literals (`int`/`float`/`string`/`bool`/`null`),
    /// plus at most one trailing catch-all (a wildcard `_` or a bare binding) and no `when` guards,
    /// lowers to a native PHP `match` expression — the idiomatic, modern form, replacing the verbose
    /// `if/elseif` chain (and, in expression position, the IIFE). Returns the full
    /// `match (subject) { … }` string, or `None` if any arm is a variant/type/struct pattern, carries
    /// a guard, or a catch-all isn't the last arm — those keep the `instanceof` `emit_match` path.
    ///
    /// Strict equality: PHP `match` compares arm values with `===`, exactly mirroring Phorj's literal
    /// patterns (`classify_pattern` already emits `=== <lit>`), so the branch taken is byte-identical.
    /// A bare-binding catch-all is assigned *inside the subject* (`match ($x = E)`), so the `default`
    /// arm body can reference it while `E` is evaluated once — and this works in both statement and
    /// expression position (no preceding statement needed). An exhaustive literal set (no catch-all)
    /// emits no `default`; PHP then throws `\UnhandledMatchError` on the unreachable no-match, the same
    /// defensive behavior as the if-chain's terminal `throw`.
    pub(super) fn try_native_match(
        &mut self,
        scrutinee: &Expr,
        arms: &[MatchArm],
    ) -> Result<Option<String>, String> {
        // Eligibility scan (no emission): every arm is a literal, except at most one *trailing*
        // catch-all. A guard, or a catch-all that isn't last (which would let later arms run under
        // PHP's position-independent `default` — diverging from Phorj's first-match-wins), bails out.
        let mut catch_all_binding: Option<&str> = None;
        for (i, arm) in arms.iter().enumerate() {
            if arm.guard.is_some() {
                return Ok(None);
            }
            match &arm.pattern {
                Pattern::Int(..)
                | Pattern::Float(..)
                | Pattern::Str(..)
                | Pattern::Bool(..)
                | Pattern::Null(_) => {}
                Pattern::Wildcard(_) => {
                    if i != arms.len() - 1 {
                        return Ok(None);
                    }
                }
                Pattern::Binding { name, .. } => {
                    if i != arms.len() - 1 {
                        return Ok(None);
                    }
                    catch_all_binding = Some(name);
                }
                // Variant/type/struct → `instanceof` path (T2/the if-chain). A decimal pattern is
                // scale-insensitive numeric equality (`1.5d` matches `1.50d`), which a PHP `switch`
                // (strict-ish `===` over its cases) would NOT honor — so route it to the if-chain,
                // which emits an explicit loose `==` test (M-NUM S1).
                Pattern::Variant { .. }
                | Pattern::Type { .. }
                | Pattern::Struct { .. }
                | Pattern::Decimal { .. } => {
                    return Ok(None);
                }
            }
        }
        // Eligible: emit. A fresh scope holds the optional catch-all binding (so the body resolves it
        // to `$name`); the binding is assigned in the subject so it's live in the `default` arm.
        let subj = self.emit_expr(scrutinee)?;
        self.push_scope();
        let subject = match catch_all_binding {
            Some(name) => {
                self.declare(name);
                // T6b: the catch-all binds the scrutinee value, so it shares its operand kind
                // (`match n { 0 => …, x => x + 1 }` — `x` is `n`'s kind).
                let k = self.expr_kind(scrutinee);
                self.declare_kind(name, k);
                format!("${name} = {subj}")
            }
            None => subj,
        };
        let mut out = format!("match ({subject}) {{");
        for arm in arms {
            let label = match &arm.pattern {
                Pattern::Int(n, _) => format!("{n}"),
                Pattern::Float(x, _) => format!("{x:?}"),
                Pattern::Str(s, _) => format!("\"{}\"", php_escape(s)),
                Pattern::Bool(b, _) => format!("{b}"),
                Pattern::Null(_) => "null".to_string(),
                Pattern::Wildcard(_) | Pattern::Binding { .. } => "default".to_string(),
                // Eligibility scan already excluded these.
                _ => unreachable!("non-literal arm survived the native-match eligibility scan"),
            };
            let body = self.emit_expr(&arm.body)?;
            out.push_str(&format!(" {label} => {body},"));
        }
        out.push_str(" }");
        self.pop_scope();
        Ok(Some(out))
    }

    /// T2: lower any `match` to a native PHP `match (true) { <cond> => <body>, … }` expression, for
    /// use in *expression* position (replacing the IIFE). Each arm's pattern is classified into the
    /// same boolean `tests` + `(var, access)` bindings the if-chain uses; the bindings ride into the
    /// condition as `(($x = access) || true)` conjuncts (the proven guarded-arm technique), so the
    /// arm body — a PHP expression — can reference them without a preceding statement. A `when` guard
    /// appends `&& (guard)`. An unguarded wildcard catch-all has an empty condition → `default`; an
    /// unguarded bare-binding catch-all becomes `(($x = subj) || true)` (always true, and binds).
    ///
    /// Statement-position matches keep the `if/elseif` chain (`emit_match`): PHP `if` is a statement
    /// and `match` is an expression, so each position uses PHP's natural construct. Returns `None`
    /// only when an unguarded catch-all isn't the last arm — PHP's `default` is position-independent,
    /// so a non-terminal catch-all would diverge from Phorj's first-match-wins (caller keeps the IIFE).
    pub(super) fn try_match_true(
        &mut self,
        scrutinee: &Expr,
        arms: &[MatchArm],
    ) -> Result<Option<String>, String> {
        for (i, arm) in arms.iter().enumerate() {
            let is_catch_all =
                matches!(arm.pattern, Pattern::Wildcard(_) | Pattern::Binding { .. })
                    && arm.guard.is_none();
            if is_catch_all && i != arms.len() - 1 {
                return Ok(None);
            }
        }
        let subj = self.emit_expr(scrutinee)?;
        let mut out = String::from("match (true) {");
        for arm in arms {
            self.push_scope();
            let (tests, binds) = self.classify_pattern(&arm.pattern, &subj)?;
            for (name, _) in &binds {
                self.declare(name);
            }
            // T6b: top-level catch-all binding takes the scrutinee's operand kind.
            if let Pattern::Binding { name, .. } = &arm.pattern {
                let k = self.expr_kind(scrutinee);
                self.declare_kind(name, k);
            }
            let guard = match &arm.guard {
                Some(g) => Some(self.emit_expr(g)?),
                None => None,
            };
            let body = self.emit_expr(&arm.body)?;
            let mut parts: Vec<String> = tests.clone();
            for (name, access) in &binds {
                parts.push(format!("((${name} = {access}) || true)"));
            }
            if let Some(g) = &guard {
                parts.push(format!("({g})"));
            }
            let label = if parts.is_empty() {
                "default".to_string()
            } else {
                parts.join(" && ")
            };
            out.push_str(&format!(" {label} => {body},"));
            self.pop_scope();
        }
        out.push_str(" }");
        Ok(Some(out))
    }

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
        // T1: a literal value `match` becomes a native PHP `match` expression in `return`/assign
        // position. Falls through to the `instanceof` if-chain below for variant/type/struct/guarded
        // matches (`try_native_match` returns `None` without emitting the scrutinee in that case).
        if let Some(m) = self.try_native_match(scrutinee, arms)? {
            let stmt = match &target {
                MatchTarget::Return => format!("return {m};"),
                MatchTarget::Assign(v) => format!("${v} = {m};"),
            };
            self.line(&stmt);
            return Ok(());
        }
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
            // T6b: a top-level catch-all binding takes the scrutinee's operand kind (nested
            // variant/struct/type bindings are kinded inside `classify_pattern`).
            if let Pattern::Binding { name, .. } = &arm.pattern {
                let k = self.expr_kind(scrutinee);
                self.declare_kind(name, k);
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
            // A decimal pattern is scale-insensitive numeric equality (`1.5d` matches `1.50d`). A
            // decimal value is a PHP string; PHP's LOOSE `==` on two numeric strings compares them
            // numerically (`"1.5" == "1.50"` is true), exactly mirroring the `eq_val` decimal arm the
            // interpreter/VM use — so `==` (not `===`) is the byte-identity-correct test here.
            Pattern::Decimal {
                unscaled, scale, ..
            } => (
                vec![format!(
                    "{subj} == \"{}\"",
                    crate::value::fmt_decimal(*unscaled, *scale)
                )],
                Vec::new(),
            ),
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
                    Some(name) => {
                        // T6b: the narrowed binding is an instance of the tested type → field reads
                        // on it resolve.
                        self.declare_kind(name, OpKind::Class(type_name.clone()));
                        vec![(name.clone(), subj.to_string())]
                    }
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
                let kinds = self
                    .variant_field_kinds
                    .get(vname)
                    .cloned()
                    .unwrap_or_default();
                let mut tests = vec![format!("{subj} instanceof {}", self.variant_ref(vname))];
                let mut binds = Vec::new();
                for (i, fp) in pats.iter().enumerate() {
                    let prop = props
                        .get(i)
                        .ok_or("transpile error: variant pattern arity mismatch")?;
                    // T6b: a direct payload binding takes the variant field's declared kind.
                    if let Pattern::Binding { name, .. } = fp {
                        if let Some(k) = kinds.get(i) {
                            self.declare_kind(name, k.clone());
                        }
                    }
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
                    // T6b: a direct field binding takes that class field's declared kind.
                    if let Pattern::Binding { name, .. } = &fp.pat {
                        let k = self.lookup_field_kind(type_name, &fp.field);
                        self.declare_kind(name, k);
                    }
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
