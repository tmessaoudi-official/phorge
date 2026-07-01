# Manual benchmarking with `Core.Runtime`

Phorj gives you two ways to benchmark:

- **Automated** — `phg bench --vs-php your.phg` measures the interpreter, the VM, and (transpiled)
  real PHP for you, median-of-N, output-identity gated. Use this for a quick 3-way comparison.
- **Manual** — `Core.Runtime` lets you time and measure memory *from inside* your program, so you
  can build your own harness (per-phase timings, custom reporting, warmup loops). This directory
  shows that path.

## The API (`import Core.Runtime;`)

| Native | Returns | Meaning | PHP target |
|---|---|---|---|
| `Runtime.monotonicNanos()` | `int` | Nanoseconds on a **monotonic** clock (immune to wall-clock jumps — unlike `Core.Time`). Use differences, not absolute values. | `hrtime(true)` |
| `Runtime.memoryBytes()` | `int` | Current resident memory in bytes (`0` if the platform can't sample it). | `memory_get_usage(true)` |
| `Runtime.peakMemoryBytes()` | `int` | Peak resident memory since start / last reset. | `memory_get_peak_usage(true)` |
| `Runtime.resetPeakMemory()` | `void` | Reset the peak so a later reading reflects only the code since. | `memory_reset_peak_usage()` |

## Why this is quarantined (and why that's correct)

These natives read the live process, so their output changes every run — they are **non-deterministic**
and marked `pure: false`. Phorj's correctness spine is that `run`, `runvm`, and the transpiled PHP
produce **byte-identical** output; a benchmark reading can't satisfy that. So any program importing
`Core.Runtime` is **excluded from the byte-identity example gate** (like `Core.Time` and
`Core.Process`), and exercised instead in `tests/runtime.rs` under sanity assertions. Measurement is a
*harness* concern, deliberately kept out of the byte-identical surface — you get the capability without
weakening the guarantee.

## Walkthrough

[`stopwatch-and-memory.phg`](./stopwatch-and-memory.phg) defines a small copy-me `Stopwatch` class
over `Runtime.monotonicNanos()` and times a `fib(30)` workload. Run it:

```
phg run examples/bench/manual/stopwatch-and-memory.phg
```

In your own benchmark you'd print the raw numbers, e.g.:

```
var sw = new Stopwatch();
sw.start();
doWork();
Output.printLine("took {sw.elapsedMicros()} µs, peak {Runtime.peakMemoryBytes()} bytes");
```

Sample output (numbers vary per run — that's the point):

```
took 4213 µs, peak 2621440 bytes
```
