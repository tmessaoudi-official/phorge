//! M6 W3 — HTTP serve runtime. The ONE place sockets + wall-clock non-determinism live, kept
//! deliberately OUTSIDE the byte-identity spine: `tests/differential.rs` never imports this module —
//! its conformance is covered by `tests/serve.rs` over a deterministic in-memory [`Transport`].
//!
//! The portable unit stays `handle(Request) -> Response` (W1) *inside* the served program; the
//! runtime only shuttles raw bytes to a single Phorge entry **`respond(bytes) -> bytes`** ([`SERVE_ENTRY`])
//! and writes the result back. HTTP/1.1, `Connection: close`, one request per accepted connection.
//!
//! Single-threaded by FORCE: the `Rc`-shared heap (P5a) makes `Value` non-`Send`, so a thread pool
//! is impossible; real concurrency arrives with M6 green-threads under this unchanged contract.
use crate::ast::Program;
use crate::interpreter::call_named;
use crate::value::Value;
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::rc::Rc;
use std::time::Duration;

/// The default Phorge entry the runtime calls per request: `respond(bytes) -> bytes`.
pub const SERVE_ENTRY: &str = "respond";

/// Seam between the serve loop and the world. [`TcpTransport`] is the real socket; `tests/serve.rs`
/// swaps an in-memory transport (the env-update HTTP-fixture-seam pattern) so the loop needs no port
/// and stays deterministic.
pub trait Transport {
    /// Block for the next raw request, or `Ok(None)` when the source is exhausted (shutdown).
    fn recv(&mut self) -> io::Result<Option<Vec<u8>>>;
    /// Write the raw response for the request just `recv`'d, then end that exchange.
    fn send(&mut self, response: &[u8]) -> io::Result<()>;
}

/// If the transport reports this many consecutive errors with **no** successful request in between,
/// the listener is treated as unrecoverable and the loop ends. Transient per-connection failures
/// (client resets, slow-client read timeouts) are logged and skipped far below this bound, so one
/// hostile or broken client can never take the server down — GA blocker B3.
const MAX_CONSECUTIVE_TRANSPORT_ERRORS: usize = 64;

/// Serve requests from `transport`, routing each raw buffer through the program's
/// `respond(bytes) -> bytes`. **Resilient by design (GA blockers B3/B4):** a fault on one request
/// degrades to a 500, a `send` failure (client reset / broken pipe) is logged and skipped, and a
/// `recv` error (e.g. a transient `accept()`) is logged and retried — only `MAX_CONSECUTIVE_…` recv
/// errors in a row with no progress ends the loop. Returns `Ok` when the transport reports
/// exhaustion (`recv` → `Ok(None)`).
pub fn serve<T: Transport>(program: &Program, transport: &mut T) -> io::Result<()> {
    let mut consecutive_errors = 0usize;
    loop {
        match transport.recv() {
            Ok(Some(raw)) => {
                consecutive_errors = 0;
                let response = respond_once(program, &raw);
                if let Err(e) = transport.send(&response) {
                    // One client's broken pipe / reset must not end the server.
                    eprintln!("serve: send failed (connection dropped): {e}");
                }
            }
            Ok(None) => return Ok(()), // transport exhausted → graceful shutdown
            Err(e) => {
                consecutive_errors += 1;
                eprintln!("serve: connection error (skipped): {e}");
                if consecutive_errors >= MAX_CONSECUTIVE_TRANSPORT_ERRORS {
                    eprintln!(
                        "serve: {consecutive_errors} consecutive transport errors — listener \
                         appears unrecoverable, shutting down"
                    );
                    return Err(e);
                }
            }
        }
    }
}

/// Invoke `respond(bytes) -> bytes` once. Any captured stdout (a handler calling `console.println`)
/// is treated as a server log line and written to stderr, keeping the HTTP response body clean.
/// A non-`bytes` return or a runtime fault degrades to a 500 — never a panic (EV-7).
fn respond_once(program: &Program, raw: &[u8]) -> Vec<u8> {
    let arg = Value::Bytes(Rc::new(raw.to_vec()));
    match call_named(program, SERVE_ENTRY, vec![arg]) {
        Ok((Value::Bytes(b), out)) => {
            if !out.is_empty() {
                eprint!("{out}");
            }
            b.as_ref().clone()
        }
        Ok((other, _)) => {
            eprintln!(
                "serve: `{SERVE_ENTRY}` returned {}, expected bytes",
                other.type_name()
            );
            http_500()
        }
        Err(e) => {
            eprintln!("serve: request failed: {e}");
            http_500()
        }
    }
}

/// A minimal, well-formed `500 Internal Server Error` response (`Connection: close`).
fn http_500() -> Vec<u8> {
    let body = b"internal server error";
    let head = format!(
        "HTTP/1.1 500 Internal Server Error\r\nContent-Length: {}\r\nConnection: close\r\nContent-Type: text/plain\r\n\r\n",
        body.len()
    );
    head.into_bytes()
        .into_iter()
        .chain(body.iter().copied())
        .collect()
}

/// Production transport: a single-threaded `TcpListener`, one request per accepted connection
/// (`Connection: close`). `recv` *frames* the request (reads up to `\r\n\r\n`, then `Content-Length`
/// bytes) — framing only; the program's `parse_request` does the semantic parse.
pub struct TcpTransport {
    listener: TcpListener,
    current: Option<TcpStream>,
    /// Per-connection read/write timeout (slowloris guard, GA blocker B4). `None` = no timeout.
    timeout: Option<Duration>,
}

impl TcpTransport {
    /// Bind a listener (e.g. `"127.0.0.1:8080"`, or `":0"`-style `"127.0.0.1:0"` for an ephemeral port).
    pub fn bind(addr: &str) -> io::Result<Self> {
        Ok(Self {
            listener: TcpListener::bind(addr)?,
            current: None,
            timeout: None,
        })
    }
    /// Set the per-connection read/write timeout (GA blocker B4 — bounds a slow/idle client on the
    /// single-threaded server). `None` disables it (a slow client may then hold a connection
    /// indefinitely — only appropriate for trusted/loopback use).
    pub fn set_timeout(&mut self, timeout: Option<Duration>) {
        self.timeout = timeout;
    }
    /// The actually-bound address (useful when binding to port 0).
    pub fn local_addr(&self) -> io::Result<std::net::SocketAddr> {
        self.listener.local_addr()
    }
}

impl Transport for TcpTransport {
    fn recv(&mut self) -> io::Result<Option<Vec<u8>>> {
        // Accept connections until one yields a request. An `accept()` error propagates to the serve
        // loop's circuit breaker (it decides if the listener is unrecoverable). A per-connection read
        // error — a read timeout from a slow/idle client (B4), or a reset mid-headers — is logged and
        // the *next* connection is accepted, so one bad client cannot wedge the single-threaded
        // server (B3 + B4 together).
        loop {
            let (mut stream, _peer) = self.listener.accept()?;
            if let Some(t) = self.timeout {
                // Best-effort: a platform that rejects the timeout must not crash the server.
                let _ = stream.set_read_timeout(Some(t));
                let _ = stream.set_write_timeout(Some(t));
            }
            match read_http_request(&mut stream) {
                Ok(raw) => {
                    self.current = Some(stream);
                    return Ok(Some(raw));
                }
                Err(e) => {
                    eprintln!("serve: dropping connection (read error): {e}");
                    // loop: accept the next connection
                }
            }
        }
    }
    fn send(&mut self, response: &[u8]) -> io::Result<()> {
        if let Some(mut stream) = self.current.take() {
            stream.write_all(response)?;
            stream.flush()?;
        }
        Ok(()) // dropping the stream closes the connection (Connection: close)
    }
}

/// Bind `addr` and serve until killed — the blocking accept-loop `phg serve` calls (W4). `timeout`
/// is the per-connection read/write timeout (GA blocker B4); `None` disables it.
pub fn serve_tcp(program: &Program, addr: &str, timeout: Option<Duration>) -> io::Result<()> {
    let mut t = TcpTransport::bind(addr)?;
    t.set_timeout(timeout);
    eprintln!("phg serve: listening on http://{}", t.local_addr()?);
    match timeout {
        Some(d) => eprintln!(
            "phg serve: per-connection timeout {}s; single-threaded — bind 127.0.0.1 on untrusted networks",
            d.as_secs()
        ),
        None => eprintln!(
            "phg serve: no connection timeout (pass --timeout); single-threaded — bind 127.0.0.1 on untrusted networks"
        ),
    }
    serve(program, &mut t)
}

/// Cap a single request at 8 MiB — keeps a hostile or runaway client from exhausting memory (EV-7).
const MAX_REQUEST: usize = 8 * 1024 * 1024;

/// Read one HTTP/1.1 request from `stream`: everything up to and including `\r\n\r\n`, then the
/// `Content-Length` body (0 if absent). Capped at [`MAX_REQUEST`]. Framing only — no semantic
/// validation; a partial/malformed buffer flows to the program's `parse_request`, which returns
/// `null` and yields a 400. Generic over [`Read`] so the framing is unit-testable over a `Cursor`
/// (P1-d) without binding a socket.
fn read_http_request<R: Read>(stream: &mut R) -> io::Result<Vec<u8>> {
    const SEP: &[u8] = b"\r\n\r\n";
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    // Only re-scan newly-arrived bytes for the header terminator (with a `SEP.len()-1` overlap so a
    // terminator split across two reads is still found). Scanning the whole buffer every chunk is
    // O(n²) — a CPU-DoS on a large no-terminator request; this keeps it linear.
    let mut scanned = 0usize;
    let head_end = loop {
        let from = scanned.saturating_sub(SEP.len() - 1);
        if let Some(rel) = find_subslice(&buf[from..], SEP) {
            break from + rel + SEP.len();
        }
        scanned = buf.len();
        if buf.len() > MAX_REQUEST {
            return Ok(buf);
        }
        let n = stream.read(&mut chunk)?;
        if n == 0 {
            return Ok(buf); // EOF before full headers → partial (parse → 400)
        }
        buf.extend_from_slice(&chunk[..n]);
    };
    let want = head_end
        .saturating_add(parse_content_length(&buf[..head_end]))
        .min(MAX_REQUEST);
    while buf.len() < want {
        let n = stream.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    Ok(buf)
}

/// Parse the `Content-Length` header from a request head (0 if absent or unparseable).
fn parse_content_length(head: &[u8]) -> usize {
    let text = String::from_utf8_lossy(head);
    for line in text.split("\r\n") {
        if let Some((name, value)) = line.split_once(':') {
            if name.trim().eq_ignore_ascii_case("content-length") {
                return value.trim().parse().unwrap_or(0);
            }
        }
    }
    0
}

/// First index of `needle` in `hay`, or `None`. An empty needle matches at 0 (defensive; the only
/// caller passes the non-empty `\r\n\r\n`).
fn find_subslice(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    hay.windows(needle.len()).position(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    // --- find_subslice -----------------------------------------------------

    #[test]
    fn find_subslice_basics() {
        assert_eq!(find_subslice(b"abc\r\n\r\nxyz", b"\r\n\r\n"), Some(3));
        assert_eq!(find_subslice(b"no terminator here", b"\r\n\r\n"), None);
        assert_eq!(find_subslice(b"", b"\r\n\r\n"), None);
        assert_eq!(find_subslice(b"anything", b""), Some(0)); // empty needle → 0
    }

    // --- parse_content_length ---------------------------------------------

    #[test]
    fn content_length_absent_is_zero() {
        assert_eq!(
            parse_content_length(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n"),
            0
        );
    }

    #[test]
    fn content_length_present_is_parsed() {
        assert_eq!(
            parse_content_length(b"POST / HTTP/1.1\r\nContent-Length: 42\r\n\r\n"),
            42
        );
    }

    #[test]
    fn content_length_is_case_insensitive_and_trims() {
        assert_eq!(
            parse_content_length(b"POST / HTTP/1.1\r\ncOnTeNt-LeNgTh:   7  \r\n\r\n"),
            7
        );
    }

    #[test]
    fn content_length_malformed_is_zero() {
        // Non-numeric value parses to 0 (framing reads no body; the program's parser handles it).
        assert_eq!(
            parse_content_length(b"POST / HTTP/1.1\r\nContent-Length: not-a-number\r\n\r\n"),
            0
        );
    }

    // --- read_http_request (over a Cursor, no socket) ----------------------

    #[test]
    fn reads_headers_only_request() {
        let req = b"GET / HTTP/1.1\r\nHost: x\r\n\r\n".to_vec();
        let got = read_http_request(&mut Cursor::new(req.clone())).unwrap();
        assert_eq!(got, req);
    }

    #[test]
    fn reads_request_with_body() {
        let req = b"POST / HTTP/1.1\r\nContent-Length: 5\r\n\r\nhello".to_vec();
        let got = read_http_request(&mut Cursor::new(req.clone())).unwrap();
        assert_eq!(got, req, "head + the declared 5 body bytes");
    }

    #[test]
    fn eof_before_headers_returns_partial() {
        // No CRLFCRLF, then EOF → returns whatever was read (parse → 400 downstream), never hangs.
        let req = b"GET / HTTP/1.1 no terminator".to_vec();
        let got = read_http_request(&mut Cursor::new(req.clone())).unwrap();
        assert_eq!(got, req);
    }

    /// A reader that yields its data in fixed-size pieces — exercises the accumulation loop with the
    /// `\r\n\r\n` terminator split across multiple `read` calls.
    struct ChunkedReader {
        data: Vec<u8>,
        pos: usize,
        chunk: usize,
    }
    impl Read for ChunkedReader {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let remaining = &self.data[self.pos..];
            let n = remaining.len().min(self.chunk).min(buf.len());
            buf[..n].copy_from_slice(&remaining[..n]);
            self.pos += n;
            Ok(n)
        }
    }

    #[test]
    fn terminator_and_body_split_across_chunks() {
        let req = b"POST /x HTTP/1.1\r\nContent-Length: 3\r\n\r\nabc".to_vec();
        let mut r = ChunkedReader {
            data: req.clone(),
            pos: 0,
            chunk: 1, // one byte per read → terminator and body span many reads
        };
        let got = read_http_request(&mut r).unwrap();
        assert_eq!(got, req);
    }

    /// A reader that never produces a terminator — drives the [`MAX_REQUEST`] cap.
    struct InfiniteReader;
    impl Read for InfiniteReader {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            for b in buf.iter_mut() {
                *b = b'a';
            }
            Ok(buf.len())
        }
    }

    #[test]
    fn max_request_cap_terminates() {
        // No `\r\n\r\n` ever arrives; the read must stop near the cap rather than loop forever.
        let got = read_http_request(&mut InfiniteReader).unwrap();
        assert!(got.len() > MAX_REQUEST, "stopped at the cap");
        assert!(
            got.len() <= MAX_REQUEST + 4096,
            "no more than one chunk past the cap"
        );
    }
}
