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
                Op::RemI => {
                    let (a, b) = self.pop2_int()?;
                    if b == 0 {
                        return Err("modulo by zero".to_string());
                    }
                    self.push_i(a.checked_rem(b))?;
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
                Op::RemF => {
                    let (a, b) = self.pop2_float()?;
                    self.stack.push(Value::Float(a % b));
                }

                Op::Neg => match self.pop() {
                    Value::Int(n) => self.stack.push(Value::Int(-n)),
                    Value::Float(x) => self.stack.push(Value::Float(-x)),
                    v => return Err(format!("cannot negate {}", v.type_name())),
                },
                Op::Not => match self.pop() {
                    Value::Bool(b) => self.stack.push(Value::Bool(!b)),
                    v => return Err(format!("cannot apply ! to {}", v.type_name())),
                },

                Op::Eq => {
                    let b = self.pop();
                    let a = self.pop();
                    self.stack.push(Value::Bool(a.eq_val(&b)));
                }
                Op::Ne => {
                    let b = self.pop();
                    let a = self.pop();
                    self.stack.push(Value::Bool(!a.eq_val(&b)));
                }
                Op::Lt | Op::Gt | Op::Le | Op::Ge => {
                    let b = self.pop();
                    let a = self.pop();
                    self.stack.push(Value::Bool(compare(&op, &a, &b)?));
                }

                Op::Pop => {
                    self.pop();
                }
                Op::GetLocal(slot) => {
                    let v = self.stack[slot].clone();
                    self.stack.push(v);
                }
                Op::SetLocal(slot) => {
                    let v = self.pop();
                    self.stack[slot] = v;
                }

                Op::Jump(target) => ip = target,
                Op::JumpIfFalse(target) => match self.pop() {
                    Value::Bool(false) => ip = target,
                    Value::Bool(true) => {}
                    v => return Err(format!("expected bool, found {}", v.type_name())),
                },

                Op::Concat(n) => {
                    let parts = self.split_off(n);
                    let mut s = String::new();
                    for v in &parts {
                        match v.as_display() {
                            Some(t) => s.push_str(&t),
                            None => {
                                return Err(format!(
                                    "cannot interpolate {} into a string",
                                    v.type_name()
                                ))
                            }
                        }
                    }
                    self.stack.push(Value::Str(s));
                }
                Op::MakeList(n) => {
                    let items = self.split_off(n);
                    self.stack.push(Value::List(items));
                }
                Op::Index => {
                    let idx = match self.pop() {
                        Value::Int(n) => n,
                        v => return Err(format!("expected int index, found {}", v.type_name())),
                    };
                    let list = match self.pop() {
                        Value::List(xs) => xs,
                        v => return Err(format!("cannot index {}", v.type_name())),
                    };
                    let i = usize::try_from(idx)
                        .ok()
                        .filter(|i| *i < list.len())
                        .ok_or_else(|| "list index out of range".to_string())?;
                    self.stack.push(list[i].clone());
                }
                Op::Len => match self.pop() {
                    Value::List(xs) => self.stack.push(Value::Int(xs.len() as i64)),
                    v => return Err(format!("cannot take length of {}", v.type_name())),
                },

                Op::Print(n) => {
                    let parts = self.split_off(n);
                    let mut line = String::new();
                    for (i, v) in parts.iter().enumerate() {
                        if i > 0 {
                            line.push(' ');
                        }
                        match v.as_display() {
                            Some(t) => line.push_str(&t),
                            None => {
                                return Err(format!("println cannot print {}", v.type_name()))
                            }
                        }
                    }
                    self.out.push_str(&line);
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

    /// Pop the top `n` values, returning them in stack order (bottom-most first).
    /// The compiler guarantees `n <= stack.len()`.
    fn split_off(&mut self, n: usize) -> Vec<Value> {
        let start = self.stack.len() - n;
        self.stack.split_off(start)
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

/// Ordering comparison for `Lt`/`Gt`/`Le`/`Ge` on int or float operands. Mirrors
/// `interpreter::compare`: NaN and mixed/non-numeric operands behave identically.
fn compare(op: &Op, a: &Value, b: &Value) -> Result<bool, String> {
    use std::cmp::Ordering;
    let ord = match (a, b) {
        (Value::Int(x), Value::Int(y)) => x.partial_cmp(y),
        (Value::Float(x), Value::Float(y)) => x.partial_cmp(y),
        _ => {
            return Err(format!(
                "cannot compare {} and {}",
                a.type_name(),
                b.type_name()
            ))
        }
    };
    Ok(match ord {
        Some(o) => match op {
            Op::Lt => o == Ordering::Less,
            Op::Gt => o == Ordering::Greater,
            Op::Le => o != Ordering::Greater,
            Op::Ge => o != Ordering::Less,
            _ => unreachable!("compare only called with Lt/Gt/Le/Ge"),
        },
        None => false, // NaN compares false
    })
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
        c.emit(Op::Print(1), 1);
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
        c.emit(Op::Print(1), 1);
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
        c.emit(Op::Print(1), 1);
        c.emit(Op::Return, 1);
        assert_eq!(Vm::new(&c).run().unwrap(), "-5\n");
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
        c.emit(Op::Print(1), 1);
        c.emit(Op::Return, 1);
        assert_eq!(Vm::new(&c).run().unwrap(), "true\n");
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
        c.emit(Op::Print(1), 1);
        c.emit(Op::Return, 1);
        assert_eq!(Vm::new(&c).run().unwrap(), "15\n");
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
        c.emit(Op::Print(1), 1); // 3
        let jend = c.code.len();
        c.emit(Op::Jump(0), 1); // 4 (patched below)
        let else_target = c.code.len(); // 5
        c.emit(Op::Const(two), 1); // 5
        c.emit(Op::Print(1), 1); // 6
        let end = c.code.len(); // 7
        c.emit(Op::Return, 1); // 7
        c.code[jif] = Op::JumpIfFalse(else_target);
        c.code[jend] = Op::Jump(end);
        assert_eq!(Vm::new(&c).run().unwrap(), "2\n");
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
        c.emit(Op::Print(1), 1);
        c.emit(Op::Return, 1);
        assert_eq!(Vm::new(&c).run().unwrap(), "x=7\n");
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
        c.emit(Op::Print(1), 1);
        c.emit(Op::GetLocal(0), 1);
        c.emit(Op::Const(one), 1);
        c.emit(Op::Index, 1);
        c.emit(Op::Print(1), 1);
        c.emit(Op::Return, 1);
        assert_eq!(Vm::new(&c).run().unwrap(), "3\n20\n");
    }

    #[test]
    fn print_joins_multiple_args_with_space() {
        let mut c = Chunk::new();
        let a = c.add_const(Value::Str("a".into()));
        let b = c.add_const(Value::Int(1));
        c.emit(Op::Const(a), 1);
        c.emit(Op::Const(b), 1);
        c.emit(Op::Print(2), 1);
        c.emit(Op::Return, 1);
        assert_eq!(Vm::new(&c).run().unwrap(), "a 1\n");
    }
}
