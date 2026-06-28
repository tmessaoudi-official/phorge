# Process & environment (`Core.Process` / `Core.Env`)

These are Phorj's first **ambient-environment** natives — their result depends on the *process*
(its command-line arguments and environment variables), not the program text.

```phorj
import Core.Process;
import Core.Env;

Process.args() -> List<string>      // program arguments (everything after `--`)
Env.get(name: string) -> string?    // one environment variable, or null if unset
Env.all() -> Map<string, string>    // every environment variable, sorted by key
```

A program that just needs its arguments can take them as `main`'s single `List<string>` parameter
instead of calling `Process.args()` — see [`main-args.phg`](main-args.phg), which also returns its
argument count as the process **exit code** (Batch-1 B). Both surfaces read the same argv:

```console
$ phg run examples/process/main-args.phg -- alpha beta
argc = 2
arg: alpha
arg: beta
matches Process.args() = true
$ echo $?
2
```

Run [`args-env.phg`](args-env.phg) and pass arguments after the `--` terminator:

```console
$ PHORJ_DEMO=hi phg run examples/process/args-env.phg -- alpha beta
argc = 2
arg: alpha
arg: beta
HOME = /home/developer
PHORJ_DEMO = hi
env count = 987
```

## Why this is a walkthrough, not a gated example

Every other example in this repo is **byte-identity-gated**: the tree-walking interpreter, the bytecode
VM, and the transpiled PHP must print exactly the same bytes (`tests/differential.rs`). That can't hold
here — the PHP leg runs in a *separate* `php` process whose `$argv`/`getenv()` need not match the Rust
process, and the output isn't a fixed golden anyway (it depends on your machine).

So programs that import `Core.Process` / `Core.Env` are **quarantined**: the differential skips them
(detected via the `pure: bool` marker on each native), and they are tested instead under a *controlled*
environment in [`tests/process.rs`](../../tests/process.rs), which sets the args/env it asserts on. The
`run ≡ runvm` half still holds (both Rust backends share one process) — only the PHP oracle is opted out.

## Notes

- **`--` terminator.** `phg run file.phg -- a b c` passes `a b c` to `Process.args()`. Everything
  before `--` is phg's own source spec; everything after is the program's argv.
- **Standalone binaries.** `phg build`-produced executables read the real process `argv`, so
  `Process.args()` works exactly as in a normal program.
- **Read-only.** There is no `putenv` — environment mutation is out of scope (matching the read-only
  stance of reflection). `Env.all()` is sorted by key for a stable result.
- **Transpiled PHP.** `Process.args` → `array_slice($argv, 1)`; `Env.get` → `getenv` coerced to
  `null`; `Env.all` → `getenv()` + `ksort`.
