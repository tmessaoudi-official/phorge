//! Tree-walking interpreter — construct (M-Decomp W4). See mod.rs for the struct + core + entry points.

use super::*;

impl<'c> Interp<'c> {
    /// The ordered constructor plan `class_name` runs (M-RT S6c.2) — mirrors `ast::ctor_plan`: its own
    /// ctor if declared, else (single inheritance) the parent's plan, else (multiple inheritance) every
    /// parent's plan concatenated in `extends` order. Empty when no own/inherited ctor applies. Each
    /// `(params, body)` is cloned so `construct` can run it with the instance live.
    pub(super) fn ctor_plan(&self, class_name: &str) -> Vec<(Vec<CtorParam>, Vec<Stmt>)> {
        let Some(class) = self.classes.get(class_name) else {
            return Vec::new();
        };
        if let Some(parts) = class.members.iter().find_map(|m| match m {
            ClassMember::Constructor { params, body, .. } => Some((params.clone(), body.clone())),
            _ => None,
        }) {
            return vec![parts];
        }
        // M-RT S8 (T3): a `use`d trait's constructor becomes the class's ctor, winning over a parent
        // ctor (PHP P2). Trait members live in `self.classes` under the trait name (synthetic decl).
        // Mirrors `ast::ctor_plan`; the checker rejects two unresolved trait ctors.
        if let Some(tc) = class.uses.iter().find_map(|u| {
            self.classes.get(&u.name).and_then(|t| {
                t.members.iter().find_map(|m| match m {
                    ClassMember::Constructor { params, body, .. } => {
                        Some((params.clone(), body.clone()))
                    }
                    _ => None,
                })
            })
        }) {
            return vec![tc];
        }
        let parents = class.extends.clone();
        match parents.len() {
            0 => Vec::new(),
            1 => self.ctor_plan(&parents[0]),
            _ => parents.iter().flat_map(|p| self.ctor_plan(p)).collect(),
        }
    }

    /// Construct a class instance. Applies constructor *promotion* at runtime
    /// (EV-4): each promoted ctor param (carrying a visibility modifier) becomes a
    /// field. Required for the §6 empty-body constructor to populate `name`.
    pub(super) fn construct(&mut self, class_name: &str, args: Vec<Value>) -> R<Value> {
        debug_assert!(
            self.classes.contains_key(class_name),
            "caller checked the class exists"
        );
        // M-RT S6c.2: a no-own-ctor class runs its inherited constructor *plan* — for single
        // inheritance the parent's ctor, for multiple inheritance every parent's in `extends` order.
        // The full args are the plan entries' params concatenated; each entry takes its slice.
        let plan = self.ctor_plan(class_name);
        // M-perf S1b: allocate with the class's shared slot layout (same source as the VM). A class
        // with no storage fields gets an empty layout (checker-unreachable miss → empty, EV-7).
        let layout = self
            .layouts
            .get(class_name)
            .cloned()
            .unwrap_or_else(|| crate::value::ClassLayout::new(vec![]));
        let inst = Instance::new(class_name.to_string(), layout);
        let total: usize = plan.iter().map(|(p, _)| p.len()).sum();
        if plan.is_empty() {
            if !args.is_empty() {
                return rt(format!("`{class_name}` has no constructor but got args"));
            }
            // A no-constructor class still runs its expression field initializers (Feature B).
            let rc = Rc::new(inst);
            self.run_field_inits(&rc)?;
            return Ok(Value::Instance(rc));
        }
        if args.len() != total {
            return rt(format!(
                "constructor of `{class_name}` expects {total} args, got {}",
                args.len()
            ));
        }
        let promoted = |p: &CtorParam| {
            p.modifiers.iter().any(|m| {
                matches!(
                    m,
                    Modifier::Public | Modifier::Private | Modifier::Protected
                )
            })
        };
        // Promote every promoted param across the whole plan first (before any body runs), matching the
        // VM's `MakeInstance` populating all fields up front — so a parent body can read a field a later
        // parent promotes, identically on both backends. We still solely own `inst`, so `get_mut` skips
        // the runtime borrow.
        let mut offset = 0;
        for (params, _) in &plan {
            for (i, p) in params.iter().enumerate() {
                if promoted(p) {
                    inst.set_field(&p.name, args[offset + i].clone());
                }
            }
            offset += params.len();
        }
        // Share one `Rc` between the `this` receiver and the returned instance (M2 P5a). Then run each
        // plan entry's body in order with its param slice + `this` in scope. Bodies cannot change the
        // result (a ctor body's return is discarded); their side effect is field initialization.
        let rc = Rc::new(inst);
        // Feature B: evaluate expression field initializers per-instance, in declaration order, after
        // promotion and BEFORE any constructor body — so an initializer reads `this` (promoted params
        // + an earlier-initialized sibling) and the ctor body may still override the result.
        self.run_field_inits(&rc)?;
        let ctor = format!("{}::new", rc.class);
        let mut offset = 0;
        for (params, body) in &plan {
            let names: Vec<String> = params.iter().map(|p| p.name.clone()).collect();
            let slice = args[offset..offset + params.len()].to_vec();
            offset += params.len();
            self.run_call(
                &ctor,
                &names,
                body,
                slice,
                Some(Value::Instance(rc.clone())),
                Some(rc.class.as_str()),
            )?;
        }
        Ok(Value::Instance(rc))
    }

    /// Evaluate `rc`'s expression field initializers (Feature B) in declaration order, each with `this`
    /// bound to `rc`, setting the field as it goes — so a later initializer reads an earlier one via
    /// `this`. Shared by the constructor and no-constructor construction paths. The ordered list comes
    /// from the shared `ast::field_initializers` (base-first across ancestors), so every backend sets
    /// the same fields to the same values.
    pub(super) fn run_field_inits(&mut self, rc: &Rc<Instance>) -> R<()> {
        if let Some(inits) = self.field_inits.get(&rc.class).cloned() {
            for (fname, init) in &inits {
                let v = self.run_hook_get(Value::Instance(rc.clone()), init)?;
                rc.set_field(fname, v);
            }
        }
        Ok(())
    }

    /// The cloned `get` expression of a property hook `class.name`, if the class declares one with a
    /// `get` (M-mut.7b). `None` if there is no such hook or it is write-only.
    pub(super) fn hook_get(&self, class: &str, name: &str) -> Option<Expr> {
        let decl = self.classes.get(class)?;
        let own = decl.members.iter().find_map(|m| match m {
            ClassMember::Hook {
                name: n,
                get: Some(g),
                ..
            } if n == name => Some(g.clone()),
            _ => None,
        });
        // M-RT S8 (T4): fall back to a `use`d trait's hook (trait members live under the trait name).
        own.or_else(|| decl.uses.iter().find_map(|u| self.hook_get(&u.name, name)))
    }

    /// The cloned `set` parameter + block of a property hook `class.name`, if the class declares one
    /// with a `set` (M-mut.7b). `None` if there is no such hook or it is read-only.
    pub(super) fn hook_set(
        &self,
        class: &str,
        name: &str,
    ) -> Option<(crate::ast::Param, Vec<Stmt>)> {
        let decl = self.classes.get(class)?;
        let own = decl.members.iter().find_map(|m| match m {
            ClassMember::Hook {
                name: n,
                set: Some(s),
                ..
            } if n == name => Some(s.clone()),
            _ => None,
        });
        // M-RT S8 (T4): fall back to a `use`d trait's hook.
        own.or_else(|| decl.uses.iter().find_map(|u| self.hook_set(&u.name, name)))
    }

    /// Evaluate a property hook's `get` expression with `this` bound to the receiver, in a fresh
    /// frame (M-mut.7b) — the value-returning analogue of `run_call`, but for an expression body.
    pub(super) fn run_hook_get(&mut self, recv: Value, get: &Expr) -> R<Value> {
        if self.depth >= crate::limits::MAX_CALL_DEPTH {
            return rt("stack overflow");
        }
        self.depth += 1;
        let saved_frame = std::mem::replace(&mut self.frame, CallScopes::new());
        let saved_this = self.this.replace(recv);
        let result = self.eval(get);
        self.frame = saved_frame;
        self.this = saved_this;
        self.depth -= 1;
        result
    }
}
