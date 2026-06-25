use super::*;
use crate::chunk::{BytecodeProgram, Chunk, Function, Op};
use crate::value::Value;

/// Compile a loose program through the real front-end (loader → check → compile) for tests that
/// need a faithful `BytecodeProgram` rather than a hand-built one.
fn compile_source(src: &str) -> BytecodeProgram {
    let unit = crate::loader::load_loose_src(src).unwrap();
    let checked = crate::cli::check_and_expand(&unit.program, &unit.diag_src).unwrap();
    crate::compiler::compile(&checked).unwrap()
}

#[test]
fn vm_fault_carries_call_stack() {
    let program = compile_source(
        "package Main;\n\
             function f() -> int { var xs = [1]; return xs[5]; }\n\
             function main() -> void { var r = f(); }",
    );
    let err = Vm::new(&program).run().unwrap_err();
    assert_eq!(err.frames.len(), 2, "callee + main: {:?}", err.frames);
    assert_eq!(err.frames[0].function, "f");
    assert_eq!(err.frames[1].function, "main");
}

#[test]
fn run_and_runvm_traces_match() {
    // The slice-1 invariant: a fault yields byte-identical trace text on both backends.
    for src in [
        "package Main;\n\
             function g() -> int { var xs = [1]; return xs[9]; }\n\
             function main() -> void { var r = g(); }",
        "package Main;\nfunction main() -> void { var x = 1 / 0; }",
    ] {
        let unit = crate::loader::load_loose_src(src).unwrap();
        let checked = crate::cli::check_and_expand(&unit.program, &unit.diag_src).unwrap();
        let interp_err = crate::interpreter::interpret(&checked).unwrap_err();
        let program = crate::compiler::compile(&checked).unwrap();
        let vm_err = Vm::new(&program).run().unwrap_err();
        assert_eq!(
            interp_err.render(""),
            vm_err.render(""),
            "run vs runvm trace text diverged for:\n{src}"
        );
    }
}

/// Emit the standard function terminator: push `Unit`, then `Return` (P3-7).
fn term(c: &mut Chunk) {
    let u = c.add_const(Value::Unit);
    c.emit(Op::Const(u), 1);
    c.emit(Op::Return, 1);
}

/// Wrap a single hand-built chunk as `main` and run it. Renders the runtime `Diagnostic` to a
/// string so the existing `.contains(...)` assertions on fault bodies keep working.
fn run_chunk(chunk: Chunk) -> Result<String, String> {
    let program = BytecodeProgram {
        functions: vec![Function {
            name: "main".into(),
            arity: 0,
            n_captures: 0,
            chunk,
        }],
        main: 0,
        enum_descs: Vec::new(),
        class_descs: Vec::new(),
        names: Vec::new(),
        methods: HashMap::new(),
        class_implements: std::collections::BTreeMap::new(),
        class_tables: crate::native::ClassTables::default(),
        static_inits: Vec::new(),
        overloads: Vec::new(),
        method_overloads: std::collections::HashMap::new(),
    };
    Vm::new(&program).run().map_err(|d| d.to_string())
}

/// Build a chunk for `2 * 3 + 4` then print it.
fn arith_chunk() -> Chunk {
    let mut c = Chunk::new();
    let two = c.add_const(Value::Int(2));
    let three = c.add_const(Value::Int(3));
    let four = c.add_const(Value::Int(4));
    c.emit(Op::Const(two), 1);
    c.emit(Op::Const(three), 1);
    c.emit(Op::MulI, 1);
    c.emit(Op::Const(four), 1);
    c.emit(Op::AddI, 1);
    c.emit(Op::CallNative(crate::native::CONSOLE_PRINTLN, 1), 1);
    term(&mut c);
    c
}

#[test]
fn run_rejects_invalid_bytecode_before_executing() {
    // Out-of-range const: `validate()` (run's first action) must fault cleanly, not panic.
    let mut c = Chunk::new();
    c.emit(Op::Const(42), 1); // empty const pool
    c.emit(Op::Return, 1);
    let err = run_chunk(c).unwrap_err();
    assert!(err.contains("invalid bytecode"), "{err}");
    assert!(err.contains("const index 42"), "{err}");
}

// Debug-only: `debug_assert!` is a no-op in release, so this `should_panic` test only holds
// under `cfg(debug_assertions)`. A `GetLocal` past the (empty) main locals window passes
// `validate()` — slots aren't statically checkable — and trips `frame_slot`'s guard.
#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "vm local out of range")]
fn getlocal_past_window_trips_debug_assert() {
    let mut c = Chunk::new();
    c.emit(Op::GetLocal(5), 1);
    c.emit(Op::Return, 1);
    let _ = run_chunk(c);
}

#[test]
fn runs_integer_arithmetic_and_prints() {
    let out = run_chunk(arith_chunk()).unwrap();
    assert_eq!(out, "10\n");
}

#[test]
fn float_print_matches_interpreter_formatting() {
    // 1.5 + 2.5 = 4.0 -> rendered "4" via Rust `{}` (parity with value::as_display).
    let mut c = Chunk::new();
    let a = c.add_const(Value::Float(1.5));
    let b = c.add_const(Value::Float(2.5));
    c.emit(Op::Const(a), 1);
    c.emit(Op::Const(b), 1);
    c.emit(Op::AddF, 1);
    c.emit(Op::CallNative(crate::native::CONSOLE_PRINTLN, 1), 1);
    term(&mut c);
    assert_eq!(run_chunk(c).unwrap(), "4\n");
}

#[test]
fn division_by_zero_is_runtime_error() {
    let mut c = Chunk::new();
    let a = c.add_const(Value::Int(1));
    let z = c.add_const(Value::Int(0));
    c.emit(Op::Const(a), 1);
    c.emit(Op::Const(z), 1);
    c.emit(Op::DivI, 1);
    term(&mut c);
    let err = run_chunk(c).unwrap_err();
    assert!(err.contains("division by zero"), "{err}");
}

#[test]
fn negate_works_for_int_and_float() {
    let mut c = Chunk::new();
    let a = c.add_const(Value::Int(5));
    c.emit(Op::Const(a), 1);
    c.emit(Op::Neg, 1);
    c.emit(Op::CallNative(crate::native::CONSOLE_PRINTLN, 1), 1);
    term(&mut c);
    assert_eq!(run_chunk(c).unwrap(), "-5\n");
}

#[test]
fn comparison_and_equality() {
    // 3 < 5  -> true
    let mut c = Chunk::new();
    let a = c.add_const(Value::Int(3));
    let b = c.add_const(Value::Int(5));
    c.emit(Op::Const(a), 1);
    c.emit(Op::Const(b), 1);
    c.emit(Op::Lt, 1);
    c.emit(Op::CallNative(crate::native::CONSOLE_PRINTLN, 1), 1);
    term(&mut c);
    assert_eq!(run_chunk(c).unwrap(), "true\n");
}

#[test]
fn locals_get_and_set() {
    // local0 = 10; local0 = local0 + 5; print local0  -> 15
    let mut c = Chunk::new();
    let ten = c.add_const(Value::Int(10));
    let five = c.add_const(Value::Int(5));
    c.emit(Op::Const(ten), 1); // slot 0 (stays on stack)
    c.emit(Op::GetLocal(0), 1);
    c.emit(Op::Const(five), 1);
    c.emit(Op::AddI, 1);
    c.emit(Op::SetLocal(0), 1);
    c.emit(Op::GetLocal(0), 1);
    c.emit(Op::CallNative(crate::native::CONSOLE_PRINTLN, 1), 1);
    term(&mut c);
    assert_eq!(run_chunk(c).unwrap(), "15\n");
}

#[test]
fn jump_if_false_skips_branch() {
    // if (false) print 1 else print 2  -> "2"
    let mut c = Chunk::new();
    let f = c.add_const(Value::Bool(false));
    let one = c.add_const(Value::Int(1));
    let two = c.add_const(Value::Int(2));
    c.emit(Op::Const(f), 1); // 0
    let jif = c.code.len();
    c.emit(Op::JumpIfFalse(0), 1); // 1 (patched below)
    c.emit(Op::Const(one), 1); // 2
    c.emit(Op::CallNative(crate::native::CONSOLE_PRINTLN, 1), 1); // 3
    let jend = c.code.len();
    c.emit(Op::Jump(0), 1); // 4 (patched below)
    let else_target = c.code.len(); // 5
    c.emit(Op::Const(two), 1); // 5
    c.emit(Op::CallNative(crate::native::CONSOLE_PRINTLN, 1), 1); // 6
    let end = c.code.len(); // 7 (start of the terminator)
    term(&mut c); // 7..9
    c.code[jif] = Op::JumpIfFalse(else_target);
    c.code[jend] = Op::Jump(end);
    assert_eq!(run_chunk(c).unwrap(), "2\n");
}

#[test]
fn concat_renders_mixed_scalars() {
    // "x=" + 7  -> "x=7"
    let mut c = Chunk::new();
    let pre = c.add_const(Value::Str("x=".into()));
    let seven = c.add_const(Value::Int(7));
    c.emit(Op::Const(pre), 1);
    c.emit(Op::Const(seven), 1);
    c.emit(Op::Concat(2), 1);
    c.emit(Op::CallNative(crate::native::CONSOLE_PRINTLN, 1), 1);
    term(&mut c);
    assert_eq!(run_chunk(c).unwrap(), "x=7\n");
}

#[test]
fn list_make_index_len() {
    // xs = [10, 20, 30]; print len(xs); print xs[1]  -> "3" then "20"
    let mut c = Chunk::new();
    let a = c.add_const(Value::Int(10));
    let b = c.add_const(Value::Int(20));
    let d = c.add_const(Value::Int(30));
    let one = c.add_const(Value::Int(1));
    c.emit(Op::Const(a), 1);
    c.emit(Op::Const(b), 1);
    c.emit(Op::Const(d), 1);
    c.emit(Op::MakeList(3), 1); // slot 0 = list
    c.emit(Op::GetLocal(0), 1);
    c.emit(Op::Len, 1);
    c.emit(Op::CallNative(crate::native::CONSOLE_PRINTLN, 1), 1);
    c.emit(Op::GetLocal(0), 1);
    c.emit(Op::Const(one), 1);
    c.emit(Op::Index, 1);
    c.emit(Op::CallNative(crate::native::CONSOLE_PRINTLN, 1), 1);
    term(&mut c);
    assert_eq!(run_chunk(c).unwrap(), "3\n20\n");
}

#[test]
fn print_joins_multiple_args_with_space() {
    let mut c = Chunk::new();
    let a = c.add_const(Value::Str("a".into()));
    let b = c.add_const(Value::Int(1));
    c.emit(Op::Const(a), 1);
    c.emit(Op::Const(b), 1);
    c.emit(Op::CallNative(crate::native::CONSOLE_PRINTLN, 2), 1);
    term(&mut c);
    assert_eq!(run_chunk(c).unwrap(), "a 1\n");
}

#[test]
fn call_runs_a_second_function_and_returns() {
    // main: push 7, Call(1), Print(1), term.   f(x): GetLocal(0), Return.
    let mut m = Chunk::new();
    let seven = m.add_const(Value::Int(7));
    m.emit(Op::Const(seven), 1);
    m.emit(Op::Call(1), 1);
    m.emit(Op::CallNative(crate::native::CONSOLE_PRINTLN, 1), 1);
    term(&mut m);

    let mut f = Chunk::new();
    f.emit(Op::GetLocal(0), 1); // the single arg
    f.emit(Op::Return, 1);

    let program = BytecodeProgram {
        functions: vec![
            Function {
                name: "main".into(),
                arity: 0,
                n_captures: 0,
                chunk: m,
            },
            Function {
                name: "f".into(),
                arity: 1,
                n_captures: 0,
                chunk: f,
            },
        ],
        main: 0,
        enum_descs: Vec::new(),
        class_descs: Vec::new(),
        names: Vec::new(),
        methods: HashMap::new(),
        class_implements: std::collections::BTreeMap::new(),
        class_tables: crate::native::ClassTables::default(),
        static_inits: Vec::new(),
        overloads: Vec::new(),
        method_overloads: std::collections::HashMap::new(),
    };
    assert_eq!(Vm::new(&program).run().unwrap(), "7\n");
}

#[test]
fn make_enum_then_match_tag_and_get_field() {
    use crate::chunk::EnumDesc;
    // descs[0] = Opt::Some(int) (arity 1). Build:
    //   const 7; MakeEnum(0)          -> Some(7) becomes slot 0 (stays)
    //   GetLocal(0); MatchTag(0)      -> true        ; print
    //   GetLocal(0); GetEnumField(0)  -> 7           ; print
    let mut c = Chunk::new();
    let seven = c.add_const(Value::Int(7));
    c.emit(Op::Const(seven), 1);
    c.emit(Op::MakeEnum(0), 1);
    c.emit(Op::GetLocal(0), 1);
    c.emit(Op::MatchTag(0), 1);
    c.emit(Op::CallNative(crate::native::CONSOLE_PRINTLN, 1), 1);
    c.emit(Op::GetLocal(0), 1);
    c.emit(Op::GetEnumField(0), 1);
    c.emit(Op::CallNative(crate::native::CONSOLE_PRINTLN, 1), 1);
    term(&mut c);
    let program = BytecodeProgram {
        functions: vec![Function {
            name: "main".into(),
            arity: 0,
            n_captures: 0,
            chunk: c,
        }],
        main: 0,
        enum_descs: vec![EnumDesc {
            ty: "Opt".into(),
            variant: "Some".into(),
            arity: 1,
        }],
        class_descs: Vec::new(),
        names: Vec::new(),
        methods: HashMap::new(),
        class_implements: std::collections::BTreeMap::new(),
        class_tables: crate::native::ClassTables::default(),
        static_inits: Vec::new(),
        overloads: Vec::new(),
        method_overloads: std::collections::HashMap::new(),
    };
    assert_eq!(Vm::new(&program).run().unwrap(), "true\n7\n");
}

#[test]
fn match_tag_is_false_for_a_different_variant() {
    use crate::chunk::EnumDesc;
    // Build a `None` (desc 0), then test it against the `Some` tag (desc 1) -> false.
    let mut c = Chunk::new();
    c.emit(Op::MakeEnum(0), 1); // None (arity 0) -> slot 0
    c.emit(Op::GetLocal(0), 1);
    c.emit(Op::MatchTag(1), 1); // is it `Some`? -> false
    c.emit(Op::CallNative(crate::native::CONSOLE_PRINTLN, 1), 1);
    term(&mut c);
    let program = BytecodeProgram {
        functions: vec![Function {
            name: "main".into(),
            arity: 0,
            n_captures: 0,
            chunk: c,
        }],
        main: 0,
        enum_descs: vec![
            EnumDesc {
                ty: "Opt".into(),
                variant: "None".into(),
                arity: 0,
            },
            EnumDesc {
                ty: "Opt".into(),
                variant: "Some".into(),
                arity: 1,
            },
        ],
        class_descs: Vec::new(),
        names: Vec::new(),
        methods: HashMap::new(),
        class_implements: std::collections::BTreeMap::new(),
        class_tables: crate::native::ClassTables::default(),
        static_inits: Vec::new(),
        overloads: Vec::new(),
        method_overloads: std::collections::HashMap::new(),
    };
    assert_eq!(Vm::new(&program).run().unwrap(), "false\n");
}

#[test]
fn make_instance_then_get_field() {
    use crate::chunk::ClassDesc;
    // class Point { x, y }: build Point(3, 4) into slot 0, then read `.x` (names[0]) -> 3.
    let mut c = Chunk::new();
    let three = c.add_const(Value::Int(3));
    let four = c.add_const(Value::Int(4));
    c.emit(Op::Const(three), 1);
    c.emit(Op::Const(four), 1);
    c.emit(Op::MakeInstance(0), 1); // [Point{x:3,y:4}] becomes slot 0
    c.emit(Op::GetLocal(0), 1);
    c.emit(Op::GetField(0), 1); // names[0] == "x"
    c.emit(Op::CallNative(crate::native::CONSOLE_PRINTLN, 1), 1);
    term(&mut c);
    let program = BytecodeProgram {
        functions: vec![Function {
            name: "main".into(),
            arity: 0,
            n_captures: 0,
            chunk: c,
        }],
        main: 0,
        enum_descs: Vec::new(),
        class_descs: vec![ClassDesc {
            class: "Point".into(),
            fields: vec!["x".into(), "y".into()],
        }],
        names: vec!["x".into()],
        methods: HashMap::new(),
        class_implements: std::collections::BTreeMap::new(),
        class_tables: crate::native::ClassTables::default(),
        static_inits: Vec::new(),
        overloads: Vec::new(),
        method_overloads: std::collections::HashMap::new(),
    };
    assert_eq!(Vm::new(&program).run().unwrap(), "3\n");
}

#[test]
fn get_field_absent_faults_like_interpreter() {
    use crate::chunk::ClassDesc;
    // Empty instance, read missing field `tag` (names[0]) -> `no field` fault (parity).
    let mut c = Chunk::new();
    c.emit(Op::MakeInstance(0), 1); // [Empty{}] slot 0
    c.emit(Op::GetLocal(0), 1);
    c.emit(Op::GetField(0), 1);
    c.emit(Op::CallNative(crate::native::CONSOLE_PRINTLN, 1), 1);
    term(&mut c);
    let program = BytecodeProgram {
        functions: vec![Function {
            name: "main".into(),
            arity: 0,
            n_captures: 0,
            chunk: c,
        }],
        main: 0,
        enum_descs: Vec::new(),
        class_descs: vec![ClassDesc {
            class: "Empty".into(),
            fields: Vec::new(),
        }],
        names: vec!["tag".into()],
        methods: HashMap::new(),
        class_implements: std::collections::BTreeMap::new(),
        class_tables: crate::native::ClassTables::default(),
        static_inits: Vec::new(),
        overloads: Vec::new(),
        method_overloads: std::collections::HashMap::new(),
    };
    let err = Vm::new(&program).run().unwrap_err().to_string();
    assert!(err.contains("no field `tag` on `Empty`"), "{err}");
}
