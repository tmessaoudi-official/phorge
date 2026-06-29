//! Native coroutine bridge (M6 W4 / S4.3 step 3b-2a) — the reusable glue between a stackful
//! `corosensei` coroutine and the engine-agnostic [`run_loop`](super::exec::run_loop).
//!
//! Compiled only with the `green` feature on a non-wasm target (corosensei has no native stack to
//! switch on wasm32 — verified). The wasm playground drives tasks with the VM's frame-swap instead;
//! this module is the *native* executor's lower half. It supplies two pieces the engines reuse:
//! - [`YielderSuspend`] — implements [`Suspend`](super::exec::Suspend) over a corosensei yielder, so an
//!   engine deep in its own call stack can park the whole native stack and hand `(trap, out_fragment)`
//!   back to the scheduler loop (proven possible without `unsafe` in our crate by the 3a spike).
//! - [`CoroutineTask`] — wraps a coroutine as a [`Task`](super::exec::Task) the loop can drive,
//!   translating the coroutine's `Yield`/`Return` into the loop's [`Step`](super::exec::Step).

use super::exec::{Coop, Step, Suspend, Task};
use super::sched::Trap;
use crate::value::Value;
use corosensei::{Coroutine, CoroutineResult, Yielder};

/// The yield/return protocol every green-task coroutine speaks: it `suspend`s `(trap, fragment)` at
/// each blocking point and finally returns `(result, final_fragment)`. `Input` is `()` — a task is
/// resumed with no value (an engine coroutine captures the shared `Rc<RefCell<Coop>>`, not passed
/// per-resume). A plain (`'static`) [`Coroutine`] is used, not `ScopedCoroutine`: only `Coroutine`
/// exposes a direct `resume(&mut self)` for the external scheduler loop (the scoped variant resumes
/// only inside a consuming `scope()` closure). Its `'static` closure forces shared state to be `Rc`
/// rather than a borrow — which *removes* the self-referential-borrow hazard, and stays leak-free
/// because the loop owns the task map (so `Coop` never holds the coroutines that hold its `Rc`).
pub type TaskCoroutine = Coroutine<(), (Trap, String), (Result<Value, String>, String)>;

/// The corosensei yielder type matching [`TaskCoroutine`]'s protocol — handed to a coroutine body so it
/// can build a [`YielderSuspend`].
pub type TaskYielder = Yielder<(), (Trap, String)>;

/// A [`Suspend`] backed by a corosensei yielder. `suspend` parks the running task's native stack and
/// yields `(trap, fragment)` to the scheduler loop; it returns when the loop resumes this task.
pub struct YielderSuspend<'y> {
    yielder: &'y TaskYielder,
}

impl<'y> YielderSuspend<'y> {
    #[must_use]
    pub fn new(yielder: &'y TaskYielder) -> Self {
        Self { yielder }
    }
}

impl Suspend for YielderSuspend<'_> {
    fn suspend(&self, trap: Trap, out_fragment: String) {
        self.yielder.suspend((trap, out_fragment));
    }
}

/// Wraps a [`TaskCoroutine`] as a scheduler [`Task`]. The reusable bridge: a coroutine-hosted engine
/// becomes drivable by [`run_loop`](super::exec::run_loop) with no engine-specific loop code.
pub struct CoroutineTask {
    coro: TaskCoroutine,
}

impl CoroutineTask {
    #[must_use]
    pub fn new(coro: TaskCoroutine) -> Self {
        Self { coro }
    }
}

impl Task for CoroutineTask {
    fn resume(&mut self, _coop: &std::cell::RefCell<Coop>) -> Step {
        // The coroutine captured `&Coop` at creation, so the loop's `coop` argument is unused here —
        // resuming with `()` continues the task until its next `suspend`/return.
        match self.coro.resume(()) {
            CoroutineResult::Yield((trap, frag)) => Step::Trapped(trap, frag),
            CoroutineResult::Return((result, frag)) => Step::Finished(result, frag),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::exec::run_loop;
    use super::super::sched::TaskId;
    use super::*;

    /// End-to-end seam test: a real corosensei coroutine, wrapped as a `CoroutineTask`, driven by the
    /// engine-agnostic `run_loop` with genuine stack-parking suspension — the last unproven join
    /// between the 3a spike (raw coroutine) and the 3b-1 loop (mock tasks).
    #[test]
    fn coroutine_task_drives_through_run_loop_with_real_suspension() {
        let coop = std::cell::RefCell::new(Coop::new());
        let t0 = coop.borrow_mut().sched.spawn(); // TaskId(0)

        // A coroutine that prints "A", yields; prints "B", yields; prints "C", finishes. Suspension
        // happens via the same `Suspend` trait an engine will use — proving the abstraction carries it.
        let coro: TaskCoroutine = Coroutine::new(|yielder: &TaskYielder, ()| {
            let s = YielderSuspend::new(yielder);
            s.suspend(Trap::Yield, "A".to_string());
            s.suspend(Trap::Yield, "B".to_string());
            (Ok(Value::Int(0)), "C".to_string())
        });

        let mut tasks: std::collections::HashMap<TaskId, Box<dyn Task>> =
            std::collections::HashMap::new();
        tasks.insert(t0, Box::new(CoroutineTask::new(coro)));
        assert_eq!(run_loop(&coop, &mut tasks).unwrap(), "ABC");
    }

    /// A coroutine that faults aborts the program through the loop (its partial output still flushes).
    #[test]
    fn coroutine_task_fault_propagates_through_the_loop() {
        let coop = std::cell::RefCell::new(Coop::new());
        let t0 = coop.borrow_mut().sched.spawn();
        let coro: TaskCoroutine = Coroutine::new(|yielder: &TaskYielder, ()| {
            let s = YielderSuspend::new(yielder);
            s.suspend(Trap::Yield, "partial".to_string());
            (Err("boom".to_string()), String::new())
        });
        let mut tasks: std::collections::HashMap<TaskId, Box<dyn Task>> =
            std::collections::HashMap::new();
        tasks.insert(t0, Box::new(CoroutineTask::new(coro)));
        assert_eq!(run_loop(&coop, &mut tasks).unwrap_err(), "boom");
    }
}
