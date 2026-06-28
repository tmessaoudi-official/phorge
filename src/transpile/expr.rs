//! PHP transpiler — expr (M-Decomp W4). See mod.rs for the struct + core + entry points.

use super::*;

impl Transpiler {
    pub(super) fn emit_expr(&mut self, e: &Expr) -> Result<String, String> {
        match e {
            Expr::Int(n, _) => Ok(n.to_string()),
            Expr::Float(x, _) => Ok(format!("{x:?}")), // 12.0 -> "12.0"
            // A `decimal` literal `19.99d` → a PHP string literal `"19.99"` (BCMath operates on
            // strings; M-NUM S1). The rendered form carries exactly `scale` fractional digits, so a
            // `(string)`-of a BCMath result of the same value is identical (the byte-identity contract).
            Expr::Decimal { unscaled, scale, .. } => {
                Ok(format!("\"{}\"", crate::value::fmt_decimal(*unscaled, *scale)))
            }
            Expr::Bool(b, _) => Ok(if *b { "true".into() } else { "false".into() }),
            Expr::Ident(name, _) => Ok(self.resolve_ident(name)),
            Expr::This(_) => Ok("$this".into()),
            Expr::Unary { op, expr, .. } => {
                let inner = self.emit_expr(expr)?;
                let sym = match op {
                    UnaryOp::Neg => "-",
                    UnaryOp::Not => "!",
                    UnaryOp::BitNot => "~",
                };
                // Wrap a compound operand so the unary binds only to it (P0-2 — `-(a + b)`, `!(a && b)`).
                let inner = Self::paren_if_compound(expr, inner);
                Ok(format!("{sym}{inner}"))
            }
            Expr::Binary { op, lhs, rhs, .. } => {
                let l = self.emit_expr(lhs)?;
                let r = self.emit_expr(rhs)?;
                let bs = if self.namespaced { "\\" } else { "" };
                // Decimal `+ - *` (M-NUM S1): route to the BCMath `__phorj_dec_*` helpers, which
                // derive each operand's scale at PHP-runtime, compute the result scale (add/sub = max,
                // mul = sum), call `bcadd`/`bcsub`/`bcmul`, then bounds-check the result against i128
                // range and `throw` the same `decimal overflow` fault as the Rust kernels. A mixed
                // `decimal op int` stringifies the int operand first (a decimal is a PHP string; the
                // int isn't). Checked FIRST (before the `+`/`-`/`*` native-operator paths below), since
                // a decimal operand's kind is neither `Str` nor `Int`/`Float`, so those would
                // mis-route it. Detected when EITHER operand's kind is `Decimal`.
                if matches!(
                    op,
                    BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Rem | BinaryOp::Div
                ) {
                    let (lk, rk) = (self.expr_kind(lhs), self.expr_kind(rhs));
                    if lk == OpKind::Decimal || rk == OpKind::Decimal {
                        let ls = if lk == OpKind::Decimal {
                            l.clone()
                        } else {
                            format!("(string)({l})")
                        };
                        let rs = if rk == OpKind::Decimal {
                            r.clone()
                        } else {
                            format!("(string)({r})")
                        };
                        let helper = match op {
                            BinaryOp::Add => {
                                self.uses_dec_add = true;
                                "__phorj_dec_add"
                            }
                            BinaryOp::Sub => {
                                self.uses_dec_sub = true;
                                "__phorj_dec_sub"
                            }
                            BinaryOp::Mul => {
                                self.uses_dec_mul = true;
                                "__phorj_dec_mul"
                            }
                            // Exact decimal `%` (2026-06-27): `bcmod` at `max(scales)`, zero divisor
                            // throws (matching the Rust `decimal_rem` fault).
                            BinaryOp::Rem => {
                                self.uses_dec_rem = true;
                                "__phorj_dec_rem"
                            }
                            // Exact-or-fault decimal `/` (2026-06-27): bcdiv + exactness check + strip
                            // to minimal form; non-terminating or zero divisor throws.
                            BinaryOp::Div => {
                                self.uses_dec_div_exact = true;
                                "__phorj_dec_div_exact"
                            }
                            _ => unreachable!("matched Add/Sub/Mul/Rem/Div above"),
                        };
                        return Ok(format!("{bs}{helper}({ls}, {rs})"));
                    }
                }
                // T6: `/`, `%`, `+` are type-driven in Phorj (PHP's `/` is always float, `%` always
                // integer, `+` numeric-only with `.` for concat). When the operand kind is statically
                // known (`expr_kind`), emit the native PHP operator directly; otherwise fall back to
                // the runtime helper that branches at PHP-runtime (the checker guarantees both
                // operands share a type, so the helper's single-operand test is exact). The helper is
                // the safe fallback — a kind that can't be pinned down never produces a wrong operator.
                if matches!(op, BinaryOp::Div) {
                    // Compound operands need grouping parens; `intdiv(…)`'s args do not.
                    return Ok(match self.arith_kind(lhs, rhs) {
                        OpKind::Int => format!("intdiv({l}, {r})"),
                        OpKind::Float => {
                            let (l, r) = (Self::paren_if_compound(lhs, l), Self::paren_if_compound(rhs, r));
                            format!("{l} / {r}")
                        }
                        _ => {
                            self.uses_div = true;
                            format!("{bs}__phorj_div({l}, {r})")
                        }
                    });
                }
                if matches!(op, BinaryOp::Rem) {
                    return Ok(match self.arith_kind(lhs, rhs) {
                        OpKind::Int => {
                            let (l, r) = (Self::paren_if_compound(lhs, l), Self::paren_if_compound(rhs, r));
                            format!("{l} % {r}")
                        }
                        // Float `%` routes through `__phorj_rem` (not a bare `fmod`) so a zero divisor
                        // *throws* — PHP `fmod($x, 0.0)` returns `NAN`, but Phorj faults on any
                        // division by zero, so the helper guards `$b == 0` before the `fmod`.
                        OpKind::Float => {
                            self.uses_rem = true;
                            format!("{bs}__phorj_rem({l}, {r})")
                        }
                        _ => {
                            self.uses_rem = true;
                            format!("{bs}__phorj_rem({l}, {r})")
                        }
                    });
                }
                if matches!(op, BinaryOp::Add) {
                    let (lk, rk) = (self.expr_kind(lhs), self.expr_kind(rhs));
                    // `string + string` → `.`; numeric → `+`; unknown → the `is_string`-branching helper.
                    let native = if lk == OpKind::Str || rk == OpKind::Str {
                        Some(".")
                    } else if matches!(lk, OpKind::Int | OpKind::Float)
                        || matches!(rk, OpKind::Int | OpKind::Float)
                    {
                        Some("+")
                    } else {
                        None
                    };
                    return Ok(match native {
                        Some(sym) => {
                            let (l, r) = (Self::paren_if_compound(lhs, l), Self::paren_if_compound(rhs, r));
                            format!("{l} {sym} {r}")
                        }
                        None => {
                            self.uses_add = true;
                            format!("{bs}__phorj_add({l}, {r})")
                        }
                    });
                }
                if matches!(op, BinaryOp::Coalesce) {
                    // `??` binds loosely in PHP; parenthesize to preserve grouping.
                    return Ok(format!("({l} ?? {r})"));
                }
                // Preserve operand grouping: a compound operand is parenthesized so PHP precedence
                // cannot re-associate it (P0-2 — `a - (b - c)` must not flatten to `a - b - c`).
                let l = Self::paren_if_compound(lhs, l);
                let r = Self::paren_if_compound(rhs, r);
                Ok(format!("{l} {} {r}", Self::binop(op)))
            }
            // `value instanceof TypeName` → PHP `$value instanceof TypeName` (M-RT S1). The operand
            // is parenthesized if compound (PHP `instanceof` binds tighter than `!`/`&&`), and the
            // class name is emitted bare — single-package programs are flat, and a cross-package type
            // is rejected upstream (E-PKG-TYPE) until a later slice.
            Expr::InstanceOf {
                value, type_name, ..
            } => {
                let v = self.emit_expr(value)?;
                let v = Self::paren_if_compound(value, v);
                // M-RT S6c.3: against a decomposed MI ancestor, test its interface `I<name>` — the
                // subtype `implements I<name>` (it does not `extends <name>`).
                Ok(format!("{v} instanceof {}", self.type_pos_ref(type_name)))
            }
            // `value as TypeName` → the checked downcast (M4 casting axis 2), result `TypeName?`.
            // Lowered to an arrow-fn IIFE so `value` is evaluated EXACTLY ONCE (a naive
            // `$v instanceof T ? $v : null` would double-evaluate a side-effecting scrutinee like
            // `f() as T` and diverge from the run/runvm backends). The `$__as` parameter is local to
            // the arrow fn, so nested casts don't collide.
            Expr::Cast {
                value, type_name, ..
            } => {
                // M4 as-matrix: a primitive `as` CONVERSION was rewritten to a native call before the
                // transpiler; the only primitive `Cast` reaching here is the **identity** (`T as T`) —
                // emit the value unchanged.
                if matches!(
                    type_name.as_str(),
                    "int" | "float" | "string" | "bool" | "decimal"
                ) {
                    return self.emit_expr(value);
                }
                let v = self.emit_expr(value)?;
                Ok(format!(
                    "(fn($__as) => $__as instanceof {} ? $__as : null)({v})",
                    self.type_pos_ref(type_name)
                ))
            }
            Expr::List(items, _) => {
                let parts: Result<Vec<_>, _> = items.iter().map(|i| self.emit_expr(i)).collect();
                Ok(format!("[{}]", parts?.join(", ")))
            }
            // A map literal → a PHP `[k => v, …]` array (insertion-ordered, like Phorj), M-RT S3.
            Expr::Map(pairs, _) => {
                let mut parts = Vec::with_capacity(pairs.len());
                for (k, v) in pairs {
                    parts.push(format!("{} => {}", self.emit_expr(k)?, self.emit_expr(v)?));
                }
                Ok(format!("[{}]", parts.join(", ")))
            }
            Expr::Null(_) => Ok("null".into()),
            Expr::Index { object, index, .. } => {
                let o = self.emit_expr(object)?;
                let i = self.emit_expr(index)?;
                Ok(format!("{o}[{i}]"))
            }
            Expr::Str(parts, _) => self.emit_string(parts),
            Expr::Bytes(b, _) => Ok(format!("\"{}\"", php_escape_bytes(b))),
            Expr::Call { callee, args, .. } => self.emit_call(callee, args),
            Expr::Member {
                object, name, safe, ..
            } => {
                // A `const` class constant `ClassName.NAME` → PHP `ClassName::NAME` (Feature A),
                // checked before the static-field `::$name` path.
                if !*safe {
                    if let Some(s) = self.const_ref(object, name) {
                        return Ok(s);
                    }
                    // Static read `ClassName.field` → PHP `ClassName::$field` (M-mut.7).
                    if let Some(s) = self.static_ref(object, name) {
                        return Ok(s);
                    }
                }
                let o = self.emit_expr(object)?;
                let arrow = if *safe { "?->" } else { "->" };
                Ok(format!("{o}{arrow}{name}"))
            }
            // `inner!` → PHP 8.0's null-coalescing throw expression: `($v ?? throw new …)`. `??`
            // evaluates the receiver once and throws iff it is null — exactly the old
            // `__phorj_unwrap` helper, now inline with no runtime function (M3 S2.5). The null-fault
            // message differs from the Phorj backends' (no name/line) — a documented transpile-only
            // divergence (KNOWN_ISSUES); the present-value case is exact. `\RuntimeException` is
            // already fully qualified, so this is identical in flat and namespaced mode.
            Expr::Force { inner, .. } => {
                let v = self.emit_expr(inner)?;
                Ok(format!(
                    "({v} ?? throw new \\RuntimeException(\"force-unwrap of null\"))"
                ))
            }
            // `?` propagation is hoisted at the `VarDecl` statement level (the only position the checker
            // permits, M-faults 2a), so it never reaches expression emission in a valid program.
            Expr::Propagate { .. } => Err(
                "internal: `?` propagation reached expression emission (checker restricts it to a let-initializer)"
                    .to_string(),
            ),
            // `obj with { f = e }` → a fresh instance with the named fields overridden and the
            // constructor bypassed — byte-identical to the backends (M-mut.4a). An empty override list
            // is `clone($obj)` (valid since PHP 5). A non-empty list emits PHP 8.5's native two-arg
            // `clone($o, ['f' => e, …])` (the transpile floor is 8.5): it clones, applies the property
            // overrides, bypasses the constructor and honors `__clone` — exactly the backends'
            // shallow clone-then-override. (Pre-8.5 this needed the `__phorj_clone_with` helper.)
            Expr::CloneWith { object, fields, .. } => {
                let o = self.emit_expr(object)?;
                if fields.is_empty() {
                    return Ok(format!("clone({o})"));
                }
                let mut pairs = Vec::with_capacity(fields.len());
                for (name, e) in fields {
                    let v = self.emit_expr(e)?;
                    pairs.push(format!("'{name}' => {v}"));
                }
                Ok(format!("clone({o}, [{}])", pairs.join(", ")))
            }
            // Expression-position `match` (M11): wrap the SAME if-chain `emit_match` produces in
            // statement position inside an immediately-invoked closure, so both positions share one
            // lowering and cannot diverge. Over-capture every enclosing local by value via `use(…)`
            // — Phorj values are immutable, so by-value capture is exact; pattern-bound payload vars
            // are declared *inside* the closure and so are intentionally excluded. A regular closure
            // auto-binds `$this`, so a match inside a method keeps working.
            Expr::Match {
                scrutinee, arms, ..
            } => {
                // T1: a literal value `match` is a native PHP `match` expression (parenthesized so it
                // composes inside any larger expression) — no IIFE. The if-chain/IIFE below remains
                // for variant/type/struct/guarded matches (`try_native_match` returns `None`).
                if let Some(m) = self.try_native_match(scrutinee, arms)? {
                    return Ok(format!("({m})"));
                }
                // T2: variant/type/struct/guarded matches → native `match (true) { … }` (parenthesized
                // to compose). Retires the IIFE in expression position. Only a non-terminal catch-all
                // falls through to the IIFE below (`try_match_true` returns `None`).
                if let Some(m) = self.try_match_true(scrutinee, arms)? {
                    return Ok(format!("({m})"));
                }
                let captures: BTreeSet<String> = self.locals.iter().flatten().cloned().collect();
                let use_clause = if captures.is_empty() {
                    String::new()
                } else {
                    let names: Vec<String> = captures.iter().map(|n| format!("${n}")).collect();
                    format!(" use ({})", names.join(", "))
                };
                // Render the if-chain into a temporary buffer (one indent level deep), then splice it
                // into the closure body. Save/restore `out` and `indent` so the surrounding emission
                // is untouched.
                let saved_out = std::mem::take(&mut self.out);
                let saved_indent = self.indent;
                self.indent = 1;
                let chain = self.emit_match(scrutinee, arms, MatchTarget::Return);
                let body = std::mem::replace(&mut self.out, saved_out);
                self.indent = saved_indent;
                chain?;
                Ok(format!("(function(){use_clause} {{\n{body}}})()"))
            }
            // `__phorj_range` reproduces Phorj's range semantics under PHP: an empty/reversed range
            // (`start > hi`) yields `[]`, where PHP's bare `range()` would *descend* (QW-13 — formerly
            // a transpile-only divergence). The `run`/`runvm` backends were always byte-identical.
            Expr::Range {
                start,
                end,
                inclusive,
                ..
            } => {
                let s = self.emit_expr(start)?;
                let e = self.emit_expr(end)?;
                self.uses_range = true;
                let bs = if self.namespaced { "\\" } else { "" };
                Ok(format!(
                    "{bs}__phorj_range({s}, {e}, {})",
                    if *inclusive { "true" } else { "false" }
                ))
            }
            // Expression `if` → a PHP ternary (the idiomatic conditional expression, the TS→JS
            // analogue); parenthesized so it composes safely inside any larger expression.
            Expr::If {
                cond,
                then_expr,
                else_expr,
                ..
            } => {
                let c = self.emit_expr(cond)?;
                let t = self.emit_expr(then_expr)?;
                let e = self.emit_expr(else_expr)?;
                Ok(format!("({c} ? {t} : {e})"))
            }
            // Expression-body lambda → PHP arrow function (auto by-value capture — no explicit
            // `use` clause needed).
            // Statement-body lambda → PHP `function($x) use ($cap, ...) { … }` (by-value capture
            // with an explicit `use` clause listing only captured enclosing locals).
            Expr::Lambda { params, body, .. } => {
                let ps = params
                    .iter()
                    .map(|p| format!("${}", p.name))
                    .collect::<Vec<_>>()
                    .join(", ");
                match body {
                    LambdaBody::Expr(e) => {
                        // T6: type the params in a fresh scope so arithmetic in the arrow body
                        // specializes (`fn(int a, int b) => a + b` → `$a + $b`).
                        self.push_scope();
                        for p in params {
                            self.declare(&p.name);
                            self.declare_kind(&p.name, kind_of_type(&p.ty));
                        }
                        let body_php = self.emit_expr(e)?;
                        self.pop_scope();
                        Ok(format!("fn({ps}) => {body_php}"))
                    }
                    LambdaBody::Block(stmts) => {
                        // Compute captures: free variables that are enclosing locals, not
                        // top-level function names, variants, or classes.
                        let caps: Vec<String> = crate::ast::free_vars(params, body)
                            .into_iter()
                            .filter(|n| {
                                self.is_local(n)
                                    && !self.funcs.contains(n)
                                    && !self.variants.contains(n)
                                    && !self.classes.contains(n)
                            })
                            .map(|n| format!("${n}"))
                            .collect();
                        let use_clause = if caps.is_empty() {
                            String::new()
                        } else {
                            format!(" use ({})", caps.join(", "))
                        };
                        // Emit the block body into a temporary buffer (swapping `self.out`)
                        // so `emit_stmt` can write indented lines, then collect them as the
                        // inline closure body. Params and captures are declared in a fresh
                        // scope so inner expressions resolve them correctly.
                        let saved_out = std::mem::take(&mut self.out);
                        let saved_indent = self.indent;
                        self.indent = 0;
                        self.push_scope();
                        // Declare captures first (so params can shadow same-named captures).
                        for cap in &caps {
                            // Strip the leading `$` to get the bare name.
                            self.declare(&cap[1..]);
                        }
                        for p in params {
                            self.declare(&p.name);
                            self.declare_kind(&p.name, kind_of_type(&p.ty));
                        }
                        for s in stmts {
                            self.emit_stmt(s)?;
                        }
                        self.pop_scope();
                        self.indent = saved_indent;
                        let body_php = std::mem::replace(&mut self.out, saved_out);
                        // The body_php has one "line" per statement (each ends with '\n' from
                        // `self.line()`). Trim trailing whitespace and join with spaces for a
                        // compact inline representation.
                        let body_php = body_php
                            .lines()
                            .map(|l| l.trim())
                            .filter(|l| !l.is_empty())
                            .collect::<Vec<_>>()
                            .join(" ");
                        Ok(format!("function({ps}){use_clause} {{ {body_php} }}"))
                    }
                }
            }
            // `html"…"` literals are erased to `html.concat([…])` kernel calls by
            // `checker::resolve_html` before transpilation; the transpiler never sees one.
            Expr::Html(..) => unreachable!("html literal not resolved before transpilation"),
            Expr::New(..) => unreachable!("Expr::New is unwrapped before transpilation (checker::unwrap_new)"),
        }
    }

    /// Emit an interpolated string literal as a PHP concatenation of quoted literal chunks
    /// and parenthesized expressions. Always-correct (avoids PHP's interpolation limits,
    /// e.g. free function calls inside `"{…}"`).
    pub(super) fn emit_string(&mut self, parts: &[StrPart]) -> Result<String, String> {
        if parts.is_empty() {
            return Ok("\"\"".into());
        }
        // B-1: build native PHP interpolation. Literals and *embeddable* holes accumulate into one
        // open `"…"` chunk (holes as `{$…}`); a non-embeddable hole flushes the chunk and concatenates
        // its type-directed coercion (the pre-B-1 path), so mixed strings stay maximally idiomatic.
        let mut chunks: Vec<String> = Vec::new();
        let mut buf = String::new();
        let mut buf_open = false;
        for p in parts {
            match p {
                StrPart::Literal(s) => {
                    buf.push_str(&php_escape_interp(s));
                    buf_open = true;
                }
                StrPart::Expr(e) => {
                    let code = self.emit_expr(e)?;
                    if self.interp_embeddable(e, &code) {
                        buf.push('{');
                        buf.push_str(&code);
                        buf.push('}');
                        buf_open = true;
                    } else {
                        if buf_open {
                            chunks.push(format!("\"{buf}\""));
                            buf.clear();
                            buf_open = false;
                        }
                        chunks.push(self.coerce_hole_concat(e, code));
                    }
                }
            }
        }
        if buf_open {
            chunks.push(format!("\"{buf}\""));
        }
        Ok(chunks.join(" . "))
    }

    /// Can interpolation hole `e` (already emitted as `code`) embed as a native PHP `{$…}` segment?
    /// True iff (1) its kind is `Str`/`Int` — the only kinds whose PHP interpolation byte-matches our
    /// coercion (`bool`→`1`/``, `float`→precision-14, objects→error all diverge); (2) it is a
    /// `$`-rooted access chain (PHP forbids a top-level operator inside `{$…}` — verified: parse
    /// error); (3) the emitted code is actually `$`-rooted (excludes module/class-rooted members like
    /// `\Foo\bar()`); and (4) it carries no brace that could prematurely close the `{…}`.
    fn interp_embeddable(&mut self, e: &Expr, code: &str) -> bool {
        matches!(self.expr_kind(e), OpKind::Str | OpKind::Int)
            && Self::is_php_interp_chain(e)
            && code.starts_with('$')
            && !code.contains('{')
            && !code.contains('}')
    }

    /// A `$`-rooted PHP access chain: an identifier/`this`, member/index access over such, or a
    /// *method* call (a `Call` whose callee is a member chain — a free-function call is `f(…)`, not
    /// `$`-rooted). Everything else (operators, literals, `new`, ranges, lambdas, …) is not.
    fn is_php_interp_chain(e: &Expr) -> bool {
        match e {
            Expr::Ident(..) | Expr::This(..) => true,
            Expr::Member { object, .. } | Expr::Index { object, .. } => {
                Self::is_php_interp_chain(object)
            }
            Expr::Call { callee, .. } => {
                matches!(callee.as_ref(), Expr::Member { .. }) && Self::is_php_interp_chain(callee)
            }
            _ => false,
        }
    }

    /// Coerce a non-embeddable interpolation hole to a string for concatenation — the pre-B-1
    /// type-directed path: `string` as-is · `int` → `(string)` · `bool` → ternary · `float` →
    /// `__phorj_float` (Ryū, irreducible) · class/list/map/unknown → the `__phorj_str` dispatch.
    fn coerce_hole_concat(&mut self, e: &Expr, code: String) -> String {
        let bs = if self.namespaced { "\\" } else { "" };
        match self.expr_kind(e) {
            // A `decimal` value is already a PHP string (its rendered form) — concatenate it directly,
            // exactly like a `string` (M-NUM S1). `as_display` of a `Value::Decimal` is the same form.
            OpKind::Str | OpKind::Decimal => Self::paren_if_compound(e, code),
            OpKind::Int => format!("(string){}", Self::paren_if_compound(e, code)),
            OpKind::Bool => format!(
                "({} ? \"true\" : \"false\")",
                Self::paren_if_compound(e, code)
            ),
            OpKind::Float => {
                self.uses_float = true;
                format!("{bs}__phorj_float({code})")
            }
            OpKind::Class(_) | OpKind::List(_) | OpKind::Map(..) | OpKind::Other => {
                self.uses_str = true;
                format!("{bs}__phorj_str({code})")
            }
        }
    }

    /// A PHP reference to an enum variant subclass: fully-qualified when its enum lives in a package
    /// namespace (`new \Acme\Geometry\Circle(…)`, an `instanceof` against it), bare for a `package
    /// main` enum (`Circle`) — byte-identical to the pre-lift output for a single-package program.
    pub(super) fn variant_ref(&self, variant: &str) -> String {
        // Mangle a PHP-reserved variant class name (`Int`→`Int_`) identically to `emit_enum`'s
        // declaration, so construction (`new Int_`) and `instanceof Int_` reference the real class.
        let mangled = super::php_variant_name(variant);
        match self.variant_ns.get(variant) {
            Some(ns) if ns != "Main" => format!("\\{ns}\\{mangled}"),
            _ => mangled,
        }
    }

    /// A PHP "primary" expression: emits self-contained, so it never needs wrapping parens when used
    /// as an operand. Compound expressions (`Binary`/`Unary`/`If`/`Match`/`Lambda`) are NOT primary
    /// and get parenthesized by `paren_if_compound` (P0-2). `Force`/`Range`/`Call`/`Member`/`Index`
    /// emit as `__phorj_unwrap(…)` / `__phorj_range(…)` / `f(…)` / `$o->x` / `$o[$i]` — all primary.
    pub(super) fn is_primary(e: &Expr) -> bool {
        matches!(
            e,
            Expr::Int(..)
                | Expr::Float(..)
                | Expr::Bool(..)
                | Expr::Str(..)
                | Expr::Bytes(..)
                | Expr::Ident(..)
                | Expr::This(..)
                | Expr::Null(..)
                | Expr::Call { .. }
                | Expr::Member { .. }
                | Expr::Index { .. }
                | Expr::Force { .. }
                | Expr::Range { .. }
                | Expr::List(..)
                | Expr::Map(..)
        )
    }

    /// Parenthesize an operand's emitted `code` when the operand is compound, so PHP operator
    /// precedence cannot re-associate it (P0-2). Conservatively over-parenthesizes — correctness
    /// over minimal parens; a precedence-table refinement is a deferred polish.
    pub(super) fn paren_if_compound(e: &Expr, code: String) -> String {
        if Self::is_primary(e) {
            code
        } else {
            format!("({code})")
        }
    }

    pub(super) fn binop(op: &BinaryOp) -> &'static str {
        use BinaryOp::*;
        match op {
            Sub => "-",
            Mul => "*",
            // `**` power: PHP's native `**` is also right-associative and binds tighter than unary
            // minus, but the caller parenthesizes compound operands (`paren_if_compound`), so the
            // emitted `(a) ** (b)` preserves Phorj's grouping exactly. PHP `**` returns `int` for
            // int operands / `float` for floats — matching the type-directed Phorj result.
            Pow => "**",
            // `+`, `/`, `%` are routed through `__phorj_add`/`__phorj_div`/`__phorj_rem` before
            // binop() (`+` is string-concat-overloaded, P0-1/P0-4 for the others).
            Add => unreachable!("Add handled via __phorj_add before binop()"),
            Div => unreachable!("Div handled via __phorj_div before binop()"),
            Rem => unreachable!("Rem handled via __phorj_rem before binop()"),
            Eq => "==",
            NotEq => "!=",
            Lt => "<",
            Le => "<=",
            Gt => ">",
            Ge => ">=",
            And => "&&",
            Or => "||",
            // Bitwise (primitives P2) — PHP's integer-native operators match the value kernels 1:1
            // (no runtime helper needed). Compound operands are parenthesized by the caller.
            BitAnd => "&",
            BitOr => "|",
            BitXor => "^",
            Shl => "<<",
            Shr => ">>",
            // `??` is parenthesized at the call site, so it never reaches `binop()`.
            Coalesce => unreachable!("Coalesce handled before binop()"),
            Pipe => unreachable!("`|>` is lowered to a call in the parser"),
        }
    }

    pub(super) fn resolve_ident(&self, name: &str) -> String {
        if self.is_local(name) {
            format!("${name}")
        } else if self
            .cur_class_fields
            .as_ref()
            .is_some_and(|f| f.contains(name))
        {
            format!("$this->{name}")
        } else if self.funcs.contains(name) {
            // Bare named-function reference in value position — PHP 8.1 first-class callable.
            // Reuses the same FQN logic as emit_call: cross-package mangled names get a leading `\`.
            if self.namespaced && name.contains('\\') {
                format!("\\{name}(...)")
            } else {
                format!("{name}(...)")
            }
        } else {
            format!("${name}") // best-effort; the checker guarantees resolution
        }
    }
}
