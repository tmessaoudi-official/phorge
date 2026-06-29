//! S4.3 step-3 GATING SPIKE — prove the coroutine suspension model the tree-walking interpreter needs
//! works under phorj's `#![forbid(unsafe_code)]`. The crux (design §4): a deeply-recursive evaluator
//! must `suspend` from far down its call stack. corosensei's `Yielder` is only handed to the coroutine
//! closure, so to reach it deep in the recursion we borrow it into a lifetime-parameterized worker
//! struct created *inside* the closure (the closure is `'static`, but locals inside it may borrow the
//! yielder arg). No `unsafe` in our crate; no threading the yielder through every method signature.
//!
//! This module is compiled only with the `green` feature on a non-wasm target, and only `cfg(test)`
//! exercises it — it is a feasibility probe, deleted once the real executor (step 3b) lands.

#![cfg(all(feature = "green", not(target_arch = "wasm32"), test))]

use corosensei::{Coroutine, CoroutineResult, Yielder};

/// A toy "evaluator" that recurses, suspending (yielding the current depth) at each level — the shape
/// the interpreter needs: suspend from deep in a nested call, holding the yielder in `self`.
struct Worker<'y> {
    yielder: &'y Yielder<(), usize>,
    log: Vec<usize>,
}

impl Worker<'_> {
    /// Recurse to `depth`, suspending at each level (yielding the level number) — proves a suspend can
    /// happen arbitrarily deep without threading the yielder through the call or using `unsafe`.
    fn descend(&mut self, level: usize, max: usize) -> usize {
        if level == max {
            return level;
        }
        self.log.push(level);
        self.yielder.suspend(level); // <-- the crux: suspend from inside a nested call, via `self`
        self.descend(level + 1, max)
    }
}

#[test]
fn coroutine_suspends_from_deep_recursion_without_unsafe() {
    // The closure is `'static`; the `Worker` it builds borrows the yielder for the closure body only.
    let mut co = Coroutine::new(|yielder: &Yielder<(), usize>, ()| {
        let mut w = Worker {
            yielder,
            log: Vec::new(),
        };
        let deepest = w.descend(0, 4);
        (deepest, w.log)
    });

    // Drive it: each `resume` runs until the next `suspend`, returning the yielded level. After the
    // 4 suspends (levels 0..3) it returns the final value — exactly the scheduler↔task loop shape.
    let mut yielded = Vec::new();
    loop {
        match co.resume(()) {
            CoroutineResult::Yield(level) => yielded.push(level),
            CoroutineResult::Return((deepest, log)) => {
                assert_eq!(
                    yielded,
                    vec![0, 1, 2, 3],
                    "suspends fire at every recursion level"
                );
                assert_eq!(deepest, 4, "the coroutine resumes and runs to completion");
                assert_eq!(
                    log,
                    vec![0, 1, 2, 3],
                    "state mutated through `self` across suspends"
                );
                return;
            }
        }
    }
}
