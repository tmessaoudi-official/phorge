//! The cooperative green-thread scheduler kernel (M6 W4 / S4.3) — **single-sourced**, backend-agnostic.
//!
//! It owns only *scheduling decisions* and deals in opaque [`TaskId`]/[`ChanId`]; it never executes
//! Phorj code. A backend drives it: pop the next ready task ([`Scheduler::next_ready`]), resume that
//! task's executor (coroutine on native, VM frame on wasm) until it **traps**, then report the trap
//! ([`Scheduler::on_trap`]); a channel `send` wakes a blocked receiver ([`Scheduler::on_send`]).
//!
//! Every decision is **deterministic**: the ready queue is FIFO, per-channel receive wait-lists are
//! FIFO, and a completed task wakes its joiners in registration order. Because both backends call this
//! same kernel, they pick the same task at every step ⇒ identical interleaving ⇒ byte-identical output
//! (the `run≡runvm` spine). No `unsafe`, no backend types — pure data + logic, unit-tested in isolation.

use std::collections::{HashMap, HashSet, VecDeque};

/// Opaque handle for a green task. `TaskId(0)` is the entry (`main`) task by convention.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct TaskId(pub usize);

/// Opaque handle for a channel.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ChanId(pub usize);

/// Why a running task handed control back to the scheduler.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Trap {
    /// The task finished (the backend stores its result, keyed by `TaskId`).
    Done,
    /// The task tried to `recv` from an empty channel and must block until a value is sent.
    Recv(ChanId),
    /// The task is `join`ing another task that has not completed yet.
    Join(TaskId),
    /// A voluntary cooperative yield — the task stays ready and is re-queued.
    Yield,
}

/// The deterministic cooperative scheduler. See module docs.
#[derive(Debug, Default)]
pub struct Scheduler {
    /// Ready-to-run tasks, FIFO (round-robin fairness, deterministic order).
    ready: VecDeque<TaskId>,
    /// Tasks blocked in `recv`, per channel, FIFO (first blocked is first woken on a `send`).
    waiting_recv: HashMap<ChanId, VecDeque<TaskId>>,
    /// Tasks blocked in `join` on a key task, in registration order (woken together when it completes).
    waiting_join: HashMap<TaskId, Vec<TaskId>>,
    /// Completed tasks (so a `join` on an already-done task is immediately ready).
    done: HashSet<TaskId>,
    next_task: usize,
    next_chan: usize,
}

impl Scheduler {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a new task and enqueue it ready. The first call returns `TaskId(0)` — the convention for
    /// the entry/`main` task; each `spawn` thereafter gets the next id. The backend maps the returned id
    /// to that task's executor state (coroutine / VM frame stack).
    pub fn spawn(&mut self) -> TaskId {
        let id = TaskId(self.next_task);
        self.next_task += 1;
        self.ready.push_back(id);
        id
    }

    /// Allocate a fresh channel handle. (The backend owns the channel's value buffer; the kernel only
    /// tracks who is *blocked* on it.)
    pub fn new_channel(&mut self) -> ChanId {
        let id = ChanId(self.next_chan);
        self.next_chan += 1;
        id
    }

    /// Pop the next ready task to run (FIFO). `None` means no task is ready — combine with
    /// [`Self::is_deadlocked`] to distinguish "all done" from "everyone blocked" (a deadlock).
    pub fn next_ready(&mut self) -> Option<TaskId> {
        self.ready.pop_front()
    }

    /// Record the trap a just-run task reported, updating the queues + waking any unblocked tasks.
    pub fn on_trap(&mut self, task: TaskId, trap: Trap) {
        match trap {
            Trap::Yield => self.ready.push_back(task),
            Trap::Recv(chan) => self.waiting_recv.entry(chan).or_default().push_back(task),
            Trap::Join(target) => {
                if self.done.contains(&target) {
                    self.ready.push_back(task); // already finished → resume immediately
                } else {
                    self.waiting_join.entry(target).or_default().push(task);
                }
            }
            Trap::Done => {
                self.done.insert(task);
                // Wake every task joining this one, in registration order (deterministic).
                if let Some(joiners) = self.waiting_join.remove(&task) {
                    self.ready.extend(joiners);
                }
            }
        }
    }

    /// A value was sent on `chan`: wake the **first** task blocked receiving on it (FIFO fairness), if
    /// any. A no-op when no task is waiting (the value sits in the backend's channel buffer for a later
    /// `recv`). Returns the woken task, if one was.
    pub fn on_send(&mut self, chan: ChanId) -> Option<TaskId> {
        let woken = self
            .waiting_recv
            .get_mut(&chan)
            .and_then(VecDeque::pop_front);
        if let Some(t) = woken {
            self.ready.push_back(t);
        }
        woken
    }

    /// Whether `task` has completed.
    #[must_use]
    pub fn is_done(&self, task: TaskId) -> bool {
        self.done.contains(&task)
    }

    /// `true` when no task is ready yet tasks remain blocked (on `recv`/`join`) — a deadlock the backend
    /// must report as a clean fault rather than hang. Distinguishes a genuine all-blocked state from the
    /// normal "all tasks done" end (where nothing is ready *and* nothing is blocked).
    #[must_use]
    pub fn is_deadlocked(&self) -> bool {
        self.ready.is_empty() && self.has_blocked()
    }

    /// Whether any task is currently blocked (waiting on a `recv` or a `join`).
    #[must_use]
    pub fn has_blocked(&self) -> bool {
        self.waiting_recv.values().any(|q| !q.is_empty())
            || self.waiting_join.values().any(|v| !v.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_assigns_ids_in_order_and_enqueues_ready() {
        let mut s = Scheduler::new();
        assert_eq!(s.spawn(), TaskId(0)); // entry/main convention
        assert_eq!(s.spawn(), TaskId(1));
        assert_eq!(s.next_ready(), Some(TaskId(0))); // FIFO
        assert_eq!(s.next_ready(), Some(TaskId(1)));
        assert_eq!(s.next_ready(), None);
    }

    #[test]
    fn yield_requeues_task_at_the_back() {
        let mut s = Scheduler::new();
        let a = s.spawn();
        let b = s.spawn();
        assert_eq!(s.next_ready(), Some(a));
        s.on_trap(a, Trap::Yield); // a yields → back of the queue, behind b
        assert_eq!(s.next_ready(), Some(b));
        assert_eq!(s.next_ready(), Some(a));
    }

    #[test]
    fn recv_blocks_until_a_send_wakes_the_first_waiter_fifo() {
        let mut s = Scheduler::new();
        let a = s.spawn();
        let b = s.spawn();
        let ch = s.new_channel();
        let _ = s.next_ready(); // run a
        s.on_trap(a, Trap::Recv(ch)); // a blocks on recv
        let _ = s.next_ready(); // run b
        s.on_trap(b, Trap::Recv(ch)); // b blocks on recv (after a)
        assert_eq!(s.next_ready(), None); // both blocked
        assert!(s.is_deadlocked());
        assert_eq!(s.on_send(ch), Some(a)); // FIFO: a (blocked first) wakes first
        assert_eq!(s.next_ready(), Some(a));
        assert_eq!(s.on_send(ch), Some(b));
        assert_eq!(s.next_ready(), Some(b));
        assert_eq!(s.on_send(ch), None); // nobody waiting now
    }

    #[test]
    fn join_blocks_until_target_done_then_wakes_all_joiners_in_order() {
        let mut s = Scheduler::new();
        let worker = s.spawn();
        let j1 = s.spawn();
        let j2 = s.spawn();
        // Each task must be popped (run) before it traps — the kernel's contract.
        assert_eq!(s.next_ready(), Some(worker));
        s.on_trap(worker, Trap::Yield); // worker yields so the joiners run first
        assert_eq!(s.next_ready(), Some(j1));
        s.on_trap(j1, Trap::Join(worker)); // j1 blocks on worker
        assert_eq!(s.next_ready(), Some(j2));
        s.on_trap(j2, Trap::Join(worker)); // j2 blocks on worker
        assert_eq!(s.next_ready(), Some(worker)); // only worker left ready
        assert_eq!(s.next_ready(), None);
        s.on_trap(worker, Trap::Done); // worker finishes → both joiners wake, in registration order
        assert_eq!(s.next_ready(), Some(j1));
        assert_eq!(s.next_ready(), Some(j2));
        assert!(s.is_done(worker));
    }

    #[test]
    fn join_on_already_done_task_is_immediately_ready() {
        let mut s = Scheduler::new();
        let worker = s.spawn();
        let j = s.spawn();
        assert_eq!(s.next_ready(), Some(worker)); // run worker
        s.on_trap(worker, Trap::Done);
        assert_eq!(s.next_ready(), Some(j)); // run j (popped per contract)
        s.on_trap(j, Trap::Join(worker)); // worker already done → j resumes at once
        assert_eq!(s.next_ready(), Some(j));
        assert_eq!(s.next_ready(), None); // no duplicate enqueue
    }

    #[test]
    fn all_done_is_not_a_deadlock() {
        let mut s = Scheduler::new();
        let a = s.spawn();
        let _ = s.next_ready();
        s.on_trap(a, Trap::Done);
        assert_eq!(s.next_ready(), None);
        assert!(!s.is_deadlocked()); // nothing ready AND nothing blocked = clean finish
        assert!(!s.has_blocked());
    }
}
