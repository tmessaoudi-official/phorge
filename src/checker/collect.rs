//! `impl Checker` — collect cluster (M-Decomp W2). See checker/mod.rs for the struct + entry points.

use super::*;

impl Checker {
    /// Phase 1 — hoist all top-level declarations and the active import map. There is no longer a
    /// builtin prelude: every callable is namespaced ("nothing in the wind"), so even `println` must
    /// be reached as `console.println` after `import core.console;` (M3 Wave 1). A bare `println(…)`
    /// now resolves as an unknown function.
    pub(super) fn collect(&mut self, program: &Program) {
        use crate::ast::Item;
        self.imports = crate::native::import_map(&program.items);
        for item in &program.items {
            // PHP reserves a set of words in symbol positions (a free function / class / enum /
            // interface / trait / type-alias). Several are usable Phorge value identifiers (param /
            // field / local / property / method — e.g. `var`, `list`, `print`, `int`) but naming a
            // *symbol* with one would transpile to invalid PHP, so reject it here with a clear
            // diagnostic (kind-aware — `int` is a legal PHP function name but not a class name) rather
            // than letting the PHP oracle fail. Methods are collected inside `collect_class` (a class
            // body), so they are exempt (legal as `->list()` / `->var()`).
            if let Some((name, span, kind)) = reserved_symbol_decl(item) {
                if is_php_reserved_symbol_name(name, kind) {
                    self.err_coded(
                        span,
                        format!("`{name}` is a reserved word in PHP and cannot name a {kind}"),
                        "E-RESERVED-NAME",
                        Some(format!(
                            "`{name}` is fine as a variable, parameter, field, or method name — rename this {kind}"
                        )),
                    );
                    continue;
                }
            }
            match item {
                Item::Function(f) => self.collect_function(f),
                Item::Enum(e) => self.collect_enum(e),
                Item::Class(c) => self.collect_class(c),
                Item::Interface(i) => self.collect_interface(i),
                Item::Trait(t) => self.collect_trait(t),
                Item::Import { .. } => {} // import map already built above; nothing per-item to hoist
                Item::TypeAlias { name, ty, span } => {
                    if is_builtin_type_name(name) {
                        // Aliasing a built-in would make the checker (primitive wins) and the
                        // backend expansion (alias wins) disagree — reject it outright.
                        self.err(*span, format!("cannot redefine built-in type `{name}`"));
                    } else if self.aliases.contains_key(name) {
                        self.err(*span, format!("duplicate type name `{name}`"));
                    } else {
                        self.aliases.insert(name.clone(), ty.clone());
                    }
                }
            }
        }
        // Interfaces are fully registered now: validate the extends graph + every class's
        // `implements` (cycles, unknown names, method conformance) and build the shared
        // class→interface table the backends consume verbatim (M-RT S2).
        self.check_interface_graph(program);
    }

    /// M-RT S8: collect a trait by reusing the class machinery. A synthetic `ClassDecl` carries the
    /// trait's members into a [`ClassInfo`] (keyed by the trait name) so the trait body type-checks and
    /// the trait's members can be merged into each using class. Marked `is_abstract` so an abstract
    /// *requirement* method doesn't trip the concrete-class unimpl check on the trait itself; recorded
    /// in [`Self::traits`] so the name is rejected wherever a *type* is expected (a trait is reuse, not
    /// a type), and so construction (`Loud()`) is caught by the abstract-instantiate guard.
    pub(super) fn collect_trait(&mut self, t: &crate::ast::TraitDecl) {
        let synthetic = crate::ast::ClassDecl {
            vis: crate::ast::Visibility::Public,
            name: t.name.clone(),
            type_params: Vec::new(),
            extends: Vec::new(),
            implements: Vec::new(),
            open: false,
            is_abstract: true,
            resolutions: Vec::new(),
            uses: Vec::new(),
            members: t.members.clone(),
            span: t.span,
        };
        self.collect_class(&synthetic);
        self.traits.insert(t.name.clone());
    }

    pub(super) fn collect_interface(&mut self, i: &crate::ast::InterfaceDecl) {
        if is_builtin_type_name(&i.name) {
            self.err(
                i.span,
                format!("cannot redefine built-in type `{}`", i.name),
            );
            return;
        }
        if self.classes.contains_key(&i.name)
            || self.enums.contains_key(&i.name)
            || self.interfaces.contains_key(&i.name)
        {
            self.err(i.span, format!("type `{}` is already defined", i.name));
            return;
        }
        // Register the name first so a method signature may reference the interface itself.
        self.interfaces.insert(
            i.name.clone(),
            InterfaceInfo {
                methods: HashMap::new(),
                extends: i.extends.clone(),
            },
        );
        let mut methods = HashMap::new();
        for m in &i.methods {
            if methods.contains_key(&m.name) {
                self.err(
                    m.span,
                    format!("duplicate method `{}` in interface `{}`", m.name, i.name),
                );
                continue;
            }
            let params = m.params.iter().map(|p| self.resolve_type(&p.ty)).collect();
            // S0b: an interface method signature must declare its return type too (it never flows
            // through `check_function`, so enforce it here at collection).
            if m.ret.is_none() {
                self.err_coded(
                    m.span,
                    format!("interface method `{}` must declare a return type", m.name),
                    "E-MISSING-RETURN-TYPE",
                    Some(
                        "every function and method declares its return type — add `-> void` for a side-effecting method".into(),
                    ),
                );
            }
            let ret = match &m.ret {
                Some(t) => self.resolve_type(t),
                None => Ty::Void,
            };
            methods.insert(
                m.name.clone(),
                FnSig {
                    params,
                    // Default parameters are free-function-only in v1 (methods are a deferral); an
                    // interface method carries none.
                    defaults: vec![None; m.params.len()],
                    ret,
                    type_params: Vec::new(),
                    // Interface-method throws are not enforced through dynamic dispatch this slice
                    // (a documented deferral); keep the set empty so no call site mis-discharges.
                    throws: Vec::new(),
                },
            );
        }
        self.interfaces.get_mut(&i.name).unwrap().methods = methods;
    }

    /// Validate the interface graph and class conformance, then build [`Self::class_implements`].
    ///
    /// Reports `E-IFACE-CYCLE` (an `extends` cycle), `E-IFACE-IMPL` (a name in `implements`/`extends`
    /// that is not a declared interface), `E-IFACE-UNIMPL` (a class missing an interface method), and
    /// `E-IFACE-SIG` (a class method whose signature does not match the interface's).
    pub(super) fn check_interface_graph(&mut self, program: &crate::ast::Program) {
        use crate::ast::Item;
        // Always safe to compute (the shared fn is cycle-guarded); diagnostics below catch malformed
        // graphs, and the backends only run after a clean check, so a cyclic table never reaches them.
        self.class_implements = crate::ast::class_implements(program);
        self.class_supertypes = crate::ast::class_supertypes(program);

        // Class `extends` targets must be `open` classes; detect cycles (M-RT S6).
        let class_open: std::collections::HashMap<&str, bool> = program
            .items
            .iter()
            .filter_map(|it| match it {
                Item::Class(c) => Some((c.name.as_str(), c.open)),
                _ => None,
            })
            .collect();
        for item in &program.items {
            if let Item::Class(c) = item {
                if self
                    .class_supertypes
                    .get(&c.name)
                    .is_some_and(|s| s.contains(&c.name))
                {
                    self.err_coded(
                        c.span,
                        format!("class `{}` is part of an `extends` cycle", c.name),
                        "E-MI-CYCLE",
                        Some("a class may not extend itself transitively".into()),
                    );
                    continue; // skip per-parent checks for a cyclic class (avoids noise)
                }
                for parent in &c.extends {
                    if !self.classes.contains_key(parent) {
                        self.err_coded(
                            c.span,
                            format!(
                                "class `{}` extends `{parent}`, which is not a class",
                                c.name
                            ),
                            "E-EXTEND-UNKNOWN",
                            Some(
                                "`extends` lists parent classes; use `implements` for interfaces"
                                    .into(),
                            ),
                        );
                    } else if !class_open.get(parent.as_str()).copied().unwrap_or(false) {
                        self.err_coded(
                            c.span,
                            format!(
                                "class `{}` cannot extend `{parent}`, which is not `open`",
                                c.name
                            ),
                            "E-EXTEND-FINAL",
                            Some(format!(
                                "mark the parent `open class {parent}` to allow extension"
                            )),
                        );
                    }
                }
            }
        }

        // Inherit each class's ancestors' members into its `ClassInfo` (child wins on a clash),
        // before interface-conformance below — so an inherited method can satisfy an interface.
        self.inherit_class_members(program);

        // M-RT S6b: a `rename P.m as n` clause exposes parent `P`'s method `m` on the child under the
        // new name `n`, so a `child.n()` call type-checks (the backends dispatch it via the shared
        // origin table). `use`/`exclude` keep method names unchanged, so they need no signature edit.
        for item in &program.items {
            if let Item::Class(c) = item {
                for r in &c.resolutions {
                    if let crate::ast::Resolution::Rename {
                        parent,
                        method,
                        as_name,
                        ..
                    } = r
                    {
                        if let Some(sigs) = self
                            .classes
                            .get(parent)
                            .and_then(|p| p.methods.get(method))
                            .cloned()
                        {
                            if let Some(child) = self.classes.get_mut(&c.name) {
                                child.methods.entry(as_name.clone()).or_insert(sigs);
                            }
                        }
                    }
                }
            }
        }

        // M-RT S6: a method that overrides an ancestor's method requires that ancestor's method to be
        // `open` (final-by-default), else `E-OVERRIDE-FINAL`. (Signature-variance checking on override
        // is deferred — see KNOWN_ISSUES.) `method_open[(class, name)]` is true if the class declares
        // that name with at least one `open` overload.
        let mut method_open: std::collections::HashMap<(String, String), bool> =
            std::collections::HashMap::new();
        // Shared method-resolution order (nearest-first BFS over *every* parent) — the same table the
        // backends dispatch through, so the override check sees the exact ancestor a call would (M-RT
        // S6b: multi-parent, not just the first-parent chain).
        let mro = crate::ast::class_mro(program);
        for item in &program.items {
            if let Item::Class(c) = item {
                for m in &c.members {
                    if let crate::ast::ClassMember::Method(f) = m {
                        // An `abstract` method is implicitly `open` (it exists to be implemented).
                        let is_open = f.modifiers.contains(&crate::ast::Modifier::Open)
                            || f.modifiers.contains(&crate::ast::Modifier::Abstract);
                        method_open
                            .entry((c.name.clone(), f.name.clone()))
                            .and_modify(|v| *v = *v || is_open)
                            .or_insert(is_open);
                    }
                }
            }
        }
        for item in &program.items {
            if let Item::Class(c) = item {
                let mut checked: std::collections::BTreeSet<&str> =
                    std::collections::BTreeSet::new();
                for m in &c.members {
                    let crate::ast::ClassMember::Method(f) = m else {
                        continue;
                    };
                    if !checked.insert(f.name.as_str()) {
                        continue; // one diagnostic per overridden name
                    }
                    // Nearest ancestor (across every parent, nearest-first) that declares this name.
                    for anc in mro.get(&c.name).into_iter().flatten() {
                        if let Some(&open) = method_open.get(&(anc.clone(), f.name.clone())) {
                            if !open {
                                self.err_coded(
                                    f.span,
                                    format!(
                                        "method `{}` overrides `{anc}`'s `{}`, which is not `open`",
                                        f.name, f.name
                                    ),
                                    "E-OVERRIDE-FINAL",
                                    Some(format!(
                                        "mark it `open function {}(…)` on `{anc}` to allow overriding",
                                        f.name
                                    )),
                                );
                            }
                            break; // the nearest declaration decides
                        }
                    }
                }
            }
        }

        // M-RT S6b: an unresolved cross-parent method collision is `E-MI-CONFLICT`. The shared origin
        // resolver returns every name a class inherits from ≥2 distinct parents without a `use`/
        // `rename`/`exclude` clause (or own override) to disambiguate. A clean program produces an
        // empty list; the backends then dispatch through the same resolved table.
        let (origins, conflicts) = crate::ast::class_method_origins(program);
        for (class, name, span) in conflicts {
            self.err_coded(
                span,
                format!(
                    "method `{name}` is inherited from more than one parent of class `{class}`"
                ),
                "E-MI-CONFLICT",
                Some(format!(
                    "resolve it: `use P.{name}` to pick a parent, `rename P.{name} as <new>` to keep \
                     both, `exclude P.{name}` to drop one, or override `function {name}(…)` in `{class}`"
                )),
            );
        }

        // M-RT S6c.1: a same-named instance field inherited from ≥2 distinct parents is
        // `E-MI-FIELD-CONFLICT`. PHP has no `insteadof` for properties, so unlike a method collision
        // it can be resolved *only* by the child redeclaring the field (or renaming it in a parent).
        // A diamond-shared field (both arms reach the same declaring class) auto-merges, like methods.
        for (class, name, span) in crate::ast::class_field_conflicts(program) {
            self.err_coded(
                span,
                format!("field `{name}` is inherited from more than one parent of class `{class}`"),
                "E-MI-FIELD-CONFLICT",
                Some(format!(
                    "PHP has no `insteadof` for properties — redeclare `{name}` in `{class}` (or \
                     rename it in a parent) to resolve the collision"
                )),
            );
        }

        // M-RT S6b: abstract-method bookkeeping. `abstract_methods[(class, name)]` is set when a class
        // declares a bodyless `abstract function name`; `E-OPEN-STATIC` rejects a method that is both
        // `open` and `static` (statics are not virtual, so overridability is meaningless).
        let mut abstract_methods: std::collections::HashSet<(String, String)> =
            std::collections::HashSet::new();
        for item in &program.items {
            if let Item::Class(c) = item {
                for m in &c.members {
                    if let crate::ast::ClassMember::Method(f) = m {
                        if f.modifiers.contains(&crate::ast::Modifier::Abstract) {
                            abstract_methods.insert((c.name.clone(), f.name.clone()));
                        }
                        if f.modifiers.contains(&crate::ast::Modifier::Open)
                            && f.modifiers.contains(&crate::ast::Modifier::Static)
                        {
                            self.err_coded(
                                f.span,
                                format!("method `{}` is both `open` and `static`", f.name),
                                "E-OPEN-STATIC",
                                Some(
                                    "static methods are not virtual; drop `open` or `static`"
                                        .into(),
                                ),
                            );
                        }
                    }
                }
            }
            // M-RT S8: a trait's abstract method is a *requirement* on every using class. Recording it
            // under the trait name means the shared origins table (which maps a using class's method to
            // its `(trait, m)` origin) makes the same `E-ABSTRACT-UNIMPL` check below fire when a using
            // class leaves the requirement unmet.
            if let Item::Trait(t) = item {
                for m in &t.members {
                    if let crate::ast::ClassMember::Method(f) = m {
                        if f.modifiers.contains(&crate::ast::Modifier::Abstract) {
                            abstract_methods.insert((t.name.clone(), f.name.clone()));
                        }
                    }
                }
            }
        }
        // M-RT S8: every `use T;` must name a declared trait — not a class, interface, or unknown.
        for item in &program.items {
            if let Item::Class(c) = item {
                for u in &c.uses {
                    if !self.traits.contains(&u.name) {
                        let hint = if self.classes.contains_key(&u.name) {
                            "that name is a class — `use` composes a `trait`, `extends` inherits a class"
                        } else {
                            "declare it with `trait <Name> { … }`"
                        };
                        self.err_coded(
                            u.span,
                            format!("unknown trait `{}` in a `use` clause", u.name),
                            "E-USE-UNKNOWN",
                            Some(hint.into()),
                        );
                    }
                }
            }
        }
        // M-RT S8 (T3): trait-constructor footguns become clean ahead-of-time diagnostics (D5/D6/D8).
        for item in &program.items {
            let Item::Class(c) = item else { continue };
            let has_own_ctor = c
                .members
                .iter()
                .any(|m| matches!(m, crate::ast::ClassMember::Constructor { .. }));
            // Used traits (known + declaring a constructor), in source order.
            let trait_ctors: Vec<&str> = c
                .uses
                .iter()
                .filter(|u| {
                    self.classes
                        .get(&u.name)
                        .is_some_and(|t| t.has_ctor && self.traits.contains(&u.name))
                })
                .map(|u| u.name.as_str())
                .collect();
            if has_own_ctor {
                // The class's own ctor wins; any trait ctor is dead unless aliased (PHP P1).
                if let Some(t) = trait_ctors.first() {
                    self.warn_coded(
                        c.span,
                        format!(
                            "class `{}` declares its own constructor, so trait `{t}`'s constructor is never run",
                            c.name
                        ),
                        "W-TRAIT-CTOR-SHADOWED",
                        Some("remove the class ctor to use the trait's, or keep it (the trait ctor is intentionally shadowed)".into()),
                    );
                }
            } else if trait_ctors.len() >= 2 {
                // Two trait constructors collide — PHP would fatal; require a resolution.
                self.err_coded(
                    c.span,
                    format!(
                        "class `{}` composes constructors from multiple traits ({})",
                        c.name,
                        trait_ctors.join(", ")
                    ),
                    "E-TRAIT-CTOR-COLLISION",
                    Some("a class can compose at most one trait constructor; give one its own ctor or drop a trait".into()),
                );
            } else if trait_ctors.len() == 1 {
                // One trait ctor + a parent that has a ctor: the trait ctor wins, the parent's is not
                // auto-run (PHP P2) — surface the silent skip.
                let parent_has_ctor = c
                    .extends
                    .iter()
                    .any(|p| !crate::ast::ctor_plan(program, p).is_empty());
                if parent_has_ctor {
                    self.warn_coded(
                        c.span,
                        format!(
                            "class `{}` runs trait `{}`'s constructor; the parent constructor is not run",
                            c.name, trait_ctors[0]
                        ),
                        "W-TRAIT-CTOR-PARENT-SKIPPED",
                        Some("call the parent's initializer explicitly if it must run, or give the class its own ctor".into()),
                    );
                }
            }
        }
        // M-RT S6b: a concrete class must implement every abstract method it declares or inherits. The
        // shared dispatch table resolves each callable name to the body it runs; if that body is still
        // an abstract signature on a *non-abstract* class, the method is unimplemented. This one check
        // covers both "a concrete class declares an abstract method" (origin is itself) and "a concrete
        // subclass fails to override an inherited abstract method" (origin is an ancestor).
        if !abstract_methods.is_empty() {
            for item in &program.items {
                if let Item::Class(c) = item {
                    if c.is_abstract {
                        continue; // an abstract class may carry unimplemented abstract methods
                    }
                    let mut reported: std::collections::BTreeSet<&str> =
                        std::collections::BTreeSet::new();
                    for ((cls, name), (oc, om)) in &origins {
                        if cls != &c.name {
                            continue;
                        }
                        if abstract_methods.contains(&(oc.clone(), om.clone()))
                            && reported.insert(name.as_str())
                        {
                            self.err_coded(
                                c.span,
                                format!(
                                    "class `{}` must implement abstract method `{name}` from `{oc}`",
                                    c.name
                                ),
                                "E-ABSTRACT-UNIMPL",
                                Some(format!(
                                    "provide `function {name}(…)` in `{}`, or declare `{}` `abstract`",
                                    c.name, c.name
                                )),
                            );
                        }
                    }
                }
            }
        }

        // `extends` targets must be interfaces; detect cycles.
        for item in &program.items {
            if let Item::Interface(i) = item {
                for parent in &i.extends {
                    if !self.interfaces.contains_key(parent) {
                        self.err_coded(
                            i.span,
                            format!(
                                "interface `{}` extends `{parent}`, which is not an interface",
                                i.name
                            ),
                            "E-IFACE-IMPL",
                            Some("`extends` on an interface lists other interfaces".into()),
                        );
                    }
                }
                let mut visited = std::collections::BTreeSet::new();
                if self.iface_in_cycle(&i.name, &mut visited) {
                    self.err_coded(
                        i.span,
                        format!("interface `{}` is part of an `extends` cycle", i.name),
                        "E-IFACE-CYCLE",
                        Some("interfaces may not extend themselves transitively".into()),
                    );
                }
            }
        }

        // Class conformance: every interface method (own + inherited) must be provided.
        for item in &program.items {
            if let Item::Class(c) = item {
                for iface in &c.implements {
                    if !self.interfaces.contains_key(iface) {
                        self.err_coded(
                            c.span,
                            format!(
                                "class `{}` implements `{iface}`, which is not an interface",
                                c.name
                            ),
                            "E-IFACE-IMPL",
                            Some("`implements` lists declared interfaces".into()),
                        );
                        continue;
                    }
                    let required = self.iface_flat_methods(iface);
                    for (mname, sig) in &required {
                        match self
                            .classes
                            .get(&c.name)
                            .and_then(|ci| ci.methods.get(mname))
                        {
                            None => {
                                self.err_coded(
                                    c.span,
                                    format!(
                                        "class `{}` does not implement method `{mname}` required by interface `{iface}`",
                                        c.name
                                    ),
                                    "E-IFACE-UNIMPL",
                                    Some(format!("add `function {mname}(…)` to `{}`", c.name)),
                                );
                            }
                            Some(have) => {
                                if !self.sig_conforms(have, sig) {
                                    self.err_coded(
                                        c.span,
                                        format!(
                                            "class `{}` method `{mname}` does not match interface `{iface}`'s signature",
                                            c.name
                                        ),
                                        "E-IFACE-SIG",
                                        Some("the parameter types and return type must match the interface".into()),
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// Inherit ancestor-class members into each class's [`ClassInfo`] (M-RT S6). Every class's
    /// parents are fully merged first (so transitive members flow down), then a parent's
    /// fields/statics/methods/hooks are copied into the child where the child declares none of its
    /// own — the child's own member wins on a clash. Cycle-safe via the `done` set.
    pub(super) fn inherit_class_members(&mut self, program: &crate::ast::Program) {
        use crate::ast::Item;
        let parents: HashMap<String, Vec<String>> = program
            .items
            .iter()
            .filter_map(|it| match it {
                Item::Class(c) => Some((c.name.clone(), c.extends.clone())),
                _ => None,
            })
            .collect();
        let mut done: std::collections::HashSet<String> = std::collections::HashSet::new();
        let names: Vec<String> = parents.keys().cloned().collect();
        for name in names {
            self.merge_inherited(&name, &parents, &mut done);
        }
        // M-RT S8: after class inheritance is flattened, merge each `use`d trait's members (methods +
        // fields) into the using class's `ClassInfo`, so calling a trait method / reading a trait field
        // on the class type-checks. A class's own (and inherited) member of the same name wins
        // (`or_insert`); the dispatch table (`class_method_origins`) independently flags any unresolved
        // trait/parent/trait collision as `E-MI-CONFLICT`.
        let class_uses: Vec<(String, Vec<String>)> = program
            .items
            .iter()
            .filter_map(|it| match it {
                Item::Class(c) => Some((
                    c.name.clone(),
                    c.uses.iter().map(|u| u.name.clone()).collect(),
                )),
                _ => None,
            })
            .collect();
        for (cls, used) in class_uses {
            for t in used {
                let Some(tinfo) = self.classes.get(&t).cloned() else {
                    continue; // unknown trait — reported separately as E-USE-UNKNOWN
                };
                let Some(child) = self.classes.get_mut(&cls) else {
                    continue;
                };
                for (k, v) in &tinfo.fields {
                    if !child.fields.contains_key(k) {
                        child.fields.insert(k.clone(), v.clone());
                        if tinfo.mutable_fields.contains(k) {
                            child.mutable_fields.insert(k.clone());
                        }
                    }
                }
                for (k, v) in &tinfo.methods {
                    child.methods.entry(k.clone()).or_insert_with(|| v.clone());
                }
                // Wave 1.1: trait members flatten INTO the using class, so their visibility is
                // re-owned to `cls` (PHP `use` semantics — a trait's `private` member is accessible
                // from the using class, unlike an inherited `private` parent member). Own members win.
                for (k, v) in &tinfo.field_vis {
                    child
                        .field_vis
                        .entry(k.clone())
                        .or_insert((v.0, cls.clone()));
                }
                for (k, v) in &tinfo.method_vis {
                    child
                        .method_vis
                        .entry(k.clone())
                        .or_insert((v.0, cls.clone()));
                }
                // M-RT S8: a `use`d trait's `static` field becomes a per-using-class copy — each using
                // class gets its own `ClassName.field` (PHP `use` semantics). Merge it into the class's
                // static table so `C.field` type-checks; the backends seed a distinct slot per class.
                for (k, v) in &tinfo.statics {
                    if !child.statics.contains_key(k) {
                        child.statics.insert(k.clone(), v.clone());
                        if tinfo.static_mut.contains(k) {
                            child.static_mut.insert(k.clone());
                        }
                    }
                }
                // Feature A: a `use`d trait's constants flatten into the using class (own wins).
                for (k, v) in &tinfo.consts {
                    child.consts.entry(k.clone()).or_insert_with(|| v.clone());
                }
                // M-RT S8 (T4): a `use`d trait's property hooks flatten into the using class so
                // `c.hookName` resolves (the backends dispatch the synthetic `$get`/`$set` methods).
                for (k, v) in &tinfo.hooks {
                    child.hooks.entry(k.clone()).or_insert_with(|| v.clone());
                }
                // M-RT S8 (T3): a `use`d trait's constructor becomes the class's ctor signature,
                // replacing any inherited parent ctor (trait wins, PHP P2). The class's own ctor still
                // wins over both (`has_ctor` set in `collect_class`). First trait with a ctor wins for
                // the merge; two unresolved trait ctors are `E-TRAIT-CTOR-COLLISION`.
                if !child.has_ctor && tinfo.has_ctor {
                    child.ctor = tinfo.ctor.clone();
                    child.has_ctor = true;
                }
            }
        }
    }

    /// Ensure `name`'s [`ClassInfo`] has merged in all of its (already-merged) parents' members.
    /// Recurses parents-first; memoized via `done`, which also breaks any `extends` cycle.
    pub(super) fn merge_inherited(
        &mut self,
        name: &str,
        parents: &HashMap<String, Vec<String>>,
        done: &mut std::collections::HashSet<String>,
    ) {
        if !done.insert(name.to_string()) {
            return;
        }
        let Some(ps) = parents.get(name).cloned() else {
            return;
        };
        for p in &ps {
            self.merge_inherited(p, parents, done);
            let Some(parent_info) = self.classes.get(p).cloned() else {
                continue; // unknown parent — already reported E-EXTEND-UNKNOWN
            };
            let Some(child) = self.classes.get_mut(name) else {
                return;
            };
            for (k, v) in &parent_info.fields {
                if !child.fields.contains_key(k) {
                    child.fields.insert(k.clone(), v.clone());
                    if parent_info.mutable_fields.contains(k) {
                        child.mutable_fields.insert(k.clone());
                    }
                }
            }
            for (k, v) in &parent_info.statics {
                if !child.statics.contains_key(k) {
                    child.statics.insert(k.clone(), v.clone());
                    if parent_info.static_mut.contains(k) {
                        child.static_mut.insert(k.clone());
                    }
                }
            }
            // Feature A: inherit class constants (own/nearer wins). The `owner` in each `ConstEntry`
            // is preserved, so a `private`/`protected` access is checked against the declaring class —
            // `Sub.MAX` resolves an inherited `MAX` and PHP's `Sub::MAX` resolves it the same way.
            for (k, v) in &parent_info.consts {
                child.consts.entry(k.clone()).or_insert_with(|| v.clone());
            }
            for (k, v) in &parent_info.methods {
                child.methods.entry(k.clone()).or_insert_with(|| v.clone());
            }
            // Wave 1.1: inherit member visibility, **preserving the declaring owner** (like consts) —
            // so an inherited `private` member is checked against the parent (not visible from the
            // child, matching PHP) while a `protected` one is (the child is a subtype of the owner).
            for (k, v) in &parent_info.field_vis {
                child
                    .field_vis
                    .entry(k.clone())
                    .or_insert_with(|| v.clone());
            }
            for (k, v) in &parent_info.method_vis {
                child
                    .method_vis
                    .entry(k.clone())
                    .or_insert_with(|| v.clone());
            }
            for (k, v) in &parent_info.hooks {
                child.hooks.entry(k.clone()).or_insert_with(|| v.clone());
            }
            // M-RT S6c.2: a class with no own constructor inherits its parents' constructor
            // signature(s) for `ClassName(args)` type-checking — single inheritance takes the one
            // parent's, multiple inheritance **concatenates** every parent's in `extends` order (the
            // orchestrating ctor's params, matching the interpreter's plan + the compiler's flattened
            // descriptor). Parents merge first, so each `parent_info.ctor` is already its *effective*
            // (own-or-inherited) signature; appending across the loop builds the full concatenation. A
            // class declaring its own ctor keeps it (the deferred parent-forwarding case, KNOWN_ISSUES).
            if !child.has_ctor {
                child.ctor.extend(parent_info.ctor.iter().cloned());
                // Inherit the constructor's visibility + declaring owner (Batch A) from the first
                // parent in `extends` order — so `new Child(...)` is gated by the inherited ctor's
                // visibility (an inherited `private`/`protected __construct` blocks external
                // construction, matching PHP). `ctor_owner` defaults to the child's own name, so a
                // public-by-default chain is unaffected; only a non-public parent ctor changes it.
                if child.ctor_owner == name && parent_info.ctor_vis != MemberVis::Public {
                    child.ctor_vis = parent_info.ctor_vis;
                    child.ctor_owner = parent_info.ctor_owner.clone();
                }
            }
        }
    }

    /// True if `name`'s `extends` chain reaches `name` again (a cycle). Visited-guarded.
    pub(super) fn iface_in_cycle(
        &self,
        name: &str,
        stack: &mut std::collections::BTreeSet<String>,
    ) -> bool {
        fn walk(
            this: &Checker,
            cur: &str,
            target: &str,
            seen: &mut std::collections::BTreeSet<String>,
        ) -> bool {
            let Some(info) = this.interfaces.get(cur) else {
                return false;
            };
            for parent in &info.extends {
                if parent == target {
                    return true;
                }
                if seen.insert(parent.clone()) && walk(this, parent, target, seen) {
                    return true;
                }
            }
            false
        }
        walk(self, name, name, stack)
    }

    /// An interface's flattened method set: its own methods plus every (transitive) parent's,
    /// the child's signature winning on a name clash. Cycle-guarded.
    pub(super) fn iface_flat_methods(&self, name: &str) -> Vec<(String, (Vec<Ty>, Ty))> {
        let mut acc: HashMap<String, (Vec<Ty>, Ty)> = HashMap::new();
        let mut seen = std::collections::BTreeSet::new();
        self.iface_collect_methods(name, &mut acc, &mut seen);
        let mut out: Vec<_> = acc.into_iter().collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    pub(super) fn iface_collect_methods(
        &self,
        name: &str,
        acc: &mut HashMap<String, (Vec<Ty>, Ty)>,
        seen: &mut std::collections::BTreeSet<String>,
    ) {
        if !seen.insert(name.to_string()) {
            return;
        }
        let Some(info) = self.interfaces.get(name) else {
            return;
        };
        // Parents first, so a child interface's own signature overrides on a clash.
        for parent in &info.extends {
            self.iface_collect_methods(parent, acc, seen);
        }
        for (m, sig) in &info.methods {
            acc.insert(m.clone(), (sig.params.clone(), sig.ret.clone()));
        }
    }

    /// A class method conforms to an interface signature when arities match and each parameter type
    /// and the return type are equal (exact — no variance this slice, matching `assignable`'s
    /// function rule).
    /// `have` is the class's overload set for the method name (M-RT overloading): conformance holds
    /// when *any* overload matches the interface signature exactly.
    pub(super) fn sig_conforms(&self, have: &[FnSig], want: &(Vec<Ty>, Ty)) -> bool {
        have.iter().any(|h| {
            h.params.len() == want.0.len()
                && h.params.iter().zip(&want.0).all(|(a, b)| a == b)
                && h.ret == want.1
        })
    }

    /// Nominal subtyping for assignability and `instanceof`: `a` is a subtype of `b` when they are
    /// equal, when class `a` implements interface `b` (transitively, via [`Self::class_implements`]),
    /// when class `a` extends class `b` (transitively, via [`Self::class_supertypes`] — M-RT S6), or
    /// when interface `a` extends interface `b` (transitively).
    pub(super) fn is_subtype(&self, a: &str, b: &str) -> bool {
        if a == b {
            return true;
        }
        if self
            .class_implements
            .get(a)
            .is_some_and(|ifaces| ifaces.iter().any(|i| i == b))
        {
            return true;
        }
        // class `a` extends class `b` transitively (M-RT S6)?
        if self
            .class_supertypes
            .get(a)
            .is_some_and(|sup| sup.iter().any(|s| s == b))
        {
            return true;
        }
        // interface `a` extends `b` transitively?
        if self.interfaces.contains_key(a) {
            let mut seen = std::collections::BTreeSet::new();
            return self.iface_in_cycle_to(a, b, &mut seen);
        }
        false
    }

    pub(super) fn iface_in_cycle_to(
        &self,
        cur: &str,
        target: &str,
        seen: &mut std::collections::BTreeSet<String>,
    ) -> bool {
        let Some(info) = self.interfaces.get(cur) else {
            return false;
        };
        for parent in &info.extends {
            if parent == target {
                return true;
            }
            if seen.insert(parent.clone()) && self.iface_in_cycle_to(parent, target, seen) {
                return true;
            }
        }
        false
    }

    /// Context-aware assignability: [`Ty::assignable`] plus this checker's nominal subtyping.
    pub(super) fn ty_assignable(&self, from: &Ty, to: &Ty) -> bool {
        Ty::assignable_with(from, to, &|a, b| self.is_subtype(a, b))
    }

    pub(super) fn collect_function(&mut self, f: &crate::ast::FunctionDecl) {
        if is_intrinsic_name(&f.name) {
            self.err_coded(
                f.span,
                format!(
                    "`{}` is a reserved built-in intrinsic and cannot be redefined",
                    f.name
                ),
                "E-RESERVED-INTRINSIC",
                Some("panic/todo/unreachable/assert are built in (M-faults)".into()),
            );
            return;
        }
        self.validate_type_params(&f.type_params, f.span);
        self.reject_dup_param_names(f.params.iter().map(|p| (p.name.as_str(), p.span)));
        // Resolve the signature with the type parameters in scope so `T` becomes `Ty::Param("T")`.
        self.active_type_params = f.type_params.clone();
        let params: Vec<Ty> = f.params.iter().map(|p| self.resolve_type(&p.ty)).collect();
        let ret = match &f.ret {
            Some(t) => self.resolve_type(t),
            None => Ty::Void,
        };
        // Resolve the declared throws set with the type parameters still in scope, then clear. A
        // union `throws A | B` is flattened to its members (`throws` is a set of exception types).
        let throws = Self::flatten_throws(f.throws.iter().map(|t| self.resolve_type(t)).collect());
        self.active_type_params.clear();
        // M-RT overloading: a same-named function joins an overload set rather than colliding. The
        // set must share a return type and hold no two identical signatures; a generic overload is
        // not allowed to participate (deferred). Push regardless of legality so downstream resolution
        // sees the whole set (errors already reported).
        // M4 default parameters (free functions only in v1): validate trailing-only ordering,
        // literal-only values, and type assignability, building the per-param default list.
        let defaults = self.collect_param_defaults(&f.params, &params);
        let sig = FnSig {
            params,
            defaults,
            ret,
            type_params: f.type_params.clone(),
            throws,
        };
        let existing = self.funcs.get(&f.name).cloned().unwrap_or_default();
        self.validate_new_overload(&existing, &sig, &f.name, f.span, "function");
        self.funcs.entry(f.name.clone()).or_default().push(sig);
    }

    /// M4 default parameters: build the per-parameter default list for a free function, validating
    /// (a) trailing-only ordering — a defaulted parameter may not be followed by a required one
    /// (`E-DEFAULT-PARAM-ORDER`); (b) literal-only values (`E-DEFAULT-PARAM-EXPR`); (c) the default
    /// literal's type is assignable to the parameter type (`E-DEFAULT-PARAM-TYPE`). `resolved` is the
    /// already-resolved parameter types (parallel to `params`). Errors only — the list is returned
    /// regardless so the fill pass and arity check see the declared shape.
    pub(super) fn collect_param_defaults(
        &mut self,
        params: &[crate::ast::Param],
        resolved: &[Ty],
    ) -> Vec<Option<crate::ast::Expr>> {
        let mut out = Vec::with_capacity(params.len());
        let mut seen_default = false;
        for (p, pty) in params.iter().zip(resolved) {
            match &p.default {
                None => {
                    if seen_default {
                        self.err_coded(
                            p.span,
                            format!(
                                "required parameter `{}` cannot follow a parameter with a default",
                                p.name
                            ),
                            "E-DEFAULT-PARAM-ORDER",
                            Some("move every defaulted parameter to the end of the list".into()),
                        );
                    }
                    out.push(None);
                }
                Some(e) => {
                    seen_default = true;
                    match literal_ty(e) {
                        None => {
                            self.err_coded(
                                Self::expr_span(e),
                                format!(
                                    "default value for `{}` must be a literal constant",
                                    p.name
                                ),
                                "E-DEFAULT-PARAM-EXPR",
                                Some(
                                    "use a literal — a number, string, bool, bytes, or null".into(),
                                ),
                            );
                        }
                        Some(lt) => {
                            if !self.ty_assignable(&lt, pty) {
                                self.err_coded(
                                    Self::expr_span(e),
                                    format!(
                                        "default value of type `{lt}` is not assignable to parameter `{}` of type `{pty}`",
                                        p.name
                                    ),
                                    "E-DEFAULT-PARAM-TYPE",
                                    None,
                                );
                            }
                        }
                    }
                    out.push(Some((**e).clone()));
                }
            }
        }
        out
    }

    /// M4 default parameters are free-function-only in v1. Reject a default on any method / constructor
    /// parameter (`E-DEFAULT-PARAM-CONTEXT`) — the `fill_defaults` pass resolves free/native calls, not
    /// method dispatch, so a method default would silently never apply. (A documented deferral.)
    pub(super) fn reject_member_defaults(&mut self, params: &[crate::ast::Param], context: &str) {
        for p in params {
            if let Some(e) = &p.default {
                self.err_coded(
                    Self::expr_span(e),
                    format!(
                        "default parameter values are not yet supported on a {context} (only on free functions)"
                    ),
                    "E-DEFAULT-PARAM-CONTEXT",
                    Some("drop the default, or call the function explicitly with all arguments".into()),
                );
            }
        }
    }

    /// Reject duplicate parameter names (Soundness Batch G, finding #7) on a function/method/ctor
    /// signature — previously the last declaration silently won (`E-DUP-PARAM`). Takes `(name, span)`
    /// pairs so it serves both `Param` and `CtorParam` sites.
    pub(super) fn reject_dup_param_names<'a>(
        &mut self,
        params: impl Iterator<Item = (&'a str, Span)>,
    ) {
        let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for (name, span) in params {
            if !seen.insert(name) {
                self.err_coded(
                    span,
                    format!("duplicate parameter `{name}`"),
                    "E-DUP-PARAM",
                    Some("each parameter must have a distinct name".into()),
                );
            }
        }
    }

    /// M-RT overloading: validate a new overload `sig` against the overloads of the same name already
    /// collected in `existing`. Emits diagnostics only; the caller pushes `sig` regardless so later
    /// resolution sees the full set. A legal set shares one return type (`E-OVERLOAD-RETURN`) and has
    /// no two identical parameter signatures (`E-OVERLOAD-DUPLICATE`); a generic member cannot
    /// participate (`E-OVERLOAD-GENERIC`, deferred). The first declaration is always fine.
    pub(super) fn validate_new_overload(
        &mut self,
        existing: &[FnSig],
        sig: &FnSig,
        name: &str,
        span: Span,
        kind: &str,
    ) {
        if existing.is_empty() {
            return;
        }
        if !sig.type_params.is_empty() || existing.iter().any(|e| !e.type_params.is_empty()) {
            self.err_coded(
                span,
                format!("generic {kind} `{name}` cannot be overloaded"),
                "E-OVERLOAD-GENERIC",
                Some("a generic declaration must be the only one with its name; remove the type parameters or rename".into()),
            );
            return;
        }
        let want_ret = &existing[0].ret;
        if &sig.ret != want_ret {
            self.err_coded(
                span,
                format!(
                    "overloaded {kind} `{name}` must return the same type as its other overloads (`{want_ret}`), found `{}`",
                    sig.ret
                ),
                "E-OVERLOAD-RETURN",
                Some("overloads model one operation over different argument types; differing returns suggest separate functions or generics".into()),
            );
        }
        if existing.iter().any(|e| e.params == sig.params) {
            self.err_coded(
                span,
                format!("overloaded {kind} `{name}` has two declarations with identical parameter types"),
                "E-OVERLOAD-DUPLICATE",
                Some("each overload must differ in its parameter types".into()),
            );
        }
    }

    /// Validate a function's declared generic parameters: reject duplicates (`E-GENERIC-PARAM`) and
    /// names that shadow a built-in type (`int`, `List`, …), which would be silently ineffective
    /// because `resolve_type` matches the built-in first (M-RT S7).
    pub(super) fn validate_type_params(&mut self, type_params: &[String], span: Span) {
        let mut seen: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
        for tp in type_params {
            if is_builtin_type_name(tp) {
                self.err_coded(
                    span,
                    format!("type parameter `{tp}` shadows a built-in type"),
                    "E-GENERIC-PARAM",
                    Some("pick a distinct name, e.g. `T`, `U`, `Elem`".into()),
                );
            } else if !seen.insert(tp.as_str()) {
                self.err_coded(
                    span,
                    format!("duplicate type parameter `{tp}`"),
                    "E-GENERIC-PARAM",
                    None,
                );
            }
        }
    }

    pub(super) fn collect_enum(&mut self, e: &crate::ast::EnumDecl) {
        if is_builtin_type_name(&e.name) {
            self.err(
                e.span,
                format!("cannot redefine built-in type `{}`", e.name),
            );
            return;
        }
        if self.enums.contains_key(&e.name) || self.classes.contains_key(&e.name) {
            self.err(e.span, format!("type `{}` is already defined", e.name));
            return;
        }
        // Register the name + type parameters first so variant field types can reference the enum
        // itself (including a self-referential `Tree<T>` payload) with correct arity (M-RT generic
        // enums).
        self.validate_type_params(&e.type_params, e.span);
        self.enums.insert(
            e.name.clone(),
            EnumInfo {
                variants: HashMap::new(),
                type_params: e.type_params.clone(),
            },
        );
        // The enum's type parameters are in scope while resolving every variant field type, so a bare
        // `T` resolves to `Ty::Param("T")` (M-RT generic enums); cleared after, like a generic class.
        self.active_type_params = e.type_params.clone();
        let mut variants = HashMap::new();
        for v in &e.variants {
            let fields = v.fields.iter().map(|p| self.resolve_type(&p.ty)).collect();
            variants.insert(v.name.clone(), fields);
        }
        self.active_type_params.clear();
        self.enums.get_mut(&e.name).unwrap().variants = variants;
    }

    pub(super) fn collect_class(&mut self, c: &crate::ast::ClassDecl) {
        use crate::ast::ClassMember;
        if is_builtin_type_name(&c.name) {
            self.err(
                c.span,
                format!("cannot redefine built-in type `{}`", c.name),
            );
            return;
        }
        if self.classes.contains_key(&c.name) || self.enums.contains_key(&c.name) {
            self.err(c.span, format!("type `{}` is already defined", c.name));
            return;
        }
        // Register the name + type parameters first so members can reference the class type itself
        // (including a self-referential `Box<T> next` field) with correct arity (M-RT generics-all).
        self.validate_type_params(&c.type_params, c.span);
        self.classes.insert(
            c.name.clone(),
            ClassInfo {
                fields: HashMap::new(),
                mutable_fields: std::collections::HashSet::new(),
                statics: HashMap::new(),
                consts: HashMap::new(),
                static_mut: std::collections::HashSet::new(),
                methods: HashMap::new(),
                hooks: HashMap::new(),
                ctor: Vec::new(),
                has_ctor: false,
                ctor_vis: MemberVis::Public,
                ctor_owner: c.name.clone(),
                type_params: c.type_params.clone(),
                is_abstract: c.is_abstract,
                field_vis: HashMap::new(),
                method_vis: HashMap::new(),
            },
        );
        use crate::ast::Modifier;
        // Batch G (finding #7): reject an explicit instance field declared twice (previously the last
        // silently won). An explicit field that *also* names a promoted ctor param is intentionally
        // allowed — the explicit declaration is authoritative (`explicit_field_decl_wins_over_promotion`);
        // a duplicate *promoted* param is caught by `E-DUP-PARAM` on the constructor.
        {
            let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
            for m in &c.members {
                if let ClassMember::Field {
                    modifiers,
                    name,
                    span,
                    ..
                } = m
                {
                    if modifiers.contains(&Modifier::Static) || modifiers.contains(&Modifier::Const)
                    {
                        continue;
                    }
                    if !seen.insert(name.as_str()) {
                        self.err_coded(
                            *span,
                            format!("duplicate field `{name}`"),
                            "E-DUP-FIELD",
                            Some("each field must have a distinct name".into()),
                        );
                    }
                }
            }
        }
        let mut fields = HashMap::new();
        // Member visibility (Wave 1.1): instance-field and method name → (vis, owner==this class).
        // Inherited entries (with their original owner) are merged in by `merge_inherited`.
        let mut field_vis: HashMap<String, (MemberVis, String)> = HashMap::new();
        let mut method_vis: HashMap<String, (MemberVis, String)> = HashMap::new();
        let mut mutable_fields = std::collections::HashSet::new();
        let mut statics: HashMap<String, Ty> = HashMap::new();
        let mut consts: HashMap<String, ConstEntry> = HashMap::new();
        let mut static_mut = std::collections::HashSet::new();
        let mut methods: HashMap<String, Vec<FnSig>> = HashMap::new();
        let mut hooks: HashMap<String, HookInfo> = HashMap::new();
        let mut ctor = Vec::new();
        let mut ctor_vis = MemberVis::Public;
        // The class's type parameters are in scope while resolving every member signature (fields,
        // constructor, methods), so a bare `T` resolves to `Ty::Param("T")` (M-RT generics-all). A
        // generic method adds its own parameters on top.
        let class_tp = &c.type_params;
        // Promoted ctor params (carrying a visibility modifier) also become fields,
        // matching the evaluator's runtime promotion (EV-4). Deferred to after the
        // member loop via `or_insert` so an explicit `Field` decl of the same name
        // stays authoritative regardless of member order.
        let mut promoted: Vec<(String, Ty, MemberVis)> = Vec::new();
        for m in &c.members {
            match m {
                ClassMember::Field {
                    ty,
                    name,
                    modifiers,
                    init,
                    span,
                } => {
                    self.active_type_params = class_tp.clone();
                    let fty = self.resolve_type(ty);
                    self.active_type_params.clear();
                    if modifiers.contains(&Modifier::Const) {
                        // A `const` class constant (Feature A): compile-time, immutable, class-level,
                        // accessed only `ClassName.NAME`. It needs a literal-const initializer and must
                        // not be `mutable`. Disjoint from instance fields and statics.
                        if modifiers.contains(&Modifier::Mutable) {
                            self.err_coded(
                                *span,
                                format!("`const {name}` cannot be `mutable` — a constant is immutable"),
                                "E-CONST-MUTABLE",
                                Some("drop `mutable`, or use a `static mutable` field for class-level state".into()),
                            );
                        }
                        match init {
                            None => {
                                self.err_coded(
                                    *span,
                                    format!("`const {name}` needs an initializer"),
                                    "E-CONST-NO-INIT",
                                    Some("e.g. `const int MAX = 100;`".into()),
                                );
                            }
                            Some(e) => {
                                if crate::value::const_literal(e).is_none() {
                                    self.err_coded(
                                        Self::expr_span(e),
                                        format!(
                                            "`const {name}` initializer must be a literal constant"
                                        ),
                                        "E-CONST-NOT-LITERAL",
                                        Some("use an int/float/bool/string/null literal".into()),
                                    );
                                } else {
                                    let ity = self.check_expr(e);
                                    if !self.ty_assignable(&ity, &fty) {
                                        self.err_coded(
                                            Self::expr_span(e),
                                            format!(
                                                "`const {name}: {fty}` initialized with `{ity}`"
                                            ),
                                            "E-CONST-INIT-TYPE",
                                            None,
                                        );
                                    }
                                }
                            }
                        }
                        consts.insert(
                            name.clone(),
                            ConstEntry {
                                ty: fty,
                                vis: MemberVis::of(modifiers),
                                owner: c.name.clone(),
                            },
                        );
                    } else if modifiers.contains(&Modifier::Static) {
                        // A `static` field is class-level state (M-mut.7): it needs an initializer (no
                        // constructor sets it) and is NOT an instance field. Feature B-static lifts the
                        // old literal-only restriction — the initializer may be ANY expression, evaluated
                        // once at program start in declaration order. Its TYPE is checked later
                        // (`check_static_inits`, pass 2) where every function + static is collected, so
                        // an initializer may call a function or read another (earlier) static.
                        if init.is_none() {
                            self.err_coded(
                                *span,
                                format!("static field `{name}` needs an initializer"),
                                "E-STATIC-NO-INIT",
                                Some("e.g. `static mutable int total = 0;`".into()),
                            );
                        }
                        statics.insert(name.clone(), fty);
                        if modifiers.contains(&Modifier::Mutable) {
                            static_mut.insert(name.clone());
                        }
                    } else {
                        // A plain instance field. An optional expression initializer (Feature B) is
                        // evaluated per-instance at construction (declaration order, after promotion);
                        // its type + forward-reference are checked in `check_type_body`, where `this`
                        // and the field scope are live. Just record the field here.
                        fields.insert(name.clone(), fty);
                        field_vis.insert(name.clone(), (MemberVis::of(modifiers), c.name.clone()));
                        if modifiers.contains(&Modifier::Mutable) {
                            mutable_fields.insert(name.clone());
                        }
                    }
                }
                ClassMember::Constructor {
                    modifiers,
                    params,
                    span,
                    ..
                } => {
                    // The constructor's own visibility (Batch A). A non-visibility modifier
                    // (`abstract`/`static`/`const`/`open`/`mutable`) on a constructor is meaningless —
                    // reject it rather than silently dropping it (closes the §5 dropped-modifier gaps).
                    ctor_vis = MemberVis::of(modifiers);
                    if modifiers.iter().any(|m| {
                        !matches!(
                            m,
                            Modifier::Public | Modifier::Private | Modifier::Protected
                        )
                    }) {
                        self.err_coded(
                            *span,
                            "a constructor takes only a visibility modifier (`private`/`protected`/`public`)".to_string(),
                            "E-CTOR-MODIFIER",
                            Some("remove `abstract`/`static`/`const`/`open`/`mutable` from the constructor".into()),
                        );
                    }
                    self.reject_dup_param_names(params.iter().map(|p| (p.name.as_str(), p.span)));
                    // Resolve each param type once; reuse for both the ctor signature
                    // and field promotion to avoid duplicate "unknown type" errors.
                    self.active_type_params = class_tp.clone();
                    ctor = params
                        .iter()
                        .map(|p| {
                            let ty = self.resolve_type(&p.ty);
                            if p.modifiers.iter().any(|m| {
                                matches!(
                                    m,
                                    Modifier::Public | Modifier::Private | Modifier::Protected
                                )
                            }) {
                                promoted.push((
                                    p.name.clone(),
                                    ty.clone(),
                                    MemberVis::of(&p.modifiers),
                                ));
                                // A `public mutable int x` promoted param yields a mutable field.
                                if p.modifiers.contains(&Modifier::Mutable) {
                                    mutable_fields.insert(p.name.clone());
                                }
                            }
                            ty
                        })
                        .collect();
                    self.active_type_params.clear();
                }
                ClassMember::Method(f) => {
                    // A method reuses the free-fn machinery (M-RT generics-all): with the class's
                    // type parameters AND the method's own in scope, a bare `T`/`U` resolves to
                    // `Ty::Param`; class params are substituted with the instance's type arguments at
                    // the call site, method params unified from the call's arguments. A method param
                    // that shadows a class param is rejected so composition stays unambiguous. Erased
                    // before any backend by `erase_generics`.
                    self.reject_dup_param_names(f.params.iter().map(|p| (p.name.as_str(), p.span)));
                    self.validate_type_params(&f.type_params, f.span);
                    for tp in &f.type_params {
                        if class_tp.iter().any(|c| c == tp) {
                            self.err_coded(
                                f.span,
                                format!(
                                    "method type parameter `{tp}` shadows the class type parameter `{tp}`"
                                ),
                                "E-GENERIC-PARAM",
                                Some("rename the method's type parameter".into()),
                            );
                        }
                    }
                    let mut active = class_tp.clone();
                    active.extend(f.type_params.iter().cloned());
                    self.active_type_params = active;
                    let p = f.params.iter().map(|p| self.resolve_type(&p.ty)).collect();
                    let ret = match &f.ret {
                        Some(t) => self.resolve_type(t),
                        None => Ty::Void,
                    };
                    let throws = Self::flatten_throws(
                        f.throws.iter().map(|t| self.resolve_type(t)).collect(),
                    );
                    self.active_type_params.clear();
                    // M4 default parameters are free-function-only in v1; a default on a method param
                    // is rejected (the fill pass resolves free/native calls, not method dispatch).
                    self.reject_member_defaults(&f.params, "method");
                    // M-RT overloading: a same-named method joins an overload set (same rules as free
                    // functions — same return, no identical signatures, no generic member).
                    let sig = FnSig {
                        params: p,
                        defaults: vec![None; f.params.len()],
                        ret,
                        type_params: f.type_params.clone(),
                        throws,
                    };
                    let existing = methods.get(&f.name).cloned().unwrap_or_default();
                    self.validate_new_overload(&existing, &sig, &f.name, f.span, "method");
                    methods.entry(f.name.clone()).or_default().push(sig);
                    // First-declared overload's visibility represents the method name (Wave 1.1).
                    method_vis
                        .entry(f.name.clone())
                        .or_insert((MemberVis::of(&f.modifiers), c.name.clone()));
                }
                // A property hook (M-mut.7b): record its declared type and which accessors it
                // provides. The body is type-checked in phase 2 (`check_program`), with `this` and
                // the field scope live. Class type params are in scope for the hook's type.
                ClassMember::Hook {
                    ty, name, get, set, ..
                } => {
                    self.active_type_params = class_tp.clone();
                    let hty = self.resolve_type(ty);
                    self.active_type_params.clear();
                    if hooks.contains_key(name) {
                        self.err_coded(
                            c.span,
                            format!("property hook `{name}` is declared more than once"),
                            "E-HOOK-DUP",
                            None,
                        );
                    }
                    hooks.insert(
                        name.clone(),
                        HookInfo {
                            ty: hty,
                            has_get: get.is_some(),
                            has_set: set.is_some(),
                        },
                    );
                }
            }
        }
        // Explicit field decls win: only insert a promoted field if not already declared.
        for (name, ty, pvis) in promoted {
            fields.entry(name.clone()).or_insert(ty);
            field_vis.entry(name).or_insert((pvis, c.name.clone()));
        }
        // A property hook is virtual: its name must not also name a stored field, a static, or a
        // method (the read/write path resolves a hook before the field, so a collision would shadow
        // the storage silently). Order-independent — checked after every member is collected.
        for hname in hooks.keys() {
            if fields.contains_key(hname)
                || statics.contains_key(hname)
                || methods.contains_key(hname)
            {
                self.err_coded(
                    c.span,
                    format!("property hook `{hname}` collides with a field, static, or method of the same name"),
                    "E-HOOK-DUP",
                    Some("a hook is virtual — give it a distinct name from any stored member".into()),
                );
            }
        }
        let info = self.classes.get_mut(&c.name).unwrap();
        info.fields = fields;
        info.field_vis = field_vis;
        info.method_vis = method_vis;
        info.mutable_fields = mutable_fields;
        info.statics = statics;
        info.consts = consts;
        info.static_mut = static_mut;
        info.methods = methods;
        info.hooks = hooks;
        info.has_ctor = c
            .members
            .iter()
            .any(|m| matches!(m, ClassMember::Constructor { .. }));
        info.ctor = ctor;
        info.ctor_vis = ctor_vis;
        // `ctor_owner` was initialized to the class's own name; an own ctor keeps it. An inherited
        // ctor's owner/visibility are merged in `merge_inherited` for a class with no own ctor.
    }
}

/// The name, span, and human label of a top-level item that defines a *symbol* in PHP's namespace
/// (free function / class / enum / interface / trait / type-alias) — the positions where a
/// PHP-reserved name (e.g. `var`) would transpile to invalid PHP. `None` for imports.
fn reserved_symbol_decl(item: &crate::ast::Item) -> Option<(&str, Span, &'static str)> {
    use crate::ast::Item;
    match item {
        Item::Function(f) => Some((&f.name, f.span, "function")),
        Item::Class(c) => Some((&c.name, c.span, "class")),
        Item::Enum(e) => Some((&e.name, e.span, "enum")),
        Item::Interface(i) => Some((&i.name, i.span, "interface")),
        Item::Trait(t) => Some((&t.name, t.span, "trait")),
        Item::TypeAlias { name, span, .. } => Some((name, *span, "type alias")),
        Item::Import { .. } => None,
    }
}

/// The type of a *literal constant* expression, or `None` if `e` is not a literal (M4 default
/// parameters: a default value must be a literal, so its type is determined without invoking the full
/// checker). A string literal qualifies only when it has no interpolation holes (all `Literal` parts).
fn literal_ty(e: &crate::ast::Expr) -> Option<Ty> {
    use crate::ast::{Expr, StrPart};
    match e {
        Expr::Int(..) => Some(Ty::Int),
        Expr::Float(..) => Some(Ty::Float),
        Expr::Decimal { .. } => Some(Ty::Decimal),
        Expr::Bool(..) => Some(Ty::Bool),
        Expr::Bytes(..) => Some(Ty::Bytes),
        Expr::Null(_) => Some(Ty::Null),
        Expr::Str(parts, _) if parts.iter().all(|p| matches!(p, StrPart::Literal(_))) => {
            Some(Ty::String)
        }
        _ => None,
    }
}
