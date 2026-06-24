//! Bytecode compiler — program (M-Decomp W4.1). See compiler/mod.rs for the struct,
//! emission/scope core, and the (kept-whole) `stack_effect`.

use super::*;

pub(super) fn compile_program(program: &Program) -> Result<BytecodeProgram, String> {
    let mut order: Vec<&FunctionDecl> = Vec::new();
    let mut fns: HashMap<String, FnMeta> = HashMap::new();
    // M-RT overloading: name → every function index declared under it (declaration order). Names with
    // more than one entry become overload sets in the post-pass below.
    let mut overload_order: HashMap<String, Vec<usize>> = HashMap::new();
    // Enum pre-pass: one `EnumDesc` per variant of every declared enum, plus the variant-name →
    // metadata map both construction and `match` resolve through (decision P4-2).
    let mut enum_descs: Vec<EnumDesc> = Vec::new();
    let mut variants: HashMap<String, VariantMeta> = HashMap::new();
    // M-RT S8: each trait becomes a synthetic method-bearing decl so its methods are registered and
    // compiled under the trait name; `class_method_origins` then aliases every using class's trait
    // method to the trait's fn index. A trait is never instantiated (no `MakeInstance`), so the extra
    // class descriptor is inert. Owned here (declared before `class_decls`) so the borrow lasts.
    let trait_synths: Vec<ClassDecl> = program
        .items
        .iter()
        .filter_map(|it| match it {
            Item::Trait(t) => Some(ClassDecl {
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
            }),
            _ => None,
        })
        .collect();
    let mut class_decls: Vec<&ClassDecl> = Vec::new();
    for it in &program.items {
        match it {
            Item::Function(f) => {
                let index = order.len();
                // M-RT overloading: keep the FIRST overload's `FnMeta` (return type is shared across
                // the set, so it still types a call's result); record every overload's function index
                // so the post-pass can build the dispatch table for names with more than one.
                fns.entry(f.name.clone()).or_insert_with(|| FnMeta {
                    index,
                    ret: f.ret.as_ref().map_or(CTy::Other, resolve_cty),
                    params: f.params.iter().map(|p| resolve_cty(&p.ty)).collect(),
                    overload: None,
                });
                overload_order
                    .entry(f.name.clone())
                    .or_default()
                    .push(index);
                order.push(f);
            }
            Item::Enum(e) => {
                for v in &e.variants {
                    variants.insert(
                        v.name.clone(),
                        VariantMeta {
                            index: enum_descs.len(),
                            field_tags: v.fields.iter().map(|p| resolve_cty(&p.ty)).collect(),
                        },
                    );
                    enum_descs.push(EnumDesc {
                        ty: e.name.clone(),
                        variant: v.name.clone(),
                        arity: v.fields.len(),
                    });
                }
            }
            Item::Class(c) => class_decls.push(c),
            // Interfaces emit no bytecode; they feed only the `class_implements` table built below.
            Item::Interface(_) => {}
            // M-RT S8: traits are handled via `trait_synths` (appended to `class_decls` below).
            Item::Trait(_) => {}
            Item::Import { .. } => {}
            // Aliases are expanded out of the AST before compiling (checker::expand_aliases); this
            // arm only satisfies the exhaustive match.
            Item::TypeAlias { .. } => {}
        }
    }
    // M-RT S8: register the synthetic trait decls alongside real classes for method compilation.
    for s in &trait_synths {
        class_decls.push(s);
    }
    let main = fns
        .get("main")
        .map(|m| m.index)
        .ok_or_else(|| "no `main` function".to_string())?;

    // M-RT overloading post-pass: for every name with more than one declaration, build a dispatch
    // table (each overload's runtime `ParamKind`s + its function index) and stamp the name's `FnMeta`
    // with the set id, so `compile_call` emits `Op::CallOverload` instead of a direct `Op::Call`.
    let mut overloads: Vec<crate::dispatch::OverloadSet> = Vec::new();
    for (name, indices) in &overload_order {
        if indices.len() < 2 {
            continue;
        }
        let set: crate::dispatch::OverloadSet = indices
            .iter()
            .map(|&fi| {
                let kinds = order[fi]
                    .params
                    .iter()
                    .map(|p| crate::dispatch::param_kind(&p.ty))
                    .collect();
                (kinds, fi)
            })
            .collect();
        let set_id = overloads.len();
        overloads.push(set);
        if let Some(meta) = fns.get_mut(name) {
            meta.overload = Some(set_id);
        }
    }

    // Class pre-pass (decision P4-2/P4-4/P4-6). Function indices are laid out as
    // `[free fns | constructors | methods]` so free-function indices — and `main` — stay put.
    // `class_descs` lists the promoted fields a `MakeInstance` populates (mirroring the
    // interpreter's runtime promotion); the `names` pool interns every readable field name AND
    // every method name (so `obj.field`/`obj.m()` lower to a name-pool index); `class_field_tags`
    // records each class's field types for bare-field (`this.field`) resolution; `methods` is the
    // `(class, method) → fn index` dispatch table `Op::CallMethod` reads at runtime. Explicit
    // `Field` members are named but absent from `class_descs.fields` — like the interpreter they
    // are unpopulated, so reading one faults.
    let nfree = order.len();
    let nclasses = class_decls.len();
    let mut classes: HashMap<String, usize> = HashMap::new();
    let mut class_descs: Vec<ClassDesc> = Vec::new();
    // Program-wide field-type table, keyed by class *name* so `ctype` can resolve a field read on
    // any instance (not just `this`) — `obj.field` looks up `class_field_ctys[class_of(obj)][field]`.
    let mut class_field_ctys: HashMap<String, HashMap<String, CTy>> = HashMap::new();
    let mut names: Vec<String> = Vec::new();
    let mut names_index: HashMap<String, usize> = HashMap::new();
    let mut intern = |name: &str, names: &mut Vec<String>| {
        if !names_index.contains_key(name) {
            names_index.insert(name.to_string(), names.len());
            names.push(name.to_string());
        }
    };
    for (ci, c) in class_decls.iter().enumerate() {
        classes.insert(c.name.clone(), nfree + ci);
        // M-RT S6c.2: a no-own-ctor class's instance descriptor uses its inherited constructor(s)'
        // promoted fields — for single inheritance the parent's, for multiple inheritance every
        // parent's concatenated — so a `MakeInstance` populates exactly the fields the interpreter
        // promotes. `ctor_plan` is the shared decision; flatten its entries' params.
        let plan = crate::ast::ctor_plan(program, &c.name);
        let params: Vec<CtorParam> = plan.iter().flat_map(|(p, _)| p.iter().cloned()).collect();
        let params: &[CtorParam] = &params;
        let mut fields: Vec<String> = Vec::new();
        let mut tags: HashMap<String, CTy> = HashMap::new();
        for p in params {
            if is_promoted(p) {
                fields.push(p.name.clone());
                intern(&p.name, &mut names);
                tags.insert(p.name.clone(), resolve_cty(&p.ty));
            }
        }
        for m in &c.members {
            match m {
                ClassMember::Field {
                    name,
                    ty,
                    modifiers,
                    ..
                } => {
                    // A `static` field is class-level state (addressed by `Op::Get/SetStatic` via a
                    // static index, M-mut.7), not an instance field — it gets no name-pool entry and
                    // no instance field-tag.
                    if !modifiers.contains(&Modifier::Static) {
                        intern(name, &mut names); // readable, but unpopulated by construction
                        tags.insert(name.clone(), resolve_cty(ty));
                    }
                }
                ClassMember::Method(f) => intern(&f.name, &mut names),
                ClassMember::Constructor { .. } => {}
                // A property hook (M-mut.7b) is virtual — no instance field, no field-tag. Its
                // accessors lower to synthetic methods `<name>$get`/`$set` dispatched via
                // `Op::CallMethod`, so the method names need name-pool entries (`$` can't appear in a
                // user identifier ⇒ never collides). The methods themselves are built below.
                ClassMember::Hook { name, get, set, .. } => {
                    if get.is_some() {
                        intern(&format!("{name}$get"), &mut names);
                    }
                    if set.is_some() {
                        intern(&format!("{name}$set"), &mut names);
                    }
                }
            }
        }
        // M-RT S6b: a `rename P.m as n` resolution exposes a method under a fresh name `n` that is no
        // class member, so it needs its own name-pool entry for `obj.n()` to lower to `Op::CallMethod`.
        for r in &c.resolutions {
            if let crate::ast::Resolution::Rename { as_name, .. } = r {
                intern(as_name, &mut names);
            }
        }
        class_descs.push(ClassDesc {
            class: c.name.clone(),
            fields,
        });
        class_field_ctys.insert(c.name.clone(), tags);
    }
    // `intern`'s unique borrow of `names_index` ends at its last call above (NLL), so the
    // immutable `&names_index` borrows below are free.

    // Static fields (M-mut.7): assign each `static` field a program-wide slot index (declaration
    // order across classes) and const-fold its literal initializer into the program's `static_inits`
    // table. The VM seeds its runtime `statics` vector from this once at startup; the interpreter
    // seeds its own map from the same `const_literal` kernel (F3). The checker guarantees every
    // static has a literal-const initializer, so `unwrap_or(Unit)` is checker-unreachable.
    // Class constants (Feature A): the shared table flattens inheritance + traits. Each `(class, NAME)`
    // carries its inlined literal `Value` and operand `CTy` (from the declared `Type`), consumed by
    // `const_value`/`const_cty`. No runtime slot — a const access emits `Op::Const`.
    let consts_index: HashMap<(String, String), (Value, CTy)> = crate::ast::class_consts(program)
        .into_iter()
        .map(|(key, (v, ty))| (key, (v, resolve_cty(&ty))))
        .collect();
    let mut statics_index: HashMap<(String, String), (usize, CTy)> = HashMap::new();
    let mut static_inits: Vec<Value> = Vec::new();
    // Feature B-static: a static whose initializer is NOT a compile-time literal gets a `Unit`
    // placeholder in `static_inits` and a `(slot, init expr)` entry here; a `SetStatic` prelude is
    // emitted at the start of `main` (declaration order) to evaluate it once before any user code —
    // matching the interpreter's `eval_static_inits` and the transpiler's `__phorge_init_statics`.
    let mut static_runtime_inits: Vec<(usize, &Expr)> = Vec::new();
    for c in &class_decls {
        for m in &c.members {
            if let ClassMember::Field {
                modifiers,
                name,
                ty,
                init,
                ..
            } = m
            {
                if modifiers.contains(&Modifier::Static) {
                    let slot = static_inits.len();
                    statics_index.insert((c.name.clone(), name.clone()), (slot, resolve_cty(ty)));
                    match init.as_ref().and_then(crate::value::const_literal) {
                        Some(v) => static_inits.push(v),
                        None => {
                            static_inits.push(Value::Unit); // placeholder, set by the main prelude
                            if let Some(e) = init {
                                static_runtime_inits.push((slot, e));
                            }
                        }
                    }
                }
            }
        }
    }
    // M-RT S8: a `use`d trait's `static` field becomes a PER-USING-CLASS copy (PHP `use` semantics) —
    // each using class gets its own slot keyed `(class, field)`. The trait synthetic's own
    // `(trait, field)` entry from the loop above is inert (a trait is never a static holder at runtime).
    for it in &program.items {
        let Item::Class(c) = it else { continue };
        for u in &c.uses {
            for t in &program.items {
                let Item::Trait(td) = t else { continue };
                if td.name != u.name {
                    continue;
                }
                for m in &td.members {
                    if let ClassMember::Field {
                        modifiers,
                        name,
                        ty,
                        init,
                        ..
                    } = m
                    {
                        if modifiers.contains(&Modifier::Static) {
                            statics_index.insert(
                                (c.name.clone(), name.clone()),
                                (static_inits.len(), resolve_cty(ty)),
                            );
                            let v = init
                                .as_ref()
                                .and_then(crate::value::const_literal)
                                .unwrap_or(Value::Unit);
                            static_inits.push(v);
                        }
                    }
                }
            }
        }
    }

    // Methods follow the constructors in the index space; build the dispatch table — and the
    // `(class, method) → return type` table `ctype` reads for a method-call result — in lockstep.
    // Synthetic methods for property hooks (M-mut.7b). Each hook's `get` becomes a 0-arg method
    // `<name>$get` whose body returns the get expression; each `set` a 1-arg method `<name>$set`
    // whose body is the set block. Owned here so `methods_to_compile` can borrow them alongside the
    // real `ClassMember::Method`s. `$` is illegal in a Phorge identifier, so these never collide.
    let mut hook_methods: Vec<(usize, FunctionDecl)> = Vec::new();
    for (ci, c) in class_decls.iter().enumerate() {
        // Own hooks plus (M-RT S8 T4) each `use`d trait's hooks, all registered under THIS class's `ci`
        // so `c.hookName` dispatches to `(this class, name$get/$set)`. A hook body accesses fields by
        // name, so a trait hook body is class-agnostic and safe to register under the using class.
        let from_traits = c
            .uses
            .iter()
            .filter_map(|u| class_decls.iter().find(|d| d.name == u.name))
            .flat_map(|t| t.members.iter());
        for m in c.members.iter().chain(from_traits) {
            if let ClassMember::Hook {
                ty,
                name,
                get,
                set,
                span,
            } = m
            {
                if let Some(g) = get {
                    hook_methods.push((
                        ci,
                        FunctionDecl {
                            modifiers: Vec::new(),
                            vis: Visibility::Public,
                            name: format!("{name}$get"),
                            type_params: Vec::new(),
                            params: Vec::new(),
                            ret: Some(ty.clone()),
                            throws: Vec::new(),
                            body: vec![Stmt::Return {
                                value: Some(g.clone()),
                                span: *span,
                            }],
                            span: *span,
                        },
                    ));
                }
                if let Some((p, body)) = set {
                    hook_methods.push((
                        ci,
                        FunctionDecl {
                            modifiers: Vec::new(),
                            vis: Visibility::Public,
                            name: format!("{name}$set"),
                            type_params: Vec::new(),
                            params: vec![p.clone()],
                            ret: None,
                            throws: Vec::new(),
                            body: body.clone(),
                            span: *span,
                        },
                    ));
                }
            }
        }
    }

    let mut methods: HashMap<(String, String), usize> = HashMap::new();
    let mut method_rets: HashMap<(String, String), CTy> = HashMap::new();
    let mut methods_to_compile: Vec<(usize, &FunctionDecl)> = Vec::new();
    let mut next_idx = nfree + nclasses;
    // M-RT overloading: per (class, method), every overload's `(ParamKinds, fn index)` in declaration
    // order. Pairs with more than one become method overload sets (built after the loop).
    let mut method_order: HashMap<(String, String), crate::dispatch::OverloadSet> = HashMap::new();
    for (ci, c) in class_decls.iter().enumerate() {
        for m in &c.members {
            if let ClassMember::Method(f) = m {
                methods.insert((c.name.clone(), f.name.clone()), next_idx);
                method_rets.insert(
                    (c.name.clone(), f.name.clone()),
                    f.ret.as_ref().map_or(CTy::Other, resolve_cty),
                );
                let kinds = f
                    .params
                    .iter()
                    .map(|p| crate::dispatch::param_kind(&p.ty))
                    .collect();
                method_order
                    .entry((c.name.clone(), f.name.clone()))
                    .or_default()
                    .push((kinds, next_idx));
                methods_to_compile.push((ci, f));
                next_idx += 1;
            }
        }
    }
    // Methods of one name on one class become a dispatch set in the shared `overloads` table; the
    // runtime `CallMethod` consults `method_overloads` (keyed by the receiver's class + method name)
    // and selects exactly like a free-function `CallOverload`. Single-method names stay on the direct
    // `methods` path — byte-identical to pre-overloading output.
    let mut method_overloads: HashMap<(String, String), usize> = HashMap::new();
    for (key, set) in method_order {
        if set.len() > 1 {
            let set_id = overloads.len();
            overloads.push(set);
            method_overloads.insert(key, set_id);
        }
    }
    // Register the synthetic hook methods after the real ones — same dispatch table, same compile
    // path (`compile_method`), so a hook read/write is just a `CallMethod` into `<Class>::<name>$get`
    // / `$set`. The VM needs no new op.
    for (ci, f) in &hook_methods {
        let cname = &class_decls[*ci].name;
        methods.insert((cname.clone(), f.name.clone()), next_idx);
        method_rets.insert(
            (cname.clone(), f.name.clone()),
            f.ret.as_ref().map_or(CTy::Other, resolve_cty),
        );
        methods_to_compile.push((*ci, f));
        next_idx += 1;
    }

    // M-RT S6/S6b: alias inherited / resolution-clause / renamed method-table entries to the body that
    // actually runs. The **shared** `ast::class_method_origins` resolves each `(class, name)` to its
    // `(declaring_class, method)` — already accounting for override, multi-parent composition, diamond
    // auto-merge, and `use`/`rename`/`exclude` clauses — the exact table the interpreter dispatches
    // through, so the two backends can never disagree. No new functions are compiled: an inherited
    // entry is a table alias to the declaring class's already-registered fn index. A class's own method
    // (`origin == self`) is already registered, so it is skipped.
    {
        let (origins, _conflicts) = crate::ast::class_method_origins(program);
        for ((cname, name), (oc, om)) in &origins {
            let key = (cname.clone(), name.clone());
            if methods.contains_key(&key) {
                continue; // own / already-registered entry wins
            }
            let anc_key = (oc.clone(), om.clone());
            if let Some(idx) = methods.get(&anc_key).copied() {
                methods.insert(key.clone(), idx);
                if let Some(rty) = method_rets.get(&anc_key).cloned() {
                    method_rets.insert(key.clone(), rty);
                }
                if let Some(set_id) = method_overloads.get(&anc_key).copied() {
                    method_overloads.insert(key, set_id);
                }
            }
        }
    }

    // Arities for *every* function index — free fns, constructors, then methods (`this` + params)
    // — so `stack_effect` can size an `Op::Call` into a constructor (methods dispatch via
    // `CallMethod`, whose arg count is in the op, so their arity entries are for completeness).
    let mut arities: Vec<usize> = order.iter().map(|f| f.params.len()).collect();
    for c in &class_decls {
        // M-RT S6c.2: the synthetic ctor's arity is the sum of its constructor plan's params (own, or
        // inherited single/multi-parent), matching `compile_constructor`'s param count.
        arities.push(
            crate::ast::ctor_plan(program, &c.name)
                .iter()
                .map(|(p, _)| p.len())
                .sum(),
        );
    }
    for (_, f) in &methods_to_compile {
        arities.push(1 + f.params.len());
    }

    // Free functions have no enclosing class, so no `this` and no field scope.
    let empty_fields: HashMap<String, CTy> = HashMap::new();
    let mut functions = Vec::with_capacity(next_idx);
    // Lambdas live in a trailing block *after* all `next_idx` named functions, so every named
    // function keeps its hoist-order index (`Op::Call` targets and the `main` entry stay valid even
    // when an earlier function defines a lambda). Each function's lambdas are numbered from
    // `next_idx + lambdas.len()` and accumulated here; appended to `functions` once all are compiled.
    let mut lambdas: Vec<Function> = Vec::new();
    for f in &order {
        // This function's lambda sub-functions are numbered starting at `next_idx + lambdas.len()`
        // — the start of its slice of the trailing lambda block.
        let base = next_idx + lambdas.len();
        let mut c = Compiler::new(
            &fns,
            &arities,
            &variants,
            &enum_descs,
            &classes,
            &statics_index,
            &consts_index,
            &class_descs,
            &names_index,
            &empty_fields,
            &class_field_ctys,
            &method_rets,
            base,
        );
        for p in &f.params {
            c.add_local(&p.name, resolve_cty(&p.ty));
        }
        c.height = c.locals.len(); // params occupy slots `0..arity` (decision P3-1)
        let last_line = f.span.line;
        // Feature B-static: evaluate the non-literal static initializers once, at the very start of
        // `main`, in declaration order — `<init>` then `SetStatic(slot)` — before any user code. A
        // later static may read an earlier one (its slot is already set). No `this`/locals are needed
        // (statics are class-level); the compiler context here has none.
        if f.name == "main" {
            for (slot, init) in &static_runtime_inits {
                c.expr(init)?; // [value]
                c.emit(Op::SetStatic(*slot), last_line); // pop into the static slot
            }
        }
        for s in &f.body {
            c.stmt(s)?;
        }
        c.emit_const(Value::Unit, last_line);
        c.emit(Op::Return, last_line);
        functions.push(Function {
            name: f.name.clone(),
            arity: f.params.len(),
            n_captures: 0, // named free functions are never constructed as closures
            chunk: c.chunk,
        });
        // Drain any lambda sub-functions emitted during this body's compilation.
        lambdas.extend(c.extra_functions);
    }
    for (ci, cd) in class_decls.iter().enumerate() {
        let base = next_idx + lambdas.len();
        let (f, extras) = compile_constructor(
            program,
            cd,
            ci,
            &fns,
            &arities,
            &variants,
            &enum_descs,
            &classes,
            &statics_index,
            &consts_index,
            &class_descs,
            &names_index,
            &class_field_ctys[&cd.name],
            &class_field_ctys,
            &method_rets,
            base,
        )?;
        functions.push(f);
        lambdas.extend(extras);
    }
    for (ci, f) in &methods_to_compile {
        let class_name = &class_decls[*ci].name;
        let base = next_idx + lambdas.len();
        let (func, extras) = compile_method(
            class_name,
            f,
            &fns,
            &arities,
            &variants,
            &enum_descs,
            &classes,
            &statics_index,
            &consts_index,
            &class_descs,
            &names_index,
            &class_field_ctys[class_name],
            &class_field_ctys,
            &method_rets,
            base,
        )?;
        functions.push(func);
        lambdas.extend(extras);
    }

    // Append the trailing lambda block. Named functions occupy `0..next_idx` (hoist order); every
    // lambda follows at the index it was numbered with (`next_idx + its position within lambdas`).
    functions.extend(lambdas);

    Ok(BytecodeProgram {
        functions,
        main,
        enum_descs,
        class_descs,
        names,
        methods,
        // The single shared runtime subtype oracle (M-RT S6c.3) — parent classes AND interfaces —
        // same algorithm as the interpreter, so the VM's `Op::IsInstance`/match/overload-dispatch
        // against a class ancestor (not just an interface) is byte-identical.
        class_implements: crate::ast::instanceof_table(program),
        static_inits,
        overloads,
        method_overloads,
    })
}

/// Compile one class's synthetic constructor `<Class>::new` (decision P4-4). Layout: ctor params
/// occupy slots `0..nparams`; the prologue loads the promoted params and `MakeInstance` builds the
/// instance into slot `nparams`; the body runs for side effects with the instance live; the
/// epilogue loads and returns that instance. The body's own `return`s are redirected to the
/// epilogue (never an `Op::Return`), so — exactly like the interpreter — a ctor body cannot change
/// the result: the promoted instance is always returned.
#[allow(clippy::too_many_arguments)]
pub(super) fn compile_constructor<'a>(
    program: &Program,
    c: &ClassDecl,
    desc_idx: usize,
    fns: &'a HashMap<String, FnMeta>,
    arities: &'a [usize],
    variants: &'a HashMap<String, VariantMeta>,
    enum_descs: &'a [EnumDesc],
    classes: &'a HashMap<String, usize>,
    statics_index: &'a HashMap<(String, String), (usize, CTy)>,
    consts_index: &'a HashMap<(String, String), (Value, CTy)>,
    class_descs: &'a [ClassDesc],
    names_index: &'a HashMap<String, usize>,
    field_tags: &'a HashMap<String, CTy>,
    class_field_ctys: &'a HashMap<String, HashMap<String, CTy>>,
    method_rets: &'a HashMap<(String, String), CTy>,
    base_fn_idx: usize,
) -> Result<(Function, Vec<Function>), String> {
    // M-RT S6c.2: a no-own-ctor class compiles its inherited constructor *plan* — for single
    // inheritance the parent's, for multiple inheritance every parent's, in `extends` order. The
    // synthetic function stays `<Class>::new`; `MakeInstance(desc_idx)` builds a *this*-class instance
    // populated with every promoted param across the plan, then each plan entry's body runs in turn.
    let plan = crate::ast::ctor_plan(program, &c.name);
    let all_params: Vec<&CtorParam> = plan.iter().flat_map(|(p, _)| p.iter()).collect();
    let line = c.span.line;
    let mut comp = Compiler::new(
        fns,
        arities,
        variants,
        enum_descs,
        classes,
        statics_index,
        consts_index,
        class_descs,
        names_index,
        field_tags,
        class_field_ctys,
        method_rets,
        base_fn_idx,
    );
    comp.cur_class = Some(c.name.clone()); // `this` resolves to this class (ctype)
    for p in &all_params {
        comp.add_local(&p.name, resolve_cty(&p.ty));
    }
    comp.height = comp.locals.len();
    // Prologue: load every promoted param across the plan, in slot order, then build the instance.
    // `MakeInstance` pops exactly those values (matching `class_descs[desc_idx].fields`, built from the
    // same flattened plan), so the order lines up.
    for (slot, p) in all_params.iter().enumerate() {
        if is_promoted(p) {
            comp.emit(Op::GetLocal(slot), line);
        }
    }
    comp.emit(Op::MakeInstance(desc_idx), line);
    let inst_slot = comp.add_local("$this", CTy::Other);
    comp.this_slot = Some(inst_slot); // a ctor body may reference `this` / bare fields
                                      // Feature B: expression field initializers run after promotion and before the ctor body, in
                                      // declaration order (base-first across ancestors). Each lowers to `this.field = <init>`:
                                      // load the instance, compile the initializer (with `this` live, so it reads an earlier sibling),
                                      // then `SetField`. The shared `ast::field_initializers` list keeps all three backends in lockstep.
    for (fname, init) in crate::ast::field_initializers(program, &c.name) {
        comp.emit(Op::GetLocal(inst_slot), line); // [this]
        comp.expr(&init)?; // [this, value]
        let idx = comp.field_name_index(&fname)?;
        comp.emit(Op::SetField(idx), line); // mutate in place, pop both
    }
    // Each plan entry's body runs in sequence on the one instance. An early `return` ends *that*
    // body only (matching the interpreter's separate per-parent call) — so each gets its own
    // return-jump set, patched to the end of that body, never skipping a later parent's init.
    for (_, body) in &plan {
        comp.ctor_return_jumps = Some(Vec::new());
        for s in body {
            comp.stmt(s)?;
        }
        let jumps = comp.ctor_return_jumps.take().unwrap_or_default();
        for j in jumps {
            comp.patch_jump(j);
        }
    }
    // Epilogue: load and return the constructed instance (a ctor body cannot change the result).
    comp.emit(Op::GetLocal(inst_slot), line);
    comp.emit(Op::Return, line);
    Ok((
        Function {
            name: format!("{}::new", c.name),
            arity: all_params.len(),
            n_captures: 0, // constructors are never closures
            chunk: comp.chunk,
        },
        comp.extra_functions,
    ))
}

/// Compile one instance method as a function (decision P4-6). Layout: slot 0 is the receiver
/// (`this`), slots `1..=nparams` are the params; the body runs with `this` and the class's field
/// scope live; an implicit `Unit` return terminates it (P3-7). The frame is opened by
/// `Op::CallMethod`, which places the receiver at slot 0.
#[allow(clippy::too_many_arguments)]
pub(super) fn compile_method<'a>(
    class_name: &str,
    f: &FunctionDecl,
    fns: &'a HashMap<String, FnMeta>,
    arities: &'a [usize],
    variants: &'a HashMap<String, VariantMeta>,
    enum_descs: &'a [EnumDesc],
    classes: &'a HashMap<String, usize>,
    statics_index: &'a HashMap<(String, String), (usize, CTy)>,
    consts_index: &'a HashMap<(String, String), (Value, CTy)>,
    class_descs: &'a [ClassDesc],
    names_index: &'a HashMap<String, usize>,
    field_tags: &'a HashMap<String, CTy>,
    class_field_ctys: &'a HashMap<String, HashMap<String, CTy>>,
    method_rets: &'a HashMap<(String, String), CTy>,
    base_fn_idx: usize,
) -> Result<(Function, Vec<Function>), String> {
    let mut comp = Compiler::new(
        fns,
        arities,
        variants,
        enum_descs,
        classes,
        statics_index,
        consts_index,
        class_descs,
        names_index,
        field_tags,
        class_field_ctys,
        method_rets,
        base_fn_idx,
    );
    comp.cur_class = Some(class_name.to_string()); // `this` resolves to this class (ctype)
    comp.add_local("$this", CTy::Other); // slot 0 = receiver
    for p in &f.params {
        comp.add_local(&p.name, resolve_cty(&p.ty));
    }
    comp.this_slot = Some(0);
    comp.height = comp.locals.len();
    let last_line = f.span.line;
    for s in &f.body {
        comp.stmt(s)?;
    }
    comp.emit_const(Value::Unit, last_line);
    comp.emit(Op::Return, last_line);
    Ok((
        Function {
            name: format!("{class_name}::{}", f.name),
            arity: 1 + f.params.len(),
            n_captures: 0, // methods are never closures
            chunk: comp.chunk,
        },
        comp.extra_functions,
    ))
}
