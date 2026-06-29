//! The cooperative green-thread **executor loop** (M6 W4 / S4.3 step 3b) — engine-agnostic.
//!
//! [`sched::Scheduler`](super::sched::Scheduler) decides *which* task runs next; this module is the
//! loop that actually *drives* tasks and threads their output + results together. It is parameterized
//! over a [`Task`] trait so it can be unit-tested with a mock executor here, then driven by the real
//! per-backend executors (interpreter coroutine / VM frame-swap) without changing this coordination
//! logic — the same "single-sourced, backend-agnostic" discipline as the scheduler kernel.
//!
//! **Output ordering (the key invariant).** A task produces output as it runs, but only becomes
//! visible to the loop when it *suspends* (or completes). Between two suspension points a task runs
//! contiguously, so the output it produced in that slice is one contiguous fragment. Each `resume`
//! returns that fragment ([`Step`]); the loop appends fragments in resume order, which reconstructs
//! the correct interleaved output **without** any shared output buffer — so the per-task engines keep
//! their own `out: String` unchanged. This is what makes `run≡runvm` byte-identical: both backends
//! drive this same loop with the same scheduler, so fragments are appended in the same order.
//!
//! **Determinism.** The loop takes the next ready task from the FIFO scheduler, drains newly-spawned
//! tasks in spawn order, and appends output in resume order — no wall-clock, no nondeterminism.

use super::sched::{Scheduler, TaskId, Trap};
use crate::value::Value;

/// What a single [`Task::resume`] produced: the reason it handed control back, plus the output
/// fragment accumulated since the previous resume (possibly empty).
pub enum Step {
    /// The task suspended with this trap; the fragment is its output since the last resume.
    Trapped(Trap, String),
    /// The task finished, yielding its result value and final output fragment. A faulting task
    /// reports `Err` (the loop aborts the whole program with it — a fault is not recoverable here).
    Finished(Result<Value, String>, String),
}

/// The suspension capability an engine uses to hand control back to the scheduler from deep in its
/// own call stack (a `recv` on an empty channel / a `join` on an incomplete task / an explicit yield).
/// Feature-agnostic on purpose: the native executor implements it over a `corosensei` yielder
/// (`green::coro`), while the type stays free of any coroutine-crate dependency so the engines that
/// hold a `&dyn Suspend` compile on every target (the wasm build supplies a frame-swap implementor).
/// `suspend` takes the engine's output fragment accumulated since the last resume (see the module-level
/// output-ordering note) and returns once the scheduler resumes this task.
pub trait Suspend {
    fn suspend(&self, trap: Trap, out_fragment: String);
}

/// The cooperative-execution context an engine borrows while running a task: the shared [`Coop`] (to
/// allocate channel/task ids, queue spawns, wake receivers via `on_send`, and read join results) and
/// the [`Suspend`] handle (to block at `recv`/`join`). Bundled behind one lifetime so an engine gains a
/// single `Option<CoopCtx>` field — `Some` while running cooperatively, `None` on the ordinary
/// (non-concurrent) run path where the concurrency ops never execute.
pub struct CoopCtx<'ctx> {
    pub coop: &'ctx std::cell::RefCell<Coop>,
    pub suspend: &'ctx dyn Suspend,
}

/// One green task the loop can drive. Implemented by the per-backend executors (a coroutine-hosted
/// interpreter / VM on native; a VM frame-swap on wasm) and by the mock in this module's tests.
///
/// `resume` runs the task from where it last suspended until it next traps or finishes. The executor
/// owns the suspension mechanism (coroutine yield / frame-swap); the loop only sequences resumes.
/// Shared state is handed in as a `&RefCell<Coop>` **parameter** (not stored), so a task never borrows
/// `Coop` in its own type — that is what keeps task registration free of a self-referential borrow
/// (the loop owns both `Coop` and the task map; a task spawned mid-run is queued in `Coop.spawned`
/// and the loop moves it into the map). `send`/`spawn` mutate `coop` through this borrow; `recv`/`join`
/// inspect it to decide whether to return a value or trap.
pub trait Task {
    fn resume(&mut self, coop: &std::cell::RefCell<Coop>) -> Step;
}

/// Shared coordination state the engines mutate through a `&RefCell<Coop>` while the loop owns it.
/// Holds only what crosses task boundaries: the scheduler, finished-task results (for `join`), and the
/// queue of tasks spawned during a resume (drained by the loop after each step, preserving spawn
/// order). Channel *buffers* are NOT here — a channel is a shared `Rc<RefCell<VecDeque>>` carried by
/// its `Value::Channel`, so `send`/`recv` mutate it directly; `Coop` only tracks who is *blocked*.
pub struct Coop {
    pub sched: Scheduler,
    pub results: std::collections::HashMap<TaskId, Value>,
    /// Tasks spawned during the current resume, awaiting registration by the loop (spawn order).
    pub spawned: Vec<(TaskId, Box<dyn Task>)>,
}

impl Default for Coop {
    fn default() -> Self {
        Self {
            sched: Scheduler::new(),
            results: std::collections::HashMap::new(),
            spawned: Vec::new(),
        }
    }
}

impl Coop {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

/// Drive every task to completion, returning the merged output (or the first fault). `coop` and the
/// `tasks` map start seeded with the entry task (`TaskId(0)` = `main`); the loop registers any task
/// spawned mid-run from `coop.spawned`. Terminates when no task is ready: a clean finish if nothing is
/// blocked, else a deadlock fault (every task waiting on a `recv`/`join` that can never arrive).
///
/// # Errors
/// Returns the fault string if any task finishes with `Err`, or `"deadlock: all tasks are blocked"`
/// if no task can make progress.
pub fn run_loop(
    coop: &std::cell::RefCell<Coop>,
    tasks: &mut std::collections::HashMap<TaskId, Box<dyn Task>>,
) -> Result<String, String> {
    let mut out = String::new();
    loop {
        // Register any tasks spawned during the previous resume, in spawn order (the scheduler already
        // enqueued their ids ready when `spawn()` was called).
        let spawned: Vec<_> = std::mem::take(&mut coop.borrow_mut().spawned);
        for (id, task) in spawned {
            tasks.insert(id, task);
        }

        let Some(task_id) = coop.borrow_mut().sched.next_ready() else {
            // Nothing ready: either everything finished (clean) or everyone is blocked (deadlock).
            if coop.borrow().sched.is_deadlocked() {
                return Err("deadlock: all tasks are blocked".to_string());
            }
            return Ok(out);
        };

        // Pull the task out of the map while it runs, so its `resume` can borrow `coop` (which may
        // register newly-spawned tasks into the map) without aliasing the map itself. Re-insert it
        // unless it finished.
        let mut task = tasks
            .remove(&task_id)
            .expect("a ready task id always has a registered executor");
        let step = task.resume(coop);
        match step {
            Step::Trapped(trap, frag) => {
                out.push_str(&frag);
                coop.borrow_mut().sched.on_trap(task_id, trap);
                tasks.insert(task_id, task); // still live — put it back
            }
            Step::Finished(result, frag) => {
                out.push_str(&frag);
                let value = result?; // a fault aborts the whole program (task already removed)
                coop.borrow_mut().results.insert(task_id, value);
                coop.borrow_mut().sched.on_trap(task_id, Trap::Done);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::sched::ChanId;
    use super::*;

    /// A scripted mock task: a queue of steps it returns on successive resumes. Lets the loop's
    /// sequencing/output-ordering/wake/deadlock logic be tested without any real engine.
    struct MockTask {
        steps: std::collections::VecDeque<Step>,
    }
    impl MockTask {
        fn new(steps: Vec<Step>) -> Self {
            Self {
                steps: steps.into(),
            }
        }
    }
    impl Task for MockTask {
        fn resume(&mut self, _coop: &std::cell::RefCell<Coop>) -> Step {
            self.steps
                .pop_front()
                .expect("mock task resumed more times than scripted")
        }
    }

    fn seed(coop: &std::cell::RefCell<Coop>) -> TaskId {
        coop.borrow_mut().sched.spawn() // TaskId(0) — the entry task
    }

    #[test]
    fn single_task_output_passes_through() {
        let coop = std::cell::RefCell::new(Coop::new());
        let t0 = seed(&coop);
        let mut tasks: std::collections::HashMap<TaskId, Box<dyn Task>> =
            std::collections::HashMap::new();
        tasks.insert(
            t0,
            Box::new(MockTask::new(vec![Step::Finished(
                Ok(Value::Int(1)),
                "hello".into(),
            )])),
        );
        assert_eq!(run_loop(&coop, &mut tasks).unwrap(), "hello");
    }

    #[test]
    fn two_tasks_interleave_output_in_resume_order() {
        // main prints "A", yields; worker prints "B", done; main resumes, prints "C", done.
        // The scheduler is FIFO: after main yields it goes behind worker. Expect "ABC".
        let coop = std::cell::RefCell::new(Coop::new());
        let main = seed(&coop);
        let worker = coop.borrow_mut().sched.spawn(); // TaskId(1), enqueued ready behind main
        let mut tasks: std::collections::HashMap<TaskId, Box<dyn Task>> =
            std::collections::HashMap::new();
        tasks.insert(
            main,
            Box::new(MockTask::new(vec![
                Step::Trapped(Trap::Yield, "A".into()),
                Step::Finished(Ok(Value::Unit), "C".into()),
            ])),
        );
        tasks.insert(
            worker,
            Box::new(MockTask::new(vec![Step::Finished(
                Ok(Value::Unit),
                "B".into(),
            )])),
        );
        assert_eq!(run_loop(&coop, &mut tasks).unwrap(), "ABC");
    }

    #[test]
    fn recv_blocks_then_a_sender_wakes_it() {
        // consumer recvs (blocks), producer "sends" (wakes consumer via on_send) then finishes,
        // consumer resumes and finishes. Output "PC" (producer slice, then woken consumer).
        let coop = std::cell::RefCell::new(Coop::new());
        let consumer = seed(&coop);
        let producer = coop.borrow_mut().sched.spawn();
        let ch = coop.borrow_mut().sched.new_channel();
        let mut tasks: std::collections::HashMap<TaskId, Box<dyn Task>> =
            std::collections::HashMap::new();
        tasks.insert(
            consumer,
            Box::new(MockTask::new(vec![
                Step::Trapped(Trap::Recv(ch), String::new()), // blocks on empty channel
                Step::Finished(Ok(Value::Unit), "C".into()),  // resumes after the send
            ])),
        );
        // The producer's "send" wakes the consumer through the scheduler — modeled here by the mock
        // calling on_send during its slice via the `coop` parameter (the real `send` op does exactly
        // this). Note it does NOT store `coop`, so the task stays free of a self-referential borrow.
        struct Producer {
            ch: ChanId,
        }
        impl Task for Producer {
            fn resume(&mut self, coop: &std::cell::RefCell<Coop>) -> Step {
                coop.borrow_mut().sched.on_send(self.ch); // wake the first blocked receiver
                Step::Finished(Ok(Value::Unit), "P".into())
            }
        }
        tasks.insert(producer, Box::new(Producer { ch }));
        assert_eq!(run_loop(&coop, &mut tasks).unwrap(), "PC");
    }

    #[test]
    fn all_blocked_is_a_deadlock() {
        let coop = std::cell::RefCell::new(Coop::new());
        let t0 = seed(&coop);
        let ch = coop.borrow_mut().sched.new_channel();
        let mut tasks: std::collections::HashMap<TaskId, Box<dyn Task>> =
            std::collections::HashMap::new();
        tasks.insert(
            t0,
            Box::new(MockTask::new(vec![Step::Trapped(
                Trap::Recv(ch),
                String::new(),
            )])),
        );
        assert!(run_loop(&coop, &mut tasks)
            .unwrap_err()
            .contains("deadlock"));
    }

    #[test]
    fn a_faulting_task_aborts_the_program() {
        let coop = std::cell::RefCell::new(Coop::new());
        let t0 = seed(&coop);
        let mut tasks: std::collections::HashMap<TaskId, Box<dyn Task>> =
            std::collections::HashMap::new();
        tasks.insert(
            t0,
            Box::new(MockTask::new(vec![Step::Finished(
                Err("boom".into()),
                "partial".into(),
            )])),
        );
        assert_eq!(run_loop(&coop, &mut tasks).unwrap_err(), "boom");
    }
}
