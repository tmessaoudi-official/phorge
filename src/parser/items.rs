//! Recursive-descent parser — items (M-Decomp W3.1). See parser/mod.rs for the struct + token-stream primitives.

use super::*;

impl Parser {
    /// Parse one top-level item: an optional visibility prefix (`public`/`internal`/`private`)
    /// followed by `import` / `function` / `enum` / `class` / `interface` / `type`. The prefix is
    /// stamped onto the declaration by the free `stamp_visibility`.
    pub fn parse_item(&mut self) -> Result<Item, Diagnostic> {
        let sp = self.peek_span();
        // Optional leading declaration visibility (visibility modifiers): at most one of
        // public/internal/private. Absent ⇒ the default `Visibility::Public`.
        let vis = self.parse_decl_visibility()?;
        // Optional `open`/`abstract` class prefixes (M-RT S6/S6b), in any order. Both apply only to a
        // class; `abstract` implies extensibility (an abstract class exists to be subclassed), so it
        // also marks the class `open`.
        let mut is_open = false;
        let mut is_abstract = false;
        loop {
            if self.eat(&TokenKind::Open) {
                is_open = true;
            } else if self.eat(&TokenKind::Abstract) {
                is_abstract = true;
            } else {
                break;
            }
        }
        if (is_open || is_abstract) && !self.check(&TokenKind::Class) {
            return Err(self.error("only a class can be declared `open` or `abstract`"));
        }
        let item = match self.peek() {
            TokenKind::Import => {
                if vis != Visibility::Public {
                    return Err(self.error("an import cannot carry a visibility modifier"));
                }
                return self.parse_import(sp);
            }
            TokenKind::TypeKw => {
                if vis != Visibility::Public {
                    return Err(self.error("a type alias cannot carry a visibility modifier yet"));
                }
                return self.parse_type_alias(sp);
            }
            TokenKind::Function => Item::Function(self.parse_function(Vec::new(), sp)?),
            TokenKind::Enum => Item::Enum(self.parse_enum(sp)?),
            TokenKind::Class => {
                Item::Class(self.parse_class(sp, is_open || is_abstract, is_abstract)?)
            }
            TokenKind::Interface => Item::Interface(self.parse_interface(sp)?),
            TokenKind::Trait => {
                if vis != Visibility::Public {
                    return Err(self.error("a trait cannot carry a visibility modifier yet"));
                }
                return Ok(Item::Trait(self.parse_trait(sp)?));
            }
            TokenKind::Package => {
                return Err(self.error(
                    "'package' must be the first declaration, before any import or definition",
                ))
            }
            _ => {
                return Err(self
                    .error("a top-level item (import, function, enum, class, interface, or type)"))
            }
        };
        Ok(stamp_visibility(item, vis))
    }

    /// Read an optional single leading declaration-visibility keyword. Two visibility keywords in a
    /// row (`public private`) is an error; absent ⇒ the default `Visibility::Public`.
    pub(super) fn parse_decl_visibility(&mut self) -> Result<Visibility, Diagnostic> {
        let first = match self.peek() {
            TokenKind::Public => Visibility::Public,
            TokenKind::Internal => Visibility::Internal,
            TokenKind::Private => Visibility::Private,
            _ => return Ok(Visibility::Public),
        };
        self.advance();
        if matches!(
            self.peek(),
            TokenKind::Public | TokenKind::Internal | TokenKind::Private
        ) {
            return Err(self.error("a single visibility (public, internal, or private), not two"));
        }
        Ok(first)
    }

    /// Entry point: parse a whole program — an optional leading `package …;` (M5: required by the
    /// checker, but parsed optionally so its absence is a typed `E-NO-PACKAGE`, not a parse error)
    /// followed by zero or more top-level items until EOF.
    pub fn parse_program(&mut self) -> Result<Program, Diagnostic> {
        let sp = self.peek_span();
        let package = if self.check(&TokenKind::Package) {
            self.parse_package()?
        } else {
            Vec::new()
        };
        let mut items = Vec::new();
        while !self.check(&TokenKind::Eof) {
            items.push(self.parse_item()?);
        }
        Ok(Program {
            package,
            items,
            span: sp,
        })
    }

    /// `package a.b.c;` — dotted package path at the file top. Assumes current token is `package`.
    pub(super) fn parse_package(&mut self) -> Result<Vec<String>, Diagnostic> {
        self.expect(&TokenKind::Package, "'package'")?;
        let mut path = vec![self.expect_ident("a package path segment")?];
        while self.eat(&TokenKind::Dot) {
            path.push(self.expect_ident("a package path segment after '.'")?);
        }
        self.expect(&TokenKind::Semicolon, "';' after package")?;
        Ok(path)
    }

    /// `import a.b.c;` / `import a.b.c as leaf;` — a module import (Go-qualified `c.fn()` calls).
    /// `import type a.b.C;` / `import type a.b.C as D;` — a *terminal type* import: the leaf `C` is a
    /// user/library type, bound bare (or as `D`). `type` and `as` are **contextual** keywords
    /// (recognized only here), so they stay valid identifiers elsewhere. Assumes current token is
    /// `import`.
    pub(super) fn parse_import(&mut self, sp: Span) -> Result<Item, Diagnostic> {
        self.expect(&TokenKind::Import, "'import'")?;
        let type_only = self.eat(&TokenKind::TypeKw); // contextual `import type …`
        let mut path = vec![self.expect_ident("a module path segment")?];
        while self.eat(&TokenKind::Dot) {
            path.push(self.expect_ident("a module path segment after '.'")?);
        }
        let alias = if matches!(self.peek(), TokenKind::Ident(s) if s == "as") {
            self.advance(); // consume `as`
            Some(self.expect_ident("an alias after 'as'")?)
        } else {
            None
        };
        self.expect(&TokenKind::Semicolon, "';' after import")?;
        Ok(Item::Import {
            path,
            alias,
            type_only,
            span: sp,
        })
    }

    /// `type Name = Type;` — a top-level alias. Assumes the current token is `type`.
    pub(super) fn parse_type_alias(&mut self, sp: Span) -> Result<Item, Diagnostic> {
        self.expect(&TokenKind::TypeKw, "'type'")?;
        let name = self.expect_ident("an alias name after 'type'")?;
        self.expect(&TokenKind::Eq, "'=' in type alias")?;
        let ty = self.parse_type()?;
        self.expect(&TokenKind::Semicolon, "';' after type alias")?;
        Ok(Item::TypeAlias { name, ty, span: sp })
    }

    /// `function name(params) [-> RetType] BLOCK`. `modifiers` are pre-parsed by the caller
    /// (empty for a free function; populated for a method).
    pub(super) fn parse_function(
        &mut self,
        modifiers: Vec<Modifier>,
        sp: Span,
    ) -> Result<FunctionDecl, Diagnostic> {
        self.expect(&TokenKind::Function, "'function'")?;
        let name = self.expect_ident("a function name")?;
        let type_params = self.parse_type_params()?;
        self.expect(&TokenKind::LParen, "'(' after function name")?;
        let params = self.parse_params()?;
        self.expect(&TokenKind::RParen, "')' to close parameters")?;
        let ret = if self.eat(&TokenKind::Arrow) {
            Some(self.parse_type()?)
        } else {
            None
        };
        // `throws T (| T)*` (M-faults 2b). `parse_type` consumes a `A | B` union natively, so one
        // call captures the whole declared set as a single (possibly `Union`) `Type`; the checker
        // flattens it. Empty when the clause is absent.
        let throws = if self.eat(&TokenKind::Throws) {
            vec![self.parse_type()?]
        } else {
            Vec::new()
        };
        // M-RT S6b: an `abstract` method is a bodyless signature terminated by `;` (a concrete
        // subclass supplies the body). Every other method/function parses a block.
        let body = if modifiers.contains(&Modifier::Abstract) {
            self.expect(
                &TokenKind::Semicolon,
                "';' after an abstract method signature",
            )?;
            Vec::new()
        } else {
            self.parse_block()?
        };
        Ok(FunctionDecl {
            modifiers,
            vis: Visibility::Public,
            name,
            type_params,
            params,
            ret,
            throws,
            body,
            span: sp,
        })
    }

    /// Comma-separated `Type name` parameters up to (not including) `)`.
    /// Allows zero params; allows a trailing comma.
    pub(super) fn parse_params(&mut self) -> Result<Vec<Param>, Diagnostic> {
        let mut params = Vec::new();
        if self.check(&TokenKind::RParen) {
            return Ok(params);
        }
        loop {
            let sp = self.peek_span();
            let ty = self.parse_type()?;
            let name = self.expect_ident("a parameter name")?;
            params.push(Param { ty, name, span: sp });
            if !self.eat(&TokenKind::Comma) {
                break;
            }
            if self.check(&TokenKind::RParen) {
                break; // trailing comma
            }
        }
        Ok(params)
    }

    /// `enum Name { Variant[(Type field, …)], … }` — assumes current token is `enum`.
    pub(super) fn parse_enum(&mut self, sp: Span) -> Result<EnumDecl, Diagnostic> {
        self.expect(&TokenKind::Enum, "'enum'")?;
        let name = self.expect_ident("an enum name")?;
        // Optional generic parameter list `<T, E>` immediately after the enum name (M-RT generic
        // enums) — `enum Result<T, E> { Ok(T value), Err(E error) }`.
        let type_params = self.parse_type_params()?;
        self.expect(&TokenKind::LBrace, "'{' to open enum body")?;
        let mut variants = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.check(&TokenKind::Eof) {
            let vsp = self.peek_span();
            let vname = self.expect_ident("a variant name")?;
            let fields = if self.eat(&TokenKind::LParen) {
                let f = self.parse_params()?;
                self.expect(&TokenKind::RParen, "')' to close variant fields")?;
                f
            } else {
                Vec::new()
            };
            variants.push(EnumVariant {
                name: vname,
                fields,
                span: vsp,
            });
            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }
        self.expect(&TokenKind::RBrace, "'}' to close enum")?;
        Ok(EnumDecl {
            vis: Visibility::Public,
            name,
            type_params,
            variants,
            span: sp,
        })
    }

    /// `[open] class Name<T> [extends A, B] [implements I1, I2] { member* }` — assumes current token
    /// is `class`. The `open` flag is parsed at the item level (`parse_item`) and threaded in.
    pub(super) fn parse_class(
        &mut self,
        sp: Span,
        open: bool,
        is_abstract: bool,
    ) -> Result<ClassDecl, Diagnostic> {
        self.expect(&TokenKind::Class, "'class'")?;
        let name = self.expect_ident("a class name")?;
        // Optional generic parameter list `<T, U>` immediately after the class name (M-RT
        // generics-all), before `extends`/`implements` — `class Box<T> extends … implements … { … }`.
        let type_params = self.parse_type_params()?;
        // Optional `extends A, B` parent-class list (M-RT S6) — before `implements`.
        let extends = if self.eat(&TokenKind::Extends) {
            self.parse_name_list("a class name after 'extends'")?
        } else {
            Vec::new()
        };
        let implements = if self.eat(&TokenKind::Implements) {
            self.parse_name_list("an interface name after 'implements'")?
        } else {
            Vec::new()
        };
        self.expect(&TokenKind::LBrace, "'{' to open class body")?;
        let mut members = Vec::new();
        let mut resolutions = Vec::new();
        let mut uses = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.check(&TokenKind::Eof) {
            // A leading contextual `use`/`rename`/`exclude` (lexed as identifiers, never reserved)
            // introduces a clause rather than a member. Types are PascalCase, so these lowercase
            // leaders are unambiguous in member position. M-RT S8 dot-lookahead: `use P.m` (a `.`
            // after the name) is an S6b resolution clause; `use T;` / `use A, B;` is trait composition.
            let leader = if let TokenKind::Ident(kw) = self.peek() {
                Some(kw.clone())
            } else {
                None
            };
            if let Some(kw) = leader {
                match kw.as_str() {
                    "use" => {
                        let is_resolution = matches!(
                            self.tokens.get(self.pos + 2).map(|t| &t.kind),
                            Some(&TokenKind::Dot)
                        );
                        if is_resolution {
                            resolutions.push(self.parse_resolution()?);
                        } else {
                            uses.extend(self.parse_use_traits()?);
                        }
                        continue;
                    }
                    "rename" | "exclude" => {
                        resolutions.push(self.parse_resolution()?);
                        continue;
                    }
                    _ => {}
                }
            }
            members.push(self.parse_class_member()?);
        }
        self.expect(&TokenKind::RBrace, "'}' to close class")?;
        Ok(ClassDecl {
            vis: Visibility::Public,
            name,
            type_params,
            extends,
            implements,
            open,
            is_abstract,
            resolutions,
            uses,
            members,
            span: sp,
        })
    }

    /// M-RT S8 trait composition: `use Name [, Name]* ;` → one or more [`crate::ast::UseTrait`].
    /// Assumes the current token is the contextual `use` keyword and the name is NOT dot-qualified
    /// (the caller disambiguated this from an S6b `use P.m` resolution clause via dot-lookahead).
    pub(super) fn parse_use_traits(&mut self) -> Result<Vec<crate::ast::UseTrait>, Diagnostic> {
        self.expect_ident("'use'")?; // consume the contextual `use`
        let mut out = Vec::new();
        loop {
            let sp = self.peek_span();
            let name = self.expect_ident("a trait name after 'use'")?;
            out.push(crate::ast::UseTrait { name, span: sp });
            if self.eat(&TokenKind::Comma) {
                continue;
            }
            break;
        }
        self.expect(&TokenKind::Semicolon, "';' after a trait `use` clause")?;
        Ok(out)
    }

    /// `trait Name { members }` (M-RT S8) — assumes the current token is `trait`. Members use the exact
    /// class-member grammar (methods, fields, const, static, hooks, constructor, abstract requirements).
    /// A trait has no `extends`/`implements`/generics this slice.
    pub(super) fn parse_trait(&mut self, sp: Span) -> Result<crate::ast::TraitDecl, Diagnostic> {
        self.expect(&TokenKind::Trait, "'trait'")?;
        let name = self.expect_ident("a trait name")?;
        self.expect(&TokenKind::LBrace, "'{' to open trait body")?;
        let mut members = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.check(&TokenKind::Eof) {
            members.push(self.parse_class_member()?);
        }
        self.expect(&TokenKind::RBrace, "'}' to close trait")?;
        Ok(crate::ast::TraitDecl {
            name,
            members,
            span: sp,
        })
    }

    /// A multi-inheritance resolution clause (M-RT S6b): `use P.m` | `rename P.m as n` | `exclude P.m`,
    /// with an optional trailing `;`. Assumes the current token is the contextual keyword.
    pub(super) fn parse_resolution(&mut self) -> Result<crate::ast::Resolution, Diagnostic> {
        let sp = self.peek_span();
        let kw = self.expect_ident("a resolution clause keyword")?;
        let parent = self.expect_ident("a parent class name")?;
        self.expect(&TokenKind::Dot, "'.' between the parent and method")?;
        let method = self.expect_ident("a method name")?;
        let res = match kw.as_str() {
            "use" => crate::ast::Resolution::Use {
                parent,
                method,
                span: sp,
            },
            "exclude" => crate::ast::Resolution::Exclude {
                parent,
                method,
                span: sp,
            },
            "rename" => {
                let as_kw = self.expect_ident("'as' in a rename clause")?;
                if as_kw != "as" {
                    return Err(self.error("'as' after 'rename P.m'"));
                }
                let as_name = self.expect_ident("the new method name after 'as'")?;
                crate::ast::Resolution::Rename {
                    parent,
                    method,
                    as_name,
                    span: sp,
                }
            }
            _ => unreachable!("caller gated the keyword"),
        };
        // Optional terminator.
        if self.check(&TokenKind::Semicolon) {
            self.advance();
        }
        Ok(res)
    }

    /// `interface Name [extends A, B] { (function sig;)* }` — assumes current token is `interface`.
    /// Each member is a method *signature*: `function name(params) [-> Ret];` with no body, stored as
    /// a `FunctionDecl` whose body is empty (M-RT S2).
    pub(super) fn parse_interface(
        &mut self,
        sp: Span,
    ) -> Result<crate::ast::InterfaceDecl, Diagnostic> {
        self.expect(&TokenKind::Interface, "'interface'")?;
        let name = self.expect_ident("an interface name")?;
        let extends = if self.eat(&TokenKind::Extends) {
            self.parse_name_list("an interface name after 'extends'")?
        } else {
            Vec::new()
        };
        self.expect(&TokenKind::LBrace, "'{' to open interface body")?;
        let mut methods = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.check(&TokenKind::Eof) {
            let msp = self.peek_span();
            self.expect(
                &TokenKind::Function,
                "'function' for an interface method signature",
            )?;
            let mname = self.expect_ident("a method name")?;
            self.expect(&TokenKind::LParen, "'(' after method name")?;
            let params = self.parse_params()?;
            self.expect(&TokenKind::RParen, "')' to close parameters")?;
            let ret = if self.eat(&TokenKind::Arrow) {
                Some(self.parse_type()?)
            } else {
                None
            };
            let throws = if self.eat(&TokenKind::Throws) {
                vec![self.parse_type()?]
            } else {
                Vec::new()
            };
            self.expect(
                &TokenKind::Semicolon,
                "';' after an interface method signature",
            )?;
            methods.push(FunctionDecl {
                modifiers: Vec::new(),
                vis: Visibility::Public,
                name: mname,
                type_params: Vec::new(),
                params,
                ret,
                throws,
                body: Vec::new(),
                span: msp,
            });
        }
        self.expect(&TokenKind::RBrace, "'}' to close interface")?;
        Ok(crate::ast::InterfaceDecl {
            vis: Visibility::Public,
            name,
            extends,
            methods,
            span: sp,
        })
    }

    /// A comma-separated list of one-or-more identifiers (no trailing comma), used for a class's
    /// `implements` list and an interface's `extends` list.
    pub(super) fn parse_name_list(&mut self, what: &str) -> Result<Vec<String>, Diagnostic> {
        let mut names = vec![self.expect_ident(what)?];
        while self.eat(&TokenKind::Comma) {
            names.push(self.expect_ident(what)?);
        }
        Ok(names)
    }

    /// One class member: a field, a constructor, or a method. Modifiers preceding
    /// `constructor` are consumed and dropped (M1: constructors are implicitly public).
    pub(super) fn parse_class_member(&mut self) -> Result<ClassMember, Diagnostic> {
        let sp = self.peek_span();
        let modifiers = self.parse_modifiers();
        match self.peek() {
            TokenKind::Constructor => {
                self.advance();
                self.expect(&TokenKind::LParen, "'(' after 'constructor'")?;
                let params = self.parse_ctor_params()?;
                self.expect(&TokenKind::RParen, "')' to close constructor parameters")?;
                let body = self.parse_block()?;
                Ok(ClassMember::Constructor {
                    params,
                    body,
                    span: sp,
                })
            }
            TokenKind::Function => Ok(ClassMember::Method(self.parse_function(modifiers, sp)?)),
            _ => {
                // field or property hook: [modifiers] Type name …
                let ty = self.parse_type()?;
                let name = self.expect_ident("a field name")?;
                // A `{` after the name opens a **property hook** body (M-mut.7b):
                // `Type name { get => expr; set(Type v) { stmts } }`. Anything else is a field. A
                // hook is virtual behavior, not storage, so it carries no modifiers (`mutable`/
                // `static` would describe a backing slot it doesn't have).
                if self.check(&TokenKind::LBrace) {
                    if !modifiers.is_empty() {
                        return Err(self.error("a property hook to carry no modifiers"));
                    }
                    return self.parse_property_hook(ty, name, sp);
                }
                // field: [modifiers] Type name [= init] ;
                // An optional field-level initializer (`static mutable int total = 0;`). The checker
                // requires it for `static` fields and forbids it on instance fields (M-mut.7).
                let init = if self.check(&TokenKind::Eq) {
                    self.advance();
                    Some(self.parse_expr()?)
                } else {
                    None
                };
                self.expect(&TokenKind::Semicolon, "';' after field declaration")?;
                Ok(ClassMember::Field {
                    modifiers,
                    ty,
                    name,
                    init,
                    span: sp,
                })
            }
        }
    }

    /// A property hook body (M-mut.7b): `{ get => expr; [set(Type v) { stmts }] }` — clauses in
    /// either order, each at most once, at least one required. Assumes the current token is `{`.
    pub(super) fn parse_property_hook(
        &mut self,
        ty: Type,
        name: String,
        sp: Span,
    ) -> Result<ClassMember, Diagnostic> {
        self.expect(&TokenKind::LBrace, "'{' to open a property hook body")?;
        let mut get: Option<Expr> = None;
        let mut set: Option<(Param, Vec<Stmt>)> = None;
        while !self.check(&TokenKind::RBrace) && !self.check(&TokenKind::Eof) {
            let clause = self.expect_ident("`get` or `set`")?;
            match clause.as_str() {
                "get" => {
                    if get.is_some() {
                        return Err(self.error("a single `get` clause"));
                    }
                    // `get => expr ;`
                    self.expect(&TokenKind::FatArrow, "'=>' after `get`")?;
                    let body = self.parse_expr()?;
                    self.expect(&TokenKind::Semicolon, "';' after the `get` expression")?;
                    get = Some(body);
                }
                "set" => {
                    if set.is_some() {
                        return Err(self.error("a single `set` clause"));
                    }
                    // `set(Type v) { stmts }`
                    self.expect(&TokenKind::LParen, "'(' after `set`")?;
                    let params = self.parse_params()?;
                    self.expect(&TokenKind::RParen, "')' to close the `set` parameter")?;
                    if params.len() != 1 {
                        return Err(self.error("exactly one `set` parameter `set(Type v)`"));
                    }
                    let body = self.parse_block()?;
                    set = Some((params.into_iter().next().unwrap(), body));
                }
                _ => return Err(self.error("`get` or `set` in a property hook")),
            }
        }
        self.expect(&TokenKind::RBrace, "'}' to close the property hook body")?;
        if get.is_none() && set.is_none() {
            return Err(self.error("at least a `get` or `set` clause in the property hook"));
        }
        Ok(ClassMember::Hook {
            ty,
            name,
            get,
            set,
            span: sp,
        })
    }

    /// Consume any run of visibility/binding modifiers.
    pub(super) fn parse_modifiers(&mut self) -> Vec<Modifier> {
        let mut mods = Vec::new();
        loop {
            let m = match self.peek() {
                TokenKind::Public => Modifier::Public,
                TokenKind::Private => Modifier::Private,
                TokenKind::Protected => Modifier::Protected,
                TokenKind::Const => Modifier::Const,
                // `open` method — opts into override (M-RT S6); final-by-default otherwise.
                TokenKind::Open => Modifier::Open,
                // `mutable` field / promoted ctor param (M-mut.6); immutable by default.
                TokenKind::Mutable => Modifier::Mutable,
                // `static` class field (M-mut.7) — class-level state.
                TokenKind::Static => Modifier::Static,
                // `abstract` method (M-RT S6b) — bodyless, implicitly `open`.
                TokenKind::Abstract => Modifier::Abstract,
                _ => break,
            };
            self.advance();
            mods.push(m);
        }
        mods
    }

    /// Constructor parameters: like normal params, but each may carry promotion modifiers
    /// (`constructor(private string name)`). Allows zero; allows a trailing comma.
    pub(super) fn parse_ctor_params(&mut self) -> Result<Vec<CtorParam>, Diagnostic> {
        let mut params = Vec::new();
        if self.check(&TokenKind::RParen) {
            return Ok(params);
        }
        loop {
            let sp = self.peek_span();
            let modifiers = self.parse_modifiers();
            let ty = self.parse_type()?;
            let name = self.expect_ident("a parameter name")?;
            params.push(CtorParam {
                modifiers,
                ty,
                name,
                span: sp,
            });
            if !self.eat(&TokenKind::Comma) {
                break;
            }
            if self.check(&TokenKind::RParen) {
                break; // trailing comma
            }
        }
        Ok(params)
    }
}
