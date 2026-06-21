//! Phorge → PHP transpiler. Walks the untyped AST (the same AST the evaluator walks)
//! and emits runnable PHP 8.x source. Entry point: [`emit`].
use crate::ast::*;
use std::collections::{BTreeSet, HashMap, HashSet};

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
    /// An enum variant's PHP namespace (`namespace_of` of the — possibly mangled — enum name), so a
    /// cross-package variant is constructed and `instanceof`-tested as a fully-qualified class
    /// (`new \Acme\Geometry\Circle(…)`). A `package main` (bare) enum maps to `Main` ⇒ bare emission.
    variant_ns: HashMap<String, String>,
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
    /// Set when `/`, `%`, an interpolation, or a range is emitted — each defines a once-per-file
    /// runtime helper (M7) that reproduces Phorge's type-driven semantics under PHP's looser rules:
    /// `__phorge_div` (int `/` ⇒ `intdiv`), `__phorge_rem` (float `%` ⇒ `fmod`), `__phorge_str`
    /// (bool ⇒ `"true"/"false"`), `__phorge_range` (empty/reversed ⇒ `[]`, never descending).
    uses_div: bool,
    uses_rem: bool,
    uses_str: bool,
    uses_range: bool,
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

/// A type *reference* in PHP: a mangled (`\`-bearing) cross-package name becomes an absolute FQN
/// (leading `\`, so it resolves regardless of the surrounding `namespace` block — uniform with
/// function de-mangling, no `use`); a bare same-/`Main`-namespace name stays bare (M-RT cross-package
/// types). Byte-identical to the pre-lift output for a single-package program (no `\` names).
fn php_type_ref(name: &str) -> String {
    if name.contains('\\') {
        format!("\\{name}")
    } else {
        name.to_string()
    }
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
            variant_ns: HashMap::new(),
            out: String::new(),
            indent: 0,
            locals: Vec::new(),
            cur_class_fields: None,
            imports: HashMap::new(),
            uses_force: false,
            uses_div: false,
            uses_rem: false,
            uses_str: false,
            uses_range: false,
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
                // Interfaces are not callable/constructible, so they need no resolution index;
                // they are emitted as PHP `interface` blocks in pass 2.
                Item::Interface(_) => {}
                Item::Enum(e) => {
                    let ns = namespace_of(&e.name);
                    for v in &e.variants {
                        self.variants.insert(v.name.clone());
                        self.variant_ns.insert(v.name.clone(), ns.clone());
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
        self.namespaced = program.items.iter().any(|it| match it {
            Item::Function(f) => f.name.contains('\\'),
            // A cross-package *type* (class/enum/interface) is mangled too — a project may export
            // only types and no functions (M-RT cross-package types), so check type names as well.
            Item::Class(c) => c.name.contains('\\'),
            Item::Enum(e) => e.name.contains('\\'),
            Item::Interface(i) => i.name.contains('\\'),
            _ => false,
        });
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
                Item::Interface(i) => self.emit_interface(i)?,
                // Aliases are expanded out of the AST before transpiling; arm only for exhaustiveness.
                Item::TypeAlias { .. } => {}
            }
        }
        // The interpreter auto-invokes `main`; PHP does not. Emit the call so the output
        // is a runnable program, not just definitions.
        if self.funcs.contains("main") {
            self.line("main();");
        }
        // The runtime helpers, each defined once when used. PHP hoists top-level function
        // declarations, so emitting them after `main();` is still callable from any body.
        self.emit_runtime_helpers();
        Ok(())
    }

    /// Multi-package emission (M5 S2c, M5-7): one `namespace …{}` brace-block per package, then a
    /// nameless `namespace {}` block that bootstraps `\Main\main()` and holds the global `opt!`
    /// helper. A definition's namespace is its mangled prefix (`Acme\Util\compute` ⇒ `Acme\Util`,
    /// `Acme\Geometry\Point` ⇒ `Acme\Geometry`); bare names (the `main` package) land in `Main`. A
    /// cross-package type's definition (class/enum/interface) is bucketed into its own namespace
    /// (M-RT cross-package types). The bootstrap block is emitted last so every package's functions
    /// and types are already declared when it runs.
    fn emit_program_namespaced(&mut self, program: &Program) -> Result<(), String> {
        use std::collections::BTreeMap;
        self.out.push_str("<?php\n");
        let mut buckets: BTreeMap<String, Vec<&Item>> = BTreeMap::new();
        for item in &program.items {
            let ns = match item {
                Item::Function(f) => namespace_of(&f.name),
                Item::Enum(e) => namespace_of(&e.name),
                Item::Class(c) => namespace_of(&c.name),
                Item::Interface(i) => namespace_of(&i.name),
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
                    Item::Interface(i) => self.emit_interface(i)?,
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
        self.emit_runtime_helpers();
        self.indent -= 1;
        self.line("}");
        Ok(())
    }

    /// The once-per-file runtime helpers (each gated by its `uses_*` flag). In flat mode they are
    /// top-level globals; in namespaced mode they are emitted inside the nameless block, so their
    /// fully-qualified names are `\__phorge_*` (which the call sites emit via the `bs` prefix). Each
    /// mirrors a Phorge value kernel / `as_display` so the PHP leg matches `run`/`runvm` byte-for-byte.
    fn emit_runtime_helpers(&mut self) {
        if self.uses_force {
            self.line("function __phorge_unwrap($v) {");
            self.indent += 1;
            self.line(
                "if ($v === null) { throw new \\RuntimeException(\"force-unwrap of null\"); }",
            );
            self.line("return $v;");
            self.indent -= 1;
            self.line("}");
        }
        if self.uses_div {
            // Phorge `/`: int/int truncates toward zero (`intdiv`); float/float is real division.
            self.line("function __phorge_div($a, $b) {");
            self.indent += 1;
            self.line("return (is_int($a) && is_int($b)) ? intdiv($a, $b) : $a / $b;");
            self.indent -= 1;
            self.line("}");
        }
        if self.uses_rem {
            // Phorge `%`: int/int integer modulo; float/float `fmod` (sign of dividend, like Rust `%`).
            self.line("function __phorge_rem($a, $b) {");
            self.indent += 1;
            self.line("return (is_int($a) && is_int($b)) ? $a % $b : fmod($a, $b);");
            self.indent -= 1;
            self.line("}");
        }
        if self.uses_str {
            // Mirror Value::as_display: bool ⇒ "true"/"false"; float ⇒ Rust `{}` formatting (via
            // __phorge_float); everything else PHP string cast. A naked `(string)$float` uses PHP's
            // `precision=14` and switches to scientific notation for large/small magnitudes — both
            // diverge from the Rust backends, which print the shortest round-trip, always positional.
            self.line("function __phorge_str($v) {");
            self.indent += 1;
            self.line("if (is_bool($v)) { return $v ? \"true\" : \"false\"; }");
            self.line("if (is_float($v)) { return __phorge_float($v); }");
            self.line("return (string)$v;");
            self.indent -= 1;
            self.line("}");
            // Reproduce Rust's `f64` Display exactly (EV-6): the shortest decimal that round-trips to
            // the same double, in positional notation (never scientific, for any magnitude), with an
            // integer-valued float rendered without a trailing `.0`. The `%.{p}e` loop finds the
            // minimal precision that round-trips (Ryū/Grisu shortest is unique); the mantissa digits
            // are then placed positionally. Only tier-1 PHP functions, so it is correct under `php -n`.
            self.line("function __phorge_float($v) {");
            self.indent += 1;
            self.line("if (is_nan($v)) { return \"NaN\"; }");
            self.line("if (is_infinite($v)) { return $v < 0 ? \"-inf\" : \"inf\"; }");
            self.line("if ($v == 0.0) { return (fdiv(1.0, $v) < 0) ? \"-0\" : \"0\"; }");
            self.line("$neg = $v < 0;");
            self.line("$a = $neg ? -$v : $v;");
            self.line("$repr = sprintf(\"%.16e\", $a);");
            self.line("for ($p = 0; $p <= 16; $p++) {");
            self.indent += 1;
            self.line("$cand = sprintf(\"%.{$p}e\", $a);");
            self.line("if ((float)$cand === $a) { $repr = $cand; break; }");
            self.indent -= 1;
            self.line("}");
            self.line("$epos = strpos($repr, \"e\");");
            self.line("$exp = (int)substr($repr, $epos + 1);");
            self.line("$mant = str_replace(\".\", \"\", substr($repr, 0, $epos));");
            self.line("$mant = rtrim($mant, \"0\");");
            self.line("if ($mant === \"\") { $mant = \"0\"; }");
            self.line("$ndig = strlen($mant);");
            self.line("if ($exp >= $ndig - 1) {");
            self.indent += 1;
            self.line("$s = $mant . str_repeat(\"0\", $exp - ($ndig - 1));");
            self.indent -= 1;
            self.line("} elseif ($exp >= 0) {");
            self.indent += 1;
            self.line("$s = substr($mant, 0, $exp + 1) . \".\" . substr($mant, $exp + 1);");
            self.indent -= 1;
            self.line("} else {");
            self.indent += 1;
            self.line("$s = \"0.\" . str_repeat(\"0\", -$exp - 1) . $mant;");
            self.indent -= 1;
            self.line("}");
            self.line("return $neg ? \"-\" . $s : $s;");
            self.indent -= 1;
            self.line("}");
        }
        if self.uses_range {
            // Phorge range: empty when start > hi; never descends (PHP `range()` descends — QW-13).
            self.line("function __phorge_range($a, $b, $inclusive) {");
            self.indent += 1;
            self.line("$hi = $inclusive ? $b : $b - 1;");
            self.line("return ($a <= $hi) ? range($a, $hi) : [];");
            self.indent -= 1;
            self.line("}");
        }
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

    /// Render a `ClassName.field` static access as the PHP `ClassName::$field` lvalue (M-mut.7), or
    /// `None` if `object` is not a class name (then it is an instance member). Mirrors the backends'
    /// locals-first rule: a local binding shadowing a class name is an instance access.
    fn static_ref(&self, object: &Expr, name: &str) -> Option<String> {
        if let Expr::Ident(cls, _) = object {
            if !self.is_local(cls) && self.classes.contains(cls) {
                return Some(format!("{cls}::${name}"));
            }
        }
        None
    }

    fn emit_type(ty: &Type) -> String {
        match ty {
            Type::Named { name, .. } => match name.as_str() {
                "int" => "int".into(),
                "float" => "float".into(),
                "bool" => "bool".into(),
                "string" => "string".into(),
                // PHP strings ARE byte arrays — `bytes` erases to `string` (M6 W0).
                "bytes" => "string".into(),
                // `Html` and `Attr` are render-ready text — both erase to `string`. The escaping
                // boundary lives in the `core.html` natives, not the type (see core.html design spec).
                "Html" | "Attr" => "string".into(),
                "List" | "Map" | "Set" => "array".into(),
                other => php_type_ref(other), // enum / class / interface name (FQN if cross-package)
            },
            // A union → PHP 8.0 native `A|B` (M-RT S4). Members emit via the same `emit_type`, so a
            // cross-package member would carry its FQN; dedup defensively (the checker already
            // guarantees ≥2 distinct members). `int|int` can't occur in a well-typed program.
            Type::Union(members, _) => {
                let mut parts: Vec<String> = Vec::new();
                for m in members {
                    let p = Self::emit_type(m);
                    if !parts.contains(&p) {
                        parts.push(p);
                    }
                }
                parts.join("|")
            }
            // An intersection → PHP 8.1 native `A&B` (M-RT S5). Members emit via `emit_type` (a
            // cross-package member carries its FQN); dedup defensively. The checker guarantees ≥2
            // distinct members, all interfaces plus at most one class — all valid PHP intersection
            // operands.
            Type::Intersection(members, _) => {
                let mut parts: Vec<String> = Vec::new();
                for m in members {
                    let p = Self::emit_type(m);
                    if !parts.contains(&p) {
                        parts.push(p);
                    }
                }
                parts.join("&")
            }
            // A function-typed parameter/return erases to PHP `\Closure` (M3 S3).
            Type::Function { .. } => "\\Closure".into(),
            // An erased generic type parameter (M-RT S7) → PHP `mixed` (the runtime is untyped; the
            // checker already proved the program well-typed before erasure).
            Type::Erased(_) => "mixed".into(),
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
        // The base + its variant subclasses are declared inside the enum's own `namespace` block, so
        // both use the bare trailing segment (`Acme\Geometry\Color` ⇒ `Color`); a single-package enum
        // is unchanged. Variant subclass names are never mangled (they aren't types).
        let base = last_segment(&e.name);
        self.line(&format!("abstract class {} {{}}", base));
        for v in &e.variants {
            self.line(&format!("final class {} extends {} {{", v.name, base));
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
        let implements = if c.implements.is_empty() {
            String::new()
        } else {
            let ifaces: Vec<String> = c.implements.iter().map(|i| php_type_ref(i)).collect();
            format!(" implements {}", ifaces.join(", "))
        };
        // Declared inside its `namespace` block in multi-package mode ⇒ bare trailing segment.
        let disp = if self.namespaced {
            last_segment(&c.name)
        } else {
            &c.name
        };
        self.line(&format!("class {}{} {{", disp, implements));
        self.indent += 1;
        let prev = self.cur_class_fields.replace(fields);
        for m in &c.members {
            match m {
                ClassMember::Field {
                    modifiers,
                    ty,
                    name,
                    init,
                    ..
                } => {
                    // A field that is ALSO a promoted ctor param is declared by the
                    // promotion — emitting it again is a PHP "redeclare" fatal.
                    if promoted_names.contains(name) {
                        continue;
                    }
                    // A typed PHP property requires a visibility keyword (`int $x;` is a syntax
                    // error). Phorge fields are immutable-by-default and visibility is not enforced
                    // at runtime by the backends, so a field with no explicit visibility (e.g.
                    // `mutable int x;`) emits as `public` — the spine-safe choice (M-mut.6).
                    let v = vis(modifiers);
                    let v = if v.is_empty() { "public" } else { v };
                    if modifiers.contains(&Modifier::Static) {
                        // A `static` field (M-mut.7) → PHP `public static <type> $name = <init>;`. The
                        // initializer is a literal constant (checker-enforced), so it round-trips.
                        let init_php = match init {
                            Some(e) => self.emit_expr(e)?,
                            None => "null".to_string(),
                        };
                        self.line(&format!(
                            "{v} static {} ${name} = {init_php};",
                            Self::emit_type(ty)
                        ));
                    } else {
                        self.line(&format!("{v} {} ${name};", Self::emit_type(ty)));
                    }
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

    /// Emit a PHP `interface` (M-RT S2): the name, an optional `extends A, B` clause, and one
    /// abstract method signature per declared method (`public function name(params): ret;`). PHP
    /// interface methods are implicitly public + abstract, so only the signature is emitted.
    fn emit_interface(&mut self, i: &crate::ast::InterfaceDecl) -> Result<(), String> {
        let extends = if i.extends.is_empty() {
            String::new()
        } else {
            let parents: Vec<String> = i.extends.iter().map(|e| php_type_ref(e)).collect();
            format!(" extends {}", parents.join(", "))
        };
        let disp = if self.namespaced {
            last_segment(&i.name)
        } else {
            &i.name
        };
        self.line(&format!("interface {}{} {{", disp, extends));
        self.indent += 1;
        for m in &i.methods {
            let params: Vec<String> = m
                .params
                .iter()
                .map(|p| format!("{} ${}", Self::emit_type(&p.ty), p.name))
                .collect();
            self.line(&format!(
                "public function {}({}): {};",
                m.name,
                params.join(", "),
                Self::ret_hint(&m.ret)
            ));
        }
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
            Stmt::Assign { target, value, .. } => {
                // Reassignment (`$x = …;`) and value-type element set (`$xs[$i] = …;`, M-mut.5) share
                // one shape: the lhs is the target rendered as an expression. PHP arrays are COW value
                // types, so `$xs[$i] = $e` has the same value semantics as Phorge's `Op::SetIndex`.
                // `mutable`/immutable is erased (PHP locals are always mutable).
                let lhs = match target {
                    Expr::Ident(n, _) => format!("${n}"),
                    // Element set `$xs[$i]` (M-mut.5) and instance-field set `$o->f` (M-mut.6) both
                    // render the target as a PHP lvalue expression. PHP objects are mutable handles,
                    // so `$o->f = $e` is byte-identical to the backends' `Op::SetField`.
                    Expr::Index { .. } | Expr::Member { .. } => self.emit_expr(target)?,
                    _ => unreachable!("checker rejects other assignment targets"),
                };
                let e = self.emit_expr(value)?;
                self.line(&format!("{lhs} = {e};"));
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
            Stmt::While {
                cond,
                body,
                post_cond,
                ..
            } => {
                if *post_cond {
                    self.line("do {");
                    self.indent += 1;
                    self.push_scope();
                    for st in body {
                        self.emit_stmt(st)?;
                    }
                    self.pop_scope();
                    self.indent -= 1;
                    let c = self.emit_expr(cond)?;
                    self.line(&format!("}} while ({c});"));
                } else {
                    let c = self.emit_expr(cond)?;
                    self.line(&format!("while ({c}) {{"));
                    self.indent += 1;
                    self.push_scope();
                    for st in body {
                        self.emit_stmt(st)?;
                    }
                    self.pop_scope();
                    self.indent -= 1;
                    self.line("}");
                }
            }
            Stmt::CFor {
                init,
                cond,
                step,
                body,
                ..
            } => {
                // A real PHP `for (init; cond; step)` — NOT lowered to a `while`, because PHP's
                // `continue` inside a `for` runs the `step` (a `while` would skip it).
                self.push_scope(); // init's binding
                let init_s = match init {
                    Some(s) => self.emit_for_clause(s)?,
                    None => String::new(),
                };
                let cond_s = match cond {
                    Some(c) => self.emit_expr(c)?,
                    None => String::new(),
                };
                let step_s = match step {
                    Some(s) => self.emit_for_clause(s)?,
                    None => String::new(),
                };
                self.line(&format!("for ({init_s}; {cond_s}; {step_s}) {{"));
                self.indent += 1;
                self.push_scope();
                for st in body {
                    self.emit_stmt(st)?;
                }
                self.pop_scope();
                self.indent -= 1;
                self.line("}");
                self.pop_scope();
            }
            Stmt::Break(_) => self.line("break;"),
            Stmt::Continue(_) => self.line("continue;"),
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

    /// Render a C-`for` init/step clause inline (no trailing `;`, no newline) for the PHP for-header
    /// (M-mut.3). The clause is a `VarDecl` (`$i = …`), an `Assign`/compound-assign desugar (`$i =
    /// …`), or a bare expression.
    fn emit_for_clause(&mut self, s: &Stmt) -> Result<String, String> {
        Ok(match s {
            Stmt::VarDecl { name, init, .. } => {
                let e = self.emit_expr(init)?;
                self.declare(name);
                format!("${name} = {e}")
            }
            Stmt::Assign { target, value, .. } => {
                let lhs = match target {
                    Expr::Ident(n, _) => format!("${n}"),
                    Expr::Index { .. } | Expr::Member { .. } => self.emit_expr(target)?,
                    _ => unreachable!("checker rejects other assignment targets"),
                };
                let e = self.emit_expr(value)?;
                format!("{lhs} = {e}")
            }
            Stmt::Expr(e, _) => self.emit_expr(e)?,
            _ => unreachable!("c-for init/step is a decl, assignment, or expression"),
        })
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
                // Wrap a compound operand so the unary binds only to it (P0-2 — `-(a + b)`, `!(a && b)`).
                let inner = Self::paren_if_compound(expr, inner);
                Ok(format!("{sym}{inner}"))
            }
            Expr::Binary { op, lhs, rhs, .. } => {
                let l = self.emit_expr(lhs)?;
                let r = self.emit_expr(rhs)?;
                let bs = if self.namespaced { "\\" } else { "" };
                // `/` and `%` are type-driven in Phorge (int vs float) but PHP's `/` is always float
                // and `%` always integer. Route through a runtime helper that branches on operand
                // types at PHP-runtime, mirroring the value kernels (P0-1, P0-4). Helper args are
                // comma-delimited, so the raw operand code needs no precedence parens.
                if matches!(op, BinaryOp::Div) {
                    self.uses_div = true;
                    return Ok(format!("{bs}__phorge_div({l}, {r})"));
                }
                if matches!(op, BinaryOp::Rem) {
                    self.uses_rem = true;
                    return Ok(format!("{bs}__phorge_rem({l}, {r})"));
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
                Ok(format!("{v} instanceof {}", php_type_ref(type_name)))
            }
            Expr::List(items, _) => {
                let parts: Result<Vec<_>, _> = items.iter().map(|i| self.emit_expr(i)).collect();
                Ok(format!("[{}]", parts?.join(", ")))
            }
            // A map literal → a PHP `[k => v, …]` array (insertion-ordered, like Phorge), M-RT S3.
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
                // Static read `ClassName.field` → PHP `ClassName::$field` (M-mut.7).
                if !*safe {
                    if let Some(s) = self.static_ref(object, name) {
                        return Ok(s);
                    }
                }
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
            // `obj with { f = e }` → PHP 8.5 `clone($obj, ['f' => e, …])`: a fresh instance with the
            // named fields overridden and the constructor bypassed — byte-identical to the backends
            // (M-mut.4a). An empty override list is just `clone($obj)`.
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
            // — Phorge values are immutable, so by-value capture is exact; pattern-bound payload vars
            // are declared *inside* the closure and so are intentionally excluded. A regular closure
            // auto-binds `$this`, so a match inside a method keeps working.
            Expr::Match {
                scrutinee, arms, ..
            } => {
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
            // `__phorge_range` reproduces Phorge's range semantics under PHP: an empty/reversed range
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
                    "{bs}__phorge_range({s}, {e}, {})",
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
                    LambdaBody::Expr(e) => Ok(format!("fn({ps}) => {}", self.emit_expr(e)?)),
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
                    // Coerce via `__phorge_str` so a `bool` renders `true`/`false` (not PHP's `1`/``)
                    // — mirrors Value::as_display (P0-3). A no-op for int/float/string. The helper
                    // call is itself a primary, so it replaces the old grouping parens.
                    self.uses_str = true;
                    let bs = if self.namespaced { "\\" } else { "" };
                    chunks.push(format!("{bs}__phorge_str({code})"));
                }
            }
        }
        Ok(chunks.join(" . "))
    }

    /// A PHP reference to an enum variant subclass: fully-qualified when its enum lives in a package
    /// namespace (`new \Acme\Geometry\Circle(…)`, an `instanceof` against it), bare for a `package
    /// main` enum (`Circle`) — byte-identical to the pre-lift output for a single-package program.
    fn variant_ref(&self, variant: &str) -> String {
        match self.variant_ns.get(variant) {
            Some(ns) if ns != "Main" => format!("\\{ns}\\{variant}"),
            _ => variant.to_string(),
        }
    }

    fn emit_call(&mut self, callee: &Expr, args: &[Expr]) -> Result<String, String> {
        if let Expr::Ident(name, _) = callee {
            let argv = self.emit_args(args)?;
            // Enum variant or class construction → `new`; mirrors the evaluator's dispatch. A
            // cross-package class name is mangled (FQN); a variant subclass lives in its enum's
            // namespace, so a cross-package variant is constructed fully-qualified too.
            if self.variants.contains(name) {
                return Ok(format!("new {}({argv})", self.variant_ref(name)));
            }
            if self.classes.contains(name) {
                return Ok(format!("new {}({argv})", php_type_ref(name)));
            }
            // A closure stored in a local variable (e.g. a `\Closure` parameter or a `var`-bound
            // lambda) must be called as `$f(…)` — PHP requires the `$` sigil on variable-call sites.
            if self.is_local(name) {
                return Ok(format!("${name}({argv})"));
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
        if let Expr::Lambda { .. } = callee {
            let f = self.emit_expr(callee)?;
            let argv = self.emit_args(args)?;
            return Ok(format!("({f})({argv})"));
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
                    self.line(&format!(
                        "{cond_kw} ({subj} instanceof {}) {{ {bind}{} }}",
                        php_type_ref(type_name),
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

    /// A PHP "primary" expression: emits self-contained, so it never needs wrapping parens when used
    /// as an operand. Compound expressions (`Binary`/`Unary`/`If`/`Match`/`Lambda`) are NOT primary
    /// and get parenthesized by `paren_if_compound` (P0-2). `Force`/`Range`/`Call`/`Member`/`Index`
    /// emit as `__phorge_unwrap(…)` / `__phorge_range(…)` / `f(…)` / `$o->x` / `$o[$i]` — all primary.
    fn is_primary(e: &Expr) -> bool {
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
    fn paren_if_compound(e: &Expr, code: String) -> String {
        if Self::is_primary(e) {
            code
        } else {
            format!("({code})")
        }
    }

    fn binop(op: &BinaryOp) -> &'static str {
        use BinaryOp::*;
        match op {
            Add => "+",
            Sub => "-",
            Mul => "*",
            // `/` and `%` are routed through `__phorge_div`/`__phorge_rem` before binop() (P0-1/P0-4).
            Div => unreachable!("Div handled via __phorge_div before binop()"),
            Rem => unreachable!("Rem handled via __phorge_rem before binop()"),
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
            Pipe => unreachable!("`|>` is lowered to a call in the parser"),
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

/// Escape a literal string chunk for embedding in a PHP double-quoted string.
/// `$` is escaped so PHP does not attempt its own interpolation on emitted literals.
fn php_escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('$', "\\$")
}

/// Escape a `bytes` literal for a PHP double-quoted string. Printable ASCII is emitted verbatim (with
/// `\` `"` `$` escaped); every other octet becomes a two-digit `\xHH` (always two digits so PHP's
/// greedy `\x` escape can't merge with a following hex character). PHP strings are byte arrays, so the
/// round-trip is exact (M6 W0).
fn php_escape_bytes(bytes: &[u8]) -> String {
    let mut out = String::new();
    for &b in bytes {
        match b {
            b'\\' => out.push_str("\\\\"),
            b'"' => out.push_str("\\\""),
            b'$' => out.push_str("\\$"),
            0x20..=0x7E => out.push(b as char),
            _ => out.push_str(&format!("\\x{b:02x}")),
        }
    }
    out
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
        // Ranges route through `__phorge_range` (QW-13): the helper yields `[]` for an empty/reversed
        // range, where PHP's bare `range()` would descend. The `inclusive` flag is the third arg.
        let out = php(r#"import Core.Console;
function main() { for (int i in 0..3) { Console.println("{i}"); } }"#);
        assert!(out.contains("__phorge_range(0, 3, false)"), "{out}");
        assert!(out.contains("function __phorge_range"), "{out}");
        let inc = php(r#"import Core.Console;
function main() { for (int i in 1..=3) { Console.println("{i}"); } }"#);
        assert!(inc.contains("__phorge_range(1, 3, true)"), "{inc}");
    }

    #[test]
    fn expression_if_emits_ternary() {
        let out = php("function pick(bool b) -> int { return if (b) { 1 } else { 2 }; }");
        assert!(out.contains("($b ? 1 : 2)"), "{out}");
    }

    #[test]
    fn interpolation_emits_concatenation() {
        // Each interpolated value is coerced via `__phorge_str` (P0-3: bool ⇒ "true"/"false").
        let out = php("function greet(string name) -> string { return \"Hello {name}\"; }");
        assert!(
            out.contains(r#"return "Hello " . __phorge_str($name);"#),
            "{out}"
        );
    }

    #[test]
    fn float_interpolation_emits_phorge_float_helper() {
        // A float reaches PHP only through interpolation (`Console.println` takes `string`), so the
        // `__phorge_str` chokepoint routes floats through `__phorge_float`, which reproduces Rust's
        // shortest-round-trip positional `f64` Display (no PHP precision-14 / scientific divergence).
        let out = php("function f(float x) -> string { return \"v={x}\"; }");
        assert!(
            out.contains(r#"return "v=" . __phorge_str($x);"#),
            "call site routes through __phorge_str: {out}"
        );
        assert!(
            out.contains("if (is_float($v)) { return __phorge_float($v); }"),
            "__phorge_str delegates floats to __phorge_float: {out}"
        );
        assert!(
            out.contains("function __phorge_float($v) {")
                && out.contains(r#"$cand = sprintf("%.{$p}e", $a);"#),
            "__phorge_float helper is defined with the shortest-round-trip loop: {out}"
        );
        // Only tier-1 PHP functions — must stay correct under `php -n` (extension policy).
        for forbidden in ["mb_", "ctype_", "iconv", "bcadd"] {
            assert!(
                !out.contains(forbidden),
                "__phorge_float must use tier-1 functions only, found `{forbidden}`: {out}"
            );
        }
    }

    #[test]
    fn pure_string_literal_no_concat() {
        let out = php("function f() -> string { return \"hi\"; }");
        assert!(out.contains(r#"return "hi";"#), "{out}");
    }

    #[test]
    fn literal_match_emits_strict_eq_elseif_chain() {
        // Literal patterns → `=== <lit>` guards. The arms must be `if/elseif/else`-chained (not
        // independent `if`s) so an assign-position match doesn't fall through to the defensive throw.
        let out = php(
            "function sign(int n) -> string { string s = match n { 0 => \"z\", 1 => \"one\", x => \"other\" }; return s; }",
        );
        assert!(out.contains("if ($n === 0) { $s = \"z\"; }"), "{out}");
        assert!(out.contains("elseif ($n === 1) { $s = \"one\"; }"), "{out}");
        assert!(out.contains("else { $x = $n; $s = \"other\"; }"), "{out}");
        // No unconditional throw stranded after the assign chain.
        assert!(!out.contains("$s = \"other\"; }\n    throw"), "{out}");
    }

    #[test]
    fn expression_position_match_emits_iife() {
        // A `match` used as a sub-expression wraps the shared if-chain in an immediately-invoked
        // closure, capturing enclosing locals by value via `use(...)`.
        let out = php(
            "function f(int n) -> int { int base = 5; int r = (match n { 0 => 10, x => x }) + base; return r; }",
        );
        // Over-captures every enclosing local by value (both `$base` and the param `$n`).
        assert!(out.contains("(function() use ($base, $n) {"), "{out}");
        assert!(out.contains("if ($n === 0) { return 10; }"), "{out}");
        assert!(out.contains("else { $x = $n; return $x; }"), "{out}");
        assert!(out.contains("})()"), "{out}");
    }

    #[test]
    fn println_becomes_echo() {
        let out = php("import Core.Console; function main() { Console.println(\"hi\"); }");
        assert!(out.contains(r#"echo "hi" . "\n";"#), "{out}");
    }

    #[test]
    fn main_is_invoked_when_present() {
        let out = php("import Core.Console; function main() { Console.println(\"hi\"); }");
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
        // bare field ref inside a method resolves to $this->name (coerced via __phorge_str — P0-3)
        assert!(
            out.contains(r#"return "Hello " . __phorge_str($this->name);"#),
            "{out}"
        );
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
             function main() { Greeter g = Greeter(\"Tak\"); Console.println(g.greet()); }",
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
                                                            // P0-2: a compound operand keeps grouping parens (`3.14159 * r * r` is left-assoc Mul, so the
                                                            // left operand of the outer `*` is the inner product, conservatively parenthesized).
        assert!(out.contains("return (3.14159 * $r) * $r;"), "{out}");
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
    fn match_as_call_argument_emits_iife() {
        // `match` as a call argument is expression position (M11): it lowers to an immediately-invoked
        // closure wrapping the same variant if-chain, with payload bindings declared inside the closure.
        let prog = parse_only(&format!(
            "{SHAPE} function f(Shape s) -> float {{ \
               float a = id(match s {{ Circle(r) => r, Rect(w, h) => w, }}); return a; }}"
        ));
        let out = emit(&prog).expect("expression-position match transpiles");
        assert!(out.contains("id((function() use ($s) {"), "{out}");
        assert!(
            out.contains("if ($s instanceof Circle) { $r = $s->radius; return $r; }"),
            "{out}"
        );
        assert!(
            out.contains("elseif ($s instanceof Rect) { $w = $s->w; $h = $s->h; return $w; }"),
            "{out}"
        );
        assert!(out.contains("})())"), "{out}");
    }

    // ── M3 S3 Task 5: expression lambdas + named-fn references ──────────────

    #[test]
    fn transpiles_expression_lambda_to_arrow_fn() {
        let php_out = php("package main; import Core.Console; function main(){ var d = fn(int x) => x*2; Console.println(\"{d(5)}\"); }");
        assert!(php_out.contains("fn($x) => $x * 2"), "{php_out}");
    }

    #[test]
    fn transpiles_named_fn_reference() {
        let php_out = php("package main; function inc(int x)->int{return x+1;} function apply(int x,(int)->int f)->int{return f(x);} function main(){ apply(1, inc); }");
        assert!(
            php_out.contains("inc(...)"),
            "first-class callable: {php_out}"
        );
    }
}
