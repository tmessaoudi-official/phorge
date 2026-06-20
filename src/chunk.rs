//! Bytecode chunk + instruction set for the M2 VM.
//! See docs/specs/2026-06-15-m2-bytecode-vm-design.md (§4, §5).
//! P2 scope: full M1 expression/statement surface for `main` (see
//! docs/plans/2026-06-15-m2-plan2-compiler-runvm.md). P4a adds single-payload enums + `match`;
//! P4b adds classes (construction + constructor promotion + field reads); P4c adds methods + `this`.
//! Reuses `value::Value` directly for scalars, lists, *and* enums/instances — the VM mirrors the
//! interpreter's value-semantics object model (clone-on-use, no heap; decision P4-1). An arena is
//! a deferred, bench-gated perf milestone, not a correctness requirement.

use crate::value::Value;
use std::collections::{BTreeMap, HashMap};

/// Hashable identity of an internable constant. `Value` can't derive `Hash`/`Eq` (it holds `f64`
/// and composite types), so the constant pool dedups via this projection: floats by their bit
/// pattern (`to_bits`), strings by content, the rest by value. Composite constants (`List`,
/// instances, enums) are never interned — they have no key and always get a fresh slot.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum ConstKey {
    Int(i64),
    /// `f64::to_bits` — so `+0.0`/`-0.0` and distinct `NaN`s key apart, and equal floats dedup.
    Float(u64),
    Bool(bool),
    Str(String),
    Unit,
}

impl ConstKey {
    /// The dedup key for a scalar constant, or `None` for a composite (never interned).
    fn of(v: &Value) -> Option<ConstKey> {
        Some(match v {
            Value::Int(n) => ConstKey::Int(*n),
            Value::Float(x) => ConstKey::Float(x.to_bits()),
            Value::Bool(b) => ConstKey::Bool(*b),
            Value::Str(s) => ConstKey::Str(s.clone()),
            Value::Unit => ConstKey::Unit,
            _ => return None,
        })
    }
}

/// Which fixed runtime fault an [`Op::Fault`] raises. The message lives here (single-sourced) so the
/// VM and the tree-walking interpreter stay byte-identical — the `agree_err` oracle classifies
/// faults by body substring (M3 S2.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaultMsg {
    /// Non-exhaustive `match` fall-through (checker-unreachable backstop).
    NonExhaustiveMatch,
    /// `opt!` force-unwrap of a `null` value.
    ForceUnwrapNull,
}

impl FaultMsg {
    /// The fault body. Must match the interpreter's `rt(..)` text for each fault so both backends
    /// classify to the same `FaultKind`.
    pub fn message(self) -> &'static str {
        match self {
            FaultMsg::NonExhaustiveMatch => "non-exhaustive match at runtime",
            FaultMsg::ForceUnwrapNull => "force-unwrap of null",
        }
    }
}

/// One VM instruction. Typed operands — no raw-byte decode (decision M2-7).
/// Jump targets are absolute instruction indices (decision P2-2).
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    /// Push `consts[idx]`.
    Const(usize),
    // Type-specialized arithmetic (the checker guarantees operand types).
    AddI,
    SubI,
    MulI,
    DivI,
    RemI,
    AddF,
    SubF,
    MulF,
    DivF,
    RemF,
    /// Negate the top of stack (int or float).
    Neg,
    /// Logical not (bool).
    Not,
    // Comparison / equality — runtime-generic (decision P2-8).
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
    /// Discard the top of stack.
    Pop,
    /// Push a copy of the local at stack slot `n`.
    GetLocal(usize),
    /// Pop and store into the local at stack slot `n` (set-and-pop, decision P2-4).
    SetLocal(usize),
    /// Unconditional jump to absolute instruction index.
    Jump(usize),
    /// Pop a bool; if false, jump to absolute instruction index (decision P2-5).
    JumpIfFalse(usize),
    /// Pop `n` values, concatenate their `as_display` (interpolation), push the `Str`.
    Concat(usize),
    /// Pop `n` values into a `List` (top-of-stack is the last element).
    MakeList(usize),
    /// Pop an int index and a list; push the element clone (bounds-checked).
    Index,
    /// Pop a list; push its length as an `Int`.
    Len,
    /// Pop two ints (`end` on top, then `start`) and push a `List<int>` materialization of the
    /// range: `[start, …, end-1]` exclusive, or `[start, …, end]` inclusive (the `bool` flag). Empty
    /// when `start >= end` (exclusive) / `start > end` (inclusive). Built via Rust's native
    /// `start..end` / `start..=end`, which stop at `i64::MAX` without a counter overflow (EV-7).
    /// Carries no static index, so — like `GetEnumField` — it needs no `validate` arm (decision
    /// S1-R, M3 S1.2).
    MakeRange(bool),
    /// Call the native (built-in) function at `native::registry()[idx]` with the top `argc` values
    /// as its arguments (source order): pop them, run the native's shared `eval` (which may append
    /// to the program output), and push its return value. The migrated former `Op::Print`
    /// (`console.println` is `CallNative(native::CONSOLE_PRINTLN, 1)`); the namespaced stdlib's one
    /// runtime entry point (M3 Wave 1). Pushes a result, so it carries a `validate` arm (the index
    /// is bounded by the registry length).
    CallNative(usize, usize),
    /// Call `functions[idx]`: its args are already on top of the stack; the new frame's
    /// local window opens at `stack.len() - functions[idx].arity` (decision P3-1, P3-3).
    Call(usize),
    /// Pop the return value, unwind the current frame (truncate its slot window), pop the
    /// frame, push the return value onto the caller's stack. End execution when the last
    /// (`main`) frame returns (decision P3-2).
    Return,
    /// Construct an enum value from `enum_descs[idx]`: pop `desc.arity` payload values (in
    /// source order — top of stack is the last field) and push `Value::Enum` (decision P4-3).
    MakeEnum(usize),
    /// Pop the scrutinee and push a `Bool`: whether it is a `Value::Enum` whose variant equals
    /// `enum_descs[idx].variant`. Variant names are globally unique (the checker keys them by
    /// name), so the variant string alone disambiguates. Used by `match` arm dispatch (P4-7).
    MatchTag(usize),
    /// Pop an enum value and push a clone of its payload element `i`. The compiler only emits
    /// this for an index a preceding `MatchTag` already proved in range (P4-7); a defensive
    /// runtime fault covers misuse (EV-7).
    GetEnumField(usize),
    /// Abort with a fixed runtime-fault message selected by [`FaultMsg`]. Generalizes the former
    /// `MatchFail` (M3 S2.5): both the `match` exhaustiveness backstop and `opt!`-on-null lower to
    /// this one op, so S2 adds **no new `Op` variant**. The message text lives in the handler (not
    /// the const pool), keeping both backends byte-identical (the `agree_err` oracle classifies
    /// faults by body substring). Carries no static index, so — like `MakeRange` — it needs no
    /// `validate` arm.
    Fault(FaultMsg),
    /// Construct a class instance from `class_descs[idx]`: pop `desc.fields.len()` promoted-field
    /// values (in declaration order — top of stack is the last field), zip them with
    /// `desc.fields`, and push a `Value::Instance`. Emitted only inside a synthetic constructor
    /// function, after its promoted params have been loaded (decision P4-4).
    MakeInstance(usize),
    /// Pop an instance and push a clone of its field named `names[idx]`. A read of an absent field
    /// (a checker-valid but uninitialized explicit `Field` member) faults
    /// `no field \`{name}\` on \`{class}\`` — byte-identical to the interpreter (decision P4-5).
    GetField(usize),
    /// Call an instance method `(name_idx, argc)`: the receiver and its `argc` args sit on the
    /// stack as `[.., receiver, arg0 … arg_{argc-1}]`. At runtime, resolve
    /// `(receiver.class, names[name_idx])` through `BytecodeProgram.methods` to a function index and
    /// open a frame whose slot 0 is the receiver (`this`) and slots `1..=argc` are the args
    /// (decision P4-6). Method existence is checker-enforced; the resolution-miss fault is a
    /// defensive backstop (byte-identical to the interpreter).
    CallMethod(usize, usize),
    /// Build a `Value::Closure` from `functions[idx]`: pop the top `functions[idx].n_captures`
    /// values (captures, in sorted order — invariant #8), then push the closure. For a named
    /// function reference (0 captures), nothing is popped. (M3 S3, Task 4.)
    MakeClosure(usize),
    /// Call a first-class function value: pop `argc` args (source order, top is the last),
    /// pop the `Value::Closure` beneath them, push captures then args into the new frame's slot
    /// window (layout `[caps.., args..]` mirrors the sub-compiler's `[caps, params]` ordering),
    /// and open a frame. Carries no static table index, so — like `GetEnumField` — it needs no
    /// `validate` arm. (M3 S3, Task 4.)
    CallValue(usize),
    /// Pop the top value and push a `Bool`: whether it is a `Value::Instance` whose class equals the
    /// carried class name (`value instanceof TypeName`, M-RT S1). A non-instance value is `false`,
    /// never a fault — matching the interpreter and PHP's `instanceof`. The class name is carried
    /// inline (like `Fault(FaultMsg)`), not via a pool index, so — like `MakeRange`/`Fault` — it
    /// needs no `validate` arm.
    IsInstance(String),
}

/// A unit of compiled bytecode: instructions, a constant pool, and a per-instruction
/// source-line table (for runtime-error reporting).
#[derive(Debug, Clone, Default)]
pub struct Chunk {
    pub code: Vec<Op>,
    pub consts: Vec<Value>,
    pub lines: Vec<u32>,
    /// Build-time interning table: scalar constant → its pool index, so `add_const` dedups
    /// repeated literals instead of growing the pool per occurrence. Not part of the emitted
    /// bytecode — it only steers `add_const` while a `Chunk` is under construction.
    const_index: HashMap<ConstKey, usize>,
}

impl Chunk {
    pub fn new() -> Self {
        Self::default()
    }

    /// Intern a constant, returning its pool index. A repeated scalar (same int / bit-equal float /
    /// equal string / bool / unit) reuses its existing slot, so the pool grows with *distinct*
    /// values, not occurrences — keeping the constant pool (and the future P4/P5 GC root set that
    /// scans it) lean. Composite constants have no key and always get a fresh slot.
    pub fn add_const(&mut self, v: Value) -> usize {
        if let Some(key) = ConstKey::of(&v) {
            if let Some(&idx) = self.const_index.get(&key) {
                return idx;
            }
            let idx = self.consts.len();
            self.const_index.insert(key, idx);
            self.consts.push(v);
            idx
        } else {
            self.consts.push(v);
            self.consts.len() - 1
        }
    }

    /// Append an instruction tagged with its source line.
    pub fn emit(&mut self, op: Op, line: u32) {
        self.code.push(op);
        self.lines.push(line);
    }
}

/// A compiled function: name, parameter count, and its own bytecode chunk. Each function
/// owns its chunk so its jump targets and constant pool are self-contained (decision P3-1).
/// `n_captures` is the number of captured values that `Op::MakeClosure` pops from the enclosing
/// frame before constructing the closure; it is 0 for named free functions, constructors, and
/// methods (they are never constructed via `MakeClosure` with captures).
#[derive(Debug, Clone)]
pub struct Function {
    pub name: String,
    pub arity: usize,
    /// Number of captured values this closure pops off the enclosing stack at construction time
    /// (`Op::MakeClosure`). Always 0 for named functions, constructors, and methods.
    pub n_captures: usize,
    pub chunk: Chunk,
}

/// A static descriptor for one enum variant: its enum type, variant name, and payload arity.
/// Built once in the compiler pre-pass (every variant of every declared enum) and indexed by
/// `Op::MakeEnum`/`Op::MatchTag` — the enum analogue of the constant pool (decision P4-2).
#[derive(Debug, Clone)]
pub struct EnumDesc {
    pub ty: String,
    pub variant: String,
    pub arity: usize,
}

/// A static descriptor for one class: its name and the ordered list of promoted-field names a
/// constructor populates. Built once in the compiler pre-pass and indexed by `Op::MakeInstance`
/// (decision P4-2/P4-4). Explicit (non-promoted) `Field` members are *not* listed here — like the
/// interpreter, construction populates only promoted ctor params; reading an explicit field is a
/// runtime `no field` fault.
#[derive(Debug, Clone)]
pub struct ClassDesc {
    pub class: String,
    pub fields: Vec<String>,
}

/// A whole compiled program: every top-level function (free functions, then synthetic
/// constructors, then methods), the index of `main`, and the program-level descriptor tables
/// shared across all functions — enum-variant descriptors, class descriptors, an interned
/// member/field-name pool indexed by `Op::GetField`, and the `(class, method) → function index`
/// dispatch table read by `Op::CallMethod` (decision P4-2/P4-6).
#[derive(Debug, Clone)]
pub struct BytecodeProgram {
    pub functions: Vec<Function>,
    pub main: usize,
    pub enum_descs: Vec<EnumDesc>,
    pub class_descs: Vec<ClassDesc>,
    pub names: Vec<String>,
    pub methods: HashMap<(String, String), usize>,
    /// The transitively-flattened interface set each class implements, keyed by class name — the
    /// runtime `instanceof`-against-an-interface table (M-RT S2). Built once by
    /// [`crate::ast::class_implements`] (the same call the interpreter + checker make), so the
    /// `Op::IsInstance` test is byte-identical across backends. A `BTreeMap`/sorted values keep it
    /// deterministic for `disasm`.
    pub class_implements: BTreeMap<String, Vec<String>>,
}

impl BytecodeProgram {
    /// Check that every index-carrying instruction references something in range, before the VM
    /// executes a single op. An out-of-range `Const`/`Call`/jump is always a *compiler* bug, never
    /// user error — but surfacing it as a clean `Err` (rather than a bare `index out of bounds`
    /// panic, or a silent wrong read) keeps the VM's no-crash contract (EV-7). Slot operands
    /// (`GetLocal`/`SetLocal`) can't be range-checked here — their bound is the runtime locals
    /// window, not anything static — so they stay covered by the VM's `frame_slot` debug-assert.
    ///
    /// P4a added the index-carrying ops `MakeEnum`/`MatchTag` (into `enum_descs`); P4b added
    /// `MakeInstance` (into `class_descs`) and `GetField` (into the `names` pool); P4c adds
    /// `CallMethod` (name into the `names` pool; its function target is resolved at runtime via the
    /// method table, range-checked after the per-op loop). M3 Wave 1 adds `CallNative` (index into
    /// `native::registry()`). Each new index-carrying op extends the
    /// match below in lockstep (see memory `op-variant-match-coupling`). `GetEnumField` carries a
    /// payload index with no static bound (like a local slot) — covered by the VM's runtime guard;
    /// M3 S1.2's `MakeRange(bool)` carries a flag, not an index, so it likewise needs no arm here.
    pub fn validate(&self) -> Result<(), String> {
        let nfns = self.functions.len();
        if self.main >= nfns {
            return Err(format!(
                "invalid bytecode: main index {} out of range ({nfns} functions)",
                self.main
            ));
        }
        let ndescs = self.enum_descs.len();
        let nclasses = self.class_descs.len();
        let nnames = self.names.len();
        let nnatives = crate::native::registry().len();
        for (fi, f) in self.functions.iter().enumerate() {
            let code_len = f.chunk.code.len();
            let const_len = f.chunk.consts.len();
            for (ip, op) in f.chunk.code.iter().enumerate() {
                // Exhaustive over `Op` — deliberately NO `_` wildcard. Every variant either
                // carries a pool index that is range-checked here, or is listed in the no-index
                // arm below. A newly added `Op` is therefore a COMPILE ERROR in this match until
                // its bounds intent is declared, closing the old `_ => None` gap that let a new
                // index-carrying op skip its EV-7 check (QW-15 / P1-#16). `exec_op` (src/vm.rs)
                // and `stack_effect` (src/compiler.rs) are already exhaustive; this brings
                // `validate` to the same guarantee. The `.then(|| …)` arms reject on exactly the
                // same condition as the previous guarded arms — rejection behaviour is unchanged.
                let problem = match op {
                    Op::Const(i) => (*i >= const_len)
                        .then(|| format!("const index {i} out of range (pool has {const_len})")),
                    Op::Call(idx) => (*idx >= nfns)
                        .then(|| format!("call target {idx} out of range ({nfns} functions)")),
                    Op::MakeEnum(idx) | Op::MatchTag(idx) => (*idx >= ndescs).then(|| {
                        format!("enum descriptor index {idx} out of range ({ndescs} descriptors)")
                    }),
                    Op::MakeInstance(idx) => (*idx >= nclasses).then(|| {
                        format!(
                            "class descriptor index {idx} out of range ({nclasses} descriptors)"
                        )
                    }),
                    Op::GetField(idx) | Op::CallMethod(idx, _) => (*idx >= nnames).then(|| {
                        format!("field-name index {idx} out of range (name pool has {nnames})")
                    }),
                    Op::CallNative(idx, _) => (*idx >= nnatives).then(|| {
                        format!("native index {idx} out of range (registry has {nnatives})")
                    }),
                    // Absolute targets; `== code_len` is the legal "fall off the end → implicit
                    // return" landing the run loop already handles, so only `>` is invalid.
                    Op::Jump(t) | Op::JumpIfFalse(t) => (*t > code_len)
                        .then(|| format!("jump target {t} out of range (code len {code_len})")),
                    // `MakeClosure` carries a function-table index (must be in range).
                    Op::MakeClosure(idx) => (*idx >= nfns)
                        .then(|| format!("closure target {idx} out of range ({nfns} functions)")),
                    // No pool index to range-check here. These carry either nothing, a count
                    // (`Concat`/`MakeList`/`CallValue` arg counts), a local stack slot
                    // (`GetLocal`/`SetLocal`, bounded by frame sizing, not a pool), or a payload
                    // index a preceding `MatchTag` already proved (`GetEnumField`). Listed
                    // explicitly so the match stays exhaustive.
                    Op::AddI
                    | Op::SubI
                    | Op::MulI
                    | Op::DivI
                    | Op::RemI
                    | Op::AddF
                    | Op::SubF
                    | Op::MulF
                    | Op::DivF
                    | Op::RemF
                    | Op::Neg
                    | Op::Not
                    | Op::Eq
                    | Op::Ne
                    | Op::Lt
                    | Op::Gt
                    | Op::Le
                    | Op::Ge
                    | Op::Pop
                    | Op::GetLocal(_)
                    | Op::SetLocal(_)
                    | Op::Concat(_)
                    | Op::MakeList(_)
                    | Op::Index
                    | Op::Len
                    | Op::MakeRange(_)
                    | Op::Return
                    | Op::GetEnumField(_)
                    | Op::Fault(_)
                    | Op::CallValue(_)
                    // Carries the class name inline (like `Fault`), not a pool index.
                    | Op::IsInstance(_) => None,
                };
                if let Some(what) = problem {
                    return Err(format!(
                        "invalid bytecode in fn `{}` (#{fi}) at ip {ip}: {what}",
                        f.name
                    ));
                }
            }
        }
        // `Op::CallMethod` resolves its target through the method table at runtime (the function
        // index isn't in the op), so range-check every dispatch target here instead.
        for ((class, method), &idx) in &self.methods {
            if idx >= nfns {
                return Err(format!(
                    "invalid bytecode: method `{class}::{method}` target {idx} out of range ({nfns} functions)"
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::Value;

    #[test]
    fn add_const_returns_sequential_indices() {
        let mut c = Chunk::new();
        assert_eq!(c.add_const(Value::Int(1)), 0);
        assert_eq!(c.add_const(Value::Int(2)), 1);
        assert_eq!(c.consts.len(), 2);
    }

    #[test]
    fn add_const_interns_duplicate_scalars() {
        let mut c = Chunk::new();
        // Repeated scalars reuse their slot: the pool grows with distinct values, not occurrences.
        assert_eq!(c.add_const(Value::Int(7)), 0);
        assert_eq!(c.add_const(Value::Int(7)), 0); // same int → same index
        assert_eq!(c.add_const(Value::Float(1.5)), 1);
        assert_eq!(c.add_const(Value::Float(1.5)), 1); // bit-equal float → same index
        assert_eq!(c.add_const(Value::Str("hi".into())), 2);
        assert_eq!(c.add_const(Value::Str("hi".into())), 2); // equal string → same index
        assert_eq!(c.add_const(Value::Int(8)), 3); // distinct value → new slot
        assert_eq!(c.consts.len(), 4);
    }

    #[test]
    fn add_const_does_not_intern_composites() {
        let mut c = Chunk::new();
        // Lists have no dedup key — each gets a fresh slot even if structurally equal.
        assert_eq!(c.add_const(Value::List(vec![Value::Int(1)].into())), 0);
        assert_eq!(c.add_const(Value::List(vec![Value::Int(1)].into())), 1);
        assert_eq!(c.consts.len(), 2);
    }

    #[test]
    fn emit_tracks_code_and_lines() {
        let mut c = Chunk::new();
        c.emit(Op::Const(0), 1);
        c.emit(Op::Return, 2);
        assert_eq!(c.code.len(), 2);
        assert_eq!(c.lines, vec![1, 2]);
    }

    #[test]
    fn validate_accepts_a_well_formed_program() {
        let mut c = Chunk::new();
        let k = c.add_const(Value::Int(1));
        c.emit(Op::Const(k), 1);
        c.emit(Op::Jump(2), 1); // == code_len after the next emit: legal "fall off → return"
        c.emit(Op::Return, 1);
        let prog = BytecodeProgram {
            functions: vec![Function {
                name: "main".into(),
                arity: 0,
                n_captures: 0,
                chunk: c,
            }],
            main: 0,
            enum_descs: Vec::new(),
            class_descs: Vec::new(),
            names: Vec::new(),
            methods: HashMap::new(),
            class_implements: BTreeMap::new(),
        };
        assert_eq!(prog.validate(), Ok(()));
    }

    #[test]
    fn validate_rejects_out_of_range_const() {
        let mut c = Chunk::new(); // empty const pool
        c.emit(Op::Const(99), 1);
        c.emit(Op::Return, 1);
        let prog = BytecodeProgram {
            functions: vec![Function {
                name: "main".into(),
                arity: 0,
                n_captures: 0,
                chunk: c,
            }],
            main: 0,
            enum_descs: Vec::new(),
            class_descs: Vec::new(),
            names: Vec::new(),
            methods: HashMap::new(),
            class_implements: BTreeMap::new(),
        };
        let err = prog.validate().unwrap_err();
        assert!(err.contains("invalid bytecode"), "{err}");
        assert!(err.contains("const index 99"), "{err}");
    }

    #[test]
    fn validate_rejects_out_of_range_call_and_bad_main() {
        let mut c = Chunk::new();
        c.emit(Op::Call(7), 1); // only 1 function exists
        c.emit(Op::Return, 1);
        let prog = BytecodeProgram {
            functions: vec![Function {
                name: "main".into(),
                arity: 0,
                n_captures: 0,
                chunk: c,
            }],
            main: 0,
            enum_descs: Vec::new(),
            class_descs: Vec::new(),
            names: Vec::new(),
            methods: HashMap::new(),
            class_implements: BTreeMap::new(),
        };
        assert!(prog.validate().unwrap_err().contains("call target 7"));

        let bad_main = BytecodeProgram {
            functions: vec![],
            main: 0,
            enum_descs: Vec::new(),
            class_descs: Vec::new(),
            names: Vec::new(),
            methods: HashMap::new(),
            class_implements: BTreeMap::new(),
        };
        assert!(bad_main.validate().unwrap_err().contains("main index 0"));
    }

    #[test]
    fn validate_rejects_out_of_range_enum_desc() {
        let mut c = Chunk::new();
        c.emit(Op::MakeEnum(3), 1); // no descriptors in the table
        c.emit(Op::Return, 1);
        let prog = BytecodeProgram {
            functions: vec![Function {
                name: "main".into(),
                arity: 0,
                n_captures: 0,
                chunk: c,
            }],
            main: 0,
            enum_descs: Vec::new(),
            class_descs: Vec::new(),
            names: Vec::new(),
            methods: HashMap::new(),
            class_implements: BTreeMap::new(),
        };
        let err = prog.validate().unwrap_err();
        assert!(err.contains("enum descriptor index 3"), "{err}");
    }

    #[test]
    fn validate_rejects_out_of_range_class_and_field() {
        let mut c = Chunk::new();
        c.emit(Op::MakeInstance(2), 1); // no class descriptors
        c.emit(Op::Return, 1);
        let prog = BytecodeProgram {
            functions: vec![Function {
                name: "main".into(),
                arity: 0,
                n_captures: 0,
                chunk: c,
            }],
            main: 0,
            enum_descs: Vec::new(),
            class_descs: Vec::new(),
            names: Vec::new(),
            methods: HashMap::new(),
            class_implements: BTreeMap::new(),
        };
        assert!(prog
            .validate()
            .unwrap_err()
            .contains("class descriptor index 2"));

        let mut c2 = Chunk::new();
        c2.emit(Op::GetField(5), 1); // empty name pool
        c2.emit(Op::Return, 1);
        let prog2 = BytecodeProgram {
            functions: vec![Function {
                name: "main".into(),
                arity: 0,
                n_captures: 0,
                chunk: c2,
            }],
            main: 0,
            enum_descs: Vec::new(),
            class_descs: Vec::new(),
            names: Vec::new(),
            methods: HashMap::new(),
            class_implements: BTreeMap::new(),
        };
        assert!(prog2.validate().unwrap_err().contains("field-name index 5"));
    }

    #[test]
    fn validate_rejects_out_of_range_native() {
        let mut c = Chunk::new();
        c.emit(Op::CallNative(9999, 1), 1); // far past the registry length
        c.emit(Op::Return, 1);
        let prog = BytecodeProgram {
            functions: vec![Function {
                name: "main".into(),
                arity: 0,
                n_captures: 0,
                chunk: c,
            }],
            main: 0,
            enum_descs: Vec::new(),
            class_descs: Vec::new(),
            names: Vec::new(),
            methods: HashMap::new(),
            class_implements: BTreeMap::new(),
        };
        assert!(prog.validate().unwrap_err().contains("native index 9999"));
    }

    #[test]
    fn validate_rejects_out_of_range_closure() {
        // `MakeClosure` carries a function-table index; the only function is `main` (index 0),
        // so index 4 is out of range. Guards the EV-7 bound that the exhaustive `validate` match
        // keeps (the closure arm), distinct from `Op::Call`'s.
        let mut c = Chunk::new();
        c.emit(Op::MakeClosure(4), 1);
        c.emit(Op::Return, 1);
        let prog = BytecodeProgram {
            functions: vec![Function {
                name: "main".into(),
                arity: 0,
                n_captures: 0,
                chunk: c,
            }],
            main: 0,
            enum_descs: Vec::new(),
            class_descs: Vec::new(),
            names: Vec::new(),
            methods: HashMap::new(),
            class_implements: BTreeMap::new(),
        };
        let err = prog.validate().unwrap_err();
        assert!(err.contains("closure target 4"), "{err}");
    }

    #[test]
    fn validate_accepts_unchecked_no_index_ops() {
        // The no-index arm returns `None` (no rejection) for ops that carry a count or local slot
        // rather than a pool index — e.g. a large `CallValue` arg count and a high `GetLocal`
        // slot. This pins the "behaviour unchanged" half of making the match exhaustive: these
        // are validated elsewhere (frame sizing / runtime), never by `validate`.
        let mut c = Chunk::new();
        c.emit(Op::GetLocal(9999), 1);
        c.emit(Op::CallValue(250), 1);
        c.emit(Op::Return, 1);
        let prog = BytecodeProgram {
            functions: vec![Function {
                name: "main".into(),
                arity: 0,
                n_captures: 0,
                chunk: c,
            }],
            main: 0,
            enum_descs: Vec::new(),
            class_descs: Vec::new(),
            names: Vec::new(),
            methods: HashMap::new(),
            class_implements: BTreeMap::new(),
        };
        assert!(prog.validate().is_ok());
    }

    #[test]
    fn bytecode_program_holds_functions_and_main_index() {
        let mut c = Chunk::new();
        c.emit(Op::Return, 1);
        let prog = BytecodeProgram {
            functions: vec![Function {
                name: "main".into(),
                arity: 0,
                n_captures: 0,
                chunk: c,
            }],
            main: 0,
            enum_descs: Vec::new(),
            class_descs: Vec::new(),
            names: Vec::new(),
            methods: HashMap::new(),
            class_implements: BTreeMap::new(),
        };
        assert_eq!(prog.functions[prog.main].name, "main");
        assert_eq!(prog.functions[0].arity, 0);
    }
}
