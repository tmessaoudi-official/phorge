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

/// Resolve a program **entry point** (`main` / `handle`) — the single source of truth all backends
/// share so they invoke the same function (Batch-1 D, `docs/specs/2026-06-27-class-entry-points-design.md`).
///
/// An entry is **either** a top-level free function named `name` (returns `Some((None, decl))`) **or**
/// a `static` method named `name` on some class (`Some((Some(class), decl))`). An *instance* method
/// named `name` is **not** an entry (an ordinary method). Top-level wins the scan order, but a valid
/// program has at most one entry — [`entry_point_count`] backs the checker's `E-MULTIPLE-MAIN`, so by
/// the time any backend calls this the entry is unambiguous.
pub fn entry_point<'a>(
    program: &'a Program,
    name: &str,
) -> Option<(Option<&'a str>, &'a FunctionDecl)> {
    for item in &program.items {
        if let Item::Function(f) = item {
            if f.name == name {
                return Some((None, f));
            }
        }
    }
    for item in &program.items {
        if let Item::Class(c) = item {
            for m in &c.members {
                if let ClassMember::Method(f) = m {
                    if f.name == name && f.modifiers.contains(&Modifier::Static) {
                        return Some((Some(c.name.as_str()), f));
                    }
                }
            }
        }
    }
    None
}

/// How many distinct entry points named `name` a program declares (a top-level function plus every
/// class-static method of that name). `> 1` is the checker's `E-MULTIPLE-MAIN` — an ambiguous entry is
/// an error, never a silent pick.
pub fn entry_point_count(program: &Program, name: &str) -> usize {
    let mut n = 0;
    for item in &program.items {
        match item {
            Item::Function(f) if f.name == name => n += 1,
            Item::Class(c) => {
                for m in &c.members {
                    if let ClassMember::Method(f) = m {
                        if f.name == name && f.modifiers.contains(&Modifier::Static) {
                            n += 1;
                        }
                    }
                }
            }
            _ => {}
        }
    }
    n
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

/// Direct parents (`extends`) of every class, keyed by class name — `Dog → [Animal]`, `Both →
/// [Left, Right]`, a root class → `[]`. The immediate-parent view (vs. [`class_mro`]'s transitive
/// ancestors); used by `parent`/super resolution (M-RT super/parent dispatch).
pub fn class_parents(program: &Program) -> std::collections::BTreeMap<String, Vec<String>> {
    let mut out = std::collections::BTreeMap::new();
    for item in &program.items {
        if let Item::Class(c) = item {
            out.insert(c.name.clone(), c.extends.clone());
        }
    }
    out
}

/// Why a `parent`/super call could not resolve (M-RT super/parent dispatch). Mapped to the
/// `E-PARENT-*` diagnostics by the checker; the backends never see an error (the build fails first),
/// but they call the same [`resolve_parent_method`] so a resolved target is byte-identical across
/// `run`/`runvm`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParentResolveError {
    /// The lexical class has no parents at all (`E-PARENT-NO-PARENT`).
    NoParent,
    /// `parent(A)` where `A` is not a transitive ancestor of the lexical class (`E-PARENT-NOT-ANCESTOR`).
    NotAncestor,
    /// No ancestor (or the named one) declares/inherits the method (`E-PARENT-NO-METHOD`).
    NoMethod,
    /// Bare `parent.m()` in a multiple-inheritance class where ≥2 parent arms resolve `m` to distinct
    /// origins (`E-PARENT-AMBIGUOUS`); the fix is to qualify with `parent(A).m()`.
    Ambiguous,
}

/// Resolve a `parent`/super method call to its concrete `(declaring_class, method)` target — the
/// **single source** consumed by the checker (errors + typing), the interpreter (dispatch), and the
/// compiler (bakes the function index), so `run ≡ runvm` by construction (the same discipline as
/// [`class_method_origins`]). `parent` is **lexical**: `lexical_class` is the class whose method body
/// contains the call (where the override is *written*), NOT the receiver's runtime class — so an
/// override calling `parent.m()` reaches the version it shadows.
///
/// - `ancestor = Some(A)` (`parent(A).m()`): `A` must be in `mro[lexical_class]`
///   (`NotAncestor`); the target is `origins[(A, method)]` — the `m` an instance of `A` runs
///   (its own or its nearest ancestor's). `A` may be **any** transitive ancestor (C++-style jump).
/// - `ancestor = None` (`parent.m()`): the nearest ancestor declaring `m`, found by collecting the
///   distinct origins each **direct** parent resolves `m` to — exactly one ⇒ that target; zero ⇒
///   `NoMethod`; ≥2 distinct ⇒ `Ambiguous` (only possible under multiple inheritance).
///
/// `method` may be `"constructor"` (a parent-constructor call); origins never contains a constructor
/// entry, so a ctor call always resolves via the direct-parent arm against `parents`, handled by the
/// caller — this function is method-only and the caller special-cases the constructor.
pub fn resolve_parent_method(
    parents: &std::collections::BTreeMap<String, Vec<String>>,
    mro: &std::collections::BTreeMap<String, Vec<String>>,
    origins: &std::collections::BTreeMap<(String, String), (String, String)>,
    lexical_class: &str,
    ancestor: Option<&str>,
    method: &str,
) -> Result<(String, String), ParentResolveError> {
    let direct = parents.get(lexical_class).map_or(&[][..], Vec::as_slice);
    if direct.is_empty() {
        return Err(ParentResolveError::NoParent);
    }
    match ancestor {
        Some(a) => {
            let is_ancestor = mro
                .get(lexical_class)
                .is_some_and(|anc| anc.iter().any(|x| x == a));
            if !is_ancestor {
                return Err(ParentResolveError::NotAncestor);
            }
            origins
                .get(&(a.to_string(), method.to_string()))
                .cloned()
                .ok_or(ParentResolveError::NoMethod)
        }
        None => {
            // Distinct origins each direct parent resolves `method` to (a diamond's shared base
            // collapses to one origin, so it is not ambiguous).
            let mut found: Vec<(String, String)> = Vec::new();
            for p in direct {
                if let Some(t) = origins.get(&(p.clone(), method.to_string())) {
                    if !found.contains(t) {
                        found.push(t.clone());
                    }
                }
            }
            match found.len() {
                0 => Err(ParentResolveError::NoMethod),
                1 => Ok(found.into_iter().next().unwrap()),
                _ => Err(ParentResolveError::Ambiguous),
            }
        }
    }
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

/// The **instance-field layout** of every class: each class name maps to the deterministic, sorted list
/// of its storage field *names* (the slot order is its index in this list). The single source both
/// backends consult to build an instance's slot-indexed `Vec` and to resolve a `name → slot` at a field
/// access (M-perf slot-indexed fields). Because *both* construction and access go through this same
/// layout resolved against the instance's **runtime** class, the slot order is irrelevant to
/// correctness — so a stable sorted order suffices and the multiple-inheritance base-offset hazard never
/// arises (slots are always runtime-resolved, never statically baked).
///
/// "Storage field" = every name that could ever be populated on an instance: an explicit non-`static`
/// `Field` member, a promoted constructor parameter (`public`/`private`/`protected`), the same from each
/// `use`d trait, and all of the above inherited transitively through `extends`. Property hooks, `static`
/// fields, and `const`s are excluded (they are not per-instance storage). The union is intentionally a
/// **superset-or-equal** of what construction populates — an extra never-populated slot is harmless
/// (stays the `None` sentinel), whereas a missing slot would be fatal, so the walk errs toward
/// inclusion. Cycle-safe via a visited set (an `extends` cycle is `E-MI-CYCLE` elsewhere).
pub fn class_field_layout(program: &Program) -> std::collections::BTreeMap<String, Vec<String>> {
    use std::collections::{BTreeMap, BTreeSet};
    // Own storage field names per class OR trait (promoted ctor params + non-static `Field` members).
    let own_fields = |members: &[ClassMember]| -> Vec<String> {
        let mut v = Vec::new();
        for m in members {
            match m {
                ClassMember::Field {
                    name, modifiers, ..
                } if !modifiers.contains(&Modifier::Static)
                    && !modifiers.contains(&Modifier::Const) =>
                {
                    v.push(name.clone());
                }
                ClassMember::Constructor { params, .. } => {
                    for p in params {
                        if p.modifiers.iter().any(|m| {
                            matches!(
                                m,
                                Modifier::Public | Modifier::Private | Modifier::Protected
                            )
                        }) {
                            v.push(p.name.clone());
                        }
                    }
                }
                _ => {}
            }
        }
        v
    };
    let mut own: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut extends: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut uses: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for item in &program.items {
        match item {
            Item::Class(c) => {
                own.insert(c.name.clone(), own_fields(&c.members));
                extends.insert(c.name.clone(), c.extends.clone());
                uses.insert(
                    c.name.clone(),
                    c.uses.iter().map(|u| u.name.clone()).collect(),
                );
            }
            Item::Trait(t) => {
                own.insert(t.name.clone(), own_fields(&t.members));
            }
            _ => {}
        }
    }
    // Transitively gather a class's full storage-field set (own + used-trait + every ancestor's),
    // cycle-safe. The `extends`/`uses` of a trait are empty, so a trait contributes only its own.
    fn gather(
        name: &str,
        own: &BTreeMap<String, Vec<String>>,
        extends: &BTreeMap<String, Vec<String>>,
        uses: &BTreeMap<String, Vec<String>>,
        seen: &mut BTreeSet<String>,
        out: &mut BTreeSet<String>,
    ) {
        if !seen.insert(name.to_string()) {
            return;
        }
        if let Some(fs) = own.get(name) {
            out.extend(fs.iter().cloned());
        }
        for t in uses.get(name).into_iter().flatten() {
            gather(t, own, extends, uses, seen, out);
        }
        for p in extends.get(name).into_iter().flatten() {
            gather(p, own, extends, uses, seen, out);
        }
    }
    let mut layout = BTreeMap::new();
    for class in extends.keys() {
        let mut set = BTreeSet::new();
        let mut seen = BTreeSet::new();
        gather(class, &own, &extends, &uses, &mut seen, &mut set);
        // `BTreeSet` iteration is sorted ⇒ a deterministic, backend-independent slot order.
        layout.insert(class.clone(), set.into_iter().collect());
    }
    layout
}

/// The flattened **class-constant** table (Feature A — `const` class constants): for each
/// `(class, NAME)` it gives the constant's literal `Value` and its declared `Type`. Includes a class's
/// own `const` members **plus** those inherited through `extends` and folded in from `use`d traits —
/// own declaration wins over an inherited one (PHP redeclare semantics; the field analog of
/// [`class_field_conflicts`]). A subclass therefore reaches an inherited const through its **own** name
/// (`Sub.MAX` resolves even when `MAX` is declared on a parent), matching PHP.
///
/// This is the single source consumed by all three backends — the interpreter inlines the `Value`, the
/// compiler emits `Op::Const` + derives the operand `CTy` from the `Type`, and the transpiler keys its
/// `ClassName::NAME` access emission on the table's keys — so a const access can never diverge between
/// `run`, `runvm`, and real PHP. The checker validates each initializer is a compile-time literal
/// before any backend runs, so `const_literal` here is total (a checker-unreachable non-literal folds
/// to `Unit`, like a static's). The `extends`/`use` walk is cycle-safe via a visited set.
pub fn class_consts(
    program: &Program,
) -> std::collections::BTreeMap<(String, String), (crate::value::Value, Type)> {
    use std::collections::{BTreeMap, BTreeSet};
    // Own `const` members per class/trait: NAME → (literal Value, Type).
    let mut own: BTreeMap<String, BTreeMap<String, (crate::value::Value, Type)>> = BTreeMap::new();
    let mut extends: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut uses: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let collect_own = |members: &[ClassMember]| -> BTreeMap<String, (crate::value::Value, Type)> {
        let mut m = BTreeMap::new();
        for mem in members {
            if let ClassMember::Field {
                modifiers,
                name,
                ty,
                init,
                ..
            } = mem
            {
                if modifiers.contains(&Modifier::Const) {
                    let v = init
                        .as_ref()
                        .and_then(crate::value::const_literal)
                        .unwrap_or(crate::value::Value::Unit);
                    m.insert(name.clone(), (v, ty.clone()));
                }
            }
        }
        m
    };
    for item in &program.items {
        match item {
            Item::Class(c) => {
                own.insert(c.name.clone(), collect_own(&c.members));
                extends.insert(c.name.clone(), c.extends.clone());
                uses.insert(
                    c.name.clone(),
                    c.uses.iter().map(|u| u.name.clone()).collect(),
                );
            }
            Item::Trait(t) => {
                own.insert(t.name.clone(), collect_own(&t.members));
            }
            _ => {}
        }
    }
    // Flatten `c`'s consts: own + each `use`d trait's + each parent's (transitively), own/nearer wins.
    fn flatten(
        c: &str,
        own: &BTreeMap<String, BTreeMap<String, (crate::value::Value, Type)>>,
        extends: &BTreeMap<String, Vec<String>>,
        uses: &BTreeMap<String, Vec<String>>,
        seen: &mut BTreeSet<String>,
    ) -> BTreeMap<String, (crate::value::Value, Type)> {
        let mut acc: BTreeMap<String, (crate::value::Value, Type)> = BTreeMap::new();
        if !seen.insert(c.to_string()) {
            return acc; // `extends` cycle — `E-MI-CYCLE` reported elsewhere
        }
        // Own declarations win — insert them first and never overwrite.
        if let Some(m) = own.get(c) {
            for (k, v) in m {
                acc.insert(k.clone(), v.clone());
            }
        }
        // Then `use`d traits, then parents — only filling names not already bound (nearer wins).
        for t in uses.get(c).map(Vec::as_slice).unwrap_or(&[]) {
            for (k, v) in flatten(t, own, extends, uses, seen) {
                acc.entry(k).or_insert(v);
            }
        }
        for p in extends.get(c).map(Vec::as_slice).unwrap_or(&[]) {
            for (k, v) in flatten(p, own, extends, uses, seen) {
                acc.entry(k).or_insert(v);
            }
        }
        seen.remove(c);
        acc
    }
    let mut out: BTreeMap<(String, String), (crate::value::Value, Type)> = BTreeMap::new();
    for item in &program.items {
        if let Item::Class(c) = item {
            let mut seen = BTreeSet::new();
            for (name, vt) in flatten(&c.name, &own, &extends, &uses, &mut seen) {
                out.insert((c.name.clone(), name), vt);
            }
        }
    }
    out
}

/// The class's **own** expression field initializers (Feature B): each `(field_name, init_expr)` for a
/// plain instance field (non-`static`, non-`const`) carrying an initializer, in declaration order.
/// Used by the transpiler to build *this* class's `__construct` prelude (PHP doesn't auto-chain, so a
/// class's constructor runs only its own initializers — see [`field_initializers`]).
pub fn own_field_initializers(decl: &ClassDecl) -> Vec<(String, Expr)> {
    decl.members
        .iter()
        .filter_map(|m| match m {
            ClassMember::Field {
                modifiers,
                name,
                init: Some(e),
                ..
            } if !modifiers.contains(&Modifier::Static)
                && !modifiers.contains(&Modifier::Const) =>
            {
                Some((name.clone(), e.clone()))
            }
            _ => None,
        })
        .collect()
}

/// The ordered **expression field initializers** to run when constructing `class` (Feature B) — the
/// own initializers of the class whose constructor PHP actually invokes for `new class()`.
///
/// PHP does **not** auto-chain `parent::__construct`, so `new C()` runs exactly *one* class's
/// constructor body+prelude: `C`'s own if `C` declares a constructor or has its own field
/// initializers, otherwise the nearest ancestor's (the one PHP inherits the constructor from). This
/// helper returns that class's own field initializers, in declaration order — so an initializer may
/// read `this` and an **earlier-declared sibling** (a later field is `E-FIELD-INIT-FORWARD-REF`).
///
/// Returning the *invoked* constructor's own initializers (rather than every ancestor's) keeps the
/// three backends byte-identical with PHP's constructor inheritance: the interpreter sets these after
/// promotion, the compiler emits `SetField` for them, and the transpiler prepends exactly these to the
/// same class's `__construct`. Cycle-safe (reuses [`class_mro`]).
pub fn field_initializers(program: &Program, class: &str) -> Vec<(String, Expr)> {
    let find = |name: &str| -> Option<&ClassDecl> {
        program.items.iter().find_map(|it| match it {
            Item::Class(c) if c.name == name => Some(c),
            _ => None,
        })
    };
    // self-first, then ancestors nearest-first (the order PHP resolves an inherited constructor).
    let chain: Vec<String> = std::iter::once(class.to_string())
        .chain(class_mro(program).get(class).cloned().unwrap_or_default())
        .collect();
    for cls in &chain {
        let Some(decl) = find(cls) else { continue };
        let has_ctor = decl
            .members
            .iter()
            .any(|m| matches!(m, ClassMember::Constructor { .. }));
        let own = own_field_initializers(decl);
        // The first class with an own constructor OR own field initializers is the one whose
        // `__construct` PHP invokes — run its initializers (which may be empty if it only has a ctor).
        if has_ctor || !own.is_empty() {
            return own;
        }
    }
    Vec::new()
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
