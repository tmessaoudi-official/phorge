//! Stack VM that executes a `Chunk`. See docs/specs/2026-06-15-m2-bytecode-vm-design.md (§6).
//! P1: scalar arithmetic, negate, print, return. Output is captured into a String (mirrors
//! `interpreter::interpret`) so the VM can be differential-tested against the tree-walker.
//!
//! `Err` carries the bare runtime-error message (no "runtime error:" prefix), matching
//! `interpreter`'s `RuntimeError.message`; the future `cmd_runvm` adds the prefix, keeping
//! error parity with `cmd_run`.

use crate::chunk::{Chunk, Op};
use crate::value::Value;

pub struct Vm<'a> {
    chunk: &'a Chunk,
    stack: Vec<Value>,
    out: String,
}

impl<'a> Vm<'a> {
    pub fn new(chunk: &'a Chunk) -> Self {
        Self { chunk, stack: Vec::new(), out: String::new() }
    }

    /// Execute the chunk, returning captured output (`Ok`) or a runtime error (`Err`).
    pub fn run(mut self) -> Result<String, String> {
        let mut ip = 0;
        while ip < self.chunk.code.len() {
            // Clone the op (cheap) so we don't hold a borrow of `self.chunk` across the
            // mutable stack operations below.
            let op = self.chunk.code[ip].clone();
            ip += 1;
            match op {
                Op::Const(i) => self.stack.push(self.chunk.consts[i].clone()),

                Op::AddI => {
                    let (a, b) = self.pop2_int()?;
                    self.push_i(a.checked_add(b))?;
                }
                Op::SubI => {
                    let (a, b) = self.pop2_int()?;
                    self.push_i(a.checked_sub(b))?;
                }
                Op::MulI => {
                    let (a, b) = self.pop2_int()?;
                    self.push_i(a.checked_mul(b))?;
                }
                Op::DivI => {
                    let (a, b) = self.pop2_int()?;
                    if b == 0 {
                        return Err("division by zero".to_string());
                    }
                    self.push_i(a.checked_div(b))?;
                }

                Op::AddF => {
                    let (a, b) = self.pop2_float()?;
                    self.stack.push(Value::Float(a + b));
                }
                Op::SubF => {
                    let (a, b) = self.pop2_float()?;
                    self.stack.push(Value::Float(a - b));
                }
                Op::MulF => {
                    let (a, b) = self.pop2_float()?;
                    self.stack.push(Value::Float(a * b));
                }
                Op::DivF => {
                    let (a, b) = self.pop2_float()?;
                    self.stack.push(Value::Float(a / b));
                }

                Op::Neg => match self.pop() {
                    Value::Int(n) => self.stack.push(Value::Int(-n)),
                    Value::Float(x) => self.stack.push(Value::Float(-x)),
                    v => return Err(format!("cannot negate {}", v.type_name())),
                },

                Op::Print => {
                    let v = self.pop();
                    let s = v
                        .as_display()
                        .ok_or_else(|| format!("cannot print {}", v.type_name()))?;
                    self.out.push_str(&s);
                    self.out.push('\n');
                }

                Op::Return => return Ok(self.out),
            }
        }
        Ok(self.out)
    }

    fn pop(&mut self) -> Value {
        self.stack.pop().expect("vm stack underflow (compiler bug)")
    }

    /// Pop two ints in operand order: returns `(lhs, rhs)` for `lhs OP rhs`.
    fn pop2_int(&mut self) -> Result<(i64, i64), String> {
        let b = self.pop_int()?;
        let a = self.pop_int()?;
        Ok((a, b))
    }

    fn pop2_float(&mut self) -> Result<(f64, f64), String> {
        let b = self.pop_float()?;
        let a = self.pop_float()?;
        Ok((a, b))
    }

    fn pop_int(&mut self) -> Result<i64, String> {
        match self.pop() {
            Value::Int(n) => Ok(n),
            v => Err(format!("expected int, found {}", v.type_name())),
        }
    }

    fn pop_float(&mut self) -> Result<f64, String> {
        match self.pop() {
            Value::Float(x) => Ok(x),
            v => Err(format!("expected float, found {}", v.type_name())),
        }
    }

    fn push_i(&mut self, r: Option<i64>) -> Result<(), String> {
        let n = r.ok_or_else(|| "integer overflow".to_string())?;
        self.stack.push(Value::Int(n));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk::{Chunk, Op};
    use crate::value::Value;

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
        c.emit(Op::Print, 1);
        c.emit(Op::Return, 1);
        c
    }

    #[test]
    fn runs_integer_arithmetic_and_prints() {
        let out = Vm::new(&arith_chunk()).run().unwrap();
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
        c.emit(Op::Print, 1);
        c.emit(Op::Return, 1);
        assert_eq!(Vm::new(&c).run().unwrap(), "4\n");
    }

    #[test]
    fn division_by_zero_is_runtime_error() {
        let mut c = Chunk::new();
        let a = c.add_const(Value::Int(1));
        let z = c.add_const(Value::Int(0));
        c.emit(Op::Const(a), 1);
        c.emit(Op::Const(z), 1);
        c.emit(Op::DivI, 1);
        c.emit(Op::Return, 1);
        let err = Vm::new(&c).run().unwrap_err();
        assert!(err.contains("division by zero"), "{err}");
    }

    #[test]
    fn negate_works_for_int_and_float() {
        let mut c = Chunk::new();
        let a = c.add_const(Value::Int(5));
        c.emit(Op::Const(a), 1);
        c.emit(Op::Neg, 1);
        c.emit(Op::Print, 1);
        c.emit(Op::Return, 1);
        assert_eq!(Vm::new(&c).run().unwrap(), "-5\n");
    }
}
