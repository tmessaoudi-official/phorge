//! Cooperative green-thread driver for the tree-walking interpreter (M6 W4 / S4.3 cutover).
//!
//! Native-only (`green` feature, non-wasm): each green task runs *that task's own* [`Interp`] inside a
//! stackful `corosensei` coroutine, all driven by the shared, backend-agnostic
//! [`run_loop`](crate::green::exec::run_loop) over the single-sourced
//! [`Scheduler`](crate::green::sched::Scheduler) — so `run`'s task interleaving is identical to the
//! VM's (`runvm`), the byte-identity spine. `spawn` **defers** (args eval'd eagerly in the spawning
//! task, the resolved function body run as the coroutine's root call — *not* a synthetic lambda, so a
//! fault inside it traces exactly like a direct call; the reverted thunk's lambda frame was what broke
//! that, `b5053a4`); `recv`-on-empty / `join`-on-incomplete suspend via the coroutine yielder until the
//! scheduler wakes the task. wasm keeps the eager model (corosensei has no native stack to switch).
//!
//! Wired into `cmd_run`/`cmd_run_exit` (S4.3 flip): a `uses_concurrency` program routes here, every
//! other program stays on the unchanged synchronous interpreter. `cmd_runvm` routes to the VM twin
//! [`vm::coop`](crate::vm) in the same step, so the byte-identity spine (`run≡runvm`) holds.

use super::*;
use crate::green::coro::{CoroutineTask, TaskCoroutine, TaskYielder, YielderSuspend};
use crate::green::exec::{run_loop, Coop, Suspend, Task};
use crate::green::sched::TaskId;
use corosensei::Coroutine;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

impl<'c> Interp<'c> {
    /// Build a fresh task interpreter over `program`, sharing `coop` with every sibling task and
    /// suspending on `suspend` (this task's coroutine yielder). Mirrors the synchronous constructors
    /// but threads the cooperative handles + the owning program (so this task can itself `spawn`).
    pub(super) fn for_task(
        program: Rc<Program>,
        coop: Rc<RefCell<Coop>>,
        suspend: &'c dyn Suspend,
    ) -> Self {
        let mut interp = Interp {
            funcs: HashMap::new(),
            classes: HashMap::new(),
            class_implements: std::collections::BTreeMap::new(),
            class_tables: crate::native::ClassTables::default(),
            method_origins: std::collections::BTreeMap::new(),
            variants: HashMap::new(),
            statics: HashMap::new(),
            consts: HashMap::new(),
            field_inits: HashMap::new(),
            layouts: HashMap::new(),
            frame: CallScopes::new(),
            this: None,
            cur_class: None,
            parent_parents: std::collections::BTreeMap::new(),
            parent_mro: std::collections::BTreeMap::new(),
            out: String::new(),
            trace_stack: Vec::new(),
            depth: 0,
            pending_throw: None,
            coop,
            coop_suspend: Some(suspend),
            program: Some(program.clone()),
        };
        interp.collect(&program);
        interp
    }

    /// `spawn` on the cooperative path. **Defers** only a single-overload **free-function** call — the
    /// exact form the VM compiler lowers to `Op::SpawnCall` and defers — so the two backends agree on
    /// what runs as a separate task. The args are evaluated **now**, in the spawning task (the new task
    /// interpreter has a fresh scope and cannot see the spawner's locals), and the function body is the
    /// new coroutine's root call (no synthetic lambda → fault traces match a direct call). Every other
    /// operand (method / overloaded / closure / variant call) runs **inline** here, matching the VM
    /// (whose compiler emits `<call>; Op::Spawn`, run inline) — so `run≡runvm`; those forms are
    /// synchronous-degenerate for now (true concurrency for them is a documented follow-up).
    pub(super) fn spawn_cooperative(&mut self, call: &Expr) -> R<Value> {
        // Defer iff the operand is a call to a bare identifier naming a single-overload free function.
        if let Expr::Call { callee, args, .. } = call {
            if let Expr::Ident(name, _) = &**callee {
                if let Some(set) = self.funcs.get(name) {
                    if set.len() == 1 {
                        let f = set[0].clone();
                        let argv = self.eval_args(args)?;
                        if argv.len() == f.params.len() {
                            return self.defer_free_fn_task(f, argv);
                        }
                    }
                }
            }
        }
        // Fallback: run the call inline in this task (synchronous-degenerate), then register it as a
        // finished task — byte-identical to the VM's cooperative `Op::Spawn`.
        let result = self.eval(call)?;
        let id = self.coop.borrow_mut().sched.spawn();
        self.coop.borrow_mut().results.insert(id, result);
        Ok(Value::Task(id))
    }

    /// Queue a single-overload free function (with already-evaluated args) as a new scheduler task whose
    /// coroutine root is the function body itself, and return its `Task` handle.
    fn defer_free_fn_task(&mut self, f: FunctionDecl, argv: Vec<Value>) -> R<Value> {
        let id = self.coop.borrow_mut().sched.spawn();
        let program = self
            .program
            .clone()
            .expect("a cooperative task interpreter holds its program");
        let coop = self.coop.clone();
        let coro: TaskCoroutine = Coroutine::new(move |yielder: &TaskYielder, ()| {
            let ys = YielderSuspend::new(yielder);
            let mut task = Interp::for_task(program, coop, &ys);
            let names: Vec<String> = f.params.iter().map(|p| p.name.clone()).collect();
            let result = run_task_call(&mut task, &f.name, &names, &f.body, argv);
            (result, std::mem::take(&mut task.out))
        });
        self.coop
            .borrow_mut()
            .spawned
            .push((id, Box::new(CoroutineTask::new(coro))));
        Ok(Value::Task(id))
    }
}

/// Run a task's root call, flattening the interpreter's `Signal` control flow into the
/// `Result<Value, String>` the coroutine protocol carries (a fault body; the driver aborts the
/// program with it). Used for `main` (task 0) and every `spawn`ed task alike.
fn run_task_call(
    task: &mut Interp<'_>,
    fn_name: &str,
    names: &[String],
    body: &[Stmt],
    args: Vec<Value>,
) -> Result<Value, String> {
    match task.run_call(fn_name, names, body, args, None, None) {
        Ok(v) => Ok(v),
        Err(Signal::Return(v)) => Ok(v),
        Err(Signal::Runtime(d)) => Err(d.message),
        Err(Signal::Throw(v)) => Err(format!("uncaught exception `{}`", throw_what(&v))),
        Err(Signal::Break | Signal::Continue) => {
            Err("internal error: loop control escaped a task".to_string())
        }
    }
}

/// Cooperative interpreter entry point (S4.3): run a `uses_concurrency` program with real task
/// interleaving. Seeds task 0 = `main` as a coroutine, then drives [`run_loop`]. Returns the merged
/// output + `main`'s exit code, or a runtime `Diagnostic` (a task fault / deadlock). The synchronous
/// [`interpret_main`](super::interpret_main) still serves every non-concurrent program, byte-identical.
pub fn run_cooperative_interp(program: &Program) -> Result<(String, i64), Diagnostic> {
    let prog = Rc::new(program.clone());
    let coop = Rc::new(RefCell::new(Coop::new()));
    let t0 = coop.borrow_mut().sched.spawn(); // TaskId(0) — the entry/main task

    // Resolve `main` (top-level or class-static) exactly like the synchronous entry.
    let (entry_class, main) = match crate::ast::entry_point(program, "main") {
        Some(e) => e,
        None => {
            return Err(Diagnostic::runtime(
                "no entry point: running needs a `main` function",
            ))
        }
    };
    let names: Vec<String> = main.params.iter().map(|p| p.name.clone()).collect();
    let args = if names.is_empty() {
        vec![]
    } else {
        vec![crate::native::process_args_value()]
    };
    let call_name = match entry_class {
        Some(c) => format!("{c}::main"),
        None => "main".to_string(),
    };
    let body = main.body.clone();

    let prog0 = prog.clone();
    let coop0 = coop.clone();
    let coro: TaskCoroutine = Coroutine::new(move |yielder: &TaskYielder, ()| {
        let ys = YielderSuspend::new(yielder);
        let mut task = Interp::for_task(prog0.clone(), coop0, &ys);
        // Non-literal static initializers run once, before main (mirrors `interpret_main`).
        if let Err(sig) = task.eval_static_inits(&prog0) {
            let msg = match sig {
                Signal::Runtime(d) => d.message,
                Signal::Throw(v) => format!("uncaught exception `{}`", throw_what(&v)),
                _ => "internal error: control escaped a static initializer".to_string(),
            };
            return (Err(msg), std::mem::take(&mut task.out));
        }
        let result = run_task_call(&mut task, &call_name, &names, &body, args);
        (result, std::mem::take(&mut task.out))
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
    use super::run_cooperative_interp;

    /// Parse + check + alias/generics-expand a source program to the backend-ready AST, then run it on
    /// the cooperative interpreter. Mirrors the front-end the CLI runs before a backend.
    fn coop_run(src: &str) -> Result<String, String> {
        let program = crate::cli::parse_checked_program(src)?;
        run_cooperative_interp(&program)
            .map(|(out, _exit)| out)
            .map_err(|d| d.message)
    }

    /// THE LITMUS (S4.3 acceptance): a `recv`-ing consumer is **spawned**, so under the eager model it
    /// would run at `spawn` and fault `recv from empty channel`. Under the cooperative driver the call
    /// is deferred — `main` sends first, then the consumer runs and finds the value — so the program
    /// succeeds. This is exactly the plan's `spawn consume(ch); send(42)` litmus; passing here proves
    /// `spawn` truly defers on the interpreter (the VM half + the run≡runvm flip are the next step).
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
        assert_eq!(coop_run(src).unwrap(), "got 42\ndone 42\n");
    }

    /// Genuine suspend/resume: `main` itself `recv`s on an empty channel (the producer is spawned and
    /// has not run), so it must BLOCK, yield to the spawned producer, be woken by the producer's
    /// `send`, and resume — all without deadlocking. Proves deep-stack coroutine suspension on the
    /// interpreter works without `unsafe` (the `green::spike` shape, now in the real engine).
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
        assert_eq!(coop_run(src).unwrap(), "recv 99\ndone 1\n");
    }

    /// The existing synchronous-degenerate `concurrency.phg` surface (producer fills before consumer
    /// drains, `spawn`+`join`) must produce the same output through the cooperative driver — the
    /// guarantee that flipping the entry point will not change non-blocking programs.
    #[test]
    fn fork_join_and_buffered_channels_match_eager_output() {
        let src = r#"
package Main;
import Core.Console;

function square(int n): int { return n * n; }

function main(): void {
    Task<int> t = spawn square(9);
    Console.println("9 squared = {t.join()}");
    Channel<string> words = Channel.create();
    words.send("hello");
    words.send("world");
    Console.println("{words.recv()} {words.recv()}");
}
"#;
        assert_eq!(coop_run(src).unwrap(), "9 squared = 81\nhello world\n");
    }
}
