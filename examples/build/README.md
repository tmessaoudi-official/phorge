# Standalone executables — `phg build`

`phg build` compiles a program into a **single native executable** that runs on the bytecode VM
with no Phorj install. The program *source* is embedded in a CRC-guarded, versioned section of the
output binary (`.phorj` on ELF); at startup the binary detects that section and runs it on the VM —
a third surface on the parity spine, so it must match `phg runvm` byte-for-byte.

```bash
phg build app.phg                 # -> ./app for the host (x86_64-linux-gnu)
phg build app.phg -o dist/app     # choose the output path
./app                                # runs with no phorj on the machine
```

Building `app.phg` here (host build) and running the result prints exactly what
`phg runvm app.phg` prints:

```
phorj standalone build
fib(0) = 0
fib(1) = 1
fib(2) = 1
fib(3) = 2
fib(4) = 3
fib(5) = 5
fib(6) = 8
fib(7) = 13
fib(8) = 21
fib(9) = 34
```

- The output is a normal native executable (host: ELF64 `x86_64-linux-gnu`). It carries the VM plus
  the embedded program, so its size tracks the Phorj runtime, not the length of `app.phg`.
- `app.phg` is also in the byte-identity sweep — it runs on both backends
  (`phg run app.phg` / `phg runvm app.phg`) like every example here.
- `tests/build.rs` gates that a built binary's output equals `runvm`, so the embedded-source path
  can never silently drift from the VM.

## Cross-compiling (other OSes)

```bash
phg build app.phg --target x86_64-unknown-linux-musl   # one target
phg build app.phg --all                                # every supported target
```

Cross builds use **cargo-zigbuild** (the zig toolchain as the linker) and a per-target stub cache
keyed on the Phorj binary's own hash (rebuilding Phorj invalidates stale stubs). Supported today:
Linux `x86_64-musl`, `aarch64-{gnu,musl}`, and `x86_64-pc-windows-gnu`. Each produced binary
self-reads its own object format (ELF / PE / Mach-O) via std-only, checked-arithmetic section
readers. The macOS reader ships and is fixture-tested, but producing a *signed* macOS stub is
deferred — see `ROADMAP.md` (M2.5 Phase 3: distribution & signing).

## Build profiles — `--dev` (M-DX S0)

A built artifact carries a **build profile** baked into its container: **Release by default**, or
**Dev** with `--dev`.

```bash
phg build app.phg            # Release artifact (the shipped default)
phg build app.phg --dev      # Dev artifact (debug-oriented)
```

The profile gates *side-channels only* — value inspection, richer fault detail, the debugger (as
those land). It is **secure by construction**: the profile lives in the artifact's `.phorj`
container, chosen at build time, so **no environment variable can flip a Release binary into Dev at
runtime**. And it is invisible to program behavior — a Dev and a Release build of the same program
print byte-for-byte the same output (the M-DX keystone: a profile never changes observable
behavior). `phg run`/`runvm` are Dev (the interactive tool); `phg serve` is Release unless `--dev`
(its rich HTML fault page leaks traces/source, so it is Dev-only).
