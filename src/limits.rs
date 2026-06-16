//! Central, documented home for every resource limit the pipeline enforces — "symmetry as
//! policy" (M2 P3.5 Wave 2 Task 2.2). These previously lived as scattered bare consts in
//! `value.rs` (`MAX_CALL_DEPTH`), `parser.rs` (`MAX_NEST_DEPTH`), and `checker.rs`
//! (`MAX_EXPR_DEPTH`); collecting them here makes the Wave 0 crash-guards a single, legible
//! posture rather than three independent patches, and gives one place to tune them.
//!
//! All three depth limits exist to keep adversarial-but-bounded input faulting *cleanly* (exit 1
//! with a `Diagnostic`) rather than overflowing the native stack (SIGABRT). They are reachable
//! because `cli::on_deep_stack` runs the whole pipeline on a 256 MB worker thread (see
//! `recursion-guards-pipeline-thread`).

/// Maximum call-frame depth, enforced **identically by both backends** — the interpreter's
/// `run_call` depth counter and the VM's `frames` cap. Exceeding it is a clean `"stack overflow"`
/// runtime error (exit 1), never an abort. A *single shared* limit is what keeps `run` ≡ `runvm`
/// in the fault path: separate limits would let one backend succeed where the other errors.
///
/// The value is far below what the VM's heap-allocated frames could hold (it formerly capped at
/// `64*1024`) because the interpreter recurses on the *native* Rust stack (~14 KB/frame in debug,
/// so ~875 frames fit a default 12.2 MB stack). `cli::cmd_run`/`cmd_runvm` run the whole pipeline
/// on a dedicated 256 MB-stack thread so this limit is reachable with >4× native margin.
pub const MAX_CALL_DEPTH: usize = 4096;

/// Cap on expression-nesting depth in the recursive-descent parser. Past it, the parser returns a
/// clean `Diagnostic` instead of overflowing the native stack (SIGABRT) — measured: nested parens
/// abort the parser around ~1750 levels on the default 12.2 MB stack. The parser runs on the
/// *main* thread (unlike the interpreter's 256 MB worker), so this limit is its own, far below
/// that ceiling; real source never nests beyond a few dozen. Every nesting vector (parens, unary
/// chains, index/list/arg re-entry) routes through `parse_unary` exactly once per level, so a
/// single counter there bounds all of them.
pub const MAX_NEST_DEPTH: usize = 512;

/// Cap on expression-tree depth walked by the checker's `check_expr`. [`MAX_NEST_DEPTH`] bounds
/// *nesting* (parens, unary chains), but a long left-associative chain like `1+1+…` is built
/// *iteratively* and so escapes that limit — yet still produces a deeply left-leaning AST that
/// every recursive walker (checker, interpreter, compiler) descends. The checker is the gate both
/// backends share, so bounding depth here faults such input cleanly (identically on `run`/`runvm`)
/// instead of letting a downstream walker overflow its stack. Measured: a chain overflows the
/// 256 MB pipeline thread around ~50–100k terms, so this sits well below with margin.
pub const MAX_EXPR_DEPTH: usize = 10_000;

/// Bit width of the language's integer scalar (`int` → Rust `i64`, two's-complement). Documented
/// here as policy; the overflow bound itself is enforced by the checked kernels in `value.rs`
/// (`int_add`/`int_mul`/… → `FAULT_INT_OVERFLOW`). A future sized-int / bignum surface (M3) is the
/// expected consumer.
pub const INT_BITS: u32 = i64::BITS;

/// Bit width of the language's floating scalar (`float` → Rust `f64`, IEEE-754 binary64). Float
/// arithmetic does not fault on overflow (it yields `inf`/`NaN`, matching the interpreter); this
/// constant documents the width as policy alongside [`INT_BITS`].
pub const FLOAT_BITS: u32 = 64;

#[cfg(test)]
mod tests {
    use super::*;

    /// Lock the limit values: a change here is a deliberate policy decision, not an accident.
    #[test]
    fn limit_values_are_stable() {
        assert_eq!(MAX_CALL_DEPTH, 4096);
        assert_eq!(MAX_NEST_DEPTH, 512);
        assert_eq!(MAX_EXPR_DEPTH, 10_000);
        assert_eq!(INT_BITS, 64);
        assert_eq!(FLOAT_BITS, 64);
    }
}
