//! M6 W3 conformance — the serve runtime, checked OUTSIDE the byte-identity spine.
//!
//! `tests/differential.rs` never touches `src/serve.rs` (the determinism quarantine); this file
//! drives the serve loop over a deterministic in-memory [`Transport`] so no socket is needed. It
//! asserts each response is exactly the expected raw HTTP/1.1 bytes AND that the loop's output
//! equals calling the program's `respond(bytes) -> bytes` directly (self-consistency). One
//! `#[ignore]`d smoke test exercises the real `TcpTransport` end to end.
use std::collections::VecDeque;
use std::rc::Rc;

use phorge::interpreter::call_named;
use phorge::serve::{serve, Transport};
use phorge::value::Value;

/// A small but complete serve program: W1-style parse/serialize + a 2-route dispatch + the single
/// `respond(bytes) -> bytes` entry (malformed → 400, all in pure Phorge). `main` keeps it a valid
/// `package main` entry, but the tests call `respond`/`serve`, never `main`.
const SERVE_PROGRAM: &str = r#"
package main;
import core.console;
import core.bytes;
import core.text;

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
  string nl = bytes.toString(b"\x0d\x0a") ?? "";
  int sep = bytes.find(raw, b"\x0d\x0a\x0d\x0a") ?? -1;
  if (sep < 0) { return null; }
  bytes headBytes = bytes.slice(raw, 0, sep);
  bytes body = bytes.slice(raw, sep + 4, bytes.len(raw));
  string head = bytes.toString(headBytes) ?? "";
  List<string> lines = text.split(head, nl);
  string requestLine = lines[0];
  List<string> rl = text.split(requestLine, " ");
  string method = rl[0];
  string path = rl[1];
  return Request(method, path, body);
}

function serializeResponse(Response resp) -> bytes {
  string nl = bytes.toString(b"\x0d\x0a") ?? "";
  string reason = reasonPhrase(resp.status);
  int st = resp.status;
  string statusLine = "HTTP/1.1 {st} {reason}";
  int bodyLen = bytes.len(resp.body);
  string userHeaders = text.join(resp.headerLines, nl);
  string head = "{statusLine}{nl}Content-Length: {bodyLen}{nl}{userHeaders}{nl}{nl}";
  return bytes.concat(bytes.fromString(head), resp.body);
}

function dispatch(Request req) -> Response {
  if (req.method == "GET") {
    if (req.path == "/") {
      return Response(200, bytes.fromString("home"), ["Content-Type: text/plain"]);
    }
  }
  return Response(404, bytes.fromString("not found"), ["Content-Type: text/plain"]);
}

function badRequest() -> Response {
  return Response(400, bytes.fromString("bad request"), ["Content-Type: text/plain"]);
}

function respond(bytes raw) -> bytes {
  if (var req = parseRequest(raw)) {
    return serializeResponse(dispatch(req));
  } else {
    return serializeResponse(badRequest());
  }
}

function main() {
  bytes raw = b"GET / HTTP/1.1\x0d\x0aHost: localhost\x0d\x0a\x0d\x0a";
  int len = bytes.len(respond(raw));
  console.println("served {len} bytes");
}
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

fn program() -> phorge::ast::Program {
    phorge::cli::parse_checked_program(SERVE_PROGRAM).expect("serve program type-checks")
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
    serve(&prog, &mut fx).expect("serve loop completes");

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
fn unknown_entry_reports_cleanly() {
    let prog = program();
    let err = call_named(&prog, "no_such_fn", vec![]).expect_err("missing entry is an error");
    assert!(err.to_string().contains("no `no_such_fn` function"));
}

#[test]
#[ignore = "binds a real TCP socket; run with `cargo test --test serve -- --ignored`"]
fn tcp_smoke() {
    use phorge::serve::TcpTransport;
    use std::io::{Read, Write};
    use std::net::TcpStream;

    let prog = program();
    let mut t = TcpTransport::bind("127.0.0.1:0").expect("bind ephemeral port");
    let addr = t.local_addr().expect("addr");
    // Detached server thread: serves the one connection we make, then blocks on the next accept
    // (harmless — the process exits at end of test).
    std::thread::spawn(move || {
        let _ = serve(&prog, &mut t);
    });

    let mut s = TcpStream::connect(addr).expect("connect");
    s.write_all(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n")
        .expect("write request");
    let mut resp = Vec::new();
    s.read_to_end(&mut resp).expect("read response");
    assert_eq!(resp, http("HTTP/1.1 200 OK", "home"));
}
