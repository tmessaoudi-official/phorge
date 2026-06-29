//! Green-thread runtime (M6 W4 / S4.3) — uncolored cooperative concurrency: `spawn` + channels on a
//! single OS thread (the `Rc` `Value` heap is `!Send`, so this is cooperative, not parallel).
//!
//! The architecture (developer-locked, `docs/specs/2026-06-29-m6-w4-green-threads-design.md`):
//! - [`sched`] is the **single-sourced, backend-agnostic scheduler kernel** — it owns ONLY scheduling
//!   decisions (run-queue order, channel wait-lists, wake/pick) over opaque `TaskId`/`ChanId`. Both the
//!   interpreter and the VM drive the SAME kernel (like `value.rs` kernels are single-sourced), so the
//!   two backends make identical scheduling decisions ⇒ byte-identical task interleaving ⇒ `run≡runvm`.
//! - The **executor** (resuming a task until it traps) is per-backend: native uses stackful coroutines
//!   on both backends; `wasm32` runs tasks on the VM frame-swap (coroutines don't compile on wasm). The
//!   kernel here is target-independent and identical everywhere.
//!
//! This module currently contains only the kernel; the executor wiring lands in the next build steps.

pub mod sched;

// S4.3 step-3 gating spike (feasibility probe; compiled only under `green` + non-wasm + test).
#[cfg(all(feature = "green", not(target_arch = "wasm32"), test))]
mod spike;
