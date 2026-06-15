//! Bytecode chunk + instruction set for the M2 VM.
//! See docs/specs/2026-06-15-m2-bytecode-vm-design.md (§4, §5).
//! P2 scope: full M1 expression/statement surface for `main` (see
//! docs/plans/2026-06-15-m2-plan2-compiler-runvm.md). Reuses `value::Value` (scalar
//! formatting parity with the interpreter is free); lists are inline in P2 — the VM
//! heap/handle object model arrives in P4.

use crate::value::Value;

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
    /// End execution, returning captured output.
    Return,
}

/// A unit of compiled bytecode: instructions, a constant pool, and a per-instruction
/// source-line table (for runtime-error reporting).
#[derive(Debug, Clone, Default)]
pub struct Chunk {
    pub code: Vec<Op>,
    pub consts: Vec<Value>,
    pub lines: Vec<u32>,
}

impl Chunk {
    pub fn new() -> Self {
        Self::default()
    }

    /// Intern a constant, returning its pool index.
    pub fn add_const(&mut self, v: Value) -> usize {
        self.consts.push(v);
        self.consts.len() - 1
    }

    /// Append an instruction tagged with its source line.
    pub fn emit(&mut self, op: Op, line: u32) {
        self.code.push(op);
        self.lines.push(line);
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
    fn emit_tracks_code_and_lines() {
        let mut c = Chunk::new();
        c.emit(Op::Const(0), 1);
        c.emit(Op::Return, 2);
        assert_eq!(c.code.len(), 2);
        assert_eq!(c.lines, vec![1, 2]);
    }
}
