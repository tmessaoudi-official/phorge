//! PHP transpiler — types (M-Decomp W4). See mod.rs for the struct + core + entry points.

use super::*;

impl Transpiler {
    /// Render a `ClassName.field` static access as the PHP `ClassName::$field` lvalue (M-mut.7), or
    /// `None` if `object` is not a class name (then it is an instance member). Mirrors the backends'
    /// locals-first rule: a local binding shadowing a class name is an instance access.
    pub(super) fn static_ref(&self, object: &Expr, name: &str) -> Option<String> {
        if let Expr::Ident(cls, _) = object {
            if !self.is_local(cls) && self.classes.contains(cls) {
                return Some(format!("{cls}::${name}"));
            }
        }
        None
    }

    /// Render a type-name reference in a *type position* (param/return/field type, `instanceof` RHS,
    /// match type-pattern). M-RT S6c.3: a reference to a **decomposed** class (an ancestor of some
    /// multi-parent class, lowered to `interface I<name>` + `trait T<name>`) emits as its interface
    /// `I<name>` — a multi-parent subtype `implements I<name>` but does NOT `extends <name>`, so a
    /// `<name>`-typed slot or `instanceof <name>` would reject it under PHP. Construction (`new <name>`)
    /// and single `extends <name>` keep the concrete class (they use `php_type_ref` directly). S6 is
    /// `package Main`-only, so a decomposed name is bare ⇒ `I<name>` needs no namespace.
    pub(super) fn type_pos_ref(&self, name: &str) -> String {
        if self.decomposed.contains(name) {
            format!("I{name}")
        } else {
            php_type_ref(name)
        }
    }

    pub(super) fn emit_type(&self, ty: &Type) -> String {
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
                // The bottom type → PHP 8.1 native `never` (M-RT totality cluster). Valid only in
                // return position, which is where a `-> never` function uses it.
                "never" => "never".into(),
                // The two-type nothing model (S0a). `void` → PHP `: void` (forbids capture, matching
                // Phorge's uncapturable `void`). `Empty` → `mixed` (capturable: a `-> Empty` function
                // returns PHP `null`, which `: mixed` accepts — `: void` would make the result
                // uncapturable in PHP and break byte-identity for the holdable case).
                "void" => "void".into(),
                "Empty" => "mixed".into(),
                "List" | "Map" | "Set" => "array".into(),
                // enum / class / interface name (FQN if cross-package; `I<name>` if a decomposed
                // multi-inheritance ancestor — M-RT S6c.3).
                other => self.type_pos_ref(other),
            },
            // A union → PHP 8.0 native `A|B` (M-RT S4). Members emit via the same `emit_type`, so a
            // cross-package member would carry its FQN; dedup defensively (the checker already
            // guarantees ≥2 distinct members). `int|int` can't occur in a well-typed program.
            Type::Union(members, _) => {
                let mut parts: Vec<String> = Vec::new();
                for m in members {
                    let p = self.emit_type(m);
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
                    let p = self.emit_type(m);
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

    pub(super) fn ret_hint(&self, ret: &Option<Type>) -> String {
        match ret {
            // `Empty` return → NO PHP return hint (empty string). PHP then accepts a fall-off, a bare
            // `return;`, or `return null;` — all yielding a capturable `null`, the holdable-nothing
            // value. (`: mixed` would reject a fall-off — "none returned"; `: null` would reject a
            // bare `return;`.) `ret_suffix` drops the `:` entirely for this case.
            Some(Type::Named { name, .. }) if name == "Empty" => String::new(),
            Some(t) => self.emit_type(t),
            None => "void".into(),
        }
    }

    /// The PHP return-type suffix for a signature — `": int"`, `": void"`, … — or the empty string
    /// when there is no hint (an `Empty` return, see [`Self::ret_hint`]), so the `:` is dropped and
    /// PHP infers a capturable `null`.
    pub(super) fn ret_suffix(&self, ret: &Option<Type>) -> String {
        let hint = self.ret_hint(ret);
        if hint.is_empty() {
            String::new()
        } else {
            format!(": {hint}")
        }
    }
}
