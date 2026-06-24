//! Parser tests — items (M-Decomp W3.1b, mirrors the source clusters).

use super::support::*;

#[test]
fn parses_private_class_visibility() {
    match &prog("package Main;\nprivate class P {}").items[0] {
        Item::Class(c) => assert_eq!(c.vis, Visibility::Private),
        other => panic!("expected class, got {other:?}"),
    }
}

#[test]
fn parses_internal_function_visibility() {
    match &prog("package Main;\ninternal function f() -> void {}").items[0] {
        Item::Function(f) => assert_eq!(f.vis, Visibility::Internal),
        other => panic!("expected function, got {other:?}"),
    }
}

#[test]
fn parses_internal_enum_and_interface_visibility() {
    match &prog("package Main;\ninternal enum E { A() }").items[0] {
        Item::Enum(e) => assert_eq!(e.vis, Visibility::Internal),
        other => panic!("expected enum, got {other:?}"),
    }
    match &prog("package Main;\nprivate interface I { function m() -> int; }").items[0] {
        Item::Interface(i) => assert_eq!(i.vis, Visibility::Private),
        other => panic!("expected interface, got {other:?}"),
    }
}

#[test]
fn bare_decl_defaults_to_public() {
    match &prog("package Main;\nclass C {}").items[0] {
        Item::Class(c) => assert_eq!(c.vis, Visibility::Public),
        other => panic!("expected class, got {other:?}"),
    }
}

#[test]
fn s8_use_dot_lookahead_splits_trait_from_resolution() {
    // M-RT S8 D9: `use T;` (no dot) is trait composition; `use A.foo` (dot) is an S6b resolution
    // clause. Both can appear in the same class body and must land in the right buckets.
    match &prog(
        "package Main;\nopen class A { open function foo() -> int { return 1; } }\n\
             trait T { function bar() -> int { return 2; } }\n\
             class C extends A { use T; use A.foo }",
    )
    .items
    .last()
    .unwrap()
    {
        Item::Class(c) => {
            assert_eq!(c.uses.len(), 1, "one trait `use`");
            assert_eq!(c.uses[0].name, "T");
            assert_eq!(c.resolutions.len(), 1, "one resolution clause");
        }
        other => panic!("expected class, got {other:?}"),
    }
}

#[test]
fn explicit_public_enum_parses() {
    match &prog("package Main;\npublic enum E { A() }").items[0] {
        Item::Enum(e) => assert_eq!(e.vis, Visibility::Public),
        other => panic!("expected enum, got {other:?}"),
    }
}

#[test]
fn conflicting_visibility_prefix_is_rejected() {
    let err = prog_err("package Main;\npublic private class C {}");
    assert!(err.contains("a single visibility"), "got: {err}");
}

#[test]
fn visibility_on_import_is_rejected() {
    let err = prog_err("package Main;\nprivate import Core.Console;");
    assert!(err.contains("cannot carry a visibility"), "got: {err}");
}

#[test]
fn parses_generic_function_type_params() {
    // `function id<T>(T x) -> T { … }` records the type parameter list (M-RT S7).
    match item("function id<T, U>(T a, U b) -> T { return a; }") {
        Item::Function(f) => assert_eq!(f.type_params, vec!["T".to_string(), "U".to_string()]),
        other => panic!("expected a generic function, got {other:?}"),
    }
    // A non-generic function has an empty type-param list.
    match item("function plain(int x) -> int { return x; }") {
        Item::Function(f) => assert!(f.type_params.is_empty()),
        other => panic!("expected a function, got {other:?}"),
    }
}

#[test]
fn parses_generic_methods() {
    // M-RT generics-all: a method may declare `<T>` just like a free function.
    let item = parser("class C { function m<T>(T x) -> T { return x; } }")
        .parse_item()
        .expect("generic method should parse");
    match item {
        Item::Class(c) => match &c.members[0] {
            crate::ast::ClassMember::Method(f) => {
                assert_eq!(f.type_params, vec!["T".to_string()]);
            }
            _ => panic!("expected a method"),
        },
        _ => panic!("expected a class"),
    }
}

#[test]
fn parses_fn_throws_clause() {
    // Single declared exception type.
    match &prog("package Main;\nfunction f() -> int throws ParseError { return 1; }").items[0] {
        Item::Function(f) => {
            assert_eq!(f.throws.len(), 1);
            assert!(matches!(&f.throws[0], Type::Named { name, .. } if name == "ParseError"));
        }
        other => panic!("expected function, got {other:?}"),
    }
    // `throws A | B` captures the whole union as one `Type::Union`.
    match &prog("package Main;\nfunction g() -> void throws A | B { return; }").items[0] {
        Item::Function(f) => {
            assert_eq!(f.throws.len(), 1);
            assert!(matches!(&f.throws[0], Type::Union(members, _) if members.len() == 2));
        }
        other => panic!("expected function, got {other:?}"),
    }
    // No throws clause ⇒ empty.
    match &prog("package Main;\nfunction h() -> void {}").items[0] {
        Item::Function(f) => assert!(f.throws.is_empty()),
        other => panic!("expected function, got {other:?}"),
    }
}

#[test]
fn parses_function_decl() {
    match item("function area(Shape s) -> float { return s; }") {
        Item::Function(f) => {
            assert_eq!(f.name, "area");
            assert_eq!(f.params.len(), 1);
            assert_eq!(f.params[0].name, "s");
            assert!(f.ret.is_some());
            assert_eq!(f.body.len(), 1);
            assert!(f.modifiers.is_empty());
        }
        other => panic!("got {other:?}"),
    }
}

#[test]
fn parses_function_no_ret_no_params() {
    // The PARSER stays permissive: a function with no `-> T` parses with `ret == None`. The
    // return-type *mandate* (S0b, `E-MISSING-RETURN-TYPE`) is a CHECKER rule, not a parser one.
    match item("function main() { Console.println(1); }") {
        Item::Function(f) => {
            assert_eq!(f.name, "main");
            assert!(f.params.is_empty());
            assert!(f.ret.is_none());
            assert_eq!(f.body.len(), 1);
        }
        other => panic!("got {other:?}"),
    }
}

#[test]
fn parses_enum_decl() {
    let src = "enum Shape { Circle(float radius), Rect(float w, float h), Unit, }";
    match item(src) {
        Item::Enum(e) => {
            assert_eq!(e.name, "Shape");
            assert_eq!(e.variants.len(), 3);
            assert_eq!(e.variants[0].name, "Circle");
            assert_eq!(e.variants[0].fields.len(), 1);
            assert_eq!(e.variants[1].fields.len(), 2);
            assert!(e.variants[2].fields.is_empty()); // bare variant
        }
        other => panic!("got {other:?}"),
    }
}

#[test]
fn parses_class_decl() {
    let src = "class Greeter { \
                     private string name; \
                     constructor(private string name) {} \
                     function greet() -> string { return name; } \
                   }";
    match item(src) {
        Item::Class(c) => {
            assert_eq!(c.name, "Greeter");
            assert_eq!(c.members.len(), 3);
            match &c.members[0] {
                ClassMember::Field {
                    modifiers, name, ..
                } => {
                    assert_eq!(name, "name");
                    assert_eq!(modifiers, &vec![Modifier::Private]);
                }
                other => panic!("member 0: {other:?}"),
            }
            match &c.members[1] {
                ClassMember::Constructor { params, .. } => {
                    assert_eq!(params.len(), 1);
                    assert_eq!(params[0].modifiers, vec![Modifier::Private]);
                    assert_eq!(params[0].name, "name");
                }
                other => panic!("member 1: {other:?}"),
            }
            match &c.members[2] {
                ClassMember::Method(f) => assert_eq!(f.name, "greet"),
                other => panic!("member 2: {other:?}"),
            }
        }
        other => panic!("got {other:?}"),
    }
}

#[test]
fn parses_mutable_field_and_ctor_param_modifier() {
    // M-mut.6: `mutable` is accepted in field + promoted-ctor-param modifier position.
    let src = "class C { \
                     mutable int count; \
                     constructor(public mutable int total) {} \
                   }";
    match item(src) {
        Item::Class(c) => {
            match &c.members[0] {
                ClassMember::Field {
                    modifiers, name, ..
                } => {
                    assert_eq!(name, "count");
                    assert_eq!(modifiers, &vec![Modifier::Mutable]);
                }
                other => panic!("member 0: {other:?}"),
            }
            match &c.members[1] {
                ClassMember::Constructor { params, .. } => {
                    assert_eq!(
                        params[0].modifiers,
                        vec![Modifier::Public, Modifier::Mutable]
                    );
                }
                other => panic!("member 1: {other:?}"),
            }
        }
        other => panic!("got {other:?}"),
    }
}

#[test]
fn open_method_modifier_and_final_retired() {
    // S6a.1: `open` parses as a method modifier. (Methods use block bodies, not `=> expr`.)
    match item("class C { open function f() -> int { return 1; } }") {
        Item::Class(c) => match &c.members[0] {
            ClassMember::Method(m) => {
                assert_eq!(m.name, "f");
                assert_eq!(m.modifiers, vec![Modifier::Open]);
            }
            other => panic!("member 0: {other:?}"),
        },
        other => panic!("got {other:?}"),
    }
    // S6a.1: `final` is no longer a keyword — it now lexes as an ordinary identifier.
    let toks = lex("final").expect("lex ok");
    assert!(
        matches!(&toks[0].kind, TokenKind::Ident(s) if s == "final"),
        "expected `final` to lex as Ident, got {:?}",
        toks[0].kind
    );
}

#[test]
fn parses_open_class_with_single_extends() {
    // S6a.2: `open` class prefix + a single `extends` parent.
    let p = prog("package Main;\nopen class Animal {}\nclass Dog extends Animal {}");
    let animal = match &p.items[0] {
        Item::Class(c) => c,
        o => panic!("item 0: {o:?}"),
    };
    assert!(animal.open, "Animal should be open");
    assert!(animal.extends.is_empty(), "Animal extends nothing");
    let dog = match &p.items[1] {
        Item::Class(c) => c,
        o => panic!("item 1: {o:?}"),
    };
    assert!(!dog.open, "Dog is final-by-default (not open)");
    assert_eq!(dog.extends, vec!["Animal".to_string()]);
}

#[test]
fn open_prefix_on_a_non_class_is_an_error() {
    // S6a.2: `open` only applies to classes.
    let msg = prog_err("package Main;\nopen function f() -> void {}");
    assert!(msg.contains("only a class"), "got: {msg}");
}

#[test]
fn parses_static_field_with_initializer() {
    // M-mut.7: `static mutable int total = 0;` — static modifier + field-level initializer.
    let src = "class C { static mutable int total = 0; }";
    match item(src) {
        Item::Class(c) => match &c.members[0] {
            ClassMember::Field {
                modifiers,
                name,
                init,
                ..
            } => {
                assert_eq!(name, "total");
                assert_eq!(modifiers, &vec![Modifier::Static, Modifier::Mutable]);
                assert!(matches!(init, Some(Expr::Int(0, _))));
            }
            other => panic!("member 0: {other:?}"),
        },
        other => panic!("got {other:?}"),
    }
}

#[test]
fn parses_property_hook_get_and_set() {
    // M-mut.7b: `float fahrenheit { get => …; set(float v) { … } }` — a property hook with
    // both a computed-read body and an intercepted-write body.
    let src = "class Temp { \
                     mutable float celsius; \
                     float fahrenheit { \
                       get => this.celsius * 2.0; \
                       set(float v) { this.celsius = v; } \
                     } \
                   }";
    match item(src) {
        Item::Class(c) => match &c.members[1] {
            ClassMember::Hook {
                name, get, set, ty, ..
            } => {
                assert_eq!(name, "fahrenheit");
                assert!(matches!(ty, Type::Named { name, .. } if name == "float"));
                assert!(get.is_some(), "expected a get body");
                let (p, stmts) = set.as_ref().expect("expected a set body");
                assert_eq!(p.name, "v");
                assert_eq!(stmts.len(), 1);
            }
            other => panic!("member 1: {other:?}"),
        },
        other => panic!("got {other:?}"),
    }
}

#[test]
fn parses_read_only_property_hook() {
    // A get-only hook (no `set`) is a read-only computed property.
    match item("class C { int doubled { get => 2; } }") {
        Item::Class(c) => match &c.members[0] {
            ClassMember::Hook { get, set, .. } => {
                assert!(get.is_some());
                assert!(set.is_none());
            }
            other => panic!("member 0: {other:?}"),
        },
        other => panic!("got {other:?}"),
    }
}

#[test]
fn parses_class_implements_list() {
    // M-RT S2: `implements A, B` is parsed into ClassDecl.implements.
    match item("class Dog implements Speaker, Pet { function speak() -> string { return \"w\"; } }")
    {
        Item::Class(c) => {
            assert_eq!(c.name, "Dog");
            assert_eq!(c.implements, vec!["Speaker".to_string(), "Pet".to_string()]);
            assert_eq!(c.members.len(), 1);
        }
        other => panic!("got {other:?}"),
    }
    // No `implements` ⇒ empty list.
    match item("class Plain {}") {
        Item::Class(c) => assert!(c.implements.is_empty()),
        other => panic!("got {other:?}"),
    }
}

#[test]
fn parses_interface_decl() {
    // M-RT S2: an interface is method signatures (no bodies) + an optional `extends` list.
    match item("interface Pet extends Speaker, Named { function speak() -> string; function age() -> int; }") {
            Item::Interface(i) => {
                assert_eq!(i.name, "Pet");
                assert_eq!(i.extends, vec!["Speaker".to_string(), "Named".to_string()]);
                assert_eq!(i.methods.len(), 2);
                assert_eq!(i.methods[0].name, "speak");
                assert!(i.methods[0].body.is_empty(), "signature has no body");
                assert_eq!(i.methods[1].name, "age");
            }
            other => panic!("got {other:?}"),
        }
}

#[test]
fn parses_import() {
    match item("import Core.Console;") {
        Item::Import { path, .. } => assert_eq!(path, vec!["Core", "Console"]),
        other => panic!("got {other:?}"),
    }
    match item("import a;") {
        Item::Import { path, .. } => assert_eq!(path, vec!["a"]),
        other => panic!("got {other:?}"),
    }
}

#[test]
fn parses_package_declaration() {
    // `package a.b;` is captured on the Program, not as an Item (M5 S1).
    let prog = parser("package app.util; function main() -> void {}")
        .parse_program()
        .expect("parse ok");
    assert_eq!(prog.package, vec!["app".to_string(), "util".to_string()]);
    // A bare file parses with an empty package — the checker, not the parser, enforces presence.
    let bare = parser("function main() -> void {}")
        .parse_program()
        .expect("parse ok");
    assert!(bare.package.is_empty());
    // `package` after another item is a parse error (it must be the first declaration).
    assert!(parser("function main() -> void {} package app;")
        .parse_program()
        .is_err());
}

#[test]
fn parses_program_multiple_items() {
    let src = "import Core.Console; enum E { A, } function main() -> void { return; }";
    let prog = parser(src).parse_program().expect("parse ok");
    assert_eq!(prog.items.len(), 3);
    assert!(matches!(prog.items[0], Item::Import { .. }));
    assert!(matches!(prog.items[1], Item::Enum(_)));
    assert!(matches!(prog.items[2], Item::Function(_)));
}

#[test]
fn empty_program_parses() {
    let prog = parser("").parse_program().expect("parse ok");
    assert!(prog.items.is_empty());
}
