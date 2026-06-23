//! Class/interface graph queries over the AST (M-Decomp W3.3): implements/supertypes
//! tables, MRO, method origins, field conflicts, constructor plan. Re-exported from `ast`.

use super::*;

/// The transitively-flattened interface set each concrete class implements, keyed by class name.
///
/// `class Dog implements Speaker` where `interface Speaker extends Named` ⇒ `Dog → [Named, Speaker]`
/// (every interface in the `implements` set *and* the `extends` closure of each). This is the single
/// runtime table behind `instanceof` against an interface: `x instanceof I` is true iff `I` is in
/// `class_implements[class_of(x)]`. It is computed **once** by this shared function and consumed
/// identically by the checker (subtyping + conformance), the interpreter, and the compiler/VM — one
/// algorithm, so the three backends can never diverge (the same discipline as [`free_vars`]).
///
/// The per-class list is **sorted** (invariant #8: deterministic order for all backends) and the
/// `extends` walk is **cycle-safe** via a visited set, so a malformed cyclic interface graph (which
/// the checker rejects as `E-IFACE-CYCLE` before any backend runs) can never make this loop forever.
/// Names are whatever the (already loader-mangled, if multi-package) AST carries — consistent across
/// every consumer.
pub fn class_implements(program: &Program) -> std::collections::BTreeMap<String, Vec<String>> {
    use std::collections::{BTreeMap, BTreeSet};
    // Direct `extends` edges for interfaces and for classes (M-RT S6), plus each class's own
    // `implements` list. A class inherits the interfaces of all its ancestor classes.
    let mut iface_extends: BTreeMap<&str, &[String]> = BTreeMap::new();
    let mut class_extends: BTreeMap<&str, &[String]> = BTreeMap::new();
    let mut own_implements: BTreeMap<&str, &[String]> = BTreeMap::new();
    for item in &program.items {
        match item {
            Item::Interface(i) => {
                iface_extends.insert(i.name.as_str(), &i.extends);
            }
            Item::Class(c) => {
                class_extends.insert(c.name.as_str(), &c.extends);
                own_implements.insert(c.name.as_str(), &c.implements);
            }
            _ => {}
        }
    }
    // Transitive closure of a name's `extends` chain (the name itself included), visited-guarded
    // against cycles. Used for both the interface graph and the class graph.
    fn closure<'a>(
        name: &'a str,
        edges: &BTreeMap<&'a str, &'a [String]>,
        acc: &mut BTreeSet<String>,
    ) {
        if !acc.insert(name.to_string()) {
            return; // already visited — also breaks any cycle
        }
        if let Some(parents) = edges.get(name) {
            for p in parents.iter() {
                closure(p, edges, acc);
            }
        }
    }
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for item in &program.items {
        if let Item::Class(c) = item {
            // The class itself plus every ancestor class (so inherited interfaces flow down, M-RT S6).
            let mut family: BTreeSet<String> = BTreeSet::new();
            closure(c.name.as_str(), &class_extends, &mut family);
            let mut ifaces: BTreeSet<String> = BTreeSet::new();
            for cls in &family {
                if let Some(impls) = own_implements.get(cls.as_str()) {
                    for i in impls.iter() {
                        closure(i, &iface_extends, &mut ifaces);
                    }
                }
            }
            out.insert(c.name.clone(), ifaces.into_iter().collect());
        }
    }
    out
}

/// Transitive parent-class closure for every class: `class_supertypes[c]` is the sorted set of all
/// ancestor class names reachable through `extends` — **not** including `c` itself, except when `c`
/// is part of an `extends` cycle (then `c` appears in its own set, which the checker uses to report
/// `E-MI-CYCLE`). Mirrors [`class_implements`]; the `extends` walk is cycle-safe via a visited set.
/// Consumed by the checker's nominal-subtype oracle (so `Dog <: Animal`) and (S6b+) the backends for
/// `instanceof` against a parent class — one algorithm, so the three backends can never diverge.
pub fn class_supertypes(program: &Program) -> std::collections::BTreeMap<String, Vec<String>> {
    use std::collections::{BTreeMap, BTreeSet};
    let mut class_extends: BTreeMap<&str, &[String]> = BTreeMap::new();
    for item in &program.items {
        if let Item::Class(c) = item {
            class_extends.insert(c.name.as_str(), &c.extends);
        }
    }
    // Accumulate the ancestors of `name` (parents, grandparents, …) — `name` itself is added only if
    // a cycle leads back to it.
    fn ancestors<'a>(
        name: &'a str,
        edges: &BTreeMap<&'a str, &'a [String]>,
        acc: &mut BTreeSet<String>,
    ) {
        if let Some(parents) = edges.get(name) {
            for p in parents.iter() {
                if acc.insert(p.clone()) {
                    ancestors(p, edges, acc);
                }
            }
        }
    }
    let mut out: std::collections::BTreeMap<String, Vec<String>> = BTreeMap::new();
    for item in &program.items {
        if let Item::Class(c) = item {
            let mut anc: BTreeSet<String> = BTreeSet::new();
            ancestors(c.name.as_str(), &class_extends, &mut anc);
            out.insert(c.name.clone(), anc.into_iter().collect());
        }
    }
    out
}

/// The **runtime subtype oracle** (M-RT S6c.3): for each class, every type name it is an instance of —
/// its transitive parent classes ([`class_supertypes`]) **and** its transitive interfaces
/// ([`class_implements`]). This is the single source consumed by `instanceof`, match type-patterns, and
/// overload subtyping on **both** backends, so a `Dog instanceof Animal` / `Duck instanceof Swimmer`
/// (a *class* ancestor, not just an interface) is true and can never diverge between `run` and `runvm`.
/// (The checker keeps a separate interfaces-only `class_implements` for interface *conformance*; its
/// `is_subtype` already consults `class_supertypes` independently.)
pub fn instanceof_table(program: &Program) -> std::collections::BTreeMap<String, Vec<String>> {
    let mut table = class_implements(program);
    for (cls, sups) in class_supertypes(program) {
        let entry = table.entry(cls).or_default();
        for s in sups {
            if !entry.contains(&s) {
                entry.push(s);
            }
        }
    }
    for v in table.values_mut() {
        v.sort();
        v.dedup();
    }
    table
}

/// Method-resolution order for every class: `class_mro[c]` is `c`'s ancestor classes in
/// **nearest-first breadth-first** order (direct parents in `extends` order, then their parents, …),
/// excluding `c` itself. Cycle-safe via a visited set. This is the **single source of dispatch
/// precedence** consumed by both the interpreter's `call_method` parent walk and the compiler's
/// method-table pre-flatten (M-RT S6b), so the two backends can never disagree on *which* ancestor a
/// method is inherited from. A method is resolved by scanning `[c] ++ class_mro[c]` and taking the
/// first class that declares it (so a nearer declaration overrides a farther one); a diamond shared
/// base is visited once, auto-merging when both arms reach the same declaring method.
pub fn class_mro(program: &Program) -> std::collections::BTreeMap<String, Vec<String>> {
    use std::collections::{BTreeMap, HashSet};
    let parents: BTreeMap<&str, &[String]> = program
        .items
        .iter()
        .filter_map(|it| match it {
            Item::Class(c) => Some((c.name.as_str(), c.extends.as_slice())),
            _ => None,
        })
        .collect();
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for item in &program.items {
        if let Item::Class(c) = item {
            let mut order = Vec::new();
            let mut seen: HashSet<String> = HashSet::new();
            let mut queue: Vec<String> = c.extends.clone();
            let mut i = 0;
            while i < queue.len() {
                let p = queue[i].clone();
                i += 1;
                if !seen.insert(p.clone()) {
                    continue;
                }
                order.push(p.clone());
                if let Some(gps) = parents.get(p.as_str()) {
                    queue.extend(gps.iter().cloned());
                }
            }
            out.insert(c.name.clone(), order);
        }
    }
    out
}

/// The fully-resolved method-dispatch table for every class (M-RT S6b): for each `(class, name)` it
/// gives the `(declaring_class, declaring_method)` a call of `name` on an instance of `class` runs.
/// This is the **single source of dispatch** for *both* backends — the interpreter looks up the
/// origin and the compiler aliases the bytecode method-table entry to it — so multi-parent dispatch
/// (including resolution clauses and renamed aliases) can never diverge between `run` and `runvm`.
///
/// Composition: a class's own methods map to itself (override); each direct parent contributes its
/// own resolved table; a diamond shared base auto-merges (both arms reach the *same* declaring
/// method). Resolution clauses (`use`/`rename`/`exclude`) are applied before finalizing. The second
/// return value lists every **unresolved** cross-parent collision as `(class, name, class_span)` —
/// the checker reports each as `E-MI-CONFLICT`. On a conflict the table still records a deterministic
/// pick so a backend never panics (the checker fails the build first).
#[allow(clippy::type_complexity)]
pub fn class_method_origins(
    program: &Program,
) -> (
    std::collections::BTreeMap<(String, String), (String, String)>,
    Vec<(String, String, Span)>,
) {
    use std::collections::{BTreeMap, BTreeSet};

    struct Ctx {
        decl: BTreeMap<String, BTreeSet<String>>,
        extends: BTreeMap<String, Vec<String>>,
        /// M-RT S8: class → the traits it `use`s. A used trait contributes its own methods exactly like
        /// a parent contributes its resolved table, so trait-vs-trait / trait-vs-parent collisions and
        /// `use`/`rename`/`exclude` resolution clauses all reuse the same machinery.
        uses: BTreeMap<String, Vec<String>>,
        /// M-RT S8: trait name → its declared method names.
        trait_decl: BTreeMap<String, BTreeSet<String>>,
        resolutions: BTreeMap<String, Vec<Resolution>>,
        spans: BTreeMap<String, Span>,
        memo: BTreeMap<String, BTreeMap<String, (String, String)>>,
        conflicts: Vec<(String, String, Span)>,
        in_progress: BTreeSet<String>,
    }

    impl Ctx {
        fn resolve(&mut self, c: &str) -> BTreeMap<String, (String, String)> {
            if let Some(m) = self.memo.get(c) {
                return m.clone();
            }
            if !self.in_progress.insert(c.to_string()) {
                // `extends` cycle — reported as `E-MI-CYCLE` elsewhere; break to avoid infinite loop.
                return BTreeMap::new();
            }
            let mut map: BTreeMap<String, (String, String)> = BTreeMap::new();
            // Own methods win over anything inherited (override).
            if let Some(ms) = self.decl.get(c).cloned() {
                for m in ms {
                    map.insert(m.clone(), (c.to_string(), m));
                }
            }
            // Gather each direct parent's resolved contributions, tracking the direct parent the
            // method arrives through (so a `use/rename/exclude P.m` clause can target it) and the true
            // origin (so a diamond dedups by origin).
            let mut contrib: BTreeMap<String, Vec<(String, (String, String))>> = BTreeMap::new();
            for p in self.extends.get(c).cloned().unwrap_or_default() {
                let p_map = self.resolve(&p);
                for (name, origin) in p_map {
                    if map.contains_key(&name) {
                        continue; // overridden by C itself
                    }
                    contrib.entry(name).or_default().push((p.clone(), origin));
                }
            }
            // M-RT S8: each `use`d trait contributes its own declared methods. Tracked by the trait
            // name (so a `use/rename/exclude Trait.m` clause can target it) with origin `(trait, m)`
            // (so two traits supplying the *same* method collide and need resolution; the class's own
            // method still wins via the `map.contains_key` guard).
            for t in self.uses.get(c).cloned().unwrap_or_default() {
                for name in self.trait_decl.get(&t).cloned().unwrap_or_default() {
                    if map.contains_key(&name) {
                        continue; // overridden by C itself
                    }
                    contrib
                        .entry(name.clone())
                        .or_default()
                        .push((t.clone(), (t.clone(), name)));
                }
            }
            // Apply resolution clauses in source order.
            for r in self.resolutions.get(c).cloned().unwrap_or_default() {
                match r {
                    Resolution::Use { parent, method, .. } => {
                        if let Some(v) = contrib.get_mut(&method) {
                            v.retain(|(pn, _)| pn == &parent);
                        }
                    }
                    Resolution::Exclude { parent, method, .. } => {
                        if let Some(v) = contrib.get_mut(&method) {
                            v.retain(|(pn, _)| pn != &parent);
                        }
                    }
                    Resolution::Rename {
                        parent,
                        method,
                        as_name,
                        ..
                    } => {
                        let moved: Vec<(String, (String, String))> =
                            if let Some(v) = contrib.get_mut(&method) {
                                let (keep, take): (Vec<_>, Vec<_>) =
                                    v.drain(..).partition(|(pn, _)| pn != &parent);
                                *v = keep;
                                take
                            } else {
                                Vec::new()
                            };
                        if !moved.is_empty() {
                            contrib.entry(as_name).or_default().extend(moved);
                        }
                    }
                }
            }
            // Finalize each inherited name: dedup by origin (diamond), else conflict.
            for (name, v) in contrib {
                if map.contains_key(&name) {
                    continue;
                }
                let distinct: BTreeSet<(String, String)> = v.into_iter().map(|(_, o)| o).collect();
                let mut it = distinct.into_iter();
                match it.next() {
                    None => {}
                    Some(first) => {
                        if it.next().is_some() {
                            let span = self.spans.get(c).copied().unwrap_or(Span {
                                start: 0,
                                len: 0,
                                line: 1,
                                col: 1,
                            });
                            self.conflicts.push((c.to_string(), name.clone(), span));
                        }
                        map.insert(name, first); // deterministic pick (sorted-first)
                    }
                }
            }
            self.in_progress.remove(c);
            self.memo.insert(c.to_string(), map.clone());
            map
        }
    }

    let mut ctx = Ctx {
        decl: BTreeMap::new(),
        extends: BTreeMap::new(),
        uses: BTreeMap::new(),
        trait_decl: BTreeMap::new(),
        resolutions: BTreeMap::new(),
        spans: BTreeMap::new(),
        memo: BTreeMap::new(),
        conflicts: Vec::new(),
        in_progress: BTreeSet::new(),
    };
    for item in &program.items {
        match item {
            Item::Class(c) => {
                let mut ms = BTreeSet::new();
                for m in &c.members {
                    if let ClassMember::Method(f) = m {
                        ms.insert(f.name.clone());
                    }
                }
                ctx.decl.insert(c.name.clone(), ms);
                ctx.extends.insert(c.name.clone(), c.extends.clone());
                ctx.uses.insert(
                    c.name.clone(),
                    c.uses.iter().map(|u| u.name.clone()).collect(),
                );
                ctx.resolutions
                    .insert(c.name.clone(), c.resolutions.clone());
                ctx.spans.insert(c.name.clone(), c.span);
            }
            Item::Trait(t) => {
                let mut ms = BTreeSet::new();
                for m in &t.members {
                    if let ClassMember::Method(f) = m {
                        ms.insert(f.name.clone());
                    }
                }
                ctx.trait_decl.insert(t.name.clone(), ms);
            }
            _ => {}
        }
    }
    let names: Vec<String> = ctx.extends.keys().cloned().collect();
    for n in &names {
        ctx.resolve(n);
    }
    let mut out: BTreeMap<(String, String), (String, String)> = BTreeMap::new();
    for (c, m) in &ctx.memo {
        for (name, origin) in m {
            out.insert((c.clone(), name.clone()), origin.clone());
        }
    }
    (out, ctx.conflicts)
}

/// M-RT S6c — instance-field collision detection, the field analog of [`class_method_origins`].
/// Returns every `(class, field, class_span)` where a class inherits a same-named instance field from
/// **two or more distinct declaring origins** without redeclaring it — the checker reports each as
/// `E-MI-FIELD-CONFLICT`. Unlike methods there are no resolution clauses (PHP has no `insteadof` for
/// properties), so a child can resolve a collision only by redeclaring the field itself.
///
/// "Instance field" = an explicit non-`static` `Field` member plus every promoted constructor
/// parameter (one carrying a `public`/`private`/`protected` modifier — these become fields, EV-4).
/// A diamond auto-merges exactly like a shared method: a field reached through two arms that resolve
/// to the *same* declaring class dedups (no conflict). Static fields are out of scope this slice.
pub fn class_field_conflicts(program: &Program) -> Vec<(String, String, Span)> {
    use std::collections::{BTreeMap, BTreeSet};

    struct Ctx {
        decl: BTreeMap<String, BTreeSet<String>>,
        extends: BTreeMap<String, Vec<String>>,
        spans: BTreeMap<String, Span>,
        memo: BTreeMap<String, BTreeMap<String, String>>,
        conflicts: Vec<(String, String, Span)>,
        in_progress: BTreeSet<String>,
    }

    impl Ctx {
        /// Resolve `c`'s flat instance-field table: each field name → its single declaring origin
        /// class. Own fields win (redeclare); a name arriving from ≥2 distinct origins is recorded as
        /// a conflict (a deterministic pick still lands in the table so the build can continue).
        fn resolve(&mut self, c: &str) -> BTreeMap<String, String> {
            if let Some(m) = self.memo.get(c) {
                return m.clone();
            }
            if !self.in_progress.insert(c.to_string()) {
                return BTreeMap::new(); // `extends` cycle — `E-MI-CYCLE` reported elsewhere
            }
            let mut map: BTreeMap<String, String> = BTreeMap::new();
            // Own fields win over anything inherited (the child redeclaring resolves a collision).
            if let Some(fs) = self.decl.get(c).cloned() {
                for f in fs {
                    map.insert(f, c.to_string());
                }
            }
            // Gather each direct parent's resolved fields, tracking the true declaring origin so a
            // diamond dedups by origin.
            let mut contrib: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
            for p in self.extends.get(c).cloned().unwrap_or_default() {
                for (name, origin) in self.resolve(&p) {
                    if map.contains_key(&name) {
                        continue; // redeclared by C itself
                    }
                    contrib.entry(name).or_default().insert(origin);
                }
            }
            for (name, origins) in contrib {
                if map.contains_key(&name) {
                    continue;
                }
                let mut it = origins.into_iter();
                if let Some(first) = it.next() {
                    if it.next().is_some() {
                        let span = self.spans.get(c).copied().unwrap_or(Span {
                            start: 0,
                            len: 0,
                            line: 1,
                            col: 1,
                        });
                        self.conflicts.push((c.to_string(), name.clone(), span));
                    }
                    map.insert(name, first); // deterministic pick (sorted-first)
                }
            }
            self.in_progress.remove(c);
            self.memo.insert(c.to_string(), map.clone());
            map
        }
    }

    let mut ctx = Ctx {
        decl: BTreeMap::new(),
        extends: BTreeMap::new(),
        spans: BTreeMap::new(),
        memo: BTreeMap::new(),
        conflicts: Vec::new(),
        in_progress: BTreeSet::new(),
    };
    for item in &program.items {
        if let Item::Class(c) = item {
            let mut fs = BTreeSet::new();
            for m in &c.members {
                match m {
                    ClassMember::Field {
                        name, modifiers, ..
                    } if !modifiers.contains(&Modifier::Static) => {
                        fs.insert(name.clone());
                    }
                    ClassMember::Constructor { params, .. } => {
                        for p in params {
                            if p.modifiers.iter().any(|m| {
                                matches!(
                                    m,
                                    Modifier::Public | Modifier::Private | Modifier::Protected
                                )
                            }) {
                                fs.insert(p.name.clone());
                            }
                        }
                    }
                    _ => {}
                }
            }
            ctx.decl.insert(c.name.clone(), fs);
            ctx.extends.insert(c.name.clone(), c.extends.clone());
            ctx.spans.insert(c.name.clone(), c.span);
        }
    }
    let names: Vec<String> = ctx.extends.keys().cloned().collect();
    for n in &names {
        ctx.resolve(n);
    }
    ctx.conflicts
}

/// The ordered list of constructors a `ClassName(args)` call runs (M-RT S6c.2). Each entry is one
/// `(params, body)` to execute, in order, on the single instance being built; the call's full argument
/// list is the entries' params concatenated in this order, sliced per entry.
///
/// - A class with its **own** constructor → just that one (`[own]`).
/// - **Single** inheritance, no own ctor → the parent's plan (the nearest ancestor's ctor, transitively
///   chained — S6c.2a).
/// - **Multiple** inheritance, no own ctor → each parent's plan concatenated in `extends` order, so
///   every parent's constructor runs and initializes its fields (S6c.2b). A diamond-shared base's ctor
///   runs once per arm — identically on all three backends, so byte-identity holds.
/// - No ctor anywhere → `[]` (a zero-arg `ClassName()` builds an empty instance).
///
/// Single source of the construction decision: checker (signature = concatenated param types),
/// compiler (instance descriptor + synthetic ctor body + arity), interpreter (run each entry with its
/// arg slice). A child that declares its *own* ctor under inheritance returns just its own — initializing
/// inherited state then needs the deferred `super`-replacement (KNOWN_ISSUES).
pub fn ctor_plan(program: &Program, class: &str) -> Vec<(Vec<CtorParam>, Vec<Stmt>)> {
    let Some(decl) = program.items.iter().find_map(|it| match it {
        Item::Class(c) if c.name == class => Some(c),
        _ => None,
    }) else {
        return Vec::new();
    };
    if let Some((p, b)) = decl.members.iter().find_map(|m| match m {
        ClassMember::Constructor { params, body, .. } => Some((params.clone(), body.clone())),
        _ => None,
    }) {
        return vec![(p, b)];
    }
    // M-RT S8: a `use`d trait's constructor becomes the class's constructor and **wins over an
    // inherited parent ctor** (PHP P2 — the parent ctor is not auto-run; the checker warns
    // `W-TRAIT-CTOR-PARENT-SKIPPED`). The checker rejects two unresolved trait ctors
    // (`E-TRAIT-CTOR-COLLISION`), so a clean program has at most one — take it deterministically.
    if let Some(tc) = decl.uses.iter().find_map(|u| {
        program.items.iter().find_map(|it| match it {
            Item::Trait(t) if t.name == u.name => t.members.iter().find_map(|m| match m {
                ClassMember::Constructor { params, body, .. } => {
                    Some((params.clone(), body.clone()))
                }
                _ => None,
            }),
            _ => None,
        })
    }) {
        return vec![tc];
    }
    match decl.extends.len() {
        0 => Vec::new(),
        1 => ctor_plan(program, &decl.extends[0]),
        _ => decl
            .extends
            .iter()
            .flat_map(|p| ctor_plan(program, p))
            .collect(),
    }
}
