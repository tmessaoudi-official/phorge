//! PHP transpiler — program (M-Decomp W4). See mod.rs for the struct + core + entry points.

use super::*;

impl Transpiler {
    /// Pass 1 — index top-level names so call dispatch and match binding can resolve them.
    pub(super) fn collect(&mut self, program: &Program) {
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
                // M-RT S8: a trait is emitted as a native PHP `trait` in pass 2; it needs no call/
                // construction resolution index (it is never called or constructed by name).
                Item::Trait(_) => {}
                // Aliases are expanded out of the AST before transpiling; arm only for exhaustiveness.
                Item::TypeAlias { .. } => {}
            }
        }
    }

    pub(super) fn emit_program(&mut self, program: &Program) -> Result<(), String> {
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
        let mut emitted_overloads: HashSet<String> = HashSet::new();
        for item in &program.items {
            match item {
                Item::Import { .. } => {}
                Item::Function(f) => {
                    self.emit_free_fn(&program.items, f, &mut emitted_overloads)?
                }
                Item::Enum(e) => self.emit_enum(e)?,
                Item::Class(c) => {
                    // M-RT S6b: multiple inheritance lowers to traits/interfaces (PHP has no MI).
                    if c.extends.len() >= 2 {
                        self.emit_multi_class(c, program)?;
                    } else if self.decomposed.contains(&c.name) {
                        self.emit_decomposed_class(c, program)?;
                    } else {
                        self.emit_class(c)?;
                    }
                }
                Item::Interface(i) => self.emit_interface(i)?,
                // M-RT S8: a native PHP `trait` (composed by classes via `use`).
                Item::Trait(t) => self.emit_trait(t)?,
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
    pub(super) fn emit_program_namespaced(&mut self, program: &Program) -> Result<(), String> {
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
        let mut emitted_overloads: HashSet<String> = HashSet::new();
        for (ns, items) in &buckets {
            self.line(&format!("namespace {ns} {{"));
            self.indent += 1;
            for item in items {
                match item {
                    Item::Function(f) => {
                        // Group M-RT overloads within this package's bucket (same full name).
                        let group: Vec<&FunctionDecl> = items
                            .iter()
                            .filter_map(|it| match &**it {
                                Item::Function(g) if g.name == f.name => Some(g),
                                _ => None,
                            })
                            .collect();
                        if group.len() > 1 {
                            if emitted_overloads.insert(f.name.clone()) {
                                self.emit_overload_set(&f.name, &group, false)?;
                            }
                        } else {
                            self.emit_function(f, false)?;
                        }
                    }
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
    pub(super) fn emit_runtime_helpers(&mut self) {
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
        if self.uses_clone_with {
            // `obj with { f = e }` on PHP 8.4 (native two-arg `clone` is 8.5+): clone, then set each
            // overridden field. Phorge fields emit as plain public PHP properties, so the writes are
            // valid; the constructor is bypassed (matches the backends' shallow clone-then-override).
            self.line("function __phorge_clone_with($o, $changes) {");
            self.indent += 1;
            self.line("$c = clone $o;");
            self.line("foreach ($changes as $k => $v) { $c->$k = $v; }");
            self.line("return $c;");
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
        if self.uses_add {
            // Phorge `+` is overloaded: `string + string` concatenates, numbers add. The checker
            // guarantees both operands share a type, so `is_string($a)` selects the branch exactly
            // (PHP's `+` would TypeError on strings; `.` is its concat operator).
            self.line("function __phorge_add($a, $b) {");
            self.indent += 1;
            self.line("return is_string($a) ? $a . $b : $a + $b;");
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

    pub(super) fn emit_function(
        &mut self,
        f: &FunctionDecl,
        is_method: bool,
    ) -> Result<(), String> {
        self.emit_function_named(f, is_method, None)
    }

    /// Emit a function/method, optionally under an overridden name (M-RT overloading emits each
    /// overload's body under a mangled `<name>__ovl_<i>` name; the dispatcher takes the original).
    pub(super) fn emit_function_named(
        &mut self,
        f: &FunctionDecl,
        is_method: bool,
        name_override: Option<&str>,
    ) -> Result<(), String> {
        let params: Vec<String> = f
            .params
            .iter()
            .map(|p| format!("{} ${}", self.emit_type(&p.ty), p.name))
            .collect();
        // In namespaced mode a top-level function is declared inside its `namespace` block, so emit
        // only its trailing segment (`Acme\Util\compute` ⇒ `compute`). Methods keep their name.
        let disp = match name_override {
            Some(n) => n,
            None if self.namespaced && !is_method => last_segment(&f.name),
            None => &f.name,
        };
        self.line(&format!(
            "function {}({}){} {{",
            disp,
            params.join(", "),
            self.ret_suffix(&f.ret)
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

    /// Emit one free function, grouping M-RT overloads: a name declared more than once in `items`
    /// becomes a single overload set (emitted once, on first occurrence); a unique name emits
    /// directly. `emitted` guards against re-emitting a set as later overloads are walked.
    pub(super) fn emit_free_fn(
        &mut self,
        items: &[Item],
        f: &FunctionDecl,
        emitted: &mut HashSet<String>,
    ) -> Result<(), String> {
        let group: Vec<&FunctionDecl> = items
            .iter()
            .filter_map(|it| match it {
                Item::Function(g) if g.name == f.name => Some(g),
                _ => None,
            })
            .collect();
        if group.len() > 1 {
            if emitted.insert(f.name.clone()) {
                self.emit_overload_set(&f.name, &group, false)?;
            }
            Ok(())
        } else {
            self.emit_function(f, false)
        }
    }

    /// Emit an overloaded free-function / method set (M-RT dynamic dispatch): each overload's body
    /// under a mangled `<leaf>__ovl_<i>` name, then one dispatcher under the original name that
    /// selects on the runtime argument types (`is_int`/`is_string`/`instanceof`), branches ordered
    /// most-specific-first — so the emitted PHP picks the same body the backends' `select_overload`
    /// does for every resolvable call. (An *ambiguous* call faults in the backends; the PHP chain
    /// would take the first match — a transpile-only divergence on faulting input, never in a runnable
    /// example. Overloads that erase to the same PHP test — `string`/`bytes`, or `List`/`Map`/`Set`,
    /// all of which become PHP `string`/`array` — likewise cannot be told apart in PHP; KNOWN_ISSUES.)
    pub(super) fn emit_overload_set(
        &mut self,
        name: &str,
        ovls: &[&FunctionDecl],
        is_method: bool,
    ) -> Result<(), String> {
        let leaf = last_segment(name).to_string();
        for (i, f) in ovls.iter().enumerate() {
            let mangled = format!("{leaf}__ovl_{i}");
            self.emit_function_named(f, is_method, Some(&mangled))?;
        }
        let kinds: Vec<Vec<ParamKind>> = ovls
            .iter()
            .map(|f| {
                f.params
                    .iter()
                    .map(|p| crate::dispatch::param_kind(&p.ty))
                    .collect()
            })
            .collect();
        let mut order: Vec<usize> = (0..ovls.len()).collect();
        order.sort_by(|&a, &b| {
            if crate::dispatch::dominates(&kinds[a], &kinds[b], &self.class_implements) {
                std::cmp::Ordering::Less
            } else if crate::dispatch::dominates(&kinds[b], &kinds[a], &self.class_implements) {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        });
        let disp = if self.namespaced && !is_method {
            leaf.clone()
        } else {
            name.to_string()
        };
        let ret = self.ret_suffix(&ovls[0].ret);
        self.line(&format!("function {disp}(...$args){ret} {{"));
        self.indent += 1;
        for &i in &order {
            let test = self.overload_branch_test(&kinds[i]);
            let mangled = format!("{leaf}__ovl_{i}");
            let target = if is_method {
                format!("$this->{mangled}")
            } else {
                mangled
            };
            self.line(&format!("if ({test}) {{ return {target}(...$args); }}"));
        }
        self.line(&format!(
            "throw new \\LogicException(\"no matching overload for {leaf}\");"
        ));
        self.indent -= 1;
        self.line("}");
        Ok(())
    }

    /// The PHP boolean test that an argument tuple matches one overload's parameter kinds (M-RT).
    pub(super) fn overload_branch_test(&self, kinds: &[ParamKind]) -> String {
        let mut conds = vec![format!("count($args) === {}", kinds.len())];
        for (k, kind) in kinds.iter().enumerate() {
            let a = format!("$args[{k}]");
            conds.push(match kind {
                ParamKind::Int => format!("is_int({a})"),
                ParamKind::Float => format!("is_float({a})"),
                ParamKind::Bool => format!("is_bool({a})"),
                // `bytes` erases to a PHP string, so it shares `string`'s test (indistinguishable).
                ParamKind::Str | ParamKind::Bytes => format!("is_string({a})"),
                // `List`/`Map`/`Set` all erase to a PHP array (indistinguishable).
                ParamKind::List | ParamKind::Map | ParamKind::Set => format!("is_array({a})"),
                ParamKind::Fn => format!("({a} instanceof \\Closure)"),
                ParamKind::Named(n) => {
                    // The built-in `Error` marker is a PHP `\Throwable`; a class/interface/enum uses
                    // its (possibly cross-package FQN) name.
                    let ty = if last_segment(n) == "Error" {
                        "\\Throwable".to_string()
                    } else {
                        php_type_ref(n)
                    };
                    format!("({a} instanceof {ty})")
                }
                ParamKind::Any => "true".to_string(),
            });
        }
        conds.join(" && ")
    }

    /// An enum with payload variants becomes an abstract base class plus one `final`
    /// subclass per variant, with promoted public props for the payload fields.
    pub(super) fn emit_enum(&mut self, e: &EnumDecl) -> Result<(), String> {
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
                    .map(|p| format!("public {} ${}", self.emit_type(&p.ty), p.name))
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

    pub(super) fn emit_class(&mut self, c: &ClassDecl) -> Result<(), String> {
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
        // M-faults 2b: a class `implements Error` becomes a real PHP exception — `extends \Exception`
        // (so `throw` targets a `\Throwable`, and native `getMessage()` works). The built-in `Error`
        // marker has no PHP declaration, so it is dropped from the `implements` list; any *other*
        // interfaces stay. A promoted/declared field whose name collides with one of `\Exception`'s
        // own properties (`message`/`code`/`file`/`line`) must be emitted **untyped** — PHP rejects a
        // typed redeclaration of an inherited untyped property.
        let is_error = c.implements.iter().any(|i| last_segment(i) == "Error");
        let other_ifaces: Vec<String> = c
            .implements
            .iter()
            .filter(|i| last_segment(i) != "Error")
            .map(|i| php_type_ref(i))
            .collect();
        let extends_clause = if is_error {
            " extends \\Exception".to_string()
        } else if let Some(parent) = c.extends.first() {
            // M-RT S6: single inheritance → PHP `extends Parent`. (Multiple parents lower via trait
            // decomposition in S6b.)
            format!(" extends {}", php_type_ref(parent))
        } else {
            String::new()
        };
        let implements = if other_ifaces.is_empty() {
            String::new()
        } else {
            format!(" implements {}", other_ifaces.join(", "))
        };
        // Declared inside its `namespace` block in multi-package mode ⇒ bare trailing segment.
        let disp = if self.namespaced {
            last_segment(&c.name)
        } else {
            &c.name
        };
        // M-RT S6: final-by-default — a non-`open` class emits as a PHP `final class` (it can never be
        // a parent, since the checker rejects `extends` of a non-`open` class via E-EXTEND-FINAL). An
        // `open` class emits as a plain `class` so a subclass may `extends` it.
        let final_kw = if c.open { "" } else { "final " };
        self.line(&format!(
            "{final_kw}class {disp}{extends_clause}{implements} {{"
        ));
        self.indent += 1;
        // M-RT S8: compose each `use`d trait. A non-conflicting `use Trait;` is emitted per trait
        // (trait-vs-trait conflict resolution emission — PHP `insteadof`/`as` — is a follow-up; the
        // checker rejects an *unresolved* collision, and the PHP oracle would catch a resolved one).
        for u in &c.uses {
            self.line(&format!("use {};", self.type_pos_ref(&u.name)));
        }
        let prev = self.cur_class_fields.replace(fields);
        self.emit_class_members(c, &promoted_names, is_error, false)?;
        self.cur_class_fields = prev;
        self.indent -= 1;
        self.line("}");
        Ok(())
    }

    /// M-RT S8: emit a native PHP `trait` from a [`crate::ast::TraitDecl`]. Members are emitted in
    /// trait mode (`as_trait = true`) — promoted ctor params become plain properties — reusing the
    /// shared `emit_class_members`. A trait is `package Main`-only this slice, so its name is bare.
    pub(super) fn emit_trait(&mut self, t: &crate::ast::TraitDecl) -> Result<(), String> {
        let mut promoted_names: HashSet<String> = HashSet::new();
        let mut fields: HashSet<String> = HashSet::new();
        for m in &t.members {
            match m {
                ClassMember::Constructor { params, .. } => {
                    for p in params {
                        if is_promoted(&p.modifiers) {
                            promoted_names.insert(p.name.clone());
                            fields.insert(p.name.clone());
                        }
                    }
                }
                ClassMember::Field { name, .. } => {
                    fields.insert(name.clone());
                }
                _ => {}
            }
        }
        let synthetic = ClassDecl {
            vis: crate::ast::Visibility::Public,
            name: t.name.clone(),
            type_params: Vec::new(),
            extends: Vec::new(),
            implements: Vec::new(),
            open: true,
            is_abstract: false,
            resolutions: Vec::new(),
            uses: Vec::new(),
            members: t.members.clone(),
            span: t.span,
        };
        let disp = if self.namespaced {
            last_segment(&t.name)
        } else {
            &t.name
        };
        self.line(&format!("trait {disp} {{"));
        self.indent += 1;
        let prev = self.cur_class_fields.replace(fields);
        // `as_trait = false`: a USER trait emits like a normal class body — including a real
        // `__construct` with promotion (M-RT S8 T3). PHP makes that `__construct` the using class's
        // constructor automatically (a class composes at most one trait ctor — the checker rejects two
        // via `E-TRAIT-CTOR-COLLISION`). This differs from the S6 MI decomposition, which uses
        // `as_trait = true` precisely to suppress colliding multi-parent trait ctors.
        self.emit_class_members(&synthetic, &promoted_names, false, false)?;
        self.cur_class_fields = prev;
        self.indent -= 1;
        self.line("}");
        Ok(())
    }

    /// Emit a class's members (fields, constructor, methods, hooks) — the shared body used by a plain
    /// `class` (`emit_class`) and a multi-parent class (`emit_multi_class`, M-RT S6b). The caller has
    /// already emitted the class header + opening `{`, raised the indent, and set `cur_class_fields`;
    /// it restores them after.
    ///
    /// `as_trait` (M-RT S6c.2b): when emitting a decomposed class's *trait* body, a constructor cannot
    /// be a `__construct` (two trait constructors collide fatally in PHP), so its promoted params are
    /// emitted as PLAIN `public` fields and its body is dropped — the construction logic moves to an
    /// explicit-assignment `__construct` on the concrete class / multi-parent subclass
    /// (`emit_synth_construct`).
    pub(super) fn emit_class_members(
        &mut self,
        c: &ClassDecl,
        promoted_names: &HashSet<String>,
        is_error: bool,
        as_trait: bool,
    ) -> Result<(), String> {
        let mut emitted_method_overloads: HashSet<String> = HashSet::new();
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
                    if modifiers.contains(&Modifier::Const) {
                        // A `const` class constant (Feature A) → a PHP **typed class constant**
                        // `[vis] const TYPE NAME = <literal>;` (PHP 8.3+; floor 8.5 ✓). Accessed
                        // `Class::NAME` (no `$`), distinct from a static field's `Class::$name`. The
                        // initializer is a checker-validated literal, so it round-trips byte-identically.
                        let init_php = match init {
                            Some(e) => self.emit_expr(e)?,
                            None => "null".to_string(),
                        };
                        self.line(&format!(
                            "{v} const {} {name} = {init_php};",
                            self.emit_type(ty)
                        ));
                    } else if modifiers.contains(&Modifier::Static) {
                        // A `static` field (M-mut.7) → PHP `public static <type> $name = <init>;`. The
                        // initializer is a literal constant (checker-enforced), so it round-trips.
                        let init_php = match init {
                            Some(e) => self.emit_expr(e)?,
                            None => "null".to_string(),
                        };
                        self.line(&format!(
                            "{v} static {} ${name} = {init_php};",
                            self.emit_type(ty)
                        ));
                    } else if is_error && exception_reserved(name) {
                        // Collides with an inherited \Exception property → emit untyped.
                        self.line(&format!("{v} ${name};"));
                    } else {
                        self.line(&format!("{v} {} ${name};", self.emit_type(ty)));
                    }
                }
                ClassMember::Constructor { params, body, .. } => {
                    // M-RT S6c.2b: in a decomposed class's trait, a constructor can't be `__construct`
                    // (two trait `__construct`s are a PHP fatal). Emit its promoted params as plain
                    // `public` fields (the trait owns the storage); the construction logic moves to the
                    // concrete class / multi-parent subclass via `emit_synth_construct`.
                    if as_trait {
                        for p in params {
                            if is_promoted(&p.modifiers) {
                                self.line(&format!(
                                    "public {} ${};",
                                    self.emit_type(&p.ty),
                                    p.name
                                ));
                            }
                        }
                        continue;
                    }
                    // M-faults 2c: a promoted `cause` param of marker-`Error` type on an Error subtype
                    // feeds PHP's native exception chain (`$previous`) — recognized by name + type so a
                    // mis-typed `cause` stays a plain field. Emitted as `?\Throwable` (the `$previous`
                    // type), not the engine `Error` class.
                    let is_cause = |p: &CtorParam| {
                        is_error
                            && !vis(&p.modifiers).is_empty()
                            && p.name == "cause"
                            && is_error_marker_type(&p.ty)
                    };
                    let ps: Vec<String> = params
                        .iter()
                        .map(|p| {
                            let v = vis(&p.modifiers);
                            // A promoted param whose name collides with an \Exception property is
                            // emitted untyped (PHP rejects a typed redeclaration); a plain param keeps
                            // its type (it is not a property).
                            let untyped = is_error && !v.is_empty() && exception_reserved(&p.name);
                            if is_cause(p) {
                                format!("{v} ?\\Throwable ${}", p.name)
                            } else if v.is_empty() {
                                format!("{} ${}", self.emit_type(&p.ty), p.name)
                            } else if untyped {
                                format!("{} ${}", v, p.name)
                            } else {
                                format!("{} {} ${}", v, self.emit_type(&p.ty), p.name)
                            }
                        })
                        .collect();
                    // For an Error subtype, feed \Exception's own stores via `parent::__construct`:
                    // `$message` (so native `getMessage()` works) and, when a conventional `cause` is
                    // promoted, `$cause` as the 3rd `$previous` arg (so `getPrevious()` reports the
                    // cause chain idiomatically — interop + the 2c bridge). `$code` is 0 (Phorge has no
                    // exception-code surface). Either, both, or neither may be present.
                    let has_message = is_error
                        && params
                            .iter()
                            .any(|p| !vis(&p.modifiers).is_empty() && p.name == "message");
                    let has_cause = params.iter().any(is_cause);
                    let parent_args = match (has_message, has_cause) {
                        (true, true) => Some("$message, 0, $cause"),
                        (false, true) => Some("\"\", 0, $cause"),
                        (true, false) => Some("$message"),
                        (false, false) => None,
                    };
                    // Feature B: this class's own expression field initializers lower into the ctor
                    // prelude (after promotion + any `parent::__construct`, before the body), so an
                    // initializer reads `this` and an earlier sibling — matching the Rust backends.
                    let field_inits = crate::ast::own_field_initializers(c);
                    if body.is_empty() && parent_args.is_none() && field_inits.is_empty() {
                        self.line(&format!("function __construct({}) {{}}", ps.join(", ")));
                    } else {
                        self.line(&format!("function __construct({}) {{", ps.join(", ")));
                        self.indent += 1;
                        self.push_scope();
                        for p in params {
                            self.declare(&p.name);
                        }
                        if let Some(args) = parent_args {
                            self.line(&format!("parent::__construct({args});"));
                        }
                        for (fname, init) in &field_inits {
                            let e = self.emit_expr(init)?;
                            self.line(&format!("$this->{fname} = {e};"));
                        }
                        for s in body {
                            self.emit_stmt(s)?;
                        }
                        self.pop_scope();
                        self.indent -= 1;
                        self.line("}");
                    }
                }
                ClassMember::Method(f) => {
                    // Group M-RT method overloads (methods of one name on this class).
                    let group: Vec<&FunctionDecl> = c
                        .members
                        .iter()
                        .filter_map(|mm| match mm {
                            ClassMember::Method(g) if g.name == f.name => Some(g),
                            _ => None,
                        })
                        .collect();
                    if group.len() > 1 {
                        if emitted_method_overloads.insert(f.name.clone()) {
                            self.emit_overload_set(&f.name, &group, true)?;
                        }
                    } else {
                        self.emit_function(f, true)?;
                    }
                }
                // A property hook (M-mut.7b) → a PHP 8.4 property hook. The hook is virtual (no
                // backing store), so it emits no default; the get expression and set block reference
                // *other* (real) fields. `public` because Phorge does not enforce field visibility.
                ClassMember::Hook {
                    ty, name, get, set, ..
                } => {
                    let pty = self.emit_type(ty);
                    self.line(&format!("public {pty} ${name} {{"));
                    self.indent += 1;
                    if let Some(g) = get {
                        let e = self.emit_expr(g)?;
                        self.line(&format!("get => {e};"));
                    }
                    if let Some((p, body)) = set {
                        self.line(&format!("set({pty} ${}) {{", p.name));
                        self.indent += 1;
                        self.push_scope();
                        self.declare(&p.name);
                        for s in body {
                            self.emit_stmt(s)?;
                        }
                        self.pop_scope();
                        self.indent -= 1;
                        self.line("}");
                    }
                    self.indent -= 1;
                    self.line("}");
                }
            }
        }
        // Feature B: a class with expression field initializers but NO constructor needs a synthesized
        // zero-arg `__construct` to run them (PHP property defaults can't be arbitrary expressions). Not
        // for a decomposed trait body (`as_trait`) — its construction is emitted via `emit_synth_construct`.
        if !as_trait
            && !c
                .members
                .iter()
                .any(|m| matches!(m, ClassMember::Constructor { .. }))
        {
            let field_inits = crate::ast::own_field_initializers(c);
            if !field_inits.is_empty() {
                self.line("function __construct() {");
                self.indent += 1;
                self.push_scope();
                for (fname, init) in &field_inits {
                    let e = self.emit_expr(init)?;
                    self.line(&format!("$this->{fname} = {e};"));
                }
                self.pop_scope();
                self.indent -= 1;
                self.line("}");
            }
        }
        Ok(())
    }

    /// M-RT S6b: emit a class that is an ancestor of some multi-parent class as the interface+trait
    /// decomposition PHP needs for multiple inheritance — `interface I<name>` (the type side, so a
    /// subtype is `instanceof` it), `trait T<name>` (the impl side, `use`d by subclasses), and a
    /// concrete `class <name> implements I<name> { use T<name>; }` so the class is still directly
    /// instantiable and single-`extends`able. An ancestor's own parents are decomposed too, so the
    /// interface `extends I<parent>` and the trait `use T<parent>` (which is how a diamond shared base
    /// auto-merges — both arms reach the same flattened trait method).
    /// M-RT S6c.2b: emit an explicit-assignment `__construct` from a class's constructor *plan*
    /// (`ast::ctor_plan`) — used where promotion cannot be (a decomposed concrete class and a
    /// multi-parent subclass, whose fields live in `use`d traits as plain properties). Params are the
    /// plan entries' params concatenated; the body sets each promoted param (`$this->p = $p;`) then runs
    /// each entry's body, in order — mirroring the interpreter's per-entry promote-then-body and the
    /// VM's `MakeInstance`-then-bodies. Emits nothing for an empty plan (a zero-arg class).
    pub(super) fn emit_synth_construct(
        &mut self,
        c: &ClassDecl,
        program: &Program,
    ) -> Result<(), String> {
        let plan = crate::ast::ctor_plan(program, &c.name);
        if plan.is_empty() {
            return Ok(());
        }
        let params: Vec<String> = plan
            .iter()
            .flat_map(|(ps, _)| ps.iter())
            .map(|p| format!("{} ${}", self.emit_type(&p.ty), p.name))
            .collect();
        self.line(&format!(
            "public function __construct({}) {{",
            params.join(", ")
        ));
        self.indent += 1;
        self.push_scope();
        for (ps, _) in &plan {
            for p in ps {
                self.declare(&p.name);
            }
        }
        for (ps, body) in &plan {
            for p in ps {
                if is_promoted(&p.modifiers) {
                    self.line(&format!("$this->{0} = ${0};", p.name));
                }
            }
            for s in body {
                self.emit_stmt(s)?;
            }
        }
        self.pop_scope();
        self.indent -= 1;
        self.line("}");
        Ok(())
    }

    pub(super) fn emit_decomposed_class(
        &mut self,
        c: &ClassDecl,
        program: &Program,
    ) -> Result<(), String> {
        // interface I<name> [extends I<parent>, …] { method signatures }
        let iparents: Vec<String> = c.extends.iter().map(|p| format!("I{p}")).collect();
        let iext = if iparents.is_empty() {
            String::new()
        } else {
            format!(" extends {}", iparents.join(", "))
        };
        self.line(&format!("interface I{}{} {{", c.name, iext));
        self.indent += 1;
        let mut sig_emitted: HashSet<String> = HashSet::new();
        for m in &c.members {
            if let ClassMember::Method(f) = m {
                // One signature per name (a PHP interface cannot redeclare a name; overload sets in a
                // decomposed class are rare and resolved by the trait body).
                if !sig_emitted.insert(f.name.clone()) {
                    continue;
                }
                let params: Vec<String> = f
                    .params
                    .iter()
                    .map(|p| format!("{} ${}", self.emit_type(&p.ty), p.name))
                    .collect();
                self.line(&format!(
                    "public function {}({}){};",
                    f.name,
                    params.join(", "),
                    self.ret_suffix(&f.ret)
                ));
            }
        }
        self.indent -= 1;
        self.line("}");

        // trait T<name> { [use T<parent>, …;] members }
        self.line(&format!("trait T{} {{", c.name));
        self.indent += 1;
        if !c.extends.is_empty() {
            let tparents: Vec<String> = c.extends.iter().map(|p| format!("T{p}")).collect();
            self.line(&format!("use {};", tparents.join(", ")));
        }
        let (promoted_names, fields, is_error) = self.class_field_context(c);
        let prev = self.cur_class_fields.replace(fields);
        // `as_trait = true`: promoted ctor params become plain fields, the constructor is NOT emitted
        // here (it would be a colliding trait `__construct`).
        self.emit_class_members(c, &promoted_names, is_error, true)?;
        self.cur_class_fields = prev;
        self.indent -= 1;
        self.line("}");

        // concrete class <name> implements I<name> { use T<name>; <explicit __construct> } — directly
        // instantiable + single-`extends`able. The constructor logic the trait dropped lives here as an
        // explicit-assignment ctor (M-RT S6c.2b).
        self.line(&format!("class {0} implements I{0} {{", c.name));
        self.indent += 1;
        self.line(&format!("use T{};", c.name));
        let prev = self.cur_class_fields.replace(self.class_field_context(c).1);
        self.emit_synth_construct(c, program)?;
        self.cur_class_fields = prev;
        self.indent -= 1;
        self.line("}");
        Ok(())
    }

    /// M-RT S6b: emit a multi-parent class (`class C extends A, B`) as a PHP class that `implements`
    /// each parent's interface and `use`s each parent's trait, with `insteadof`/`as` clauses resolving
    /// cross-parent method collisions (from the `use`/`rename`/`exclude` resolution clauses). A diamond
    /// shared base needs no clause — PHP auto-dedups a method reached identically through two traits.
    pub(super) fn emit_multi_class(
        &mut self,
        c: &ClassDecl,
        program: &Program,
    ) -> Result<(), String> {
        let iparents: Vec<String> = c.extends.iter().map(|p| format!("I{p}")).collect();
        let tparents: Vec<String> = c.extends.iter().map(|p| format!("T{p}")).collect();
        let final_kw = if c.open { "" } else { "final " };
        self.line(&format!(
            "{final_kw}class {} implements {} {{",
            c.name,
            iparents.join(", ")
        ));
        self.indent += 1;
        let clauses = self.build_trait_clauses(c, program);
        if clauses.is_empty() {
            self.line(&format!("use {};", tparents.join(", ")));
        } else {
            self.line(&format!("use {} {{", tparents.join(", ")));
            self.indent += 1;
            for cl in &clauses {
                self.line(cl);
            }
            self.indent -= 1;
            self.line("}");
        }
        let (promoted_names, fields, is_error) = self.class_field_context(c);
        let prev = self.cur_class_fields.replace(fields);
        self.emit_class_members(c, &promoted_names, is_error, false)?;
        // M-RT S6c.2b: a multi-parent class with no own constructor gets a synthesized orchestrating
        // `__construct` (explicit assignments + each parent body, from `ctor_plan`); its fields live in
        // the `use`d parent traits. A class that declares its own ctor already emitted it above.
        if !c
            .members
            .iter()
            .any(|m| matches!(m, ClassMember::Constructor { .. }))
        {
            self.emit_synth_construct(c, program)?;
        }
        self.cur_class_fields = prev;
        self.indent -= 1;
        self.line("}");
        Ok(())
    }

    /// The `insteadof`/`as` clauses for a multi-parent class's `use` block (M-RT S6b). A method name
    /// supplied by ≥2 direct parents with **distinct origins** is a real PHP trait collision needing
    /// `insteadof` (a diamond shared base — same origin through both arms — is skipped, PHP auto-merges
    /// it). The winner is the parent named by a `use P.m` clause, else the single parent left after
    /// `rename`/`exclude` remove the others; every other providing parent's trait is listed after
    /// `insteadof`. A class that overrides the method itself needs no clause (the class method wins). A
    /// `rename P.m as n` also emits `T<P>::m as n;`.
    pub(super) fn build_trait_clauses(&self, c: &ClassDecl, program: &Program) -> Vec<String> {
        use crate::ast::Resolution;
        let (origins, _conflicts) = crate::ast::class_method_origins(program);
        // method name -> [(direct parent, origin (declaring class, method))]
        let mut provides: std::collections::BTreeMap<String, Vec<(String, Origin)>> =
            std::collections::BTreeMap::new();
        for ((cls, name), origin) in &origins {
            if c.extends.contains(cls) {
                provides
                    .entry(name.clone())
                    .or_default()
                    .push((cls.clone(), origin.clone()));
            }
        }
        let own: std::collections::BTreeSet<&str> = c
            .members
            .iter()
            .filter_map(|m| match m {
                ClassMember::Method(f) => Some(f.name.as_str()),
                _ => None,
            })
            .collect();
        let mut clauses = Vec::new();
        for (m, entries) in &provides {
            let distinct: std::collections::BTreeSet<&Origin> =
                entries.iter().map(|(_, o)| o).collect();
            if distinct.len() < 2 || own.contains(m.as_str()) {
                continue; // diamond auto-merge, single source, or overridden by the class itself
            }
            let providing: std::collections::BTreeSet<String> =
                entries.iter().map(|(p, _)| p.clone()).collect();
            // The winner: `use P.m` names it; otherwise the one parent left after rename/exclude.
            let used = c.resolutions.iter().find_map(|r| match r {
                Resolution::Use { parent, method, .. } if method == m => Some(parent.clone()),
                _ => None,
            });
            let removed: std::collections::BTreeSet<String> = c
                .resolutions
                .iter()
                .filter_map(|r| match r {
                    Resolution::Rename { parent, method, .. } if method == m => {
                        Some(parent.clone())
                    }
                    Resolution::Exclude { parent, method, .. } if method == m => {
                        Some(parent.clone())
                    }
                    _ => None,
                })
                .collect();
            let winner = used.or_else(|| providing.iter().find(|p| !removed.contains(*p)).cloned());
            if let Some(w) = winner {
                let losers: Vec<String> = providing
                    .iter()
                    .filter(|p| **p != w)
                    .map(|p| format!("T{p}"))
                    .collect();
                if !losers.is_empty() {
                    clauses.push(format!("T{w}::{m} insteadof {};", losers.join(", ")));
                }
            }
        }
        for r in &c.resolutions {
            if let Resolution::Rename {
                parent,
                method,
                as_name,
                ..
            } = r
            {
                clauses.push(format!("T{parent}::{method} as {as_name};"));
            }
        }
        clauses
    }

    /// The `(promoted ctor-param names, instance-field set, is_error)` context a class body needs to
    /// emit its members — shared setup for `emit_class`, `emit_multi_class`, and `emit_decomposed_class`.
    pub(super) fn class_field_context(
        &self,
        c: &ClassDecl,
    ) -> (HashSet<String>, HashSet<String>, bool) {
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
        let mut fields: HashSet<String> = promoted_names.clone();
        for m in &c.members {
            if let ClassMember::Field { name, .. } = m {
                fields.insert(name.clone());
            }
        }
        let is_error = c.implements.iter().any(|i| last_segment(i) == "Error");
        (promoted_names, fields, is_error)
    }

    /// Emit a PHP `interface` (M-RT S2): the name, an optional `extends A, B` clause, and one
    /// abstract method signature per declared method (`public function name(params): ret;`). PHP
    /// interface methods are implicitly public + abstract, so only the signature is emitted.
    pub(super) fn emit_interface(&mut self, i: &crate::ast::InterfaceDecl) -> Result<(), String> {
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
                .map(|p| format!("{} ${}", self.emit_type(&p.ty), p.name))
                .collect();
            self.line(&format!(
                "public function {}({}){};",
                m.name,
                params.join(", "),
                self.ret_suffix(&m.ret)
            ));
        }
        self.indent -= 1;
        self.line("}");
        Ok(())
    }
}
