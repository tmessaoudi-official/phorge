//! Cooperative green-thread driver for the bytecode VM (M6 W4 / S4.3 cutover) — the `runvm` twin of
//! [`interpreter::coop`](crate::interpreter). Native + `green` only (stackful coroutines don't compile
//! on wasm32).
//!
//! Each green task runs *its own* `Vm` inside a `corosensei` coroutine, driven by the SAME
//! [`run_loop`](crate::green::exec::run_loop) over the SAME [`Scheduler`](crate::green::sched) the
//! interpreter uses — so the two backends pick the same task at every step ⇒ byte-identical task
//! interleaving (`run≡runvm`). A task-VM borrows a captured `Rc<BytecodeProgram>` and holds a clone of
//! that `Rc` ([`Vm::program_rc`]) so a `SpawnCall` can build a child task-VM coroutine. `spawn` defers
//! (the function body is the coroutine root — no lambda); `recv`/`join` suspend via the yielder.
//!
//! Wired into `cmd_runvm`/`cmd_runvm_exit` (S4.3 flip): a `uses_concurrency` program routes here, in
//! the same step `cmd_run` routes to the interpreter twin — so the byte-identity spine (`run≡runvm`)
//! holds. Every non-concurrent program stays on the unchanged synchronous [`run_main`](Vm::run_main).

use super::*;
use crate::green::coro::{CoroutineTask, TaskCoroutine, TaskYielder, YielderSuspend};
use crate::green::exec::{run_loop, Coop, Task};
use crate::green::sched::TaskId;
use corosensei::Coroutine;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

impl<'a> Vm<'a> {
    /// Defer a `SpawnCall` as a new scheduler task (cooperative path): allocate a task id, build a child
    /// task-VM coroutine rooted at `func_idx` with `args`, queue it, and return the id. The coroutine
    /// captures a `'static` `Rc` of the program (so it outlives the borrow) + the shared `coop`.
    pub(super) fn spawn_call_cooperative(&mut self, func_idx: usize, args: Vec<Value>) -> TaskId {
        let id = self.coop.borrow_mut().sched.spawn();
        let prog = self
            .program_rc
            .clone()
            .expect("a cooperative task-VM holds its program");
        let coop = self.coop.clone();
        let coro: TaskCoroutine = Coroutine::new(move |yielder: &TaskYielder, ()| {
            let ys = YielderSuspend::new(yielder);
            let mut task = Vm::new_cooperative(&prog, coop, &ys);
            task.program_rc = Some(prog.clone());
            let result = task.run_task_function(func_idx, args);
            (result, std::mem::take(&mut task.out))
        });
        self.coop
            .borrow_mut()
            .spawned
            .push((id, Box::new(CoroutineTask::new(coro))));
        id
    }

    /// Run a spawned free function (by table index) as this task-VM's root frame, returning its value.
    /// The function's `Op::Return` at frame depth 1 stashes the value in `exit_value` (see `do_return`),
    /// which `run_until(0)` leaves set; a fall-off-end (`void`) task yields `Unit`.
    fn run_task_function(&mut self, func_idx: usize, args: Vec<Value>) -> Result<Value, String> {
        let slot_base = self.stack.len();
        self.stack.extend(args);
        self.frames.push(Frame {
            func: func_idx,
            ip: 0,
            slot_base,
        });
        self.run_until(0)?;
        Ok(std::mem::replace(&mut self.exit_value, Value::Unit))
    }

    /// Run `main` as task 0's root, mirroring [`run_main`](Vm::run_main)'s entry-slot setup (static-main
    /// dummy receiver + argv param) but capturing the return `Value` (the scheduler reads its exit code).
    fn run_main_task(&mut self) -> Result<Value, String> {
        if self.program.main_is_static {
            self.stack.push(Value::Unit);
        }
        if self.program.main_params == 1 {
            self.stack.push(crate::native::process_args_value());
        }
        self.frames.push(Frame {
            func: self.program.main,
            ip: 0,
            slot_base: 0,
        });
        self.run_until(0)?;
        Ok(std::mem::replace(&mut self.exit_value, Value::Unit))
    }
}

/// Cooperative VM entry point (S4.3): run a `uses_concurrency` program with real task interleaving —
/// the `runvm` twin of [`run_cooperative_interp`](crate::interpreter::run_cooperative_interp). Seeds
/// task 0 = `main` as a coroutine, then drives [`run_loop`]. Returns merged output + `main`'s exit code,
/// or a runtime `Diagnostic` (task fault / deadlock). The synchronous [`run_main`](Vm::run_main) still
/// serves every non-concurrent program, byte-identical.
pub fn run_cooperative_vm(program: &BytecodeProgram) -> Result<(String, i64), Diagnostic> {
    program.validate().map_err(Diagnostic::runtime)?;
    let prog = Rc::new(program.clone());
    let coop = Rc::new(RefCell::new(Coop::new()));
    let t0 = coop.borrow_mut().sched.spawn(); // TaskId(0) — the entry/main task

    let prog0 = prog.clone();
    let coop0 = coop.clone();
    let coro: TaskCoroutine = Coroutine::new(move |yielder: &TaskYielder, ()| {
        let ys = YielderSuspend::new(yielder);
        let mut vm = Vm::new_cooperative(&prog0, coop0, &ys);
        vm.program_rc = Some(prog0.clone());
        let result = vm.run_main_task();
        (result, std::mem::take(&mut vm.out))
    });

    let mut tasks: HashMap<TaskId, Box<dyn Task>> = HashMap::new();
    tasks.insert(t0, Box::new(CoroutineTask::new(coro)));
    match run_loop(&coop, &mut tasks) {
        Ok(out) => {
            let exit = coop.borrow().results.get(&t0).map_or(0, exit_code_of);
            Ok((out, exit))
        }
        Err(msg) => Err(Diagnostic::runtime(msg)),
    }
}

#[cfg(test)]
mod tests {
    use super::run_cooperative_vm;

    /// Front-end (parse→check→expand→compile) then run on the cooperative VM. Mirrors `cmd_runvm`'s
    /// pipeline but routes the compiled program to the cooperative driver.
    fn coop_runvm(src: &str) -> Result<String, String> {
        let prog = crate::cli::parse_checked_program(src)?;
        let program = crate::compiler::compile(&prog).map_err(|d| d.to_string())?;
        run_cooperative_vm(&program)
            .map(|(out, _exit)| out)
            .map_err(|d| d.message)
    }

    /// THE LITMUS on the VM — must match `interpreter::coop`'s identical test byte-for-byte: a spawned
    /// `recv`-er would fault under the eager model; deferring lets `main` send first, so it succeeds.
    #[test]
    fn litmus_spawned_recver_succeeds_only_when_deferred() {
        let src = r#"
package Main;
import Core.Console;

function consume(Channel<int> ch): int {
    int v = ch.recv();
    Console.println("got {v}");
    return v;
}

function main(): void {
    Channel<int> ch = Channel.create();
    Task<int> t = spawn consume(ch);
    ch.send(42);
    int got = t.join();
    Console.println("done {got}");
}
"#;
        assert_eq!(coop_runvm(src).unwrap(), "got 42\ndone 42\n");
    }

    /// Genuine suspend/resume on the VM: `main` recvs on an empty channel (producer spawned, not yet
    /// run), blocks, is woken by the producer's `send`, resumes — no deadlock.
    #[test]
    fn main_recv_blocks_until_spawned_producer_sends() {
        let src = r#"
package Main;
import Core.Console;

function produce(Channel<int> ch): int {
    ch.send(99);
    return 1;
}

function main(): void {
    Channel<int> ch = Channel.create();
    Task<int> p = spawn produce(ch);
    int v = ch.recv();
    Console.println("recv {v}");
    int r = p.join();
    Console.println("done {r}");
}
"#;
        assert_eq!(coop_runvm(src).unwrap(), "recv 99\ndone 1\n");
    }
}
