# M6 W3 — Serve Runtime Design (the determinism quarantine)

> **Status:** DESIGNED, ready to implement (no code written yet). Worked out in the session of
> 2026-06-18 right after W2 (`0fad849`) landed; persisted here before a compact so the detailed
> design survives. Parent: `docs/specs/2026-06-18-m6-web-design.md` (W3 = socket runtime + `Transport`
> seam, OUTSIDE the byte-identity spine). Predecessors shipped: W0 bytes, W1 handler, W2 router.

## 0. What W3 delivers

`src/serve.rs` — the ONE place sockets + wall-clock non-determinism live. Deliberately **outside**
`tests/differential.rs` (its conformance is checked by a new `tests/serve.rs` over a deterministic
in-memory `Transport`). The portable unit stays `handle(Request) -> Response` (W1) *inside* the
served program; the runtime only shuttles raw bytes to a single Phorj entry **`respond(bytes) ->
bytes`** and writes the result back. HTTP/1.1, `Connection: close`, one request per connection.

**Single-threaded by FORCE:** the `Rc`-shared heap (P5a) makes `Value` non-`Send`, so no thread pool
is possible; real concurrency arrives with M6 green-threads under this unchanged contract.

## 1. Key decision — serve uses `interpreter::call_named` (NOT the VM) for the spike

The design memory said "VM call-named-fn-with-arg," but the *spirit* is "call a named fn with a
constructed arg." The interpreter path is ~12 lines (reuses `run_call`, which already takes args +
returns a `Value`), trivially correct, and **`run` ≡ `runvm` (differential harness) guarantees the
VM would compute identical bytes**. The VM has no return-value capture today (`do_return` drops the
rv when `frames` empties; `run` returns `self.out`, not a value) — a VM `call_named` needs a
sentinel-frame trampoline (~50 lines of careful loop duplication). **Decision: interpreter now, VM
fast-path deferred** (a clean follow-up; localhost dev serving doesn't need VM speed). Surface this
in the post-W4 review.

## 2. The single entry contract — `respond(bytes) -> bytes`

The served program exposes ONE function the runtime calls per request:
```phorj
function respond(bytes raw) -> bytes {
  if (var req = parse_request(raw)) { return serialize_response(dispatch(req)); }
  else { return serialize_response(bad_request()); }   // malformed → 400, in Phorj
}
```
All HTTP logic (parse, route, serialize, 400) stays in **pure Phorj** (tested in-spine via the W4
example). The runtime is the thinnest possible glue: `bytes` in → one call → `bytes` out. No opaque
`Value` shuttling, no Rust knowledge of the `Request`/`Response` class layout. The PHP bridge (W4)
mirrors this: a front-controller builds a `Request` from superglobals → `handle` → echo (the value
unit `handle(Request)->Response` is what round-trips; the bytes↔superglobal adapter is runtime glue).

## 3. `src/interpreter.rs` — add `call_named` (place right after `interpret`)

```rust
/// Call a single named top-level function with pre-built `args`, returning its value + captured
/// stdout. The serve runtime (W3) uses this to invoke `respond(bytes) -> bytes` per request — the
/// one entry the socket bridge needs, backend-agnostic at the value level (the interpreter is the
/// reference backend; `run` ≡ `runvm` guarantees the VM would agree).
pub fn call_named(program: &Program, name: &str, args: Vec<Value>) -> Result<(Value, String), Diagnostic> {
    let mut interp = Interp {
        funcs: HashMap::new(), classes: HashMap::new(), variants: HashMap::new(),
        frame: CallScopes::new(), this: None, out: String::new(), depth: 0,
    };
    interp.collect(program);
    let f = match interp.funcs.get(name) {
        Some(f) => f.clone(),
        None => return Err(Diagnostic::runtime(format!("no `{name}` function"))),
    };
    if f.params.len() != args.len() {
        return Err(Diagnostic::runtime(format!(
            "`{name}` expects {} argument(s), got {}", f.params.len(), args.len())));
    }
    let names: Vec<String> = f.params.iter().map(|p| p.name.clone()).collect();
    match interp.run_call(&names, &f.body, args, None) {
        Ok(v) => Ok((v, interp.out)),
        Err(Signal::Return(v)) => Ok((v, interp.out)),
        Err(Signal::Runtime(e)) => Err(e),
    }
}
```
(`Interp`, `CallScopes`, `Signal`, `Value`, `HashMap`, `Diagnostic` are all already in scope in
interpreter.rs — mirror the existing `interpret` fn exactly.)

## 4. `src/serve.rs` (NEW) — register `pub mod serve;` in `src/lib.rs`

```rust
//! M6 W3 — HTTP serve runtime. The ONE place sockets + non-determinism live; OUTSIDE the
//! byte-identity spine (tests/differential.rs never touches it — tests/serve.rs covers it over a
//! deterministic in-memory Transport). Single-threaded by force (Rc heap → Value: !Send).
use crate::ast::Program;
use crate::interpreter::call_named;
use crate::value::Value;
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::rc::Rc;

/// The default Phorj entry the runtime calls per request.
pub const SERVE_ENTRY: &str = "respond";

/// Seam between the serve loop and the world. TcpTransport is the real socket; tests/serve.rs swaps
/// an in-memory transport (the env-update HTTP-fixture-seam pattern) so the loop needs no port.
pub trait Transport {
    /// Block for the next raw request, or Ok(None) when exhausted (shutdown).
    fn recv(&mut self) -> io::Result<Option<Vec<u8>>>;
    /// Write the raw response for the request just recv'd, then end that exchange.
    fn send(&mut self, response: &[u8]) -> io::Result<()>;
}

/// Serve requests from `transport`, routing each through the program's `respond(bytes) -> bytes`.
/// A fault on one request becomes a 500 (logged to stderr); the loop continues.
pub fn serve<T: Transport>(program: &Program, transport: &mut T) -> io::Result<()> {
    while let Some(raw) = transport.recv()? {
        let response = respond_once(program, &raw);
        transport.send(&response)?;
    }
    Ok(())
}

fn respond_once(program: &Program, raw: &[u8]) -> Vec<u8> {
    let arg = Value::Bytes(Rc::new(raw.to_vec()));
    match call_named(program, SERVE_ENTRY, vec![arg]) {
        Ok((Value::Bytes(b), out)) => { if !out.is_empty() { eprint!("{out}"); } b.as_ref().clone() }
        Ok((other, _)) => { eprintln!("serve: `{SERVE_ENTRY}` returned {}, expected bytes", other.type_name()); http_500() }
        Err(e) => { eprintln!("serve: request failed: {e}"); http_500() }
    }
}

fn http_500() -> Vec<u8> {
    let body = b"internal server error";
    let head = format!("HTTP/1.1 500 Internal Server Error\r\nContent-Length: {}\r\nConnection: close\r\nContent-Type: text/plain\r\n\r\n", body.len());
    head.into_bytes().into_iter().chain(body.iter().copied()).collect()
}

/// Production transport: single-threaded TcpListener, one request per accepted connection
/// (Connection: close). recv() frames the request (reads to \r\n\r\n then Content-Length bytes) —
/// framing, NOT parsing; the program's parse_request does the semantic parse.
pub struct TcpTransport { listener: TcpListener, current: Option<TcpStream> }
impl TcpTransport {
    pub fn bind(addr: &str) -> io::Result<Self> { Ok(Self { listener: TcpListener::bind(addr)?, current: None }) }
    pub fn local_addr(&self) -> io::Result<std::net::SocketAddr> { self.listener.local_addr() }
}
impl Transport for TcpTransport {
    fn recv(&mut self) -> io::Result<Option<Vec<u8>>> {
        let (mut stream, _peer) = self.listener.accept()?;
        let raw = read_http_request(&mut stream)?;
        self.current = Some(stream);
        Ok(Some(raw))
    }
    fn send(&mut self, response: &[u8]) -> io::Result<()> {
        if let Some(mut stream) = self.current.take() { stream.write_all(response)?; stream.flush()?; }
        Ok(())  // dropping stream closes it (Connection: close)
    }
}

/// Bind addr and serve until killed (the blocking accept-loop `phg serve` calls in W4).
pub fn serve_tcp(program: &Program, addr: &str) -> io::Result<()> {
    let mut t = TcpTransport::bind(addr)?;
    eprintln!("phg serve: listening on http://{}", t.local_addr()?);
    serve(program, &mut t)
}

const MAX_REQUEST: usize = 8 * 1024 * 1024;
/// Read one HTTP/1.1 request: up to+including \r\n\r\n, then Content-Length body (0 if absent).
/// Capped at MAX_REQUEST (EV-7 spirit). Framing only — no semantic validation.
fn read_http_request(stream: &mut TcpStream) -> io::Result<Vec<u8>> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        if let Some(pos) = find_subslice(&buf, b"\r\n\r\n") { break pos + 4; }
        if buf.len() > MAX_REQUEST { return Ok(buf); }
        let n = stream.read(&mut chunk)?;
        if n == 0 { return Ok(buf); }            // EOF before full headers → partial (parse → 400)
        buf.extend_from_slice(&chunk[..n]);
    };
    let want = head_end.saturating_add(parse_content_length(&buf[..head_end])).min(MAX_REQUEST);
    while buf.len() < want {
        let n = stream.read(&mut chunk)?;
        if n == 0 { break; }
        buf.extend_from_slice(&chunk[..n]);
    }
    Ok(buf)
}
fn parse_content_length(head: &[u8]) -> usize {
    let text = String::from_utf8_lossy(head);
    for line in text.split("\r\n") {
        if let Some((name, value)) = line.split_once(':') {
            if name.trim().eq_ignore_ascii_case("content-length") { return value.trim().parse().unwrap_or(0); }
        }
    }
    0
}
fn find_subslice(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() { return Some(0); }
    hay.windows(needle.len()).position(|w| w == needle)
}
```

## 5. `tests/serve.rs` (NEW) — conformance OUTSIDE the spine

A `FixtureTransport` impl of `Transport` (a `VecDeque<Vec<u8>>` of canned requests, a `Vec<Vec<u8>>`
of captured responses). Build a checked `Program` from an inline serve source via the public
front-end (`phorj::parser` + `phorj::cli::check_and_expand`, or add a tiny pub
`cli::parse_checked_program(src)` helper — `parse_checked` is currently private). Then:
- feed 2-3 canned raw requests (a known route, an unknown route → 404, a malformed buffer → 400),
- run `serve(&program, &mut fixture)`,
- assert each captured response equals the expected raw bytes AND equals calling the program's
  `respond` directly via `call_named` (self-consistency).
Keep it deterministic (no real socket). Optionally ONE `#[ignore]`-able real-socket smoke test that
binds `127.0.0.1:0`, spawns a thread, sends a request via `TcpStream`, asserts the response.

## 6. W4 (the next + final spike step) — `phg serve` CLI + PHP bridge + docs

- `main.rs`/`cli.rs`: add `serve <file> [--addr 127.0.0.1:8080]` — load the program (loader, like
  run), check, then `serve::serve_tcp(&program, addr)`. Per-command `--help` with an example.
  Built binaries still ignore argv; `serve` is a CLI-only subcommand.
- `examples/web/server.phg`: the W4 example — defines `respond(bytes) -> bytes` (reusing W1
  parse/serialize + W2 router) PLUS a `main()` that exercises `respond` on a `b"…"` fixture and
  prints a summary, so it stays byte-identity-gated on run/runvm/real-PHP like every example.
- **PHP front-controller bridge** (`examples/web/server.php` or a generated stub): transpile the
  program to PHP, add a thin front-controller that builds a `Request` from `$_SERVER`/`php://input`,
  calls `handle`, and `echo`es the serialized response — runnable under `php -S`. Document the
  dual-bridge story (native `phg serve` vs `php -S`) in `examples/web/README.md`.
- Update `CHANGELOG.md` (consolidated M6 web entry), `FEATURES.md` (serve row), `examples/README.md`.

## 7. THEN STOP — review + re-plan (the developer's explicit instruction)

After W4: a *functional* `phg serve app.phg` exists. **Stop for a code + plan review**, then re-plan
(fold in: M8 importer build, Track A closures unblocking core.list/middleware/path-params, the
language gaps named-args/variadics/union→enum, the parked "Phorj > PHP" benchmark).

## 8. Invariants honored

- **Determinism quarantine** — serve.rs is the only non-deterministic module; differential.rs never
  imports it; conformance via deterministic `Transport` fixture in tests/serve.rs.
- **No new `Op` / `Value` variant** — `call_named` reuses `run_call`; bytes↔socket is plain Rust.
- **`#![forbid(unsafe_code)]`, std-only** — `TcpListener`/`TcpStream`/`Read`/`Write`, no crates.
- **EV-7 no-crash** — request size capped; malformed/partial buffers flow to Phorj `parse_request`
  which returns `null` → a 400; faults degrade to a 500. Never panics on socket input.
