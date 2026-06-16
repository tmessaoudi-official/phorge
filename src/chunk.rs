//! Bytecode chunk + instruction set for the M2 VM.
//! See docs/specs/2026-06-15-m2-bytecode-vm-design.md (§4, §5).
//! P2 scope: full M1 expression/statement surface for `main` (see
//! docs/plans/2026-06-15-m2-plan2-compiler-runvm.md). Reuses `value::Value` (scalar
//! formatting parity with the interpreter is free); lists are inline in P2 — the VM
//! heap/handle object model arrives in P4.

use crate::value::Value;
use std::collections::HashMap;

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
    /// Pop `n` values, space-join their `as_display`, append a line to output.
    Print(usize),
    /// Call `functions[idx]`: its args are already on top of the stack; the new frame's
    /// local window opens at `stack.len() - functions[idx].arity` (decision P3-1, P3-3).
    Call(usize),
    /// Pop the return value, unwind the current frame (truncate its slot window), pop the
    /// frame, push the return value onto the caller's stack. End execution when the last
    /// (`main`) frame returns (decision P3-2).
    Return,
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
#[derive(Debug, Clone)]
pub struct Function {
    pub name: String,
    pub arity: usize,
    pub chunk: Chunk,
}

/// A whole compiled program: every top-level function plus the index of `main`.
#[derive(Debug, Clone)]
pub struct BytecodeProgram {
    pub functions: Vec<Function>,
    pub main: usize,
}

impl BytecodeProgram {
    /// Check that every index-carrying instruction references something in range, before the VM
    /// executes a single op. An out-of-range `Const`/`Call`/jump is always a *compiler* bug, never
    /// user error — but surfacing it as a clean `Err` (rather than a bare `index out of bounds`
    /// panic, or a silent wrong read) keeps the VM's no-crash contract (EV-7). Slot operands
    /// (`GetLocal`/`SetLocal`) can't be range-checked here — their bound is the runtime locals
    /// window, not anything static — so they stay covered by the VM's `frame_slot` debug-assert.
    ///
    /// P4 adds index-carrying ops (`MakeInstance`, `GetField(idx)`, `MatchTag`) that multiply this
    /// surface; extend the match below in lockstep (see memory `op-variant-match-coupling`).
    pub fn validate(&self) -> Result<(), String> {
        let nfns = self.functions.len();
        if self.main >= nfns {
            return Err(format!(
                "invalid bytecode: main index {} out of range ({nfns} functions)",
                self.main
            ));
        }
        for (fi, f) in self.functions.iter().enumerate() {
            let code_len = f.chunk.code.len();
            let const_len = f.chunk.consts.len();
            for (ip, op) in f.chunk.code.iter().enumerate() {
                let problem = match op {
                    Op::Const(i) if *i >= const_len => Some(format!(
                        "const index {i} out of range (pool has {const_len})"
                    )),
                    Op::Call(idx) if *idx >= nfns => {
                        Some(format!("call target {idx} out of range ({nfns} functions)"))
                    }
                    // Absolute targets; `== code_len` is the legal "fall off the end → implicit
                    // return" landing the run loop already handles, so only `>` is invalid.
                    Op::Jump(t) | Op::JumpIfFalse(t) if *t > code_len => Some(format!(
                        "jump target {t} out of range (code len {code_len})"
                    )),
                    _ => None,
                };
                if let Some(what) = problem {
                    return Err(format!(
                        "invalid bytecode in fn `{}` (#{fi}) at ip {ip}: {what}",
                        f.name
                    ));
                }
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
        assert_eq!(c.add_const(Value::List(vec![Value::Int(1)])), 0);
        assert_eq!(c.add_const(Value::List(vec![Value::Int(1)])), 1);
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
                chunk: c,
            }],
            main: 0,
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
                chunk: c,
            }],
            main: 0,
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
                chunk: c,
            }],
            main: 0,
        };
        assert!(prog.validate().unwrap_err().contains("call target 7"));

        let bad_main = BytecodeProgram {
            functions: vec![],
            main: 0,
        };
        assert!(bad_main.validate().unwrap_err().contains("main index 0"));
    }

    #[test]
    fn bytecode_program_holds_functions_and_main_index() {
        let mut c = Chunk::new();
        c.emit(Op::Return, 1);
        let prog = BytecodeProgram {
            functions: vec![Function {
                name: "main".into(),
                arity: 0,
                chunk: c,
            }],
            main: 0,
        };
        assert_eq!(prog.functions[prog.main].name, "main");
        assert_eq!(prog.functions[0].arity, 0);
    }
}
