# Profiling example — `workload.phg`

A focused workload for measuring the two M2 backends, plus a guide to **how** Phorge collects the
time and memory numbers. (Phorge the language has no clock or syscall surface — `println` is the only
builtin — so a `.phg` program *cannot* profile itself. All measurement lives in the Rust `bench`
tool; this page documents that mechanism.)

## What the workload exercises

| Section | Cost center |
|---|---|
| `fib(18)` | **CPU** — exponential call volume, shallow recursion. Where the bytecode VM's per-call overhead advantage over the tree-walker shows up. |
| `allocate_chain(1000)` | **Heap + stack** — 1000 `Cell` instances simultaneously live (each recursion frame keeps its `c` alive across the call) on the `Rc`-shared object heap, 1000-deep recursion. |
| `for (Cell c in […])` | **Object access** — a method call + field read per list element. |

It runs byte-identically on `phg run` and `phg runvm` (gated by `tests/differential.rs`, which
globs `examples/**/*.phg`).

## Running it

```sh
phg bench  examples/bench/workload.phg            # per-phase wall-clock + memory
phg bench --vs-php examples/bench/workload.phg    # + a 3-way comparison against transpiled PHP
phg disasm examples/bench/workload.phg            # the bytecode the VM executes
phg run    examples/bench/workload.phg            # tree-walking interpreter
phg runvm  examples/bench/workload.phg            # bytecode VM
```

`bench` runs the whole program **101×** (median of 101, one untimed warmup), so it takes several
seconds — that's the sampling cost, not the program's runtime.

## `--vs-php` — who's the winner?

`phg bench --vs-php <file>` adds a head-to-head against the **PHP backend**: it transpiles the
program to PHP, runs it once to **gate output identity** (a transpile divergence aborts the
comparison — it's a bug report, not a timing result), then median-times `php <file>` the same way.
The report gains a `vs PHP` section naming the faster of the Phorge VM and PHP. Requires `php` on
PATH; absent it, the section is a graceful skip note.

A representative run on this workload (`fib(18)` + 1000-instance allocation):

```
  tree-walk run 89.078 ms
  vm run        11.910 ms
verdict: vm run is 7.48× faster than tree-walk run

vs PHP — PHP 8.6.0-dev (cli)
  php run       38.519 ms  (spawns a process per sample)
  winner: Phorge (vm) — 3.23× faster than PHP (38.519 ms → 11.910 ms)
```

**Read it honestly:** this is the Rust bytecode VM vs the PHP interpreter on the *same algorithm* —
informative, but apples-to-oranges (different runtimes). The PHP timing includes process spawn and
depends on whether opcache/JIT is enabled in your `php.ini` (the figure above is a debug PHP build, so
a tuned PHP would close the gap). The number to trust most is the in-process `run`-vs-`runvm` verdict.

## How execution time is collected

`bench` times four phases with the standard-library monotonic clock (`std::time::Instant`), each as
the **median of 101 samples** after one untimed warmup (warmup pays one-time allocation/cache costs
outside the measured window; the median rejects scheduler-jitter outliers a mean would absorb):

- `parse+check` — front-end (lex → parse → type-check)
- `compile` — AST → bytecode (one-time, VM only)
- `tree-walk run` — the interpreter executing the program
- `vm run` — the bytecode VM executing the program

Before any timing, an **output-identity gate** runs both backends once and aborts the benchmark if
their stdout differs — comparing the speed of two backends that *disagree* would be meaningless
(this is the differential harness's parity contract, enforced at run time).

## How memory is collected

Memory sampling (`src/mem.rs`) is **Linux-only and std-only** — no crates, no `unsafe`, just reading
and writing files under `/proc/self`:

- **current RSS** (`VmRSS`) and **peak RSS** (`VmHWM`) are parsed out of `/proc/self/status`.
- The peak is a kernel-tracked high-water mark. Writing `5` to `/proc/self/clear_refs` (Linux ≥ 4.0)
  **rewinds** `VmHWM` down to the current `VmRSS`, so a single execution's *growth* can be isolated.

`bench` reports:

- **`cold run +N RSS`** — the peak-RSS growth of **one tree-walk execution from a cold heap**,
  measured *before* the timing loops run. This is the honest per-execution figure: it must be taken
  cold because once glibc has mapped pages for the heap it almost never returns them to the OS, so a
  post-warmup or sequential per-backend measurement reads ~0 KiB and would mislead. (True
  per-backend attribution would need a fresh process per backend — out of scope for this tool.)
- **`process peak`** (`VmHWM`) and **`resident now`** (`VmRSS`) — the bench process's lifetime
  high-water mark and current resident set.

On any non-Linux host (including the cross-built Windows/macOS binaries), `/proc` is absent: every
sampling function returns `None` and `bench` prints `memory: unavailable on this platform` instead
of failing.
