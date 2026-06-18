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

struct Transpiler {
    funcs: HashSet<String>,
    classes: HashSet<String>,
    variants: HashSet<String>,
    variant_fields: HashMap<String, Vec<String>>,
    out: String,
    indent: usize,
    locals: Vec<HashSet<String>>,
    cur_class_fields: Option<HashSet<String>>,
    /// Active import map (leaf qualifier → full dotted module path) — how a namespaced native call
    /// `console.println(x)` is distinguished from a method call on a value (M3 Wave 1). The
    /// transpiler tracks no variable scope, so unlike the interpreter/compiler it cannot use a
    /// locals-first heuristic; the import map is the authority.
    imports: HashMap<String, String>,
    /// Set when an `opt!` force-unwrap is emitted, so the `__phorge_unwrap` helper is defined once
    /// per file (PHP hoists top-level function declarations, so its position is immaterial).
    uses_force: bool,
    /// True when the program carries mangled (`\`-bearing) names — a multi-package project (M5 S2c).
    /// Switches emission from the flat single-package form to one `namespace …{}` brace-block per
    /// package + a nameless bootstrap block, and forces fully-qualified (leading-`\`) call emission.
    namespaced: bool,
}

/// Where a `match` expression's arm values flow: a `return` or an assignment to `$name`.
enum MatchTarget {
    Return,
    Assign(String),
}

/// The PHP namespace of a (possibly mangled) function name: the prefix before the last `\`
/// (`Acme\Util\compute` ⇒ `Acme\Util`), or `Main` for a bare name (the `main` package).
fn namespace_of(name: &str) -> String {
    match name.rfind('\\') {
        Some(i) => name[..i].to_string(),
        None => "Main".to_string(),
    }
}

/// The trailing segment of a mangled name (`Acme\Util\compute` ⇒ `compute`), used as the function's
/// declared name inside its `namespace` block. A bare name is returned unchanged.
fn last_segment(name: &str) -> &str {
    name.rsplit('\\').next().unwrap_or(name)
}

/// Whether a native's PHP erasure is a global function call (`strlen(...)`, `str_replace(...)`) — an
/// identifier immediately followed by `(`. Such calls need a leading `\` inside a namespace block so
/// they resolve to the global PHP builtin, not `CurrentNs\strlen`. A language construct like
/// `echo … . "\n"` (`console.println`) is not a function call and is left alone (M5-8).
fn looks_like_global_call(s: &str) -> bool {
    let mut chars = s.char_indices();
    match chars.next() {
        Some((_, c)) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    for (_, c) in chars {
        if c == '(' {
            return true;
        }
        if !(c.is_ascii_alphanumeric() || c == '_') {
            return false;
        }
    }
    false
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
            imports: HashMap::new(),
            uses_force: false,
            namespaced: false,
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
                        self.variant_fields.insert(
                            v.name.clone(),
                            v.fields.iter().map(|p| p.name.clone()).collect(),
                        );
                    }
                }
                Item::Import { path, .. } => {
                    if let Some(leaf) = path.last() {
                        self.imports.insert(leaf.clone(), path.join("."));
                    }
                }
                // Aliases are expanded out of the AST before transpiling; arm only for exhaustiveness.
                Item::TypeAlias { .. } => {}
            }
        }
    }

    fn emit_program(&mut self, program: &Program) -> Result<(), String> {
        // A mangled (`\`-bearing) top-level name means a multi-package project (M5 S2c): switch to
        // the brace-namespace form. A single-package program (every existing example) has no `\`
        // names and stays on the flat path — byte-identical to today's output.
        self.namespaced = program
            .items
            .iter()
            .any(|it| matches!(it, Item::Function(f) if f.name.contains('\\')));
        if self.namespaced {
            return self.emit_program_namespaced(program);
        }
        self.out.push_str("<?php\n");
        for item in &program.items {
            match item {
                Item::Import { .. } => {}
                Item::Function(f) => self.emit_function(f, false)?,
                Item::Enum(e) => self.emit_enum(e)?,
                Item::Class(c) => self.emit_class(c)?,
                // Aliases are expanded out of the AST before transpiling; arm only for exhaustiveness.
                Item::TypeAlias { .. } => {}
            }
        }
        // The interpreter auto-invokes `main`; PHP does not. Emit the call so the output
        // is a runnable program, not just definitions.
        if self.funcs.contains("main") {
            self.line("main();");
        }
        // The `opt!` runtime helper, defined once when used. PHP hoists top-level function
        // declarations, so emitting it after `main();` is still callable from any body (M3 S2.5).
        if self.uses_force {
            self.emit_force_helper();
        }
        Ok(())
    }

    /// Multi-package emission (M5 S2c, M5-7): one `namespace …{}` brace-block per package, then a
    /// nameless `namespace {}` block that bootstraps `\Main\main()` and holds the global `opt!`
    /// helper. A function's namespace is its mangled prefix (`Acme\Util\compute` ⇒ `Acme\Util`);
    /// bare names (the `main` package) and all enums/classes (library types are rejected, so types
    /// are `main`-only) land in `Main`. The bootstrap block is emitted last so every package's
    /// functions are already declared when it runs.
    fn emit_program_namespaced(&mut self, program: &Program) -> Result<(), String> {
        use std::collections::BTreeMap;
        self.out.push_str("<?php\n");
        let mut buckets: BTreeMap<String, Vec<&Item>> = BTreeMap::new();
        for item in &program.items {
            let ns = match item {
                Item::Function(f) => namespace_of(&f.name),
                Item::Enum(_) | Item::Class(_) => "Main".to_string(),
                _ => continue,
            };
            buckets.entry(ns).or_default().push(item);
        }
        for (ns, items) in &buckets {
            self.line(&format!("namespace {ns} {{"));
            self.indent += 1;
            for item in items {
                match item {
                    Item::Function(f) => self.emit_function(f, false)?,
                    Item::Enum(e) => self.emit_enum(e)?,
                    Item::Class(c) => self.emit_class(c)?,
                    _ => {}
                }
            }
            self.indent -= 1;
            self.line("}");
        }
        self.line("namespace {");
        self.indent += 1;
        if self.funcs.contains("main") {
            self.line("\\Main\\main();");
        }
        if self.uses_force {
            self.emit_force_helper();
        }
        self.indent -= 1;
        self.line("}");
        Ok(())
    }

    /// The `opt!` runtime helper. In flat mode it is a top-level global; in namespaced mode it is
    /// emitted inside the nameless block (so its fully-qualified name is `\__phorge_unwrap`, which
    /// the `Expr::Force` call site emits).
    fn emit_force_helper(&mut self) {
        self.line("function __phorge_unwrap($v) {");
        self.indent += 1;
        self.line("if ($v === null) { throw new \\RuntimeException(\"force-unwrap of null\"); }");
        self.line("return $v;");
        self.indent -= 1;
        self.line("}");
    }

    /// Indentation-aware line writer.
    fn line(&mut self, s: &str) {
        for _ in 0..self.indent {
            self.out.push_str("    ");
        }
        self.out.push_str(s);
        self.out.push('\n');
    }

    fn push_scope(&mut self) {
        self.locals.push(HashSet::new());
    }
    fn pop_scope(&mut self) {
        self.locals.pop();
    }
    fn declare(&mut self, name: &str) {
        if let Some(s) = self.locals.last_mut() {
            s.insert(name.to_string());
        }
    }
    fn is_local(&self, name: &str) -> bool {
        self.locals.iter().any(|s| s.contains(name))
    }

    fn emit_type(ty: &Type) -> String {
        match ty {
            Type::Named { name, .. } => match name.as_str() {
                "int" => "int".into(),
                "float" => "float".into(),
                "bool" => "bool".into(),
                "string" => "string".into(),
                "List" | "Map" | "Set" => "array".into(),
                other => other.to_string(), // enum / class name
            },
            // Optional types are a deferred corner the checker already rejects; be defensive.
            _ => "mixed".into(),
        }
    }

    fn ret_hint(ret: &Option<Type>) -> String {
        match ret {
            Some(t) => Self::emit_type(t),
            None => "void".into(),
        }
    }

    fn emit_function(&mut self, f: &FunctionDecl, is_method: bool) -> Result<(), String> {
        let params: Vec<String> = f
            .params
            .iter()
            .map(|p| format!("{} ${}", Self::emit_type(&p.ty), p.name))
            .collect();
        // In namespaced mode a top-level function is declared inside its `namespace` block, so emit
        // only its trailing segment (`Acme\Util\compute` ⇒ `compute`). Methods keep their name.
        let disp = if self.namespaced && !is_method {
            last_segment(&f.name)
        } else {
            &f.name
        };
        self.line(&format!(
            "function {}({}): {} {{",
            disp,
            params.join(", "),
            Self::ret_hint(&f.ret)
        ));
        self.indent += 1;
        self.push_scope();
        for p in &f.params {
            self.declare(&p.name);
        }
        for s in &f.body {
            self.emit_stmt(s)?;
        }
        self.pop_scope();
        self.indent -= 1;
        self.line("}");
        Ok(())
    }

    /// An enum with payload variants becomes an abstract base class plus one `final`
    /// subclass per variant, with promoted public props for the payload fields.
    fn emit_enum(&mut self, e: &EnumDecl) -> Result<(), String> {
        self.line(&format!("abstract class {} {{}}", e.name));
        for v in &e.variants {
            self.line(&format!("final class {} extends {} {{", v.name, e.name));
            self.indent += 1;
            if !v.fields.is_empty() {
                let props: Vec<String> = v
                    .fields
                    .iter()
                    .map(|p| format!("public {} ${}", Self::emit_type(&p.ty), p.name))
                    .collect();
                self.line(&format!(
                    "public function __construct({}) {{}}",
                    props.join(", ")
                ));
            }
            self.indent -= 1;
            self.line("}");
        }
        Ok(())
    }

    fn emit_class(&mut self, c: &ClassDecl) -> Result<(), String> {
        // Names of ctor params that PHP will promote to properties.
        let mut promoted_names: HashSet<String> = HashSet::new();
        for m in &c.members {
            if let ClassMember::Constructor { params, .. } = m {
                for p in params {
                    if is_promoted(&p.modifiers) {
                        promoted_names.insert(p.name.clone());
                    }
                }
            }
        }
        // Field set for `$this->` resolution = explicit decls + promoted ctor params
        // (mirrors the checker's `collect_class`).
        let mut fields: HashSet<String> = promoted_names.clone();
        for m in &c.members {
            if let ClassMember::Field { name, .. } = m {
                fields.insert(name.clone());
            }
        }
        self.line(&format!("class {} {{", c.name));
        self.indent += 1;
        let prev = self.cur_class_fields.replace(fields);
        for m in &c.members {
            match m {
                ClassMember::Field {
                    modifiers,
                    ty,
                    name,
                    ..
                } => {
                    // A field that is ALSO a promoted ctor param is declared by the
                    // promotion — emitting it again is a PHP "redeclare" fatal.
                    if promoted_names.contains(name) {
                        continue;
                    }
                    self.line(&format!(
                        "{} {} ${name};",
                        vis(modifiers),
                        Self::emit_type(ty)
                    ));
                }
                ClassMember::Constructor { params, body, .. } => {
                    let ps: Vec<String> = params
                        .iter()
                        .map(|p| {
                            let v = vis(&p.modifiers);
                            if v.is_empty() {
                                format!("{} ${}", Self::emit_type(&p.ty), p.name)
                            } else {
                                format!("{} {} ${}", v, Self::emit_type(&p.ty), p.name)
                            }
                        })
                        .collect();
                    if body.is_empty() {
                        self.line(&format!("function __construct({}) {{}}", ps.join(", ")));
                    } else {
                        self.line(&format!("function __construct({}) {{", ps.join(", ")));
                        self.indent += 1;
                        self.push_scope();
                        for p in params {
                            self.declare(&p.name);
                        }
                        for s in body {
                            self.emit_stmt(s)?;
                        }
                        self.pop_scope();
                        self.indent -= 1;
                        self.line("}");
                    }
                }
                ClassMember::Method(f) => self.emit_function(f, true)?,
            }
        }
        self.cur_class_fields = prev;
        self.indent -= 1;
        self.line("}");
        Ok(())
    }

    fn emit_stmt(&mut self, s: &Stmt) -> Result<(), String> {
        match s {
            // `match` is handled at statement granularity (return / var-decl-init position).
            // These specific arms must precede the generic VarDecl/Return arms.
            Stmt::Return {
                value: Some(Expr::Match {
                    scrutinee, arms, ..
                }),
                ..
            } => {
                self.emit_match(scrutinee, arms, MatchTarget::Return)?;
            }
            Stmt::VarDecl {
                name,
                init: Expr::Match {
                    scrutinee, arms, ..
                },
                ..
            } => {
                self.declare(name);
                self.emit_match(scrutinee, arms, MatchTarget::Assign(name.clone()))?;
            }
            Stmt::VarDecl { name, init, .. } => {
                let e = self.emit_expr(init)?;
                self.declare(name);
                self.line(&format!("${name} = {e};"));
            }
            Stmt::Return { value, .. } => match value {
                Some(e) => {
                    let s = self.emit_expr(e)?;
                    self.line(&format!("return {s};"));
                }
                None => self.line("return;"),
            },
            Stmt::If {
                cond,
                bind,
                then_block,
                else_block,
                ..
            } => {
                let c = self.emit_expr(cond)?;
                // `if (var x = opt)` → PHP `if (($x = <scrutinee>) !== null)`: the assignment-in-
                // condition binds `$x` and the `!== null` test mirrors the optional narrowing.
                match bind {
                    Some(name) => self.line(&format!("if ((${name} = {c}) !== null) {{")),
                    None => self.line(&format!("if ({c}) {{")),
                }
                self.indent += 1;
                self.push_scope();
                if let Some(name) = bind {
                    self.declare(name);
                }
                for st in then_block {
                    self.emit_stmt(st)?;
                }
                self.pop_scope();
                self.indent -= 1;
                if let Some(eb) = else_block {
                    self.line("} else {");
                    self.indent += 1;
                    self.push_scope();
                    for st in eb {
                        self.emit_stmt(st)?;
                    }
                    self.pop_scope();
                    self.indent -= 1;
                }
                self.line("}");
            }
            Stmt::For {
                name, iter, body, ..
            } => {
                let it = self.emit_expr(iter)?;
                self.line(&format!("foreach ({it} as ${name}) {{"));
                self.indent += 1;
                self.push_scope();
                self.declare(name);
                for st in body {
                    self.emit_stmt(st)?;
                }
                self.pop_scope();
                self.indent -= 1;
                self.line("}");
            }
            Stmt::Block(stmts, _) => {
                self.line("{");
                self.indent += 1;
                self.push_scope();
                for st in stmts {
                    self.emit_stmt(st)?;
                }
                self.pop_scope();
                self.indent -= 1;
                self.line("}");
            }
            Stmt::Expr(e, _) => {
                let s = self.emit_expr(e)?;
                self.line(&format!("{s};"));
            }
        }
        Ok(())
    }

    fn emit_expr(&mut self, e: &Expr) -> Result<String, String> {
        match e {
            Expr::Int(n, _) => Ok(n.to_string()),
            Expr::Float(x, _) => Ok(format!("{x:?}")), // 12.0 -> "12.0"
            Expr::Bool(b, _) => Ok(if *b { "true".into() } else { "false".into() }),
            Expr::Ident(name, _) => Ok(self.resolve_ident(name)),
            Expr::This(_) => Ok("$this".into()),
            Expr::Unary { op, expr, .. } => {
                let inner = self.emit_expr(expr)?;
                let sym = match op {
                    UnaryOp::Neg => "-",
                    UnaryOp::Not => "!",
                };
                Ok(format!("{sym}{inner}"))
            }
            Expr::Binary { op, lhs, rhs, .. } => {
                if matches!(op, BinaryOp::Is | BinaryOp::Pipe) {
                    return Err("transpile error: `is`/`|>` operators are not yet supported".into());
                }
                let l = self.emit_expr(lhs)?;
                let r = self.emit_expr(rhs)?;
                if matches!(op, BinaryOp::Coalesce) {
                    // `??` binds loosely in PHP; parenthesize to preserve grouping.
                    return Ok(format!("({l} ?? {r})"));
                }
                Ok(format!("{l} {} {r}", Self::binop(op)))
            }
            Expr::List(items, _) => {
                let parts: Result<Vec<_>, _> = items.iter().map(|i| self.emit_expr(i)).collect();
                Ok(format!("[{}]", parts?.join(", ")))
            }
            Expr::Null(_) => Ok("null".into()),
            Expr::Index { object, index, .. } => {
                let o = self.emit_expr(object)?;
                let i = self.emit_expr(index)?;
                Ok(format!("{o}[{i}]"))
            }
            Expr::Str(parts, _) => self.emit_string(parts),
            Expr::Call { callee, args, .. } => self.emit_call(callee, args),
            Expr::Member {
                object, name, safe, ..
            } => {
                let o = self.emit_expr(object)?;
                let arrow = if *safe { "?->" } else { "->" };
                Ok(format!("{o}{arrow}{name}"))
            }
            // `inner!` → a once-per-file helper that throws on null, else returns the value (M3
            // S2.5). The null-fault message differs from the Phorge backends' (no name/line) — a
            // documented transpile-only divergence (KNOWN_ISSUES); the present-value case is exact.
            Expr::Force { inner, .. } => {
                let v = self.emit_expr(inner)?;
                self.uses_force = true;
                // Namespaced mode puts the helper in the nameless global block → call it `\…`.
                let bs = if self.namespaced { "\\" } else { "" };
                Ok(format!("{bs}__phorge_unwrap({v})"))
            }
            // Implemented in Task 6:
            Expr::Match { .. } => {
                Err("transpile error: match in this position is not yet supported".into())
            }
            // PHP `range()` is inclusive, so an exclusive `a..b` emits `range($a, $b - 1)`. NB this
            // differs from Phorge for an *empty/reversed* range (`a >= b`): PHP `range` descends
            // instead of yielding `[]` — a transpile-only caveat documented in KNOWN_ISSUES; the
            // Phorge backends (run/runvm) are byte-identical and unaffected.
            Expr::Range {
                start,
                end,
                inclusive,
                ..
            } => {
                let s = self.emit_expr(start)?;
                let e = self.emit_expr(end)?;
                Ok(if *inclusive {
                    format!("range({s}, {e})")
                } else {
                    format!("range({s}, {e} - 1)")
                })
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
        }
    }

    /// Emit an interpolated string literal as a PHP concatenation of quoted literal chunks
    /// and parenthesized expressions. Always-correct (avoids PHP's interpolation limits,
    /// e.g. free function calls inside `"{…}"`).
    fn emit_string(&mut self, parts: &[StrPart]) -> Result<String, String> {
        if parts.is_empty() {
            return Ok("\"\"".into());
        }
        let mut chunks: Vec<String> = Vec::new();
        for p in parts {
            match p {
                StrPart::Literal(s) => chunks.push(format!("\"{}\"", php_escape(s))),
                StrPart::Expr(e) => {
                    let code = self.emit_expr(e)?;
                    chunks.push(format!("({code})"));
                }
            }
        }
        Ok(chunks.join(" . "))
    }

    fn emit_call(&mut self, callee: &Expr, args: &[Expr]) -> Result<String, String> {
        if let Expr::Ident(name, _) = callee {
            let argv = self.emit_args(args)?;
            // Enum variant or class construction → `new`; mirrors the evaluator's dispatch.
            if self.variants.contains(name) || self.classes.contains(name) {
                return Ok(format!("new {name}({argv})"));
            }
            // A resolved cross-package call carries a mangled (`\`-bearing) name → emit it
            // fully-qualified (leading `\`). A bare name (same-`Main`-namespace call) stays bare.
            if self.namespaced && name.contains('\\') {
                return Ok(format!("\\{name}({argv})"));
            }
            return Ok(format!("{name}({argv})")); // free function
        }
        if let Expr::Member { .. } = callee {
            return self.emit_member_call(callee, args);
        }
        Err("transpile error: unsupported call target".into())
    }

    fn emit_args(&mut self, args: &[Expr]) -> Result<String, String> {
        let parts: Result<Vec<_>, _> = args.iter().map(|a| self.emit_expr(a)).collect();
        Ok(parts?.join(", "))
    }

    fn emit_member_call(&mut self, callee: &Expr, args: &[Expr]) -> Result<String, String> {
        if let Expr::Member {
            object, name, safe, ..
        } = callee
        {
            // Namespaced native call: `console.println(x)` → the native's PHP erasure (M3 Wave 1).
            // Resolved through the import map (the transpiler has no variable scope to tell a
            // qualifier from a value; the checker rejects a local shadowing an imported qualifier,
            // so a same-spelled value receiver is impossible).
            if !*safe {
                if let Expr::Ident(q, _) = &**object {
                    if let Some(idx) = self
                        .imports
                        .get(q)
                        .and_then(|m| crate::native::index_of(m, name))
                    {
                        let argv: Vec<String> = args
                            .iter()
                            .map(|a| self.emit_expr(a))
                            .collect::<Result<_, _>>()?;
                        let php = (crate::native::registry()[idx].php)(&argv);
                        // Inside a namespace block a bare `strlen(...)` would resolve to
                        // `CurrentNs\strlen`; emit `\strlen(...)` for global-function natives (M5-8).
                        return Ok(if self.namespaced && looks_like_global_call(&php) {
                            format!("\\{php}")
                        } else {
                            php
                        });
                    }
                }
            }
            let o = self.emit_expr(object)?;
            let a = self.emit_args(args)?;
            let arrow = if *safe { "?->" } else { "->" };
            return Ok(format!("{o}{arrow}{name}({a})"));
        }
        Err("transpile error: bad member call".into())
    }

    /// Emit a `match` as an ordered `instanceof` chain. Each arm yields its body either as
    /// `return …;` or `$target = …;` depending on `target`. Payload vars bind positionally
    /// from the subclass's promoted props. A non-exhaustive chain ends with a defensive
    /// `throw` (the checker already guarantees exhaustiveness).
    fn emit_match(
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
        let mut has_catch_all = false;
        for arm in arms {
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
                    let body = self.emit_expr(&arm.body)?;
                    self.line(&format!(
                        "if ({subj} instanceof {vname}) {{ {binds}{} }}",
                        yield_stmt(&target, &body)
                    ));
                    self.pop_scope();
                }
                Pattern::Wildcard(_) => {
                    has_catch_all = true;
                    let body = self.emit_expr(&arm.body)?;
                    self.line(&format!("{{ {} }}", yield_stmt(&target, &body)));
                }
                Pattern::Binding { name, .. } => {
                    // bare identifier arm binds the whole scrutinee (catch-all)
                    has_catch_all = true;
                    self.push_scope();
                    self.declare(name);
                    let body = self.emit_expr(&arm.body)?;
                    self.line(&format!(
                        "{{ ${name} = {subj}; {} }}",
                        yield_stmt(&target, &body)
                    ));
                    self.pop_scope();
                }
                // `null` arm over an optional scrutinee (M3 S2.6) → a `=== null` guard. Correct in
                // return position (the first matching arm exits); see the sequential-`if` model used
                // by the variant arms above.
                Pattern::Null(_) => {
                    let body = self.emit_expr(&arm.body)?;
                    self.line(&format!(
                        "if ({subj} === null) {{ {} }}",
                        yield_stmt(&target, &body)
                    ));
                }
                _ => {
                    return Err(
                        "transpile error: literal patterns in match are not yet supported".into(),
                    )
                }
            }
        }
        if !has_catch_all {
            self.line("throw new \\UnhandledMatchError();");
        }
        Ok(())
    }

    fn binop(op: &BinaryOp) -> &'static str {
        use BinaryOp::*;
        match op {
            Add => "+",
            Sub => "-",
            Mul => "*",
            Div => "/",
            Rem => "%",
            Eq => "==",
            NotEq => "!=",
            Lt => "<",
            Le => "<=",
            Gt => ">",
            Ge => ">=",
            And => "&&",
            Or => "||",
            // `??` is parenthesized at the call site, so it never reaches `binop()`.
            Coalesce => unreachable!("Coalesce handled before binop()"),
            Is | Pipe => unreachable!("Is/Pipe handled before binop()"),
        }
    }

    fn resolve_ident(&self, name: &str) -> String {
        if self.is_local(name) {
            format!("${name}")
        } else if self
            .cur_class_fields
            .as_ref()
            .is_some_and(|f| f.contains(name))
        {
            format!("$this->{name}")
        } else {
            format!("${name}") // best-effort; the checker guarantees resolution
        }
    }
}

/// Escape a literal string chunk for embedding in a PHP double-quoted string.
/// `$` is escaped so PHP does not attempt its own interpolation on emitted literals.
fn php_escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('$', "\\$")
}

/// A ctor param is promoted (becomes a field) iff it carries a visibility modifier —
/// matches the evaluator (EV-4) and the checker's `collect_class`.
fn is_promoted(mods: &[Modifier]) -> bool {
    mods.iter().any(|m| {
        matches!(
            m,
            Modifier::Public | Modifier::Private | Modifier::Protected
        )
    })
}

/// PHP visibility keyword for a member's modifiers (empty string = no keyword).
fn vis(mods: &[Modifier]) -> &'static str {
    if mods.iter().any(|m| matches!(m, Modifier::Private)) {
        "private"
    } else if mods.iter().any(|m| matches!(m, Modifier::Protected)) {
        "protected"
    } else if mods.iter().any(|m| matches!(m, Modifier::Public)) {
        "public"
    } else {
        ""
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

    fn parse_only(src: &str) -> crate::ast::Program {
        // Auto-prepend the reserved `package main;` (M5 S1, line-preserving) unless declared, so
        // transpiler tests need no per-case edit. The transpiler ignores the package in S1 (flat
        // emission); brace-namespaces for non-`main` packages land in S2c.
        let src = if src.trim_start().starts_with("package ") {
            src.to_string()
        } else {
            format!("package main; {src}")
        };
        let tokens = lex(&src).expect("lex");
        Parser::new(tokens).parse_program().expect("parse")
    }

    #[test]
    fn empty_program_emits_php_open_tag() {
        assert_eq!(php(""), "<?php\n");
    }

    #[test]
    fn free_function_with_params_and_arithmetic() {
        let out = php("function add(int a, int b) -> int { int c = a + b; return c; }");
        assert!(out.contains("function add(int $a, int $b): int {"), "{out}");
        assert!(out.contains("$c = $a + $b;"), "{out}");
        assert!(out.contains("return $c;"), "{out}");
    }

    #[test]
    fn no_return_type_is_void() {
        let out = php("function f() { return; }");
        assert!(out.contains("function f(): void {"), "{out}");
    }

    #[test]
    fn if_and_for_and_unary() {
        // Phorge is immutable (no reassignment) — use fresh var decls inside branches.
        let out = php("function f(int n) -> int { \
               List<int> xs = [1, 2]; \
               for (int x in xs) { if (x > 0) { int a = -x; } else { bool b = !true; } } \
               return n; }");
        assert!(out.contains("foreach ($xs as $x) {"), "{out}");
        assert!(out.contains("if ($x > 0) {"), "{out}");
        assert!(out.contains("} else {"), "{out}");
        assert!(
            out.contains("$a = -$x;") && out.contains("$b = !true;"),
            "{out}"
        );
        assert!(out.contains("[1, 2]"), "{out}");
    }

    #[test]
    fn indexing_emits_php_subscript() {
        let out = php("function at(List<int> xs, int i) -> int { return xs[i]; }");
        assert!(out.contains("$xs[$i]"), "{out}");
    }

    #[test]
    fn ranges_emit_php_range() {
        // PHP `range` is inclusive, so exclusive `0..3` emits `range(0, 3 - 1)`.
        let out = php(r#"import core.console;
function main() { for (int i in 0..3) { console.println("{i}"); } }"#);
        assert!(out.contains("range(0, 3 - 1)"), "{out}");
        let inc = php(r#"import core.console;
function main() { for (int i in 1..=3) { console.println("{i}"); } }"#);
        assert!(inc.contains("range(1, 3)"), "{inc}");
    }

    #[test]
    fn expression_if_emits_ternary() {
        let out = php("function pick(bool b) -> int { return if (b) { 1 } else { 2 }; }");
        assert!(out.contains("($b ? 1 : 2)"), "{out}");
    }

    #[test]
    fn interpolation_emits_concatenation() {
        let out = php("function greet(string name) -> string { return \"Hello {name}\"; }");
        assert!(out.contains(r#"return "Hello " . ($name);"#), "{out}");
    }

    #[test]
    fn pure_string_literal_no_concat() {
        let out = php("function f() -> string { return \"hi\"; }");
        assert!(out.contains(r#"return "hi";"#), "{out}");
    }

    #[test]
    fn println_becomes_echo() {
        let out = php("import core.console; function main() { console.println(\"hi\"); }");
        assert!(out.contains(r#"echo "hi" . "\n";"#), "{out}");
    }

    #[test]
    fn main_is_invoked_when_present() {
        let out = php("import core.console; function main() { console.println(\"hi\"); }");
        assert!(out.trim_end().ends_with("main();"), "{out}");
        // no main -> no call
        let no_main = php("function helper() -> int { return 1; }");
        assert!(!no_main.contains("main();"), "{no_main}");
    }

    const SHAPE: &str = "enum Shape { Circle(float radius), Rect(float w, float h), }";

    #[test]
    fn enum_emits_base_and_subclasses() {
        let out = php(SHAPE);
        assert!(out.contains("abstract class Shape {}"), "{out}");
        assert!(out.contains("final class Circle extends Shape {"), "{out}");
        assert!(
            out.contains("public function __construct(public float $radius) {}"),
            "{out}"
        );
        assert!(out.contains("final class Rect extends Shape {"), "{out}");
        assert!(
            out.contains("public function __construct(public float $w, public float $h) {}"),
            "{out}"
        );
    }

    #[test]
    fn variant_construction_uses_new() {
        let out = php(&format!(
            "{SHAPE} function f() -> Shape {{ return Circle(2.0); }}"
        ));
        assert!(out.contains("return new Circle(2.0);"), "{out}");
    }

    #[test]
    fn free_function_call_no_new() {
        let out = php("function inc(int n) -> int { return n + 1; } \
             function f() -> int { return inc(1); }");
        assert!(out.contains("return inc(1);"), "{out}");
    }

    #[test]
    fn class_with_promotion_and_method() {
        let out = php("class Greeter { constructor(private string name) {} \
               function greet() -> string { return \"Hello {name}\"; } }");
        assert!(out.contains("class Greeter {"), "{out}");
        assert!(
            out.contains("function __construct(private string $name) {}"),
            "{out}"
        );
        assert!(out.contains("function greet(): string {"), "{out}");
        // bare field ref inside a method resolves to $this->name
        assert!(out.contains(r#"return "Hello " . ($this->name);"#), "{out}");
    }

    #[test]
    fn explicit_non_promoted_field_emitted() {
        // A plain field (not a ctor param) is emitted as a standalone property.
        let out = php("class C { private int count; constructor() {} }");
        assert!(out.contains("private int $count;"), "{out}");
    }

    #[test]
    fn promoted_field_not_redeclared() {
        // Declared both explicitly AND via promotion: emit only the promotion (PHP forbids
        // redeclaring a promoted property as a separate one — caught by the round-trip test).
        let out = php("class C { private int total; constructor(private int total) {} }");
        assert!(
            out.contains("function __construct(private int $total) {}"),
            "{out}"
        );
        assert!(
            !out.contains("private int $total;"),
            "standalone redeclaration must be gone: {out}"
        );
    }

    #[test]
    fn member_access_and_method_call() {
        let out = php(
            "import core.console; class Greeter { constructor(private string name) {} \
               function greet() -> string { return name; } } \
             function main() { Greeter g = Greeter(\"Tak\"); console.println(g.greet()); }",
        );
        assert!(out.contains(r#"$g = new Greeter("Tak");"#), "{out}");
        assert!(out.contains("$g->greet()"), "{out}");
    }

    #[test]
    fn match_in_return_emits_instanceof_chain() {
        let out = php(&format!(
            "{SHAPE} function area(Shape s) -> float {{ \
               return match s {{ Circle(r) => 3.14159 * r * r, Rect(w, h) => w * h, }}; }}"
        ));
        assert!(out.contains("if ($s instanceof Circle) {"), "{out}");
        assert!(out.contains("$r = $s->radius;"), "{out}"); // positional: r <- field 0 (radius)
        assert!(out.contains("return 3.14159 * $r * $r;"), "{out}");
        assert!(out.contains("if ($s instanceof Rect) {"), "{out}");
        assert!(
            out.contains("$w = $s->w;") && out.contains("$h = $s->h;"),
            "{out}"
        );
        assert!(out.contains("throw new \\UnhandledMatchError();"), "{out}");
    }

    #[test]
    fn match_in_var_decl_assigns_in_each_arm() {
        let out = php(&format!(
            "{SHAPE} function f(Shape s) -> float {{ \
               float a = match s {{ Circle(r) => r, Rect(w, h) => w, }}; return a; }}"
        ));
        assert!(
            out.contains("if ($s instanceof Circle) { $r = $s->radius; $a = $r; }"),
            "{out}"
        );
        assert!(out.contains("if ($s instanceof Rect) {"), "{out}");
    }

    #[test]
    fn wildcard_arm_has_no_trailing_throw() {
        let out = php(&format!(
            "{SHAPE} function area(Shape s) -> float {{ \
               return match s {{ Circle(r) => r, _ => 0.0, }}; }}"
        ));
        assert!(!out.contains("UnhandledMatchError"), "{out}");
    }

    #[test]
    fn match_in_expression_position_errors_cleanly() {
        // match as a call argument is an unsupported expression position.
        let prog = parse_only(&format!(
            "{SHAPE} function f(Shape s) -> float {{ \
               float a = id(match s {{ Circle(r) => r, Rect(w, h) => w, }}); return a; }}"
        ));
        let err = emit(&prog).unwrap_err();
        assert!(
            err.contains("match in this position is not yet supported"),
            "{err}"
        );
    }
}
