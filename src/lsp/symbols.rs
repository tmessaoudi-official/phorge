//! Symbol resolution for LSP hover + go-to-definition (Item D). A lightweight, robust v1: rather than
//! a full position→type index over the checker, it finds the **identifier token** at the cursor (via
//! the lexer's spans) and resolves that *name* to a top-level declaration (function / class / enum /
//! interface / trait / type alias). This covers the high-value cases — jump to / hover a named symbol
//! — without instrumenting the checker. Locals and field-vs-name collisions are a v2 refinement.
//!
//! Hover renders the declaration's signature by **slicing the source** from the decl's span to the
//! first `{`/`;`, so it shows exactly what the user wrote (no AST type renderer needed).

use super::scope;
use crate::ast::{ClassMember, Item, Program};
use crate::lexer::lex;
use crate::token::{Span, TokenKind};

/// Convert an LSP 0-based `(line, character)` to a byte offset into `text`. `character` is counted in
/// Unicode scalars (correct for ASCII/BMP code; astral-plane columns may differ from a UTF-16 client,
/// a documented v1 simplification). Clamps to the line end; `None` if the line is out of range.
pub fn offset_at(text: &str, line: u32, character: u32) -> Option<usize> {
    let mut byte = 0usize;
    for (i, l) in text.split_inclusive('\n').enumerate() {
        if i as u32 == line {
            for (chars, (boff, _)) in l.char_indices().enumerate() {
                if chars as u32 == character {
                    return Some(byte + boff);
                }
            }
            return Some(byte + l.trim_end_matches(['\r', '\n']).len());
        }
        byte += l.len();
    }
    None
}

/// The identifier name whose token span contains `offset`, if any. Lexes `text` (a lex error ⇒ no
/// symbol — the diagnostics path already reports it).
pub fn ident_at(text: &str, offset: usize) -> Option<String> {
    let tokens = lex(text).ok()?;
    for t in &tokens {
        if let TokenKind::Ident(name) = &t.kind {
            if offset >= t.span.start && offset < t.span.start + t.span.len {
                return Some(name.clone());
            }
        }
    }
    None
}

/// The span of the token whose range *starts at* `offset` (LSP v2 true end-ranges) — used to widen a
/// diagnostic's single-caret position to the full token. Prefers an exact start match (a diagnostic's
/// `(line, col)` points at the offending token's start); falls back to a token that merely contains
/// `offset`. `None` on a lex error or when no token aligns (the caller keeps the caret+1 fallback).
pub fn token_span_at(text: &str, offset: usize) -> Option<Span> {
    let tokens = lex(text).ok()?;
    let mut containing: Option<Span> = None;
    for t in &tokens {
        if t.span.start == offset {
            return Some(t.span);
        }
        if offset > t.span.start && offset < t.span.start + t.span.len {
            containing = Some(t.span);
        }
    }
    containing
}

/// The span of the first identifier token named `name` at or after byte offset `from` — the precise
/// name range of a declaration (whose own span covers the keyword), for a document-symbol
/// `selectionRange`. `None` if not found (the caller falls back to the keyword span).
pub fn name_token_span(text: &str, from: usize, name: &str) -> Option<Span> {
    let tokens = lex(text).ok()?;
    tokens.into_iter().find_map(|t| match t.kind {
        TokenKind::Ident(ref n) if n == name && t.span.start >= from => Some(t.span),
        _ => None,
    })
}

/// Every identifier token in `text` named `name`, by span (in source order). The raw occurrence set
/// for references/rename/highlight; the caller filters each occurrence to those resolving to the same
/// declaration (scope-accurate). `[]` on a lex error.
pub fn all_ident_spans(text: &str, name: &str) -> Vec<Span> {
    match lex(text) {
        Ok(tokens) => tokens
            .into_iter()
            .filter_map(|t| match t.kind {
                TokenKind::Ident(ref n) if n == name => Some(t.span),
                _ => None,
            })
            .collect(),
        Err(_) => Vec::new(),
    }
}

/// The declaration of a top-level item named `name`: its kind label, name, and span. `None` if no such
/// top-level symbol (a local/parameter/field is not resolved in v1).
pub fn definition_of<'a>(program: &'a Program, name: &str) -> Option<(&'a str, Span)> {
    for item in &program.items {
        let hit = match item {
            Item::Function(f) if f.name == name => Some(("function", f.span)),
            Item::Class(c) if c.name == name => Some(("class", c.span)),
            Item::Enum(e) if e.name == name => Some(("enum", e.span)),
            Item::Interface(i) if i.name == name => Some(("interface", i.span)),
            Item::Trait(t) if t.name == name => Some(("trait", t.span)),
            Item::TypeAlias { name: n, span, .. } if n == name => Some(("type", *span)),
            _ => None,
        };
        if hit.is_some() {
            return hit;
        }
    }
    None
}

/// Render the one-line signature of a declaration by slicing the source from the decl's start to the
/// first `{` (body open) or `;` (type alias) — exactly the user's written signature.
pub fn signature_text(text: &str, span: Span) -> String {
    // The decl's `span` covers only its keyword/name, so scan forward from its start to the body open
    // `{` (or `;` for a type alias / expression-body fn) — that delimiter always appears within a
    // short distance for these item kinds. Char-safe and defensively capped so a malformed buffer
    // can't slice the whole file or a non-char boundary.
    let start = span.start.min(text.len());
    let rest = &text[start..];
    let mut cut = rest.len();
    for (i, c) in rest.char_indices() {
        if c == '{' || c == ';' || i > 500 {
            cut = i;
            break;
        }
    }
    rest[..cut].split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Every top-level declaration as `(name, CompletionItemKind)` for completion. Imports are skipped
/// (they bind a module qualifier, not a completable value name). CompletionItemKind: Function=3,
/// Class=7, Interface=8, Enum=13, Struct=22 (trait), TypeParameter=25 (type alias).
pub fn top_level_symbols(program: &Program) -> Vec<(String, u32)> {
    let mut out = Vec::new();
    for it in &program.items {
        match it {
            Item::Function(f) => out.push((f.name.clone(), 3)),
            Item::Class(c) => out.push((c.name.clone(), 7)),
            Item::Interface(i) => out.push((i.name.clone(), 8)),
            Item::Enum(e) => out.push((e.name.clone(), 13)),
            Item::Trait(t) => out.push((t.name.clone(), 22)),
            Item::TypeAlias { name, .. } => out.push((name.clone(), 25)),
            Item::Import { .. } | Item::Test { .. } => {}
        }
    }
    out
}

/// A hierarchical `DocumentSymbol[]` outline (LSP v2). Each top-level item's `range` is
/// `[item.start .. next_item.start)` (so children are always contained, per the spec's nesting rule)
/// and its `selectionRange` is the name token. Classes/enums/interfaces/traits carry their
/// members/variants as children. SymbolKind: Function=12, Class=5, Enum=10, Interface=11, Struct=23
/// (trait), TypeParameter=26 (type alias); members Field=8, Method=6, Constructor=9, Property=7,
/// EnumMember=22.
pub fn document_symbols_json(text: &str, program: &Program) -> String {
    let items = &program.items;
    let mut out: Vec<String> = Vec::new();
    for (i, it) in items.iter().enumerate() {
        let range_start = scope::item_span(it).start;
        let range_end = items
            .get(i + 1)
            .map_or(text.len(), |n| scope::item_span(n).start);
        let node = match it {
            Item::Function(f) => Some(symbol_node(text, &f.name, 12, range_start, range_end, &[])),
            Item::Class(c) => {
                let kids = class_children(text, &c.members);
                Some(symbol_node(text, &c.name, 5, range_start, range_end, &kids))
            }
            Item::Trait(t) => {
                let kids = class_children(text, &t.members);
                Some(symbol_node(
                    text,
                    &t.name,
                    23,
                    range_start,
                    range_end,
                    &kids,
                ))
            }
            Item::Interface(itf) => {
                let kids: Vec<String> = itf
                    .methods
                    .iter()
                    .map(|m| leaf_node(text, &m.name, 6, m.span.start))
                    .collect();
                Some(symbol_node(
                    text,
                    &itf.name,
                    11,
                    range_start,
                    range_end,
                    &kids,
                ))
            }
            Item::Enum(e) => {
                let kids: Vec<String> = e
                    .variants
                    .iter()
                    .map(|v| leaf_node(text, &v.name, 22, v.span.start))
                    .collect();
                Some(symbol_node(
                    text,
                    &e.name,
                    10,
                    range_start,
                    range_end,
                    &kids,
                ))
            }
            Item::TypeAlias { name, .. } => {
                Some(symbol_node(text, name, 26, range_start, range_end, &[]))
            }
            Item::Import { .. } | Item::Test { .. } => None,
        };
        if let Some(n) = node {
            out.push(n);
        }
    }
    format!("[{}]", out.join(","))
}

/// The child `DocumentSymbol`s for a class/trait body (fields, constructor, methods, property hooks).
fn class_children(text: &str, members: &[ClassMember]) -> Vec<String> {
    members
        .iter()
        .map(|m| match m {
            ClassMember::Field { name, span, .. } => leaf_node(text, name, 8, span.start),
            ClassMember::Method(f) => leaf_node(text, &f.name, 6, f.span.start),
            ClassMember::Constructor { span, .. } => leaf_node(text, "constructor", 9, span.start),
            ClassMember::Hook { name, span, .. } => leaf_node(text, name, 7, span.start),
        })
        .collect()
}

/// A `DocumentSymbol` with children: `range` spans `[range_start .. range_end)`, `selectionRange` is
/// the name token (or a 1-char fallback at `range_start`).
fn symbol_node(
    text: &str,
    name: &str,
    kind: u32,
    range_start: usize,
    range_end: usize,
    children: &[String],
) -> String {
    let (sl, sc) = scope::position_at(text, range_start);
    let (el, ec) = scope::position_at(text, range_end);
    let sel = selection_range(text, range_start, name);
    format!(
        "{{\"name\":\"{}\",\"kind\":{kind},\"range\":{},\"selectionRange\":{},\"children\":[{}]}}",
        json_escape(name),
        range_obj(sl, sc, el, ec),
        sel,
        children.join(",")
    )
}

/// A leaf `DocumentSymbol` (no children): `range` == `selectionRange` == the name token (or a 1-char
/// fallback at `anchor`).
fn leaf_node(text: &str, name: &str, kind: u32, anchor: usize) -> String {
    let sel = selection_range(text, anchor, name);
    format!(
        "{{\"name\":\"{}\",\"kind\":{kind},\"range\":{sel},\"selectionRange\":{sel}}}",
        json_escape(name),
    )
}

/// The LSP `Range` of the name token at/after `anchor` (or a 1-char range at `anchor` if not found).
fn selection_range(text: &str, anchor: usize, name: &str) -> String {
    let span = name_token_span(text, anchor, name);
    let (start, len) = span.map_or((anchor, 1), |s| (s.start, s.len.max(1)));
    let (sl, sc) = scope::position_at(text, start);
    let (el, ec) = scope::position_at(text, start + len);
    range_obj(sl, sc, el, ec)
}

/// An LSP `Range` JSON object from 0-based start/end `(line, character)` pairs.
fn range_obj(sl: u32, sc: u32, el: u32, ec: u32) -> String {
    format!(
        "{{\"start\":{{\"line\":{sl},\"character\":{sc}}},\"end\":{{\"line\":{el},\"character\":{ec}}}}}"
    )
}

/// Minimal JSON string escaping for symbol names (a local copy; `mod.rs::escape` is module-private).
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// Render a *local* binding's one-line text by slicing from its span to the first statement/clause
/// delimiter (`;`, `,`, `)`, `{`, or a newline) — so a `var x = expr;` hover shows `var x = expr` and a
/// parameter `int n` hover shows `int n`. Char-safe, capped at 200 bytes.
pub fn local_signature_text(text: &str, span: Span) -> String {
    let start = span.start.min(text.len());
    let rest = &text[start..];
    let mut cut = rest.len();
    for (i, c) in rest.char_indices() {
        if matches!(c, ';' | ',' | ')' | '{' | '\n') || i > 200 {
            cut = i;
            break;
        }
    }
    rest[..cut].split_whitespace().collect::<Vec<_>>().join(" ")
}
