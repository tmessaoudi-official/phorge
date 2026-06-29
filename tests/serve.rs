//! M6 W3 conformance — the serve runtime, checked OUTSIDE the byte-identity spine.
//!
//! `tests/differential.rs` never touches `src/serve.rs` (the determinism quarantine); this file
//! drives the serve loop over a deterministic in-memory [`Transport`] so no socket is needed. It
//! asserts each response is exactly the expected raw HTTP/1.1 bytes AND that the loop's output
//! equals calling the program's `respond(bytes) -> bytes` directly (self-consistency). One
//! `#[ignore]`d smoke test exercises the real `TcpTransport` end to end.
use std::collections::VecDeque;
use std::rc::Rc;

use phorj::interpreter::call_named;
use phorj::serve::{serve, Transport};
use phorj::value::Value;

/// A small but complete serve program: W1-style parse/serialize + a 2-route dispatch + the single
/// `respond(bytes) -> bytes` entry (malformed → 400, all in pure Phorj). `main` keeps it a valid
/// `package Main` entry, but the tests call `respond`/`serve`, never `main`.
const SERVE_PROGRAM: &str = r#"
package Main;
import Core.Console;
import Core.Bytes;
import Core.Text;

class Request {
  constructor(public string method, public string path, public bytes body) {}
}
class Response {
  constructor(public int status, public bytes body, public List<string> headerLines) {}
}

function reasonPhrase(int s) -> string {
  return if (s == 200) { "OK" }
    else { if (s == 400) { "Bad Request" }
    else { if (s == 404) { "Not Found" }
    else { "Internal Server Error" } } };
}

function parseRequest(bytes raw) -> Request? {
  string nl = Bytes.toString(b"\x0d\x0a") ?? "";
  int sep = Bytes.find(raw, b"\x0d\x0a\x0d\x0a") ?? -1;
  if (sep < 0) { return null; }
  bytes headBytes = Bytes.slice(raw, 0, sep);
  bytes body = Bytes.slice(raw, sep + 4, Bytes.length(raw));
  string head = Bytes.toString(headBytes) ?? "";
  List<string> lines = Text.split(head, nl);
  string requestLine = lines[0];
  List<string> rl = Text.split(requestLine, " ");
  string method = rl[0];
  string path = rl[1];
  return new Request(method, path, body);
}

function serializeResponse(Response resp) -> bytes {
  string nl = Bytes.toString(b"\x0d\x0a") ?? "";
  string reason = reasonPhrase(resp.status);
  int st = resp.status;
  string statusLine = "HTTP/1.1 {st} {reason}";
  int bodyLen = Bytes.length(resp.body);
  string userHeaders = Text.join(resp.headerLines, nl);
  string head = "{statusLine}{nl}Content-Length: {bodyLen}{nl}{userHeaders}{nl}{nl}";
  return Bytes.concat(Bytes.fromString(head), resp.body);
}

function dispatch(Request req) -> Response {
  if (req.method == "GET") {
    if (req.path == "/") {
      return new Response(200, Bytes.fromString("home"), ["Content-Type: text/plain"]);
    }
  }
  return new Response(404, Bytes.fromString("not found"), ["Content-Type: text/plain"]);
}

function badRequest() -> Response {
  return new Response(400, Bytes.fromString("bad request"), ["Content-Type: text/plain"]);
}

function respond(bytes raw) -> bytes {
  if (var req = parseRequest(raw)) {
    return serializeResponse(dispatch(req));
  } else {
    return serializeResponse(badRequest());
  }
}

function main() -> void {
  bytes raw = b"GET / HTTP/1.1\x0d\x0aHost: localhost\x0d\x0a\x0d\x0a";
  int len = Bytes.length(respond(raw));
  Console.println("served {len} bytes");
}
"#;

/// slice B1: a Core.Http program that defines ONLY `handle(Request) -> Response` — no parse/serialize,
/// no `respond`. The Core.Http injection supplies `Request`/`Response` and synthesizes the
/// `respond(bytes) -> bytes` serve bridge that wraps `handle` (malformed → 400). Closes Batch-1 C: a
/// bare handler is directly servable.
const HTTP_HANDLE_PROGRAM: &str = r#"
package Main;
import Core.Http;
function handle(Request req) -> Response {
  if (req.path == "/") {
    return Response.text(200, "home");
  }
  return Response.text(404, "missing");
}
function main() -> void { }
"#;

/// Deterministic in-memory transport: `recv` pops a canned request; `send` records the response.
struct FixtureTransport {
    inbox: VecDeque<Vec<u8>>,
    sent: Vec<Vec<u8>>,
}
impl FixtureTransport {
    fn new(requests: Vec<Vec<u8>>) -> Self {
        Self {
            inbox: requests.into_iter().collect(),
            sent: Vec::new(),
        }
    }
}
impl Transport for FixtureTransport {
    fn recv(&mut self) -> std::io::Result<Option<Vec<u8>>> {
        Ok(self.inbox.pop_front())
    }
    fn send(&mut self, response: &[u8]) -> std::io::Result<()> {
        self.sent.push(response.to_vec());
        Ok(())
    }
}

fn program() -> phorj::ast::Program {
    phorj::cli::parse_checked_program(SERVE_PROGRAM).expect("serve program type-checks")
}

/// Build the exact raw HTTP/1.1 response the program emits (the serializer always recomputes
/// Content-Length, lists `Content-Type` as the sole user header, then CRLFCRLF + body).
fn http(status_line: &str, body: &str) -> Vec<u8> {
    format!(
        "{status_line}\r\nContent-Length: {}\r\nContent-Type: text/plain\r\n\r\n{body}",
        body.len()
    )
    .into_bytes()
}

#[test]
fn serves_known_unknown_and_malformed() {
    let prog = program();
    let get_root = b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n".to_vec();
    let get_missing = b"GET /missing HTTP/1.1\r\nHost: localhost\r\n\r\n".to_vec();
    let malformed = b"GET / HTTP/1.1 no terminator".to_vec();

    let mut fx = FixtureTransport::new(vec![
        get_root.clone(),
        get_missing.clone(),
        malformed.clone(),
    ]);
    serve(&prog, &mut fx, false).expect("serve loop completes");

    assert_eq!(fx.sent.len(), 3, "one response per request");
    assert_eq!(fx.sent[0], http("HTTP/1.1 200 OK", "home"));
    assert_eq!(fx.sent[1], http("HTTP/1.1 404 Not Found", "not found"));
    assert_eq!(fx.sent[2], http("HTTP/1.1 400 Bad Request", "bad request"));

    // Self-consistency: the serve loop's output equals calling `respond` directly.
    for (req, expected) in [
        (get_root, &fx.sent[0]),
        (get_missing, &fx.sent[1]),
        (malformed, &fx.sent[2]),
    ] {
        let (v, out) =
            call_named(&prog, "respond", vec![Value::Bytes(Rc::new(req))]).expect("respond ok");
        assert!(out.is_empty(), "respond emits no stdout");
        match v {
            Value::Bytes(b) => assert_eq!(b.as_ref(), expected),
            other => panic!("respond returned {}, expected bytes", other.type_name()),
        }
    }
}

#[test]
fn core_http_handle_is_servable_via_injected_respond_bridge() {
    // The program has no `respond` of its own — the Core.Http injection supplies it, wrapping `handle`.
    let prog = phorj::cli::parse_checked_program(HTTP_HANDLE_PROGRAM)
        .expect("Core.Http handle program type-checks");
    let get_root = b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n".to_vec();
    let get_missing = b"GET /missing HTTP/1.1\r\nHost: localhost\r\n\r\n".to_vec();
    let malformed = b"not a request".to_vec();

    let mut fx = FixtureTransport::new(vec![
        get_root.clone(),
        get_missing.clone(),
        malformed.clone(),
    ]);
    serve(&prog, &mut fx, false).expect("serve loop completes");

    assert_eq!(fx.sent.len(), 3, "one response per request");
    assert_eq!(fx.sent[0], http("HTTP/1.1 200 OK", "home"));
    assert_eq!(fx.sent[1], http("HTTP/1.1 404 Not Found", "missing"));
    // A malformed request → the injected bridge's 400 (body "Bad Request").
    assert_eq!(fx.sent[2], http("HTTP/1.1 400 Bad Request", "Bad Request"));
}

/// A transport with a scripted sequence of `recv` results (including errors), so the loop's
/// resilience (GA blocker B3) can be tested deterministically without a socket.
struct ScriptedTransport {
    recvs: VecDeque<std::io::Result<Option<Vec<u8>>>>,
    sent: Vec<Vec<u8>>,
}
impl Transport for ScriptedTransport {
    fn recv(&mut self) -> std::io::Result<Option<Vec<u8>>> {
        self.recvs.pop_front().unwrap_or(Ok(None))
    }
    fn send(&mut self, response: &[u8]) -> std::io::Result<()> {
        self.sent.push(response.to_vec());
        Ok(())
    }
}

/// Type-check an inline program for the degradation tests below.
fn checked(src: &str) -> phorj::ast::Program {
    phorj::cli::parse_checked_program(src).expect("program type-checks")
}

/// B3: a per-connection `recv` error (client reset, transient accept) is logged and skipped — the
/// surrounding good request is still served and the loop ends cleanly on `Ok(None)`.
#[test]
fn recv_error_does_not_kill_the_loop() {
    let prog = program();
    let good = b"GET / HTTP/1.1\r\nHost: x\r\n\r\n".to_vec();
    let mut t = ScriptedTransport {
        recvs: VecDeque::from(vec![
            Err(std::io::Error::new(
                std::io::ErrorKind::ConnectionReset,
                "reset",
            )),
            Ok(Some(good)),
            Err(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "pipe")),
            Ok(None),
        ]),
        sent: Vec::new(),
    };
    serve(&prog, &mut t, false).expect("loop survives per-connection errors and ends cleanly");
    assert_eq!(
        t.sent.len(),
        1,
        "the one good request was served despite surrounding errors"
    );
    assert_eq!(t.sent[0], http("HTTP/1.1 200 OK", "home"));
}

/// B3: a listener that only ever errors (unrecoverable) eventually shuts the loop down via the
/// consecutive-error circuit breaker, rather than spinning forever.
#[test]
fn unrecoverable_listener_eventually_stops() {
    let prog = program();
    let recvs = (0..1000)
        .map(|_| Err(std::io::Error::other("listener dead")))
        .collect();
    let mut t = ScriptedTransport {
        recvs,
        sent: Vec::new(),
    };
    assert!(
        serve(&prog, &mut t, false).is_err(),
        "a listener that only errors must eventually end the loop"
    );
    assert!(t.sent.is_empty(), "nothing could be served");
}

/// P1-e: a request that *faults* inside `respond` degrades to a 500 and the loop continues to the
/// next request (one bad request never aborts the server).
#[test]
fn respond_fault_degrades_to_500_and_loop_continues() {
    let prog = checked(
        "package Main;\nfunction respond(bytes raw) -> bytes { List<bytes> xs = [raw]; return xs[5]; }\n",
    );
    let req = b"GET / HTTP/1.1\r\n\r\n".to_vec();
    let mut fx = FixtureTransport::new(vec![req.clone(), req]);
    serve(&prog, &mut fx, false).expect("loop completes despite per-request faults");
    assert_eq!(
        fx.sent.len(),
        2,
        "both faulting requests answered; loop continued"
    );
    for resp in &fx.sent {
        assert!(
            resp.starts_with(b"HTTP/1.1 500 Internal Server Error"),
            "a request fault degrades to 500, got: {}",
            String::from_utf8_lossy(&resp[..resp.len().min(40)])
        );
    }
}

/// P1-e: a `respond` that returns a non-`bytes` value also degrades to a 500 (the runtime never
/// trusts the return type — it checks the actual value).
#[test]
fn respond_non_bytes_return_degrades_to_500() {
    let prog = checked("package Main;\nfunction respond(bytes raw) -> int { return 7; }\n");
    let mut fx = FixtureTransport::new(vec![b"GET / HTTP/1.1\r\n\r\n".to_vec()]);
    serve(&prog, &mut fx, false).expect("loop completes");
    assert_eq!(fx.sent.len(), 1);
    assert!(fx.sent[0].starts_with(b"HTTP/1.1 500 Internal Server Error"));
}

#[test]
fn unknown_entry_reports_cleanly() {
    let prog = program();
    let err = call_named(&prog, "no_such_fn", vec![]).expect_err("missing entry is an error");
    assert!(err.to_string().contains("no `no_such_fn` function"));
}

#[test]
fn tcp_smoke() {
    use phorj::serve::TcpTransport;
    use std::io::{Read, Write};
    use std::net::TcpStream;

    let prog = program();
    let mut t = TcpTransport::bind("127.0.0.1:0").expect("bind ephemeral port");
    let addr = t.local_addr().expect("addr");
    // Detached server thread: serves the one connection we make, then blocks on the next accept
    // (harmless — the process exits at end of test).
    std::thread::spawn(move || {
        let _ = serve(&prog, &mut t, false);
    });

    let mut s = TcpStream::connect(addr).expect("connect");
    s.write_all(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n")
        .expect("write request");
    let mut resp = Vec::new();
    s.read_to_end(&mut resp).expect("read response");
    assert_eq!(resp, http("HTTP/1.1 200 OK", "home"));
}

/// S4.1 — HTTP/1.1 keep-alive: two requests on ONE socket get two responses (the connection is reused,
/// not closed after the first). Keep-alive requires a configured timeout (the idle-socket guard), so the
/// transport is bound with one. Each response is self-delimiting (`Content-Length`), so the client reads
/// exactly one response's worth of bytes per request.
#[test]
fn tcp_keepalive_serves_two_requests_on_one_socket() {
    use phorj::serve::TcpTransport;
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::time::Duration;

    let prog = program();
    let mut t = TcpTransport::bind("127.0.0.1:0").expect("bind ephemeral port");
    t.set_timeout(Some(Duration::from_secs(5))); // keep-alive only with a timeout (idle guard)
    let addr = t.local_addr().expect("addr");
    std::thread::spawn(move || {
        let _ = serve(&prog, &mut t, false);
    });

    let expected = http("HTTP/1.1 200 OK", "home");
    let mut s = TcpStream::connect(addr).expect("connect");
    s.set_read_timeout(Some(Duration::from_secs(5)))
        .expect("client timeout");
    // Two HTTP/1.1 requests (no `Connection: close`) on the SAME socket.
    for i in 0..2 {
        s.write_all(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n")
            .unwrap_or_else(|e| panic!("write request {i}: {e}"));
        let mut resp = vec![0u8; expected.len()];
        s.read_exact(&mut resp)
            .unwrap_or_else(|e| panic!("read response {i} on the kept-alive socket: {e}"));
        assert_eq!(resp, expected, "response {i}");
    }
}

/// S4.1 — `Connection: close` closes the socket after one response even with keep-alive available: the
/// client's `read_to_end` returns exactly one response and then EOF (the server drops the socket).
#[test]
fn tcp_connection_close_closes_after_one_response() {
    use phorj::serve::TcpTransport;
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::time::Duration;

    let prog = program();
    let mut t = TcpTransport::bind("127.0.0.1:0").expect("bind ephemeral port");
    t.set_timeout(Some(Duration::from_secs(5)));
    let addr = t.local_addr().expect("addr");
    std::thread::spawn(move || {
        let _ = serve(&prog, &mut t, false);
    });

    let mut s = TcpStream::connect(addr).expect("connect");
    s.set_read_timeout(Some(Duration::from_secs(5)))
        .expect("client timeout");
    s.write_all(b"GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
        .expect("write request");
    let mut resp = Vec::new();
    s.read_to_end(&mut resp).expect("read response then EOF");
    assert_eq!(resp, http("HTTP/1.1 200 OK", "home"));
}

/// M6 W3 — the worker pool serves many concurrent connections correctly. 24 clients hit a 4-worker
/// pool at once; every one must get the exact `home` response (correctness under concurrency — no
/// deadlock, no interleaved/corrupted responses, no lost connection). Real sockets on an ephemeral
/// port. Robust by construction (asserts correctness of all responses, not flaky wall-clock overlap).
#[test]
fn pool_serves_concurrent_connections() {
    use phorj::serve::serve_pool;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};

    let prog = program();
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    let addr = listener.local_addr().expect("addr");
    // Detached 4-worker pool (blocks forever in accept; the process exits at end of test).
    std::thread::spawn(move || {
        let _ = serve_pool(listener, &prog, None, false, 4);
    });

    let expected = http("HTTP/1.1 200 OK", "home");
    let clients: Vec<_> = (0..24)
        .map(|_| {
            std::thread::spawn(move || {
                let mut s = TcpStream::connect(addr).expect("connect");
                s.write_all(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n")
                    .expect("write");
                let mut resp = Vec::new();
                s.read_to_end(&mut resp).expect("read");
                resp
            })
        })
        .collect();

    for (i, c) in clients.into_iter().enumerate() {
        let resp = c.join().expect("client thread");
        assert_eq!(
            resp, expected,
            "concurrent client {i} got the wrong response"
        );
    }
}

/// S4.2 — graceful shutdown: after a request is served, flipping the shutdown flag makes
/// `serve_pool_with` stop accepting, drain in-flight work, **join every worker**, and return `Ok` —
/// no abrupt cut, no hang. (The `join` blocks the test until the drain completes, so a regression that
/// failed to drain/return would surface as a hang the harness times out.)
#[test]
fn pool_graceful_shutdown_drains_and_returns() {
    use phorj::serve::serve_pool_with;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    let prog = program();
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    let addr = listener.local_addr().expect("addr");
    let flag = Arc::new(AtomicBool::new(false));
    let server_flag = Arc::clone(&flag);
    let server = std::thread::spawn(move || {
        serve_pool_with(
            listener,
            &prog,
            Some(Duration::from_secs(5)),
            false,
            2,
            Some(server_flag),
        )
    });

    // One request completes normally before shutdown.
    {
        let mut s = TcpStream::connect(addr).expect("connect");
        s.write_all(b"GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
            .expect("write");
        let mut resp = Vec::new();
        s.read_to_end(&mut resp).expect("read");
        assert_eq!(resp, http("HTTP/1.1 200 OK", "home"));
    }

    // Signal shutdown; the pool must drain, join workers, and return Ok.
    flag.store(true, Ordering::SeqCst);
    let joined = server.join().expect("server thread panicked");
    assert!(
        joined.is_ok(),
        "graceful shutdown returns Ok, got {joined:?}"
    );
}

/// S4.1 — the worker pool also keeps connections alive (when a timeout is configured): two requests on
/// one socket served by the same worker get two responses. Exercises the pool's per-connection
/// keep-alive loop (a separate code path from the single-threaded `TcpTransport`).
#[test]
fn pool_keepalive_serves_two_requests_on_one_socket() {
    use phorj::serve::serve_pool;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::time::Duration;

    let prog = program();
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    let addr = listener.local_addr().expect("addr");
    std::thread::spawn(move || {
        let _ = serve_pool(listener, &prog, Some(Duration::from_secs(5)), false, 2);
    });

    let expected = http("HTTP/1.1 200 OK", "home");
    let mut s = TcpStream::connect(addr).expect("connect");
    s.set_read_timeout(Some(Duration::from_secs(5)))
        .expect("client timeout");
    for i in 0..2 {
        s.write_all(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n")
            .unwrap_or_else(|e| panic!("write request {i}: {e}"));
        let mut resp = vec![0u8; expected.len()];
        s.read_exact(&mut resp)
            .unwrap_or_else(|e| panic!("read response {i} on the kept-alive pool socket: {e}"));
        assert_eq!(resp, expected, "pool response {i}");
    }
}
