//! Resolved (internal) type representation, distinct from the AST's `Type`.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ty {
    Int,
    Float,
    Bool,
    String,
    /// A sequence of raw octets (not UTF-8). Converts to/from `string` only via `core.bytes`.
    Bytes,
    /// Escaped, render-ready HTML. A distinct *nominal* type — like `bytes`, it erases to PHP
    /// `string` at transpile and rides `Value::Str` at runtime, but the checker keeps it separate
    /// from `string`: untrusted text cannot reach `Html` except through `core.html.text` (escape) or
    /// the audited `core.html.raw`. That non-interchangeability is the whole XSS-safety property, and
    /// it falls out for free from `assignable`'s `from == to` final arm (no coercion added).
    Html,
    /// A single rendered HTML attribute — e.g. ` href="…"` (note the leading space) — produced by
    /// `core.html.attr` / `bool_attr` and consumed by `core.html.el` / `void_el`. Like `Html`, a
    /// distinct nominal type that erases to PHP `string` and rides `Value::Str` at runtime; kept
    /// separate so an attribute fragment can't be spliced where element content (`Html`) is expected
    /// and vice versa. core.html Wave 2 (the builders).
    Attr,
    Unit,
    /// A nominal enum, interface, or class type, by name, with type arguments. `args` is empty for a
    /// non-generic nominal (every enum/interface, and a non-generic class) — so the common case is
    /// `Ty::Named(name, vec![])`. A generic class instance carries its inferred arguments
    /// (`Box<int>` ⇒ `Ty::Named("Box", [Int])`, M-RT generics-all): construction infers them by
    /// unifying the constructor parameters against the call's arguments, and member access substitutes
    /// the class's type parameters → these arguments. The arguments are **erased before any backend**
    /// (a class's own `<T>`-typed members become `Type::Erased`; a use-site `Box<int>` annotation
    /// keeps its args but the backends ignore them — `resolve_cty`/`emit_type` key on the name only).
    Named(String, Vec<Ty>),
    List(Box<Ty>),
    Map(Box<Ty>, Box<Ty>),
    Set(Box<Ty>),
    /// `T?` — an optional: holds a `T` or `null`. The non-null guarantee lives in
    /// `assignable` (a non-optional `T` can never hold `null`).
    Optional(Box<Ty>),
    /// The type of the bare `null` literal: assignable to any `T?` and to nothing else. Lets
    /// `null` flow into an optional with no element type, while `var x = null;` stays an error.
    Null,
    /// A generic type *parameter* — `T` in `function id<T>(T x) -> T` (M-RT S7). It appears only in
    /// the *stored signature* of a generic function (so a call site can [`unify`](crate::checker)
    /// it against concrete argument types) and inside that function's body (where it behaves as an
    /// opaque nominal type — assignable only to the same-named parameter, no coercion). It is fully
    /// **erased** before any backend runs (`checker::erase_generics` rewrites the AST `Type::Named`
    /// for a param into `Type::Erased`), so the interpreter/compiler/transpiler never see it — the
    /// same "compile-time-only, expanded out" discipline as `type` aliases and `html"…"`.
    Param(String),
    /// A union of two or more distinct types — `A | B | C` (M-RT S4). **Normalized**: members are
    /// flattened (no nested unions), deduplicated, and sorted into a canonical order (by `Display`),
    /// so `A | B` and `B | A` are the *same* `Ty` and assignability is order-independent. A union that
    /// would collapse to a single member *is* that member (built via [`Ty::union_of`]). Members are
    /// classes/interfaces/primitives (the checker rejects enum/optional/function members). Erased to
    /// PHP 8.0 `A|B`; the backends never see a union as a runtime *value* shape (a value is always a
    /// concrete instance/primitive) — the annotation gates only the checker and the PHP signature.
    Union(Vec<Ty>),
    /// An intersection of two or more distinct types — `A & B & C` (M-RT S5), the narrowing dual of a
    /// union. **Normalized** like a union (flatten/dedupe/canonical-sort by `Display`), so `A & B` and
    /// `B & A` are the *same* `Ty` and assignability is order-independent. A collapse to one member *is*
    /// that member (built via [`Ty::intersection_of`]). Members are interfaces plus at most one class
    /// (the checker enforces the kind rule). Erased to PHP 8.1 `A&B`; the backends never see an
    /// intersection as a runtime *value* shape (a value is always a concrete instance) — the annotation
    /// gates only the checker and the PHP signature, and member access searches every member.
    Intersection(Vec<Ty>),
    /// Poison type: a failed sub-expression yields this. Assignable both ways so a
    /// single error does not cascade into many.
    Error,
    /// A function type: `(int, string) -> bool`. Exact match only — no subtyping variance (A6).
    Function(Vec<Ty>, Box<Ty>),
}

impl Ty {
    /// `from` may be used where `to` is expected. `Error` unifies with anything to
    /// suppress cascade errors. No numeric widening (spec §3: no implicit coercion).
    /// Optionals are covariant and non-null-disciplined: a non-optional `T` widens to
    /// `T?` (and `U?` -> `T?` when `U` -> `T`), but a `T?` never widens to a
    /// non-optional `T` — it must be unwrapped (`??`/`?.`/`if (var …)`/`!`).
    pub fn assignable(from: &Ty, to: &Ty) -> bool {
        // No nominal subtyping by default; callers with a class/interface table use
        // [`Ty::assignable_with`] to supply one (M-RT S2).
        Ty::assignable_with(from, to, &|_, _| false)
    }

    /// Build a normalized union from `members` (M-RT S4): flatten nested unions, dedupe (preserving
    /// the equality used everywhere else), then sort into a canonical order by `Display` so member
    /// order never affects identity or assignability. A 0-member input yields `Error`; a 1-member
    /// input (after dedupe) *is* that member (so `A | A` ≡ `A`, and the checker treats that collapse
    /// as the `E-UNION-ARITY` degenerate case).
    pub fn union_of(members: Vec<Ty>) -> Ty {
        let mut flat: Vec<Ty> = Vec::new();
        for m in members {
            match m {
                Ty::Union(inner) => {
                    for i in inner {
                        if !flat.contains(&i) {
                            flat.push(i);
                        }
                    }
                }
                other => {
                    if !flat.contains(&other) {
                        flat.push(other);
                    }
                }
            }
        }
        flat.sort_by_key(std::string::ToString::to_string);
        match flat.len() {
            0 => Ty::Error,
            1 => flat.into_iter().next().expect("len checked == 1"),
            _ => Ty::Union(flat),
        }
    }

    /// Build a normalized intersection from `members` (M-RT S5) — the exact mirror of [`Ty::union_of`]:
    /// flatten nested intersections, dedupe, canonical-sort by `Display`. A 0-member input yields
    /// `Error`; a 1-member input (after dedupe) *is* that member (so `A & A` ≡ `A`, the
    /// `E-INTERSECT-ARITY` degenerate case). The shared normalizer makes `A & B` and `B & A` identical.
    pub fn intersection_of(members: Vec<Ty>) -> Ty {
        let mut flat: Vec<Ty> = Vec::new();
        for m in members {
            match m {
                Ty::Intersection(inner) => {
                    for i in inner {
                        if !flat.contains(&i) {
                            flat.push(i);
                        }
                    }
                }
                other => {
                    if !flat.contains(&other) {
                        flat.push(other);
                    }
                }
            }
        }
        flat.sort_by_key(std::string::ToString::to_string);
        match flat.len() {
            0 => Ty::Error,
            1 => flat.into_iter().next().expect("len checked == 1"),
            _ => Ty::Intersection(flat),
        }
    }

    /// Like [`Ty::assignable`] but consults a nominal-subtyping oracle for two named types:
    /// `subtype(a, b)` answers whether the type named `a` is a subtype of the type named `b`
    /// (a class implementing interface `b`, or an interface extending `b`, transitively). Threading
    /// the oracle here keeps the optional/function recursion in one chokepoint, so subtyping flows
    /// through covariant positions (`Dog -> Speaker?` works because `Dog -> Speaker` does). The
    /// checker passes a closure over its `class_implements`/interface tables; everyone else passes
    /// `|_, _| false`.
    pub fn assignable_with(from: &Ty, to: &Ty, subtype: &dyn Fn(&str, &str) -> bool) -> bool {
        if *from == Ty::Error || *to == Ty::Error {
            return true;
        }
        match (from, to) {
            // A bare `null` fits any optional (and itself); nothing else accepts it.
            (Ty::Null, Ty::Optional(_) | Ty::Null) => true,
            (Ty::Null, _) => false,
            // `U? -> T?` when `U -> T`; a non-optional `T -> T?` (covariant widening).
            (Ty::Optional(f), Ty::Optional(t)) => Ty::assignable_with(f, t, subtype),
            (other, Ty::Optional(t)) => Ty::assignable_with(other, t, subtype),
            // Union types (M-RT S4). Into a union: every member of a union `from` must fit, else a
            // non-union `from` need only fit *some* member (subset-in / member-in). Out of a union to
            // a non-union `to`: every member must fit `to` (so `A|B -> I` holds when both implement
            // interface `I`). Checked before the nominal/`_` arms so a union on either side is handled
            // by structure, not equality.
            (Ty::Union(fs), Ty::Union(_)) => fs.iter().all(|f| Ty::assignable_with(f, to, subtype)),
            (_, Ty::Union(ts)) => ts.iter().any(|t| Ty::assignable_with(from, t, subtype)),
            (Ty::Union(fs), _) => fs.iter().all(|f| Ty::assignable_with(f, to, subtype)),
            // Intersection types (M-RT S5) — the dual of the union arms, placed after them so a
            // union-on-either-side mix is already handled (a union `from` to an intersection `to` is
            // caught by `(Ty::Union(fs), _)`, recursing here per member). Into an intersection: `from`
            // must fit *every* member (all-members-required-in) — so a `Dog` flows into
            // `Drawable & Named` iff it implements both. Out of an intersection to a non-intersection
            // `to`: *some* member must fit (some-member-out) — so `A & B -> A` and `A & B -> B` hold.
            // The `(Intersection, Intersection)` case composes: `(_, Intersection(ts))` fires first
            // and recurses each `t` into `(Intersection(fs), _)`, giving `ts.all(|t| fs.any(…))`.
            (_, Ty::Intersection(ts)) => ts.iter().all(|t| Ty::assignable_with(from, t, subtype)),
            (Ty::Intersection(fs), _) => fs.iter().any(|f| Ty::assignable_with(f, to, subtype)),
            // Function types are exact-match only — no co/contra-variance (spec A6).
            (Ty::Function(fp, fr), Ty::Function(tp, tr)) => {
                fp.len() == tp.len() && fp.iter().zip(tp.iter()).all(|(a, b)| a == b) && fr == tr
            }
            // Nominal types: a subtype edge (class→interface, interface→parent interface) by name, or
            // the same head with **invariant** type arguments (matching `List`/`Map`/`Set`: `Box<int>`
            // is not a `Box<float>`). A non-generic nominal has empty args on both sides, so this
            // reduces to the name check. A `T?` never widens to a non-optional `T` — handled above.
            (Ty::Named(a, aa), Ty::Named(b, ba)) => subtype(a, b) || (a == b && aa == ba),
            // A type parameter is opaque inside its generic body: assignable only to the same
            // parameter (by name), with no coercion to/from any concrete type. Call sites do not
            // reach here — they unify the parameter away first (M-RT S7).
            (Ty::Param(a), Ty::Param(b)) => a == b,
            _ => from == to,
        }
    }
}

impl fmt::Display for Ty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Ty::Int => write!(f, "int"),
            Ty::Float => write!(f, "float"),
            Ty::Bool => write!(f, "bool"),
            Ty::String => write!(f, "string"),
            Ty::Bytes => write!(f, "bytes"),
            Ty::Html => write!(f, "Html"),
            Ty::Attr => write!(f, "Attr"),
            Ty::Unit => write!(f, "unit"),
            Ty::Named(n, args) => {
                if args.is_empty() {
                    write!(f, "{n}")
                } else {
                    let a = args
                        .iter()
                        .map(|a| a.to_string())
                        .collect::<Vec<_>>()
                        .join(", ");
                    write!(f, "{n}<{a}>")
                }
            }
            Ty::List(e) => write!(f, "List<{e}>"),
            Ty::Map(k, v) => write!(f, "Map<{k}, {v}>"),
            Ty::Set(e) => write!(f, "Set<{e}>"),
            Ty::Optional(e) => write!(f, "{e}?"),
            Ty::Null => write!(f, "null"),
            Ty::Param(n) => write!(f, "{n}"),
            Ty::Union(members) => {
                let m = members
                    .iter()
                    .map(|t| t.to_string())
                    .collect::<Vec<_>>()
                    .join(" | ");
                write!(f, "{m}")
            }
            Ty::Intersection(members) => {
                let m = members
                    .iter()
                    .map(|t| t.to_string())
                    .collect::<Vec<_>>()
                    .join(" & ");
                write!(f, "{m}")
            }
            Ty::Error => write!(f, "<error>"),
            Ty::Function(params, ret) => {
                let ps = params
                    .iter()
                    .map(|p| p.to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(f, "({ps}) -> {ret}")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assignable_is_equality_plus_error() {
        assert!(Ty::assignable(&Ty::Int, &Ty::Int));
        assert!(!Ty::assignable(&Ty::Int, &Ty::Float)); // no widening
        assert!(Ty::assignable(&Ty::Error, &Ty::Int)); // poison unifies
        assert!(Ty::assignable(&Ty::Int, &Ty::Error));
        assert!(Ty::assignable(
            &Ty::List(Box::new(Ty::Int)),
            &Ty::List(Box::new(Ty::Int))
        ));
        assert!(!Ty::assignable(
            &Ty::List(Box::new(Ty::Int)),
            &Ty::List(Box::new(Ty::Float))
        ));
    }

    #[test]
    fn html_is_not_interchangeable_with_string() {
        // The XSS-safety wall: a raw `string` cannot stand in for `Html`, and vice versa. The only
        // bridges are `core.html.text`/`raw` (string -> Html) and `core.html.render` (Html -> string).
        assert!(Ty::assignable(&Ty::Html, &Ty::Html));
        assert!(!Ty::assignable(&Ty::String, &Ty::Html)); // untrusted text can't become HTML
        assert!(!Ty::assignable(&Ty::Html, &Ty::String)); // and HTML must be explicitly rendered out
        assert_eq!(Ty::Html.to_string(), "Html");
    }

    #[test]
    fn optional_assignability() {
        let int_opt = Ty::Optional(Box::new(Ty::Int));
        assert!(Ty::assignable(&Ty::Int, &int_opt)); // T -> T? (widen)
        assert!(!Ty::assignable(&int_opt, &Ty::Int)); // T? -/-> T (must unwrap)
        assert!(Ty::assignable(&int_opt, &int_opt)); // T? -> T?
        assert!(!Ty::assignable(
            &Ty::Optional(Box::new(Ty::Int)),
            &Ty::Optional(Box::new(Ty::Float))
        ));
        assert_eq!(int_opt.to_string(), "int?"); // Display
                                                 // the bare-`null` type fits any optional and nothing else
        assert!(Ty::assignable(&Ty::Null, &int_opt)); // null -> int?
        assert!(!Ty::assignable(&Ty::Null, &Ty::Int)); // null -/-> int
        assert_eq!(Ty::Null.to_string(), "null");
    }

    #[test]
    fn display_renders_generics() {
        assert_eq!(
            Ty::List(Box::new(Ty::Named("Shape".into(), vec![]))).to_string(),
            "List<Shape>"
        );
    }

    #[test]
    fn named_type_arguments_are_invariant() {
        // A generic class instance carries its arguments (M-RT generics-all). Display shows them, and
        // assignability is invariant on the arguments (matching List/Map/Set) — `Box<int>` is not a
        // `Box<float>` — while a non-generic nominal (empty args) reduces to the plain name check.
        let box_int = Ty::Named("Box".into(), vec![Ty::Int]);
        let box_int2 = Ty::Named("Box".into(), vec![Ty::Int]);
        let box_float = Ty::Named("Box".into(), vec![Ty::Float]);
        assert_eq!(box_int.to_string(), "Box<int>");
        assert!(Ty::assignable(&box_int, &box_int2)); // same head + same args
        assert!(!Ty::assignable(&box_int, &box_float)); // invariant in the argument
        let plain = Ty::Named("Dog".into(), vec![]);
        assert!(Ty::assignable(&plain, &Ty::Named("Dog".into(), vec![])));
        assert!(!Ty::assignable(&plain, &Ty::Named("Cat".into(), vec![])));
    }

    #[test]
    fn type_param_is_opaque() {
        // A type parameter is assignable only to the same-named parameter — no coercion to/from
        // any concrete type. Call sites unify it away before it reaches `assignable` (M-RT S7).
        let t = Ty::Param("T".into());
        let t2 = Ty::Param("T".into());
        let u = Ty::Param("U".into());
        assert!(Ty::assignable(&t, &t2)); // same name
        assert!(!Ty::assignable(&t, &u)); // distinct params
        assert!(!Ty::assignable(&t, &Ty::Int)); // no concretization
        assert!(!Ty::assignable(&Ty::Int, &t));
        assert!(Ty::assignable(&t, &Ty::Error)); // poison still unifies
        assert_eq!(t.to_string(), "T");
        // A `List<T>` matches only an identical `List<T>`.
        assert!(Ty::assignable(
            &Ty::List(Box::new(Ty::Param("T".into()))),
            &Ty::List(Box::new(Ty::Param("T".into())))
        ));
    }

    #[test]
    fn union_of_normalizes() {
        // Flatten nested, dedupe, canonical-sort by Display; a 1-member collapse is that member.
        let a = Ty::Named("A".into(), vec![]);
        let b = Ty::Named("B".into(), vec![]);
        // B | A | B  →  A | B  (sorted, deduped)
        let u = Ty::union_of(vec![b.clone(), a.clone(), b.clone()]);
        assert_eq!(u.to_string(), "A | B");
        // nested unions flatten: (A | B) | C  →  A | B | C
        let c = Ty::Named("C".into(), vec![]);
        let nested = Ty::union_of(vec![u.clone(), c]);
        assert_eq!(nested.to_string(), "A | B | C");
        // collapse: A | A  ≡  A (not a Union)
        assert_eq!(Ty::union_of(vec![a.clone(), a.clone()]), a);
        // order-independence: A | B == B | A
        assert_eq!(
            Ty::union_of(vec![a.clone(), b.clone()]),
            Ty::union_of(vec![b, a])
        );
    }

    #[test]
    fn union_assignability_member_in_and_all_out() {
        let a = Ty::Named("A".into(), vec![]);
        let b = Ty::Named("B".into(), vec![]);
        let c = Ty::Named("C".into(), vec![]);
        let ab = Ty::union_of(vec![a.clone(), b.clone()]);
        // member-in: a non-union fits the union iff it equals some member.
        assert!(Ty::assignable(&a, &ab));
        assert!(Ty::assignable(&b, &ab));
        assert!(!Ty::assignable(&c, &ab)); // C is not A or B
                                           // subset-in: a union fits a wider union.
        let abc = Ty::union_of(vec![a.clone(), b.clone(), c.clone()]);
        assert!(Ty::assignable(&ab, &abc)); // {A,B} ⊆ {A,B,C}
        assert!(!Ty::assignable(&abc, &ab)); // {A,B,C} ⊄ {A,B}
                                             // all-members-out: A|B fits a non-union only if every member does — here only via the oracle.
        let speaker = Ty::Named("Speaker".into(), vec![]);
        let oracle = |x: &str, t: &str| t == "Speaker" && (x == "A" || x == "B");
        assert!(Ty::assignable_with(&ab, &speaker, &oracle)); // both implement Speaker
        assert!(!Ty::assignable_with(&ab, &speaker, &|_, _| false)); // without the edge, no
    }

    #[test]
    fn intersection_of_normalizes() {
        // Flatten nested, dedupe, canonical-sort by Display; a 1-member collapse is that member.
        let a = Ty::Named("A".into(), vec![]);
        let b = Ty::Named("B".into(), vec![]);
        let c = Ty::Named("C".into(), vec![]);
        // B & A & B  →  A & B  (sorted, deduped)
        let i = Ty::intersection_of(vec![b.clone(), a.clone(), b.clone()]);
        assert_eq!(i.to_string(), "A & B");
        // nested intersections flatten: (A & B) & C  →  A & B & C
        let nested = Ty::intersection_of(vec![i.clone(), c]);
        assert_eq!(nested.to_string(), "A & B & C");
        // collapse: A & A  ≡  A (not an Intersection)
        assert_eq!(Ty::intersection_of(vec![a.clone(), a.clone()]), a);
        // order-independence: A & B == B & A
        assert_eq!(
            Ty::intersection_of(vec![a.clone(), b.clone()]),
            Ty::intersection_of(vec![b, a])
        );
    }

    #[test]
    fn intersection_assignability_all_in_some_out() {
        // The dual of the union rules. `Dog` flows into `Drawable & Named` iff it (via the oracle)
        // is a subtype of *both*; out of `A & B`, *some* member must fit the target.
        let drawable = Ty::Named("Drawable".into(), vec![]);
        let named = Ty::Named("Named".into(), vec![]);
        let dn = Ty::intersection_of(vec![drawable.clone(), named.clone()]);
        // all-members-required-in: a class implementing both is assignable; one is not.
        let both = |x: &str, t: &str| x == "Dog" && (t == "Drawable" || t == "Named");
        let one = |x: &str, t: &str| x == "Cat" && t == "Drawable"; // implements only Drawable
        let dog = Ty::Named("Dog".into(), vec![]);
        let cat = Ty::Named("Cat".into(), vec![]);
        assert!(Ty::assignable_with(&dog, &dn, &both)); // Dog: Drawable & Named ✓
        assert!(!Ty::assignable_with(&cat, &dn, &one)); // Cat: only Drawable ✗
                                                        // some-member-out: A & B fits A and fits B.
        assert!(Ty::assignable(&dn, &drawable)); // A & B -> A
        assert!(Ty::assignable(&dn, &named)); // A & B -> B
        let other = Ty::Named("Other".into(), vec![]);
        assert!(!Ty::assignable(&dn, &other)); // A & B -/-> Other
                                               // intersection ⊆ intersection: A & B & C -> A & B (every target member met by some source).
        let c = Ty::Named("C".into(), vec![]);
        let dnc = Ty::intersection_of(vec![drawable, named, c]);
        assert!(Ty::assignable(&dnc, &dn)); // {A,B,C} satisfies {A,B}
        assert!(!Ty::assignable(&dn, &dnc)); // {A,B} cannot satisfy C
    }

    #[test]
    fn union_intersection_cross_assignability() {
        // A union `from` flows into an intersection `to` iff EVERY union member fits the intersection,
        // i.e. every member satisfies every intersection member. Here both A and B implement I and J.
        let a = Ty::Named("A".into(), vec![]);
        let b = Ty::Named("B".into(), vec![]);
        let ab = Ty::union_of(vec![a.clone(), b.clone()]);
        let i = Ty::Named("I".into(), vec![]);
        let j = Ty::Named("J".into(), vec![]);
        let ij = Ty::intersection_of(vec![i.clone(), j.clone()]);
        let both_impl = |x: &str, t: &str| (x == "A" || x == "B") && (t == "I" || t == "J");
        assert!(Ty::assignable_with(&ab, &ij, &both_impl)); // (A|B) -> (I&J)
                                                            // If only A implements both, the union no longer fits (B fails).
        let only_a = |x: &str, t: &str| x == "A" && (t == "I" || t == "J");
        assert!(!Ty::assignable_with(&ab, &ij, &only_a));
    }

    #[test]
    fn function_type_assignability_is_exact() {
        let int_to_int = Ty::Function(vec![Ty::Int], Box::new(Ty::Int));
        let int_to_int2 = Ty::Function(vec![Ty::Int], Box::new(Ty::Int));
        let int_to_float = Ty::Function(vec![Ty::Int], Box::new(Ty::Float));
        assert!(Ty::assignable(&int_to_int, &int_to_int2));
        assert!(!Ty::assignable(&int_to_int, &int_to_float)); // no variance (A6)
        assert!(!Ty::assignable(&Ty::Int, &int_to_int)); // int is not a function
        assert_eq!(format!("{int_to_int}"), "(int) -> int");
    }
}
