use phorge::ast::{ClassMember, Item};
use phorge::lexer::lex;
use phorge::parser::Parser;

/// The complete sample program from the design spec (§6), verbatim.
const SAMPLE: &str = r#"
import Core.Console;

enum Shape {
    Circle(float radius),
    Rect(float w, float h),
}

function area(Shape s) -> float {
    return match s {
        Circle(r)  => 3.14159 * r * r,
        Rect(w, h) => w * h,
    };
}

class Greeter {
    private string name;

    constructor(private string name) {}

    function greet() -> string {
        return "Hello {name}";
    }
}

function main() {
    Greeter g = Greeter("Tak");
    Console.println(g.greet());

    List<Shape> shapes = [Circle(2.0), Rect(3.0, 4.0)];
    for (Shape s in shapes) {
        Console.println("area = {area(s)}");
    }
}
"#;

#[test]
fn parses_full_sample_program() {
    let tokens = lex(SAMPLE).expect("lex ok");
    let prog = Parser::new(tokens).parse_program().expect("parse ok");

    // import, enum, function area, class Greeter, function main
    assert_eq!(prog.items.len(), 5);

    assert!(matches!(prog.items[0], Item::Import { .. }));

    match &prog.items[1] {
        Item::Enum(e) => {
            assert_eq!(e.name, "Shape");
            assert_eq!(e.variants.len(), 2);
        }
        other => panic!("item 1: {other:?}"),
    }

    match &prog.items[2] {
        Item::Function(f) => {
            assert_eq!(f.name, "area");
            assert_eq!(f.params.len(), 1);
            assert!(f.ret.is_some());
            assert_eq!(f.body.len(), 1); // a single `return match …;`
        }
        other => panic!("item 2: {other:?}"),
    }

    match &prog.items[3] {
        Item::Class(c) => {
            assert_eq!(c.name, "Greeter");
            assert_eq!(c.members.len(), 3);
            assert!(matches!(c.members[0], ClassMember::Field { .. }));
            assert!(matches!(c.members[1], ClassMember::Constructor { .. }));
            assert!(matches!(c.members[2], ClassMember::Method(_)));
        }
        other => panic!("item 3: {other:?}"),
    }

    match &prog.items[4] {
        Item::Function(f) => {
            assert_eq!(f.name, "main");
            assert!(f.ret.is_none());
            // Greeter g = …;  console.println(…);  List<Shape> shapes = …;  for (…) {…}
            assert_eq!(f.body.len(), 4);
        }
        other => panic!("item 4: {other:?}"),
    }
}
