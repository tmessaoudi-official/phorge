use std::time::{Duration, Instant};

use crate::compiler::compile;
use crate::interpreter::interpret;
use crate::mem;
use crate::vm::Vm;

use super::*;

/// Default sample count for `phg bench`. Odd, so the median is a real observed sample rather
/// than an average of two; large enough to damp scheduler jitter on the small M2 corpus without
/// making the CLI feel slow.
const BENCH_DEFAULT_ITERS: usize = 101;

/// `bench`: *measure* the M2 thesis ("the VM executes faster than the tree-walker") instead of
/// asserting it. Parses+checks once, then reports median-of-N wall-clock for the front-end
/// (parse+check), the one-time bytecode compile, and each backend's execution phase, plus a
/// speedup verdict. Establishes the baseline that turns every later perf claim (Copy-on-`Op`,
/// deep-copy elimination, hot-path micro-perf) from Speculative into Verified — no perf-motivated
/// change should ship without a before/after number from this harness.
pub fn cmd_bench(src: &str) -> Result<String, String> {
    bench_report(src, BENCH_DEFAULT_ITERS)
}

/// `bench --vs-php`: the standard bench report plus a transpile-and-time-PHP comparison (Track D).
pub fn cmd_bench_vs_php(src: &str) -> Result<String, String> {
    bench_report_opts(src, BENCH_DEFAULT_ITERS, true)
}

/// `php --version`'s first line, or `None` if `php` is not on `PATH`. Used to gate + label the
/// `--vs-php` comparison.
fn php_version_line() -> Option<String> {
    let out = std::process::Command::new("php")
        .arg("--version")
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .next()
            .unwrap_or("php")
            .to_string(),
    )
}

/// Transpile `prog` to PHP, gate its output against `expected` (the Phorj backends' shared output),
/// then median-time `php <file>`. Returns a report section comparing PHP to the faster Phorj backend
/// (`tw`/`vm` medians), or a graceful note when `php` is absent or the transpiled output diverges.
/// Each sample spawns a `php` process — that cost is part of what's measured and is called out.
fn php_bench_section(
    prog: &Program,
    iters: usize,
    expected: &str,
    tw: Duration,
    vm: Duration,
) -> String {
    let Some(ver) = php_version_line() else {
        return "\nvs PHP: `php` not on PATH — skipping (install php to enable --vs-php)\n"
            .to_string();
    };
    let php_src = match crate::transpile::emit(prog) {
        Ok(s) => s,
        Err(e) => return format!("\nvs PHP: transpile failed ({e}) — skipping\n"),
    };
    let path = std::env::temp_dir().join(format!("phorj_bench_{}.php", std::process::id()));
    if std::fs::write(&path, &php_src).is_err() {
        return "\nvs PHP: could not write temp file — skipping\n".to_string();
    }
    let run_php = || -> Result<String, String> {
        let o = std::process::Command::new("php")
            .arg(&path)
            .output()
            .map_err(|e| e.to_string())?;
        if !o.status.success() {
            return Err(format!(
                "php exited {}: {}",
                o.status.code().unwrap_or(-1),
                String::from_utf8_lossy(&o.stderr).trim()
            ));
        }
        Ok(String::from_utf8_lossy(&o.stdout).into_owned())
    };
    let section = match run_php() {
        Err(e) => format!("\nvs PHP: run failed ({e}) — skipping\n"),
        // Output-identity gate — the same parity contract used between the Phorj backends. A
        // divergence is a transpile-bug report, not a timing result.
        Ok(out) if out != expected => format!(
            "\nvs PHP: transpiled output differs from Phorj ({} vs {} bytes) — skipping \
             (transpile divergence, not a timing result)\n",
            out.len(),
            expected.len()
        ),
        Ok(_) => match median_of(iters, run_php) {
            Err(e) => format!("\nvs PHP: timing failed ({e})\n"),
            Ok(php) => {
                let mut s = format!("\nvs PHP — {ver}\n");
                s.push_str(&format!(
                    "  php run       {}  (spawns a process per sample)\n",
                    fmt_dur(php)
                ));
                let best = tw.min(vm);
                let best_name = if vm <= tw { "vm" } else { "tree-walk" };
                let (a, b) = (best.as_nanos(), php.as_nanos());
                if a > 0 && b > 0 {
                    if a <= b {
                        s.push_str(&format!(
                            "  winner: Phorj ({best_name}) — {:.2}× faster than PHP ({} → {})\n",
                            b as f64 / a as f64,
                            fmt_dur(php),
                            fmt_dur(best)
                        ));
                    } else {
                        s.push_str(&format!(
                            "  winner: PHP — {:.2}× faster than Phorj ({best_name}) ({} → {})\n",
                            a as f64 / b as f64,
                            fmt_dur(best),
                            fmt_dur(php)
                        ));
                    }
                }
                s.push_str(
                    "  note: PHP timing includes process spawn and depends on opcache/JIT (php.ini)\n",
                );
                s
            }
        },
    };
    let _ = std::fs::remove_file(&path);
    section
}

/// Median wall-clock of `f` over `iters` samples after one untimed warmup. Generic over the
/// closure's `Ok` value so the same path times the interpreter (`String`), the VM (`String`), and
/// the compiler (`BytecodeProgram`). Propagates the first error — a faulting program can't be
/// benchmarked. The warmup pays one-time allocation/cache costs outside the measured window.
fn median_of<T>(
    iters: usize,
    mut f: impl FnMut() -> Result<T, String>,
) -> Result<Duration, String> {
    f()?; // warmup (untimed)
    let mut samples: Vec<Duration> = Vec::with_capacity(iters);
    for _ in 0..iters {
        let t0 = Instant::now();
        f()?;
        samples.push(t0.elapsed());
    }
    samples.sort_unstable();
    Ok(samples[samples.len() / 2])
}

/// Adaptive duration rendering (ns / µs / ms) so a fast and a slow stage stay legible in the same
/// report instead of a fixed unit truncating one of them to `0.000`.
fn fmt_dur(d: Duration) -> String {
    let ns = d.as_nanos();
    if ns < 1_000 {
        format!("{ns} ns")
    } else if ns < 1_000_000 {
        format!("{:.3} µs", ns as f64 / 1_000.0)
    } else {
        format!("{:.3} ms", ns as f64 / 1_000_000.0)
    }
}

/// Peak resident-memory *growth* (KiB) a single run of `f` causes: rewind the kernel high-water
/// mark, sample the current RSS, run `f` once, then read the new peak and subtract the baseline.
/// Resetting the mark per phase makes the number baseline-independent, so the tree-walker and VM
/// stay comparable even though they execute sequentially in one process (and glibc rarely returns
/// freed pages to the OS, so a lifetime peak would unfairly charge each later phase for the
/// earlier ones). `None` when `/proc` is unavailable (non-Linux). One run is enough — peak memory
/// is deterministic, so there's nothing to median. Propagates a faulting program's error.
fn peak_growth_of<T>(mut f: impl FnMut() -> Result<T, String>) -> Result<Option<u64>, String> {
    mem::reset_peak_rss();
    let before = mem::current_rss_kb();
    f()?;
    let peak = mem::peak_rss_kb();
    // `saturating_sub`: if the peak somehow reads below the baseline (sampling race), report 0
    // growth rather than underflowing.
    Ok(match (before, peak) {
        (Some(b), Some(p)) => Some(p.saturating_sub(b)),
        _ => None,
    })
}

/// Render an optional KiB measurement adaptively (`KiB` / `MiB`), or `n/a` when unavailable.
fn fmt_kb(kb: Option<u64>) -> String {
    match kb {
        None => "n/a".to_string(),
        Some(k) if k < 1024 => format!("{k} KiB"),
        Some(k) => format!("{:.2} MiB", k as f64 / 1024.0),
    }
}

/// The bench engine (separated from [`cmd_bench`] so tests can pass a small `iters`). Runs on the
/// deep-stack worker like every other pipeline command.
pub(super) fn bench_report(src: &str, iters: usize) -> Result<String, String> {
    bench_report_opts(src, iters, false)
}

/// Bench engine with an opt-in PHP comparison (`--vs-php`, Track D). `vs_php` transpiles the program,
/// gates its PHP output against the Phorj backends' output, and median-times `php <file>`.
pub(super) fn bench_report_opts(src: &str, iters: usize, vs_php: bool) -> Result<String, String> {
    on_deep_stack(|| {
        let prog = parse_checked(src)?;
        let program = compile(&prog).map_err(|e| e.to_string())?;

        // Cold memory probe — measured *first*, before the parity gate and timing loops warm the
        // allocator. Peak-RSS growth is only meaningful from a cold heap: once glibc has mapped
        // pages it almost never returns them to the OS, so a post-warmup or sequential
        // per-backend figure reads ~0 and misleads. One honest cold-run number, plus the process
        // peak below, is the defensible memory signal (full per-backend attribution would need a
        // fresh process per backend — out of scope here).
        let cold_alloc = peak_growth_of(|| interpret(&prog).map_err(|e| e.to_string()))?;

        // Output-identity gate: comparing the speed of two backends that *disagree* is
        // meaningless. This is the differential harness's parity contract, enforced here at run
        // time before any timing — if it ever fails, the divergence (not the timing) is the news.
        let tw_out = interpret(&prog).map_err(|e| e.to_string())?;
        let vm_out = Vm::new(&program).run().map_err(|e| e.to_string())?;
        if tw_out != vm_out {
            return Err(format!(
                "bench aborted: backends disagree — tree-walk produced {} bytes, vm {} bytes; \
                 fix parity (see the differential harness) before benchmarking",
                tw_out.len(),
                vm_out.len()
            ));
        }

        let front = median_of(iters, || parse_checked(src))?;
        let comp = median_of(iters, || compile(&prog).map_err(|e| e.to_string()))?;
        let tw = median_of(iters, || interpret(&prog).map_err(|e| e.to_string()))?;
        let vm = median_of(iters, || Vm::new(&program).run().map_err(|e| e.to_string()))?;

        // Branch on integer nanos (no float equality); convert to f64 only for the ratio display.
        let tw_ns = tw.as_nanos();
        let vm_ns = vm.as_nanos();
        let verdict = if tw_ns == 0 || vm_ns == 0 {
            "verdict: backend execution too fast to measure at this sample size — \
             use a heavier corpus"
                .to_string()
        } else if vm_ns <= tw_ns {
            format!(
                "verdict: vm run is {:.2}× faster than tree-walk run ({} → {})",
                tw_ns as f64 / vm_ns as f64,
                fmt_dur(tw),
                fmt_dur(vm)
            )
        } else {
            format!(
                "verdict: tree-walk run is {:.2}× faster than vm run ({} → {})",
                vm_ns as f64 / tw_ns as f64,
                fmt_dur(vm),
                fmt_dur(tw)
            )
        };

        let mut out = String::new();
        out.push_str(&format!(
            "phg bench — median of {iters} (warmup 1, std Instant)\n"
        ));
        out.push_str(&format!(
            "output: {} bytes, identical on both backends\n\n",
            tw_out.len()
        ));
        out.push_str(&format!("  parse+check   {}\n", fmt_dur(front)));
        out.push_str(&format!(
            "  compile       {}  (one-time, vm only)\n",
            fmt_dur(comp)
        ));
        out.push_str(&format!("  tree-walk run {}\n", fmt_dur(tw)));
        out.push_str(&format!("  vm run        {}\n\n", fmt_dur(vm)));
        out.push_str(&verdict);
        out.push('\n');

        // Optional PHP comparison (Track D) — appended after the Phorj verdict, before memory.
        if vs_php {
            out.push_str(&php_bench_section(&prog, iters, &tw_out, tw, vm));
        }

        // Memory (Linux /proc). The cold-run growth (captured before any warmup) is the workload's
        // own resident footprint for one execution; the process figures are the bench process's
        // lifetime high-water mark and current resident set.
        match cold_alloc {
            None => out.push_str("\nmemory: unavailable on this platform (requires Linux /proc)\n"),
            Some(g) => {
                out.push_str("\nmemory\n");
                out.push_str(&format!(
                    "  cold run      +{} RSS  (one tree-walk execution from a cold heap)\n",
                    fmt_kb(Some(g))
                ));
                out.push_str(&format!(
                    "  process peak  {}  (VmHWM)\n",
                    fmt_kb(mem::peak_rss_kb())
                ));
                out.push_str(&format!(
                    "  resident now  {}  (VmRSS)\n",
                    fmt_kb(mem::current_rss_kb())
                ));
            }
        }
        Ok(out)
    })
}
