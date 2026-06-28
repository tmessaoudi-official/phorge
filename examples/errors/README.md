# `errors/` — stack traces & fault reporting

A runtime fault that *aborts* the program (a bad index, a division by zero, a force-unwrap of null)
can't be a runnable byte-identity example — it produces no stdout — so it's documented here instead of
shipped as a globbed `.phg`.

## What a fault looks like

This program faults two calls deep:

```phorj
package Main;

function f() -> int {
  var xs = [1];
  return xs[5];          // index out of range
}

function main() {
  var r = f();
}
```

Running it on **either backend** (`phg run` or `phg runvm`) prints a **byte-identical** trace to stderr
and exits non-zero:

```
runtime error at 5: list index out of range
  return xs[5];          // index out of range
stack trace (most recent call first):
  → f                  line 5
    main               line 9
```

The trace is the same on `run` and `runvm` by construction — the VM walks its real call frames and the
tree-walking interpreter keeps a logical frame stack that mirrors them (enforced by a `run ≡ runvm`
trace-parity test). In a multi-file project, each frame shows its origin `file:line`; the caret line is
drawn from that file's source.

## In the browser (`phg serve --dev`)

When a served handler (`respond(bytes) -> bytes`) hits an uncaught fault, **`phg serve --dev`** returns
a styled HTML **500 page** with the same fault message, the call stack, and the request's start-line +
headers — every interpolated value HTML-escaped (the `Core.Html` discipline), so the page is XSS-safe.

**Production never leaks a trace.** Without `--dev`, an uncaught fault returns a bare generic
`500 Internal Server Error` — no message, no stack, no source. The dev page is a development tool only.

## Notes

- Faults are compared across backends *and* real PHP by semantic kind (`FaultKind`), not trace text —
  so traces stay rich without touching the byte-identity spine (the trace rides on stderr, never stdout).
- Catching/handling faults (a `try`/`catch` or `Result<T, E>` model) is a separate, later slice; this
  is purely about *reporting* faults that abort.
