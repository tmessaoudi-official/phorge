//! PHP transpiler — program (M-Decomp W4). See mod.rs for the struct + core + entry points.

use super::*;

/// Feature B-static: the program's **non-literal** static-field initializers, as `(class, field,
/// init_expr)` in declaration order. These can't be PHP property defaults (PHP requires a constant
/// expression), so they are set once by a generated `__phorge_init_statics()` called before `main()`.
/// A literal static stays a plain PHP `static $x = <lit>;` default and is absent here.
fn runtime_static_inits(program: &Program) -> Vec<(&str, &str, &Expr)> {
    let mut out = Vec::new();
    for it in &program.items {
        if let Item::Class(c) = it {
            for m in &c.members {
                if let ClassMember::Field {
                    modifiers,
                    name,
                    init: Some(e),
                    ..
                } = m
                {
                    if modifiers.contains(&Modifier::Static)
                        && !modifiers.contains(&Modifier::Const)
                        && crate::value::const_literal(e).is_none()
                    {
                        out.push((c.name.as_str(), name.as_str(), e));
                    }
                }
            }
        }
    }
    out
}

/// The entry-point bootstrap shape (Batch-1 B): `(main takes an argv param, main returns int)`. Drives
/// the PHP call site — argv is passed as `array_slice($argv ?? [], 1)` (matching `Core.Process.args()`)
/// and an `int`-returning `main` is wrapped in `exit(…)` so the return value becomes the process exit
/// status. A `void` `main()` keeps the bare `main();` call (byte-identical to pre-Batch-1 B output).
fn main_entry_shape(program: &Program) -> (bool, bool) {
    match crate::ast::entry_point(program, "main") {
        Some((_, f)) => {
            let has_argv = !f.params.is_empty();
            let returns_int = matches!(&f.ret, Some(Type::Named { name, .. }) if name == "int");
            (has_argv, returns_int)
        }
        None => (false, false),
    }
}

/// The PHP statement that invokes the entry point (Batch-1 B/D), given the namespace prefix (`""` in
/// flat mode, `"\Main\"` namespaced). A top-level entry is `{prefix}main(...)`; a class-static entry
/// (Batch-1 D) is `{prefix}App::main(...)`. Empty string when the program has no entry (a library/web
/// file) — the caller guards on that too. Composes [`main_entry_shape`]'s argv + exit-code decisions.
fn main_bootstrap_stmt(program: &Program, ns_prefix: &str) -> String {
    let Some((entry_class, _)) = crate::ast::entry_point(program, "main") else {
        return String::new();
    };
    let (has_argv, returns_int) = main_entry_shape(program);
    let callee = match entry_class {
        Some(c) => format!("{ns_prefix}{c}::main"),
        None => format!("{ns_prefix}main"),
    };
    let call = if has_argv {
        format!("{callee}(array_slice($argv ?? [], 1))")
    } else {
        format!("{callee}()")
    };
    if returns_int {
        format!("exit({call});")
    } else {
        format!("{call};")
    }
}

/// Whether class `cls` declares its own `private`/`protected` constructor (Batch A). A static-field
/// initializer of such a class (the singleton pattern — `static C inst = new C(...)`) must run in the
/// class's own scope in PHP, else PHP rejects the construction from the global `__phorge_init_statics`
/// while the Phorge backends (which treat a static init as in-class) accept it — a byte-identity break.
fn class_has_restricted_ctor(program: &Program, cls: &str) -> bool {
    program.items.iter().any(|it| {
        matches!(it, Item::Class(c) if c.name == cls
            && c.members.iter().any(|m| matches!(m,
                ClassMember::Constructor { modifiers, .. }
                    if modifiers.iter().any(|md| matches!(md, Modifier::Private | Modifier::Protected)))))
    })
}

impl Transpiler {
    /// Pass 1 — index top-level names so call dispatch and match binding can resolve them.
    pub(super) fn collect(&mut self, program: &Program) {
        for item in &program.items {
            match item {
                Item::Function(f) => {
                    self.funcs.insert(f.name.clone());
                    // T6c: a free function's return kind — overloads with differing kinds collapse
                    // to `Other` (the safe fallback), since the call site can't pick statically.
                    let rk = f.ret.as_ref().map_or(OpKind::Other, kind_of_type);
                    match self.fn_ret_kinds.get(&f.name) {
                        Some(existing) if *existing != rk => {
                            self.fn_ret_kinds.insert(f.name.clone(), OpKind::Other);
                        }
                        None => {
                            self.fn_ret_kinds.insert(f.name.clone(), rk);
                        }
                        _ => {}
                    }
                }
                Item::Class(c) => {
                    self.classes.insert(c.name.clone());
                    // T6b: record this class's own field/hook/promoted-ctor-param operand kinds and
                    // its parents, so field reads (`p.x`, `this.x`) resolve to a native operand.
                    self.class_parents.insert(c.name.clone(), c.extends.clone());
                    let mut fields: HashMap<String, OpKind> = HashMap::new();
                    for m in &c.members {
                        match m {
                            ClassMember::Field { ty, name, .. }
                            | ClassMember::Hook { ty, name, .. } => {
                                fields.insert(name.clone(), kind_of_type(ty));
                            }
                            ClassMember::Constructor { params, .. } => {
                                // Promoted params (those with a visibility modifier) become fields;
                                // a non-promoted param is ctor-local and never read as `o.x`, so
                                // recording it is harmless.
                                for p in params {
                                    fields.insert(p.name.clone(), kind_of_type(&p.ty));
                                }
                            }
                            // T6c: method return kinds — differing overloads collapse to `Other`.
                            ClassMember::Method(f) => {
                                let key = (c.name.clone(), f.name.clone());
                                let rk = f.ret.as_ref().map_or(OpKind::Other, kind_of_type);
                                match self.method_ret_kinds.get(&key) {
                                    Some(existing) if *existing != rk => {
                                        self.method_ret_kinds.insert(key, OpKind::Other);
                                    }
                                    None => {
                                        self.method_ret_kinds.insert(key, rk);
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                    self.class_field_kinds.insert(c.name.clone(), fields);
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
                        // T6b: payload kinds (positional) for variant-payload match bindings.
                        self.variant_field_kinds.insert(
                            v.name.clone(),
                            v.fields.iter().map(|p| kind_of_type(&p.ty)).collect(),
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
                        self.emit_class(c, program)?;
                    }
                }
                Item::Interface(i) => self.emit_interface(i)?,
                // M-RT S8: a native PHP `trait` (composed by classes via `use`).
                Item::Trait(t) => self.emit_trait(t)?,
                // Aliases are expanded out of the AST before transpiling; arm only for exhaustiveness.
                Item::TypeAlias { .. } => {}
            }
        }
        // Feature B-static: runtime static initializers run once, before `main` (matching the Rust
        // backends' eager-at-startup eval). PHP hoists the function, so emitting its body after the
        // call is fine.
        let rt_statics = runtime_static_inits(program);
        // The interpreter auto-invokes `main`; PHP does not. Emit the call so the output
        // is a runnable program, not just definitions.
        // Batch-1 D: the entry may be a top-level `main` OR a class-static `main` (so the guard is
        // `entry_point`, not `funcs.contains("main")` — a static entry isn't a free function).
        if crate::ast::entry_point(program, "main").is_some() {
            if !rt_statics.is_empty() {
                self.line("__phorge_init_statics();");
            }
            let stmt = main_bootstrap_stmt(program, "");
            self.line(&stmt);
        }
        if !rt_statics.is_empty() {
            self.line("function __phorge_init_statics() {");
            self.indent += 1;
            for (cls, field, e) in &rt_statics {
                let v = self.emit_expr(e)?;
                if class_has_restricted_ctor(program, cls) {
                    // Run the initializer in the class's own scope so a `private`/`protected` ctor is
                    // callable here (the singleton pattern), matching the Phorge backends (Batch A).
                    self.line(&format!(
                        "{cls}::${field} = (\\Closure::bind(static fn() => {v}, null, {cls}::class))();"
                    ));
                } else {
                    self.line(&format!("{cls}::${field} = {v};"));
                }
            }
            self.indent -= 1;
            self.line("}");
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
                    Item::Class(c) => self.emit_class(c, program)?,
                    Item::Interface(i) => self.emit_interface(i)?,
                    _ => {}
                }
            }
            self.indent -= 1;
            self.line("}");
        }
        self.line("namespace {");
        self.indent += 1;
        if crate::ast::entry_point(program, "main").is_some() {
            let stmt = main_bootstrap_stmt(program, "\\Main\\");
            self.line(&stmt);
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
            // A zero divisor *throws* (Phorge faults on any division by zero): PHP `$a % 0` already
            // throws, but `fmod($a, 0.0)` would return `NAN`, so guard `$b == 0` first to agree.
            self.line("function __phorge_rem($a, $b) {");
            self.indent += 1;
            self.line("if ($b == 0) { throw new \\DivisionByZeroError(\"Modulo by zero\"); }");
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
        }
        // `__phorge_float` is needed by `__phorge_str` AND directly by a statically-float interpolation
        // hole (T6) — so it is emitted whenever either is in play, independent of the `__phorge_str`
        // dispatch helper above.
        if self.uses_str || self.uses_float || self.uses_json_encode || self.uses_math_number_format
        {
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
        if self.uses_reflect_kind {
            // `Reflect.kind` — the coarse, erasure-stable type tag, mirroring the Rust `reflect_kind`
            // arm exactly. Order is load-bearing: a PHP closure is BOTH `is_callable` and
            // `is_object`, so `is_callable` is tested first (Phorge closures ⇒ "callable", instances
            // and enum variants ⇒ "object"). Only tier-1 functions, so it is correct under `php -n`.
            self.line("function __phorge_kind($v) {");
            self.indent += 1;
            self.line("if (is_callable($v)) { return \"callable\"; }");
            self.line("if (is_object($v)) { return \"object\"; }");
            self.line("if (is_array($v)) { return \"array\"; }");
            self.line("if (is_int($v)) { return \"int\"; }");
            self.line("if (is_float($v)) { return \"float\"; }");
            self.line("if (is_bool($v)) { return \"bool\"; }");
            self.line("if (is_string($v)) { return \"string\"; }");
            self.line("return \"null\";");
            self.indent -= 1;
            self.line("}");
        }
        if self.uses_reflect_class_name {
            // `Reflect.className` — runtime class name for an object, else null. Mirrors the Rust
            // `reflect_class_name` arm: a closure is is_object in PHP but reports as not-a-class
            // (null) on both sides, so it is excluded. Single-evaluates `$v`. Tier-1 only (`php -n`).
            self.line("function __phorge_class_name($v) {");
            self.indent += 1;
            self.line("if (is_object($v) && !($v instanceof \\Closure)) { return get_class($v); }");
            self.line("return null;");
            self.indent -= 1;
            self.line("}");
        }
        if self.uses_reflect_tables {
            self.emit_reflect_table();
        }
        self.emit_json_helpers();
        if self.uses_text_parse_int {
            // Mirror Rust's `i64::from_str`: `^[+-]?[0-9]+$`, in i64 range, no surrounding whitespace.
            // PHP's `(int)` clamps on overflow (≠ Rust's None), so detect overflow by re-deriving the
            // magnitude digits from the cast value and comparing to the input's (sign + leading zeros
            // stripped) — a mismatch means it clamped. Tier-1 only (PCRE), correct under `php -n`.
            self.line("function __phorge_parse_int($s) {");
            self.indent += 1;
            self.line("if (preg_match('/^[+-]?[0-9]+$/', $s) !== 1) { return null; }");
            self.line("$n = (int)$s;");
            self.line("$neg = ($s[0] === '-');");
            self.line("$digits = ltrim(ltrim($s, '+-'), '0');");
            self.line("if ($digits === '') { $digits = '0'; }");
            self.line("if ((string)($neg ? -$n : $n) !== $digits) { return null; }");
            self.line("return $n;");
            self.indent -= 1;
            self.line("}");
        }
        if self.uses_text_parse_float {
            // Mirror the Rust `valid_float` grammar (strict / permissive), rejecting inf/nan, then cast.
            // PCRE only (tier-1, correct under `php -n`); `(float)` matches `f64::from_str` for the
            // accepted grammar (typical decimals; extreme-precision divergence is documented).
            self.line("function __phorge_parse_float($s, $permissive) {");
            self.indent += 1;
            self.line("$re = $permissive");
            self.line("    ? '/^[+-]?(?:[0-9]+\\.?[0-9]*|\\.[0-9]+)(?:[eE][+-]?[0-9]+)?$/'");
            self.line("    : '/^[+-]?[0-9]+(?:\\.[0-9]+)?(?:[eE][+-]?[0-9]+)?$/';");
            self.line("return preg_match($re, $s) === 1 ? (float)$s : null;");
            self.indent -= 1;
            self.line("}");
        }
        // --- Decimal (BCMath) helpers (M-NUM S1). Each mirrors the Rust `value::decimal_*` kernel:
        // derive operand scales from the strings, compute the result scale (add/sub = max, mul = sum),
        // call the matching `bc*` with that scale, then bounds-check the result's unscaled magnitude
        // against i128 range and `throw` the same `decimal overflow` body the Rust backends fault with
        // (the `agree_err` oracle classifies by body substring). BCMath is tier-1 (works under `php -n`).
        if self.uses_dec_add
            || self.uses_dec_sub
            || self.uses_dec_mul
            || self.uses_dec_rem
            || self.uses_dec_div_exact
            || self.uses_dec_div
            || self.uses_dec_round
        {
            // Scale of a BCMath decimal string = digits after the dot (0 if none). Matches the Rust
            // kernel deriving scale from `(unscaled, scale)`; a `bc*` result is always normalized.
            self.line("function __phorge_dec_scale($x) {");
            self.indent += 1;
            self.line("$p = strpos($x, '.');");
            self.line("return $p === false ? 0 : strlen($x) - $p - 1;");
            self.indent -= 1;
            self.line("}");
            // Fault if the result's unscaled magnitude leaves signed-i128 range, byte-identically to
            // the Rust `checked_*` overflow. The unscaled magnitude is the result digits with the dot
            // and sign removed; compared against i128::MAX (2^127 - 1) via `bccomp` (string-exact).
            self.line("function __phorge_dec_check($r) {");
            self.indent += 1;
            self.line("$digits = str_replace(['-', '.'], '', $r);");
            self.line("$digits = ltrim($digits, '0');");
            self.line("if ($digits === '') { $digits = '0'; }");
            self.line(
                "if (bccomp($digits, '170141183460469231731687303715887105727', 0) > 0) { \
                 throw new \\RuntimeException('decimal overflow'); }",
            );
            self.line("return $r;");
            self.indent -= 1;
            self.line("}");
        }
        if self.uses_dec_add {
            self.line("function __phorge_dec_add($a, $b) {");
            self.indent += 1;
            self.line("$s = max(__phorge_dec_scale($a), __phorge_dec_scale($b));");
            self.line("return __phorge_dec_check(bcadd($a, $b, $s));");
            self.indent -= 1;
            self.line("}");
        }
        if self.uses_dec_sub {
            self.line("function __phorge_dec_sub($a, $b) {");
            self.indent += 1;
            self.line("$s = max(__phorge_dec_scale($a), __phorge_dec_scale($b));");
            self.line("return __phorge_dec_check(bcsub($a, $b, $s));");
            self.indent -= 1;
            self.line("}");
        }
        if self.uses_dec_rem {
            // Exact decimal remainder (bare `%`): `bcmod` at `max(scales)`; a zero divisor throws,
            // matching the Rust `decimal_rem` fault ("any division by zero throws").
            self.line("function __phorge_dec_rem($a, $b) {");
            self.indent += 1;
            self.line("$s = max(__phorge_dec_scale($a), __phorge_dec_scale($b));");
            self.line(
                "if (bccomp($b, '0', $s) === 0) { throw new \\DivisionByZeroError('decimal modulo by zero'); }",
            );
            self.line("return __phorge_dec_check(bcmod($a, $b, $s));");
            self.indent -= 1;
            self.line("}");
        }
        if self.uses_dec_div_exact {
            // Exact-or-fault bare `decimal /`: divide at high precision, verify the quotient is exact
            // (bcmul back == dividend), strip trailing zeros to the canonical minimal form (matching
            // the Rust `decimal_div_exact` result), then i128-bound-check. A non-terminating quotient
            // fails the exactness check and throws; a zero divisor throws. Byte-identical to the Rust
            // kernel's fault boundary + minimal output.
            self.line("function __phorge_dec_div_exact($a, $b) {");
            self.indent += 1;
            self.line("$sb = __phorge_dec_scale($b);");
            self.line(
                "if (bccomp($b, '0', $sb) === 0) { throw new \\DivisionByZeroError('decimal division by zero'); }",
            );
            self.line("$prec = __phorge_dec_scale($a) + $sb + 80;");
            self.line("$q = bcdiv($a, $b, $prec);");
            self.line(
                "if (bccomp(bcmul($q, $b, $prec * 2), $a, $prec) !== 0) { throw new \\RuntimeException('decimal division is not exact'); }",
            );
            self.line(
                "if (strpos($q, '.') !== false) { $q = rtrim($q, '0'); $q = rtrim($q, '.'); }",
            );
            self.line("if ($q === '' || $q === '-' || $q === '-0') { $q = '0'; }");
            self.line("return __phorge_dec_check($q);");
            self.indent -= 1;
            self.line("}");
        }
        if self.uses_dec_mul {
            self.line("function __phorge_dec_mul($a, $b) {");
            self.indent += 1;
            self.line("$s = __phorge_dec_scale($a) + __phorge_dec_scale($b);");
            self.line("return __phorge_dec_check(bcmul($a, $b, $s));");
            self.indent -= 1;
            self.line("}");
        }
        if self.uses_dec_of {
            // `Decimal.of(s) -> decimal?`: validate the literal grammar (optional sign, digits with an
            // optional single fractional part — `12`, `12.34`, `.5`; NO exponent/underscore/whitespace)
            // with a PCRE, then bounds-check the i128 range; return the normalized string or null.
            // Mirrors the Rust `value::decimal_of` exactly. The string is already its own decimal form
            // (no `bc*` normalization needed — Phorge preserves trailing zeros as scale).
            self.line("function __phorge_dec_of($s) {");
            self.indent += 1;
            self.line("if (preg_match('/^[+-]?(?:[0-9]+(?:\\.[0-9]+)?|\\.[0-9]+)$/', $s) !== 1) { return null; }");
            self.line("$digits = ltrim(str_replace(['-', '+', '.'], '', $s), '0');");
            self.line("if ($digits === '') { $digits = '0'; }");
            self.line("if (bccomp($digits, '170141183460469231731687303715887105727', 0) > 0) { return null; }");
            // Normalize a leading `+` away (Phorge's render has no `+`); keep the scale (trailing zeros).
            self.line("return ltrim($s, '+');");
            self.indent -= 1;
            self.line("}");
        }
        // --- Decimal division + rounding (M-NUM S2). Replicate the Rust `value::round_div` kernel via
        // BCMath integer arithmetic on the *unscaled* integer strings (`bcdiv`/`bcmod` truncate toward
        // zero / take the dividend's sign — verified identical to Rust i128 `/`/`%`), so every rounding
        // mode matches `run`/`runvm` byte-for-byte. The `RoundingMode` enum value arrives as a PHP
        // object (`new HalfUp()` ⇒ an instance of the injected global class `HalfUp`); the helper reads
        // its short class name and switches on it, exactly as the Rust native reads `Value::Enum.variant`.
        if self.uses_dec_div || self.uses_dec_round {
            // Unscaled integer-string of a decimal string: drop the dot. `"19.99"`→`"1999"`,
            // `"-2.5"`→`"-25"`, `"100"`→`"100"`. Matches `(unscaled, _)` in the Rust `(unscaled, scale)`.
            self.line("function __phorge_dec_unscaled($x) {");
            self.indent += 1;
            self.line("return str_replace('.', '', $x);");
            self.indent -= 1;
            self.line("}");
            // Short (namespace-free) class name of the RoundingMode value — `HalfUp`, `Floor`, …
            self.line("function __phorge_round_mode($mode) {");
            self.indent += 1;
            self.line("$c = get_class($mode);");
            self.line("$p = strrpos($c, '\\\\');");
            self.line("return $p === false ? $c : substr($c, $p + 1);");
            self.indent -= 1;
            self.line("}");
            // round_div(n, d, mode) on integer strings — the verbatim Rust kernel. `n`/`d` are signed
            // integer strings; the caller guarantees `d != 0`. Returns the rounded integer string.
            self.line("function __phorge_round_div($n, $d, $mode) {");
            self.indent += 1;
            // 1. Normalise the divisor sign so d > 0 (quotient sign unchanged).
            self.line(
                "if (bccomp($d, '0', 0) < 0) { $n = bcmul($n, '-1', 0); $d = bcmul($d, '-1', 0); }",
            );
            // 2. Truncating quotient + dividend-signed remainder.
            self.line("$q = bcdiv($n, $d, 0);");
            self.line("$rem = bcmod($n, $d);");
            self.line("if (bccomp($rem, '0', 0) === 0) { return $q; }");
            // s = sign of the dividend.
            self.line("$s = bccomp($n, '0', 0) > 0 ? '1' : '-1';");
            // half-comparison without doubling: |rem| vs d - |rem| (both >= 0).
            self.line("$absRem = ltrim($rem, '-');");
            self.line("$comp = bcsub($d, $absRem, 0);");
            self.line("$cmp = bccomp($absRem, $comp, 0);"); // -1/0/1
            self.line("$mode = __phorge_round_mode($mode);");
            self.line("switch ($mode) {");
            self.indent += 1;
            self.line("case 'Down': return $q;");
            self.line("case 'Up': return bcadd($q, $s, 0);");
            self.line("case 'Ceiling': return bccomp($n, '0', 0) > 0 ? bcadd($q, '1', 0) : $q;");
            self.line("case 'Floor': return bccomp($n, '0', 0) < 0 ? bcadd($q, '-1', 0) : $q;");
            self.line("case 'HalfUp': return $cmp >= 0 ? bcadd($q, $s, 0) : $q;");
            self.line("case 'HalfDown': return $cmp > 0 ? bcadd($q, $s, 0) : $q;");
            self.line("case 'HalfEven':");
            self.indent += 1;
            self.line("if ($cmp > 0) { return bcadd($q, $s, 0); }");
            self.line("if ($cmp < 0) { return $q; }");
            // exact tie → round to even: bump only if q is currently odd.
            self.line("return bccomp(bcmod($q, '2'), '0', 0) !== 0 ? bcadd($q, $s, 0) : $q;");
            self.indent -= 1;
            self.line("default: throw new \\RuntimeException('unknown RoundingMode');");
            self.indent -= 1;
            self.line("}");
            self.indent -= 1;
            self.line("}");
            // Format a (bounds-checked) unscaled integer string at `scale` fractional digits — the
            // BCMath-padding form, matching the Rust `value::fmt_decimal` (never `-0`).
            self.line("function __phorge_dec_fmt($u, $scale) {");
            self.indent += 1;
            self.line("__phorge_dec_check($u);"); // i128 range guard (same overflow fault)
            self.line("$neg = bccomp($u, '0', 0) < 0;");
            self.line("$digits = ltrim($u, '-');");
            self.line("if ($scale === 0) { $body = $digits; }");
            self.line("else {");
            self.indent += 1;
            self.line("$digits = str_pad($digits, $scale + 1, '0', STR_PAD_LEFT);");
            self.line("$dot = strlen($digits) - $scale;");
            self.line("$body = substr($digits, 0, $dot) . '.' . substr($digits, $dot);");
            self.indent -= 1;
            self.line("}");
            self.line("return ($neg && bccomp($u, '0', 0) !== 0) ? '-' . $body : $body;");
            self.indent -= 1;
            self.line("}");
        }
        if self.uses_dec_div {
            // `Decimal.div(a, b, scale, mode)`: N = au*10^(sb+scale), D = bu*10^sa; round_div(N,D);
            // format at `scale`. scale<0 / b==0 throw the same bodies as the Rust kernel.
            self.line("function __phorge_dec_div($a, $b, $scale, $mode) {");
            self.indent += 1;
            self.line(
                "if ($scale < 0) { throw new \\RuntimeException('decimal scale out of range'); }",
            );
            self.line("$sa = __phorge_dec_scale($a); $sb = __phorge_dec_scale($b);");
            self.line("$au = __phorge_dec_unscaled($a); $bu = __phorge_dec_unscaled($b);");
            self.line("if (bccomp($bu, '0', 0) === 0) { throw new \\RuntimeException('decimal division by zero'); }");
            self.line("$N = bcmul($au, bcpow('10', (string)($sb + $scale), 0), 0);");
            self.line("$D = bcmul($bu, bcpow('10', (string)$sa, 0), 0);");
            self.line("$u = __phorge_round_div($N, $D, $mode);");
            self.line("return __phorge_dec_fmt($u, $scale);");
            self.indent -= 1;
            self.line("}");
        }
        if self.uses_dec_round {
            // `Decimal.round(d, scale, mode)`: up-scale is exact (u*10^Δ), down-scale rounds via
            // round_div(u, 10^Δ). scale<0 throws.
            self.line("function __phorge_dec_round($d, $scale, $mode) {");
            self.indent += 1;
            self.line(
                "if ($scale < 0) { throw new \\RuntimeException('decimal scale out of range'); }",
            );
            self.line("$sd = __phorge_dec_scale($d);");
            self.line("$u = __phorge_dec_unscaled($d);");
            self.line("if ($scale >= $sd) {");
            self.indent += 1;
            self.line("$r = bcmul($u, bcpow('10', (string)($scale - $sd), 0), 0);");
            self.indent -= 1;
            self.line("} else {");
            self.indent += 1;
            self.line("$divisor = bcpow('10', (string)($sd - $scale), 0);");
            self.line("$r = __phorge_round_div($u, $divisor, $mode);");
            self.indent -= 1;
            self.line("}");
            self.line("return __phorge_dec_fmt($r, $scale);");
            self.indent -= 1;
            self.line("}");
        }
        if self.uses_float_to_int {
            // `Convert.toInt($f) -> int?`: null on NaN/±∞/out-of-i64-range, else truncate toward zero.
            // The upper bound is the EXCLUSIVE `9.2233720368547758E18` (i64::MAX is not exactly f64-
            // representable); the lower bound is the exact i64::MIN as f64. Matches `value::float_to_int`,
            // and avoids PHP's surprising `(int)NAN == 0`.
            self.line("function __phorge_float_to_int($f) {");
            self.indent += 1;
            // `$t` is the truncate-toward-zero of `$f` (Rust `f64::trunc`): floor for >=0, ceil for <0.
            self.line("if (!is_finite($f)) { return null; }");
            self.line("$t = ($f < 0) ? ceil($f) : floor($f);");
            self.line(
                "return ($t >= -9.2233720368547758E18 && $t < 9.2233720368547758E18) ? (int)$t : null;",
            );
            self.indent -= 1;
            self.line("}");
        }
        if self.uses_dec_to_int {
            // `Convert.decimalToInt($s) -> int?`: the carrier string's integer part (before the dot),
            // truncated toward zero, or null if outside i64 range. Mirrors `value::decimal_to_int`
            // (i128 `unscaled / 10^scale`). Uses `bccomp` against the i64 bounds (BCMath is loaded for
            // decimals already). `(int)"123"` is exact for in-range integer strings.
            self.line("function __phorge_dec_to_int($s) {");
            self.indent += 1;
            self.line("$dot = strpos($s, '.');");
            self.line("$int = $dot === false ? $s : substr($s, 0, $dot);");
            self.line("if ($int === '' || $int === '-') { $int = '0'; }");
            self.line(
                "if (bccomp($int, '9223372036854775807', 0) > 0 || bccomp($int, '-9223372036854775808', 0) < 0) { return null; }",
            );
            self.line("return (int)$int;");
            self.indent -= 1;
            self.line("}");
        }
        if self.uses_float_to_int_exact {
            // `Convert.floatToIntExact($f) -> int?` (M4 `float as int`): integral-or-null, never a
            // silent truncate. Mirrors `value::float_to_int_exact` (`fmod==0` then the finite+range
            // guard of `__phorge_float_to_int`). `fmod(-3.0,1.0)` is `-0.0` (== 0.0 in PHP), so a
            // negative integral passes; `(int)$f` is exact for an integral in-range float.
            self.line("function __phorge_float_to_int_exact($f) {");
            self.indent += 1;
            self.line("if (!is_finite($f) || fmod($f, 1.0) != 0.0) { return null; }");
            self.line(
                "return ($f >= -9.2233720368547758E18 && $f < 9.2233720368547758E18) ? (int)$f : null;",
            );
            self.indent -= 1;
            self.line("}");
        }
        if self.uses_dec_to_int_exact {
            // `Convert.decimalToIntExact($s) -> int?` (M4 `decimal as int`): integral-or-null. The
            // carrier always renders exactly `scale` fractional digits, so a non-zero fraction
            // (after stripping trailing zeros) means non-integral → null. Mirrors
            // `value::decimal_to_int_exact` (`unscaled % 10^scale != 0`).
            self.line("function __phorge_dec_to_int_exact($s) {");
            self.indent += 1;
            self.line("$dot = strpos($s, '.');");
            self.line("if ($dot !== false) {");
            self.indent += 1;
            self.line("if (rtrim(substr($s, $dot + 1), '0') !== '') { return null; }");
            self.line("$int = substr($s, 0, $dot);");
            self.indent -= 1;
            self.line("} else { $int = $s; }");
            self.line("if ($int === '' || $int === '-') { $int = '0'; }");
            self.line(
                "if (bccomp($int, '9223372036854775807', 0) > 0 || bccomp($int, '-9223372036854775808', 0) < 0) { return null; }",
            );
            self.line("return (int)$int;");
            self.indent -= 1;
            self.line("}");
        }
        if self.uses_math_gcd {
            // `Math.gcd` — Euclid over the magnitudes (gmp is absent under `php -n`). Mirrors the Rust
            // `math_gcd` native body for every in-range input (the `i64::MIN` magnitude edge faults in
            // Phorge, never reached by a byte-identity example).
            self.line("function __phorge_gcd($a, $b) {");
            self.indent += 1;
            self.line("if ($a < 0) { $a = -$a; }");
            self.line("if ($b < 0) { $b = -$b; }");
            self.line("while ($b != 0) { $t = $b; $b = $a % $b; $a = $t; }");
            self.line("return $a;");
            self.indent -= 1;
            self.line("}");
        }
        if self.uses_math_number_format {
            // `Math.numberFormat($v, $d)` — digit-string rounding, mirroring `value::number_format`
            // byte-for-byte: round the *shortest-round-trip* decimal string (`__phorge_float`, identical
            // to Rust's `{}` Display) half-away-from-zero by carry — NOT `round($v * 10^$d)` — so the
            // `.5`-boundary divergence is gone (both legs round the intended decimal). Then group by
            // threes and join with `.`. Single-sourced here (NOT PHP's `number_format`).
            self.line("function __phorge_number_format($v, $d) {");
            self.indent += 1;
            self.line("if ($d < 0) { $d = 0; }");
            self.line("if (!is_finite($v)) { return __phorge_float($v); }");
            self.line("$s = __phorge_float($v);");
            self.line("$neg = ($s[0] ?? '') === '-';");
            self.line("if ($neg) { $s = substr($s, 1); }");
            self.line("$dot = strpos($s, '.');");
            self.line("$int = $dot === false ? $s : substr($s, 0, $dot);");
            self.line("$frac = $dot === false ? '' : substr($s, $dot + 1);");
            self.line("$intd = str_split($int);");
            self.line("$fracd = strlen($frac) > 0 ? str_split($frac) : [];");
            self.line("$round_up = isset($fracd[$d]) && ord($fracd[$d]) >= ord('5');");
            self.line("$fracd = array_slice($fracd, 0, $d);");
            self.line("while (count($fracd) < $d) { $fracd[] = '0'; }");
            self.line("if ($round_up) {");
            self.indent += 1;
            self.line("$carry = 1;");
            self.line("for ($i = count($fracd) - 1; $i >= 0 && $carry; $i--) {");
            self.indent += 1;
            self.line("$x = (ord($fracd[$i]) - 48) + $carry; $fracd[$i] = chr(48 + $x % 10); $carry = intdiv($x, 10);");
            self.indent -= 1;
            self.line("}");
            self.line("for ($i = count($intd) - 1; $i >= 0 && $carry; $i--) {");
            self.indent += 1;
            self.line("$x = (ord($intd[$i]) - 48) + $carry; $intd[$i] = chr(48 + $x % 10); $carry = intdiv($x, 10);");
            self.indent -= 1;
            self.line("}");
            self.line("if ($carry) { array_unshift($intd, chr(48 + $carry)); }");
            self.indent -= 1;
            self.line("}");
            self.line("while (count($intd) > 1 && $intd[0] === '0') { array_shift($intd); }");
            self.line(
                "$all_zero = !in_array(true, array_map(fn($c) => $c !== '0', array_merge($intd, $fracd)), true);",
            );
            self.line("$out = ($neg && !$all_zero) ? '-' : '';");
            self.line("$n = count($intd);");
            self.line("for ($i = 0; $i < $n; $i++) {");
            self.indent += 1;
            self.line("if ($i > 0 && ($n - $i) % 3 === 0) { $out .= ','; }");
            self.line("$out .= $intd[$i];");
            self.indent -= 1;
            self.line("}");
            self.line("if ($d > 0) { $out .= '.' . implode('', $fracd); }");
            self.line("return $out;");
            self.indent -= 1;
            self.line("}");
        }
        if self.uses_rng {
            // `Core.Random` — the SAME xorshift64 as the Rust kernel (`src/native/random.rs`), so a
            // seeded sequence is byte-identical across all backends. State persists in a by-reference
            // function-static (no global statement needed). `GOLDEN` is the signed-i64 reinterpretation
            // of `0x9E3779B97F4A7C15` (the unsigned literal exceeds PHP_INT_MAX → would parse as float).
            // PHP `>>` is arithmetic, so the `>> 7` masks the 7 sign-extended top bits to emulate Rust's
            // logical `u64 >>`. `next()` masks the high bit (`& PHP_INT_MAX`) for a non-negative i64.
            self.line("function &__phorge_rng_state() {");
            self.indent += 1;
            self.line("static $s = -7046029254386353131;");
            self.line("return $s;");
            self.indent -= 1;
            self.line("}");
            self.line("function __phorge_rng_step() {");
            self.indent += 1;
            self.line("$r = &__phorge_rng_state();");
            self.line("$x = $r;");
            self.line("$x ^= ($x << 13);");
            self.line("$x ^= (($x >> 7) & 0x01FFFFFFFFFFFFFF);");
            self.line("$x ^= ($x << 17);");
            self.line("$r = $x;");
            self.line("return $x;");
            self.indent -= 1;
            self.line("}");
            self.line("function __phorge_rng_seed($seed) {");
            self.indent += 1;
            self.line("$r = &__phorge_rng_state();");
            self.line("$r = $seed ^ (-7046029254386353131);");
            self.line("if ($r === 0) { $r = -7046029254386353131; }");
            self.line("return null;");
            self.indent -= 1;
            self.line("}");
            self.line("function __phorge_rng_next() {");
            self.indent += 1;
            self.line("return __phorge_rng_step() & PHP_INT_MAX;");
            self.indent -= 1;
            self.line("}");
            self.line("function __phorge_rng_int_between($lo, $hi) {");
            self.indent += 1;
            self.line("$span = $hi - $lo + 1;");
            self.line("return $lo + ((__phorge_rng_step() & PHP_INT_MAX) % $span);");
            self.indent -= 1;
            self.line("}");
        }
        if self.uses_list_sort {
            // Natural ascending over a COPY (Phorge lists are immutable). String by byte (`strcmp`,
            // ≡ Rust `String` Ord) — PHP's `<=>` would juggle numeric strings; ints/floats/bools via
            // `<=>` (≡ Rust numeric). `usort` is stable on PHP 8.0+ (≡ Rust `sort_by`).
            self.line("function __phorge_sort($xs) {");
            self.indent += 1;
            self.line("$ys = $xs;");
            self.line("usort($ys, function($a, $b) { return is_string($a) ? strcmp($a, $b) : ($a <=> $b); });");
            self.line("return $ys;");
            self.indent -= 1;
            self.line("}");
        }
        if self.uses_list_sort_with {
            // Comparator sort over a COPY; the user closure returns the `<=>`-style int directly.
            self.line("function __phorge_sort_with($xs, $cmp) {");
            self.indent += 1;
            self.line("$ys = $xs;");
            self.line("usort($ys, $cmp);");
            self.line("return $ys;");
            self.indent -= 1;
            self.line("}");
        }
        if self.uses_map_set {
            // A NEW map (Phorge maps are immutable). `$m` is passed by value, and PHP arrays are
            // copy-on-write, so assigning into it produces a fresh array — the caller's is untouched.
            self.line("function __phorge_map_set($m, $k, $v) {");
            self.indent += 1;
            self.line("$m[$k] = $v;");
            self.line("return $m;");
            self.indent -= 1;
            self.line("}");
        }
        if self.uses_map_remove {
            self.line("function __phorge_map_remove($m, $k) {");
            self.indent += 1;
            self.line("unset($m[$k]);");
            self.line("return $m;");
            self.indent -= 1;
            self.line("}");
        }
        if self.uses_list_index_of {
            // PHP `array_search($needle, $xs, true)` returns the int key or `false`; map `false` to
            // `null` for the `int?` return (strict `===` matches Phorge's `eq_val` for scalars).
            self.line("function __phorge_index_of($xs, $needle) {");
            self.indent += 1;
            self.line("$i = array_search($needle, $xs, true);");
            self.line("return $i === false ? null : $i;");
            self.indent -= 1;
            self.line("}");
        }
        if self.uses_text_index_of {
            // PHP `strpos` returns the byte offset or `false` (note: 0 is a valid offset); map only
            // `false` to `null` for the `int?` return.
            self.line("function __phorge_text_index_of($s, $needle) {");
            self.indent += 1;
            self.line("$i = strpos($s, $needle);");
            self.line("return $i === false ? null : $i;");
            self.indent -= 1;
            self.line("}");
        }
    }

    /// The `Core.Json` recursive helpers (each gated by its `uses_json_*` flag). They walk the injected
    /// `Json` enum's PHP class hierarchy — mangled variant classes `Null_`/`Bool_`/`Int_`/`Float_` and
    /// bare `Str`/`Arr`/`Obj` (the reserved-name mangle from this slice's prerequisite). Encoding
    /// mirrors the Rust `native::json` kernels byte-for-byte: a string scalar uses native
    /// `json_encode` (authoritative escaping); a float uses `__phorge_float` (positional shortest
    /// round-trip — NOT json's scientific notation, so it matches `run`/`runvm`); structure is
    /// hand-walked. Decoding delegates to native `json_decode` (objects → `stdClass` so `{}` ≠ `[]`),
    /// returning `null` (Phorge `None`) on any parse error, then rebuilds the enum hierarchy.
    fn emit_json_helpers(&mut self) {
        if self.uses_json_encode {
            self.line("function __phorge_json_encode($j) {");
            self.indent += 1;
            self.line("if ($j instanceof Null_) { return \"null\"; }");
            self.line("if ($j instanceof Bool_) { return $j->value ? \"true\" : \"false\"; }");
            self.line("if ($j instanceof Int_) { return (string)$j->value; }");
            self.line("if ($j instanceof Float_) { return __phorge_float($j->value); }");
            self.line("if ($j instanceof Str) { return json_encode($j->value); }");
            self.line("if ($j instanceof Arr) {");
            self.indent += 1;
            self.line("$parts = [];");
            self.line("foreach ($j->items as $x) { $parts[] = __phorge_json_encode($x); }");
            self.line("return \"[\" . implode(\",\", $parts) . \"]\";");
            self.indent -= 1;
            self.line("}");
            self.line("$parts = [];");
            self.line(
                "foreach ($j->entries as $k => $v) { $parts[] = json_encode((string)$k) . \":\" . __phorge_json_encode($v); }",
            );
            self.line("return \"{\" . implode(\",\", $parts) . \"}\";");
            self.indent -= 1;
            self.line("}");
        }
        if self.uses_json_pretty {
            self.line(
                "function __phorge_json_encode_pretty($j) { return __phorge_json_pretty($j, 0); }",
            );
            self.line("function __phorge_json_pretty($j, $indent) {");
            self.indent += 1;
            self.line("if ($j instanceof Arr && count($j->items) > 0) {");
            self.indent += 1;
            self.line("$pad = str_repeat(\" \", $indent + 4);");
            self.line("$parts = [];");
            self.line(
                "foreach ($j->items as $x) { $parts[] = $pad . __phorge_json_pretty($x, $indent + 4); }",
            );
            self.line(
                "return \"[\\n\" . implode(\",\\n\", $parts) . \"\\n\" . str_repeat(\" \", $indent) . \"]\";",
            );
            self.indent -= 1;
            self.line("}");
            self.line("if ($j instanceof Obj && count($j->entries) > 0) {");
            self.indent += 1;
            self.line("$pad = str_repeat(\" \", $indent + 4);");
            self.line("$parts = [];");
            self.line(
                "foreach ($j->entries as $k => $v) { $parts[] = $pad . json_encode((string)$k) . \": \" . __phorge_json_pretty($v, $indent + 4); }",
            );
            self.line(
                "return \"{\\n\" . implode(\",\\n\", $parts) . \"\\n\" . str_repeat(\" \", $indent) . \"}\";",
            );
            self.indent -= 1;
            self.line("}");
            self.line("return __phorge_json_encode($j);");
            self.indent -= 1;
            self.line("}");
        }
        if self.uses_json_decode {
            self.line("function __phorge_json_decode($s) {");
            self.indent += 1;
            self.line("$d = json_decode($s);");
            self.line("if (json_last_error() !== JSON_ERROR_NONE) { return null; }");
            self.line("return __phorge_json_build($d);");
            self.indent -= 1;
            self.line("}");
            self.line("function __phorge_json_build($d) {");
            self.indent += 1;
            self.line("if (is_null($d)) { return new Null_(); }");
            self.line("if (is_bool($d)) { return new Bool_($d); }");
            self.line("if (is_int($d)) { return new Int_($d); }");
            self.line("if (is_float($d)) { return new Float_($d); }");
            self.line("if (is_string($d)) { return new Str($d); }");
            self.line("if (is_array($d)) {");
            self.indent += 1;
            self.line("$items = [];");
            self.line("foreach ($d as $x) { $items[] = __phorge_json_build($x); }");
            self.line("return new Arr($items);");
            self.indent -= 1;
            self.line("}");
            self.line("$entries = [];");
            self.line(
                "foreach (get_object_vars($d) as $k => $v) { $entries[(string)$k] = __phorge_json_build($v); }",
            );
            self.line("return new Obj($entries);");
            self.indent -= 1;
            self.line("}");
        }
    }

    /// Emit `__phorge_reflect_of($v, $kind)` + its static table, built from the SAME `ClassTables` the
    /// Rust backends read — so `Reflect.interfaces`/`parents`/… are byte-identical by construction
    /// (no reliance on PHP's `class_implements`/`get_class_methods` with their own semantics). A
    /// non-object → `[]`; an unknown class / kind → `[]` (matching the Rust `unwrap_or_default`).
    fn emit_reflect_table(&mut self) {
        // The union of every class that appears in any table, in sorted (BTreeMap) order.
        let mut classes: std::collections::BTreeSet<&String> = std::collections::BTreeSet::new();
        for m in [
            &self.class_tables.interfaces,
            &self.class_tables.parents,
            &self.class_tables.methods,
            &self.class_tables.fields,
        ] {
            classes.extend(m.keys());
        }
        let php_list = |names: &[String]| -> String {
            let items: Vec<String> = names
                .iter()
                .map(|n| format!("'{}'", php_escape(n)))
                .collect();
            format!("[{}]", items.join(", "))
        };
        // Build every entry string up front (immutable borrow of `class_tables`), then emit (which
        // borrows `self` mutably via `line`) — avoids a borrow conflict.
        let empty = Vec::new();
        let entries: Vec<String> = classes
            .iter()
            .map(|c| {
                format!(
                    "'{}' => ['interfaces' => {}, 'parents' => {}, 'methods' => {}, 'fields' => {}],",
                    php_escape(c),
                    php_list(self.class_tables.interfaces.get(*c).unwrap_or(&empty)),
                    php_list(self.class_tables.parents.get(*c).unwrap_or(&empty)),
                    php_list(self.class_tables.methods.get(*c).unwrap_or(&empty)),
                    php_list(self.class_tables.fields.get(*c).unwrap_or(&empty)),
                )
            })
            .collect();
        self.line("function __phorge_reflect_of($v, $kind) {");
        self.indent += 1;
        self.line("if (!is_object($v)) { return []; }");
        self.line("static $t = [");
        self.indent += 1;
        for e in entries {
            self.line(&e);
        }
        self.indent -= 1;
        self.line("];");
        self.line("return $t[get_class($v)][$kind] ?? [];");
        self.indent -= 1;
        self.line("}");
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
        // Batch-1 D: a `static` method must be emitted `static` in PHP, else a class-static entry call
        // (`App::main()`) is a "non-static method called statically" fatal. Safe for every static
        // method: the checker forbids `this` inside one (`E-STATIC-THIS`), and PHP still permits an
        // instance-style call (`$m->staticMethod()`), so the existing `m.square(5)` pattern is unaffected.
        let static_prefix = if is_method && f.modifiers.contains(&Modifier::Static) {
            "static "
        } else {
            ""
        };
        self.line(&format!(
            "{}function {}({}){} {{",
            static_prefix,
            disp,
            params.join(", "),
            self.ret_suffix(&f.ret)
        ));
        self.indent += 1;
        self.push_scope();
        for p in &f.params {
            self.declare(&p.name);
            // T6: a typed param is a known operand kind for native-operator specialization.
            self.declare_kind(&p.name, kind_of_type(&p.ty));
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
        // Mangle a reserved enum-class name (`RoundingMode` → `RoundingMode_`) so it can't collide
        // with a PHP built-in enum (M-NUM S2); a non-reserved name is unchanged.
        let base = super::php_class_name(last_segment(&e.name));
        self.line(&format!("abstract class {} {{}}", base));
        for v in &e.variants {
            // A variant whose name is a PHP-reserved class word (`Int`/`Bool`/`Null`/…) is mangled
            // (`Int_`); the construction + `instanceof` sites mangle identically via `variant_ref`.
            let vname = super::php_variant_name(&v.name);
            self.line(&format!("final class {} extends {} {{", vname, base));
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

    pub(super) fn emit_class(&mut self, c: &ClassDecl, program: &Program) -> Result<(), String> {
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
        // M-RT S8 + Wave 1.3: compose each `use`d trait. A collision-free composition emits a plain
        // `use Trait;` per trait. When two composed traits supply the same method name (resolved on the
        // Phorge side by `use P.m`/`rename`/`exclude`), emit a single combined `use P, Q { … }` block
        // with the PHP `insteadof`/`as` clauses — otherwise PHP rejects the composition with a trait
        // method collision. Mirrors `build_trait_clauses` (the MI-decomposition analogue) for the
        // explicit trait-composition path. Trait names are used directly (no `T` prefix, unlike MI).
        if !c.uses.is_empty() {
            let clauses = self.build_use_trait_clauses(c, program);
            if clauses.is_empty() {
                for u in &c.uses {
                    self.line(&format!("use {};", self.type_pos_ref(&u.name)));
                }
            } else {
                let names: Vec<String> =
                    c.uses.iter().map(|u| self.type_pos_ref(&u.name)).collect();
                self.line(&format!("use {} {{", names.join(", ")));
                self.indent += 1;
                for cl in &clauses {
                    self.line(cl);
                }
                self.indent -= 1;
                self.line("}");
            }
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
        // T6b: `this` inside these method bodies resolves to `c`'s class for field-read kinds.
        let prev_class = self.cur_class.replace(c.name.clone());
        let result = self.emit_class_members_inner(c, promoted_names, is_error, as_trait);
        self.cur_class = prev_class;
        result
    }

    fn emit_class_members_inner(
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
                        // A `static` field → PHP `public static <type> $name`. A **literal** initializer
                        // round-trips as a PHP default (`= 0;`). A **non-literal** initializer (Feature
                        // B-static) can't be a PHP property default (PHP requires a constant expression),
                        // so the property is declared *without* a default and set once by
                        // `__phorge_init_statics()` before `main()`.
                        match init
                            .as_ref()
                            .filter(|e| crate::value::const_literal(e).is_some())
                        {
                            Some(e) => {
                                let init_php = self.emit_expr(e)?;
                                self.line(&format!(
                                    "{v} static {} ${name} = {init_php};",
                                    self.emit_type(ty)
                                ));
                            }
                            None => {
                                self.line(&format!("{v} static {} ${name};", self.emit_type(ty)));
                            }
                        }
                    } else if is_error && exception_reserved(name) {
                        // Collides with an inherited \Exception property → emit untyped.
                        self.line(&format!("{v} ${name};"));
                    } else {
                        self.line(&format!("{v} {} ${name};", self.emit_type(ty)));
                    }
                }
                ClassMember::Constructor {
                    modifiers,
                    params,
                    body,
                    ..
                } => {
                    // Batch A: a `private`/`protected constructor` emits the PHP visibility keyword on
                    // `__construct` (so PHP enforces it natively, matching the checker). A public/default
                    // ctor stays bare (`function __construct`) for byte-identity with prior output.
                    let cvis = match vis(modifiers) {
                        "private" => "private ",
                        "protected" => "protected ",
                        _ => "",
                    };
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
                        self.line(&format!(
                            "{cvis}function __construct({}) {{}}",
                            ps.join(", ")
                        ));
                    } else {
                        self.line(&format!("{cvis}function __construct({}) {{", ps.join(", ")));
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

    /// The `insteadof`/`as` clauses for an explicit trait-composition (`use P; use Q;`) block when two
    /// composed traits supply the same method name (Wave 1.3). The Phorge-side resolution
    /// (`use P.m`/`rename`/`exclude`) is already validated by the checker; this lowers it to PHP. The
    /// trait-composition analogue of [`build_trait_clauses`] (which handles MI-decomposed parents and
    /// uses `T<parent>` names): here the providing sources are the directly-declared methods of each
    /// `use`d trait, named directly. A method the class overrides itself, or supplied by only one trait,
    /// needs no clause. (A collision via a trait's *own* nested `use` is not detected here — only direct
    /// declarations; that narrower case is caught by the PHP oracle if it ever arises.)
    pub(super) fn build_use_trait_clauses(&self, c: &ClassDecl, program: &Program) -> Vec<String> {
        use crate::ast::{Item, Resolution};
        // Directly-declared method names of a `use`d trait.
        let trait_methods = |name: &str| -> std::collections::BTreeSet<String> {
            program
                .items
                .iter()
                .find_map(|it| match it {
                    Item::Trait(t) if t.name == name => Some(
                        t.members
                            .iter()
                            .filter_map(|m| match m {
                                ClassMember::Method(f) => Some(f.name.clone()),
                                _ => None,
                            })
                            .collect(),
                    ),
                    _ => None,
                })
                .unwrap_or_default()
        };
        // method name -> set of composed traits supplying it directly.
        let mut provides: std::collections::BTreeMap<String, std::collections::BTreeSet<String>> =
            std::collections::BTreeMap::new();
        for u in &c.uses {
            for m in trait_methods(&u.name) {
                provides.entry(m).or_default().insert(u.name.clone());
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
        for (m, traits) in &provides {
            if traits.len() < 2 || own.contains(m.as_str()) {
                continue; // single source, or the class overrides it (the class method wins)
            }
            // The winner: `use P.m` names it; else the one trait left after `rename`/`exclude`.
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
            let winner = used.or_else(|| traits.iter().find(|p| !removed.contains(*p)).cloned());
            if let Some(w) = winner {
                let losers: Vec<String> = traits.iter().filter(|p| **p != w).cloned().collect();
                if !losers.is_empty() {
                    clauses.push(format!("{w}::{m} insteadof {};", losers.join(", ")));
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
                clauses.push(format!("{parent}::{method} as {as_name};"));
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
