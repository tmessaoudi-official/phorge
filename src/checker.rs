//! Sound static type-checker. Two sub-phases: `collect` (hoist decls + prelude),
//! then `check` (walk bodies). Returns all type errors at once.

use std::collections::HashMap;

use crate::ast::Program;
use crate::diagnostic::{Diagnostic, Stage};
use crate::limits::MAX_EXPR_DEPTH;
use crate::token::Span;
use crate::types::Ty;

#[derive(Clone)]
struct FnSig {
    params: Vec<Ty>,
    ret: Ty,
    /// Generic type parameters this function declares (`["T"]` for `function id<T>(T x) -> T`).
    /// Empty for a non-generic function — the common case. When non-empty, `params`/`ret` contain
    /// `Ty::Param` occurrences that a call site unifies away (M-RT S7). Free functions AND class
    /// methods may be generic (M-RT generics-all); interface method signatures stay non-generic
    /// (the parser builds them with empty `type_params`), so theirs is always empty.
    type_params: Vec<String>,
    /// Checked exception types this function declares (`throws A | B` ⇒ `[A, B]`), resolved at
    /// collection time. Empty for the common no-throws case. A call site must *discharge* each
    /// member — catch it in an enclosing `try`, or propagate it with `?` and a matching enclosing
    /// `throws` (M-faults 2b). Free functions and class methods carry their declared set; interface
    /// method signatures keep it empty (dynamic-dispatch throws enforcement is a documented deferral).
    throws: Vec<Ty>,
}

struct EnumInfo {
    /// variant name -> field types (in declaration order)
    variants: HashMap<String, Vec<Ty>>,
    /// Generic type parameters this enum declares (`["T"]` for `enum Option<T>`). Empty for a
    /// non-generic enum. When non-empty, `variants`' field types may contain `Ty::Param` occurrences:
    /// construction unifies a variant's fields against the call arguments to bind them, and a `match`
    /// substitutes them with the scrutinee's type arguments (M-RT generic enums).
    type_params: Vec<String>,
}

#[derive(Clone)]
struct ClassInfo {
    fields: HashMap<String, Ty>,
    /// Names of the `mutable` fields (M-mut.6) — explicit `mutable Type f;` decls and promoted ctor
    /// params carrying `mutable`. Only these may be the target of `o.f = e` (`E-ASSIGN-IMMUTABLE`);
    /// every other field is immutable by default. A subset of `fields`' keys.
    mutable_fields: std::collections::HashSet<String>,
    /// `static` field name → type (M-mut.7). Class-level, accessed as `ClassName.field` — disjoint
    /// from `fields` (statics are never instance members). Each has a literal-const initializer.
    statics: HashMap<String, Ty>,
    /// The subset of `statics` declared `static mutable` — only these may be the target of
    /// `ClassName.field = e` (`E-ASSIGN-IMMUTABLE`).
    static_mut: std::collections::HashSet<String>,
    /// Method overload sets (M-RT overloading): a name maps to one *or more* signatures (dynamic
    /// multiple dispatch). Length 1 in the common case; >1 when methods share a name with distinct
    /// parameter signatures (all sharing a return type — `E-OVERLOAD-RETURN`).
    methods: HashMap<String, Vec<FnSig>>,
    /// Property hooks (M-mut.7b) — virtual members keyed by name. Disjoint from `fields`/`statics`
    /// (a hook has no storage). The flags record whether the hook is readable (`has_get`) and/or
    /// writable (`has_set`): reading a `!has_get` hook is `E-HOOK-NO-GET`, writing a `!has_set` one
    /// is `E-HOOK-NO-SET`. A member read/write resolves a hook here before the instance-field path.
    hooks: HashMap<String, HookInfo>,
    /// constructor parameter types, for `ClassName(args)` calls. For a class with no own constructor
    /// under single inheritance (M-RT S6c.2a), this is the *inherited* parent constructor's signature.
    ctor: Vec<Ty>,
    /// Whether the class declares its **own** constructor (vs. inheriting one). Distinguishes a class
    /// with a zero-arg ctor from one with no ctor at all (both leave `ctor` empty) — `merge_inherited`
    /// inherits a single parent's `ctor` only into a class that has none of its own (M-RT S6c.2a).
    has_ctor: bool,
    /// Generic type parameters this class declares (`["T"]` for `class Box<T>`). Empty for a
    /// non-generic class. When non-empty, `fields`/`ctor`/`methods` may contain `Ty::Param`
    /// occurrences: construction unifies the ctor against the arguments to bind them, and member
    /// access substitutes them with the instance's type arguments (M-RT generics-all).
    type_params: Vec<String>,
    /// `abstract class` (M-RT S6b) — not instantiable (`E-ABSTRACT-INSTANTIATE`); may carry abstract
    /// (bodyless) methods a concrete subclass must implement.
    is_abstract: bool,
}

/// A property hook's declared type and which accessors it provides (M-mut.7b).
#[derive(Clone)]
struct HookInfo {
    ty: Ty,
    has_get: bool,
    has_set: bool,
}

/// An interface's own method signatures plus its declared parent interfaces (`extends`). The
/// flattened method set (own + every parent's) is computed on demand, cycle-guarded (M-RT S2).
struct InterfaceInfo {
    methods: HashMap<String, FnSig>,
    extends: Vec<String>,
}

pub struct Checker {
    /// Free-function overload sets (M-RT overloading): a name maps to one *or more* signatures
    /// (dynamic multiple dispatch). Length 1 in the common case.
    funcs: HashMap<String, Vec<FnSig>>,
    enums: HashMap<String, EnumInfo>,
    classes: HashMap<String, ClassInfo>,
    interfaces: HashMap<String, InterfaceInfo>,
    /// Trait names (M-RT S8). A trait's members are collected into [`Self::classes`] under its name
    /// (so member lookup while checking the trait body and merging into a using class reuse the class
    /// machinery), but a trait is **not a type**: this set lets `resolve_type`/`instanceof`/construction
    /// reject a trait name where a type is expected.
    traits: std::collections::HashSet<String>,
    /// Transitively-flattened interface set each class implements (the `instanceof`/subtyping table),
    /// computed once via [`crate::ast::class_implements`] and shared verbatim with the backends so
    /// the runtime test can never diverge from the static one (M-RT S2).
    class_implements: std::collections::BTreeMap<String, Vec<String>>,
    /// Transitively-flattened ancestor-class set for every class (M-RT S6), computed once via
    /// [`crate::ast::class_supertypes`]. Drives nominal subtyping (`Dog <: Animal`) alongside
    /// `class_implements`; a class appearing in its own set marks an `extends` cycle (`E-MI-CYCLE`).
    class_supertypes: std::collections::BTreeMap<String, Vec<String>>,
    /// lexical block scopes; last is innermost. Each binding carries its type and whether it is
    /// `mutable` (reassignable) — immutable by default (M-mut.1); only a `mutable` binding may be
    /// the target of `Stmt::Assign`.
    scopes: Vec<HashMap<String, (Ty, bool)>>,
    errors: Vec<Diagnostic>,
    /// Non-fatal lints (e.g. `W-FORCE-UNWRAP`). Surfaced to stderr by the CLI but never fail the
    /// build — the first member of Phorge's warning channel (M3 S2.5).
    warnings: Vec<Diagnostic>,
    /// return type of the function/method currently being checked
    cur_ret: Ty,
    /// Checked-exception set the function/method currently being checked is declared to `throws`
    /// (M-faults 2b). A `throw e` or a `?`-propagated throwing call discharges against this set;
    /// saved/restored exactly like [`Self::cur_ret`] at every body-checking site. Empty inside a
    /// constructor, property hook, or lambda (none may declare `throws`).
    cur_throws: Vec<Ty>,
    /// Whether the function currently being checked is the program entry `main` (M-faults 2b).
    /// `main` may not declare `throws`, and an undischarged throw reaching it is `E-UNCAUGHT-THROW`
    /// (rather than the generic `E-THROW-UNDECLARED`).
    cur_is_main: bool,
    /// Stack of the active enclosing `try` blocks' catch-type sets — innermost last — while checking
    /// a `try` *body* (popped before its own catch/finally are checked, since a throw there is not
    /// caught by the same `try`). A thrown type is *caught* iff it is `<:` some type in any frame.
    /// Cleared around a lambda body (a closure passed to a native does not see the lexical `try`).
    try_catch_stack: Vec<Vec<Ty>>,
    /// One-shot flag set by a throws-mode `?` so the immediately-wrapped throwing call skips its own
    /// call-site discharge check (the `?` propagates instead). Consumed (taken) by the call.
    skip_throws_discharge: bool,
    /// class currently being checked (for `this` and bare field refs)
    cur_class: Option<String>,
    /// live `check_expr` recursion depth, bounded by [`MAX_EXPR_DEPTH`]
    depth: usize,
    /// number of enclosing loops being checked (M-mut.3). `break`/`continue` are valid only when
    /// this is `> 0` (`E-BREAK-OUTSIDE-LOOP`/`E-CONTINUE-OUTSIDE-LOOP`).
    loop_depth: u32,
    /// `type Name = Type;` aliases, stored as raw AST types and expanded in `resolve_type`.
    aliases: HashMap<String, crate::ast::Type>,
    /// alias names currently being expanded — detects `type A = B; type B = A;` cycles.
    alias_stack: Vec<String>,
    /// Active import map (leaf qualifier → full dotted module path; see [`crate::native::import_map`]).
    /// Drives namespaced native-call resolution (`console.println`) and the shadowing guard that
    /// keeps an imported qualifier disjoint from every value binding (M3 Wave 1).
    imports: HashMap<String, String>,
    /// Span-keyed node substitutions applied by [`resolve_html`] after a successful check, so the
    /// backend-facing AST is free of front-end-only nodes. Keyed by `Span.start` (byte offset —
    /// unique per source occurrence in a single file). Two producers share it:
    /// (1) type-directed `html"…"` desugarings — each entry is the `html.concat([…])` replacement
    ///     for an [`crate::ast::Expr::Html`] literal (core.html Wave 3);
    /// (2) throws-mode `?` erasure — a throwing call's `?` is a checker-only propagation marker, so
    ///     its [`crate::ast::Expr::Propagate`] node is replaced by its inner call expression (the
    ///     call's own throw already unwinds; M-faults 2b). Result-mode `?` is *not* recorded here —
    ///     it carries real lowering and is left for the backends.
    html_resolutions: HashMap<usize, crate::ast::Expr>,
    /// Type parameters in scope while resolving the signature/body of the generic function currently
    /// being checked (`["T", "U"]`). A bare type name in this set resolves to `Ty::Param` rather than
    /// being looked up as an alias/enum/class. Set around each generic function and cleared after;
    /// empty for everything else (M-RT S7).
    active_type_params: Vec<String>,
    /// Type parameters of the generic *class* whose body is currently being checked (`["T"]` for a
    /// method/constructor inside `class Box<T>`). Unioned with the method's own `type_params` so a
    /// method body sees both. Empty for free functions and non-generic classes (M-RT generics-all).
    cur_class_type_params: Vec<String>,
}

impl Checker {
    fn new() -> Self {
        // Built-in `Error` marker interface (M-faults 2b): a thrown type is `class X implements
        // Error`. It declares **no** methods (a pure marker) — the `message` field is conventional and
        // special-cased only by the transpiler (`extends \Exception` + `parent::__construct`). The name
        // is reserved (see `is_builtin_type_name`), so user code cannot redefine it. Class `extends`
        // (inheritance) is a future slice (S6), so an interface is the only available shape today.
        let mut interfaces = HashMap::new();
        interfaces.insert(
            "Error".to_string(),
            InterfaceInfo {
                methods: HashMap::new(),
                extends: Vec::new(),
            },
        );
        Checker {
            funcs: HashMap::new(),
            enums: HashMap::new(),
            classes: HashMap::new(),
            interfaces,
            traits: std::collections::HashSet::new(),
            class_implements: std::collections::BTreeMap::new(),
            class_supertypes: std::collections::BTreeMap::new(),
            scopes: Vec::new(),
            errors: Vec::new(),
            warnings: Vec::new(),
            cur_ret: Ty::Unit,
            cur_throws: Vec::new(),
            cur_is_main: false,
            try_catch_stack: Vec::new(),
            skip_throws_discharge: false,
            cur_class: None,
            depth: 0,
            loop_depth: 0,
            aliases: HashMap::new(),
            alias_stack: Vec::new(),
            imports: HashMap::new(),
            html_resolutions: HashMap::new(),
            active_type_params: Vec::new(),
            cur_class_type_params: Vec::new(),
        }
    }

    /// Record an error and return the poison type so callers can keep going.
    fn err(&mut self, span: Span, msg: impl Into<String>) -> Ty {
        self.errors
            .push(Diagnostic::new(Stage::Type, msg, span.line, span.col));
        Ty::Error
    }

    /// Like [`Self::err`] but attaches a stable diagnostic `code` (for `phg explain`) and an
    /// optional hint.
    fn err_coded(
        &mut self,
        span: Span,
        msg: impl Into<String>,
        code: &'static str,
        hint: Option<String>,
    ) -> Ty {
        let mut d = Diagnostic::new(Stage::Type, msg, span.line, span.col).with_code(code);
        d.hint = hint;
        self.errors.push(d);
        Ty::Error
    }

    /// Record a non-fatal lint (the warning channel — M3 S2.5). Unlike [`err_coded`] this does not
    /// poison a type; it is collected separately and surfaced to stderr without failing the build.
    fn warn_coded(
        &mut self,
        span: Span,
        msg: impl Into<String>,
        code: &'static str,
        hint: Option<String>,
    ) {
        let mut d = Diagnostic::new(Stage::Type, msg, span.line, span.col).with_code(code);
        d.hint = hint;
        self.warnings.push(d);
    }

    /// Assignment-failure diagnostic. Recognizes the optional-misuse case (a `T?` used where a
    /// non-optional `T` is required) and attaches `E-OPT-ASSIGN` + an unwrap hint; otherwise the
    /// generic type-mismatch message.
    fn err_assign(&mut self, span: Span, actual: &Ty, declared: &Ty) {
        let optional_misuse =
            matches!(actual, Ty::Optional(_) | Ty::Null) && !matches!(declared, Ty::Optional(_));
        if optional_misuse {
            self.err_coded(
                span,
                format!("cannot use `{actual}` where non-optional `{declared}` is required"),
                "E-OPT-ASSIGN",
                Some("unwrap it first with `??`, `?.`, `if (var …)`, or `!`".into()),
            );
        } else {
            self.err(span, format!("expected `{declared}`, found `{actual}`"));
        }
    }

    /// Every name currently visible — block-scope locals + top-level functions + (inside a method)
    /// the current class's fields — used to suggest the nearest match on an unknown identifier.
    fn in_scope_names(&self) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        for scope in &self.scopes {
            names.extend(scope.keys().cloned());
        }
        names.extend(self.funcs.keys().cloned());
        if let Some(cls) = &self.cur_class {
            if let Some(info) = self.classes.get(cls) {
                names.extend(info.fields.keys().cloned());
            }
        }
        names
    }

    /// The closest candidate to `name` within a small edit distance (≤ 2), if any — the
    /// "did you mean `…`?" suggestion.
    fn nearest_name(&self, name: &str, candidates: &[String]) -> Option<String> {
        candidates
            .iter()
            .map(|c| (levenshtein(name, c), c))
            .filter(|(d, _)| *d > 0 && *d <= 2)
            .min_by_key(|(d, _)| *d)
            .map(|(_, c)| c.clone())
    }

    /// Resolve an AST type annotation to an internal `Ty`. Records and poisons on
    /// unknown / deferred types.
    fn resolve_type(&mut self, ty: &crate::ast::Type) -> Ty {
        use crate::ast::Type;
        match ty {
            Type::Optional { inner, .. } => Ty::Optional(Box::new(self.resolve_type(inner))),
            Type::Union(members, span) => {
                // M-RT S4: resolve each member, validate its kind (classes/interfaces/primitives
                // only — enums/optionals/functions are rejected so the PHP `A|B` emission and the
                // instanceof-based match stay sound), then normalize. A degenerate union that
                // collapses to one member after dedupe is `E-UNION-ARITY`.
                let resolved: Vec<Ty> = members.iter().map(|m| self.resolve_type(m)).collect();
                for ty in &resolved {
                    let ok = match ty {
                        Ty::Int
                        | Ty::Float
                        | Ty::Bool
                        | Ty::String
                        | Ty::Bytes
                        | Ty::Html
                        | Ty::Attr
                        | Ty::Error => true,
                        Ty::Named(n, _) => {
                            self.classes.contains_key(n) || self.interfaces.contains_key(n)
                        }
                        _ => false,
                    };
                    if !ok {
                        self.err_coded(
                            *span,
                            format!(
                                "union member `{ty}` is not allowed — members must be classes, interfaces, or primitives"
                            ),
                            "E-UNION-MEMBER",
                            Some(
                                "enum, optional `T?`, and function members are not supported in a union this slice".into(),
                            ),
                        );
                    }
                }
                let norm = Ty::union_of(resolved);
                if !matches!(norm, Ty::Union(_) | Ty::Error) {
                    // ≥2 source members collapsed to one (`A | A`): a union needs ≥2 distinct types.
                    self.err_coded(
                        *span,
                        "a union needs two or more distinct types".to_string(),
                        "E-UNION-ARITY",
                        None,
                    );
                }
                norm
            }
            Type::Intersection(members, span) => {
                // M-RT S5: resolve each member, validate kinds (D1: interfaces, plus *at most one*
                // concrete class — a value has exactly one class, so two distinct classes are the
                // bottom type), then enforce shared-method signature agreement (D2: no overloading
                // yet, so two members whose shared method differs is uninhabited) and normalize.
                let resolved: Vec<Ty> = members.iter().map(|m| self.resolve_type(m)).collect();
                let mut class_count = 0;
                for ty in &resolved {
                    match ty {
                        Ty::Error => {}
                        Ty::Named(n, _) if self.interfaces.contains_key(n) => {}
                        Ty::Named(n, _) if self.classes.contains_key(n) => class_count += 1,
                        _ => {
                            self.err_coded(
                                *span,
                                format!(
                                    "intersection member `{ty}` is not allowed — members must be interfaces, with at most one concrete class"
                                ),
                                "E-INTERSECT-MEMBER",
                                Some("primitives, enums, optionals, and function types cannot be intersection members".into()),
                            );
                        }
                    }
                }
                if class_count >= 2 {
                    self.err_coded(
                        *span,
                        "an intersection may name at most one concrete class — no value can be two distinct classes at once".to_string(),
                        "E-INTERSECT-MULTI-CLASS",
                        Some("compose with interfaces instead; a second class becomes possible only when class `extends` lands (S6)".into()),
                    );
                }
                // D2: a method declared by two members with differing signatures can be satisfied by no
                // class (Phorge has no overloading — a class has exactly one `foo`), so the intersection
                // is uninhabited. Reject it here, where it is honest about *why*.
                let mut method_sigs: HashMap<String, (Vec<Ty>, Ty)> = HashMap::new();
                let mut sig_conflict: Option<String> = None;
                for ty in &resolved {
                    if let Ty::Named(n, _) = ty {
                        let methods: Vec<(String, (Vec<Ty>, Ty))> =
                            if self.interfaces.contains_key(n) {
                                self.iface_flat_methods(n)
                            } else if let Some(info) = self.classes.get(n) {
                                // Overloaded methods (M-RT): the intersection signature-agreement check
                                // uses the first overload as the representative — a full overload-aware
                                // intersection check is a documented follow-up.
                                info.methods
                                    .iter()
                                    .filter_map(|(m, s)| {
                                        s.first().map(|s0| {
                                            (m.clone(), (s0.params.clone(), s0.ret.clone()))
                                        })
                                    })
                                    .collect()
                            } else {
                                Vec::new()
                            };
                        for (m, sig) in methods {
                            match method_sigs.get(&m) {
                                Some(existing) if *existing != sig && sig_conflict.is_none() => {
                                    sig_conflict = Some(m.clone());
                                }
                                Some(_) => {}
                                None => {
                                    method_sigs.insert(m, sig);
                                }
                            }
                        }
                    }
                }
                if let Some(m) = sig_conflict {
                    self.err_coded(
                        *span,
                        format!(
                            "intersection members declare method `{m}` with conflicting signatures — no class could implement both"
                        ),
                        "E-INTERSECT-SIG",
                        Some("a method shared across intersection members must have identical parameter and return types (Phorge has no overloading)".into()),
                    );
                }
                let norm = Ty::intersection_of(resolved);
                if !matches!(norm, Ty::Intersection(_) | Ty::Error) {
                    // ≥2 source members collapsed to one (`A & A`): an intersection needs ≥2 distinct.
                    self.err_coded(
                        *span,
                        "an intersection needs two or more distinct types".to_string(),
                        "E-INTERSECT-ARITY",
                        None,
                    );
                }
                norm
            }
            Type::Function { params, ret, .. } => Ty::Function(
                params.iter().map(|p| self.resolve_type(p)).collect(),
                Box::new(self.resolve_type(ret)),
            ),
            // `var` is intercepted in `check_stmt`; reaching here means it was written somewhere it
            // is not allowed (a parameter, field, or return type).
            Type::Infer(span) => self.err(
                *span,
                "`var` type inference is only valid for a local variable declaration",
            ),
            // Defensive: `Type::Erased` is produced by `erase_generics` *after* a successful check,
            // so a normal pipeline never resolves it. Treat it as poison so a stray re-check of an
            // already-erased program can't cascade (M-RT S7).
            Type::Erased(_) => Ty::Error,
            Type::Named { name, args, span } => match name.as_str() {
                "int" => self.no_args(name, args, *span, Ty::Int),
                "float" => self.no_args(name, args, *span, Ty::Float),
                "bool" => self.no_args(name, args, *span, Ty::Bool),
                "string" => self.no_args(name, args, *span, Ty::String),
                "bytes" => self.no_args(name, args, *span, Ty::Bytes),
                // The bottom type (M-RT totality cluster): a `-> never` function never returns. Only
                // meaningful in return position, but resolvable anywhere a type name appears.
                "never" => self.no_args(name, args, *span, Ty::Never),
                "Html" => self.no_args(name, args, *span, Ty::Html),
                "Attr" => self.no_args(name, args, *span, Ty::Attr),
                "List" => Ty::List(Box::new(self.one_arg(name, args, *span))),
                "Set" => Ty::Set(Box::new(self.one_arg(name, args, *span))),
                "Map" => {
                    if args.len() != 2 {
                        return self.err(
                            *span,
                            format!("Map expects 2 type arguments, got {}", args.len()),
                        );
                    }
                    let k = self.resolve_type(&args[0]);
                    let v = self.resolve_type(&args[1]);
                    Ty::Map(Box::new(k), Box::new(v))
                }
                "decimal" | "double" | "i8" | "i16" | "i32" | "i64" | "u8" | "u16" | "u32"
                | "u64" => self.err(
                    *span,
                    format!("the numeric type `{name}` is not yet supported in M1"),
                ),
                other => {
                    if self.active_type_params.iter().any(|p| p == other) {
                        // A generic type parameter in scope (`T` in `function id<T>(T x)`) is an
                        // opaque `Ty::Param`, unified away at call sites and erased before backends.
                        // A type arg on it (`T<int>`) is meaningless — reject it.
                        if args.is_empty() {
                            Ty::Param(other.to_string())
                        } else {
                            self.err(
                                *span,
                                format!("type parameter `{other}` takes no type arguments"),
                            )
                        }
                    } else if self.aliases.contains_key(other) {
                        if self.alias_stack.iter().any(|n| n == other) {
                            return self.err(*span, format!("type alias cycle through `{other}`"));
                        }
                        let aliased = self.aliases.get(other).cloned().expect("alias present");
                        self.alias_stack.push(other.to_string());
                        let ty = self.resolve_type(&aliased);
                        self.alias_stack.pop();
                        ty
                    } else if self.interfaces.contains_key(other) {
                        // Interfaces take no type arguments this slice (generic interfaces deferred —
                        // M-RT generics-all).
                        self.no_args(other, args, *span, Ty::Named(other.to_string(), Vec::new()))
                    } else if self.traits.contains(other) {
                        // M-RT S8: a trait is collected into `classes` for member lookup, but it is
                        // **not a type** — a value can never be typed as a trait. Reject it here before
                        // the class branch would accept it.
                        self.err_coded(
                            *span,
                            format!("`{other}` is a trait, not a type"),
                            "E-USE-AS-TYPE",
                            Some(
                                "a trait is reuse, not a type — `use` it in a class; you cannot type a value as a trait"
                                    .into(),
                            ),
                        )
                    } else if let Some(arity) = self
                        .classes
                        .get(other)
                        .map(|c| c.type_params.len())
                        .or_else(|| self.enums.get(other).map(|e| e.type_params.len()))
                    {
                        // A class or enum. A generic one requires exactly its declared number of type
                        // arguments (`Box<int>`, `Option<int>`); a non-generic one takes none (M-RT
                        // generics-all / generic enums).
                        if args.len() != arity {
                            let plural = if arity == 1 { "" } else { "s" };
                            self.err(
                                *span,
                                format!(
                                    "type `{other}` expects {arity} type argument{plural}, got {}",
                                    args.len()
                                ),
                            )
                        } else {
                            let resolved = args.iter().map(|a| self.resolve_type(a)).collect();
                            Ty::Named(other.to_string(), resolved)
                        }
                    } else {
                        self.err_coded(
                            *span,
                            format!("unknown type `{other}`"),
                            "E-UNKNOWN-TYPE",
                            None,
                        )
                    }
                }
            },
        }
    }

    fn no_args(&mut self, name: &str, args: &[crate::ast::Type], span: Span, ty: Ty) -> Ty {
        if args.is_empty() {
            ty
        } else {
            self.err(span, format!("type `{name}` takes no type arguments"))
        }
    }

    fn one_arg(&mut self, name: &str, args: &[crate::ast::Type], span: Span) -> Ty {
        if args.len() != 1 {
            self.err(
                span,
                format!("{name} expects 1 type argument, got {}", args.len()),
            );
            Ty::Error
        } else {
            self.resolve_type(&args[0])
        }
    }

    /// Phase 1 — hoist all top-level declarations and the active import map. There is no longer a
    /// builtin prelude: every callable is namespaced ("nothing in the wind"), so even `println` must
    /// be reached as `console.println` after `import core.console;` (M3 Wave 1). A bare `println(…)`
    /// now resolves as an unknown function.
    fn collect(&mut self, program: &Program) {
        use crate::ast::Item;
        self.imports = crate::native::import_map(&program.items);
        for item in &program.items {
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
    fn collect_trait(&mut self, t: &crate::ast::TraitDecl) {
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

    fn collect_interface(&mut self, i: &crate::ast::InterfaceDecl) {
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
            let ret = match &m.ret {
                Some(t) => self.resolve_type(t),
                None => Ty::Unit,
            };
            methods.insert(
                m.name.clone(),
                FnSig {
                    params,
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
    fn check_interface_graph(&mut self, program: &crate::ast::Program) {
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
    fn inherit_class_members(&mut self, program: &crate::ast::Program) {
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
    fn merge_inherited(
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
            for (k, v) in &parent_info.methods {
                child.methods.entry(k.clone()).or_insert_with(|| v.clone());
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
            }
        }
    }

    /// True if `name`'s `extends` chain reaches `name` again (a cycle). Visited-guarded.
    fn iface_in_cycle(&self, name: &str, stack: &mut std::collections::BTreeSet<String>) -> bool {
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
    fn iface_flat_methods(&self, name: &str) -> Vec<(String, (Vec<Ty>, Ty))> {
        let mut acc: HashMap<String, (Vec<Ty>, Ty)> = HashMap::new();
        let mut seen = std::collections::BTreeSet::new();
        self.iface_collect_methods(name, &mut acc, &mut seen);
        let mut out: Vec<_> = acc.into_iter().collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    fn iface_collect_methods(
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
    fn sig_conforms(&self, have: &[FnSig], want: &(Vec<Ty>, Ty)) -> bool {
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
    fn is_subtype(&self, a: &str, b: &str) -> bool {
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

    fn iface_in_cycle_to(
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
    fn ty_assignable(&self, from: &Ty, to: &Ty) -> bool {
        Ty::assignable_with(from, to, &|a, b| self.is_subtype(a, b))
    }

    // ---- M-faults 2b: checked-exception discharge ----

    /// Flatten any union member of a resolved `throws` list into its individual exception types, so
    /// `throws A | B` becomes the set `{A, B}` — discharge and validation operate per type, and a
    /// call/`?` that propagates a union must discharge every member (M-faults 2b).
    fn flatten_throws(tys: Vec<Ty>) -> Vec<Ty> {
        let mut out = Vec::new();
        for t in tys {
            match t {
                Ty::Union(members) => out.extend(members),
                other => out.push(other),
            }
        }
        out
    }

    /// Whether `t` is a thrown/declared/caught exception type — a class or interface that is `<:` the
    /// built-in `Error` marker (the `Error` interface itself qualifies). Anything else (a primitive,
    /// an enum, a function, a poison `<error>`) is not throwable/catchable (`E-THROW-TYPE`/
    /// `E-CATCH-TYPE`). Poison (`Ty::Error`) returns `true` so a prior error doesn't cascade.
    fn is_error_type(&self, t: &Ty) -> bool {
        match t {
            Ty::Error => true,
            Ty::Named(n, _) => self.is_subtype(n, "Error"),
            _ => false,
        }
    }

    /// Whether a thrown `e` is *declared* by the enclosing function — `<:` some member of
    /// [`Self::cur_throws`]. The propagate (`?`) path and a bare `throw e` both discharge this way.
    fn throws_declared(&self, e: &Ty) -> bool {
        self.cur_throws.iter().any(|d| self.ty_assignable(e, d))
    }

    /// Whether a thrown `e` is *caught* by an enclosing `try` we are currently inside the body of —
    /// `<:` some catch-clause type in any active [`Self::try_catch_stack`] frame (innermost or outer).
    fn covered_by_try(&self, e: &Ty) -> bool {
        self.try_catch_stack
            .iter()
            .any(|frame| frame.iter().any(|c| self.ty_assignable(e, c)))
    }

    /// If `inner` is a call to a free function declaring a non-empty `throws` set, return that set
    /// (owned). The one operand shape a throws-mode `?` recognises this slice — `f()?` where `f`
    /// declares `throws E`. Method/native/closure throwing calls are a documented deferral (their
    /// throws are still discharged at the call site, just not propagable with `?`).
    fn free_call_throws(&self, inner: &crate::ast::Expr) -> Option<Vec<Ty>> {
        if let crate::ast::Expr::Call { callee, .. } = inner {
            if let crate::ast::Expr::Ident(name, _) = &**callee {
                // Throws-mode `?` on an overloaded function uses the first overload's throws (the
                // common case is a single overload); a fully overload-aware `?`-throws is a deferral.
                if let Some(sig) = self.funcs.get(name).and_then(|v| v.first()) {
                    if !sig.throws.is_empty() {
                        return Some(sig.throws.clone());
                    }
                }
            }
        }
        None
    }

    /// Throws-mode `?` (M-faults 2b): `f()?` where `f` declares `throws E`. Checks the inner call
    /// (suppressing its own call-site discharge), requires every thrown type to be *declared* by the
    /// enclosing `throws` (propagation — not a `try`), records the node for erasure, and returns the
    /// call's *normal* return type. Returns `None` when `inner` is not a free throwing call, so the
    /// caller falls back to Result-mode (`check_propagate`) or `E-PROPAGATE-POSITION`.
    fn try_throws_propagate(&mut self, inner: &crate::ast::Expr, span: Span) -> Option<Ty> {
        let throws = self.free_call_throws(inner)?;
        // The wrapped call propagates instead of discharging locally — suppress its own check.
        self.skip_throws_discharge = true;
        let t = self.check_expr(inner);
        for e in &throws {
            if !self.throws_declared(e) {
                self.err_coded(
                    span,
                    format!(
                        "`?` propagates `{e}`, but the enclosing function does not declare `throws {e}`"
                    ),
                    "E-CALL-UNHANDLED",
                    Some(format!(
                        "add `throws {e}` to the enclosing function, or wrap the call in `try`/`catch`"
                    )),
                );
            }
        }
        // Record for erasure: a throws-mode `?` is a checker-only marker (the call's own throw
        // unwinds), so the backend-facing AST keeps just the inner call (see `resolve_html`).
        self.html_resolutions.insert(span.start, inner.clone());
        Some(t)
    }

    /// Validate a function's `throws` declaration (M-faults 2b): the entry `main` may not declare it
    /// (`E-UNCAUGHT-THROW`); every declared type must implement `Error` (`E-THROW-TYPE`); and naming
    /// the bare `Error` root is too broad (`E-THROWS-TOO-BROAD` — declare the specific subtype so
    /// callers know what to catch). `throws` is resolved into `resolved` in declaration order.
    fn validate_throws_decl(&mut self, f: &crate::ast::FunctionDecl, resolved: &[Ty]) {
        if f.name == "main" && !f.throws.is_empty() {
            self.err_coded(
                f.span,
                "`main` is the program entry point and may not declare `throws`",
                "E-UNCAUGHT-THROW",
                Some(
                    "handle every error inside `main` with `try`/`catch` — nothing may escape the entry point"
                        .into(),
                ),
            );
        }
        for t in resolved {
            match t {
                Ty::Error => {} // poison from an earlier error — don't cascade
                Ty::Named(n, _) if n == "Error" => {
                    self.err_coded(
                        f.span,
                        "`throws Error` is too broad — declare the specific exception type(s) you throw",
                        "E-THROWS-TOO-BROAD",
                        Some("e.g. `throws BadInput` so callers know exactly what to catch".into()),
                    );
                }
                _ if !self.is_error_type(t) => {
                    self.err_coded(
                        f.span,
                        format!(
                            "`throws {t}` is not allowed — a thrown type must implement `Error`"
                        ),
                        "E-THROW-TYPE",
                        Some("declare the throwing type `class Foo implements Error { … }`".into()),
                    );
                }
                _ => {}
            }
        }
    }

    fn collect_function(&mut self, f: &crate::ast::FunctionDecl) {
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
        // Resolve the signature with the type parameters in scope so `T` becomes `Ty::Param("T")`.
        self.active_type_params = f.type_params.clone();
        let params = f.params.iter().map(|p| self.resolve_type(&p.ty)).collect();
        let ret = match &f.ret {
            Some(t) => self.resolve_type(t),
            None => Ty::Unit,
        };
        // Resolve the declared throws set with the type parameters still in scope, then clear. A
        // union `throws A | B` is flattened to its members (`throws` is a set of exception types).
        let throws = Self::flatten_throws(f.throws.iter().map(|t| self.resolve_type(t)).collect());
        self.active_type_params.clear();
        // M-RT overloading: a same-named function joins an overload set rather than colliding. The
        // set must share a return type and hold no two identical signatures; a generic overload is
        // not allowed to participate (deferred). Push regardless of legality so downstream resolution
        // sees the whole set (errors already reported).
        let sig = FnSig {
            params,
            ret,
            type_params: f.type_params.clone(),
            throws,
        };
        let existing = self.funcs.get(&f.name).cloned().unwrap_or_default();
        self.validate_new_overload(&existing, &sig, &f.name, f.span, "function");
        self.funcs.entry(f.name.clone()).or_default().push(sig);
    }

    /// M-RT overloading: validate a new overload `sig` against the overloads of the same name already
    /// collected in `existing`. Emits diagnostics only; the caller pushes `sig` regardless so later
    /// resolution sees the full set. A legal set shares one return type (`E-OVERLOAD-RETURN`) and has
    /// no two identical parameter signatures (`E-OVERLOAD-DUPLICATE`); a generic member cannot
    /// participate (`E-OVERLOAD-GENERIC`, deferred). The first declaration is always fine.
    fn validate_new_overload(
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
    fn validate_type_params(&mut self, type_params: &[String], span: Span) {
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

    fn collect_enum(&mut self, e: &crate::ast::EnumDecl) {
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

    fn collect_class(&mut self, c: &crate::ast::ClassDecl) {
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
                static_mut: std::collections::HashSet::new(),
                methods: HashMap::new(),
                hooks: HashMap::new(),
                ctor: Vec::new(),
                has_ctor: false,
                type_params: c.type_params.clone(),
                is_abstract: c.is_abstract,
            },
        );
        use crate::ast::Modifier;
        let mut fields = HashMap::new();
        let mut mutable_fields = std::collections::HashSet::new();
        let mut statics: HashMap<String, Ty> = HashMap::new();
        let mut static_mut = std::collections::HashSet::new();
        let mut methods: HashMap<String, Vec<FnSig>> = HashMap::new();
        let mut hooks: HashMap<String, HookInfo> = HashMap::new();
        let mut ctor = Vec::new();
        // The class's type parameters are in scope while resolving every member signature (fields,
        // constructor, methods), so a bare `T` resolves to `Ty::Param("T")` (M-RT generics-all). A
        // generic method adds its own parameters on top.
        let class_tp = &c.type_params;
        // Promoted ctor params (carrying a visibility modifier) also become fields,
        // matching the evaluator's runtime promotion (EV-4). Deferred to after the
        // member loop via `or_insert` so an explicit `Field` decl of the same name
        // stays authoritative regardless of member order.
        let mut promoted: Vec<(String, Ty)> = Vec::new();
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
                    if modifiers.contains(&Modifier::Static) {
                        // A `static` field is class-level state (M-mut.7): it needs a literal-const
                        // initializer (no constructor sets it) and is NOT an instance field.
                        match init {
                            None => {
                                self.err_coded(
                                    *span,
                                    format!("static field `{name}` needs an initializer"),
                                    "E-STATIC-NO-INIT",
                                    Some("e.g. `static mutable int total = 0;`".into()),
                                );
                            }
                            Some(e) => {
                                if crate::value::const_literal(e).is_none() {
                                    self.err_coded(
                                        Self::expr_span(e),
                                        format!("static field `{name}` initializer must be a literal constant"),
                                        "E-STATIC-INIT-CONST",
                                        Some("use an int/float/bool/string/null literal".into()),
                                    );
                                } else {
                                    let ity = self.check_expr(e);
                                    if !self.ty_assignable(&ity, &fty) {
                                        self.err_coded(
                                            Self::expr_span(e),
                                            format!("static field `{name}: {fty}` initialized with `{ity}`"),
                                            "E-STATIC-INIT-TYPE",
                                            None,
                                        );
                                    }
                                }
                            }
                        }
                        statics.insert(name.clone(), fty);
                        if modifiers.contains(&Modifier::Mutable) {
                            static_mut.insert(name.clone());
                        }
                    } else {
                        // An instance field is set via the constructor; a field-level initializer is
                        // not supported yet (M-mut.7 deferral).
                        if init.is_some() {
                            self.err_coded(
                                *span,
                                format!("instance field `{name}` cannot have an initializer"),
                                "E-FIELD-INIT",
                                Some(format!(
                                    "set it in the constructor (`this.{name} = …`) or make it `static`"
                                )),
                            );
                        }
                        fields.insert(name.clone(), fty);
                        if modifiers.contains(&Modifier::Mutable) {
                            mutable_fields.insert(name.clone());
                        }
                    }
                }
                ClassMember::Constructor { params, .. } => {
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
                                promoted.push((p.name.clone(), ty.clone()));
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
                        None => Ty::Unit,
                    };
                    let throws = Self::flatten_throws(
                        f.throws.iter().map(|t| self.resolve_type(t)).collect(),
                    );
                    self.active_type_params.clear();
                    // M-RT overloading: a same-named method joins an overload set (same rules as free
                    // functions — same return, no identical signatures, no generic member).
                    let sig = FnSig {
                        params: p,
                        ret,
                        type_params: f.type_params.clone(),
                        throws,
                    };
                    let existing = methods.get(&f.name).cloned().unwrap_or_default();
                    self.validate_new_overload(&existing, &sig, &f.name, f.span, "method");
                    methods.entry(f.name.clone()).or_default().push(sig);
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
        for (name, ty) in promoted {
            fields.entry(name).or_insert(ty);
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
        info.mutable_fields = mutable_fields;
        info.statics = statics;
        info.static_mut = static_mut;
        info.methods = methods;
        info.hooks = hooks;
        info.has_ctor = c
            .members
            .iter()
            .any(|m| matches!(m, ClassMember::Constructor { .. }));
        info.ctor = ctor;
    }

    /// Phase 2 — check every function/method body.
    fn check_program(&mut self, program: &Program) {
        use crate::ast::Item;
        // Reshape slice 2a: identifier casing is a hard, front-end-only rule. Run it first so its
        // diagnostics surface regardless of body-level errors (it is purely declaration-shaped).
        self.check_casing(program);
        // M5 S1: every file is packaged, never inferred. Empty ⇒ no declaration; a `core` root is
        // reserved for the standard library. (Strict folder=path and loose-mode `main`-only land
        // with the project model in S2 — `docs/specs/2026-06-18-m5-project-model-design.md`.)
        if program.package.is_empty() {
            self.err_coded(
                program.span,
                "every file must declare a package (e.g. `package Main;`) as its first line",
                "E-NO-PACKAGE",
                Some("add `package Main;` at the top of the file".into()),
            );
        } else if program.package[0] == "Core" {
            self.err_coded(
                program.span,
                "`Core` is a reserved package root (the standard library)",
                "E-RESERVED-PACKAGE",
                Some("use a different root, e.g. `package App;`".into()),
            );
        }
        // Reshape slice 2b: package + import path segments are PascalCase (`E-PKG-CASE`) — a 1:1
        // mapping to PHP namespaces with no casing transform. Front-end-only, so it cannot affect
        // byte-identity (every backend sees the same AST; the rule only gates which programs reach
        // them). The reserved `Main`/`Core` roots are already PascalCase. An empty package is left to
        // `E-NO-PACKAGE` above (the loop is empty), so the two never double-report.
        for seg in &program.package {
            if !is_pascal(seg) {
                self.err_coded(
                    program.span,
                    format!("package segment `{seg}` must be PascalCase"),
                    "E-PKG-CASE",
                    Some(format!("did you mean `package {}`?", to_pascal(seg))),
                );
            }
        }
        for item in &program.items {
            if let Item::Import {
                path, alias, span, ..
            } = item
            {
                for seg in path {
                    if !is_pascal(seg) {
                        self.err_coded(
                            *span,
                            format!("import segment `{seg}` must be PascalCase"),
                            "E-PKG-CASE",
                            Some(format!("did you mean `{}`?", to_pascal(seg))),
                        );
                    }
                }
                // An alias renames the call-site qualifier (`import A.B as C;`), so it occupies a
                // package-leaf position and follows the same PascalCase rule.
                if let Some(a) = alias {
                    if !is_pascal(a) {
                        self.err_coded(
                            *span,
                            format!("import alias `{a}` must be PascalCase"),
                            "E-PKG-CASE",
                            Some(format!("did you mean `as {}`?", to_pascal(a))),
                        );
                    }
                }
            }
        }
        for item in &program.items {
            match item {
                Item::Function(f) => self.check_function(f),
                Item::Class(c) => self.check_type_body(&c.name, &c.type_params, &c.members),
                // M-RT S8: a trait's method/ctor/hook bodies are checked once, in trait context
                // (correct spans, no double-reporting), with the trait's own collected members as
                // `this`. A trait has no type parameters this slice.
                Item::Trait(t) => self.check_type_body(&t.name, &[], &t.members),
                // Interface method signatures have no body to check (the conformance/graph
                // validation ran in `collect`); enums/imports/aliases have nothing here.
                Item::Enum(_)
                | Item::Interface(_)
                | Item::Import { .. }
                | Item::TypeAlias { .. } => {}
            }
        }
    }

    /// Check the method/constructor/hook bodies of a class or trait (M-RT S8 shares this between the
    /// two). `this` resolves to `type_name`'s already-collected [`ClassInfo`]; `type_params` are in
    /// scope across every body (empty for a trait this slice).
    fn check_type_body(
        &mut self,
        type_name: &str,
        type_params: &[String],
        members: &[crate::ast::ClassMember],
    ) {
        use crate::ast::ClassMember;
        let prev = self.cur_class.replace(type_name.to_string());
        let prev_tp = std::mem::replace(&mut self.cur_class_type_params, type_params.to_vec());
        for m in members {
            match m {
                ClassMember::Method(f) => self.check_function(f),
                ClassMember::Constructor { params, body, .. } => {
                    let prev_ret = std::mem::replace(&mut self.cur_ret, Ty::Unit);
                    // type params in scope for any `T` annotation in the body
                    self.active_type_params = type_params.to_vec();
                    self.push_scope();
                    // constructor params are in scope inside its body
                    let ctor = self
                        .classes
                        .get(type_name)
                        .map(|info| info.ctor.clone())
                        .unwrap_or_default();
                    for (p, t) in params.iter().zip(ctor) {
                        self.declare(&p.name, t, p.span);
                    }
                    self.check_body(body);
                    self.pop_scope();
                    self.active_type_params.clear();
                    self.cur_ret = prev_ret;
                }
                // A property hook (M-mut.7b) — type-check the `get` expression against the hook's
                // declared type and the `set` block with the assigned value `v` bound to that type,
                // both with `this` + the field scope live.
                ClassMember::Hook {
                    ty, name, get, set, ..
                } => {
                    self.active_type_params = type_params.to_vec();
                    let hook_ty = self.resolve_type(ty);
                    if let Some(e) = get {
                        self.push_scope();
                        let ety = self.check_expr(e);
                        if !self.ty_assignable(&ety, &hook_ty) {
                            self.err_coded(
                                Self::expr_span(e),
                                format!("`get` of `{name}` yields `{ety}`, expected `{hook_ty}`"),
                                "E-HOOK-TYPE",
                                None,
                            );
                        }
                        self.pop_scope();
                    }
                    if let Some((p, body)) = set {
                        let prev_ret = std::mem::replace(&mut self.cur_ret, Ty::Unit);
                        self.push_scope();
                        let pty = self.resolve_type(&p.ty);
                        if !(self.ty_assignable(&pty, &hook_ty)
                            && self.ty_assignable(&hook_ty, &pty))
                        {
                            self.err_coded(
                                p.span,
                                format!(
                                    "`set` parameter of `{name}` is `{pty}`, expected the hook type `{hook_ty}`"
                                ),
                                "E-HOOK-TYPE",
                                Some(format!("declare it `set({hook_ty} {})`", p.name)),
                            );
                        }
                        // Bind `v` at the hook's type so the body checks consistently even when the
                        // declared parameter type mismatched.
                        self.declare(&p.name, hook_ty.clone(), p.span);
                        self.check_body(body);
                        self.pop_scope();
                        self.cur_ret = prev_ret;
                    }
                    self.active_type_params.clear();
                }
                ClassMember::Field { .. } => {}
            }
        }
        self.cur_class_type_params = prev_tp;
        self.cur_class = prev;
    }

    // ---- identifier casing (reshape slice 2a) ----
    /// Enforce the casing discipline as **hard** errors (front-end-only, so it cannot affect
    /// byte-identity — every backend sees the same AST, the rule just gates which programs reach
    /// them). Value identifiers (functions, methods, parameters, fields, `var` bindings, lambda
    /// parameters) must be camelCase (`E-NAME-CASE`); type identifiers (class, enum, enum variant,
    /// `type` alias names) must be PascalCase (`E-TYPE-CASE`). Package segments are NOT checked here
    /// — that is reshape slice 2b (`E-PKG-CASE`).
    fn check_casing(&mut self, program: &Program) {
        use crate::ast::{ClassMember, Item};
        for item in &program.items {
            match item {
                Item::Function(f) => self.check_fn_casing(f),
                Item::Class(c) => {
                    self.want_type_case(&c.name, c.span);
                    // Generic class type parameters are type names — PascalCase (M-RT generics-all).
                    for tp in &c.type_params {
                        self.want_type_case(tp, c.span);
                    }
                    for m in &c.members {
                        match m {
                            ClassMember::Field { name, span, .. } => {
                                self.want_name_case(name, *span);
                            }
                            ClassMember::Constructor { params, .. } => {
                                for p in params {
                                    self.want_name_case(&p.name, p.span);
                                }
                            }
                            ClassMember::Method(f) => self.check_fn_casing(f),
                            // A hook name + its `set` parameter follow field/var casing (camelCase).
                            ClassMember::Hook {
                                name, set, span, ..
                            } => {
                                self.want_name_case(name, *span);
                                if let Some((p, _)) = set {
                                    self.want_name_case(&p.name, p.span);
                                }
                            }
                        }
                    }
                }
                Item::Enum(e) => {
                    self.want_type_case(&e.name, e.span);
                    for v in &e.variants {
                        self.want_type_case(&v.name, v.span);
                    }
                }
                Item::Interface(i) => {
                    self.want_type_case(&i.name, i.span);
                    for m in &i.methods {
                        self.check_fn_casing(m);
                    }
                }
                // M-RT S8: a trait name is a type identifier (PascalCase); its members follow the same
                // value-identifier casing as a class's.
                Item::Trait(t) => {
                    self.want_type_case(&t.name, t.span);
                    for m in &t.members {
                        match m {
                            ClassMember::Field { name, span, .. } => {
                                self.want_name_case(name, *span)
                            }
                            ClassMember::Constructor { params, .. } => {
                                for p in params {
                                    self.want_name_case(&p.name, p.span);
                                }
                            }
                            ClassMember::Method(f) => self.check_fn_casing(f),
                            ClassMember::Hook {
                                name, set, span, ..
                            } => {
                                self.want_name_case(name, *span);
                                if let Some((p, _)) = set {
                                    self.want_name_case(&p.name, p.span);
                                }
                            }
                        }
                    }
                }
                Item::TypeAlias { name, span, .. } => self.want_type_case(name, *span),
                Item::Import { .. } => {}
            }
        }
    }

    /// Casing for a function/method declaration: its name + parameters are camelCase, and its body
    /// is walked for `var` bindings and lambda parameters.
    fn check_fn_casing(&mut self, f: &crate::ast::FunctionDecl) {
        self.want_name_case(&f.name, f.span);
        // Generic type parameters are type names — PascalCase, like classes/enums (M-RT S7).
        for tp in &f.type_params {
            self.want_type_case(tp, f.span);
        }
        for p in &f.params {
            self.want_name_case(&p.name, p.span);
        }
        for s in &f.body {
            self.check_stmt_casing(s);
        }
    }

    /// Walk a statement for value-binding casing (`var` declarations, `for`-loop variables,
    /// if-let bindings) and any nested lambda parameters.
    fn check_stmt_casing(&mut self, s: &crate::ast::Stmt) {
        use crate::ast::Stmt;
        match s {
            Stmt::VarDecl {
                name, init, span, ..
            } => {
                self.want_name_case(name, *span);
                self.check_expr_casing(init);
            }
            Stmt::Return { value, .. } => {
                if let Some(e) = value {
                    self.check_expr_casing(e);
                }
            }
            Stmt::If {
                cond,
                then_block,
                else_block,
                ..
            } => {
                self.check_expr_casing(cond);
                for st in then_block {
                    self.check_stmt_casing(st);
                }
                if let Some(eb) = else_block {
                    for st in eb {
                        self.check_stmt_casing(st);
                    }
                }
            }
            Stmt::For { iter, body, .. } => {
                self.check_expr_casing(iter);
                for st in body {
                    self.check_stmt_casing(st);
                }
            }
            Stmt::While { cond, body, .. } => {
                self.check_expr_casing(cond);
                for st in body {
                    self.check_stmt_casing(st);
                }
            }
            Stmt::CFor {
                init,
                cond,
                step,
                body,
                ..
            } => {
                if let Some(s) = init {
                    self.check_stmt_casing(s);
                }
                if let Some(c) = cond {
                    self.check_expr_casing(c);
                }
                if let Some(s) = step {
                    self.check_stmt_casing(s);
                }
                for st in body {
                    self.check_stmt_casing(st);
                }
            }
            Stmt::Break(_) | Stmt::Continue(_) => {}
            Stmt::Block(stmts, _) => {
                for st in stmts {
                    self.check_stmt_casing(st);
                }
            }
            Stmt::Assign { target, value, .. } => {
                self.check_expr_casing(target);
                self.check_expr_casing(value);
            }
            Stmt::Expr(e, _) => self.check_expr_casing(e),
            Stmt::Throw { value, .. } => self.check_expr_casing(value),
            Stmt::Try {
                body,
                catches,
                finally_block,
                ..
            } => {
                for st in body {
                    self.check_stmt_casing(st);
                }
                for c in catches {
                    self.want_name_case(&c.name, c.span);
                    for st in &c.body {
                        self.check_stmt_casing(st);
                    }
                }
                if let Some(fb) = finally_block {
                    for st in fb {
                        self.check_stmt_casing(st);
                    }
                }
            }
        }
    }

    /// Walk an expression for lambda parameters (the only value bindings introduced inside an
    /// expression) and recurse through every sub-expression.
    fn check_expr_casing(&mut self, e: &crate::ast::Expr) {
        use crate::ast::{Expr, LambdaBody, StrPart};
        match e {
            Expr::Int(..)
            | Expr::Float(..)
            | Expr::Bool(..)
            | Expr::Null(..)
            | Expr::Bytes(..)
            | Expr::Ident(..)
            | Expr::This(..) => {}
            Expr::Str(parts, _) | Expr::Html(parts, _) => {
                for p in parts {
                    if let StrPart::Expr(inner) = p {
                        self.check_expr_casing(inner);
                    }
                }
            }
            Expr::List(items, _) => {
                for it in items {
                    self.check_expr_casing(it);
                }
            }
            Expr::Map(pairs, _) => {
                for (k, v) in pairs {
                    self.check_expr_casing(k);
                    self.check_expr_casing(v);
                }
            }
            Expr::Unary { expr, .. } => self.check_expr_casing(expr),
            Expr::Binary { lhs, rhs, .. } => {
                self.check_expr_casing(lhs);
                self.check_expr_casing(rhs);
            }
            Expr::InstanceOf { value, .. } => self.check_expr_casing(value),
            Expr::Call { callee, args, .. } => {
                self.check_expr_casing(callee);
                for a in args {
                    self.check_expr_casing(a);
                }
            }
            Expr::Member { object, .. } => self.check_expr_casing(object),
            Expr::Index { object, index, .. } => {
                self.check_expr_casing(object);
                self.check_expr_casing(index);
            }
            Expr::Force { inner, .. } => self.check_expr_casing(inner),
            Expr::Propagate { inner, .. } => self.check_expr_casing(inner),
            Expr::CloneWith { object, fields, .. } => {
                self.check_expr_casing(object);
                for (_, e) in fields {
                    self.check_expr_casing(e);
                }
            }
            Expr::Match {
                scrutinee, arms, ..
            } => {
                self.check_expr_casing(scrutinee);
                for arm in arms {
                    self.check_expr_casing(&arm.body);
                }
            }
            Expr::Range { start, end, .. } => {
                self.check_expr_casing(start);
                self.check_expr_casing(end);
            }
            Expr::If {
                cond,
                then_expr,
                else_expr,
                ..
            } => {
                self.check_expr_casing(cond);
                self.check_expr_casing(then_expr);
                self.check_expr_casing(else_expr);
            }
            Expr::Lambda { params, body, .. } => {
                for p in params {
                    self.want_name_case(&p.name, p.span);
                }
                match body {
                    LambdaBody::Expr(inner) => self.check_expr_casing(inner),
                    LambdaBody::Block(stmts) => {
                        for st in stmts {
                            self.check_stmt_casing(st);
                        }
                    }
                }
            }
        }
    }

    /// A value identifier must be camelCase; otherwise `E-NAME-CASE` with a converted-form hint.
    ///
    /// The loader's cross-package mangling (M5 S2c) rewrites a library def name to a PHP-FQN key
    /// (`acme.util` + `compute` ⇒ `Acme\Util\compute`) *before* the checker runs. Casing applies to
    /// the **original source identifier**, so validate only the last `\`-segment — the leaf — which
    /// is byte-for-byte the name the developer wrote.
    fn want_name_case(&mut self, name: &str, span: Span) {
        let leaf = leaf_ident(name);
        if !is_camel(leaf) {
            self.err_coded(
                span,
                format!("`{leaf}` must be camelCase"),
                "E-NAME-CASE",
                Some(format!("did you mean `{}`?", to_camel(leaf))),
            );
        }
    }

    /// A type identifier must be PascalCase; otherwise `E-TYPE-CASE` with a converted-form hint.
    /// Validates the leaf identifier (see [`Self::want_name_case`] for why the FQN prefix is
    /// stripped). Cross-package types do not exist yet (`E-PKG-TYPE`), so a type name is never
    /// mangled today — but the leaf-strip keeps this robust if that changes.
    fn want_type_case(&mut self, name: &str, span: Span) {
        let leaf = leaf_ident(name);
        if !is_pascal(leaf) {
            self.err_coded(
                span,
                format!("`{leaf}` must be PascalCase"),
                "E-TYPE-CASE",
                Some(format!("did you mean `{}`?", to_pascal(leaf))),
            );
        }
    }

    /// Check one free function or method body. Seeds a fresh scope with params. A generic function's
    /// type parameters are made active for the whole body so `T`-typed params/locals resolve to
    /// `Ty::Param` (M-RT S7). Functions never nest, so a flat set + clear is sufficient.
    fn check_function(&mut self, f: &crate::ast::FunctionDecl) {
        // A method of a generic class sees both the class's type parameters and its own (M-RT
        // generics-all); `cur_class_type_params` is empty for free functions and non-generic classes.
        let mut active = self.cur_class_type_params.clone();
        active.extend(f.type_params.iter().cloned());
        self.active_type_params = active;
        let ret = match &f.ret {
            Some(t) => self.resolve_type(t),
            None => Ty::Unit,
        };
        // Resolve + validate the declared throws set (type params still in scope), then make it the
        // active discharge context for the body (M-faults 2b).
        let throws = Self::flatten_throws(f.throws.iter().map(|t| self.resolve_type(t)).collect());
        self.validate_throws_decl(f, &throws);
        let prev_ret = std::mem::replace(&mut self.cur_ret, ret.clone());
        let prev_throws = std::mem::replace(&mut self.cur_throws, throws);
        let prev_main = std::mem::replace(&mut self.cur_is_main, f.name == "main");
        self.push_scope();
        for p in &f.params {
            let pty = self.resolve_type(&p.ty);
            self.declare(&p.name, pty, p.span);
        }
        self.check_body(&f.body);
        self.pop_scope();
        self.cur_ret = prev_ret;
        self.cur_throws = prev_throws;
        self.cur_is_main = prev_main;
        self.active_type_params.clear();
        // Totality: a non-`unit` function must return (or diverge) on every path (M-RT totality
        // cluster). Run after the body walk so all signatures are visible to the divergence analysis.
        // An `abstract` method (M-RT S6b) is a bodyless signature — exempt, like an interface method.
        if !f.modifiers.contains(&crate::ast::Modifier::Abstract) {
            self.check_return_totality(&ret, &f.body, f.span);
        }
    }

    // ---- scopes ----
    fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }
    fn pop_scope(&mut self) {
        self.scopes.pop();
    }
    fn declare(&mut self, name: &str, ty: Ty, span: Span) {
        self.declare_binding(name, ty, false, span);
    }
    /// Declare a binding with an explicit mutability (M-mut.1). `declare` is the immutable-default
    /// shorthand used by params, patterns, and if-let bindings (none of which are reassignable).
    fn declare_binding(&mut self, name: &str, ty: Ty, mutable: bool, span: Span) {
        // A value binding may not shadow an imported module qualifier: were `console` both a local
        // and `import core.console;`, the run backends (locals-first) would treat `console.x()` as a
        // method call while the transpiler (import-map-driven) would emit the native — a silent
        // divergence. Forbidding the overlap keeps all four backends consistent (M3 Wave 1).
        if self.imports.contains_key(name) {
            self.err_coded(
                span,
                format!("`{name}` shadows the imported module qualifier `{name}`"),
                "E-SHADOW-IMPORT",
                Some(format!(
                    "rename the binding, or remove the matching `import …{name};`"
                )),
            );
        }
        // Likewise, a value binding may not shadow a top-level function name. A bare `f(…)` call
        // dispatches functions-first in the run backends but locals-first in the transpiler, and a
        // bare `f` value reference resolves to the function in the backends but the local in PHP —
        // so an overlap is a silent four-backend divergence (the function-name analogue of
        // E-SHADOW-IMPORT, made reachable once functions became first-class values in M3 S3).
        if self.funcs.contains_key(name) {
            self.err_coded(
                span,
                format!("`{name}` shadows the top-level function `{name}`"),
                "E-SHADOW-FN",
                Some(format!(
                    "rename the binding; a local may not share a name with a function (`{name}` is callable as a value)"
                )),
            );
        }
        if let Some(top) = self.scopes.last_mut() {
            top.insert(name.to_string(), (ty, mutable));
        }
    }
    /// A local binding's `(type, mutable)` — locals only (does not fall through to class fields).
    /// Used by the reassignment check (M-mut.1): a non-local target is `E-ASSIGN-UNKNOWN`.
    fn lookup_binding(&self, name: &str) -> Option<(Ty, bool)> {
        for scope in self.scopes.iter().rev() {
            if let Some(b) = scope.get(name) {
                return Some(b.clone());
            }
        }
        None
    }
    fn lookup(&self, name: &str) -> Option<Ty> {
        for scope in self.scopes.iter().rev() {
            if let Some((t, _)) = scope.get(name) {
                return Some(t.clone());
            }
        }
        // bare field reference inside a method
        if let Some(cls) = &self.cur_class {
            if let Some(info) = self.classes.get(cls) {
                if let Some(t) = info.fields.get(name) {
                    return Some(t.clone());
                }
            }
        }
        None
    }

    // ---- totality: structural termination analysis (M-RT totality cluster) ----
    // Conservatively answers "does control definitely never fall through this block / statement?".
    // Drives return-on-all-paths (`E-MISSING-RETURN`/`E-NEVER-RETURN`) and the `W-UNREACHABLE` lint.
    // Soundness direction: returns `true` only for shapes that *provably* diverge — a false `true`
    // would silently suppress a real missing-return error, so it never over-claims.

    /// A block diverges iff *any* of its statements diverges (everything after a diverging statement
    /// is dead, so the block as a whole never falls through).
    fn block_terminates(&self, stmts: &[crate::ast::Stmt]) -> bool {
        stmts.iter().any(|s| self.stmt_terminates(s))
    }

    fn stmt_terminates(&self, s: &crate::ast::Stmt) -> bool {
        use crate::ast::Stmt;
        match s {
            Stmt::Return { .. } => true,
            Stmt::Block(b, _) => self.block_terminates(b),
            Stmt::If {
                then_block,
                else_block: Some(eb),
                ..
            } => self.block_terminates(then_block) && self.block_terminates(eb),
            // An `if` with no `else` always has a path (the false branch) that falls through.
            Stmt::If {
                else_block: None, ..
            } => false,
            // A condition loop diverges only when it cannot exit: an always-true condition with no
            // `break` bound to it. A `do { … } while (…)` additionally diverges when its body diverges
            // on the guaranteed first iteration. A plain `while`/`for` body may run zero times, so a
            // diverging body alone does NOT make the loop diverge.
            Stmt::While {
                cond,
                body,
                post_cond,
                ..
            } => {
                (*post_cond && self.block_terminates(body))
                    || (is_true_lit(cond) && !breaks_this_loop(body))
            }
            Stmt::CFor { cond, body, .. } => {
                let cond_always = match cond {
                    None => true,
                    Some(c) => is_true_lit(c),
                };
                cond_always && !breaks_this_loop(body)
            }
            // `for (T x in iter)` always terminates over a finite list — never a divergence source.
            Stmt::Expr(e, _) => self.expr_is_never(e),
            // A `throw` always diverges (it unwinds out of the current frame; M-faults 2b).
            Stmt::Throw { .. } => true,
            // A `try` diverges iff control can never leave it normally: a `finally` that itself
            // diverges forces divergence; otherwise every exit edge must diverge — the body AND
            // every catch body. (An uncaught throw out of the body also diverges, so requiring the
            // body to terminate is sound — if it falls through normally, so does the `try`.)
            Stmt::Try {
                body,
                catches,
                finally_block,
                ..
            } => {
                if finally_block
                    .as_ref()
                    .is_some_and(|fb| self.block_terminates(fb))
                {
                    return true;
                }
                self.block_terminates(body)
                    && catches.iter().all(|c| self.block_terminates(&c.body))
            }
            _ => false,
        }
    }

    /// Whether an expression has the bottom type `never` — recognised *without* re-checking (emits no
    /// diagnostics): a call to a free function whose signature returns `never`, or an `if`/`match`
    /// expression every arm of which is itself `never`. Method/closure `never`-returns are deferred
    /// (need receiver typing) — see the design's KNOWN_ISSUES.
    fn expr_is_never(&self, e: &crate::ast::Expr) -> bool {
        use crate::ast::Expr;
        match e {
            Expr::Call { callee, .. } => {
                if let Expr::Ident(name, _) = &**callee {
                    // A `never`-typed fault intrinsic (`panic`/`todo`/`unreachable` — not `assert`,
                    // which is `unit`), or a user function declared `-> never` (M-faults 2a).
                    matches!(name.as_str(), "panic" | "todo" | "unreachable")
                        || self
                            .funcs
                            .get(name)
                            .and_then(|v| v.first())
                            .is_some_and(|s| s.ret == Ty::Never)
                } else {
                    false
                }
            }
            Expr::If {
                then_expr,
                else_expr,
                ..
            } => self.expr_is_never(then_expr) && self.expr_is_never(else_expr),
            Expr::Match { arms, .. } => {
                !arms.is_empty() && arms.iter().all(|a| self.expr_is_never(&a.body))
            }
            _ => false,
        }
    }

    /// Return-on-all-paths gate (M-RT totality cluster). A function whose declared return type carries
    /// a value must return (or diverge) on every path; `never` is the inverse — it must provably
    /// diverge. `unit` (the no-annotation default) and `<error>` (poison) are exempt.
    fn check_return_totality(&mut self, ret: &Ty, body: &[crate::ast::Stmt], span: Span) {
        match ret {
            Ty::Unit | Ty::Error => {}
            Ty::Never => {
                if !self.block_terminates(body) {
                    self.err_coded(
                        span,
                        "a `never` function must never return, but this body can fall through",
                        "E-NEVER-RETURN",
                        Some("a `-> never` function must diverge on every path (e.g. an infinite loop); drop the `never` return type if it can return normally".into()),
                    );
                }
            }
            _ => {
                if !self.block_terminates(body) {
                    self.err_coded(
                        span,
                        format!("function does not return `{ret}` on all paths"),
                        "E-MISSING-RETURN",
                        Some("add a `return` (or diverge) on every path — e.g. an `if` without an `else` leaves the false branch falling through".into()),
                    );
                }
            }
        }
    }

    // ---- statements ----

    /// Check a statement sequence in the *current* scope (no scope push), flagging the first
    /// unreachable statement after a diverging one (`W-UNREACHABLE`, once per dead region). Used for
    /// function, constructor, and `set`-hook bodies; `check_block` wraps it in a fresh scope for
    /// nested `{ … }` blocks.
    fn check_body(&mut self, stmts: &[crate::ast::Stmt]) {
        let mut dead = false;
        let mut warned = false;
        for s in stmts {
            if dead && !warned {
                self.warn_coded(
                    Self::stmt_span(s),
                    "unreachable code: control never reaches this statement",
                    "W-UNREACHABLE",
                    Some(
                        "a preceding statement always returns or diverges; remove the dead code"
                            .into(),
                    ),
                );
                warned = true;
            }
            self.check_stmt(s);
            if self.stmt_terminates(s) {
                dead = true;
            }
        }
    }

    fn check_block(&mut self, stmts: &[crate::ast::Stmt]) {
        self.push_scope();
        self.check_body(stmts);
        self.pop_scope();
    }

    fn stmt_span(s: &crate::ast::Stmt) -> Span {
        use crate::ast::Stmt;
        match s {
            Stmt::VarDecl { span, .. }
            | Stmt::Assign { span, .. }
            | Stmt::Return { span, .. }
            | Stmt::If { span, .. }
            | Stmt::For { span, .. }
            | Stmt::While { span, .. }
            | Stmt::CFor { span, .. }
            | Stmt::Expr(_, span)
            | Stmt::Block(_, span)
            | Stmt::Throw { span, .. }
            | Stmt::Try { span, .. } => *span,
            Stmt::Break(span) | Stmt::Continue(span) => *span,
        }
    }

    fn check_stmt(&mut self, stmt: &crate::ast::Stmt) {
        use crate::ast::Stmt;
        match stmt {
            Stmt::VarDecl {
                ty,
                name,
                init,
                mutable,
                span,
            } => {
                // A let-initializer is the one position where Result-mode `?` propagation is allowed
                // (M-faults 2a): detect it here and type it via `check_propagate` (the unwrapped `Ok`
                // payload). Throws-mode `?` (a throwing call) is allowed in *any* position and tried
                // first; it returns the call's normal type and erases the node (`try_throws_propagate`).
                let actual = match init {
                    crate::ast::Expr::Propagate { inner, span: psp } => self
                        .try_throws_propagate(inner, *psp)
                        .unwrap_or_else(|| self.check_propagate(inner, *psp)),
                    _ => self.check_expr(init),
                };
                let declared = match ty {
                    crate::ast::Type::Infer(infer_span) => {
                        // `var` binds the initializer's type — but a bare `null` (type `Ty::Null`)
                        // has no inferable element type and needs an explicit annotation, e.g.
                        // `int? x = null;` (S0.2 / S2).
                        if matches!(actual, Ty::Null) {
                            self.err_coded(
                                *infer_span,
                                "cannot infer a type from `null`",
                                "E-INFER-NULL",
                                Some("annotate the optional, e.g. `int? x = null;`".into()),
                            )
                        } else {
                            actual.clone()
                        }
                    }
                    _ => {
                        let declared = self.resolve_type(ty);
                        if !self.ty_assignable(&actual, &declared) {
                            self.err_assign(*span, &actual, &declared);
                        }
                        declared
                    }
                };
                self.declare_binding(name, declared, *mutable, *span);
            }
            Stmt::Assign {
                target,
                value,
                span,
            } => {
                use crate::ast::Expr;
                // Always check the value (surfaces nested errors regardless of the target's fate).
                let vty = self.check_expr(value);
                match target {
                    Expr::Ident(name, _) => self.check_local_reassign(name, &vty, target, value),
                    // Value-type element set `xs[i] = e` / `m[k] = e` (M-mut.5).
                    Expr::Index { object, index, .. } => {
                        self.check_index_assign(object, index, &vty, value, *span)
                    }
                    // Shared-mutable instance field set `o.f = e` / `this.f = e` (M-mut.6).
                    Expr::Member {
                        object, name, safe, ..
                    } => self.check_field_assign(object, name, *safe, &vty, value, *span),
                    _ => {
                        self.err_coded(
                            *span,
                            "assignment target must be a variable, an indexed element, or a field",
                            "E-ASSIGN-TARGET",
                            Some(
                                "only `name = e;`, `container[i] = e;`, and `obj.field = e;` are supported; nested places (`a.b.c`, `this.f[i]`) land in a later slice"
                                    .into(),
                            ),
                        );
                    }
                }
            }
            Stmt::Return { value, span } => {
                let actual = match value {
                    Some(e) => self.check_expr(e),
                    None => Ty::Unit,
                };
                let want = self.cur_ret.clone();
                if !self.ty_assignable(&actual, &want) {
                    self.err_assign(*span, &actual, &want);
                }
            }
            Stmt::If {
                cond,
                bind,
                then_block,
                else_block,
                span,
            } => {
                let c = self.check_expr(cond);
                if let Some(name) = bind {
                    // `if (var name = cond)`: the scrutinee must be optional; inside the then-block
                    // `name` is smart-cast to the non-optional inner `T` (and only there). The else
                    // block sees neither `name` nor any narrowing.
                    let inner = match &c {
                        Ty::Optional(i) => (**i).clone(),
                        Ty::Error => Ty::Error,
                        other => self.err_coded(
                            *span,
                            format!("`if (var {name} = …)` requires an optional `T?` scrutinee, found `{other}`"),
                            "E-IF-LET-TYPE",
                            Some("if-let narrows an optional to its non-null inner; the scrutinee is already non-optional".into()),
                        ),
                    };
                    self.push_scope();
                    self.declare(name, inner, *span);
                    self.check_block(then_block);
                    self.pop_scope();
                } else {
                    if !self.ty_assignable(&c, &Ty::Bool) {
                        self.err(*span, format!("`if` condition must be `bool`, found `{c}`"));
                    }
                    // instanceof smart-cast (M-RT S1, extended to interfaces in S2): inside the
                    // then-block, a local tested by `if (x instanceof T)` is narrowed to `T` — a
                    // class or an interface — so member/method access through it type-checks. Reuses
                    // the if-let scope mechanism (push_scope + declare). Only a bare-identifier
                    // operand against a known class/interface narrows.
                    let mut narrowed = false;
                    if let crate::ast::Expr::InstanceOf {
                        value, type_name, ..
                    } = cond
                    {
                        if let crate::ast::Expr::Ident(name, _) = &**value {
                            if self.classes.contains_key(type_name)
                                || self.interfaces.contains_key(type_name)
                            {
                                self.push_scope();
                                // `instanceof` carries no type arguments at runtime (`instanceof
                                // Box<int>` ≡ `instanceof Box`), so a narrowed generic class instance
                                // has erased (poison) type arguments — its generic members read as
                                // `mixed` (M-RT generics-all).
                                let arity = self
                                    .classes
                                    .get(type_name)
                                    .map_or(0, |c| c.type_params.len());
                                let args = vec![Ty::Error; arity];
                                // The narrowed shadow inherits the outer binding's mutability, so a
                                // `mutable` variable stays reassignable inside the narrowed block
                                // (reassignment is still type-checked against the narrowed type, so
                                // narrowing stays sound) — M-mut.1 smart-cast interaction.
                                let m = self.lookup_binding(name).map(|(_, m)| m).unwrap_or(false);
                                self.declare_binding(
                                    name,
                                    Ty::Named(type_name.clone(), args),
                                    m,
                                    *span,
                                );
                                self.check_block(then_block);
                                self.pop_scope();
                                narrowed = true;
                            }
                        }
                    }
                    if !narrowed {
                        self.check_block(then_block);
                    }
                }
                if let Some(eb) = else_block {
                    self.check_block(eb);
                }
            }
            Stmt::For { .. } => self.check_for(stmt), // implemented in Task 5
            Stmt::While {
                cond,
                body,
                post_cond,
                span,
            } => self.check_while(cond, body, *post_cond, *span),
            Stmt::CFor {
                init,
                cond,
                step,
                body,
                ..
            } => self.check_cfor(init.as_deref(), cond.as_ref(), step.as_deref(), body),
            Stmt::Break(span) => {
                if self.loop_depth == 0 {
                    self.err_coded(
                        *span,
                        "`break` outside a loop",
                        "E-BREAK-OUTSIDE-LOOP",
                        Some(
                            "`break` may only appear inside a `for`/`while`/`do-while` loop".into(),
                        ),
                    );
                }
            }
            Stmt::Continue(span) => {
                if self.loop_depth == 0 {
                    self.err_coded(
                        *span,
                        "`continue` outside a loop",
                        "E-CONTINUE-OUTSIDE-LOOP",
                        Some(
                            "`continue` may only appear inside a `for`/`while`/`do-while` loop"
                                .into(),
                        ),
                    );
                }
            }
            Stmt::Block(stmts, _) => self.check_block(stmts),
            Stmt::Expr(e, _) => {
                self.check_expr(e);
            }
            // M-faults 2b.3: `throw e` — the value must implement `Error` (`E-THROW-TYPE`), and the
            // exception must be *discharged* in context: caught by an enclosing `try` or declared in
            // the enclosing `throws` (`E-THROW-UNDECLARED`, or `E-UNCAUGHT-THROW` inside `main`).
            Stmt::Throw { value, span } => {
                let e = self.check_expr(value);
                if matches!(e, Ty::Error) {
                    // poison — an earlier error already reported
                } else if !self.is_error_type(&e) {
                    self.err_coded(
                        *span,
                        format!(
                            "can only `throw` a value whose type implements `Error`, found `{e}`"
                        ),
                        "E-THROW-TYPE",
                        Some("define the thrown type as `class Foo implements Error { … }`".into()),
                    );
                } else if !self.covered_by_try(&e) && !self.throws_declared(&e) {
                    if self.cur_is_main {
                        self.err_coded(
                            *span,
                            format!("`{e}` thrown in `main` escapes the program entry point"),
                            "E-UNCAUGHT-THROW",
                            Some("wrap it in `try { … } catch (… e) { … }` — `main` may not let an exception escape".into()),
                        );
                    } else {
                        self.err_coded(
                            *span,
                            format!("`{e}` is thrown here but neither caught nor declared"),
                            "E-THROW-UNDECLARED",
                            Some(format!("add `throws {e}` to the enclosing function, or wrap this in `try`/`catch`")),
                        );
                    }
                }
            }
            // M-faults 2b.3: a `try` — validate each catch type (`<: Error`, flag a shadowed clause
            // `W-CATCH-UNREACHABLE`), check the body with the catch set active so a throw inside is
            // discharged, then each catch body with its binding in scope, then `finally`.
            Stmt::Try {
                body,
                catches,
                finally_block,
                ..
            } => {
                // Resolve + validate catch types, building the active frame and the per-clause
                // binding types. A union catch `(A | B e)` contributes both members to the frame.
                let mut frame: Vec<Ty> = Vec::new();
                let mut seen: Vec<Ty> = Vec::new();
                let mut clause_tys: Vec<Ty> = Vec::with_capacity(catches.len());
                for c in catches {
                    let cty = self.resolve_type(&c.ty);
                    let members: Vec<Ty> = match &cty {
                        Ty::Union(ms) => ms.clone(),
                        other => vec![other.clone()],
                    };
                    for m in &members {
                        if !self.is_error_type(m) {
                            self.err_coded(
                                c.span,
                                format!("a `catch` type must implement `Error`, found `{m}`"),
                                "E-CATCH-TYPE",
                                Some("catch a type defined `class Foo implements Error { … }` (or the `Error` base itself)".into()),
                            );
                        }
                    }
                    // A clause every member of which is already covered by an earlier clause can
                    // never run (PHP is silent here; Phorge lints — see the totality cluster).
                    if !members.is_empty()
                        && members
                            .iter()
                            .all(|m| seen.iter().any(|s| self.ty_assignable(m, s)))
                    {
                        self.warn_coded(
                            c.span,
                            "unreachable `catch`: an earlier clause already catches this type",
                            "W-CATCH-UNREACHABLE",
                            Some(
                                "remove it, or reorder so the more specific clause comes first"
                                    .into(),
                            ),
                        );
                    }
                    seen.extend(members.iter().cloned());
                    frame.extend(members.iter().cloned());
                    clause_tys.push(cty);
                }
                // The catch set covers throws inside the *body* only (a throw in a catch/finally is
                // not caught by the same `try`): push for the body, pop before the clauses.
                self.try_catch_stack.push(frame);
                self.check_block(body);
                self.try_catch_stack.pop();
                for (c, cty) in catches.iter().zip(clause_tys) {
                    self.push_scope();
                    self.declare(&c.name, cty, c.span);
                    self.check_block(&c.body);
                    self.pop_scope();
                }
                if let Some(fb) = finally_block {
                    self.check_block(fb);
                }
            }
        }
    }

    // ---- expressions ----
    /// Depth-guarded entry to expression checking. Every recursive descent (`check_binary`,
    /// `check_call`, … all call back through here) is bounded by [`MAX_EXPR_DEPTH`], so a
    /// pathologically deep AST faults cleanly instead of overflowing the walker's stack. `depth`
    /// is balanced on every path (the result is captured before the decrement).
    fn check_expr(&mut self, expr: &crate::ast::Expr) -> Ty {
        self.depth += 1;
        let ty = if self.depth > MAX_EXPR_DEPTH {
            self.err(
                Self::expr_span(expr),
                format!("expression nests too deeply (limit {MAX_EXPR_DEPTH})"),
            )
        } else {
            self.check_expr_inner(expr)
        };
        self.depth -= 1;
        ty
    }

    fn check_expr_inner(&mut self, expr: &crate::ast::Expr) -> Ty {
        use crate::ast::Expr;
        match expr {
            Expr::Int(_, _) => Ty::Int,
            Expr::Float(_, _) => Ty::Float,
            Expr::Bool(_, _) => Ty::Bool,
            Expr::Null(_) => Ty::Null,
            Expr::Str(parts, _) => self.check_str(parts), // Task 7
            Expr::Bytes(_, _) => Ty::Bytes,
            Expr::Ident(name, span) => match self.lookup(name) {
                Some(t) => t,
                None => {
                    // A4: bare named-function reference in value position — `fn_name` where
                    // `fn_name` is a top-level function, not a local. Return its function type so
                    // it can be passed as a first-class argument or stored in a variable.
                    if let Some(sigs) = self.funcs.get(name) {
                        // An overloaded function has no single first-class value (which overload?).
                        if sigs.len() > 1 {
                            return self.err_coded(
                                *span,
                                format!("`{name}` is overloaded — an overloaded function has no single first-class value"),
                                "E-OVERLOAD-FN-VALUE",
                                Some("call it directly, or wrap the intended overload in a lambda".into()),
                            );
                        }
                        let sig = &sigs[0];
                        let param_tys = sig.params.clone();
                        let ret_ty = sig.ret.clone();
                        return Ty::Function(param_tys, Box::new(ret_ty));
                    }
                    let cands = self.in_scope_names();
                    let hint = self
                        .nearest_name(name, &cands)
                        .map(|c| format!("did you mean `{c}`?"));
                    self.err_coded(
                        *span,
                        format!("unknown identifier `{name}`"),
                        "E-UNKNOWN-IDENT",
                        hint,
                    )
                }
            },
            Expr::This(span) => match &self.cur_class {
                // Inside a generic class body, `this` carries the class's own type parameters as
                // opaque `Ty::Param`s, so `this.value` (a `T` field) types as `T` and member access
                // substitutes identically (M-RT generics-all). Empty args for a non-generic class.
                Some(c) => {
                    let args = self
                        .cur_class_type_params
                        .iter()
                        .map(|p| Ty::Param(p.clone()))
                        .collect();
                    Ty::Named(c.clone(), args)
                }
                None => self.err(*span, "`this` is only valid inside a method"),
            },
            Expr::List(elems, span) => self.check_list(elems, *span), // Task 5
            Expr::Map(pairs, span) => self.check_map(pairs, *span),   // M-RT S3
            Expr::Unary { op, expr, span } => self.check_unary(*op, expr, *span),
            Expr::Binary { op, lhs, rhs, span } => self.check_binary(*op, lhs, rhs, *span),
            Expr::InstanceOf {
                value,
                type_name,
                span,
            } => self.check_instanceof(value, type_name, *span),
            Expr::Call { callee, args, span } => self.check_call(callee, args, *span), // Task 4
            Expr::Member {
                object,
                name,
                safe,
                span,
            } => self.check_member(object, name, *safe, *span),
            Expr::Index {
                object,
                index,
                span,
            } => self.check_index(object, index, *span), // Task 5
            Expr::Force { inner, span } => self.check_force(inner, *span),
            // A `?` in general expression position. Throws-mode `?` (a throwing call) is valid here
            // and returns the call's normal type (M-faults 2b). Result-mode `?` is *not* — it is
            // restricted to a let-initializer (the one position with a clean PHP hoist; M-faults 2a):
            // flag it, but still check the inner to avoid a cascade.
            Expr::Propagate { inner, span } => self.try_throws_propagate(inner, *span).unwrap_or_else(
                || {
                    self.check_expr(inner);
                    self.err_coded(
                        *span,
                        "Result-mode `?` propagation is only allowed as the whole initializer of a `var`/typed binding",
                        "E-PROPAGATE-POSITION",
                        Some("bind the call's result to a local first (`var r = f(); …`), then handle it".into()),
                    )
                },
            ),
            Expr::CloneWith {
                object,
                fields,
                span,
            } => self.check_clone_with(object, fields, *span),
            Expr::Match {
                scrutinee,
                arms,
                span,
            } => self.check_match(scrutinee, arms, *span), // Task 8
            Expr::Range {
                start, end, span, ..
            } => self.check_range(start, end, *span),
            Expr::If {
                cond,
                then_expr,
                else_expr,
                span,
            } => self.check_if_expr(cond, then_expr, else_expr, *span),
            Expr::Lambda {
                params,
                ret,
                body,
                span,
            } => self.check_lambda(params, ret, body, *span),
            Expr::Html(parts, span) => self.check_html(parts, *span),
        }
    }

    fn check_unary(&mut self, op: crate::ast::UnaryOp, expr: &crate::ast::Expr, span: Span) -> Ty {
        use crate::ast::UnaryOp;
        let t = self.check_expr(expr);
        if t == Ty::Error {
            return Ty::Error;
        }
        match op {
            UnaryOp::Neg if t == Ty::Int || t == Ty::Float => t,
            UnaryOp::Neg => self.err(
                span,
                format!("unary `-` requires int or float, found `{t}`"),
            ),
            UnaryOp::Not if t == Ty::Bool => Ty::Bool,
            UnaryOp::Not => self.err(span, format!("unary `!` requires `bool`, found `{t}`")),
        }
    }

    fn check_binary(
        &mut self,
        op: crate::ast::BinaryOp,
        lhs: &crate::ast::Expr,
        rhs: &crate::ast::Expr,
        span: Span,
    ) -> Ty {
        use crate::ast::BinaryOp;
        let l = self.check_expr(lhs);
        let r = self.check_expr(rhs);
        if l == Ty::Error || r == Ty::Error {
            return match op {
                BinaryOp::Eq
                | BinaryOp::NotEq
                | BinaryOp::Lt
                | BinaryOp::Gt
                | BinaryOp::Le
                | BinaryOp::Ge
                | BinaryOp::And
                | BinaryOp::Or => Ty::Bool,
                _ => Ty::Error,
            };
        }
        match op {
            BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Rem => {
                if (l == Ty::Int && r == Ty::Int) || (l == Ty::Float && r == Ty::Float) {
                    l
                } else {
                    self.err(span, format!("arithmetic requires matching int or float operands, found `{l}` and `{r}`"))
                }
            }
            BinaryOp::Lt | BinaryOp::Gt | BinaryOp::Le | BinaryOp::Ge => {
                if (l == Ty::Int && r == Ty::Int) || (l == Ty::Float && r == Ty::Float) {
                    Ty::Bool
                } else {
                    self.err(span, format!("comparison requires matching int or float operands, found `{l}` and `{r}`"));
                    Ty::Bool
                }
            }
            BinaryOp::Eq | BinaryOp::NotEq => {
                if l != r {
                    self.err(
                        span,
                        format!(
                            "cross-type comparison requires explicit conversion (`{l}` vs `{r}`)"
                        ),
                    );
                }
                Ty::Bool
            }
            BinaryOp::And | BinaryOp::Or => {
                if l != Ty::Bool || r != Ty::Bool {
                    self.err(
                        span,
                        format!("`&&`/`||` require `bool` operands, found `{l}` and `{r}`"),
                    );
                }
                Ty::Bool
            }
            BinaryOp::Coalesce => {
                match &l {
                    Ty::Error => Ty::Error,
                    Ty::Null => r.clone(), // `null ?? b` is always `b`
                    Ty::Optional(inner) => {
                        let inner = (**inner).clone();
                        if self.ty_assignable(&r, &inner) {
                            inner // `a ?? b` yields the unwrapped `T` when the default is a `T`
                        } else {
                            if !self.ty_assignable(&r, &Ty::Optional(Box::new(inner.clone()))) {
                                self.err(
                                span,
                                format!("`??` default of type `{r}` is not compatible with `{inner}?`"),
                            );
                            }
                            Ty::Optional(Box::new(inner)) // both sides optional → stays `T?`
                        }
                    }
                    other => self.err(
                        span,
                        format!("left operand of `??` must be optional, found `{other}`"),
                    ),
                }
            }
            BinaryOp::Pipe => unreachable!("`|>` is lowered to a call in the parser"),
        }
    }

    /// `value instanceof TypeName` (M-RT S1): a runtime type test that always yields `bool`. The
    /// right operand must name a known class **or interface** (M-RT S2); the left operand must be a
    /// class instance (a `Ty::Named`). The smart-cast that narrows the operand inside an `if`
    /// then-block lives in `check_stmt`'s `Stmt::If` arm (it needs the surrounding block), not here.
    fn check_instanceof(&mut self, value: &crate::ast::Expr, type_name: &str, span: Span) -> Ty {
        let v = self.check_expr(value);
        // M-RT S8: a trait is reuse, not a type — it cannot be an `instanceof` target (it is collected
        // into `classes` for member lookup, so this explicit guard is needed before the class check).
        if self.traits.contains(type_name) {
            return self.err_coded(
                span,
                format!("`{type_name}` is a trait, not a type"),
                "E-INSTANCEOF-TYPE",
                Some("a trait is reuse, not a type; test against a class or interface".into()),
            );
        }
        if !self.classes.contains_key(type_name) && !self.interfaces.contains_key(type_name) {
            return self.err_coded(
                span,
                format!(
                    "`instanceof` requires a class or interface name on the right, found `{type_name}`"
                ),
                "E-INSTANCEOF-TYPE",
                Some("only a declared class or interface can be tested with `instanceof`".into()),
            );
        }
        match &v {
            // A poisoned operand already reported its own error; still type the test as `bool`.
            Ty::Error => {}
            // A class instance — or a union (M-RT S4) / intersection (M-RT S5) of them — is the
            // meaningful left operand.
            Ty::Named(..) | Ty::Union(..) | Ty::Intersection(..) => {}
            other => {
                self.err_coded(
                    span,
                    format!("`instanceof` left operand must be a class instance, found `{other}`"),
                    "E-INSTANCEOF-TYPE",
                    Some("`instanceof` tests whether a class instance is of a given class".into()),
                );
            }
        }
        Ty::Bool
    }

    // ---- stubs replaced in later tasks ----
    fn check_str(&mut self, parts: &[crate::ast::StrPart]) -> Ty {
        use crate::ast::StrPart;
        for part in parts {
            if let StrPart::Expr(e) = part {
                let t = self.check_expr(e);
                let ok = matches!(t, Ty::Int | Ty::Float | Ty::Bool | Ty::String | Ty::Error);
                if !ok {
                    let sp = Self::expr_span(e);
                    self.err(sp, format!("type `{t}` cannot be interpolated into a string (only primitives auto-stringify in M1)"));
                }
            }
        }
        Ty::String
    }

    /// Check an `html"…"` literal (core.html Wave 3) and record its type-directed desugaring.
    ///
    /// Each literal chunk becomes `html.raw(chunk)` (author markup is trusted); each `{e}` hole is
    /// resolved **by `e`'s type**: an `Html` value embeds as-is (already safe — lets you nest
    /// builders / other `html"…"`); a `string` is wrapped in `html.text(e)` (auto-escaped — the safe
    /// default for raw data); an `int`/`float`/`bool` is stringified then escaped; anything else is a
    /// clean `E-HTML-HOLE`. The default hole behavior is **escape** — injecting trusted markup
    /// requires writing `{html.raw(x)}` explicitly (unsafe is long, safe is short). The pieces are
    /// concatenated with `html.concat([…])`; the whole tree uses only Wave-1/2 natives, which are
    /// already byte-identical across the three backends, so parity is inherited, not re-proved.
    ///
    /// The replacement is stored by the literal's `Span.start` and applied by [`resolve_html`] after
    /// checking — `check` itself never mutates the AST (it borrows it). Returns [`Ty::Html`].
    fn check_html(&mut self, parts: &[crate::ast::StrPart], span: Span) -> Ty {
        use crate::ast::{Expr, StrPart};
        // `html"…"` desugars to `<leaf>.raw/.text/.concat` calls, so the program must import
        // core.html. Resolve whatever leaf maps to it (robust to `import core.html as h;`).
        let leaf = self
            .imports
            .iter()
            .find(|(_, full)| full.as_str() == "Core.Html")
            .map(|(leaf, _)| leaf.clone());
        let leaf = match leaf {
            Some(l) => l,
            None => {
                return self.err_coded(
                    span,
                    "`html\"…\"` requires the Core.Html module",
                    "E-HTML-IMPORT",
                    Some("add `import Core.Html;` (or `import Core.Html as h;`)".into()),
                );
            }
        };
        // Build `<leaf>.<name>(args)` as a plain `Member`-headed call (resolved like any namespaced
        // native by the backends, via the import map). All synthetic nodes carry the literal's span.
        let call = |name: &str, args: Vec<Expr>| -> Expr {
            Expr::Call {
                callee: Box::new(Expr::Member {
                    object: Box::new(Expr::Ident(leaf.clone(), span)),
                    name: name.to_string(),
                    safe: false,
                    span,
                }),
                args,
                span,
            }
        };
        let str_lit = |s: &str| Expr::Str(vec![StrPart::Literal(s.to_string())], span);

        let mut elems: Vec<Expr> = Vec::with_capacity(parts.len());
        for part in parts {
            match part {
                StrPart::Literal(chunk) => elems.push(call("raw", vec![str_lit(chunk)])),
                StrPart::Expr(e) => {
                    let t = self.check_expr(e);
                    match t {
                        // already an Html fragment — embed verbatim (no double-escape).
                        Ty::Html => elems.push((**e).clone()),
                        // raw text — escape it (the safe default).
                        Ty::String => elems.push(call("text", vec![(**e).clone()])),
                        // primitives stringify (via a one-hole string interp) then escape, for
                        // uniformity — numbers carry no markup but go through the same wall.
                        Ty::Int | Ty::Float | Ty::Bool => {
                            let stringified =
                                Expr::Str(vec![StrPart::Expr(Box::new((**e).clone()))], span);
                            elems.push(call("text", vec![stringified]));
                        }
                        // a poisoned hole already reported its own error; keep going without piling
                        // on, and emit *something* well-typed so the replacement stays buildable.
                        Ty::Error => elems.push(call("text", vec![str_lit("")])),
                        other => {
                            self.err_coded(
                                Self::expr_span(e),
                                format!(
                                    "cannot interpolate `{other}` into html; render it to a string or Html first"
                                ),
                                "E-HTML-HOLE",
                                Some(
                                    "wrap it with `Html.text(…)`/`Html.raw(…)`, or build it with the html builders"
                                        .into(),
                                ),
                            );
                            elems.push(call("text", vec![str_lit("")]));
                        }
                    }
                }
            }
        }

        let replacement = call("concat", vec![Expr::List(elems, span)]);
        self.html_resolutions.insert(span.start, replacement);
        Ty::Html
    }

    /// The source span of an expression (used to position errors precisely).
    fn expr_span(e: &crate::ast::Expr) -> Span {
        use crate::ast::Expr;
        match e {
            Expr::Int(_, s)
            | Expr::Float(_, s)
            | Expr::Bool(_, s)
            | Expr::Str(_, s)
            | Expr::Bytes(_, s)
            | Expr::Ident(_, s)
            | Expr::List(_, s)
            | Expr::Map(_, s) => *s,
            Expr::Null(s) | Expr::This(s) => *s,
            Expr::Unary { span, .. }
            | Expr::Binary { span, .. }
            | Expr::InstanceOf { span, .. }
            | Expr::Call { span, .. }
            | Expr::Member { span, .. }
            | Expr::Index { span, .. }
            | Expr::Force { span, .. }
            | Expr::Propagate { span, .. }
            | Expr::Match { span, .. }
            | Expr::Range { span, .. }
            | Expr::If { span, .. }
            | Expr::Lambda { span, .. }
            | Expr::CloneWith { span, .. }
            | Expr::Html(_, span) => *span,
        }
    }
    fn check_list(&mut self, elems: &[crate::ast::Expr], span: Span) -> Ty {
        if elems.is_empty() {
            // empty list element type cannot be inferred without an expected type;
            // the §6 sample has no empty list (YAGNI to thread expected types now).
            return self.err(span, "cannot infer element type of empty list literal");
        }
        let first = self.check_expr(&elems[0]);
        for e in &elems[1..] {
            let t = self.check_expr(e);
            if !self.ty_assignable(&t, &first) && !self.ty_assignable(&first, &t) {
                self.err(
                    span,
                    format!("list elements must share one type; found `{first}` and `{t}`"),
                );
            }
        }
        Ty::List(Box::new(first))
    }
    /// `[k => v, …]` (M-RT S3): infer the key type `K` and value type `V`, unifying across pairs
    /// (each must share one type, like list elements). The parser guarantees ≥1 pair (an empty `[]`
    /// is the empty *list*). Keys must be the hashable subset — `int`/`bool`/`string` — else
    /// `E-MAP-KEY` (a `float`/instance/list key has no `HKey`). Result: `Ty::Map(K, V)`.
    fn check_map(&mut self, pairs: &[(crate::ast::Expr, crate::ast::Expr)], span: Span) -> Ty {
        let (k0, v0) = &pairs[0];
        let key_ty = self.check_expr(k0);
        let val_ty = self.check_expr(v0);
        for (k, v) in &pairs[1..] {
            let kt = self.check_expr(k);
            if !self.ty_assignable(&kt, &key_ty) && !self.ty_assignable(&key_ty, &kt) {
                self.err(
                    span,
                    format!("map keys must share one type; found `{key_ty}` and `{kt}`"),
                );
            }
            let vt = self.check_expr(v);
            if !self.ty_assignable(&vt, &val_ty) && !self.ty_assignable(&val_ty, &vt) {
                self.err(
                    span,
                    format!("map values must share one type; found `{val_ty}` and `{vt}`"),
                );
            }
        }
        if !matches!(key_ty, Ty::Int | Ty::Bool | Ty::String | Ty::Error) {
            return self.err_coded(
                span,
                format!("map key type must be `int`, `bool`, or `string`, found `{key_ty}`"),
                "E-MAP-KEY",
                None,
            );
        }
        Ty::Map(Box::new(key_ty), Box::new(val_ty))
    }
    fn check_index(
        &mut self,
        object: &crate::ast::Expr,
        index: &crate::ast::Expr,
        span: Span,
    ) -> Ty {
        let obj = self.check_expr(object);
        let idx = self.check_expr(index);
        match obj {
            Ty::List(elem) => {
                if !self.ty_assignable(&idx, &Ty::Int) {
                    self.err(span, format!("list index must be `int`, found `{idx}`"));
                }
                *elem
            }
            // `m[k]` (M-RT S3): the index must match the key type; the result is the value type. A
            // missing key faults at runtime (byte-identical present-key, like list-OOB the fault path
            // is excluded from differential gating).
            Ty::Map(k, v) => {
                if !self.ty_assignable(&idx, &k) {
                    self.err(span, format!("map index must be `{k}`, found `{idx}`"));
                }
                *v
            }
            Ty::Error => Ty::Error,
            other => self.err(span, format!("type `{other}` cannot be indexed")),
        }
    }
    /// `start..end` / `start..=end`: both bounds must be `int`; the range's type is `List<int>` (its
    /// only role this slice is `for … in`). A non-int bound is `E-RANGE-TYPE` (decision S1-R).
    fn check_range(&mut self, start: &crate::ast::Expr, end: &crate::ast::Expr, span: Span) -> Ty {
        let s = self.check_expr(start);
        let e = self.check_expr(end);
        let ok = |t: &Ty| matches!(t, Ty::Int | Ty::Error);
        if !ok(&s) || !ok(&e) {
            return self.err_coded(
                span,
                format!("range bounds must be `int`, found `{s}` and `{e}`"),
                "E-RANGE-TYPE",
                None,
            );
        }
        Ty::List(Box::new(Ty::Int))
    }
    /// Expression `if`: the condition must be `bool` and both arms must share one type `T`, which is
    /// the expression's type. (`else` is mandatory at the parser, so there is no missing-else case
    /// here.) Mirrors `check_match`'s arm-unification rule (M3 S1.3).
    fn check_if_expr(
        &mut self,
        cond: &crate::ast::Expr,
        then_e: &crate::ast::Expr,
        else_e: &crate::ast::Expr,
        span: Span,
    ) -> Ty {
        let c = self.check_expr(cond);
        if !self.ty_assignable(&c, &Ty::Bool) {
            self.err(span, format!("`if` condition must be `bool`, found `{c}`"));
        }
        let t = self.check_expr(then_e);
        let e = self.check_expr(else_e);
        if t != Ty::Error
            && e != Ty::Error
            && !self.ty_assignable(&e, &t)
            && !self.ty_assignable(&t, &e)
        {
            self.err(
                span,
                format!("`if` branches must share one type; found `{t}` and `{e}`"),
            );
        }
        if t == Ty::Error {
            e
        } else {
            t
        }
    }

    /// Type-check a lambda expression (M3 S3, Task 3). Returns `Ty::Function(params, ret)`.
    ///
    /// The checker rejects a lambda that references `this` (F8 / `E-LAMBDA-THIS`): capturing
    /// `this` would create a run↔runvm divergence (the interpreter's `this` vs. the VM's slot 0).
    /// Workaround: `var self = this;` before the lambda captures the value explicitly.
    fn check_lambda(
        &mut self,
        params: &[crate::ast::Param],
        ret: &Option<crate::ast::Type>,
        body: &crate::ast::LambdaBody,
        span: Span,
    ) -> Ty {
        use crate::ast::LambdaBody;
        // F8: reject any lambda that directly references `this` inside its body.
        if lambda_uses_this(body) {
            self.err_coded(
                span,
                "a lambda cannot reference `this` yet",
                "E-LAMBDA-THIS",
                Some("bind `var self = this;` before the lambda and capture `self` instead".into()),
            );
        }
        let param_tys: Vec<Ty> = params.iter().map(|p| self.resolve_type(&p.ty)).collect();
        // Save and replace the current return type (a lambda has its own return scope).
        let saved_ret = std::mem::replace(&mut self.cur_ret, Ty::Error);
        // A lambda is a separate callable: it declares no `throws`, and it does not see the lexical
        // `try` it is written inside (it may be invoked elsewhere — e.g. passed to a native). So a
        // `throw` in its body discharges against an empty context (M-faults 2b).
        let saved_throws = std::mem::take(&mut self.cur_throws);
        let saved_try = std::mem::take(&mut self.try_catch_stack);
        let saved_main = std::mem::replace(&mut self.cur_is_main, false);
        self.push_scope();
        for p in params {
            let pty = self.resolve_type(&p.ty);
            self.declare(&p.name, pty, p.span);
        }
        let ret_ty = match body {
            LambdaBody::Expr(e) => {
                let inferred = self.check_expr(e);
                if let Some(rt) = ret {
                    let declared = self.resolve_type(rt);
                    if !self.ty_assignable(&inferred, &declared) {
                        self.err_assign(span, &inferred, &declared);
                    }
                    declared
                } else {
                    inferred
                }
            }
            LambdaBody::Block(stmts) => {
                // A2/F10: an explicit `-> T` annotation is required for statement-body lambdas.
                match ret {
                    Some(rt) => {
                        let declared = self.resolve_type(rt);
                        self.cur_ret = declared.clone();
                        for s in stmts {
                            self.check_stmt(s);
                        }
                        declared
                    }
                    None => self.err(
                        span,
                        "a statement-body lambda requires an explicit `-> T` return type",
                    ),
                }
            }
        };
        self.pop_scope();
        self.cur_ret = saved_ret;
        self.cur_throws = saved_throws;
        self.try_catch_stack = saved_try;
        self.cur_is_main = saved_main;
        Ty::Function(param_tys, Box::new(ret_ty))
    }

    fn check_call(
        &mut self,
        callee: &crate::ast::Expr,
        args: &[crate::ast::Expr],
        span: Span,
    ) -> Ty {
        use crate::ast::Expr;
        match callee {
            Expr::Ident(name, _) => {
                // Built-in fault intrinsics (M-faults 2a): `panic`/`todo`/`unreachable` (→ `never`) and
                // `assert` (→ `unit`). Recognized here before any user-function lookup; the names are
                // reserved (`E-RESERVED-INTRINSIC`) so this can't be shadowed.
                if let Some(t) = self.check_intrinsic_call(name, args, span) {
                    return t;
                }
                // If the name is a local (or a `match`-arm binding) with function type, treat it
                // as a function-value call rather than a named-function call — the latter only
                // looks in `self.funcs` (top-level declarations) and would report "unknown
                // function `name`" for a lambda-typed local (M3 S3 Task 4).
                if let Some(Ty::Function(param_tys, ret_ty)) = self.lookup(name) {
                    self.check_args("<lambda>", &param_tys, args, span);
                    return *ret_ty;
                }
                self.check_named_call(name, args, span)
            }
            Expr::Member {
                object, name, safe, ..
            } => {
                // Namespaced native call: `console.println(x)` — head is an imported module
                // qualifier. The shadowing guard keeps an imported qualifier disjoint from every
                // value binding, so membership in the import map is decisive (no scope check).
                if !*safe {
                    if let Expr::Ident(q, _) = &**object {
                        if let Some(idx) = self
                            .imports
                            .get(q)
                            .and_then(|m| crate::native::index_of(m, name))
                        {
                            return self.check_native_call(idx, args, span);
                        }
                    }
                }
                self.check_method_call(object, name, args, *safe, span)
            }
            other => {
                // Evaluate the callee to see if it is a function value (closure or named-fn ref).
                let callee_ty = self.check_expr(other);
                match callee_ty {
                    Ty::Function(param_tys, ret_ty) => {
                        self.check_args("<lambda>", &param_tys, args, span);
                        *ret_ty
                    }
                    Ty::Optional(inner) if matches!(*inner, Ty::Function(..)) => {
                        for a in args {
                            self.check_expr(a);
                        }
                        self.err(
                            span,
                            "not callable — the function value is optional; unwrap it first with `??` or `if (var …)`",
                        )
                    }
                    Ty::Error => {
                        for a in args {
                            self.check_expr(a);
                        }
                        Ty::Error
                    }
                    _ => {
                        for a in args {
                            self.check_expr(a);
                        }
                        self.err(span, "expression is not callable")
                    }
                }
            }
        }
    }

    /// `name(args)` — a free function, enum-variant constructor (Task 5), or class
    /// constructor (Task 6). Free-function case here.
    fn check_named_call(&mut self, name: &str, args: &[crate::ast::Expr], span: Span) -> Ty {
        // Consume the throws-mode `?` suppression flag up front (a throwing call under `?` propagates
        // instead of discharging locally). Taken before the variant/ctor probe so it cannot leak —
        // the flag is only ever set for a free throwing function (`free_call_throws`), never a ctor.
        let skip_throws = std::mem::take(&mut self.skip_throws_discharge);
        if let Some(t) = self.try_variant_or_class_call(name, args, span) {
            return t;
        }
        let sigs = match self.funcs.get(name) {
            Some(s) => s.clone(),
            None => {
                for a in args {
                    self.check_expr(a);
                }
                return self.err(span, format!("unknown function `{name}`"));
            }
        };
        // Single overload — the common case, identical to pre-overloading behaviour (incl. generics).
        if sigs.len() == 1 {
            let sig = &sigs[0];
            // Discharge each checked exception the callee declares: a bare call must catch it in an
            // enclosing `try` (M-faults 2b); the propagate (`?`) path used the suppression flag.
            if !skip_throws {
                for e in &sig.throws {
                    self.discharge_call_throw(name, e, span);
                }
            }
            return if sig.type_params.is_empty() {
                self.check_args(name, &sig.params, args, span);
                sig.ret.clone()
            } else {
                self.check_generic_call(name, &sig.params, &sig.ret, args, span)
            };
        }
        // Overload set (M-RT): generic members were rejected at collection, so every overload is
        // monomorphic. The call's result is the shared return type (`E-OVERLOAD-RETURN`); resolution
        // here is *static* (for typing) — the runtime dispatch is byte-identical by construction.
        self.check_overload_call(name, &sigs, args, span, skip_throws)
    }

    /// Resolve a multi-overload free-function call (M-RT). Evaluates the argument types, selects the
    /// statically-matching overloads (arity + assignability), reports `E-OVERLOAD-NO-MATCH` when none
    /// match, discharges the union of the matching overloads' checked exceptions, and returns the
    /// shared return type (all overloads share it by `E-OVERLOAD-RETURN`).
    fn check_overload_call(
        &mut self,
        name: &str,
        sigs: &[FnSig],
        args: &[crate::ast::Expr],
        span: Span,
        skip_throws: bool,
    ) -> Ty {
        let arg_tys: Vec<Ty> = args.iter().map(|a| self.check_expr(a)).collect();
        let matches: Vec<&FnSig> = sigs
            .iter()
            .filter(|s| {
                s.params.len() == arg_tys.len()
                    && s.params
                        .iter()
                        .zip(&arg_tys)
                        .all(|(p, a)| self.ty_assignable(a, p))
            })
            .collect();
        if matches.is_empty() {
            let got = arg_tys
                .iter()
                .map(|t| t.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            return self.err_coded(
                span,
                format!("no overload of `{name}` accepts arguments `({got})`"),
                "E-OVERLOAD-NO-MATCH",
                Some("the argument types must match one overload's parameter types".into()),
            );
        }
        if !skip_throws {
            let mut discharged: Vec<Ty> = Vec::new();
            for m in &matches {
                for e in &m.throws {
                    if !discharged.contains(e) {
                        discharged.push(e.clone());
                        self.discharge_call_throw(name, e, span);
                    }
                }
            }
        }
        matches[0].ret.clone()
    }

    /// Resolve a *method* call against its overload set `applied` (class type-parameters already
    /// substituted in each `(params, ret)`). One overload → the pre-overloading path, including a
    /// method-level generic (`check_generic_call`). Multiple → a static match by arity +
    /// assignability, returning the shared return type (`E-OVERLOAD-RETURN`); none → `E-OVERLOAD-NO-MATCH`.
    fn check_method_sigs(
        &mut self,
        name: &str,
        applied: &[(Vec<Ty>, Ty)],
        args: &[crate::ast::Expr],
        span: Span,
    ) -> Ty {
        if applied.len() == 1 {
            let (params, ret) = &applied[0];
            return if params.iter().any(ty_has_param) || ty_has_param(ret) {
                self.check_generic_call(name, params, ret, args, span)
            } else {
                self.check_args(name, params, args, span);
                ret.clone()
            };
        }
        let arg_tys: Vec<Ty> = args.iter().map(|a| self.check_expr(a)).collect();
        let matched = applied.iter().find(|(params, _)| {
            params.len() == arg_tys.len()
                && params
                    .iter()
                    .zip(&arg_tys)
                    .all(|(p, a)| self.ty_assignable(a, p))
        });
        match matched {
            Some((_, ret)) => ret.clone(),
            None => {
                let got = arg_tys
                    .iter()
                    .map(|t| t.to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                self.err_coded(
                    span,
                    format!("no overload of method `{name}` accepts arguments `({got})`"),
                    "E-OVERLOAD-NO-MATCH",
                    Some("the argument types must match one overload's parameter types".into()),
                )
            }
        }
    }

    /// Discharge one checked exception `e` a called function `name` may throw at a *bare* (non-`?`)
    /// call site: it must be caught by an enclosing `try`, else `E-CALL-UNHANDLED`. Propagation is
    /// the `?` path ([`Self::try_throws_propagate`]) — a bare call may not silently propagate.
    fn discharge_call_throw(&mut self, name: &str, e: &Ty, span: Span) {
        if self.covered_by_try(e) {
            return;
        }
        self.err_coded(
            span,
            format!("call to `{name}` can throw `{e}`, which is not handled here"),
            "E-CALL-UNHANDLED",
            Some(format!(
                "wrap the call in `try {{ … }} catch ({e} e) {{ … }}`, or propagate it with `?` and declare `throws {e}`"
            )),
        );
    }

    /// Check a call to a *generic* function (M-RT S7). Unifies each declared parameter type (which
    /// contains `Ty::Param` occurrences) against the inferred argument type to build a substitution
    /// `θ`, then applies `θ` to the declared return type. First-binding-wins, structural; `θ` lives
    /// only here and never touches the AST (the function's type params are erased separately, before
    /// any backend). A unification failure is a normal argument-type error.
    fn check_generic_call(
        &mut self,
        name: &str,
        params: &[Ty],
        ret: &Ty,
        args: &[crate::ast::Expr],
        span: Span,
    ) -> Ty {
        if params.len() != args.len() {
            self.err(
                span,
                format!(
                    "`{name}` expects {} argument(s), found {}",
                    params.len(),
                    args.len()
                ),
            );
            for a in args {
                self.check_expr(a);
            }
            return Ty::Error;
        }
        let mut theta: HashMap<String, Ty> = HashMap::new();
        let mut ok = true;
        for (i, (param, arg)) in params.iter().zip(args).enumerate() {
            let at = self.check_arg(arg, param);
            if !self.unify(param, &at, &mut theta) {
                ok = false;
                let want = apply_subst(param, &theta);
                self.err(
                    span,
                    format!("`{name}` argument {} expects `{want}`, found `{at}`", i + 1),
                );
            }
        }
        if !ok {
            return Ty::Error;
        }
        apply_subst(ret, &theta)
    }

    /// Structural unification of a declared type (possibly containing `Ty::Param`) against a concrete
    /// argument type, accumulating bindings in `θ`. Returns false on a mismatch. A parameter binds
    /// the first concrete type it meets; a later occurrence must be *consistent* (assignable either
    /// way, so subtyping is tolerated). A non-parameter position falls back to ordinary
    /// assignability. `Ty::Error` (poison) unifies with anything (M-RT S7).
    fn unify(&self, declared: &Ty, actual: &Ty, theta: &mut HashMap<String, Ty>) -> bool {
        if matches!(declared, Ty::Error) || matches!(actual, Ty::Error) {
            return true;
        }
        match (declared, actual) {
            (Ty::Param(p), a) => match theta.get(p) {
                None => {
                    theta.insert(p.clone(), a.clone());
                    true
                }
                Some(bound) => self.ty_assignable(a, bound) || self.ty_assignable(bound, a),
            },
            (Ty::List(d), Ty::List(a)) | (Ty::Set(d), Ty::Set(a)) => self.unify(d, a, theta),
            (Ty::Optional(d), Ty::Optional(a)) => self.unify(d, a, theta),
            (Ty::Map(dk, dv), Ty::Map(ak, av)) => {
                self.unify(dk, ak, theta) && self.unify(dv, av, theta)
            }
            (Ty::Function(dp, dr), Ty::Function(ap, ar)) => {
                dp.len() == ap.len()
                    && dp.iter().zip(ap).all(|(d, a)| self.unify(d, a, theta))
                    && self.unify(dr, ar, theta)
            }
            // Two generic class instances with the same head — unify their arguments so a generic
            // function over a generic class (`function unwrap<T>(Box<T> b) -> T`) binds `T` from a
            // `Box<int>` argument (M-RT generics-all). Different heads fall through to assignability.
            (Ty::Named(dn, da), Ty::Named(an, aa)) if dn == an && da.len() == aa.len() => {
                da.iter().zip(aa).all(|(d, a)| self.unify(d, a, theta))
            }
            // No type parameter at this position — ordinary assignability (actual → declared).
            (d, a) => self.ty_assignable(a, d),
        }
    }

    /// `console.println(args)` — a namespaced native call resolved through the import map (M3
    /// Wave 1). The native single-sources its signature, so checking is the same arg/arity pass as a
    /// free function; the leaf-qualified label (`console.println`) drives the error messages.
    fn check_native_call(&mut self, idx: usize, args: &[crate::ast::Expr], span: Span) -> Ty {
        let n = &crate::native::registry()[idx];
        let leaf = n.module.rsplit('.').next().unwrap_or(n.module);
        let label = format!("{leaf}.{}", n.name);
        // A native whose stored signature carries a type parameter (`Map.keys(Map<K,V>) -> List<K>`,
        // `List.reverse(List<T>) -> List<T>`) is checked exactly like a generic free function: unify
        // the declared params against the argument types, then substitute into the return (M-RT S7b).
        // `θ` lives only in `check_generic_call`; the native's `Ty::Param` is registry-only and never
        // reaches a backend (the compiler types a native call by expression shape → `CTy::Other`, and
        // the transpiler emits via the `php` closure). `n` borrows the `'static` registry, so passing
        // `&n.params`/`&n.ret` alongside `&mut self` does not alias.
        if n.params.iter().any(ty_has_param) || ty_has_param(&n.ret) {
            self.check_generic_call(&label, &n.params, &n.ret, args, span)
        } else {
            self.check_args(&label, &n.params, args, span);
            n.ret.clone()
        }
    }

    /// Check a single call argument against its expected parameter type. Identical to `check_expr`
    /// except that an **empty list literal** `[]` — which has no element to infer a type from —
    /// adopts the expected `List<T>` element type instead of erroring with "cannot infer element
    /// type". This is the one place an expected type is threaded into expression checking
    /// (bidirectional, call-argument-only by design); an empty `[]` in any other position (a
    /// declaration initializer, a `return`) still requires a non-empty literal. It lets the
    /// zero-attribute / zero-child HTML builders read naturally — `el("p", [], [text("hi")])`.
    fn check_arg(&mut self, arg: &crate::ast::Expr, expected: &Ty) -> Ty {
        if let crate::ast::Expr::List(elems, _) = arg {
            if elems.is_empty() {
                if let Ty::List(inner) = expected {
                    return Ty::List(inner.clone());
                }
            }
        }
        self.check_expr(arg)
    }

    /// Check call arguments against expected parameter types.
    fn check_args(&mut self, name: &str, params: &[Ty], args: &[crate::ast::Expr], span: Span) {
        if params.len() != args.len() {
            self.err(
                span,
                format!(
                    "`{name}` expects {} argument(s), found {}",
                    params.len(),
                    args.len()
                ),
            );
            for a in args {
                self.check_expr(a);
            }
            return;
        }
        for (i, (param, arg)) in params.iter().zip(args).enumerate() {
            let at = self.check_arg(arg, param);
            if !self.ty_assignable(&at, param) {
                self.err(
                    span,
                    format!(
                        "`{name}` argument {} expects `{param}`, found `{at}`",
                        i + 1
                    ),
                );
            }
        }
    }

    /// Returns `Some(ret)` if `name` is an enum variant or class constructor.
    fn try_variant_or_class_call(
        &mut self,
        name: &str,
        args: &[crate::ast::Expr],
        span: Span,
    ) -> Option<Ty> {
        // enum variant constructor: find the (unique) enum that owns this variant name
        let owner = self
            .enums
            .iter()
            .find(|(_, info)| info.variants.contains_key(name))
            .map(|(enum_name, info)| {
                (
                    enum_name.clone(),
                    info.variants[name].clone(),
                    info.type_params.clone(),
                )
            });
        if let Some((enum_name, fields, type_params)) = owner {
            if type_params.is_empty() {
                self.check_args(name, &fields, args, span);
                return Some(Ty::Named(enum_name, Vec::new()));
            }
            // A generic enum (`Option<T>`/`Result<T, E>`): infer its type arguments from the variant
            // constructor call (M-RT generic enums), the same first-binding-wins unifier as a generic
            // class constructor. A type parameter a variant does not mention (`None` for `Option<T>`,
            // `Ok(T)` w.r.t. `E`) stays un-inferred and defaults to `Ty::Error` (permissive — the call
            // site annotates the binding to fix it, e.g. `Option<int> n = None();`).
            if fields.len() != args.len() {
                self.err(
                    span,
                    format!(
                        "variant `{name}` expects {} argument(s), found {}",
                        fields.len(),
                        args.len()
                    ),
                );
                for a in args {
                    self.check_expr(a);
                }
                return Some(Ty::Named(enum_name, vec![Ty::Error; type_params.len()]));
            }
            let mut theta: HashMap<String, Ty> = HashMap::new();
            for (field, arg) in fields.iter().zip(args) {
                let at = self.check_arg(arg, field);
                if !self.unify(field, &at, &mut theta) {
                    let want = apply_subst(field, &theta);
                    self.err(
                        span,
                        format!("variant `{name}` expects `{want}`, found `{at}`"),
                    );
                }
            }
            let inst_args = type_params
                .iter()
                .map(|p| theta.get(p).cloned().unwrap_or(Ty::Error))
                .collect();
            return Some(Ty::Named(enum_name, inst_args));
        }
        // class constructor: `ClassName(args)`
        if let Some(info) = self.classes.get(name) {
            let ctor = info.ctor.clone();
            let type_params = info.type_params.clone();
            let is_abstract = info.is_abstract;
            // M-RT S6b: an abstract class has unimplemented methods and cannot be instantiated.
            if is_abstract {
                self.err_coded(
                    span,
                    format!("cannot instantiate abstract class `{name}`"),
                    "E-ABSTRACT-INSTANTIATE",
                    Some(format!(
                        "`{name}` is `abstract`; instantiate a concrete subclass that implements its \
                         abstract methods"
                    )),
                );
            }
            if type_params.is_empty() {
                self.check_args(name, &ctor, args, span);
                return Some(Ty::Named(name.to_string(), Vec::new()));
            }
            // A generic class: infer its type arguments from the constructor call (M-RT generics-all),
            // the same first-binding-wins unifier as a generic function. A parameter the constructor
            // does not mention stays un-inferred and defaults to `Ty::Error` (permissive).
            if ctor.len() != args.len() {
                self.err(
                    span,
                    format!(
                        "`{name}` expects {} argument(s), found {}",
                        ctor.len(),
                        args.len()
                    ),
                );
                for a in args {
                    self.check_expr(a);
                }
                return Some(Ty::Named(
                    name.to_string(),
                    vec![Ty::Error; type_params.len()],
                ));
            }
            let mut theta: HashMap<String, Ty> = HashMap::new();
            for (param, arg) in ctor.iter().zip(args) {
                let at = self.check_arg(arg, param);
                if !self.unify(param, &at, &mut theta) {
                    let want = apply_subst(param, &theta);
                    self.err(
                        span,
                        format!("`{name}` constructor expects `{want}`, found `{at}`"),
                    );
                }
            }
            let inst_args = type_params
                .iter()
                .map(|p| theta.get(p).cloned().unwrap_or(Ty::Error))
                .collect();
            return Some(Ty::Named(name.to_string(), inst_args));
        }
        None
    }

    fn check_method_call(
        &mut self,
        object: &crate::ast::Expr,
        name: &str,
        args: &[crate::ast::Expr],
        safe: bool,
        span: Span,
    ) -> Ty {
        let obj = self.check_expr(object);
        // Peel an optional/null receiver, enforcing the non-null discipline: a plain `.m()` on a
        // `T?` is `E-OPT-USE`; `?.m()` unwraps and re-wraps the result as optional (M3 S2.3).
        let base = match &obj {
            Ty::Error => {
                for a in args {
                    self.check_expr(a);
                }
                return Ty::Error;
            }
            Ty::Null if safe => {
                for a in args {
                    self.check_expr(a);
                }
                return Ty::Null; // `null?.m()` short-circuits to null
            }
            Ty::Optional(_) | Ty::Null if !safe => {
                for a in args {
                    self.check_expr(a);
                }
                return self.err_opt_use(span, name, &obj, "call method");
            }
            Ty::Optional(inner) => (**inner).clone(),
            other => other.clone(),
        };
        let ret = match base {
            Ty::Named(cls, cargs) => {
                // A class method, or — when `cls` is an interface (M-RT S2) — an interface method
                // from its flattened (own + `extends`) signature set. Interface-typed receivers
                // dispatch polymorphically at runtime through the concrete class, so only the static
                // signature is needed here.
                // The method's overload set (M-RT): one or more signatures sharing a return type. An
                // interface method (no overloading) contributes a single signature.
                let sigs = self
                    .classes
                    .get(&cls)
                    .and_then(|info| info.methods.get(name))
                    .map(|v| {
                        v.iter()
                            .map(|s| (s.params.clone(), s.ret.clone()))
                            .collect::<Vec<_>>()
                    })
                    .or_else(|| {
                        if self.interfaces.contains_key(&cls) {
                            self.iface_flat_methods(&cls)
                                .into_iter()
                                .find(|(m, _)| m == name)
                                .map(|(_, sig)| vec![sig])
                        } else {
                            None
                        }
                    });
                // Substitute the *class* type parameters with this instance's type arguments
                // (`Box<int>` ⇒ `{T → int}`), so a method returning/taking `T` is checked at the
                // concrete type (M-RT generics-all). Empty for a non-generic class/interface, so this
                // is the identity in the common case. Any *method-level* `<U>` that survives is then
                // inferred from the call's arguments below.
                let theta = self.class_subst(&cls, &cargs);
                match sigs {
                    Some(sigs) => {
                        let applied: Vec<(Vec<Ty>, Ty)> = sigs
                            .iter()
                            .map(|(ps, r)| {
                                (
                                    ps.iter().map(|p| apply_subst(p, &theta)).collect(),
                                    apply_subst(r, &theta),
                                )
                            })
                            .collect();
                        self.check_method_sigs(name, &applied, args, span)
                    }
                    None => {
                        for a in args {
                            self.check_expr(a);
                        }
                        self.err(span, format!("type `{cls}` has no method `{name}`"))
                    }
                }
            }
            Ty::Intersection(members) => {
                // Member access over an intersection (M-RT S5): search each member (an interface, or
                // the lone class) for `name`, resolving from the *first* member that declares it; a
                // method present in two members agrees on its signature (E-INTERSECT-SIG at the type
                // site), so first-found is unambiguous. None → E-INTERSECT-NO-MEMBER. The value is a
                // concrete instance underneath, so dispatch is polymorphic at runtime — no Op change.
                let mut found: Option<Vec<(Vec<Ty>, Ty)>> = None;
                for m in &members {
                    if let Ty::Named(mn, margs) = m {
                        let sig = self
                            .classes
                            .get(mn)
                            .and_then(|info| info.methods.get(name))
                            .map(|v| {
                                v.iter()
                                    .map(|s| (s.params.clone(), s.ret.clone()))
                                    .collect::<Vec<_>>()
                            })
                            .or_else(|| {
                                if self.interfaces.contains_key(mn) {
                                    self.iface_flat_methods(mn)
                                        .into_iter()
                                        .find(|(mm, _)| mm == name)
                                        .map(|(_, sig)| vec![sig])
                                } else {
                                    None
                                }
                            });
                        if let Some(sigs) = sig {
                            let theta = self.class_subst(mn, margs);
                            found = Some(
                                sigs.iter()
                                    .map(|(ps, r)| {
                                        (
                                            ps.iter().map(|p| apply_subst(p, &theta)).collect(),
                                            apply_subst(r, &theta),
                                        )
                                    })
                                    .collect(),
                            );
                            break;
                        }
                    }
                }
                match found {
                    Some(applied) => self.check_method_sigs(name, &applied, args, span),
                    None => {
                        for a in args {
                            self.check_expr(a);
                        }
                        self.err_coded(
                            span,
                            format!(
                                "no member of `{}` has method `{name}`",
                                Ty::Intersection(members)
                            ),
                            "E-INTERSECT-NO-MEMBER",
                            None,
                        )
                    }
                }
            }
            Ty::Error => Ty::Error,
            other => {
                for a in args {
                    self.check_expr(a);
                }
                self.err(span, format!("type `{other}` has no method `{name}`"))
            }
        };
        if safe {
            Self::opt_wrap(ret)
        } else {
            ret
        }
    }
    fn check_member(
        &mut self,
        object: &crate::ast::Expr,
        name: &str,
        safe: bool,
        span: Span,
    ) -> Ty {
        // Static field read `ClassName.field` (M-mut.7): the head is a class *name* not shadowed by a
        // local (locals-first), and `?.` makes no sense on a class. Resolved before `check_expr`,
        // which would otherwise reject the bare class name as an unknown variable.
        if !safe {
            if let crate::ast::Expr::Ident(cls, _) = object {
                if self.lookup_binding(cls).is_none() && self.classes.contains_key(cls) {
                    return match self.classes[cls].statics.get(name).cloned() {
                        Some(t) => t,
                        None => self.err_coded(
                            span,
                            format!("`{cls}` has no static field `{name}`"),
                            "E-STATIC-UNKNOWN",
                            Some(
                                "static fields are declared `static …` and read as `Class.field`"
                                    .into(),
                            ),
                        ),
                    };
                }
            }
        }
        let obj = self.check_expr(object);
        // Peel an optional/null receiver, enforcing the non-null discipline: a plain `.field` on a
        // `T?` is `E-OPT-USE`; `?.field` unwraps and re-wraps the result as optional (M3 S2.3).
        let base = match &obj {
            Ty::Error => return Ty::Error,
            Ty::Null if safe => return Ty::Null, // `null?.field` short-circuits to null
            Ty::Optional(_) | Ty::Null if !safe => {
                return self.err_opt_use(span, name, &obj, "read field");
            }
            Ty::Optional(inner) => (**inner).clone(),
            other => other.clone(),
        };
        let field_ty = match base {
            Ty::Named(cls, cargs) => {
                // A property hook (M-mut.7b) is resolved before a stored field: `o.name` runs its
                // `get`. Reading a hook with no `get` (write-only) is `E-HOOK-NO-GET`. A hook is not
                // generic (`package Main` only), so no substitution applies to its type.
                if let Some(h) = self.classes.get(&cls).and_then(|info| info.hooks.get(name)) {
                    let (hty, has_get) = (h.ty.clone(), h.has_get);
                    if !has_get {
                        return self.err_coded(
                            span,
                            format!("property `{name}` of `{cls}` is write-only (no `get`)"),
                            "E-HOOK-NO-GET",
                            Some("add a `get => …;` clause to read it".into()),
                        );
                    }
                    return if safe { Self::opt_wrap(hty) } else { hty };
                }
                let found = self
                    .classes
                    .get(&cls)
                    .and_then(|info| info.fields.get(name).cloned());
                match found {
                    // Substitute the class type parameters with the instance's type arguments, so a
                    // `T` field reads at the concrete type (`Box<int>().value : int`) — identity for a
                    // non-generic class (M-RT generics-all).
                    Some(t) => apply_subst(&t, &self.class_subst(&cls, &cargs)),
                    None => self.err(span, format!("type `{cls}` has no field `{name}`")),
                }
            }
            Ty::Intersection(members) => {
                // Only the lone class member can carry fields (interfaces have none, M-RT S5). Search
                // for the field on the class member; none → E-INTERSECT-NO-MEMBER.
                let mut found: Option<Ty> = None;
                for m in &members {
                    if let Ty::Named(mn, margs) = m {
                        if let Some(t) = self
                            .classes
                            .get(mn)
                            .and_then(|info| info.fields.get(name).cloned())
                        {
                            found = Some(apply_subst(&t, &self.class_subst(mn, margs)));
                            break;
                        }
                    }
                }
                match found {
                    Some(t) => t,
                    None => self.err_coded(
                        span,
                        format!(
                            "no member of `{}` has field `{name}`",
                            Ty::Intersection(members)
                        ),
                        "E-INTERSECT-NO-MEMBER",
                        None,
                    ),
                }
            }
            Ty::Error => Ty::Error,
            other => self.err(span, format!("type `{other}` has no field `{name}`")),
        };
        if safe {
            Self::opt_wrap(field_ty)
        } else {
            field_ty
        }
    }

    /// Build the substitution mapping a generic class's type parameters to a concrete instance's type
    /// arguments — `{T → int}` for a `Box<int>` receiver (M-RT generics-all). Empty (the identity
    /// substitution) for a non-generic class or any non-class name, so member/method access on a
    /// non-generic type is unchanged. `zip` tolerates an arity mismatch defensively.
    fn class_subst(&self, cls: &str, cargs: &[Ty]) -> HashMap<String, Ty> {
        match self.classes.get(cls) {
            Some(info) => info
                .type_params
                .iter()
                .cloned()
                .zip(cargs.iter().cloned())
                .collect(),
            None => HashMap::new(),
        }
    }

    /// The substitution mapping a generic enum's type parameters to a scrutinee's type arguments
    /// (`Option<int>` ⇒ `{T → int}`), so a `match` binds a variant payload at the concrete type
    /// (`Some(n)` ⇒ `n: int`). Empty for a non-generic enum, so it is the identity in the common case
    /// (M-RT generic enums). Mirror of [`class_subst`].
    fn enum_subst(&self, enum_name: &str, eargs: &[Ty]) -> HashMap<String, Ty> {
        match self.enums.get(enum_name) {
            Some(info) => info
                .type_params
                .iter()
                .cloned()
                .zip(eargs.iter().cloned())
                .collect(),
            None => HashMap::new(),
        }
    }
    /// `name = value` local reassignment (M-mut.1): the binding must exist and be `mutable`, and the
    /// value assignable to its type.
    fn check_local_reassign(
        &mut self,
        name: &str,
        vty: &Ty,
        target: &crate::ast::Expr,
        value: &crate::ast::Expr,
    ) {
        match self.lookup_binding(name) {
            None => {
                self.err_coded(
                    Self::expr_span(target),
                    format!("cannot assign to unknown variable `{name}`"),
                    "E-ASSIGN-UNKNOWN",
                    None,
                );
            }
            Some((bty, false)) => {
                self.err_coded(
                    Self::expr_span(target),
                    format!("`{name}` is immutable and cannot be reassigned"),
                    "E-ASSIGN-IMMUTABLE",
                    Some(format!(
                        "declare it `mutable` (e.g. `mutable {bty} {name} = …;`)"
                    )),
                );
            }
            Some((bty, true)) => {
                if !self.ty_assignable(vty, &bty) {
                    self.err_coded(
                        Self::expr_span(value),
                        format!("cannot assign `{vty}` to `{name}: {bty}`"),
                        "E-ASSIGN-TYPE",
                        None,
                    );
                }
            }
        }
    }

    /// `container[index] = value` value-type element set (M-mut.5). The container must be a `mutable`
    /// local `List<T>` or `Map<K, V>` (nested places `a[i][j]`/`this.f[i]` are a later slice →
    /// `E-ASSIGN-TARGET`). For a list the index must be `int` and the value a `T`; for a map the
    /// index must be the key type `K` and the value a `V`.
    fn check_index_assign(
        &mut self,
        object: &crate::ast::Expr,
        index: &crate::ast::Expr,
        vty: &Ty,
        value: &crate::ast::Expr,
        span: Span,
    ) {
        let ity = self.check_expr(index);
        let name = match object {
            crate::ast::Expr::Ident(n, _) => n.clone(),
            _ => {
                self.err_coded(
                    Self::expr_span(object),
                    "the container of an element assignment must be a simple variable",
                    "E-ASSIGN-TARGET",
                    Some("nested element/field assignment (`a[i][j]`, `this.f[i]`) lands in a later slice".into()),
                );
                return;
            }
        };
        let (cty, mutable) = match self.lookup_binding(&name) {
            Some(b) => b,
            None => {
                self.err_coded(
                    Self::expr_span(object),
                    format!("cannot assign into unknown variable `{name}`"),
                    "E-ASSIGN-UNKNOWN",
                    None,
                );
                return;
            }
        };
        if !mutable {
            self.err_coded(
                Self::expr_span(object),
                format!("`{name}` is immutable; its elements cannot be set"),
                "E-ASSIGN-IMMUTABLE",
                Some(format!(
                    "declare it `mutable` (e.g. `mutable {cty} {name} = …;`)"
                )),
            );
            return;
        }
        match cty {
            Ty::List(elem) => {
                if !self.ty_assignable(&ity, &Ty::Int) {
                    self.err(span, format!("list index must be `int`, found `{ity}`"));
                }
                if !self.ty_assignable(vty, &elem) {
                    self.err_coded(
                        Self::expr_span(value),
                        format!("cannot set a `{vty}` element into `{name}: List<{elem}>`"),
                        "E-ASSIGN-TYPE",
                        None,
                    );
                }
            }
            Ty::Map(k, v) => {
                if !self.ty_assignable(&ity, &k) {
                    self.err(span, format!("map key must be `{k}`, found `{ity}`"));
                }
                if !self.ty_assignable(vty, &v) {
                    self.err_coded(
                        Self::expr_span(value),
                        format!("cannot set a `{vty}` value into `{name}: Map<{k}, {v}>`"),
                        "E-ASSIGN-TYPE",
                        None,
                    );
                }
            }
            other => {
                self.err_coded(
                    Self::expr_span(object),
                    format!("`{name}: {other}` is not indexable for assignment"),
                    "E-ASSIGN-TARGET",
                    Some("only `List<T>` and `Map<K, V>` support `container[i] = e`".into()),
                );
            }
        }
    }

    /// `o.f = e` / `this.f = e` shared-mutable instance field set (M-mut.6). The object must resolve
    /// to a concrete class (`this`, a local, or any field path `a.b` whose type is a class — handle
    /// semantics make a write through any binding visible everywhere); the field must exist
    /// (`E-ASSIGN-UNKNOWN`), be declared `mutable` (`E-ASSIGN-IMMUTABLE`), and the value must be
    /// assignable to its (generics-substituted) type (`E-ASSIGN-TYPE`). A `?.` target is rejected
    /// (`E-ASSIGN-TARGET`); nested index-into-field (`this.f[i] = e`) stays deferred to a later slice.
    fn check_field_assign(
        &mut self,
        object: &crate::ast::Expr,
        name: &str,
        safe: bool,
        vty: &Ty,
        value: &crate::ast::Expr,
        span: Span,
    ) {
        if safe {
            self.err_coded(
                span,
                "cannot assign through a `?.` safe-access target",
                "E-ASSIGN-TARGET",
                Some("write `o.f = e` on a non-optional receiver".into()),
            );
            return;
        }
        // Static field write `ClassName.field = e` (M-mut.7): the head is a class name (not a local).
        // The field must be a `static mutable` of that class.
        if let crate::ast::Expr::Ident(cls, _) = object {
            if self.lookup_binding(cls).is_none() && self.classes.contains_key(cls) {
                let info = &self.classes[cls];
                match info.statics.get(name).cloned() {
                    None => {
                        self.err_coded(
                            span,
                            format!("`{cls}` has no static field `{name}` to assign"),
                            "E-ASSIGN-UNKNOWN",
                            None,
                        );
                    }
                    Some(fty) => {
                        let is_mut = info.static_mut.contains(name);
                        if !is_mut {
                            self.err_coded(
                                span,
                                format!("static field `{name}` of `{cls}` is immutable and cannot be assigned"),
                                "E-ASSIGN-IMMUTABLE",
                                Some(format!("declare it `static mutable {fty} {name} = …;`")),
                            );
                        } else if !self.ty_assignable(vty, &fty) {
                            self.err_coded(
                                Self::expr_span(value),
                                format!("cannot assign `{vty}` to static field `{name}: {fty}`"),
                                "E-ASSIGN-TYPE",
                                None,
                            );
                        }
                    }
                }
                return;
            }
        }
        let obj_ty = self.check_expr(object);
        let (class, cargs) = match &obj_ty {
            Ty::Error => return,
            Ty::Named(n, cargs) if self.classes.contains_key(n) => (n.clone(), cargs.clone()),
            other => {
                self.err_coded(
                    Self::expr_span(object),
                    format!("cannot set field `{name}` on non-class `{other}`"),
                    "E-ASSIGN-TARGET",
                    Some("field assignment requires a class instance".into()),
                );
                return;
            }
        };
        // A property hook (M-mut.7b) is resolved before a stored field: `o.name = e` runs its
        // `set`. Writing a hook with no `set` (read-only computed) is `E-HOOK-NO-SET`; otherwise the
        // value must be assignable to the hook's type. A hook is never `mutable`-gated (it has no
        // storage); the set body decides what to mutate.
        if let Some(h) = self
            .classes
            .get(&class)
            .and_then(|info| info.hooks.get(name))
        {
            let (hty, has_set) = (h.ty.clone(), h.has_set);
            if !has_set {
                self.err_coded(
                    span,
                    format!("property `{name}` of `{class}` is read-only (no `set`)"),
                    "E-HOOK-NO-SET",
                    Some("add a `set(T v) { … }` clause to assign it".into()),
                );
            } else if !self.ty_assignable(vty, &hty) {
                self.err_coded(
                    Self::expr_span(value),
                    format!("cannot assign `{vty}` to property `{name}: {hty}`"),
                    "E-ASSIGN-TYPE",
                    None,
                );
            }
            return;
        }
        let fty = match self.classes[&class].fields.get(name).cloned() {
            Some(t) => apply_subst(&t, &self.class_subst(&class, &cargs)),
            None => {
                self.err_coded(
                    span,
                    format!("`{class}` has no field `{name}` to assign"),
                    "E-ASSIGN-UNKNOWN",
                    None,
                );
                return;
            }
        };
        if !self.classes[&class].mutable_fields.contains(name) {
            self.err_coded(
                span,
                format!("field `{name}` of `{class}` is immutable and cannot be assigned"),
                "E-ASSIGN-IMMUTABLE",
                Some(format!(
                    "declare it `mutable` (e.g. `mutable {fty} {name};`)"
                )),
            );
            return;
        }
        if !self.ty_assignable(vty, &fty) {
            self.err_coded(
                Self::expr_span(value),
                format!("cannot assign `{vty}` to field `{name}: {fty}`"),
                "E-ASSIGN-TYPE",
                None,
            );
        }
    }

    /// `obj with { f = e, … }` (M-mut.4a): `obj` must be a concrete class; each overridden name must
    /// be one of its fields and each value assignable to that field's type. The result type is the
    /// class itself (a fresh instance). Codes `E-WITH-NONCLASS`/`E-WITH-FIELD`/`E-WITH-TYPE`.
    fn check_clone_with(
        &mut self,
        object: &crate::ast::Expr,
        fields: &[(String, crate::ast::Expr)],
        span: Span,
    ) -> Ty {
        let obj_ty = self.check_expr(object);
        // Always check the override value expressions (surface nested errors regardless).
        let value_tys: Vec<Ty> = fields.iter().map(|(_, e)| self.check_expr(e)).collect();
        let class = match &obj_ty {
            Ty::Error => return Ty::Error,
            Ty::Named(name, _) if self.classes.contains_key(name) => name.clone(),
            other => {
                return self.err_coded(
                    span,
                    format!("`with` requires a class instance, found `{other}`"),
                    "E-WITH-NONCLASS",
                    Some(
                        "`with` produces a copy of a class instance with some fields replaced"
                            .into(),
                    ),
                );
            }
        };
        // Snapshot the class's field types (clone to drop the borrow before `err_coded` needs &mut).
        let field_tys = self.classes[&class].fields.clone();
        for ((name, _), vty) in fields.iter().zip(value_tys.iter()) {
            match field_tys.get(name) {
                None => {
                    self.err_coded(
                        Self::expr_span(object),
                        format!("`{class}` has no field `{name}` to set in `with`"),
                        "E-WITH-FIELD",
                        None,
                    );
                }
                Some(fty) => {
                    if !self.ty_assignable(vty, fty) {
                        self.err_coded(
                            span,
                            format!("cannot set `{name}: {fty}` to `{vty}` in `with`"),
                            "E-WITH-TYPE",
                            None,
                        );
                    }
                }
            }
        }
        obj_ty
    }
    /// `opt!` checked force-unwrap (M3 S2.5): `T?` → `T`. Every use is linted (`W-FORCE-UNWRAP`) to
    /// nudge toward `??`/`?.`/if-let; force-unwrapping a non-optional is `E-OPT-UNWRAP`.
    /// Is `name` a `Result`-shaped enum — exactly an `Ok` variant (arity 1) and an `Err` variant
    /// (arity 1)? `?` propagation is defined only over this shape (M-faults 2a).
    fn is_result_enum(&self, name: &str) -> bool {
        self.enums.get(name).is_some_and(|e| {
            e.variants.get("Ok").is_some_and(|f| f.len() == 1)
                && e.variants.get("Err").is_some_and(|f| f.len() == 1)
        })
    }

    /// The type of variant `v`'s single payload field on enum `name`, with the enum's type parameters
    /// substituted by `args` (`Ok` payload of `Result<int, _>` ⇒ `int`).
    fn result_payload(&self, name: &str, v: &str, args: &[Ty]) -> Ty {
        let theta = self.enum_subst(name, args);
        self.enums[name].variants[v]
            .first()
            .map_or(Ty::Error, |t| apply_subst(t, &theta))
    }

    /// `expr?` — Result-error propagation (M-faults 2a). Unwraps an `Ok` payload to its value, or
    /// early-returns the `Err` from the enclosing function. Requires the operand to be a `Result`-shaped
    /// enum AND the enclosing function (`cur_ret`) to return that *same* enum, with the operand's `Err`
    /// payload assignable to the function's (`E-PROPAGATE-CONTEXT`/`E-PROPAGATE-ERR`). Returns the
    /// unwrapped `Ok` payload type. Called only from a let-initializer; any other position is rejected
    /// by the `Expr::Propagate` arm in `check_expr` (`E-PROPAGATE-POSITION`).
    fn check_propagate(&mut self, inner: &crate::ast::Expr, span: Span) -> Ty {
        let t = self.check_expr(inner);
        let (name, args) = match &t {
            Ty::Error => return Ty::Error,
            Ty::Named(n, a) if self.is_result_enum(n) => (n.clone(), a.clone()),
            other => {
                return self.err_coded(
                    span,
                    format!("`?` requires a `Result`-shaped operand (an enum with `Ok`/`Err` variants), found `{other}`"),
                    "E-PROPAGATE-CONTEXT",
                    Some("`?` unwraps `Ok` or early-returns `Err`".into()),
                );
            }
        };
        match self.cur_ret.clone() {
            Ty::Named(rn, rargs) if rn == name => {
                let err_in = self.result_payload(&name, "Err", &args);
                let err_ret = self.result_payload(&rn, "Err", &rargs);
                if !self.ty_assignable(&err_in, &err_ret) {
                    self.err_coded(
                        span,
                        format!("`?` propagates an `Err({err_in})` but the enclosing `{name}` carries `Err({err_ret})`"),
                        "E-PROPAGATE-ERR",
                        None,
                    );
                }
                self.result_payload(&name, "Ok", &args)
            }
            other => self.err_coded(
                span,
                format!("`?` early-returns the `Err`, so the enclosing function must return `{name}<…>`, but it returns `{other}`"),
                "E-PROPAGATE-CONTEXT",
                Some(format!("declare the function to return `{name}<…>`")),
            ),
        }
    }

    /// Recognize and check a fault intrinsic call (`panic`/`todo`/`unreachable`/`assert`); returns
    /// `Some(ty)` if `name` is one, else `None` (a normal call). Messages must be string *literals*
    /// (compile-time const) so both backends bake byte-identical fault text (M-faults 2a).
    fn check_intrinsic_call(
        &mut self,
        name: &str,
        args: &[crate::ast::Expr],
        span: Span,
    ) -> Option<Ty> {
        match name {
            "panic" => {
                if args.len() != 1 {
                    self.err(span, "`panic` takes one string-literal message");
                } else {
                    self.require_str_literal(&args[0], span);
                }
                Some(Ty::Never)
            }
            "todo" | "unreachable" => {
                if !args.is_empty() {
                    self.err(span, format!("`{name}` takes no arguments"));
                }
                Some(Ty::Never)
            }
            "assert" => {
                if args.is_empty() || args.len() > 2 {
                    self.err(
                        span,
                        "`assert` takes a bool and an optional string-literal message",
                    );
                } else {
                    let c = self.check_expr(&args[0]);
                    if !self.ty_assignable(&c, &Ty::Bool) {
                        self.err(
                            span,
                            format!("`assert` condition must be `bool`, found `{c}`"),
                        );
                    }
                    if let Some(m) = args.get(1) {
                        self.require_str_literal(m, span);
                    }
                }
                Some(Ty::Unit)
            }
            _ => None,
        }
    }

    /// Require `e` to be a string *literal* (one `StrPart::Literal`, no interpolation) — the fault
    /// intrinsics bake their message at compile time (M-faults 2a).
    fn require_str_literal(&mut self, e: &crate::ast::Expr, span: Span) {
        if !matches!(e, crate::ast::Expr::Str(parts, _)
            if parts.len() == 1 && matches!(parts[0], crate::ast::StrPart::Literal(_)))
        {
            self.err_coded(
                span,
                "this intrinsic's message must be a plain string literal",
                "E-INTRINSIC-LITERAL",
                Some("interpolation/expressions aren't allowed here yet".into()),
            );
        }
        self.check_expr(e);
    }

    fn check_force(&mut self, inner: &crate::ast::Expr, span: Span) -> Ty {
        let t = self.check_expr(inner);
        match t {
            Ty::Error => Ty::Error,
            Ty::Optional(inner_ty) => {
                self.warn_coded(
                    span,
                    "force-unwrap `!` asserts an optional is non-null and faults at runtime if it is null",
                    "W-FORCE-UNWRAP",
                    Some("prefer `??` (default), `?.` (safe access), or `if (var x = opt)` to handle null without a possible fault".into()),
                );
                *inner_ty
            }
            other => self.err_coded(
                span,
                format!("force-unwrap `!` requires an optional `T?`, found non-optional `{other}`"),
                "E-OPT-UNWRAP",
                Some("`!` unwraps a `T?` to `T`; a non-optional value is already non-null".into()),
            ),
        }
    }
    /// `E-OPT-USE`: a plain `.`/`.m()` was used on an optional (or `null`) receiver, which could
    /// dereference null. Steers the developer to `?.`, `??`, or a checked unwrap `!`.
    fn err_opt_use(&mut self, span: Span, name: &str, recv: &Ty, verb: &str) -> Ty {
        self.err_coded(
            span,
            format!("cannot {verb} `{name}` of optional `{recv}`; use `?.` for null-safe access or unwrap with `!`"),
            "E-OPT-USE",
            Some(format!("`{name}` is only present when the receiver is non-null")),
        )
    }
    /// Wrap a member/method result in `Optional` for a `?.` access (a safe access yields a nullable
    /// result), without double-wrapping an already-optional member and leaving `Error` to cascade.
    fn opt_wrap(t: Ty) -> Ty {
        match t {
            Ty::Error => Ty::Error,
            Ty::Optional(_) => t,
            other => Ty::Optional(Box::new(other)),
        }
    }
    fn check_for(&mut self, stmt: &crate::ast::Stmt) {
        if let crate::ast::Stmt::For {
            ty,
            name,
            iter,
            body,
            span,
        } = stmt
        {
            let declared = self.resolve_type(ty);
            let iter_ty = self.check_expr(iter);
            let elem = match iter_ty {
                Ty::List(e) => *e,
                Ty::Error => Ty::Error,
                other => {
                    self.err(
                        *span,
                        format!("`for`-`in` requires a List, found `{other}`"),
                    );
                    Ty::Error
                }
            };
            if !self.ty_assignable(&elem, &declared) {
                self.err(
                    *span,
                    format!("loop variable `{name}` declared `{declared}` but iterating `{elem}`"),
                );
            }
            self.push_scope();
            self.declare(name, declared, *span);
            self.loop_depth += 1;
            for s in body {
                self.check_stmt(s);
            }
            self.loop_depth -= 1;
            self.pop_scope();
        }
    }

    /// `while (cond) { .. }` / `do { .. } while (cond);` (M-mut.3). The condition must be `bool` and
    /// is checked in the loop's *outer* scope (the body's own bindings are not visible to it — true
    /// for do-while too, matching the interpreter's scope-pop-before-retest).
    fn check_while(
        &mut self,
        cond: &crate::ast::Expr,
        body: &[crate::ast::Stmt],
        _post_cond: bool,
        span: Span,
    ) {
        let ct = self.check_expr(cond);
        if !self.ty_assignable(&ct, &Ty::Bool) {
            self.err(span, format!("loop condition must be `bool`, found `{ct}`"));
        }
        self.push_scope();
        self.loop_depth += 1;
        for s in body {
            self.check_stmt(s);
        }
        self.loop_depth -= 1;
        self.pop_scope();
    }

    /// C-style `for (init; cond; step) { .. }` (M-mut.3). `init`'s binding lives in the loop's own
    /// scope and is visible to `cond`/`step`/`body`; `cond` (if present) must be `bool`.
    fn check_cfor(
        &mut self,
        init: Option<&crate::ast::Stmt>,
        cond: Option<&crate::ast::Expr>,
        step: Option<&crate::ast::Stmt>,
        body: &[crate::ast::Stmt],
    ) {
        self.push_scope();
        if let Some(s) = init {
            self.check_stmt(s);
        }
        if let Some(c) = cond {
            let ct = self.check_expr(c);
            if !self.ty_assignable(&ct, &Ty::Bool) {
                self.err(
                    Self::expr_span(c),
                    format!("loop condition must be `bool`, found `{ct}`"),
                );
            }
        }
        // `step` runs each iteration (not the loop body) but is checked once; a bare `break`/
        // `continue` in `step` is nonsensical, so it is NOT inside the loop-depth bump.
        if let Some(s) = step {
            self.check_stmt(s);
        }
        self.loop_depth += 1;
        for s in body {
            self.check_stmt(s);
        }
        self.loop_depth -= 1;
        self.pop_scope();
    }
    fn check_match(
        &mut self,
        scrutinee: &crate::ast::Expr,
        arms: &[crate::ast::MatchArm],
        span: Span,
    ) -> Ty {
        use crate::ast::Pattern;
        let scrut = self.check_expr(scrutinee);

        let mut result: Option<Ty> = None;
        let mut covered: Vec<String> = Vec::new();
        // Type-pattern coverage for match-over-union exhaustiveness (M-RT S4): the class/interface
        // names matched by `Circle c =>` arms.
        let mut covered_types: Vec<String> = Vec::new();
        let mut has_catch_all = false;
        // Once a `null` arm has matched, a later catch-all binding over a `T?` scrutinee sees only
        // the non-null inner — the smart-cast that makes `match opt { null => …, v => … }` bind
        // `v: T` (M3 S2.6 / S1.4). Tracks whether a prior arm covered `null`.
        let mut null_seen = false;
        // Reachability lints (W-MATCH-UNREACHABLE): an arm after a catch-all, or a duplicate
        // literal/variant/type arm, can never match. `catch_all_seen`/`seen_keys` reflect *prior*
        // arms only (updated at the end of each iteration), so the offending arm — not the first
        // good one — is flagged.
        let mut catch_all_seen = false;
        let mut seen_keys: Vec<String> = Vec::new();

        for arm in arms {
            if catch_all_seen {
                self.warn_coded(
                    arm.span,
                    "unreachable match arm: a previous arm already matches everything",
                    "W-MATCH-UNREACHABLE",
                    Some(
                        "a `_` or bare-identifier arm is a catch-all; arms after it never run"
                            .into(),
                    ),
                );
            } else if let Some(key) = match_arm_key(&arm.pattern) {
                if seen_keys.contains(&key) {
                    self.warn_coded(
                        arm.span,
                        "unreachable match arm: a previous arm already covers this pattern",
                        "W-MATCH-UNREACHABLE",
                        Some("remove the duplicate arm".into()),
                    );
                } else {
                    seen_keys.push(key);
                }
            }
            if matches!(arm.pattern, Pattern::Wildcard(_) | Pattern::Binding { .. }) {
                has_catch_all = true;
                catch_all_seen = true;
            }
            if let Pattern::Variant { name, .. } = &arm.pattern {
                covered.push(name.clone());
            }
            if let Pattern::Type { type_name, .. } = &arm.pattern {
                covered_types.push(type_name.clone());
            }
            // The type a catch-all binding sees: narrowed to the inner `T` when a preceding `null`
            // arm already handled absence; otherwise the scrutinee type unchanged.
            let arm_scrut = match (&scrut, null_seen) {
                (Ty::Optional(inner), true) => (**inner).clone(),
                _ => scrut.clone(),
            };
            // each arm gets its own scope for pattern bindings
            self.push_scope();
            self.check_pattern(&arm.pattern, &arm_scrut);
            let body_ty = self.check_expr(&arm.body);
            self.pop_scope();
            if matches!(arm.pattern, Pattern::Null(_)) {
                null_seen = true;
            }

            match &result {
                None => result = Some(body_ty),
                Some(first) => {
                    if !self.ty_assignable(&body_ty, first) && !self.ty_assignable(first, &body_ty)
                    {
                        self.err(
                            span,
                            format!(
                                "match arms must share one type; found `{first}` and `{body_ty}`"
                            ),
                        );
                    }
                }
            }
        }

        // exhaustiveness
        if !has_catch_all {
            match &scrut {
                Ty::Named(enum_name, _) if self.enums.contains_key(enum_name) => {
                    let all: Vec<String> = self.enums[enum_name].variants.keys().cloned().collect();
                    let mut missing: Vec<String> =
                        all.into_iter().filter(|v| !covered.contains(v)).collect();
                    // `variants` is a HashMap, so `keys()` order is nondeterministic — sort the
                    // missing list so the error message is stable across runs (otherwise it's an
                    // intermittent test/diff hazard).
                    missing.sort();
                    if !missing.is_empty() {
                        self.err(
                            span,
                            format!("non-exhaustive match: missing {}", missing.join(", ")),
                        );
                    }
                }
                // Match-over-union exhaustiveness (M-RT S4): every nominal member must be covered by a
                // type pattern naming it OR a covering supertype/interface. A primitive member can't be
                // type-matched, so a union containing one always needs a `_` (reported as missing).
                Ty::Union(members) => {
                    let mut missing: Vec<String> = members
                        .iter()
                        .filter(|m| match m {
                            Ty::Named(n, _) => !covered_types
                                .iter()
                                .any(|t| t == n || self.is_subtype(n, t)),
                            _ => true,
                        })
                        .map(std::string::ToString::to_string)
                        .collect();
                    missing.sort();
                    if !missing.is_empty() {
                        self.err(
                            span,
                            format!("non-exhaustive match: missing {}", missing.join(", ")),
                        );
                    }
                }
                Ty::Error => {}
                _ => {
                    self.err(
                        span,
                        "non-exhaustive match: add a `_` wildcard arm for non-enum scrutinees",
                    );
                }
            }
        }

        result.unwrap_or(Ty::Error)
    }

    /// Check a pattern against the scrutinee type, declaring its bindings into the
    /// current scope.
    fn check_pattern(&mut self, pat: &crate::ast::Pattern, scrut: &Ty) {
        use crate::ast::Pattern;
        match pat {
            Pattern::Wildcard(_) => {}
            Pattern::Binding { name, span } => self.declare(name, scrut.clone(), *span),
            Pattern::Int(_, span) => self.expect_prim(scrut, &Ty::Int, *span),
            Pattern::Float(_, span) => self.expect_prim(scrut, &Ty::Float, *span),
            Pattern::Str(_, span) => self.expect_prim(scrut, &Ty::String, *span),
            Pattern::Bool(_, span) => self.expect_prim(scrut, &Ty::Bool, *span),
            Pattern::Null(span) => {
                // A `null` pattern is only meaningful against an optional scrutinee (M3 S2.6).
                if !matches!(scrut, Ty::Optional(_) | Ty::Null | Ty::Error) {
                    self.err(
                        *span,
                        format!(
                            "`null` pattern requires an optional `T?` scrutinee, found `{scrut}`"
                        ),
                    );
                }
            }
            Pattern::Variant { name, fields, span } => {
                let (enum_name, eargs) = match scrut {
                    Ty::Named(n, eargs) if self.enums.contains_key(n) => (n.clone(), eargs.clone()),
                    Ty::Error => return,
                    other => {
                        self.err(*span, format!("variant pattern `{name}` requires an enum scrutinee, found `{other}`"));
                        return;
                    }
                };
                let field_tys: Vec<Ty> = match self.enums[&enum_name].variants.get(name) {
                    // Substitute the enum's type parameters with the scrutinee's type arguments
                    // (`Option<int>` ⇒ `{T → int}`) so a generic variant's payload binds at the
                    // concrete type (M-RT generic enums); identity for a non-generic enum.
                    Some(f) => {
                        let theta = self.enum_subst(&enum_name, &eargs);
                        f.iter().map(|t| apply_subst(t, &theta)).collect()
                    }
                    None => {
                        self.err(*span, format!("enum `{enum_name}` has no variant `{name}`"));
                        return;
                    }
                };
                if field_tys.len() != fields.len() {
                    self.err(
                        *span,
                        format!(
                            "variant `{name}` expects {} field(s), found {}",
                            field_tys.len(),
                            fields.len()
                        ),
                    );
                    return;
                }
                for (fp, ft) in fields.iter().zip(field_tys) {
                    // A type pattern nested in a variant field (`Wrapper(Circle c)`) is rejected this
                    // slice (M-RT S4): the transpiler only emits simple variable bindings for variant
                    // payloads, so allowing it would diverge from `run`/`runvm`. A clean rejection
                    // keeps all three backends agreeing (the byte-identity spine). Type patterns are
                    // top-level-only — that is the match-over-union surface.
                    if let Pattern::Type {
                        type_name, span, ..
                    } = fp
                    {
                        self.err_coded(
                            *span,
                            format!(
                                "type pattern `{type_name}` is only allowed at the top level of a match arm, not inside a variant pattern"
                            ),
                            "E-MATCH-TYPE",
                            Some("match the variant first, then `instanceof`/`match` its payload".into()),
                        );
                    }
                    self.check_pattern(fp, &ft);
                }
            }
            Pattern::Type {
                type_name,
                binding,
                span,
            } => {
                // M-RT S4 type pattern: the type must be a class or interface (the runtime test is
                // `instanceof`, which is class/interface-only — an enum value is never an instance).
                let known =
                    self.classes.contains_key(type_name) || self.interfaces.contains_key(type_name);
                if !known && !matches!(scrut, Ty::Error) {
                    let hint = if self.enums.contains_key(type_name) {
                        "an enum is a closed sum — match its variants directly, not via a type pattern"
                    } else {
                        "a type pattern matches a class or interface (as in a union scrutinee)"
                    };
                    self.err_coded(
                        *span,
                        format!("type pattern `{type_name}` must name a class or interface"),
                        "E-MATCH-TYPE",
                        Some(hint.into()),
                    );
                }
                // Bind the narrowed value as the named type. A generic class carries erased (poison)
                // arguments — `instanceof` keeps no type arguments at runtime, so its generic members
                // read as `mixed`, mirroring the if/instanceof smart-cast (M-RT generics-all).
                if let Some(name) = binding {
                    let arity = self
                        .classes
                        .get(type_name)
                        .map_or(0, |c| c.type_params.len());
                    let args = vec![Ty::Error; arity];
                    self.declare(name, Ty::Named(type_name.clone(), args), *span);
                }
            }
        }
    }

    fn expect_prim(&mut self, scrut: &Ty, want: &Ty, span: Span) {
        // A literal pattern matches when the scrutinee *is* that primitive, or is a union that
        // *contains* it (M-RT S4): `match code { 0 => …, "ok" => … }` over `int | string` is well
        // typed (the runtime/transpiler match by value, so a non-member literal simply never fires).
        let ok = *scrut == Ty::Error
            || scrut == want
            || matches!(scrut, Ty::Union(members) if members.contains(want));
        if !ok {
            self.err(
                span,
                format!("pattern of type `{want}` cannot match scrutinee of type `{scrut}`"),
            );
        }
    }
}

/// Returns `true` if the lambda body directly references `this` (F8 / `E-LAMBDA-THIS`).
/// Does NOT recurse into nested lambdas (they would be a separate `E-LAMBDA-THIS` site).
fn lambda_uses_this(body: &crate::ast::LambdaBody) -> bool {
    use crate::ast::{Expr, LambdaBody, Stmt};
    fn in_expr(e: &Expr) -> bool {
        match e {
            Expr::This(_) => true,
            Expr::Int(..)
            | Expr::Float(..)
            | Expr::Bool(..)
            | Expr::Null(..)
            | Expr::Bytes(..)
            | Expr::Ident(..) => false,
            Expr::Str(parts, _) | Expr::Html(parts, _) => parts.iter().any(|p| match p {
                crate::ast::StrPart::Expr(inner) => in_expr(inner),
                _ => false,
            }),
            Expr::List(items, _) => items.iter().any(in_expr),
            Expr::Map(pairs, _) => pairs.iter().any(|(k, v)| in_expr(k) || in_expr(v)),
            Expr::Unary { expr, .. } => in_expr(expr),
            Expr::Binary { lhs, rhs, .. } => in_expr(lhs) || in_expr(rhs),
            Expr::InstanceOf { value, .. } => in_expr(value),
            Expr::Call { callee, args, .. } => in_expr(callee) || args.iter().any(in_expr),
            Expr::Member { object, .. } => in_expr(object),
            Expr::Index { object, index, .. } => in_expr(object) || in_expr(index),
            Expr::Force { inner, .. } => in_expr(inner),
            Expr::Propagate { inner, .. } => in_expr(inner),
            Expr::CloneWith { object, fields, .. } => {
                in_expr(object) || fields.iter().any(|(_, e)| in_expr(e))
            }
            Expr::Match {
                scrutinee, arms, ..
            } => in_expr(scrutinee) || arms.iter().any(|a| in_expr(&a.body)),
            Expr::Range { start, end, .. } => in_expr(start) || in_expr(end),
            Expr::If {
                cond,
                then_expr,
                else_expr,
                ..
            } => in_expr(cond) || in_expr(then_expr) || in_expr(else_expr),
            // Nested lambdas: do not recurse — `this` in a nested lambda is a separate error site.
            Expr::Lambda { .. } => false,
        }
    }
    fn in_stmts(stmts: &[Stmt]) -> bool {
        stmts.iter().any(|s| match s {
            Stmt::VarDecl { init, .. } => in_expr(init),
            Stmt::Return { value, .. } => value.as_ref().is_some_and(in_expr),
            Stmt::If {
                cond,
                then_block,
                else_block,
                ..
            } => {
                in_expr(cond)
                    || in_stmts(then_block)
                    || else_block.as_ref().is_some_and(|eb| in_stmts(eb))
            }
            Stmt::For { iter, body, .. } => in_expr(iter) || in_stmts(body),
            Stmt::While { cond, body, .. } => in_expr(cond) || in_stmts(body),
            Stmt::CFor {
                init,
                cond,
                step,
                body,
                ..
            } => {
                init.as_deref()
                    .is_some_and(|s| in_stmts(std::slice::from_ref(s)))
                    || cond.as_ref().is_some_and(in_expr)
                    || step
                        .as_deref()
                        .is_some_and(|s| in_stmts(std::slice::from_ref(s)))
                    || in_stmts(body)
            }
            Stmt::Break(_) | Stmt::Continue(_) => false,
            Stmt::Assign { target, value, .. } => in_expr(target) || in_expr(value),
            Stmt::Block(stmts, _) => in_stmts(stmts),
            Stmt::Expr(e, _) => in_expr(e),
            Stmt::Throw { value, .. } => in_expr(value),
            Stmt::Try {
                body,
                catches,
                finally_block,
                ..
            } => {
                in_stmts(body)
                    || catches.iter().any(|c| in_stmts(&c.body))
                    || finally_block.as_ref().is_some_and(|fb| in_stmts(fb))
            }
        })
    }
    match body {
        LambdaBody::Expr(e) => in_expr(e),
        LambdaBody::Block(stmts) => in_stmts(stmts),
    }
}

/// Type-check a whole program. `Ok(())` means it is well-typed; otherwise every
/// detected error is returned.
/// Type-check `program`. On success returns the collected non-fatal warnings (the warning channel,
/// M3 S2.5) — possibly empty; on failure returns the errors. Warnings never gate the build: the CLI
/// renders them to stderr and proceeds.
/// Run the checker over a program and return the populated `Checker` (errors, warnings, and the
/// `html"…"` desugarings collected along the way). The single shared entry behind both [`check`]
/// (gate only) and [`check_resolutions`] (gate + html replacements for the backend pipeline).
fn run_checker(program: &Program) -> Checker {
    let mut c = Checker::new();
    c.collect(program);
    c.check_program(program);
    c
}

pub fn check(program: &Program) -> Result<Vec<Diagnostic>, Vec<Diagnostic>> {
    let c = run_checker(program);
    if c.errors.is_empty() {
        Ok(c.warnings)
    } else {
        Err(c.errors)
    }
}

/// Like [`check`], but on success also returns the `html"…"` desugarings keyed by literal
/// `Span.start` — fed to [`resolve_html`] so the backend-facing program is `Expr::Html`-free. Used
/// by the run/runvm/transpile pipeline ([`crate::cli::check_and_expand`]); plain [`check`] (e.g.
/// `phg check`) ignores the map since it never reaches a backend.
#[allow(clippy::type_complexity)]
pub fn check_resolutions(
    program: &Program,
) -> Result<(Vec<Diagnostic>, HashMap<usize, crate::ast::Expr>), Vec<Diagnostic>> {
    let c = run_checker(program);
    if c.errors.is_empty() {
        Ok((c.warnings, c.html_resolutions))
    } else {
        Err(c.errors)
    }
}

/// Classic two-row Levenshtein edit distance (ASCII-oriented; M1 identifiers are ASCII), used to
/// suggest the nearest in-scope name for an unknown identifier.
/// The reserved fault-intrinsic names (M-faults 2a) — `panic`/`todo`/`unreachable` (`never`) and
/// `assert` (`unit`). Recognized at call sites and rejected as user function names.
fn is_intrinsic_name(name: &str) -> bool {
    matches!(name, "panic" | "todo" | "unreachable" | "assert")
}

fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            cur[j + 1] = (prev[j + 1] + 1).min(cur[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

/// The original leaf identifier of a possibly loader-mangled name: the substring after the last
/// `\` (`Acme\Util\compute` ⇒ `compute`), or the whole string when unmangled. Casing is a property
/// of the source identifier, not the FQN the loader synthesizes (M5 S2c).
fn leaf_ident(name: &str) -> &str {
    name.rsplit('\\').next().unwrap_or(name)
}

/// camelCase: a lowercase ASCII first letter and no `_`. A single lowercase word (`main`, `area`,
/// `hi`) qualifies. Empty strings are not valid (the parser never produces them, but be total).
fn is_camel(s: &str) -> bool {
    s.chars().next().is_some_and(|c| c.is_ascii_lowercase()) && !s.contains('_')
}

/// PascalCase: an uppercase ASCII first letter and no `_` (`Shape`, `Circle`, `HttpRequest`).
fn is_pascal(s: &str) -> bool {
    s.chars().next().is_some_and(|c| c.is_ascii_uppercase()) && !s.contains('_')
}

/// Split a snake_case-or-otherwise identifier into its `_`-delimited words, dropping empties (so a
/// leading/trailing/doubled `_` does not yield a blank word). Shared by both converters.
fn case_words(s: &str) -> Vec<&str> {
    s.split('_').filter(|w| !w.is_empty()).collect()
}

/// Uppercase the first ASCII letter of a word, leaving the rest unchanged (`shape` → `Shape`,
/// `once` → `Once`). Non-alphabetic leads pass through.
fn upper_first(w: &str) -> String {
    let mut cs = w.chars();
    match cs.next() {
        Some(c) => c.to_ascii_uppercase().to_string() + cs.as_str(),
        None => String::new(),
    }
}

/// Convert an identifier to the suggested camelCase form (`split_once` → `splitOnce`,
/// `c_to_f` → `cToF`, `shape` → `shape`): the first word lowercased-first, each later word
/// capitalized, joined with no separator.
fn to_camel(s: &str) -> String {
    let words = case_words(s);
    let mut out = String::new();
    for (i, w) in words.iter().enumerate() {
        if i == 0 {
            let mut cs = w.chars();
            if let Some(c) = cs.next() {
                out.push(c.to_ascii_lowercase());
                out.push_str(cs.as_str());
            }
        } else {
            out.push_str(&upper_first(w));
        }
    }
    out
}

/// Convert an identifier to the suggested PascalCase form (`shape` → `Shape`,
/// `http_request` → `HttpRequest`): every word capitalized, joined with no separator.
fn to_pascal(s: &str) -> String {
    case_words(s).iter().map(|w| upper_first(w)).collect()
}

/// True for the built-in type names `resolve_type` handles directly — a `type` alias may not
/// shadow them (else the checker and the backend expansion would disagree; see `collect`).
/// Apply a unification substitution `θ` to a type, replacing each `Ty::Param(p)` by `θ[p]` (an
/// unbound parameter is left as-is). Used to compute a generic call's result type from the bindings
/// inferred at the call site (M-RT S7).
fn apply_subst(ty: &Ty, theta: &HashMap<String, Ty>) -> Ty {
    match ty {
        Ty::Param(p) => theta
            .get(p)
            .cloned()
            .unwrap_or_else(|| Ty::Param(p.clone())),
        Ty::List(e) => Ty::List(Box::new(apply_subst(e, theta))),
        Ty::Set(e) => Ty::Set(Box::new(apply_subst(e, theta))),
        Ty::Optional(e) => Ty::Optional(Box::new(apply_subst(e, theta))),
        Ty::Map(k, v) => Ty::Map(
            Box::new(apply_subst(k, theta)),
            Box::new(apply_subst(v, theta)),
        ),
        Ty::Function(ps, r) => Ty::Function(
            ps.iter().map(|p| apply_subst(p, theta)).collect(),
            Box::new(apply_subst(r, theta)),
        ),
        // A generic class instance type carries its arguments — substitute through them so a
        // `Box<T>` return / field resolves to `Box<int>` (M-RT generics-all).
        Ty::Named(n, args) => Ty::Named(
            n.clone(),
            args.iter().map(|a| apply_subst(a, theta)).collect(),
        ),
        other => other.clone(),
    }
}

/// Whether a type contains a `Ty::Param` anywhere (recursing through containers/optionals/functions).
/// A native whose stored signature contains one is checked via call-site unification, exactly like a
/// generic free function (M-RT S7b).
fn ty_has_param(ty: &Ty) -> bool {
    match ty {
        Ty::Param(_) => true,
        Ty::List(e) | Ty::Set(e) | Ty::Optional(e) => ty_has_param(e),
        Ty::Map(k, v) => ty_has_param(k) || ty_has_param(v),
        Ty::Function(ps, r) => ps.iter().any(ty_has_param) || ty_has_param(r),
        Ty::Named(_, args) => args.iter().any(ty_has_param),
        _ => false,
    }
}

/// Whether an expression is the literal `true` — the only condition an always-running loop can carry
/// for the structural termination analysis (M-RT totality cluster). Anything else (a variable, a
/// comparison) might be false, so the loop might exit and is not treated as divergent.
fn is_true_lit(e: &crate::ast::Expr) -> bool {
    matches!(e, crate::ast::Expr::Bool(true, _))
}

/// Whether `stmts` contains a `break` bound to the *current* loop. Descends into `if`/`block` (a
/// `break` there still targets the enclosing loop) but NOT into nested `while`/`for`/`do` loops (their
/// `break`s bind to them). `match` arms are expressions and carry no `break`.
fn breaks_this_loop(stmts: &[crate::ast::Stmt]) -> bool {
    use crate::ast::Stmt;
    stmts.iter().any(|s| match s {
        Stmt::Break(_) => true,
        Stmt::Block(b, _) => breaks_this_loop(b),
        Stmt::If {
            then_block,
            else_block,
            ..
        } => {
            breaks_this_loop(then_block)
                || else_block.as_ref().is_some_and(|eb| breaks_this_loop(eb))
        }
        _ => false,
    })
}

/// A stable identity for a `match` pattern, for duplicate-arm detection (`W-MATCH-UNREACHABLE`).
/// `None` for patterns that should not be deduplicated: `float` (equality is fuzzy) and the
/// catch-alls (`_`/bare binding, handled separately as a catch-all).
fn match_arm_key(p: &crate::ast::Pattern) -> Option<String> {
    use crate::ast::Pattern;
    match p {
        Pattern::Int(v, _) => Some(format!("i{v}")),
        Pattern::Str(s, _) => Some(format!("s{s}")),
        Pattern::Bool(b, _) => Some(format!("b{b}")),
        Pattern::Null(_) => Some("null".to_string()),
        Pattern::Variant { name, .. } => Some(format!("v{name}")),
        Pattern::Type { type_name, .. } => Some(format!("t{type_name}")),
        Pattern::Float(_, _) | Pattern::Wildcard(_) | Pattern::Binding { .. } => None,
    }
}

fn is_builtin_type_name(name: &str) -> bool {
    matches!(
        name,
        "int"
            | "float"
            | "bool"
            | "string"
            | "bytes"
            | "never"
            | "Html"
            | "Attr"
            | "List"
            | "Map"
            | "Set"
            | "decimal"
            | "double"
            | "i8"
            | "i16"
            | "i32"
            | "i64"
            | "u8"
            | "u16"
            | "u32"
            | "u64"
            // The built-in `Error` marker interface (M-faults 2b) — reserved so user code can't
            // redefine it (as a class/enum/interface/alias).
            | "Error"
    )
}

/// Expand every `type` alias into its underlying type and drop the alias declarations, so the
/// interpreter, compiler, and transpiler all see alias-free types (aliases are pure front-end
/// sugar). Runs *after* [`check`] succeeds — which has already rejected cycles and built-in
/// shadowing — so a fixed depth bound is a sufficient guard against a residual self-reference, and
/// the resolver can be a simple "look the name up, recurse" walk. `Expr` nodes carry no `Type` in
/// M1, so they are cloned unchanged.
/// Replace every `html"…"` literal ([`crate::ast::Expr::Html`]) with its checker-built
/// `html.concat([…])` desugaring (keyed by `Span.start`), so the interpreter, compiler, and
/// transpiler never see the node — the same "compile-time sugar, erased before backends" treatment
/// as `type` aliases. Runs after a successful [`check_resolutions`]; mirrors the owned-AST rewrite
/// walk in `loader::resolve_*`, but also descends into lambda bodies (an `html"…"` may appear there
/// too). A replacement can itself embed an `html"…"` (an Html-typed hole), so the rewrite recurses
/// into each substituted subtree. When no literal was found the program is returned untouched, so
/// programs with no `html"…"` are byte-for-byte identical to the pre-Wave-3 AST.
pub fn resolve_html(program: Program, html: &HashMap<usize, crate::ast::Expr>) -> Program {
    use crate::ast::{ClassMember, Expr, Item, LambdaBody, MatchArm, Stmt, StrPart};
    if html.is_empty() {
        return program;
    }
    type Map = HashMap<usize, Expr>;

    fn rexpr(e: Expr, h: &Map) -> Expr {
        match e {
            Expr::Html(parts, span) => match h.get(&span.start) {
                // Re-walk the substituted tree: an Html-typed hole embeds another `html"…"`.
                Some(r) => rexpr(r.clone(), h),
                None => Expr::Html(parts, span), // defensive; check populated every literal
            },
            Expr::Str(parts, span) => Expr::Str(
                parts
                    .into_iter()
                    .map(|p| match p {
                        StrPart::Expr(e) => StrPart::Expr(Box::new(rexpr(*e, h))),
                        lit => lit,
                    })
                    .collect(),
                span,
            ),
            Expr::List(items, span) => {
                Expr::List(items.into_iter().map(|e| rexpr(e, h)).collect(), span)
            }
            Expr::Map(pairs, span) => Expr::Map(
                pairs
                    .into_iter()
                    .map(|(k, v)| (rexpr(k, h), rexpr(v, h)))
                    .collect(),
                span,
            ),
            Expr::Unary { op, expr, span } => Expr::Unary {
                op,
                expr: Box::new(rexpr(*expr, h)),
                span,
            },
            Expr::Binary { op, lhs, rhs, span } => Expr::Binary {
                op,
                lhs: Box::new(rexpr(*lhs, h)),
                rhs: Box::new(rexpr(*rhs, h)),
                span,
            },
            Expr::Call { callee, args, span } => Expr::Call {
                callee: Box::new(rexpr(*callee, h)),
                args: args.into_iter().map(|a| rexpr(a, h)).collect(),
                span,
            },
            Expr::Member {
                object,
                name,
                safe,
                span,
            } => Expr::Member {
                object: Box::new(rexpr(*object, h)),
                name,
                safe,
                span,
            },
            Expr::Index {
                object,
                index,
                span,
            } => Expr::Index {
                object: Box::new(rexpr(*object, h)),
                index: Box::new(rexpr(*index, h)),
                span,
            },
            Expr::Force { inner, span } => Expr::Force {
                inner: Box::new(rexpr(*inner, h)),
                span,
            },
            // A throws-mode `?` was recorded for erasure (its `Span.start` is in `h`, mapping to the
            // inner call): unwrap it to the bare call — the call's own throw unwinds, so no backend
            // ever sees a throws-mode `Propagate`. A Result-mode `?` is absent from `h` and kept.
            Expr::Propagate { inner, span } => match h.get(&span.start) {
                Some(r) => rexpr(r.clone(), h),
                None => Expr::Propagate {
                    inner: Box::new(rexpr(*inner, h)),
                    span,
                },
            },
            Expr::Match {
                scrutinee,
                arms,
                span,
            } => Expr::Match {
                scrutinee: Box::new(rexpr(*scrutinee, h)),
                arms: arms
                    .into_iter()
                    .map(|a| MatchArm {
                        pattern: a.pattern,
                        body: rexpr(a.body, h),
                        span: a.span,
                    })
                    .collect(),
                span,
            },
            Expr::Range {
                start,
                end,
                inclusive,
                span,
            } => Expr::Range {
                start: Box::new(rexpr(*start, h)),
                end: Box::new(rexpr(*end, h)),
                inclusive,
                span,
            },
            Expr::If {
                cond,
                then_expr,
                else_expr,
                span,
            } => Expr::If {
                cond: Box::new(rexpr(*cond, h)),
                then_expr: Box::new(rexpr(*then_expr, h)),
                else_expr: Box::new(rexpr(*else_expr, h)),
                span,
            },
            Expr::Lambda {
                params,
                ret,
                body,
                span,
            } => Expr::Lambda {
                params,
                ret,
                body: match body {
                    LambdaBody::Expr(e) => LambdaBody::Expr(Box::new(rexpr(*e, h))),
                    LambdaBody::Block(stmts) => LambdaBody::Block(rblock(stmts, h)),
                },
                span,
            },
            Expr::CloneWith {
                object,
                fields,
                span,
            } => Expr::CloneWith {
                object: Box::new(rexpr(*object, h)),
                fields: fields.into_iter().map(|(n, e)| (n, rexpr(e, h))).collect(),
                span,
            },
            // leaves carry no nested expression: Int / Float / Bool / Null / Bytes / Ident / This
            leaf => leaf,
        }
    }

    fn rstmt(s: Stmt, h: &Map) -> Stmt {
        match s {
            Stmt::VarDecl {
                ty,
                name,
                init,
                mutable,
                span,
            } => Stmt::VarDecl {
                ty,
                name,
                init: rexpr(init, h),
                mutable,
                span,
            },
            Stmt::Assign {
                target,
                value,
                span,
            } => Stmt::Assign {
                target: rexpr(target, h),
                value: rexpr(value, h),
                span,
            },
            Stmt::Return { value, span } => Stmt::Return {
                value: value.map(|e| rexpr(e, h)),
                span,
            },
            Stmt::If {
                cond,
                bind,
                then_block,
                else_block,
                span,
            } => Stmt::If {
                cond: rexpr(cond, h),
                bind,
                then_block: rblock(then_block, h),
                else_block: else_block.map(|b| rblock(b, h)),
                span,
            },
            Stmt::For {
                ty,
                name,
                iter,
                body,
                span,
            } => Stmt::For {
                ty,
                name,
                iter: rexpr(iter, h),
                body: rblock(body, h),
                span,
            },
            Stmt::While {
                cond,
                body,
                post_cond,
                span,
            } => Stmt::While {
                cond: rexpr(cond, h),
                body: rblock(body, h),
                post_cond,
                span,
            },
            Stmt::CFor {
                init,
                cond,
                step,
                body,
                span,
            } => Stmt::CFor {
                init: init.map(|s| Box::new(rstmt(*s, h))),
                cond: cond.map(|e| rexpr(e, h)),
                step: step.map(|s| Box::new(rstmt(*s, h))),
                body: rblock(body, h),
                span,
            },
            Stmt::Break(span) => Stmt::Break(span),
            Stmt::Continue(span) => Stmt::Continue(span),
            Stmt::Block(stmts, span) => Stmt::Block(rblock(stmts, h), span),
            Stmt::Expr(e, span) => Stmt::Expr(rexpr(e, h), span),
            Stmt::Throw { value, span } => Stmt::Throw {
                value: rexpr(value, h),
                span,
            },
            Stmt::Try {
                body,
                catches,
                finally_block,
                span,
            } => Stmt::Try {
                body: rblock(body, h),
                catches: catches
                    .into_iter()
                    .map(|c| crate::ast::CatchClause {
                        ty: c.ty,
                        name: c.name,
                        body: rblock(c.body, h),
                        span: c.span,
                    })
                    .collect(),
                finally_block: finally_block.map(|b| rblock(b, h)),
                span,
            },
        }
    }

    fn rblock(stmts: Vec<Stmt>, h: &Map) -> Vec<Stmt> {
        stmts.into_iter().map(|s| rstmt(s, h)).collect()
    }

    let items = program
        .items
        .into_iter()
        .map(|item| match item {
            Item::Function(mut f) => {
                f.body = rblock(f.body, html);
                Item::Function(f)
            }
            Item::Class(mut c) => {
                for m in &mut c.members {
                    match m {
                        ClassMember::Method(f) => {
                            let body = std::mem::take(&mut f.body);
                            f.body = rblock(body, html);
                        }
                        ClassMember::Constructor { body, .. } => {
                            let b = std::mem::take(body);
                            *body = rblock(b, html);
                        }
                        // A property hook's get expression + set block may contain `html"…"`
                        // interpolation — rewrite both (M-mut.7b).
                        ClassMember::Hook { get, set, .. } => {
                            if let Some(e) = get.take() {
                                *get = Some(rexpr(e, html));
                            }
                            if let Some((p, body)) = set.take() {
                                *set = Some((p, rblock(body, html)));
                            }
                        }
                        ClassMember::Field { .. } => {}
                    }
                }
                Item::Class(c)
            }
            other => other,
        })
        .collect();

    Program {
        package: program.package,
        items,
        span: program.span,
    }
}

/// Erase generic type parameters from a checked program (M-RT S7). For every generic free function,
/// every type annotation that names one of *that function's* type parameters is rewritten to
/// `Type::Erased` and the parameter list is cleared, so the interpreter, compiler, and transpiler
/// all see an ordinary, type-variable-free function (PHP `mixed` at the boundary). This is the same
/// "compile-time-only, expanded out before any backend" discipline as `type` aliases and `html"…"`,
/// and it is what keeps generics zero-cost and byte-identical across the three backends: there is no
/// monomorphization, the type variables simply disappear after checking. Type parameters are scoped
/// to their own function, so only `Item::Function` items with a non-empty `type_params` are
/// rewritten; everything else is returned untouched (a program with no generics is byte-for-byte the
/// pre-S7 AST). Runs after a successful [`check`], so the `T`-bearing types it erases were already
/// validated.
pub fn erase_generics(program: Program) -> Program {
    use crate::ast::{
        ClassDecl, ClassMember, Expr, FunctionDecl, Item, LambdaBody, MatchArm, Param, Stmt,
        StrPart, Type,
    };
    use std::collections::HashSet;

    type Params<'a> = HashSet<&'a str>;

    fn member_is_generic(m: &ClassMember) -> bool {
        matches!(m, ClassMember::Method(f) if !f.type_params.is_empty())
    }

    fn rty(ty: &Type, params: &Params) -> Type {
        match ty {
            Type::Named { name, args, span } => {
                // A bare reference to a type parameter erases; a real generic container (`List<T>`)
                // keeps its head and recurses into its arguments.
                if args.is_empty() && params.contains(name.as_str()) {
                    Type::Erased(*span)
                } else {
                    Type::Named {
                        name: name.clone(),
                        args: args.iter().map(|a| rty(a, params)).collect(),
                        span: *span,
                    }
                }
            }
            Type::Optional { inner, span } => Type::Optional {
                inner: Box::new(rty(inner, params)),
                span: *span,
            },
            Type::Function {
                params: ps,
                ret,
                span,
            } => Type::Function {
                params: ps.iter().map(|p| rty(p, params)).collect(),
                ret: Box::new(rty(ret, params)),
                span: *span,
            },
            // A union erases each member (a type-param member becomes `Type::Erased`); the union
            // itself is structural and survives to the backend (M-RT S4).
            Type::Union(members, span) => {
                Type::Union(members.iter().map(|m| rty(m, params)).collect(), *span)
            }
            // An intersection erases each member (a type-param member becomes `Type::Erased`); the
            // intersection itself is structural and survives to the backend (M-RT S5).
            Type::Intersection(members, span) => {
                Type::Intersection(members.iter().map(|m| rty(m, params)).collect(), *span)
            }
            Type::Infer(s) => Type::Infer(*s),
            Type::Erased(s) => Type::Erased(*s),
        }
    }
    fn rparam(p: &Param, params: &Params) -> Param {
        Param {
            ty: rty(&p.ty, params),
            name: p.name.clone(),
            span: p.span,
        }
    }
    fn rctorparam(p: &crate::ast::CtorParam, params: &Params) -> crate::ast::CtorParam {
        crate::ast::CtorParam {
            modifiers: p.modifiers.clone(),
            ty: rty(&p.ty, params),
            name: p.name.clone(),
            span: p.span,
        }
    }
    fn rparts(parts: &[StrPart], params: &Params) -> Vec<StrPart> {
        parts
            .iter()
            .map(|p| match p {
                StrPart::Expr(e) => StrPart::Expr(Box::new(rexpr(e, params))),
                StrPart::Literal(s) => StrPart::Literal(s.clone()),
            })
            .collect()
    }
    fn rexpr(e: &Expr, params: &Params) -> Expr {
        match e {
            // The only expression that carries types: a lambda's parameters and return annotation.
            Expr::Lambda {
                params: lp,
                ret,
                body,
                span,
            } => Expr::Lambda {
                params: lp.iter().map(|p| rparam(p, params)).collect(),
                ret: ret.as_ref().map(|t| rty(t, params)),
                body: match body {
                    LambdaBody::Expr(inner) => LambdaBody::Expr(Box::new(rexpr(inner, params))),
                    LambdaBody::Block(stmts) => {
                        LambdaBody::Block(stmts.iter().map(|s| rstmt(s, params)).collect())
                    }
                },
                span: *span,
            },
            Expr::Str(parts, span) => Expr::Str(rparts(parts, params), *span),
            Expr::Html(parts, span) => Expr::Html(rparts(parts, params), *span),
            Expr::List(items, span) => {
                Expr::List(items.iter().map(|i| rexpr(i, params)).collect(), *span)
            }
            Expr::Map(pairs, span) => Expr::Map(
                pairs
                    .iter()
                    .map(|(k, v)| (rexpr(k, params), rexpr(v, params)))
                    .collect(),
                *span,
            ),
            Expr::Unary { op, expr, span } => Expr::Unary {
                op: *op,
                expr: Box::new(rexpr(expr, params)),
                span: *span,
            },
            Expr::Binary { op, lhs, rhs, span } => Expr::Binary {
                op: *op,
                lhs: Box::new(rexpr(lhs, params)),
                rhs: Box::new(rexpr(rhs, params)),
                span: *span,
            },
            Expr::InstanceOf {
                value,
                type_name,
                span,
            } => Expr::InstanceOf {
                value: Box::new(rexpr(value, params)),
                type_name: type_name.clone(),
                span: *span,
            },
            Expr::Call { callee, args, span } => Expr::Call {
                callee: Box::new(rexpr(callee, params)),
                args: args.iter().map(|a| rexpr(a, params)).collect(),
                span: *span,
            },
            Expr::Member {
                object,
                name,
                safe,
                span,
            } => Expr::Member {
                object: Box::new(rexpr(object, params)),
                name: name.clone(),
                safe: *safe,
                span: *span,
            },
            Expr::Index {
                object,
                index,
                span,
            } => Expr::Index {
                object: Box::new(rexpr(object, params)),
                index: Box::new(rexpr(index, params)),
                span: *span,
            },
            Expr::Force { inner, span } => Expr::Force {
                inner: Box::new(rexpr(inner, params)),
                span: *span,
            },
            Expr::Propagate { inner, span } => Expr::Propagate {
                inner: Box::new(rexpr(inner, params)),
                span: *span,
            },
            Expr::Match {
                scrutinee,
                arms,
                span,
            } => Expr::Match {
                scrutinee: Box::new(rexpr(scrutinee, params)),
                arms: arms
                    .iter()
                    .map(|a| MatchArm {
                        pattern: a.pattern.clone(),
                        body: rexpr(&a.body, params),
                        span: a.span,
                    })
                    .collect(),
                span: *span,
            },
            Expr::Range {
                start,
                end,
                inclusive,
                span,
            } => Expr::Range {
                start: Box::new(rexpr(start, params)),
                end: Box::new(rexpr(end, params)),
                inclusive: *inclusive,
                span: *span,
            },
            Expr::If {
                cond,
                then_expr,
                else_expr,
                span,
            } => Expr::If {
                cond: Box::new(rexpr(cond, params)),
                then_expr: Box::new(rexpr(then_expr, params)),
                else_expr: Box::new(rexpr(else_expr, params)),
                span: *span,
            },
            Expr::CloneWith {
                object,
                fields,
                span,
            } => Expr::CloneWith {
                object: Box::new(rexpr(object, params)),
                fields: fields
                    .iter()
                    .map(|(n, e)| (n.clone(), rexpr(e, params)))
                    .collect(),
                span: *span,
            },
            // leaves carry no type and no nested expression: Int / Float / Bool / Null / Bytes /
            // Ident / This — clone unchanged.
            leaf => leaf.clone(),
        }
    }
    fn rstmt(s: &Stmt, params: &Params) -> Stmt {
        match s {
            Stmt::VarDecl {
                ty,
                name,
                init,
                mutable,
                span,
            } => Stmt::VarDecl {
                ty: rty(ty, params),
                name: name.clone(),
                init: rexpr(init, params),
                mutable: *mutable,
                span: *span,
            },
            Stmt::Assign {
                target,
                value,
                span,
            } => Stmt::Assign {
                target: rexpr(target, params),
                value: rexpr(value, params),
                span: *span,
            },
            Stmt::Return { value, span } => Stmt::Return {
                value: value.as_ref().map(|e| rexpr(e, params)),
                span: *span,
            },
            Stmt::If {
                cond,
                bind,
                then_block,
                else_block,
                span,
            } => Stmt::If {
                cond: rexpr(cond, params),
                bind: bind.clone(),
                then_block: then_block.iter().map(|s| rstmt(s, params)).collect(),
                else_block: else_block
                    .as_ref()
                    .map(|b| b.iter().map(|s| rstmt(s, params)).collect()),
                span: *span,
            },
            Stmt::For {
                ty,
                name,
                iter,
                body,
                span,
            } => Stmt::For {
                ty: rty(ty, params),
                name: name.clone(),
                iter: rexpr(iter, params),
                body: body.iter().map(|s| rstmt(s, params)).collect(),
                span: *span,
            },
            Stmt::While {
                cond,
                body,
                post_cond,
                span,
            } => Stmt::While {
                cond: rexpr(cond, params),
                body: body.iter().map(|s| rstmt(s, params)).collect(),
                post_cond: *post_cond,
                span: *span,
            },
            Stmt::CFor {
                init,
                cond,
                step,
                body,
                span,
            } => Stmt::CFor {
                init: init.as_ref().map(|s| Box::new(rstmt(s, params))),
                cond: cond.as_ref().map(|e| rexpr(e, params)),
                step: step.as_ref().map(|s| Box::new(rstmt(s, params))),
                body: body.iter().map(|s| rstmt(s, params)).collect(),
                span: *span,
            },
            Stmt::Break(span) => Stmt::Break(*span),
            Stmt::Continue(span) => Stmt::Continue(*span),
            Stmt::Block(stmts, span) => {
                Stmt::Block(stmts.iter().map(|s| rstmt(s, params)).collect(), *span)
            }
            Stmt::Expr(e, span) => Stmt::Expr(rexpr(e, params), *span),
            Stmt::Throw { value, span } => Stmt::Throw {
                value: rexpr(value, params),
                span: *span,
            },
            Stmt::Try {
                body,
                catches,
                finally_block,
                span,
            } => Stmt::Try {
                body: body.iter().map(|s| rstmt(s, params)).collect(),
                catches: catches
                    .iter()
                    .map(|c| crate::ast::CatchClause {
                        ty: rty(&c.ty, params),
                        name: c.name.clone(),
                        body: c.body.iter().map(|s| rstmt(s, params)).collect(),
                        span: c.span,
                    })
                    .collect(),
                finally_block: finally_block
                    .as_ref()
                    .map(|b| b.iter().map(|s| rstmt(s, params)).collect()),
                span: *span,
            },
        }
    }

    let Program {
        package,
        items,
        span,
    } = program;
    let items = items
        .into_iter()
        .map(|item| match item {
            Item::Function(f) if !f.type_params.is_empty() => {
                let params: Params = f.type_params.iter().map(String::as_str).collect();
                Item::Function(FunctionDecl {
                    modifiers: f.modifiers.clone(),
                    vis: f.vis,
                    name: f.name.clone(),
                    type_params: Vec::new(), // erased
                    params: f.params.iter().map(|p| rparam(p, &params)).collect(),
                    ret: f.ret.as_ref().map(|t| rty(t, &params)),
                    throws: f.throws.iter().map(|t| rty(t, &params)).collect(),
                    body: f.body.iter().map(|s| rstmt(s, &params)).collect(),
                    span: f.span,
                })
            }
            // A generic class (class-level `<T>`) and/or a class with a generic method (M-RT
            // generics-all): erase the class's type parameters across *every* member (field types,
            // constructor parameters, method signatures + bodies) and each generic method's own
            // `<U>`, then clear all type-parameter lists. The class's `<T>`-typed members become PHP
            // `mixed`; the class declaration itself stays (just non-generic). A class with neither
            // class-level params nor a generic method is returned untouched by the `other` arm, so a
            // non-generic program is byte-for-byte the pre-generics AST.
            Item::Class(c)
                if !c.type_params.is_empty() || c.members.iter().any(member_is_generic) =>
            {
                let class_params: Vec<&str> = c.type_params.iter().map(String::as_str).collect();
                let members = c
                    .members
                    .into_iter()
                    .map(|m| match m {
                        ClassMember::Method(f) => {
                            // erase the class's params *and* this method's own
                            let mut set: Params = class_params.iter().copied().collect();
                            for tp in &f.type_params {
                                set.insert(tp.as_str());
                            }
                            ClassMember::Method(FunctionDecl {
                                modifiers: f.modifiers.clone(),
                                vis: f.vis,
                                name: f.name.clone(),
                                type_params: Vec::new(), // erased
                                params: f.params.iter().map(|p| rparam(p, &set)).collect(),
                                ret: f.ret.as_ref().map(|t| rty(t, &set)),
                                throws: f.throws.iter().map(|t| rty(t, &set)).collect(),
                                body: f.body.iter().map(|s| rstmt(s, &set)).collect(),
                                span: f.span,
                            })
                        }
                        ClassMember::Field {
                            modifiers,
                            ty,
                            name,
                            init,
                            span,
                        } => {
                            let set: Params = class_params.iter().copied().collect();
                            ClassMember::Field {
                                modifiers,
                                ty: rty(&ty, &set),
                                name,
                                init: init.as_ref().map(|e| rexpr(e, &set)),
                                span,
                            }
                        }
                        ClassMember::Constructor { params, body, span } => {
                            let set: Params = class_params.iter().copied().collect();
                            ClassMember::Constructor {
                                params: params.iter().map(|p| rctorparam(p, &set)).collect(),
                                body: body.iter().map(|s| rstmt(s, &set)).collect(),
                                span,
                            }
                        }
                        // A property hook (M-mut.7b): erase the class params from its type, get
                        // expression, and set parameter+block (a hook declares no `<T>` of its own).
                        ClassMember::Hook {
                            ty,
                            name,
                            get,
                            set: setter,
                            span,
                        } => {
                            let set: Params = class_params.iter().copied().collect();
                            ClassMember::Hook {
                                ty: rty(&ty, &set),
                                name,
                                get: get.as_ref().map(|e| rexpr(e, &set)),
                                set: setter.as_ref().map(|(p, b)| {
                                    (rparam(p, &set), b.iter().map(|s| rstmt(s, &set)).collect())
                                }),
                                span,
                            }
                        }
                    })
                    .collect();
                Item::Class(ClassDecl {
                    vis: c.vis,
                    name: c.name,
                    type_params: Vec::new(), // erased
                    extends: c.extends,
                    implements: c.implements,
                    open: c.open,
                    is_abstract: c.is_abstract,
                    resolutions: c.resolutions,
                    uses: c.uses,
                    members,
                    span: c.span,
                })
            }
            // A generic enum (`Option<T>`/`Result<T, E>`, M-RT generic enums): erase the enum's type
            // parameters across every variant's field types (a `T` payload becomes PHP `mixed`) and
            // clear the parameter list, so the backends see an ordinary, type-variable-free enum.
            // Same "expanded out before any backend" discipline as a generic class.
            Item::Enum(e) if !e.type_params.is_empty() => {
                let params: Params = e.type_params.iter().map(String::as_str).collect();
                Item::Enum(crate::ast::EnumDecl {
                    vis: e.vis,
                    name: e.name,
                    type_params: Vec::new(), // erased
                    variants: e
                        .variants
                        .into_iter()
                        .map(|v| crate::ast::EnumVariant {
                            name: v.name,
                            fields: v.fields.iter().map(|p| rparam(p, &params)).collect(),
                            span: v.span,
                        })
                        .collect(),
                    span: e.span,
                })
            }
            other => other,
        })
        .collect();
    Program {
        package,
        items,
        span,
    }
}

pub fn expand_aliases(program: &Program) -> Program {
    use crate::ast::{
        ClassDecl, ClassMember, CtorParam, EnumDecl, EnumVariant, FunctionDecl, InterfaceDecl,
        Item, Param, Stmt, Type,
    };
    type Aliases = HashMap<String, Type>;

    let mut aliases: Aliases = HashMap::new();
    for item in &program.items {
        if let Item::TypeAlias { name, ty, .. } = item {
            aliases.insert(name.clone(), ty.clone());
        }
    }

    fn rt(ty: &Type, a: &Aliases, depth: usize) -> Type {
        if depth > 64 {
            return ty.clone(); // defensive: check() already rejected alias cycles
        }
        match ty {
            Type::Named { name, args, span } => {
                if let Some(target) = a.get(name) {
                    rt(target, a, depth + 1)
                } else {
                    Type::Named {
                        name: name.clone(),
                        args: args.iter().map(|x| rt(x, a, depth + 1)).collect(),
                        span: *span,
                    }
                }
            }
            Type::Optional { inner, span } => Type::Optional {
                inner: Box::new(rt(inner, a, depth + 1)),
                span: *span,
            },
            Type::Function { params, ret, span } => Type::Function {
                params: params.iter().map(|p| rt(p, a, depth + 1)).collect(),
                ret: Box::new(rt(ret, a, depth + 1)),
                span: *span,
            },
            // A union expands each member (an alias used as a member dealiases here), M-RT S4.
            Type::Union(members, span) => {
                Type::Union(members.iter().map(|m| rt(m, a, depth + 1)).collect(), *span)
            }
            // An intersection expands each member likewise (M-RT S5).
            Type::Intersection(members, span) => {
                Type::Intersection(members.iter().map(|m| rt(m, a, depth + 1)).collect(), *span)
            }
            Type::Infer(s) => Type::Infer(*s),
            Type::Erased(s) => Type::Erased(*s),
        }
    }
    fn rparam(p: &Param, a: &Aliases) -> Param {
        Param {
            ty: rt(&p.ty, a, 0),
            name: p.name.clone(),
            span: p.span,
        }
    }
    fn rstmt(s: &Stmt, a: &Aliases) -> Stmt {
        match s {
            Stmt::VarDecl {
                ty,
                name,
                init,
                mutable,
                span,
            } => Stmt::VarDecl {
                ty: rt(ty, a, 0),
                name: name.clone(),
                init: init.clone(),
                mutable: *mutable,
                span: *span,
            },
            Stmt::For {
                ty,
                name,
                iter,
                body,
                span,
            } => Stmt::For {
                ty: rt(ty, a, 0),
                name: name.clone(),
                iter: iter.clone(),
                body: body.iter().map(|s| rstmt(s, a)).collect(),
                span: *span,
            },
            Stmt::If {
                cond,
                bind,
                then_block,
                else_block,
                span,
            } => Stmt::If {
                cond: cond.clone(),
                bind: bind.clone(),
                then_block: then_block.iter().map(|s| rstmt(s, a)).collect(),
                else_block: else_block
                    .as_ref()
                    .map(|b| b.iter().map(|s| rstmt(s, a)).collect()),
                span: *span,
            },
            Stmt::While {
                cond,
                body,
                post_cond,
                span,
            } => Stmt::While {
                cond: cond.clone(),
                body: body.iter().map(|s| rstmt(s, a)).collect(),
                post_cond: *post_cond,
                span: *span,
            },
            Stmt::CFor {
                init,
                cond,
                step,
                body,
                span,
            } => Stmt::CFor {
                init: init.as_ref().map(|s| Box::new(rstmt(s, a))),
                cond: cond.clone(),
                step: step.as_ref().map(|s| Box::new(rstmt(s, a))),
                body: body.iter().map(|s| rstmt(s, a)).collect(),
                span: *span,
            },
            Stmt::Block(stmts, span) => {
                Stmt::Block(stmts.iter().map(|s| rstmt(s, a)).collect(), *span)
            }
            // A `try`'s catch clause carries a type annotation (possibly an alias) — resolve it and
            // recurse into the bodies (this pass rewrites type annotations only; exprs are cloned).
            Stmt::Try {
                body,
                catches,
                finally_block,
                span,
            } => Stmt::Try {
                body: body.iter().map(|s| rstmt(s, a)).collect(),
                catches: catches
                    .iter()
                    .map(|c| crate::ast::CatchClause {
                        ty: rt(&c.ty, a, 0),
                        name: c.name.clone(),
                        body: c.body.iter().map(|s| rstmt(s, a)).collect(),
                        span: c.span,
                    })
                    .collect(),
                finally_block: finally_block
                    .as_ref()
                    .map(|b| b.iter().map(|s| rstmt(s, a)).collect()),
                span: *span,
            },
            // Throw/Assign/Return/Expr/break/continue carry only exprs or nothing (no type
            // annotations this pass rewrites).
            Stmt::Throw { .. }
            | Stmt::Return { .. }
            | Stmt::Expr(..)
            | Stmt::Assign { .. }
            | Stmt::Break(_)
            | Stmt::Continue(_) => s.clone(),
        }
    }
    fn rfunc(f: &FunctionDecl, a: &Aliases) -> FunctionDecl {
        FunctionDecl {
            modifiers: f.modifiers.clone(),
            vis: f.vis,
            name: f.name.clone(),
            type_params: f.type_params.clone(),
            params: f.params.iter().map(|p| rparam(p, a)).collect(),
            ret: f.ret.as_ref().map(|t| rt(t, a, 0)),
            throws: f.throws.iter().map(|t| rt(t, a, 0)).collect(),
            body: f.body.iter().map(|s| rstmt(s, a)).collect(),
            span: f.span,
        }
    }
    fn rmember(m: &ClassMember, a: &Aliases) -> ClassMember {
        match m {
            ClassMember::Field {
                modifiers,
                ty,
                name,
                init,
                span,
            } => ClassMember::Field {
                modifiers: modifiers.clone(),
                ty: rt(ty, a, 0),
                name: name.clone(),
                // A field initializer is a literal const (no type alias can appear inside it).
                init: init.clone(),
                span: *span,
            },
            ClassMember::Constructor { params, body, span } => ClassMember::Constructor {
                params: params
                    .iter()
                    .map(|p| CtorParam {
                        modifiers: p.modifiers.clone(),
                        ty: rt(&p.ty, a, 0),
                        name: p.name.clone(),
                        span: p.span,
                    })
                    .collect(),
                body: body.iter().map(|s| rstmt(s, a)).collect(),
                span: *span,
            },
            ClassMember::Method(f) => ClassMember::Method(rfunc(f, a)),
            // A property hook (M-mut.7b): expand aliases in its type + set parameter type; the get
            // expression and set block carry no stmt-level type annotations this pass rewrites
            // (consistent with how method-body exprs are treated above), so they pass through.
            ClassMember::Hook {
                ty,
                name,
                get,
                set,
                span,
            } => ClassMember::Hook {
                ty: rt(ty, a, 0),
                name: name.clone(),
                get: get.clone(),
                set: set.as_ref().map(|(p, b)| {
                    (
                        Param {
                            ty: rt(&p.ty, a, 0),
                            name: p.name.clone(),
                            span: p.span,
                        },
                        b.iter().map(|s| rstmt(s, a)).collect(),
                    )
                }),
                span: *span,
            },
        }
    }

    let items = program
        .items
        .iter()
        .filter_map(|item| match item {
            Item::TypeAlias { .. } => None,
            Item::Import { .. } => Some(item.clone()),
            Item::Function(f) => Some(Item::Function(rfunc(f, &aliases))),
            Item::Class(c) => Some(Item::Class(ClassDecl {
                vis: c.vis,
                name: c.name.clone(),
                type_params: c.type_params.clone(),
                extends: c.extends.clone(),
                implements: c.implements.clone(),
                open: c.open,
                is_abstract: c.is_abstract,
                resolutions: c.resolutions.clone(),
                uses: c.uses.clone(),
                members: c.members.iter().map(|m| rmember(m, &aliases)).collect(),
                span: c.span,
            })),
            // M-RT S8: a trait's member type annotations are alias-rewritten exactly like a class's.
            Item::Trait(t) => Some(Item::Trait(crate::ast::TraitDecl {
                name: t.name.clone(),
                members: t.members.iter().map(|m| rmember(m, &aliases)).collect(),
                span: t.span,
            })),
            Item::Interface(i) => Some(Item::Interface(InterfaceDecl {
                vis: i.vis,
                name: i.name.clone(),
                extends: i.extends.clone(),
                methods: i.methods.iter().map(|m| rfunc(m, &aliases)).collect(),
                span: i.span,
            })),
            Item::Enum(e) => Some(Item::Enum(EnumDecl {
                vis: e.vis,
                name: e.name.clone(),
                type_params: e.type_params.clone(),
                variants: e
                    .variants
                    .iter()
                    .map(|v| EnumVariant {
                        name: v.name.clone(),
                        fields: v.fields.iter().map(|p| rparam(p, &aliases)).collect(),
                        span: v.span,
                    })
                    .collect(),
                span: e.span,
            })),
        })
        .collect();

    Program {
        package: program.package.clone(),
        items,
        span: program.span,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::lex;
    use crate::parser::Parser;

    /// Lex + parse `src` into a Program, panicking on lex/parse failure (tests here only care
    /// about type-checking). Auto-prepends the reserved `package Main;` (M5 S1, line-preserving)
    /// unless the source already declares a package, so existing checker tests need no per-case
    /// edit. Use [`prog_raw`] when a test must exercise the package rules themselves.
    fn prog(src: &str) -> Program {
        let src = if src.trim_start().starts_with("package ") {
            src.to_string()
        } else {
            format!("package Main; {src}")
        };
        prog_raw(&src)
    }

    /// Lex + parse without injecting a package — for tests of the package rules themselves.
    fn prog_raw(src: &str) -> Program {
        let tokens = lex(src).expect("lex ok");
        Parser::new(tokens).parse_program().expect("parse ok")
    }

    /// Type-check `src` and return the errors (empty == well-typed).
    fn errors_of(src: &str) -> Vec<Diagnostic> {
        match check(&prog(src)) {
            Ok(_warnings) => Vec::new(),
            Err(e) => e,
        }
    }

    /// Type-check `src` and return the non-fatal warnings (empty unless a lint fired).
    fn warnings_of(src: &str) -> Vec<Diagnostic> {
        check(&prog(src)).unwrap_or_default()
    }

    /// Type-check a *raw* source (no injected package) and return the errors.
    fn errors_of_raw(src: &str) -> Vec<Diagnostic> {
        match check(&prog_raw(src)) {
            Ok(_) => Vec::new(),
            Err(e) => e,
        }
    }

    // --- M-RT S6: single inheritance ---

    #[test]
    fn subclass_is_assignable_and_inherits_methods() {
        // S6a.3: Dog <: Animal (assignability) + Dog inherits Animal's method.
        let errs = errors_of(
            "open class Animal { function name() -> string { return \"a\"; } } \
             class Dog extends Animal {} \
             function f() -> string { Animal a = Dog(); return a.name(); } \
             function g() -> string { Dog d = Dog(); return d.name(); }",
        );
        assert!(errs.is_empty(), "expected clean, got {errs:?}");
    }

    #[test]
    fn extending_a_non_open_class_errors() {
        let errs = errors_of("class Animal {} class Dog extends Animal {}");
        assert!(
            errs.iter().any(|e| e.code == Some("E-EXTEND-FINAL")),
            "got {errs:?}"
        );
    }

    #[test]
    fn extending_an_unknown_name_errors() {
        let errs = errors_of("class Dog extends Bogus {}");
        assert!(
            errs.iter().any(|e| e.code == Some("E-EXTEND-UNKNOWN")),
            "got {errs:?}"
        );
    }

    #[test]
    fn class_extends_cycle_errors() {
        let errs = errors_of("open class A extends B {} open class B extends A {}");
        assert!(
            errs.iter().any(|e| e.code == Some("E-MI-CYCLE")),
            "got {errs:?}"
        );
    }

    #[test]
    fn overriding_a_final_method_errors() {
        // S6a.4: Animal.kind is final-by-default; Dog redefining it is E-OVERRIDE-FINAL.
        let errs = errors_of(
            "open class Animal { function kind() -> string { return \"a\"; } } \
             class Dog extends Animal { function kind() -> string { return \"d\"; } }",
        );
        assert!(
            errs.iter().any(|e| e.code == Some("E-OVERRIDE-FINAL")),
            "got {errs:?}"
        );
    }

    #[test]
    fn overriding_an_open_method_is_allowed() {
        // S6a.4: marking the parent method `open` permits the override.
        let errs = errors_of(
            "open class Animal { open function kind() -> string { return \"a\"; } } \
             class Dog extends Animal { function kind() -> string { return \"d\"; } }",
        );
        assert!(errs.is_empty(), "got {errs:?}");
    }

    #[test]
    fn instantiating_an_abstract_class_errors() {
        // S6b.3: an abstract class cannot be constructed.
        let errs = errors_of(
            "abstract class Shape { abstract function area() -> int; } \
             function main() { Shape s = Shape(); }",
        );
        assert!(
            errs.iter()
                .any(|e| e.code == Some("E-ABSTRACT-INSTANTIATE")),
            "got {errs:?}"
        );
    }

    #[test]
    fn concrete_subclass_missing_abstract_impl_errors() {
        // S6b.3: a non-abstract subclass must implement every inherited abstract method.
        let errs = errors_of(
            "abstract class Shape { abstract function area() -> int; } \
             class Blob extends Shape {}",
        );
        assert!(
            errs.iter().any(|e| e.code == Some("E-ABSTRACT-UNIMPL")),
            "got {errs:?}"
        );
    }

    #[test]
    fn abstract_method_in_concrete_class_errors() {
        // S6b.3: a non-abstract class may not itself declare an abstract method (same check, origin is
        // the class itself).
        let errs = errors_of("class Shape { abstract function area() -> int; }");
        assert!(
            errs.iter().any(|e| e.code == Some("E-ABSTRACT-UNIMPL")),
            "got {errs:?}"
        );
    }

    #[test]
    fn s8_use_unknown_trait_errors() {
        // M-RT S8: `use` must name a declared trait.
        let errs = errors_of("class C { use Nope; } function main() {}");
        assert!(
            errs.iter().any(|e| e.code == Some("E-USE-UNKNOWN")),
            "got {errs:?}"
        );
    }

    #[test]
    fn s8_use_a_class_as_trait_errors() {
        // M-RT S8: `use` composes a trait, not a class — naming a class is E-USE-UNKNOWN.
        let errs = errors_of("class Base {} class C { use Base; } function main() {}");
        assert!(
            errs.iter().any(|e| e.code == Some("E-USE-UNKNOWN")),
            "got {errs:?}"
        );
    }

    #[test]
    fn s8_instanceof_trait_errors() {
        // M-RT S8: a trait is not a type — it cannot be an `instanceof` target.
        let errs = errors_of(
            "trait T { function f() -> int { return 1; } } \
             class C { use T; } \
             function main() { C c = C(); var b = c instanceof T; }",
        );
        assert!(
            errs.iter().any(|e| e.code == Some("E-INSTANCEOF-TYPE")),
            "got {errs:?}"
        );
    }

    #[test]
    fn s8_trait_as_type_annotation_errors() {
        // M-RT S8: a value cannot be typed as a trait.
        let errs = errors_of(
            "trait T { function f() -> int { return 1; } } \
             function main() { T x = x; }",
        );
        assert!(
            errs.iter().any(|e| e.code == Some("E-USE-AS-TYPE")),
            "got {errs:?}"
        );
    }

    #[test]
    fn s8_unmet_trait_abstract_requirement_errors() {
        // M-RT S8: a using class must satisfy a trait's abstract requirement (reuses E-ABSTRACT-UNIMPL).
        let errs = errors_of(
            "trait Greeter { abstract function name() -> string; } \
             class P { use Greeter; } function main() {}",
        );
        assert!(
            errs.iter().any(|e| e.code == Some("E-ABSTRACT-UNIMPL")),
            "got {errs:?}"
        );
    }

    #[test]
    fn s8_two_trait_constructors_collide() {
        // M-RT S8 T3: a class composing two ctor-bearing traits (no own ctor) is E-TRAIT-CTOR-COLLISION.
        let errs = errors_of(
            "trait A { constructor(public int a) {} } \
             trait B { constructor(public int b) {} } \
             class C { use A; use B; } function main() {}",
        );
        assert!(
            errs.iter()
                .any(|e| e.code == Some("E-TRAIT-CTOR-COLLISION")),
            "got {errs:?}"
        );
    }

    #[test]
    fn s8_class_ctor_shadows_trait_ctor_warns() {
        // M-RT S8 T3 (D8): a class's own ctor shadows a `use`d trait's ctor — warning, not error.
        let warns = warnings_of(
            "trait T { constructor(public int id) {} } \
             class C { use T; constructor() {} } function main() {}",
        );
        assert!(
            warns
                .iter()
                .any(|w| w.code == Some("W-TRAIT-CTOR-SHADOWED")),
            "got {warns:?}"
        );
    }

    #[test]
    fn s8_trait_ctor_skips_parent_warns() {
        // M-RT S8 T3 (D6): trait ctor wins over an inherited parent ctor — warn the silent skip.
        let warns = warnings_of(
            "open class Base { constructor(public int b) {} } \
             trait T { constructor(public int id) {} } \
             class C extends Base { use T; } function main() {}",
        );
        assert!(
            warns
                .iter()
                .any(|w| w.code == Some("W-TRAIT-CTOR-PARENT-SKIPPED")),
            "got {warns:?}"
        );
    }

    #[test]
    fn concrete_subclass_implementing_abstract_is_ok() {
        // S6b.3: providing the body satisfies the abstract contract — no error.
        let errs = errors_of(
            "abstract class Shape { abstract function area() -> int; } \
             class Square extends Shape { constructor(public int side) {} \
                 function area() -> int { return this.side * this.side; } }",
        );
        assert!(
            !errs
                .iter()
                .any(|e| matches!(e.code, Some("E-ABSTRACT-UNIMPL") | Some("E-OVERRIDE-FINAL"))),
            "got {errs:?}"
        );
    }

    #[test]
    fn open_static_method_errors() {
        // S6b.3: a method cannot be both `open` and `static` (statics are not virtual).
        let errs = errors_of("class C { open static function f() -> int { return 1; } }");
        assert!(
            errs.iter().any(|e| e.code == Some("E-OPEN-STATIC")),
            "got {errs:?}"
        );
    }

    #[test]
    fn unresolved_cross_parent_collision_errors() {
        // S6b.2: two parents each declare `move`; `Duck` neither resolves nor overrides it.
        let errs = errors_of(
            "open class Swimmer { open function move() -> string { return \"s\"; } } \
             open class Flyer { open function move() -> string { return \"f\"; } } \
             class Duck extends Swimmer, Flyer {}",
        );
        assert!(
            errs.iter().any(|e| e.code == Some("E-MI-CONFLICT")),
            "got {errs:?}"
        );
    }

    #[test]
    fn use_clause_resolves_the_collision() {
        // S6b.2: `use Swimmer.move` picks a winner — no conflict.
        let errs = errors_of(
            "open class Swimmer { open function move() -> string { return \"s\"; } } \
             open class Flyer { open function move() -> string { return \"f\"; } } \
             class Duck extends Swimmer, Flyer { use Swimmer.move }",
        );
        assert!(
            !errs.iter().any(|e| e.code == Some("E-MI-CONFLICT")),
            "got {errs:?}"
        );
    }

    #[test]
    fn exclude_clause_resolves_the_collision() {
        // S6b.2: `exclude Flyer.move` drops one source, leaving `move` unambiguous.
        let errs = errors_of(
            "open class Swimmer { open function move() -> string { return \"s\"; } } \
             open class Flyer { open function move() -> string { return \"f\"; } } \
             class Duck extends Swimmer, Flyer { exclude Flyer.move }",
        );
        assert!(
            !errs.iter().any(|e| e.code == Some("E-MI-CONFLICT")),
            "got {errs:?}"
        );
    }

    #[test]
    fn child_override_resolves_the_collision() {
        // S6b.2: declaring `move` in the child overrides both parents — no conflict (and the parent
        // methods are `open`, so the override itself is legal).
        let errs = errors_of(
            "open class Swimmer { open function move() -> string { return \"s\"; } } \
             open class Flyer { open function move() -> string { return \"f\"; } } \
             class Duck extends Swimmer, Flyer { function move() -> string { return \"d\"; } }",
        );
        assert!(
            !errs.iter().any(|e| e.code == Some("E-MI-CONFLICT")),
            "got {errs:?}"
        );
    }

    #[test]
    fn diamond_shared_base_is_not_a_conflict() {
        // S6b.2: `Mid` reaches `Base.tag` through both arms, but both resolve to the same declaring
        // method — auto-merge, never E-MI-CONFLICT.
        let errs = errors_of(
            "open class Base { open function tag() -> string { return \"b\"; } } \
             open class Left extends Base {} open class Right extends Base {} \
             class Mid extends Left, Right {}",
        );
        assert!(
            !errs.iter().any(|e| e.code == Some("E-MI-CONFLICT")),
            "got {errs:?}"
        );
    }

    #[test]
    fn overriding_a_final_method_of_the_second_parent_errors() {
        // S6b.1: override-finality is checked against *every* parent, not just the first. `Flyer.move`
        // (the second parent) is final-by-default; `Duck` redefining it is E-OVERRIDE-FINAL even
        // though the first parent has no such method.
        let errs = errors_of(
            "open class Swimmer { open function dive() -> string { return \"d\"; } } \
             open class Flyer { function move() -> string { return \"f\"; } } \
             class Duck extends Swimmer, Flyer { function move() -> string { return \"m\"; } }",
        );
        assert!(
            errs.iter().any(|e| e.code == Some("E-OVERRIDE-FINAL")),
            "got {errs:?}"
        );
    }

    // --- M-RT S7: erased generics ---

    #[test]
    fn generic_identity_typechecks_and_infers() {
        // A generic function used at two distinct concrete types — both inferred clean.
        let ok = errors_of(
            "function id<T>(T x) -> T { return x; } \
             function main() { int n = id(42); string s = id(\"hi\"); }",
        );
        assert!(ok.is_empty(), "expected clean, got {ok:?}");
    }

    #[test]
    fn generic_call_result_is_substituted() {
        // `id(42)` returns `int`, so binding it to a `string` is a type error (the return type was
        // unified to the concrete argument type, not left abstract).
        let bad = errors_of(
            "function id<T>(T x) -> T { return x; } function main() { string s = id(42); }",
        );
        assert!(!bad.is_empty(), "expected a type error, got none");
    }

    #[test]
    fn generic_unifies_through_list_and_function() {
        // `firstOr<T>(List<T>, T) -> T` binds T from the list element; `applyTwice<T>(T, (T)->T) -> T`
        // unifies a function-typed parameter. Both infer clean against concrete arguments.
        let ok = errors_of(
            "function firstOr<T>(List<T> xs, T fallback) -> T { for (T x in xs) { return x; } return fallback; } \
             function applyTwice<T>(T x, (T) -> T f) -> T { return f(f(x)); } \
             function main() { List<int> xs = [1, 2]; int a = firstOr(xs, 0); int b = applyTwice(5, fn(int v) => v + 1); }",
        );
        assert!(ok.is_empty(), "expected clean, got {ok:?}");
    }

    #[test]
    fn generic_argument_must_unify_consistently() {
        // Two `T` parameters bound to incompatible concrete types — the second arg cannot match the
        // `int` bound from the first.
        let bad = errors_of(
            "function pairEq<T>(T a, T b) -> bool { return true; } \
             function main() { bool r = pairEq(1, \"x\"); }",
        );
        assert!(!bad.is_empty(), "expected a unification error, got none");
    }

    #[test]
    fn type_param_shadowing_builtin_is_rejected() {
        let e = errors_of("function f<int>(int x) -> int { return x; } function main() {}");
        assert!(
            e.iter().any(|d| d.code == Some("E-GENERIC-PARAM")),
            "got {e:?}"
        );
    }

    #[test]
    fn duplicate_type_param_is_rejected() {
        let e = errors_of("function f<T, T>(T x) -> T { return x; } function main() {}");
        assert!(
            e.iter().any(|d| d.code == Some("E-GENERIC-PARAM")),
            "got {e:?}"
        );
    }

    #[test]
    fn type_param_must_be_pascalcase() {
        let e = errors_of("function f<t>(t x) -> t { return x; } function main() {}");
        assert!(e.iter().any(|d| d.code == Some("E-TYPE-CASE")), "got {e:?}");
    }

    // --- M-RT generics-all: generic *methods* ---

    #[test]
    fn generic_method_typechecks_and_infers() {
        // A generic method on a non-generic class, inferred from arguments at two distinct types.
        let ok = errors_of(
            "class U { function id<T>(T x) -> T { return x; } } \
             function main() { var u = U(); int n = u.id(42); string s = u.id(\"hi\"); }",
        );
        assert!(ok.is_empty(), "expected clean, got {ok:?}");
    }

    #[test]
    fn generic_method_result_is_substituted() {
        // `u.id(42)` returns `int`; binding it to a `string` is a type error — proving the method
        // sig was treated as generic (return unified to the concrete arg), not left abstract or
        // checked by the plain non-generic path.
        let bad = errors_of(
            "class U { function id<T>(T x) -> T { return x; } } \
             function main() { var u = U(); string s = u.id(42); }",
        );
        assert!(!bad.is_empty(), "expected a type error, got none");
    }

    #[test]
    fn generic_method_argument_must_unify_consistently() {
        // Two `T` parameters of a method bound to incompatible concrete types.
        let bad = errors_of(
            "class U { function pairEq<T>(T a, T b) -> bool { return true; } } \
             function main() { var u = U(); bool r = u.pairEq(1, \"x\"); }",
        );
        assert!(!bad.is_empty(), "expected a unification error, got none");
    }

    #[test]
    fn generic_method_param_must_be_pascalcase() {
        let e = errors_of("class U { function f<t>(t x) -> t { return x; } } function main() {}");
        assert!(e.iter().any(|d| d.code == Some("E-TYPE-CASE")), "got {e:?}");
    }

    // --- M-RT generics-all: generic *types* / classes ---

    #[test]
    fn generic_class_construction_infers_and_substitutes() {
        // `Box(7)` infers T=int; `get()` returns int; a two-parameter `Pair<A, B>` binds each
        // parameter independently from its constructor argument.
        let ok = errors_of(
            "class Box<T> { constructor(private T value) {} function get() -> T { return this.value; } } \
             class Pair<A, B> { constructor(private A first, private B second) {} \
                function left() -> A { return this.first; } function right() -> B { return this.second; } } \
             function main() { var b = Box(7); int x = b.get(); \
                var p = Pair(1, \"s\"); int l = p.left(); string r = p.right(); }",
        );
        assert!(ok.is_empty(), "expected clean, got {ok:?}");
    }

    #[test]
    fn generic_class_result_is_substituted() {
        // `Box(7).get()` is int; binding it to a string is an error — proving use-site reification
        // (the instance carries `T=int`, recovered at the member access), not an abstract/mixed result.
        let bad = errors_of(
            "class Box<T> { constructor(private T value) {} function get() -> T { return this.value; } } \
             function main() { var b = Box(7); string s = b.get(); }",
        );
        assert!(!bad.is_empty(), "expected a type error, got none");
    }

    #[test]
    fn generic_class_method_param_substituted() {
        // A method *taking* a `T` rejects a wrong-typed argument at the instance's concrete type.
        let bad = errors_of(
            "class Box<T> { constructor(private T value) {} function orElse(T f) -> T { return this.value; } } \
             function main() { var b = Box(7); int y = b.orElse(\"x\"); }",
        );
        assert!(!bad.is_empty(), "expected an argument type error, got none");
    }

    #[test]
    fn generic_class_annotation_arity_checked() {
        // A bare `Box` annotation (no type argument) on a generic class is an arity error.
        let bad = errors_of(
            "class Box<T> { constructor(private T value) {} function get() -> T { return this.value; } } \
             function main() { Box b = Box(7); }",
        );
        assert!(!bad.is_empty(), "expected an arity error, got none");
    }

    #[test]
    fn generic_class_explicit_type_argument_ok() {
        let ok = errors_of(
            "class Box<T> { constructor(private T value) {} function get() -> T { return this.value; } } \
             function main() { Box<int> b = Box(7); int x = b.get(); }",
        );
        assert!(ok.is_empty(), "expected clean, got {ok:?}");
    }

    // --- M-RT: generic enums (`Option<T>` / `Result<T, E>`) ---

    const OPTION: &str = "enum Option<T> { None, Some(T value) }";
    const RESULT: &str = "enum Result<T, E> { Ok(T value), Err(E error) }";

    #[test]
    fn generic_enum_construction_infers_and_binds() {
        // `Some(7)` infers `Option<int>`; matching it binds the payload at the concrete int, so using
        // the binding where an int is expected is clean.
        let ok = errors_of(&format!(
            "{OPTION} function main() {{ var o = Some(7); \
             int x = match o {{ Some(n) => n, None() => 0 }}; }}"
        ));
        assert!(ok.is_empty(), "expected clean, got {ok:?}");
    }

    #[test]
    fn generic_enum_match_payload_is_concrete() {
        // Matching an `Option<int>` and binding the `Some` payload to a string is a type error —
        // proving the payload is reified to int at the match (via the scrutinee's type argument), not
        // left abstract/mixed.
        let bad = errors_of(&format!(
            "{OPTION} function main() {{ var o = Some(7); \
             string s = match o {{ Some(n) => n, None() => \"x\" }}; }}"
        ));
        assert!(!bad.is_empty(), "expected a type error, got none");
    }

    #[test]
    fn generic_enum_annotation_arity_checked() {
        // A bare `Option` annotation (no type argument) on a generic enum is an arity error.
        let bad = errors_of(&format!(
            "{OPTION} function main() {{ Option o = Some(7); }}"
        ));
        assert!(!bad.is_empty(), "expected an arity error, got none");
    }

    #[test]
    fn generic_enum_annotated_non_inferring_variant_ok() {
        // `None` mentions no `T`, so it cannot infer the argument — annotating the binding fixes it.
        let ok = errors_of(&format!(
            "{OPTION} function main() {{ Option<int> n = None(); \
             int x = match n {{ Some(v) => v, None() => 0 }}; }}"
        ));
        assert!(ok.is_empty(), "expected clean, got {ok:?}");
    }

    #[test]
    fn generic_enum_two_params_independent() {
        // `Result<T, E>` binds `T` from `Ok`'s argument and `E` from `Err`'s, independently.
        let ok = errors_of(&format!(
            "{RESULT} function ok() -> Result<int, string> {{ return Ok(1); }} \
             function bad() -> Result<int, string> {{ return Err(\"no\"); }} \
             function main() {{ string r = match ok() {{ Ok(v) => \"v\", Err(e) => e }}; }}"
        ));
        assert!(ok.is_empty(), "expected clean, got {ok:?}");
    }

    #[test]
    fn generic_enum_variant_arity_checked() {
        // A generic variant constructor still checks its own arity: `Some` takes exactly one field.
        let bad = errors_of(&format!(
            "{OPTION} function main() {{ var o = Some(1, 2); }}"
        ));
        assert!(!bad.is_empty(), "expected an arity error, got none");
    }

    #[test]
    fn generic_enum_param_must_be_pascalcase() {
        // A type parameter shadowing a built-in type name is `E-GENERIC-PARAM`.
        let bad = errors_of("enum Box<int> { Wrap(int x) } function main() {}");
        assert!(
            bad.iter().any(|d| d.code == Some("E-GENERIC-PARAM")),
            "expected E-GENERIC-PARAM, got {bad:?}"
        );
    }

    #[test]
    fn erase_generics_strips_enum_type_params() {
        use crate::ast::{Item, Type};
        let e = erase_generics(prog(&format!("{OPTION} function main() {{}}")));
        let en = e
            .items
            .iter()
            .find_map(|it| match it {
                Item::Enum(en) if en.name == "Option" => Some(en),
                _ => None,
            })
            .expect("enum Option present");
        assert!(en.type_params.is_empty(), "enum type params not erased");
        let some = en
            .variants
            .iter()
            .find(|v| v.name == "Some")
            .expect("Some variant present");
        assert!(
            matches!(some.fields[0].ty, Type::Erased(_)),
            "Some payload not erased: {:?}",
            some.fields[0].ty
        );
    }

    #[test]
    fn non_generic_enum_rejects_type_argument() {
        // A plain (non-generic) enum still takes no type arguments.
        let bad = errors_of("enum Color { Red, Green } function main() { Color<int> c = Red(); }");
        assert!(!bad.is_empty(), "expected an arity error, got none");
    }

    // --- M-faults Slice 2a: `?` error propagation on Result ---

    const RESULT_DEF: &str = "enum Result<T, E> { Ok(T value), Err(E error) }";

    #[test]
    fn propagate_in_result_fn_is_clean() {
        // `?` in a let-initializer inside a `Result`-returning fn unwraps the `Ok` payload (an `int`).
        let ok = errors_of(&format!(
            "{RESULT_DEF} \
             function f() -> Result<int, string> {{ return Ok(1); }} \
             function g() -> Result<int, string> {{ int x = f()?; return Ok(x + 1); }} \
             function main() {{}}"
        ));
        assert!(ok.is_empty(), "expected clean, got {ok:?}");
    }

    #[test]
    fn propagate_outside_let_initializer_is_position_error() {
        // `?` nested in a larger expression is `E-PROPAGATE-POSITION` (not a whole let-initializer).
        let bad = errors_of(&format!(
            "{RESULT_DEF} \
             function f() -> Result<int, string> {{ return Ok(1); }} \
             function g() -> Result<int, string> {{ int x = f()? + 1; return Ok(x); }} \
             function main() {{}}"
        ));
        assert!(
            bad.iter().any(|d| d.code == Some("E-PROPAGATE-POSITION")),
            "expected E-PROPAGATE-POSITION, got {bad:?}"
        );
    }

    #[test]
    fn intrinsic_panic_requires_string_literal() {
        // A non-literal panic message (interpolation) is `E-INTRINSIC-LITERAL`.
        let bad = errors_of(r#"function main() { var n = 1; panic("bad {n}"); }"#);
        assert!(
            bad.iter().any(|d| d.code == Some("E-INTRINSIC-LITERAL")),
            "expected E-INTRINSIC-LITERAL, got {bad:?}"
        );
    }

    #[test]
    fn intrinsic_assert_condition_must_be_bool() {
        let bad = errors_of(r#"function main() { assert(1, "x"); }"#);
        assert!(
            !bad.is_empty(),
            "expected a type error for a non-bool assert condition"
        );
    }

    #[test]
    fn intrinsic_name_is_reserved() {
        let bad = errors_of("function unreachable() { return; } function main() {}");
        assert!(
            bad.iter().any(|d| d.code == Some("E-RESERVED-INTRINSIC")),
            "expected E-RESERVED-INTRINSIC, got {bad:?}"
        );
    }

    #[test]
    fn panic_tail_satisfies_return_totality() {
        // `panic` is `never`-typed, so a value-returning fn ending in it needs no further `return`.
        let ok = errors_of(r#"function f() -> int { panic("x"); } function main() {}"#);
        assert!(
            ok.is_empty(),
            "expected clean (never satisfies totality), got {ok:?}"
        );
    }

    #[test]
    fn propagate_in_non_result_fn_is_context_error() {
        // `?` requires the enclosing fn to return the same `Result` — otherwise `E-PROPAGATE-CONTEXT`.
        let bad = errors_of(&format!(
            "{RESULT_DEF} \
             function f() -> Result<int, string> {{ return Ok(1); }} \
             function g() -> int {{ int x = f()?; return x; }} \
             function main() {{}}"
        ));
        assert!(
            bad.iter().any(|d| d.code == Some("E-PROPAGATE-CONTEXT")),
            "expected E-PROPAGATE-CONTEXT, got {bad:?}"
        );
    }

    // --- M-faults Slice 2b: checked exceptions (throws / throw / try-catch enforcement) ---

    const ERRDEF: &str =
        "class BadInput implements Error { constructor(public string message) {} } \
         class NotFound implements Error { constructor(public string message) {} }";

    #[test]
    fn throw_undeclared_and_uncaught_is_error() {
        // A helper that throws but neither declares `throws` nor wraps it in a `try`.
        let bad = errors_of(&format!(
            "{ERRDEF} function f() {{ throw BadInput(\"x\"); }} function main() {{}}"
        ));
        assert!(
            bad.iter().any(|d| d.code == Some("E-THROW-UNDECLARED")),
            "expected E-THROW-UNDECLARED, got {bad:?}"
        );
    }

    #[test]
    fn throw_declared_then_caught_at_call_is_clean() {
        // `f` declares `throws BadInput` (discharges its own throw); `main` calls it inside a `try`
        // catching `BadInput` (discharges the call). Both sides handled — clean.
        let ok = errors_of(&format!(
            "{ERRDEF} function f() throws BadInput {{ throw BadInput(\"x\"); }} \
             function main() {{ try {{ f(); }} catch (BadInput e) {{}} }}"
        ));
        assert!(ok.is_empty(), "expected clean, got {ok:?}");
    }

    #[test]
    fn throw_in_main_is_uncaught() {
        let bad = errors_of(&format!(
            "{ERRDEF} function main() {{ throw BadInput(\"x\"); }}"
        ));
        assert!(
            bad.iter().any(|d| d.code == Some("E-UNCAUGHT-THROW")),
            "expected E-UNCAUGHT-THROW, got {bad:?}"
        );
    }

    #[test]
    fn main_may_not_declare_throws() {
        let bad = errors_of(&format!(
            "{ERRDEF} function main() throws BadInput {{ throw BadInput(\"x\"); }}"
        ));
        assert!(
            bad.iter().any(|d| d.code == Some("E-UNCAUGHT-THROW")),
            "expected E-UNCAUGHT-THROW, got {bad:?}"
        );
    }

    #[test]
    fn throws_error_root_is_too_broad() {
        let bad = errors_of(&format!(
            "{ERRDEF} function f() throws Error {{ throw BadInput(\"x\"); }} \
             function main() {{ try {{ f(); }} catch (Error e) {{}} }}"
        ));
        assert!(
            bad.iter().any(|d| d.code == Some("E-THROWS-TOO-BROAD")),
            "expected E-THROWS-TOO-BROAD, got {bad:?}"
        );
    }

    #[test]
    fn throw_non_error_value_is_type_error() {
        let bad = errors_of("function main() { throw 42; }");
        assert!(
            bad.iter().any(|d| d.code == Some("E-THROW-TYPE")),
            "expected E-THROW-TYPE, got {bad:?}"
        );
    }

    #[test]
    fn bare_call_to_throwing_fn_is_unhandled() {
        let bad = errors_of(&format!(
            "{ERRDEF} function f() throws BadInput {{ throw BadInput(\"x\"); }} \
             function main() {{ f(); }}"
        ));
        assert!(
            bad.iter().any(|d| d.code == Some("E-CALL-UNHANDLED")),
            "expected E-CALL-UNHANDLED, got {bad:?}"
        );
    }

    #[test]
    fn propagate_throws_to_declared_is_clean() {
        // `g` propagates `f`'s `BadInput` with `?` and declares it — clean; `main` catches the call.
        let ok = errors_of(&format!(
            "{ERRDEF} function f() throws BadInput {{ throw BadInput(\"x\"); }} \
             function g() throws BadInput {{ f()?; }} \
             function main() {{ try {{ g(); }} catch (BadInput e) {{}} }}"
        ));
        assert!(ok.is_empty(), "expected clean, got {ok:?}");
    }

    #[test]
    fn propagate_throws_without_declaration_is_unhandled() {
        // `g` uses `?` but does not declare `throws BadInput` — the propagation is unhandled.
        let bad = errors_of(&format!(
            "{ERRDEF} function f() throws BadInput {{ throw BadInput(\"x\"); }} \
             function g() {{ f()?; }} function main() {{}}"
        ));
        assert!(
            bad.iter().any(|d| d.code == Some("E-CALL-UNHANDLED")),
            "expected E-CALL-UNHANDLED, got {bad:?}"
        );
    }

    #[test]
    fn catch_non_error_type_is_error() {
        let bad = errors_of(&format!(
            "{ERRDEF} function main() {{ try {{}} catch (int e) {{}} }}"
        ));
        assert!(
            bad.iter().any(|d| d.code == Some("E-CATCH-TYPE")),
            "expected E-CATCH-TYPE, got {bad:?}"
        );
    }

    #[test]
    fn shadowed_catch_clause_warns() {
        // A second `catch (BadInput …)` after the first can never run — a non-fatal lint.
        let warns = warnings_of(&format!(
            "{ERRDEF} function f() throws BadInput {{ throw BadInput(\"x\"); }} \
             function main() {{ try {{ f(); }} catch (BadInput e) {{}} catch (BadInput e2) {{}} }}"
        ));
        assert!(
            warns.iter().any(|d| d.code == Some("W-CATCH-UNREACHABLE")),
            "expected W-CATCH-UNREACHABLE, got {warns:?}"
        );
    }

    #[test]
    fn union_catch_covers_each_member() {
        // `catch (BadInput | NotFound e)` discharges a call that throws `BadInput` (a member).
        let ok = errors_of(&format!(
            "{ERRDEF} function f() throws BadInput {{ throw BadInput(\"x\"); }} \
             function main() {{ try {{ f(); }} catch (BadInput | NotFound e) {{}} }}"
        ));
        assert!(ok.is_empty(), "expected clean, got {ok:?}");
    }

    #[test]
    fn try_with_returning_arms_satisfies_totality() {
        // A `-> int` fn whose `try` body and `catch` both return diverges on every path — total.
        let ok = errors_of(&format!(
            "{ERRDEF} function g() -> int {{ try {{ return 1; }} catch (BadInput e) {{ return 0; }} }} \
             function main() {{}}"
        ));
        assert!(ok.is_empty(), "expected clean (try totality), got {ok:?}");
    }

    #[test]
    fn try_falling_through_misses_return() {
        // Both arms fall through, so the `-> int` fn does not return on all paths.
        let bad = errors_of(&format!(
            "{ERRDEF} function g() -> int {{ try {{}} catch (BadInput e) {{}} }} function main() {{}}"
        ));
        assert!(
            bad.iter().any(|d| d.code == Some("E-MISSING-RETURN")),
            "expected E-MISSING-RETURN, got {bad:?}"
        );
    }

    #[test]
    fn throw_tail_satisfies_totality() {
        // A `throw` diverges, so a `-> int` fn whose only statement is a `throw` is total.
        let ok = errors_of(&format!(
            "{ERRDEF} function g() -> int throws BadInput {{ throw BadInput(\"x\"); }} \
             function main() {{ try {{ var n = g(); }} catch (BadInput e) {{}} }}"
        ));
        assert!(ok.is_empty(), "expected clean (throw diverges), got {ok:?}");
    }

    #[test]
    fn throws_mode_propagate_is_recorded_for_erasure() {
        // A throws-mode `?` is a checker-only marker: it must be recorded (mapped to the bare call)
        // so `resolve_html` erases the `Propagate` node before any backend sees it.
        let p = prog(&format!(
            "{ERRDEF} function f() throws BadInput {{ throw BadInput(\"x\"); }} \
             function g() throws BadInput {{ f()?; }} function main() {{}}"
        ));
        let (_warns, subst) = check_resolutions(&p).expect("checks clean");
        assert_eq!(
            subst.len(),
            1,
            "exactly one throws-? recorded, got {subst:?}"
        );
        assert!(
            matches!(subst.values().next(), Some(crate::ast::Expr::Call { .. })),
            "the erased `?` must map to the bare call, got {subst:?}"
        );
        // And the substitution actually removes the `Propagate` node.
        let expanded = resolve_html(p, &subst);
        assert!(
            !program_has_propagate(&expanded),
            "throws-mode `?` Propagate node was not erased"
        );
    }

    /// Recursively scan a checked program for any surviving `Expr::Propagate` node (test helper for
    /// the throws-`?` erasure invariant).
    fn program_has_propagate(p: &Program) -> bool {
        use crate::ast::{ClassMember, Expr, Item, LambdaBody, Stmt};
        fn in_expr(e: &Expr) -> bool {
            match e {
                Expr::Propagate { .. } => true,
                Expr::Unary { expr, .. } | Expr::Force { inner: expr, .. } => in_expr(expr),
                Expr::Binary { lhs, rhs, .. } => in_expr(lhs) || in_expr(rhs),
                Expr::Call { callee, args, .. } => in_expr(callee) || args.iter().any(in_expr),
                Expr::Member { object, .. } => in_expr(object),
                Expr::Index { object, index, .. } => in_expr(object) || in_expr(index),
                Expr::List(items, _) => items.iter().any(in_expr),
                Expr::Match {
                    scrutinee, arms, ..
                } => in_expr(scrutinee) || arms.iter().any(|a| in_expr(&a.body)),
                Expr::Lambda { body, .. } => match body {
                    LambdaBody::Expr(e) => in_expr(e),
                    LambdaBody::Block(b) => b.iter().any(in_stmt),
                },
                _ => false,
            }
        }
        fn in_stmt(s: &Stmt) -> bool {
            match s {
                Stmt::Expr(e, _) | Stmt::Throw { value: e, .. } => in_expr(e),
                Stmt::VarDecl { init, .. } => in_expr(init),
                Stmt::Return { value: Some(e), .. } => in_expr(e),
                Stmt::Block(b, _) => b.iter().any(in_stmt),
                Stmt::Try {
                    body,
                    catches,
                    finally_block,
                    ..
                } => {
                    body.iter().any(in_stmt)
                        || catches.iter().any(|c| c.body.iter().any(in_stmt))
                        || finally_block
                            .as_ref()
                            .is_some_and(|fb| fb.iter().any(in_stmt))
                }
                _ => false,
            }
        }
        fn in_fn(body: &[Stmt]) -> bool {
            body.iter().any(in_stmt)
        }
        p.items.iter().any(|it| match it {
            Item::Function(f) => in_fn(&f.body),
            Item::Class(c) => c.members.iter().any(|m| match m {
                ClassMember::Method(f) => in_fn(&f.body),
                ClassMember::Constructor { body, .. } => in_fn(body),
                _ => false,
            }),
            _ => false,
        })
    }

    // --- M-RT S4: union types + match-over-union ---

    const SHAPES: &str = "class Circle { constructor(public int radius) {} } \
        class Square { constructor(public int side) {} } \
        class Triangle { constructor(public int base) {} }";

    #[test]
    fn union_param_accepts_each_member() {
        let ok = errors_of(&format!(
            "{SHAPES} function f(Circle | Square s) {{}} \
             function main() {{ f(Circle(1)); f(Square(2)); }}"
        ));
        assert!(ok.is_empty(), "expected clean, got {ok:?}");
    }

    #[test]
    fn union_param_rejects_non_member() {
        let bad = errors_of(&format!(
            "{SHAPES} function f(Circle | Square s) {{}} \
             function main() {{ f(Triangle(3)); }}"
        ));
        assert!(
            !bad.is_empty(),
            "expected a type error passing a non-member"
        );
    }

    #[test]
    fn match_over_union_exhaustive_ok() {
        let ok = errors_of(&format!(
            "{SHAPES} function area(Circle | Square s) -> int {{ \
               return match s {{ Circle c => c.radius, Square sq => sq.side }}; }} \
             function main() {{ int a = area(Circle(2)); }}"
        ));
        assert!(ok.is_empty(), "expected clean, got {ok:?}");
    }

    #[test]
    fn match_over_union_non_exhaustive_lists_missing() {
        let bad = errors_of(&format!(
            "{SHAPES} function area(Circle | Square s) -> int {{ \
               return match s {{ Circle c => c.radius }}; }} \
             function main() {{}}"
        ));
        assert!(
            bad.iter()
                .any(|e| e.message.contains("non-exhaustive") && e.message.contains("Square")),
            "{bad:?}"
        );
    }

    #[test]
    fn union_rejects_enum_member() {
        let bad = errors_of(&format!(
            "{SHAPES} enum Color {{ Red, Green }} function f(Circle | Color x) {{}} function main() {{}}"
        ));
        assert!(
            bad.iter().any(|e| e.code == Some("E-UNION-MEMBER")),
            "{bad:?}"
        );
    }

    #[test]
    fn union_arity_collapse_is_error() {
        let bad = errors_of(&format!(
            "{SHAPES} function f(Circle | Circle x) {{}} function main() {{}}"
        ));
        assert!(
            bad.iter().any(|e| e.code == Some("E-UNION-ARITY")),
            "{bad:?}"
        );
    }

    #[test]
    fn type_pattern_must_name_a_class_or_interface() {
        let bad = errors_of(&format!(
            "{SHAPES} function f(Circle | Square s) -> int {{ \
               return match s {{ Circle c => c.radius, Nope n => 0 }}; }} function main() {{}}"
        ));
        assert!(
            bad.iter().any(|e| e.code == Some("E-MATCH-TYPE")),
            "{bad:?}"
        );
    }

    #[test]
    fn instanceof_narrows_a_union_operand() {
        let ok = errors_of(&format!(
            "{SHAPES} function f(Circle | Square s) -> int {{ \
               if (s instanceof Circle) {{ return s.radius; }} return 0; }} function main() {{}}"
        ));
        assert!(ok.is_empty(), "expected clean, got {ok:?}");
    }

    #[test]
    fn primitive_union_literal_match_ok() {
        let ok = errors_of(
            "function classify(int | string code) -> string { \
               return match code { 0 => \"zero\", \"ok\" => \"okay\", _ => \"other\" }; } \
             function main() { string s = classify(0); }",
        );
        assert!(ok.is_empty(), "expected clean, got {ok:?}");
    }

    #[test]
    fn primitive_union_accepts_int_and_string() {
        let ok = errors_of("function f(int | string x) {} function main() { f(1); f(\"a\"); }");
        assert!(ok.is_empty(), "expected clean, got {ok:?}");
    }

    #[test]
    fn type_pattern_nested_in_variant_is_rejected() {
        // A type pattern is top-level-only; nesting it in a variant payload would diverge from the
        // transpiler (which emits only simple payload bindings), so the checker rejects it.
        let bad = errors_of(&format!(
            "{SHAPES} enum Wrap {{ One(Circle inner) }} \
             function f(Wrap w) -> int {{ return match w {{ One(Circle c) => c.radius }}; }} \
             function main() {{}}"
        ));
        assert!(
            bad.iter().any(|e| e.code == Some("E-MATCH-TYPE")),
            "{bad:?}"
        );
    }

    // M-RT S5 — intersection types. Two interfaces and a class implementing both.
    const IFACES: &str = "interface Drawable { function draw() -> string; } \
        interface Named { function name() -> string; } \
        class Badge implements Drawable, Named { \
            constructor(public string label) {} \
            function draw() -> string { return \"[]\"; } \
            function name() -> string { return this.label; } }";

    #[test]
    fn intersection_param_accepts_a_class_implementing_both() {
        // all-members-required-in: a Badge (implements Drawable AND Named) flows into the intersection.
        let ok = errors_of(&format!(
            "{IFACES} function describe(Drawable & Named x) -> string {{ return x.draw(); }} \
             function main() {{ string s = describe(Badge(\"b\")); }}"
        ));
        assert!(ok.is_empty(), "expected clean, got {ok:?}");
    }

    #[test]
    fn intersection_member_access_reaches_each_member() {
        // A method from *each* member interface is in scope on the intersection value.
        let ok = errors_of(&format!(
            "{IFACES} function f(Drawable & Named x) -> string {{ return \"{{x.draw()}} {{x.name()}}\"; }} \
             function main() {{ string s = f(Badge(\"b\")); }}"
        ));
        assert!(ok.is_empty(), "expected clean, got {ok:?}");
    }

    #[test]
    fn intersection_flows_out_to_a_single_member() {
        // some-member-out: A & B is assignable to a slot typed as just one member.
        let ok = errors_of(&format!(
            "{IFACES} function onlyDraw(Drawable d) -> string {{ return d.draw(); }} \
             function f(Drawable & Named x) -> string {{ return onlyDraw(x); }} \
             function main() {{}}"
        ));
        assert!(ok.is_empty(), "expected clean, got {ok:?}");
    }

    #[test]
    fn intersection_one_class_plus_interface_is_allowed() {
        // D1: at most one concrete class plus interfaces is a well-formed intersection.
        let ok = errors_of(&format!(
            "{IFACES} function f(Badge & Drawable x) -> string {{ return x.draw(); }} \
             function main() {{ string s = f(Badge(\"b\")); }}"
        ));
        assert!(ok.is_empty(), "expected clean, got {ok:?}");
    }

    #[test]
    fn intersection_rejects_two_classes() {
        let bad = errors_of(&format!(
            "{SHAPES} function f(Circle & Square x) {{}} function main() {{}}"
        ));
        assert!(
            bad.iter()
                .any(|e| e.code == Some("E-INTERSECT-MULTI-CLASS")),
            "{bad:?}"
        );
    }

    #[test]
    fn intersection_rejects_primitive_member() {
        let bad = errors_of(&format!(
            "{IFACES} function f(int & Drawable x) {{}} function main() {{}}"
        ));
        assert!(
            bad.iter().any(|e| e.code == Some("E-INTERSECT-MEMBER")),
            "{bad:?}"
        );
    }

    #[test]
    fn intersection_arity_collapse_is_error() {
        let bad = errors_of(&format!(
            "{IFACES} function f(Drawable & Drawable x) {{}} function main() {{}}"
        ));
        assert!(
            bad.iter().any(|e| e.code == Some("E-INTERSECT-ARITY")),
            "{bad:?}"
        );
    }

    #[test]
    fn intersection_rejects_conflicting_shared_method_signature() {
        // D2: two members declare `tag` with differing return types — no class can implement both.
        let bad = errors_of(
            "interface A { function tag() -> string; } \
             interface B { function tag() -> int; } \
             function f(A & B x) {} function main() {}",
        );
        assert!(
            bad.iter().any(|e| e.code == Some("E-INTERSECT-SIG")),
            "{bad:?}"
        );
    }

    #[test]
    fn intersection_member_access_unknown_is_error() {
        let bad = errors_of(&format!(
            "{IFACES} function f(Drawable & Named x) -> int {{ return x.nope(); }} function main() {{}}"
        ));
        assert!(
            bad.iter().any(|e| e.code == Some("E-INTERSECT-NO-MEMBER")),
            "{bad:?}"
        );
    }

    #[test]
    fn generic_class_param_must_be_pascalcase() {
        let e = errors_of(
            "class Box<t> { constructor(private t value) {} } function main() { var b = Box(7); }",
        );
        assert!(e.iter().any(|d| d.code == Some("E-TYPE-CASE")), "got {e:?}");
    }

    #[test]
    fn method_type_param_shadowing_class_param_rejected() {
        let e = errors_of(
            "class Box<T> { constructor(private T value) {} function id<T>(T x) -> T { return x; } } \
             function main() { var b = Box(7); }",
        );
        assert!(
            e.iter().any(|d| d.code == Some("E-GENERIC-PARAM")),
            "got {e:?}"
        );
    }

    #[test]
    fn erase_generics_strips_class_type_params() {
        use crate::ast::{ClassMember, Item, Type};
        let p = prog(
            "class Box<T> { constructor(private T value) {} function get() -> T { return this.value; } } function main() {}",
        );
        let e = erase_generics(p);
        let c = e
            .items
            .iter()
            .find_map(|it| match it {
                Item::Class(c) if c.name == "Box" => Some(c),
                _ => None,
            })
            .expect("class Box present");
        assert!(c.type_params.is_empty(), "class type params not erased");
        for m in &c.members {
            match m {
                ClassMember::Constructor { params, .. } => assert!(
                    matches!(params[0].ty, Type::Erased(_)),
                    "ctor param not erased: {:?}",
                    params[0].ty
                ),
                ClassMember::Method(f) if f.name == "get" => assert!(
                    matches!(f.ret, Some(Type::Erased(_))),
                    "method ret not erased: {:?}",
                    f.ret
                ),
                _ => {}
            }
        }
    }

    #[test]
    fn erase_generics_strips_method_type_params() {
        use crate::ast::{ClassMember, Item, Type};
        let p = prog("class U { function id<T>(T x) -> T { return x; } } function main() {}");
        let e = erase_generics(p);
        let m = e
            .items
            .iter()
            .find_map(|it| match it {
                Item::Class(c) => c.members.iter().find_map(|mem| match mem {
                    ClassMember::Method(f) if f.name == "id" => Some(f),
                    _ => None,
                }),
                _ => None,
            })
            .expect("method id present");
        assert!(m.type_params.is_empty(), "method type params not erased");
        assert!(
            matches!(m.params[0].ty, Type::Erased(_)),
            "param type not erased: {:?}",
            m.params[0].ty
        );
        assert!(
            matches!(m.ret, Some(Type::Erased(_))),
            "return type not erased: {:?}",
            m.ret
        );
    }

    #[test]
    fn erase_generics_strips_type_params_and_rewrites_types() {
        use crate::ast::{Item, Type};
        let p = prog("function id<T>(T x) -> T { return x; } function main() {}");
        let e = erase_generics(p);
        let f = e
            .items
            .iter()
            .find_map(|it| match it {
                Item::Function(f) if f.name == "id" => Some(f),
                _ => None,
            })
            .expect("id present");
        assert!(f.type_params.is_empty(), "type params not erased");
        assert!(
            matches!(f.params[0].ty, Type::Erased(_)),
            "param type not erased: {:?}",
            f.params[0].ty
        );
        assert!(
            matches!(f.ret, Some(Type::Erased(_))),
            "return type not erased: {:?}",
            f.ret
        );
    }

    #[test]
    fn map_literal_and_indexing_typecheck() {
        // A well-typed map literal + index of the right key type checks clean.
        let ok =
            errors_of("function main() { Map<string, int> m = [\"a\" => 1]; int x = m[\"a\"]; }");
        assert!(ok.is_empty(), "expected clean, got {ok:?}");
        // Indexing with the wrong key type is an error.
        let bad = errors_of("function main() { Map<string, int> m = [\"a\" => 1]; int x = m[0]; }");
        assert!(
            bad.iter().any(|d| d.message.contains("map index must be")),
            "got {bad:?}"
        );
    }

    #[test]
    fn map_key_must_be_hashable() {
        // A `float` key is not hashable → E-MAP-KEY.
        let e = errors_of("function main() { Map<float, int> m = [1.0 => 1]; }");
        assert!(e.iter().any(|d| d.code == Some("E-MAP-KEY")), "got {e:?}");
    }

    #[test]
    fn package_is_mandatory_and_core_is_reserved() {
        // M5 S1: every file is packaged, never inferred. No declaration → E-NO-PACKAGE.
        let e = errors_of_raw("function main() {}");
        assert!(
            e.iter().any(|d| d.code == Some("E-NO-PACKAGE")),
            "got {e:?}"
        );
        // The `Core` root is reserved for the standard library → E-RESERVED-PACKAGE.
        let e2 = errors_of_raw("package Core; function main() {}");
        assert!(
            e2.iter().any(|d| d.code == Some("E-RESERVED-PACKAGE")),
            "got {e2:?}"
        );
        let e3 = errors_of_raw("package Core.Evil; function main() {}");
        assert!(
            e3.iter().any(|d| d.code == Some("E-RESERVED-PACKAGE")),
            "got {e3:?}"
        );
        // A well-formed user package (and the reserved `Main`) type-check cleanly.
        assert!(check(&prog_raw("package App.Util; function main() {}")).is_ok());
        assert!(check(&prog_raw("package Main; function main() {}")).is_ok());
    }

    #[test]
    fn package_and_import_segments_must_be_pascalcase() {
        // Reshape slice 2b: a lowercase package segment is rejected (E-PKG-CASE).
        let e = errors_of_raw("package app.util; function main() {}");
        assert!(e.iter().any(|d| d.code == Some("E-PKG-CASE")), "got {e:?}");
        // Each non-PascalCase segment is flagged; a single-segment lowercase package too.
        let e2 = errors_of_raw("package acme; function main() {}");
        assert!(
            e2.iter().any(|d| d.code == Some("E-PKG-CASE")),
            "got {e2:?}"
        );
        // A lowercase import path segment is rejected.
        let e3 =
            errors_of_raw("package Main; import acme.util; function main() { int x = util.f(); }");
        assert!(
            e3.iter().any(|d| d.code == Some("E-PKG-CASE")),
            "got {e3:?}"
        );
        // A lowercase import alias is rejected (it occupies a leaf position).
        let e4 = errors_of_raw(
            "package Main; import Acme.Util as util; function main() { int x = util.f(); }",
        );
        assert!(
            e4.iter().any(|d| d.code == Some("E-PKG-CASE")),
            "got {e4:?}"
        );
        // PascalCase package + import + alias type-check cleanly (no E-PKG-CASE noise).
        let ok = errors_of_raw("package App.Util; function main() {}");
        assert!(
            !ok.iter().any(|d| d.code == Some("E-PKG-CASE")),
            "got {ok:?}"
        );
    }

    #[test]
    fn optional_binding_and_null_discipline() {
        // an optional binding accepts `null` and a widened non-null `T`
        assert!(errors_of("function main() { int? x = null; }").is_empty());
        assert!(errors_of("function main() { int? y = 5; }").is_empty());
        // `null` / `T?` cannot flow into a non-optional `T`
        let e1 = errors_of("function main() { int x = null; }");
        assert!(
            e1.iter().any(|d| d.code == Some("E-OPT-ASSIGN")),
            "got {e1:?}"
        );
        let e2 = errors_of("function main() { int? x = null; int y = x; }");
        assert!(
            e2.iter().any(|d| d.code == Some("E-OPT-ASSIGN")),
            "got {e2:?}"
        );
    }

    #[test]
    fn if_let_binding_and_smart_cast() {
        // smart-cast: inside the then-block, the bound name is the non-optional inner `T`
        assert!(
            errors_of("function main() { int? o = 5; if (var x = o) { int y = x; } }").is_empty()
        );
        // the binding is NOT in scope in the else block
        let e1 = errors_of("function main() { int? o = 5; if (var x = o) {} else { int y = x; } }");
        assert!(
            e1.iter().any(|d| d.code == Some("E-UNKNOWN-IDENT")),
            "got {e1:?}"
        );
        // the binding is NOT in scope after the if
        let e2 = errors_of("function main() { int? o = 5; if (var x = o) {} int y = x; }");
        assert!(
            e2.iter().any(|d| d.code == Some("E-UNKNOWN-IDENT")),
            "got {e2:?}"
        );
        // the scrutinee must be optional — binding a non-optional is `E-IF-LET-TYPE`
        let e3 = errors_of("function main() { int n = 5; if (var x = n) {} }");
        assert!(
            e3.iter().any(|d| d.code == Some("E-IF-LET-TYPE")),
            "got {e3:?}"
        );
    }

    #[test]
    fn match_over_optional() {
        // null arm + catch-all binding is exhaustive for `T?`, and the binding narrows to inner `T`
        // (so it can be used as a non-optional — here as an `int` arithmetic operand)
        assert!(errors_of(
            "function f(int? o) -> int { return match o { null => -1, v => v + 1 }; }"
        )
        .is_empty());
        // a `null` pattern requires an optional scrutinee
        let e1 = errors_of("function main() { int n = 3; int x = match n { null => 0, v => v }; }");
        assert!(
            e1.iter().any(|d| d.message.contains("`null` pattern")),
            "got {e1:?}"
        );
        // a `null` arm alone (no catch-all for the non-null case) is non-exhaustive
        let e2 = errors_of("function f(int? o) -> int { return match o { null => -1 }; }");
        assert!(
            e2.iter().any(|d| d.message.contains("non-exhaustive")),
            "got {e2:?}"
        );
    }

    #[test]
    fn force_unwrap_typing_and_lint() {
        // `opt!` unwraps `T?` to `T`; the program type-checks and emits the W-FORCE-UNWRAP lint
        let src = "function main() { int? o = 5; int x = o!; }";
        assert!(errors_of(src).is_empty(), "got {:?}", errors_of(src));
        let w = warnings_of(src);
        assert!(
            w.iter().any(|d| d.code == Some("W-FORCE-UNWRAP")),
            "expected W-FORCE-UNWRAP, got {w:?}"
        );
        // force-unwrapping a non-optional is an error (nothing to unwrap)
        let e = errors_of("function main() { int n = 3; int x = n!; }");
        assert!(
            e.iter().any(|d| d.code == Some("E-OPT-UNWRAP")),
            "got {e:?}"
        );
    }

    #[test]
    fn coalesce_typing() {
        // `T? ?? T` and `null ?? T` both yield the non-optional `T`.
        assert!(errors_of("function main() { int? x = null; int y = x ?? 3; }").is_empty());
        assert!(errors_of("function main() { int y = null ?? 3; }").is_empty());
        // `??` on a non-optional left operand is a misuse.
        assert!(!errors_of("function main() { int a = 1; int y = a ?? 3; }").is_empty());
    }

    #[test]
    fn safe_member_access_typing() {
        let cls =
            "class Box { constructor(private int v) {} function vOf() -> int { return v; } } ";
        // `?.` on an optional yields an optional member, usable via `??`.
        let ok_field = cls.to_string() + "function main() { Box? b = null; int y = (b?.v) ?? -1; }";
        assert!(
            errors_of(&ok_field).is_empty(),
            "{:?}",
            errors_of(&ok_field)
        );
        let ok_method =
            cls.to_string() + "function main() { Box? b = null; int y = (b?.vOf()) ?? -1; }";
        assert!(
            errors_of(&ok_method).is_empty(),
            "{:?}",
            errors_of(&ok_method)
        );
        // plain `.` on an optional is the non-null-discipline violation → E-OPT-USE.
        let bad_field = cls.to_string() + "function main() { Box? b = null; int y = b.v; }";
        let e = errors_of(&bad_field);
        assert!(e.iter().any(|d| d.code == Some("E-OPT-USE")), "got {e:?}");
        let bad_method = cls.to_string() + "function main() { Box? b = null; int y = b.vOf(); }";
        let em = errors_of(&bad_method);
        assert!(em.iter().any(|d| d.code == Some("E-OPT-USE")), "got {em:?}");
    }

    #[test]
    fn empty_program_checks_ok() {
        assert!(errors_of("").is_empty());
    }

    #[test]
    fn var_infers_init_type_and_catches_later_misuse() {
        // `var x = 5` infers int; using it where a string is required is then a type error.
        let errs = errors_of("function main() { var x = 5; string y = x; }");
        assert!(
            errs.iter()
                .any(|e| e.message.contains("expected `string`, found `int`")),
            "{errs:?}"
        );
    }

    #[test]
    fn var_infers_and_well_typed_use_is_clean() {
        assert!(errors_of("function main() { var x = 5; int y = x; }").is_empty());
    }

    #[test]
    fn var_from_null_is_rejected() {
        // A bare `null` has no inferable element type — `var x = null` needs `T? x = null;`.
        let errs = errors_of("function main() { var x = null; }");
        assert!(
            errs.iter().any(|d| d.code == Some("E-INFER-NULL")),
            "got {errs:?}"
        );
    }

    #[test]
    fn type_alias_resolves_and_alias_of_alias_works() {
        // `B` -> `A` -> `int`: a param/return typed `B` checks exactly like `int`.
        let errs = errors_of(
            "type A = int; type B = A; function f(B x) -> B { return x + 1; } function main() {}",
        );
        assert!(errs.is_empty(), "{errs:?}");
    }

    #[test]
    fn type_alias_cycle_is_an_error() {
        let errs = errors_of("type A = B; type B = A; function f(A x) {} function main() {}");
        assert!(errs.iter().any(|e| e.message.contains("cycle")), "{errs:?}");
    }

    #[test]
    fn duplicate_type_name_is_an_error() {
        let errs = errors_of("type A = int; type A = float; function main() {}");
        assert!(
            errs.iter().any(|e| e.message.contains("duplicate")),
            "{errs:?}"
        );
    }

    #[test]
    fn unknown_identifier_suggests_the_nearest_in_scope_name() {
        // `cont` is one edit from the in-scope `count` → the diagnostic carries a code + hint.
        let errs = errors_of(
            "import Core.Console; function main() { int count = 0; Console.println(\"{cont}\"); }",
        );
        let d = errs
            .iter()
            .find(|e| e.message.contains("unknown identifier"))
            .expect("an unknown-identifier error");
        assert_eq!(d.code, Some("E-UNKNOWN-IDENT"));
        assert!(
            d.hint.as_deref().unwrap_or("").contains("count"),
            "hint: {:?}",
            d.hint
        );
    }

    #[test]
    fn snake_case_function_is_rejected() {
        // A function name with `_` is not camelCase → E-NAME-CASE, with a converted-form hint.
        let errs = errors_of("function c_to_f(int c) -> int { return c; } function main() {}");
        let d = errs
            .iter()
            .find(|d| d.code == Some("E-NAME-CASE"))
            .unwrap_or_else(|| panic!("expected E-NAME-CASE, got {errs:?}"));
        assert!(
            d.hint.as_deref().unwrap_or("").contains("cToF"),
            "hint: {:?}",
            d.hint
        );
    }

    #[test]
    fn snake_case_var_binding_is_rejected() {
        // A `var`/typed local binding with `_` is a value identifier → E-NAME-CASE.
        let errs = errors_of("function main() { int my_count = 0; }");
        assert!(
            errs.iter().any(|d| d.code == Some("E-NAME-CASE")),
            "got {errs:?}"
        );
    }

    #[test]
    fn non_pascal_type_enum_variant_is_rejected() {
        // class name, enum name, and a variant name that are not PascalCase → E-TYPE-CASE.
        let cls = errors_of("class box {} function main() {}");
        assert!(
            cls.iter().any(|d| d.code == Some("E-TYPE-CASE")),
            "class: {cls:?}"
        );
        let en = errors_of("enum color { red() } function main() {}");
        // both the enum name `color` and the variant `red` violate PascalCase.
        assert!(
            en.iter().filter(|d| d.code == Some("E-TYPE-CASE")).count() >= 2,
            "enum: {en:?}"
        );
        let alias = errors_of("type myInt = int; function main() {}");
        assert!(
            alias.iter().any(|d| d.code == Some("E-TYPE-CASE")),
            "alias: {alias:?}"
        );
    }

    #[test]
    fn conformant_casing_is_clean() {
        // camelCase fns/params/vars + PascalCase types/enums/variants type-check with no casing error.
        let src = "enum Shape { Circle(float r) } \
                   class Box { constructor(private int width) {} function widthOf() -> int { return width; } } \
                   function areaOf(Shape s) -> int { int localCount = 0; return localCount; } \
                   function main() {}";
        let errs = errors_of(src);
        assert!(
            !errs
                .iter()
                .any(|d| d.code == Some("E-NAME-CASE") || d.code == Some("E-TYPE-CASE")),
            "expected no casing errors, got {errs:?}"
        );
    }

    #[test]
    fn case_converters() {
        assert!(is_camel("main") && is_camel("splitOnce") && !is_camel("split_once"));
        assert!(is_pascal("Shape") && !is_pascal("shape") && !is_pascal("Http_Request"));
        assert_eq!(to_camel("split_once"), "splitOnce");
        assert_eq!(to_camel("c_to_f"), "cToF");
        assert_eq!(to_pascal("shape"), "Shape");
        assert_eq!(to_pascal("http_request"), "HttpRequest");
    }

    #[test]
    fn unknown_type_carries_a_code() {
        let errs = errors_of("function main() { Nope n = 0; }");
        let d = errs
            .iter()
            .find(|e| e.message.contains("unknown type"))
            .expect("an unknown-type error");
        assert_eq!(d.code, Some("E-UNKNOWN-TYPE"));
    }

    #[test]
    fn expand_aliases_dealiases_the_program_for_backends() {
        // After expansion the backends must see no alias names: `B`/`A` collapse to `int`.
        let p =
            prog("type A = int; type B = A; function f(B x) -> B { return x; } function main() {}");
        let e = expand_aliases(&p);
        // no TypeAlias items survive
        assert!(
            !e.items
                .iter()
                .any(|it| matches!(it, crate::ast::Item::TypeAlias { .. })),
            "alias items leaked"
        );
        // f's param + return are now `int`
        if let crate::ast::Item::Function(f) = e
            .items
            .iter()
            .find(|it| matches!(it, crate::ast::Item::Function(_)))
            .unwrap()
        {
            assert!(
                matches!(&f.params[0].ty, crate::ast::Type::Named { name, .. } if name == "int"),
                "param not de-aliased: {:?}",
                f.params[0].ty
            );
            assert!(
                matches!(&f.ret, Some(crate::ast::Type::Named { name, .. }) if name == "int"),
                "return not de-aliased: {:?}",
                f.ret
            );
        } else {
            panic!("no function item");
        }
    }

    #[test]
    fn resolve_maps_primitives_and_list() {
        use crate::ast::Type;
        use crate::token::Span;
        let sp = Span {
            start: 0,
            len: 1,
            line: 1,
            col: 1,
        };
        let mut c = Checker::new();
        assert_eq!(
            c.resolve_type(&Type::Named {
                name: "int".into(),
                args: vec![],
                span: sp
            }),
            Ty::Int
        );
        let list = Type::Named {
            name: "List".into(),
            args: vec![Type::Named {
                name: "int".into(),
                args: vec![],
                span: sp,
            }],
            span: sp,
        };
        assert_eq!(c.resolve_type(&list), Ty::List(Box::new(Ty::Int)));
        assert_eq!(c.errors.len(), 0);
    }

    #[test]
    fn unknown_type_in_var_decl_errors() {
        let errs = errors_of("function main() { Nope n = 0; }");
        assert!(
            errs.iter().any(|e| e.message.contains("unknown type")),
            "{errs:?}"
        );
    }

    #[test]
    fn optional_type_is_now_supported() {
        // `T?` was deferred in M1; M3 S2 makes it a real type (here a widened `0 : int?`).
        assert!(errors_of("function main() { int? n = 0; }").is_empty());
    }

    #[test]
    fn decimal_type_is_deferred_corner() {
        let errs = errors_of("function main() { decimal d = 0; }");
        assert!(
            errs.iter()
                .any(|e| e.message.contains("decimal") && e.message.contains("not yet supported")),
            "{errs:?}"
        );
    }

    #[test]
    fn var_decl_type_mismatch_errors() {
        let errs = errors_of("function main() { int n = true; }");
        assert!(
            errs.iter().any(|e| e.message.contains("expected `int`")),
            "{errs:?}"
        );
    }

    #[test]
    fn good_var_decl_and_arithmetic_ok() {
        assert!(errors_of("function main() { int a = 1; int b = a + 2; }").is_empty());
    }

    #[test]
    fn arithmetic_mixing_int_float_errors() {
        let errs = errors_of("function main() { float x = 1 + 2.0; }");
        assert!(!errs.is_empty(), "mixing int and float must error");
    }

    #[test]
    fn if_condition_must_be_bool() {
        let errs = errors_of("function main() { if (1) { } }");
        assert!(
            errs.iter()
                .any(|e| e.message.contains("condition must be `bool`")),
            "{errs:?}"
        );
    }

    #[test]
    fn equality_requires_same_type() {
        let errs = errors_of("function main() { bool b = 1 == true; }");
        assert!(
            errs.iter().any(|e| e.message.contains("cross-type")),
            "{errs:?}"
        );
    }

    #[test]
    fn unknown_identifier_errors() {
        let errs = errors_of("function main() { int n = missing; }");
        assert!(
            errs.iter()
                .any(|e| e.message.contains("unknown identifier")),
            "{errs:?}"
        );
    }

    #[test]
    fn block_scoping_pops_bindings() {
        let errs = errors_of("function main() { { int x = 1; } int y = x; }");
        assert!(
            errs.iter()
                .any(|e| e.message.contains("unknown identifier")),
            "{errs:?}"
        );
    }

    #[test]
    fn return_type_checked_against_signature() {
        let errs = errors_of("function f() -> int { return true; }");
        assert!(
            errs.iter().any(|e| e.message.contains("expected `int`")),
            "{errs:?}"
        );
    }

    #[test]
    fn function_call_arity_and_type_checked() {
        assert!(errors_of(
            "function inc(int n) -> int { return n + 1; } function main() { int x = inc(1); }"
        )
        .is_empty());
        let bad_arity = errors_of(
            "function inc(int n) -> int { return n; } function main() { int x = inc(1, 2); }",
        );
        assert!(
            bad_arity
                .iter()
                .any(|e| e.message.contains("expects 1 argument")),
            "{bad_arity:?}"
        );
        let bad_type = errors_of(
            "function inc(int n) -> int { return n; } function main() { int x = inc(true); }",
        );
        assert!(
            bad_type.iter().any(|e| e.message.contains("argument 1")),
            "{bad_type:?}"
        );
    }

    #[test]
    fn unknown_function_call_errors() {
        let errs = errors_of("function main() { nope(); }");
        assert!(
            errs.iter().any(|e| e.message.contains("unknown function")),
            "{errs:?}"
        );
    }

    #[test]
    fn overloaded_functions_by_arity_are_legal() {
        // M-RT overloading: same name, distinct parameter signatures, same return type — a valid
        // overload set (was rejected pre-overloading).
        let errs = errors_of("function f() {} function f(int n) {}");
        assert!(errs.is_empty(), "{errs:?}");
    }

    #[test]
    fn overloaded_functions_by_type_are_legal() {
        let errs = errors_of(
            "function show(int x) -> int { return x; } \
             function show(string s) -> int { return 1; }",
        );
        assert!(errs.is_empty(), "{errs:?}");
    }

    #[test]
    fn overload_set_must_share_return_type() {
        let errs = errors_of(
            "function f(int x) -> int { return x; } \
             function f(string s) -> string { return s; }",
        );
        assert!(
            errs.iter().any(|e| e.code == Some("E-OVERLOAD-RETURN")),
            "{errs:?}"
        );
    }

    #[test]
    fn overload_set_rejects_identical_signatures() {
        let errs = errors_of("function f(int x) {} function f(int y) {}");
        assert!(
            errs.iter().any(|e| e.code == Some("E-OVERLOAD-DUPLICATE")),
            "{errs:?}"
        );
    }

    #[test]
    fn generic_function_cannot_be_overloaded() {
        let errs = errors_of(
            "function id<T>(T x) -> T { return x; } \
             function id(int n) -> int { return n; }",
        );
        assert!(
            errs.iter().any(|e| e.code == Some("E-OVERLOAD-GENERIC")),
            "{errs:?}"
        );
    }

    #[test]
    fn overloaded_call_with_no_matching_argument_type_errors() {
        let errs = errors_of(
            "function show(int x) -> int { return x; } \
             function show(string s) -> int { return 1; } \
             function main() { var r = show(true); }",
        );
        assert!(
            errs.iter().any(|e| e.code == Some("E-OVERLOAD-NO-MATCH")),
            "{errs:?}"
        );
    }

    #[test]
    fn println_accepts_string() {
        assert!(errors_of(
            r#"import Core.Console;
function main() { Console.println("hi"); }"#
        )
        .is_empty());
    }

    #[test]
    fn console_println_rejects_non_string() {
        // The native's signature is `(string)`, so an `int` argument is a type error (M3 Wave 1).
        let errs = errors_of(
            r#"import Core.Console;
function main() { Console.println(42); }"#,
        );
        assert!(
            errs.iter().any(|e| e.message.contains("Console.println")),
            "{errs:?}"
        );
    }

    #[test]
    fn bare_println_is_unknown_function() {
        // The global `println` is retired: a bare call now resolves as an unknown free function.
        let errs = errors_of(r#"function main() { println("hi"); }"#);
        assert!(
            errs.iter()
                .any(|e| e.message.contains("unknown function") && e.message.contains("println")),
            "{errs:?}"
        );
    }

    #[test]
    fn console_println_without_import_errors() {
        // "nothing in the wind": without `import Core.Console;`, the qualifier is unbound, so the
        // member call cannot resolve to the native and is an error.
        let errs = errors_of(r#"function main() { Console.println("hi"); }"#);
        assert!(!errs.is_empty(), "expected an error without the import");
    }

    #[test]
    fn generic_native_call_infers_and_substitutes() {
        // A generic native (`Map.keys(Map<K,V>) -> List<K>`, `List.reverse(List<T>) -> List<T>`) is
        // unified at the call site exactly like a generic free function — its `Ty::Param` resolves to
        // the concrete argument types, so a well-typed program type-checks clean (M-RT S7b).
        assert!(errors_of(
            r#"package Main;
import Core.Console;
import Core.List;
import Core.Map;
function main() {
    var nums = [1, 2, 3];
    var rev = List.reverse(nums);
    var total = List.sum(rev);
    var ages = ["a" => 10, "b" => 20];
    var ks = Map.keys(ages);
    var n = Map.size(ages);
    Console.println("{total} {n}");
    for (string k in ks) { Console.println(k); }
}"#
        )
        .is_empty());
    }

    #[test]
    fn generic_native_key_type_mismatch_errors() {
        // `Map.has(Map<string,int>, K)` unifies `K = string` from the receiver, so an `int` key is a
        // type error — the unifier propagates the binding across arguments.
        let errs = errors_of(
            r#"package Main;
import Core.Map;
function main() {
    var ages = ["a" => 10];
    var bad = Map.has(ages, 7);
}"#,
        );
        assert!(
            errs.iter().any(|e| e.message.contains("Map.has")),
            "{errs:?}"
        );
    }

    #[test]
    fn local_shadowing_imported_qualifier_errors() {
        // A value binding may not shadow an imported module qualifier (keeps all backends
        // consistent — see `declare`). Coded `E-SHADOW-IMPORT`. (Stdlib qualifiers are now
        // PascalCase, so a camelCase local can never collide with one — the guard still bites a
        // lowercase user-package leaf, which is what this exercises.)
        let errs = errors_of(
            r#"import acme.helper;
function main() { int helper = 0; int x = helper; }"#,
        );
        assert!(
            errs.iter().any(|e| e.code == Some("E-SHADOW-IMPORT")),
            "{errs:?}"
        );
    }

    #[test]
    fn html_literal_bad_hole_is_coded() {
        // A hole whose type is neither Html, string, nor a primitive is `E-HTML-HOLE` (Core.Html
        // Wave 3): there is no safe HTML rendering for an enum value.
        let errs = errors_of(
            r#"import Core.Html;
enum E { A() }
function main() { var p = html"<h1>{A()}</h1>"; }"#,
        );
        assert!(
            errs.iter().any(|e| e.code == Some("E-HTML-HOLE")),
            "{errs:?}"
        );
    }

    #[test]
    fn html_literal_without_import_is_coded() {
        // `html"…"` desugars to Core.Html kernel calls, so the module must be imported; otherwise
        // `E-HTML-IMPORT`.
        let errs = errors_of(r#"function main() { var p = html"<h1>x</h1>"; }"#);
        assert!(
            errs.iter().any(|e| e.code == Some("E-HTML-IMPORT")),
            "{errs:?}"
        );
    }

    #[test]
    fn local_shadowing_function_name_errors() {
        // A value binding may not shadow a top-level function name: a bare `f(…)` call dispatches
        // functions-first in the run backends but locals-first in the transpiler, so an overlap is
        // a silent four-backend divergence (made reachable once functions became first-class values
        // in M3 S3). Coded `E-SHADOW-FN`. See `declare`.
        let errs = errors_of(
            r#"function dbl(int x) -> int { return x * 2; }
function main() { var dbl = fn(int x) => x + 1000; }"#,
        );
        assert!(
            errs.iter().any(|e| e.code == Some("E-SHADOW-FN")),
            "{errs:?}"
        );
    }

    const SHAPE: &str = "enum Shape { Circle(float radius), Rect(float w, float h), }";

    #[test]
    fn variant_constructor_returns_enum() {
        let src = format!("{SHAPE} function main() {{ Shape s = Circle(2.0); }}");
        assert!(errors_of(&src).is_empty());
    }

    #[test]
    fn variant_constructor_arg_type_checked() {
        let src = format!("{SHAPE} function main() {{ Shape s = Circle(true); }}");
        let errs = errors_of(&src);
        assert!(
            errs.iter().any(|e| e.message.contains("argument 1")),
            "{errs:?}"
        );
    }

    #[test]
    fn list_literal_unifies_elements() {
        let src = format!(
            "{SHAPE} function main() {{ List<Shape> xs = [Circle(1.0), Rect(2.0, 3.0)]; }}"
        );
        assert!(errors_of(&src).is_empty());
    }

    #[test]
    fn list_literal_mixed_elements_error() {
        let errs = errors_of("function main() { List<int> xs = [1, true]; }");
        assert!(
            errs.iter().any(|e| e.message.contains("list elements")),
            "{errs:?}"
        );
    }

    #[test]
    fn for_in_binds_element_type() {
        let src = format!(
            "{SHAPE} function area(Shape s) -> float {{ return 0.0; }} \
             function main() {{ List<Shape> xs = [Circle(1.0)]; for (Shape s in xs) {{ float a = area(s); }} }}"
        );
        assert!(errors_of(&src).is_empty(), "{:?}", errors_of(&src));
    }

    #[test]
    fn for_in_requires_list() {
        let errs = errors_of("function main() { for (int i in 5) { } }");
        assert!(
            errs.iter()
                .any(|e| e.message.contains("`for`-`in` requires a List")),
            "{errs:?}"
        );
    }

    #[test]
    fn range_in_for_checks_clean_and_binds_int() {
        assert!(errors_of("function main() { for (int i in 0..5) { int x = i + 1; } }").is_empty());
        assert!(errors_of("function main() { for (int i in 0..=5) { } }").is_empty());
        // a range bound to a local is `List<int>`
        assert!(errors_of("function main() { List<int> xs = 0..3; }").is_empty());
    }

    #[test]
    fn range_non_int_bound_is_error() {
        let errs = errors_of("function main() { for (int i in 0..3.0) { } }");
        assert!(
            errs.iter()
                .any(|e| e.message.contains("range bounds must be `int`")
                    && e.code == Some("E-RANGE-TYPE")),
            "{errs:?}"
        );
    }

    #[test]
    fn expression_if_unifies_branch_types() {
        assert!(
            errors_of("function main() { var x = if (1 < 2) { 10 } else { 20 }; int y = x; }")
                .is_empty()
        );
    }

    #[test]
    fn expression_if_branch_type_mismatch_errors() {
        let errs = errors_of("function main() { var x = if (true) { 1 } else { false }; }");
        assert!(
            errs.iter()
                .any(|e| e.message.contains("branches must share one type")),
            "{errs:?}"
        );
    }

    #[test]
    fn expression_if_condition_must_be_bool() {
        let errs = errors_of("function main() { var x = if (3) { 1 } else { 2 }; }");
        assert!(
            errs.iter()
                .any(|e| e.message.contains("condition must be `bool`")),
            "{errs:?}"
        );
    }

    #[test]
    fn list_indexing_yields_element() {
        assert!(errors_of("function main() { List<int> xs = [1, 2]; int y = xs[0]; }").is_empty());
    }

    const GREETER: &str = "class Greeter { private string name; constructor(string name) {} function greet() -> string { return \"Hi\"; } }";

    #[test]
    fn constructor_call_and_method_call_ok() {
        let src = format!(
            "{GREETER} function main() {{ Greeter g = Greeter(\"Tak\"); string s = g.greet(); }}"
        );
        assert!(errors_of(&src).is_empty(), "{:?}", errors_of(&src));
    }

    #[test]
    fn constructor_arg_type_checked() {
        let src = format!("{GREETER} function main() {{ Greeter g = Greeter(123); }}");
        let errs = errors_of(&src);
        assert!(
            errs.iter().any(|e| e.message.contains("argument 1")),
            "{errs:?}"
        );
    }

    #[test]
    fn unknown_method_errors() {
        let src =
            format!("{GREETER} function main() {{ Greeter g = Greeter(\"x\"); g.missing(); }}");
        let errs = errors_of(&src);
        assert!(
            errs.iter()
                .any(|e| e.message.contains("no method `missing`")),
            "{errs:?}"
        );
    }

    #[test]
    fn field_access_typed() {
        let src = "class Box { public int n; constructor(int n) {} } function main() { Box b = Box(1); int x = b.n; }";
        assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
    }

    #[test]
    fn bare_field_visible_in_method() {
        let src = "class C { private string name; constructor(string name) {} function who() -> string { return name; } }";
        assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
    }

    #[test]
    fn this_outside_method_errors() {
        let errs = errors_of("function main() { string s = this; }");
        assert!(
            errs.iter().any(|e| e.message.contains("`this`")),
            "{errs:?}"
        );
    }

    #[test]
    fn interpolation_allows_primitives() {
        assert!(errors_of("function main() { float x = 1.5; string s = \"v = {x}\"; }").is_empty());
        assert!(errors_of("function main() { int n = 3; string s = \"n = {n}\"; }").is_empty());
    }

    #[test]
    fn interpolation_rejects_objects() {
        let src = "class C { private int n; constructor(int n) {} } function main() { C c = C(1); string s = \"{c}\"; }";
        let errs = errors_of(src);
        assert!(
            errs.iter()
                .any(|e| e.message.contains("cannot be interpolated")),
            "{errs:?}"
        );
    }

    #[test]
    fn match_over_enum_is_typed_and_exhaustive() {
        let src = format!(
            "{SHAPE} function area(Shape s) -> float {{ \
               return match s {{ Circle(r) => 3.14 * r * r, Rect(w, h) => w * h, }}; }}"
        );
        assert!(errors_of(&src).is_empty(), "{:?}", errors_of(&src));
    }

    #[test]
    fn non_exhaustive_match_errors() {
        let src = format!(
            "{SHAPE} function area(Shape s) -> float {{ \
               return match s {{ Circle(r) => 3.14 * r * r, }}; }}"
        );
        let errs = errors_of(&src);
        assert!(
            errs.iter()
                .any(|e| e.message.contains("non-exhaustive") && e.message.contains("Rect")),
            "{errs:?}"
        );
    }

    #[test]
    fn non_exhaustive_match_lists_missing_variants_sorted() {
        // Variants declared out of alphabetical order; covering the middle one leaves Gamma+Beta
        // missing. The list must render sorted ("Beta, Gamma") regardless of the HashMap key order,
        // so the error message is deterministic across runs (no intermittent test/diff hazard).
        let src = "enum E { Gamma(int x), Alpha(int x), Beta(int x) } \
                   function f(E e) -> int { return match e { Alpha(x) => x, }; } \
                   function main() {}";
        let errs = errors_of(src);
        assert!(
            errs.iter().any(|e| e
                .message
                .contains("non-exhaustive match: missing Beta, Gamma")),
            "{errs:?}"
        );
    }

    #[test]
    fn wildcard_makes_match_exhaustive() {
        let src = format!(
            "{SHAPE} function area(Shape s) -> float {{ \
               return match s {{ Circle(r) => r, _ => 0.0, }}; }}"
        );
        assert!(errors_of(&src).is_empty(), "{:?}", errors_of(&src));
    }

    #[test]
    fn match_arm_type_mismatch_errors() {
        let src = format!(
            "{SHAPE} function f(Shape s) -> float {{ \
               return match s {{ Circle(r) => r, Rect(w, h) => true, }}; }}"
        );
        let errs = errors_of(&src);
        assert!(
            errs.iter().any(|e| e.message.contains("match arms")),
            "{errs:?}"
        );
    }

    #[test]
    fn variant_pattern_arity_checked() {
        let src = format!(
            "{SHAPE} function f(Shape s) -> float {{ \
               return match s {{ Circle(r, x) => r, Rect(w, h) => w, }}; }}"
        );
        let errs = errors_of(&src);
        assert!(
            errs.iter().any(|e| e.message.contains("expects 1 field")),
            "{errs:?}"
        );
    }

    #[test]
    fn unknown_variant_pattern_errors() {
        let src = format!(
            "{SHAPE} function f(Shape s) -> float {{ \
               return match s {{ Circle(r) => r, Triangle(x) => x, Rect(w,h) => w, }}; }}"
        );
        let errs = errors_of(&src);
        assert!(
            errs.iter()
                .any(|e| e.message.contains("no variant `Triangle`")),
            "{errs:?}"
        );
    }

    #[test]
    fn promoted_ctor_param_is_field() {
        // Constructor promotion alone (no explicit `private int total;`) must type-check:
        // the promoted param becomes an instance field, matching the evaluator (EV-4).
        let errs = errors_of(
            "class C { constructor(private int total) {} \
               function add(int n) -> int { return total + n; } }",
        );
        assert!(errs.is_empty(), "promoted field should resolve: {errs:?}");
    }

    #[test]
    fn explicit_field_decl_wins_over_promotion_type() {
        // Explicit field decl is authoritative regardless of member order; a promoted
        // param of the same name does not override its declared type.
        let errs = errors_of(
            "class C { private int total; constructor(private int total) {} \
               function add(int n) -> int { return total + n; } }",
        );
        assert!(
            errs.is_empty(),
            "redundant explicit+promoted (matching type) is fine: {errs:?}"
        );
    }

    #[test]
    fn unmodified_ctor_param_is_not_a_field() {
        // A plain ctor param (no visibility modifier) is NOT promoted, so referencing it
        // bare in a method is still an unknown identifier — matches the evaluator.
        let errs = errors_of(
            "class C { constructor(int total) {} \
               function add(int n) -> int { return total + n; } }",
        );
        assert!(
            errs.iter()
                .any(|e| e.message.contains("unknown identifier")),
            "{errs:?}"
        );
    }

    #[test]
    fn function_typed_binding_rejects_non_function() {
        // (int) -> int f = 5;  -> int not assignable to a function type
        let errs = errors_of("function main() { (int) -> int f = 5; }");
        assert!(
            errs.iter().any(|e| e.message.contains("(int) -> int")),
            "{errs:?}"
        );
    }

    // ---- M-RT S2: interfaces + implements ----

    #[test]
    fn interface_conformance_and_subtyping_ok() {
        // A class providing every interface method type-checks; its instance flows into an
        // interface-typed parameter (nominal subtyping) and an interface-typed local.
        let src = "interface Speaker { function speak() -> string; } \
                   class Dog implements Speaker { function speak() -> string { return \"w\"; } } \
                   function announce(Speaker s) -> string { return s.speak(); } \
                   function main() { Speaker sp = Dog(); announce(sp); }";
        assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
    }

    #[test]
    fn interface_missing_method_is_unimpl() {
        let src = "interface Speaker { function speak() -> string; } \
                   class Mute implements Speaker {} \
                   function main() {}";
        let e = errors_of(src);
        assert!(e.iter().any(|d| d.code == Some("E-IFACE-UNIMPL")), "{e:?}");
    }

    #[test]
    fn interface_wrong_signature_is_sig() {
        // `speak` must return `string`; returning `int` is a signature mismatch.
        let src = "interface Speaker { function speak() -> string; } \
                   class Dog implements Speaker { function speak() -> int { return 1; } } \
                   function main() {}";
        let e = errors_of(src);
        assert!(e.iter().any(|d| d.code == Some("E-IFACE-SIG")), "{e:?}");
    }

    #[test]
    fn implements_a_non_interface_is_impl_error() {
        // `implements` must name a declared interface, not a class.
        let src = "class A {} class B implements A {} function main() {}";
        let e = errors_of(src);
        assert!(e.iter().any(|d| d.code == Some("E-IFACE-IMPL")), "{e:?}");
    }

    #[test]
    fn interface_extends_cycle_is_rejected() {
        let src = "interface A extends B { function a() -> int; } \
                   interface B extends A { function b() -> int; } \
                   function main() {}";
        let e = errors_of(src);
        assert!(e.iter().any(|d| d.code == Some("E-IFACE-CYCLE")), "{e:?}");
    }

    #[test]
    fn interface_is_not_assignable_to_unrelated_class() {
        // A Speaker is not a Dog: interface → concrete class is not a subtype.
        let src = "interface Speaker { function speak() -> string; } \
                   class Dog implements Speaker { function speak() -> string { return \"w\"; } } \
                   function main() { Speaker s = Dog(); Dog d = s; }";
        let e = errors_of(src);
        assert!(!e.is_empty(), "expected an assignability error, got none");
    }

    #[test]
    fn instanceof_against_interface_narrows() {
        // `instanceof` accepts an interface RHS, and inside the then-block the operand is
        // smart-cast to the interface so its methods resolve.
        let src = "interface Speaker { function speak() -> string; } \
                   class Dog implements Speaker { function speak() -> string { return \"w\"; } } \
                   function main() { Dog d = Dog(); \
                     if (d instanceof Speaker) { d.speak(); } }";
        assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
    }

    // ---- M-mut.1: mutable locals + reassignment ----

    #[test]
    fn reassign_immutable_is_error() {
        let bad = errors_of("function main() { int x = 1; x = 2; }");
        assert!(
            bad.iter().any(|e| e.code == Some("E-ASSIGN-IMMUTABLE")),
            "{bad:?}"
        );
    }

    #[test]
    fn reassign_mutable_is_ok() {
        assert!(errors_of("function main() { mutable int x = 1; x = 2; }").is_empty());
    }

    #[test]
    fn reassign_mutable_var_inferred_is_ok() {
        assert!(errors_of("function main() { mutable var x = 1; x = 2; }").is_empty());
    }

    #[test]
    fn reassign_type_mismatch_is_error() {
        let bad = errors_of("function main() { mutable int x = 1; x = \"s\"; }");
        assert!(
            bad.iter().any(|e| e.code == Some("E-ASSIGN-TYPE")),
            "{bad:?}"
        );
    }

    #[test]
    fn reassign_unknown_is_error() {
        let bad = errors_of("function main() { y = 2; }");
        assert!(
            bad.iter().any(|e| e.code == Some("E-ASSIGN-UNKNOWN")),
            "{bad:?}"
        );
    }

    #[test]
    fn field_assign_on_non_class_is_error() {
        // A field-set target whose object is not a class instance is `E-ASSIGN-TARGET` (M-mut.6).
        let bad = errors_of("function main() { mutable int x = 1; x.f = 2; }");
        assert!(
            bad.iter().any(|e| e.code == Some("E-ASSIGN-TARGET")),
            "{bad:?}"
        );
    }

    #[test]
    fn mutable_var_stays_reassignable_in_narrowed_block() {
        // smart-cast interaction (M-mut.1): the narrowed `instanceof` shadow inherits the outer
        // binding's mutability, so a `mutable` var is still reassignable inside the narrowed block.
        let src = "class Dog { constructor() {} } \
                   function main() { mutable Dog d = Dog(); \
                     if (d instanceof Dog) { d = Dog(); } }";
        assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
    }

    // ---- M-mut.2: compound-assign + ++/-- + ??= (desugar reuses the M-mut.1 Assign arm) ----

    #[test]
    fn compound_assign_on_mutable_is_ok() {
        for op in ["+=", "-=", "*=", "/=", "%="] {
            let src = format!("function main() {{ mutable int x = 6; x {op} 2; }}");
            assert!(errors_of(&src).is_empty(), "{op}: {:?}", errors_of(&src));
        }
    }

    #[test]
    fn compound_assign_on_immutable_is_error() {
        // The desugar `x += 1` ⟶ `x = x + 1` inherits the immutability check.
        let bad = errors_of("function main() { int x = 1; x += 1; }");
        assert!(
            bad.iter().any(|e| e.code == Some("E-ASSIGN-IMMUTABLE")),
            "{bad:?}"
        );
    }

    #[test]
    fn increment_on_immutable_is_error() {
        let bad = errors_of("function main() { int x = 1; x++; }");
        assert!(
            bad.iter().any(|e| e.code == Some("E-ASSIGN-IMMUTABLE")),
            "{bad:?}"
        );
    }

    #[test]
    fn increment_on_unknown_is_error() {
        let bad = errors_of("function main() { y++; }");
        assert!(
            bad.iter().any(|e| e.code == Some("E-ASSIGN-UNKNOWN")),
            "{bad:?}"
        );
    }

    #[test]
    fn coalesce_assign_on_optional_is_ok() {
        // `x ??= 0` ⟶ `x = x ?? 0`: assigning the non-null `int` back into the `int?` slot is fine.
        assert!(
            errors_of("function main() { mutable int? x = null; x ??= 0; }").is_empty(),
            "{:?}",
            errors_of("function main() { mutable int? x = null; x ??= 0; }")
        );
    }

    #[test]
    fn increment_on_mutable_is_ok() {
        assert!(errors_of("function main() { mutable int x = 0; x++; x--; }").is_empty());
    }

    // ---- M-mut.3: condition loops + break/continue ----

    #[test]
    fn while_loop_is_ok() {
        assert!(
            errors_of("function main() { mutable int i = 0; while (i < 3) { i += 1; } }")
                .is_empty()
        );
    }

    #[test]
    fn while_condition_must_be_bool() {
        let bad = errors_of("function main() { while (1) { } }");
        assert!(!bad.is_empty(), "expected a non-bool-condition error");
    }

    #[test]
    fn c_for_is_ok() {
        assert!(errors_of(
            "import Core.Console; function main() { for (mutable int i = 0; i < 3; i++) { Console.println(\"{i}\"); } }"
        )
        .is_empty());
    }

    #[test]
    fn c_for_immutable_counter_step_is_error() {
        // The counter is reassigned by the step, so it must be `mutable` (immutable-by-default).
        let bad = errors_of("function main() { for (int i = 0; i < 3; i++) { } }");
        assert!(
            bad.iter().any(|e| e.code == Some("E-ASSIGN-IMMUTABLE")),
            "{bad:?}"
        );
    }

    #[test]
    fn break_outside_loop_is_error() {
        let bad = errors_of("function main() { break; }");
        assert!(
            bad.iter().any(|e| e.code == Some("E-BREAK-OUTSIDE-LOOP")),
            "{bad:?}"
        );
    }

    #[test]
    fn continue_outside_loop_is_error() {
        let bad = errors_of("function main() { continue; }");
        assert!(
            bad.iter()
                .any(|e| e.code == Some("E-CONTINUE-OUTSIDE-LOOP")),
            "{bad:?}"
        );
    }

    #[test]
    fn break_inside_loop_is_ok() {
        assert!(errors_of(
            "function main() { mutable int i = 0; while (i < 9) { i += 1; if (i == 3) { break; } } }"
        )
        .is_empty());
    }

    // ---- M-mut.4a: clone with ----

    #[test]
    fn clone_with_valid_is_ok() {
        let src = "class P { constructor(public int x, public int y) {} } \
                   function main() { P p = P(1, 2); P q = p with { x = 9 }; }";
        assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
    }

    #[test]
    fn clone_with_unknown_field_is_error() {
        let src = "class P { constructor(public int x) {} } \
                   function main() { P p = P(1); P q = p with { z = 9 }; }";
        let bad = errors_of(src);
        assert!(
            bad.iter().any(|e| e.code == Some("E-WITH-FIELD")),
            "{bad:?}"
        );
    }

    #[test]
    fn clone_with_type_mismatch_is_error() {
        let src = "class P { constructor(public int x) {} } \
                   function main() { P p = P(1); P q = p with { x = \"s\" }; }";
        let bad = errors_of(src);
        assert!(bad.iter().any(|e| e.code == Some("E-WITH-TYPE")), "{bad:?}");
    }

    #[test]
    fn clone_with_on_non_class_is_error() {
        let bad = errors_of("function main() { int n = 5; int m = n with { x = 1 }; }");
        assert!(
            bad.iter().any(|e| e.code == Some("E-WITH-NONCLASS")),
            "{bad:?}"
        );
    }

    // ---- M-mut.5: value-type element set ----

    #[test]
    fn list_element_set_is_ok() {
        assert!(
            errors_of("function main() { mutable List<int> xs = [1, 2]; xs[0] = 9; }").is_empty()
        );
    }

    #[test]
    fn map_element_set_is_ok() {
        let src = "function main() { mutable Map<string, int> m = [\"a\" => 1]; m[\"b\"] = 2; }";
        assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
    }

    #[test]
    fn element_set_on_immutable_is_error() {
        let bad = errors_of("function main() { List<int> xs = [1, 2]; xs[0] = 9; }");
        assert!(
            bad.iter().any(|e| e.code == Some("E-ASSIGN-IMMUTABLE")),
            "{bad:?}"
        );
    }

    #[test]
    fn element_set_wrong_value_type_is_error() {
        let bad = errors_of("function main() { mutable List<int> xs = [1]; xs[0] = \"s\"; }");
        assert!(
            bad.iter().any(|e| e.code == Some("E-ASSIGN-TYPE")),
            "{bad:?}"
        );
    }

    #[test]
    fn element_compound_set_is_ok() {
        // `xs[0] += 5` rides the M-mut.2 desugar (`xs[0] = xs[0] + 5`) on an index target.
        assert!(
            errors_of("function main() { mutable List<int> xs = [1, 2]; xs[0] += 5; }").is_empty()
        );
    }

    #[test]
    fn while_let_binds_inner_in_body() {
        // while-let narrows the optional to its non-null inner inside the body (desugars to if-let).
        assert!(errors_of(
            "import Core.Console; function main() { mutable int? o = 5; while (var v = o) { Console.println(\"{v}\"); o = null; } }"
        )
        .is_empty());
    }

    // ---- M-mut.6: shared-mutable instance field set `o.f = e` ----

    #[test]
    fn field_set_on_mutable_field_is_ok() {
        let src = "class P { constructor(public mutable int x) {} } \
                   function main() { P p = P(1); p.x = 2; }";
        assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
    }

    #[test]
    fn field_set_on_immutable_field_is_error() {
        let src = "class P { constructor(public int x) {} } \
                   function main() { P p = P(1); p.x = 2; }";
        let bad = errors_of(src);
        assert!(
            bad.iter().any(|e| e.code == Some("E-ASSIGN-IMMUTABLE")),
            "{bad:?}"
        );
    }

    #[test]
    fn field_set_unknown_field_is_error() {
        let src = "class P { constructor(public mutable int x) {} } \
                   function main() { P p = P(1); p.y = 2; }";
        let bad = errors_of(src);
        assert!(
            bad.iter().any(|e| e.code == Some("E-ASSIGN-UNKNOWN")),
            "{bad:?}"
        );
    }

    #[test]
    fn field_set_wrong_value_type_is_error() {
        let src = "class P { constructor(public mutable int x) {} } \
                   function main() { P p = P(1); p.x = \"s\"; }";
        let bad = errors_of(src);
        assert!(
            bad.iter().any(|e| e.code == Some("E-ASSIGN-TYPE")),
            "{bad:?}"
        );
    }

    #[test]
    fn field_set_via_this_in_method_is_ok() {
        // `this.f = e` inside a method resolves the receiver via `cur_class`; the explicit declared
        // `mutable` field is writable.
        let src = "class C { mutable int n; \
                     constructor(public mutable int seed) { this.n = seed; } \
                     function bump() -> int { this.n = this.n + 1; return this.n; } } \
                   function main() { C c = C(10); c.bump(); }";
        assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
    }

    #[test]
    fn field_set_through_safe_access_is_error() {
        // `o?.f = e` is a meaningless assignment target → `E-ASSIGN-TARGET`.
        let src = "class P { constructor(public mutable int x) {} } \
                   function main() { P? p = P(1); p?.x = 2; }";
        let bad = errors_of(src);
        assert!(
            bad.iter().any(|e| e.code == Some("E-ASSIGN-TARGET")),
            "{bad:?}"
        );
    }

    // ---- M-mut.7: static mutable fields `ClassName.field` ----

    #[test]
    fn static_mutable_field_read_and_write_is_ok() {
        let src = "class C { static mutable int total = 0; } \
                   function main() { C.total = C.total + 1; }";
        assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
    }

    #[test]
    fn static_write_to_immutable_is_error() {
        let src = "class C { static int x = 0; } function main() { C.x = 5; }";
        let bad = errors_of(src);
        assert!(
            bad.iter().any(|e| e.code == Some("E-ASSIGN-IMMUTABLE")),
            "{bad:?}"
        );
    }

    #[test]
    fn static_field_without_initializer_is_error() {
        let bad = errors_of("class C { static mutable int x; }");
        assert!(
            bad.iter().any(|e| e.code == Some("E-STATIC-NO-INIT")),
            "{bad:?}"
        );
    }

    #[test]
    fn static_field_non_const_initializer_is_error() {
        let bad = errors_of("class C { static mutable int x = 1 + 1; }");
        assert!(
            bad.iter().any(|e| e.code == Some("E-STATIC-INIT-CONST")),
            "{bad:?}"
        );
    }

    #[test]
    fn static_field_initializer_type_mismatch_is_error() {
        let bad = errors_of("class C { static mutable int x = \"s\"; }");
        assert!(
            bad.iter().any(|e| e.code == Some("E-STATIC-INIT-TYPE")),
            "{bad:?}"
        );
    }

    #[test]
    fn instance_field_with_initializer_is_error() {
        let bad = errors_of("class C { int x = 5; constructor() {} }");
        assert!(
            bad.iter().any(|e| e.code == Some("E-FIELD-INIT")),
            "{bad:?}"
        );
    }

    #[test]
    fn unknown_static_field_read_is_error() {
        let src = "import Core.Console; class C { static int x = 0; } \
                   function main() { Console.println(\"{C.y}\"); }";
        let bad = errors_of(src);
        assert!(
            bad.iter().any(|e| e.code == Some("E-STATIC-UNKNOWN")),
            "{bad:?}"
        );
    }

    // --- M-RT totality cluster ---

    #[test]
    fn never_resolves_as_a_return_type() {
        // A `-> never` function that diverges (infinite loop) type-checks clean.
        let src = "function spin() -> never { while (true) {} } function main() {}";
        assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
    }

    #[test]
    fn never_is_a_reserved_builtin_type_name() {
        // Aliasing `never` is rejected exactly like aliasing `int`.
        let bad = errors_of("type never = int; function main() {}");
        assert!(
            bad.iter()
                .any(|e| e.message.contains("built-in type `never`")),
            "{bad:?}"
        );
    }

    #[test]
    fn typed_fn_falling_off_the_end_is_error() {
        let bad = errors_of("function f() -> int { } function main() {}");
        assert!(
            bad.iter().any(|e| e.code == Some("E-MISSING-RETURN")),
            "{bad:?}"
        );
    }

    #[test]
    fn if_both_branches_return_is_total() {
        let src = "function f(int x) -> int { if (x > 0) { return 1; } else { return 2; } } \
                   function main() {}";
        assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
    }

    #[test]
    fn if_without_else_falls_through() {
        let bad =
            errors_of("function f(int x) -> int { if (x > 0) { return 1; } } function main() {}");
        assert!(
            bad.iter().any(|e| e.code == Some("E-MISSING-RETURN")),
            "{bad:?}"
        );
    }

    #[test]
    fn if_no_else_then_trailing_return_is_total() {
        let src = "function f(int x) -> int { if (x > 0) { return 1; } return 2; } \
                   function main() {}";
        assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
    }

    #[test]
    fn infinite_loop_tail_is_total() {
        // No explicit return, but `while (true) {}` with no break never falls through.
        let src = "function f() -> int { while (true) {} } function main() {}";
        assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
    }

    #[test]
    fn while_true_with_break_still_needs_return() {
        let bad = errors_of("function f() -> int { while (true) { break; } } function main() {}");
        assert!(
            bad.iter().any(|e| e.code == Some("E-MISSING-RETURN")),
            "{bad:?}"
        );
    }

    #[test]
    fn never_fn_that_can_return_is_error() {
        // A `-> never` body that falls through (could return normally) is rejected.
        let bad = errors_of("function f() -> never { } function main() {}");
        assert!(
            bad.iter().any(|e| e.code == Some("E-NEVER-RETURN")),
            "{bad:?}"
        );
    }

    #[test]
    fn calling_a_never_fn_diverges() {
        // An expression statement calling a `-> never` function terminates the block, so the
        // enclosing `-> int` function needs no further return.
        let src = "function spin() -> never { while (true) {} } \
                   function f() -> int { spin(); } function main() {}";
        assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
    }

    #[test]
    fn unit_fn_needs_no_return() {
        let src =
            "import Core.Console; function f() { Console.println(\"hi\"); } function main() {}";
        assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
    }

    #[test]
    fn return_match_is_total() {
        let src = "enum E { A(), B() } \
                   function f(E e) -> int { return match e { A() => 1, B() => 2 }; } \
                   function main() {}";
        assert!(errors_of(src).is_empty(), "{:?}", errors_of(src));
    }

    #[test]
    fn code_after_return_warns_unreachable_once() {
        let src = "import Core.Console; \
                   function f() -> int { return 1; Console.println(\"x\"); Console.println(\"y\"); } \
                   function main() {}";
        let warns = warnings_of(src);
        let n = warns
            .iter()
            .filter(|w| w.code == Some("W-UNREACHABLE"))
            .count();
        assert_eq!(n, 1, "exactly one dead-region warning: {warns:?}");
    }

    #[test]
    fn clean_function_has_no_unreachable_warning() {
        let src = "function f() -> int { return 1; } function main() {}";
        assert!(
            warnings_of(src)
                .iter()
                .all(|w| w.code != Some("W-UNREACHABLE")),
            "{:?}",
            warnings_of(src)
        );
    }

    #[test]
    fn match_arm_after_catch_all_warns() {
        let src = "function f(int x) -> int { return match x { _ => 0, 1 => 9 }; } \
                   function main() {}";
        assert!(
            warnings_of(src)
                .iter()
                .any(|w| w.code == Some("W-MATCH-UNREACHABLE")),
            "{:?}",
            warnings_of(src)
        );
    }

    #[test]
    fn duplicate_match_literal_arm_warns() {
        let src = "function f(int x) -> int { return match x { 1 => 1, 1 => 2, _ => 0 }; } \
                   function main() {}";
        assert!(
            warnings_of(src)
                .iter()
                .any(|w| w.code == Some("W-MATCH-UNREACHABLE")),
            "{:?}",
            warnings_of(src)
        );
    }

    #[test]
    fn exhaustive_distinct_match_has_no_unreachable_warning() {
        let src = "function f(int x) -> int { return match x { 1 => 1, 2 => 2, _ => 0 }; } \
                   function main() {}";
        assert!(
            warnings_of(src)
                .iter()
                .all(|w| w.code != Some("W-MATCH-UNREACHABLE")),
            "{:?}",
            warnings_of(src)
        );
    }
}
