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

// `#[allow(dead_code)]` is temporary scaffolding: `funcs`/`classes`/`variants`/
// `variant_fields` are populated by `collect` but not *read* until call dispatch (Task 4)
// and match binding (Task 6). Removed once every field is consumed.
#[allow(dead_code)]
struct Transpiler {
    funcs: HashSet<String>,
    classes: HashSet<String>,
    variants: HashSet<String>,
    variant_fields: HashMap<String, Vec<String>>,
    out: String,
    indent: usize,
    locals: Vec<HashSet<String>>,
    cur_class_fields: Option<HashSet<String>>,
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
                        self.variant_fields
                            .insert(v.name.clone(), v.fields.iter().map(|p| p.name.clone()).collect());
                    }
                }
                Item::Import { .. } => {}
            }
        }
    }

    fn emit_program(&mut self, program: &Program) -> Result<(), String> {
        self.out.push_str("<?php\n");
        for item in &program.items {
            match item {
                Item::Import { .. } => {}
                Item::Function(f) => self.emit_function(f, false)?,
                Item::Enum(e) => self.emit_enum(e)?,
                Item::Class(c) => self.emit_class(c)?,
            }
        }
        Ok(())
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

    fn emit_function(&mut self, f: &FunctionDecl, _is_method: bool) -> Result<(), String> {
        let params: Vec<String> = f
            .params
            .iter()
            .map(|p| format!("{} ${}", Self::emit_type(&p.ty), p.name))
            .collect();
        self.line(&format!("function {}({}): {} {{", f.name, params.join(", "), Self::ret_hint(&f.ret)));
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
                self.line(&format!("public function __construct({}) {{}}", props.join(", ")));
            }
            self.indent -= 1;
            self.line("}");
        }
        Ok(())
    }

    fn emit_class(&mut self, c: &ClassDecl) -> Result<(), String> {
        // Field set for `$this->` resolution = explicit decls + promoted ctor params
        // (mirrors the checker's `collect_class`).
        let mut fields: HashSet<String> = HashSet::new();
        for m in &c.members {
            match m {
                ClassMember::Field { name, .. } => {
                    fields.insert(name.clone());
                }
                ClassMember::Constructor { params, .. } => {
                    for p in params {
                        if is_promoted(&p.modifiers) {
                            fields.insert(p.name.clone());
                        }
                    }
                }
                ClassMember::Method(_) => {}
            }
        }
        self.line(&format!("class {} {{", c.name));
        self.indent += 1;
        let prev = self.cur_class_fields.replace(fields);
        for m in &c.members {
            match m {
                ClassMember::Field { modifiers, ty, name, .. } => {
                    self.line(&format!("{} {} ${name};", vis(modifiers), Self::emit_type(ty)));
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
            Stmt::If { cond, then_block, else_block, .. } => {
                let c = self.emit_expr(cond)?;
                self.line(&format!("if ({c}) {{"));
                self.indent += 1;
                self.push_scope();
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
            Stmt::For { name, iter, body, .. } => {
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
                Ok(format!("{l} {} {r}", Self::binop(op)))
            }
            Expr::List(items, _) => {
                let parts: Result<Vec<_>, _> = items.iter().map(|i| self.emit_expr(i)).collect();
                Ok(format!("[{}]", parts?.join(", ")))
            }
            Expr::Null(_) => Err("transpile error: null is not yet supported".into()),
            Expr::Index { .. } => Err("transpile error: indexing is not yet supported".into()),
            Expr::Str(parts, _) => self.emit_string(parts),
            Expr::Call { callee, args, .. } => self.emit_call(callee, args),
            Expr::Member { object, name, .. } => {
                let o = self.emit_expr(object)?;
                Ok(format!("{o}->{name}"))
            }
            // Implemented in Task 6:
            Expr::Match { .. } => {
                Err("transpile error: match in this position is not yet supported".into())
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
            if name == "println" {
                let a = if args.is_empty() { "\"\"".into() } else { self.emit_expr(&args[0])? };
                return Ok(format!(r#"echo {a} . "\n""#)); // trailing ';' added by Stmt::Expr
            }
            let argv = self.emit_args(args)?;
            // Enum variant or class construction → `new`; mirrors the evaluator's dispatch.
            if self.variants.contains(name) || self.classes.contains(name) {
                return Ok(format!("new {name}({argv})"));
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
        if let Expr::Member { object, name, .. } = callee {
            let o = self.emit_expr(object)?;
            let a = self.emit_args(args)?;
            return Ok(format!("{o}->{name}({a})"));
        }
        Err("transpile error: bad member call".into())
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
            Is | Pipe => unreachable!("Is/Pipe handled before binop()"),
        }
    }

    fn resolve_ident(&self, name: &str) -> String {
        if self.is_local(name) {
            format!("${name}")
        } else if self.cur_class_fields.as_ref().is_some_and(|f| f.contains(name)) {
            format!("$this->{name}")
        } else {
            format!("${name}") // best-effort; the checker guarantees resolution
        }
    }
}

/// Escape a literal string chunk for embedding in a PHP double-quoted string.
/// `$` is escaped so PHP does not attempt its own interpolation on emitted literals.
fn php_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"").replace('$', "\\$")
}

/// A ctor param is promoted (becomes a field) iff it carries a visibility modifier —
/// matches the evaluator (EV-4) and the checker's `collect_class`.
fn is_promoted(mods: &[Modifier]) -> bool {
    mods.iter()
        .any(|m| matches!(m, Modifier::Public | Modifier::Private | Modifier::Protected))
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
        let out = php(
            "function f(int n) -> int { \
               List<int> xs = [1, 2]; \
               for (int x in xs) { if (x > 0) { int a = -x; } else { bool b = !true; } } \
               return n; }",
        );
        assert!(out.contains("foreach ($xs as $x) {"), "{out}");
        assert!(out.contains("if ($x > 0) {"), "{out}");
        assert!(out.contains("} else {"), "{out}");
        assert!(out.contains("$a = -$x;") && out.contains("$b = !true;"), "{out}");
        assert!(out.contains("[1, 2]"), "{out}");
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
        let out = php("function main() { println(\"hi\"); }");
        assert!(out.contains(r#"echo "hi" . "\n";"#), "{out}");
    }

    const SHAPE: &str = "enum Shape { Circle(float radius), Rect(float w, float h), }";

    #[test]
    fn enum_emits_base_and_subclasses() {
        let out = php(SHAPE);
        assert!(out.contains("abstract class Shape {}"), "{out}");
        assert!(out.contains("final class Circle extends Shape {"), "{out}");
        assert!(out.contains("public function __construct(public float $radius) {}"), "{out}");
        assert!(out.contains("final class Rect extends Shape {"), "{out}");
        assert!(out.contains("public function __construct(public float $w, public float $h) {}"), "{out}");
    }

    #[test]
    fn variant_construction_uses_new() {
        let out = php(&format!("{SHAPE} function f() -> Shape {{ return Circle(2.0); }}"));
        assert!(out.contains("return new Circle(2.0);"), "{out}");
    }

    #[test]
    fn free_function_call_no_new() {
        let out = php(
            "function inc(int n) -> int { return n + 1; } \
             function f() -> int { return inc(1); }",
        );
        assert!(out.contains("return inc(1);"), "{out}");
    }

    #[test]
    fn class_with_promotion_and_method() {
        let out = php(
            "class Greeter { constructor(private string name) {} \
               function greet() -> string { return \"Hello {name}\"; } }",
        );
        assert!(out.contains("class Greeter {"), "{out}");
        assert!(out.contains("function __construct(private string $name) {}"), "{out}");
        assert!(out.contains("function greet(): string {"), "{out}");
        // bare field ref inside a method resolves to $this->name
        assert!(out.contains(r#"return "Hello " . ($this->name);"#), "{out}");
    }

    #[test]
    fn explicit_field_decl_emitted() {
        let out = php("class C { private int total; constructor(private int total) {} }");
        assert!(out.contains("private int $total;"), "{out}");
    }

    #[test]
    fn member_access_and_method_call() {
        let out = php(
            "class Greeter { constructor(private string name) {} \
               function greet() -> string { return name; } } \
             function main() { Greeter g = Greeter(\"Tak\"); println(g.greet()); }",
        );
        assert!(out.contains(r#"$g = new Greeter("Tak");"#), "{out}");
        assert!(out.contains("$g->greet()"), "{out}");
    }
}
