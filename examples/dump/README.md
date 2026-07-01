# Value-dump on fault — `phg run --dump-on-fault`

When a program hits an uncaught runtime fault, Phorj can print a **post-mortem** of the faulting
frame's local variables to stderr — the fastest way to see *what the values actually were* when it
broke, without reaching for a debugger.

It is **opt-in and Dev-only** by design (secrets and program data are sensitive):

- Off by default — you pass `--dump-on-fault` explicitly.
- Never in a `Release` artifact — `phg build` binaries run under the Release profile, where the
  dump machinery is gated off regardless of any flag or environment variable (see
  [`../build/README.md`](../build/README.md)).

> A value-dump is a *fault* scenario, so it has no byte-identical "Ok" output and therefore isn't a
> runnable example under the differential sweep — this walkthrough is the surface instead.

## Example

Given a program that indexes a list out of bounds inside `compute`:

```phorj
package Main;
import Core.Output;
import Core.Secret;

function compute(int n) -> int {
  int doubled = n * 2;
  Secret<string> token = new Secret("hunter2");
  List<int> xs = [10, 20];
  return xs[n] + doubled;          // n = 5 → out of range
}

function main() -> void {
  Output.printLine("{compute(5)}");
}
```

```bash
phg run --dump-on-fault program.phg
```

prints to **stderr**:

```
runtime error at 9: list index out of range
  return xs[n] + doubled;
stack trace (most recent call first):
  → compute            line 9
    main               line 13
faulting frame locals:
  doubled = 10
  n = 5
  token = Secret(<redacted>)
  xs = [10, 20]
```

Note three things:

- **Locals are named and shown with their values** (`n = 5`, `xs = [10, 20]`), sorted by name for a
  **deterministic** dump.
- **`Secret<T>` is redacted** — `token = Secret(<redacted>)`, never the wrapped plaintext. The same
  secure renderer caps depth, element count, and length, so a huge or hostile value can't flood the
  terminal.
- The dump is on **stderr** only; it never touches stdout, so it can't change a program's observable
  output (`run ≡ runvm ≡ PHP` is untouched).

## Backends

The rich named-local dump is produced by the **interpreter** (`phg run`), which holds live
`name → value` scopes at fault time. `phg runvm` shares the identical **backtrace** but not the
locals section: the bytecode VM stores slot-indexed locals with no name table (mirroring the
interpreter-only debugger — the parity spine guarantees the backends agree, so a dump taken on the
interpreter faithfully reflects a `runvm` fault too). For the value-dump, prefer `phg run`.
