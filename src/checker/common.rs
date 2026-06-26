//! Stateless checker helpers (no `Checker` state) — case conversion, Levenshtein,
//! substitution, totality/loop predicates, built-in type names. (M-Decomp W2.)

use super::*;

/// Classic two-row Levenshtein edit distance (ASCII-oriented; M1 identifiers are ASCII), used to
/// suggest the nearest in-scope name for an unknown identifier.
/// The reserved fault-intrinsic names (M-faults 2a) — `panic`/`todo`/`unreachable` (`never`) and
/// `assert` (`unit`). Recognized at call sites and rejected as user function names.
pub(super) fn is_intrinsic_name(name: &str) -> bool {
    matches!(name, "panic" | "todo" | "unreachable" | "assert")
}

pub(super) fn levenshtein(a: &str, b: &str) -> usize {
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
pub(super) fn leaf_ident(name: &str) -> &str {
    name.rsplit('\\').next().unwrap_or(name)
}

/// camelCase: a lowercase ASCII first letter and no `_`. A single lowercase word (`main`, `area`,
/// `hi`) qualifies. Empty strings are not valid (the parser never produces them, but be total).
pub(super) fn is_camel(s: &str) -> bool {
    s.chars().next().is_some_and(|c| c.is_ascii_lowercase()) && !s.contains('_')
}

/// PascalCase: an uppercase ASCII first letter and no `_` (`Shape`, `Circle`, `HttpRequest`).
pub(super) fn is_pascal(s: &str) -> bool {
    s.chars().next().is_some_and(|c| c.is_ascii_uppercase()) && !s.contains('_')
}

/// SCREAMING_SNAKE_CASE (Feature A — `const` names): an uppercase ASCII first letter, and every
/// character an uppercase letter, a digit, or `_` — no lowercase. `MAX`, `TAG`, `MAX_SIZE`, `HTTP_2`
/// qualify; `maxVal`, `Max` do not. The PHP/C/Java constant convention, chosen for legibility.
pub(super) fn is_screaming_snake(s: &str) -> bool {
    s.chars().next().is_some_and(|c| c.is_ascii_uppercase())
        && s.chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

/// Convert an identifier to the suggested SCREAMING_SNAKE_CASE form (`maxVal` → `MAX_VAL`,
/// `max_size` → `MAX_SIZE`, `PI` → `PI`): split on `_` and on camelCase humps, uppercase each word,
/// join with `_`.
pub(super) fn to_screaming_snake(s: &str) -> String {
    // First split existing `_` words, then split each on uppercase humps (`maxVal` → `max`,`Val`).
    let mut words: Vec<String> = Vec::new();
    for w in case_words(s) {
        let mut cur = String::new();
        for c in w.chars() {
            if c.is_ascii_uppercase() && !cur.is_empty() {
                words.push(std::mem::take(&mut cur));
            }
            cur.push(c);
        }
        if !cur.is_empty() {
            words.push(cur);
        }
    }
    words
        .iter()
        .map(|w| w.to_ascii_uppercase())
        .collect::<Vec<_>>()
        .join("_")
}

/// Split a snake_case-or-otherwise identifier into its `_`-delimited words, dropping empties (so a
/// leading/trailing/doubled `_` does not yield a blank word). Shared by both converters.
pub(super) fn case_words(s: &str) -> Vec<&str> {
    s.split('_').filter(|w| !w.is_empty()).collect()
}

/// Uppercase the first ASCII letter of a word, leaving the rest unchanged (`shape` → `Shape`,
/// `once` → `Once`). Non-alphabetic leads pass through.
pub(super) fn upper_first(w: &str) -> String {
    let mut cs = w.chars();
    match cs.next() {
        Some(c) => c.to_ascii_uppercase().to_string() + cs.as_str(),
        None => String::new(),
    }
}

/// Convert an identifier to the suggested camelCase form (`split_once` → `splitOnce`,
/// `c_to_f` → `cToF`, `shape` → `shape`): the first word lowercased-first, each later word
/// capitalized, joined with no separator.
pub(super) fn to_camel(s: &str) -> String {
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
pub(super) fn to_pascal(s: &str) -> String {
    case_words(s).iter().map(|w| upper_first(w)).collect()
}

/// True for the built-in type names `resolve_type` handles directly — a `type` alias may not
/// shadow them (else the checker and the backend expansion would disagree; see `collect`).
/// Apply a unification substitution `θ` to a type, replacing each `Ty::Param(p)` by `θ[p]` (an
/// unbound parameter is left as-is). Used to compute a generic call's result type from the bindings
/// inferred at the call site (M-RT S7).
pub(super) fn apply_subst(ty: &Ty, theta: &HashMap<String, Ty>) -> Ty {
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
pub(super) fn ty_has_param(ty: &Ty) -> bool {
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
pub(super) fn is_true_lit(e: &crate::ast::Expr) -> bool {
    matches!(e, crate::ast::Expr::Bool(true, _))
}

/// Whether `stmts` contains a `break` bound to the *current* loop. Descends into `if`/`block` (a
/// `break` there still targets the enclosing loop) but NOT into nested `while`/`for`/`do` loops (their
/// `break`s bind to them). `match` arms are expressions and carry no `break`.
pub(super) fn breaks_this_loop(stmts: &[crate::ast::Stmt]) -> bool {
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

/// Whether a pattern matches *every* value of its static type — it can never fall through. Only a
/// wildcard or plain binding qualifies; a literal, variant, type or struct pattern is a runtime test
/// that can fail. Drives both `match_arm_key` (a refined payload isn't a plain duplicate) and the
/// variant exhaustiveness rule in `check_match` (a refutable payload doesn't discharge coverage).
pub(super) fn is_irrefutable(pat: &crate::ast::Pattern) -> bool {
    use crate::ast::Pattern;
    matches!(pat, Pattern::Wildcard(_) | Pattern::Binding { .. })
}

/// A stable identity for a `match` pattern, for duplicate-arm detection (`W-MATCH-UNREACHABLE`).
/// `None` for patterns that should not be deduplicated: `float` (equality is fuzzy) and the
/// catch-alls (`_`/bare binding, handled separately as a catch-all).
pub(super) fn match_arm_key(p: &crate::ast::Pattern) -> Option<String> {
    use crate::ast::Pattern;
    match p {
        Pattern::Int(v, _) => Some(format!("i{v}")),
        Pattern::Str(s, _) => Some(format!("s{s}")),
        Pattern::Bool(b, _) => Some(format!("b{b}")),
        Pattern::Null(_) => Some("null".to_string()),
        // A variant arm is a duplicate of an earlier one only when both have an *irrefutable* payload
        // (every field a wildcard/binding) — `Some(x)` after `Some(y)` is unreachable, but `Some(0)`
        // and `Some(1)`, or `W(Circle c)` and `W(Square s)` (S5.2-T2), are distinct refinements and
        // must not be flagged. A refined payload yields no dedup key.
        Pattern::Variant { name, fields, .. } if fields.iter().all(is_irrefutable) => {
            Some(format!("v{name}"))
        }
        Pattern::Variant { .. } => None,
        // A type pattern, and a struct pattern with an all-binding payload, share the `t` keyspace:
        // `Point { x }` and `Point p` both match any `Point`, so a later one is an unreachable dup. A
        // struct pattern with a refined field (`Point { x: 0 }`) is not a plain duplicate.
        Pattern::Type { type_name, .. } => Some(format!("t{type_name}")),
        Pattern::Struct {
            type_name, fields, ..
        } if fields.iter().all(|f| is_irrefutable(&f.pat)) => Some(format!("t{type_name}")),
        Pattern::Struct { .. } => None,
        Pattern::Float(_, _) | Pattern::Wildcard(_) | Pattern::Binding { .. } => None,
    }
}

pub(super) fn is_builtin_type_name(name: &str) -> bool {
    matches!(
        name,
        "int"
            | "float"
            | "bool"
            | "string"
            | "bytes"
            | "never"
            | "void"
            | "Empty"
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

/// Names that are reserved *in PHP* for a top-level symbol (free function / class / enum / interface /
/// trait / type-alias) and would therefore transpile to invalid PHP. `var` is the case un-reserved as
/// a Phorge value identifier (contextual keyword): legal as a variable / parameter / field / property /
/// method name (→ PHP `$var` / `->var` / `->var()`), but a PHP parse error as `function var(){}` /
/// `class var{}` (verified against PHP 8.5). The broader PHP-reserved set (`list`/`print`/`clone`/… —
/// already usable Phorge identifiers) is a separate, pre-existing hardening item (KNOWN_ISSUES).
pub(super) fn is_php_reserved_symbol_name(name: &str) -> bool {
    name == "var"
}
