# Track Q — Observability (roadmap-completeness raw report)

## Track summary

Phorj's observability story today is **fault-reporting only**, and only one direction of it: the
in-progress stack-traces slice (`docs/specs/2026-06-21-stack-traces-and-fault-reporting-design.md`)
renders an uncaught-fault call stack identically on `run`/`runvm`, surfaced in the CLI and in a
`phg serve --dev` HTML 500 page (prod = bare 500, no leak). `src/serve.rs` writes operational
events (request failures, client resets, slow-client timeouts) to **Rust `eprintln!`/stderr** — there
is no Phorj-level logging primitive, no structured-event facility, and the stdlib registry
(`src/native.rs`) has **no `Core.Time`, `Core.Log`, `Core.Env`, or `Core.Process` module**. There is
no metrics surface, no tracing/span concept, no runtime reflection/introspection beyond `phg disasm`
(a compile-time bytecode dump), and no health/readiness endpoint for the server. Catchable errors
(`try`/catch` vs `Result`) are explicitly a *later* slice, which is the prerequisite for any
recover-and-report (crash-reporting) story.

The philosophy lens (pragmatic, legible PHP-upgrade; map to idiomatic PHP; PHP-absent features are
compile-time-only/erased) cuts cleanly here: PHP devs reach for **PSR-3 logging** (`error_log`,
Monolog), `register_shutdown_function`/`set_exception_handler` for crash capture, `microtime(true)`/
`hrtime()` for timing, and `Reflection*` for introspection. The strong adopts are the ones with a
direct, boring PHP target (a PSR-3-shaped `Core.Log`, a `Core.Time` clock, a health/readiness route
helper, structured request logging on `serve`). The defers are the genuinely heavy distributed-systems
machinery (OpenTelemetry spans, a metrics registry, a panic/recovery handler) — each needs a
prerequisite (the catchable error model, concurrency, or a network client all deferred past M6) and
none earns its surprise budget before a real `serve` workload exists. Most distributed-tracing
maximalism is a clean **reject/defer**: it is not a PHP-familiar idiom and not provably-safe-making.

## Gaps

| id | title | kind | fit | rec | milestone | effort |
|---|---|---|---|---|---|---|
| Q-corelog | `Core.Log` PSR-3 leveled logging | port | strong | adopt | M11 (stdlib breadth) | M |
| Q-coretime | `Core.Time` clock (wall + monotonic) | port | strong | adopt | M11 (stdlib breadth) | S |
| Q-serve-reqlog | Structured request/access logging on `phg serve` | new | strong | adopt | M6 W4 | M |
| Q-serve-health | Health / readiness route helper for `serve` | new | strong | adopt | M6 W4 | S |
| Q-panic-shutdown | Crash capture: shutdown/uncaught-fault hook → report | port | ok | defer | post error-model | M |
| Q-reflection | Runtime reflection / introspection API | port | weak | defer | M-RT follow-up / v2 | L |
| Q-tracing-spans | Distributed tracing / spans (OTel-shaped) | new | weak | reject | — | L |
| Q-metrics | Metrics registry (counters / gauges / histograms) | new | weak | defer | post-concurrency (M6+) | L |
| Q-log-context | Structured/contextual log fields (key-value, MDC) | new | ok | defer | with Q-corelog v2 | M |
| Q-trace-color | Colorized / source-context stack-trace output | map | ok | reject | — | S |
| Q-trace-frames | Full `file:line` for method/ctor/closure frames | port | ok | defer | stack-traces slice follow-up | M |
| Q-env-introspect | `Core.Env` / runtime config introspection | port | ok | defer | M11 (stdlib breadth) | S |
| Q-debug-dump | `dump`/`debug` value-inspection builtin (var_dump) | port | ok | defer | M11 (stdlib breadth) | S |
| Q-serve-metrics-ep | Built-in `/metrics`-style scrape endpoint | new | weak | reject | — | M |

## Rationale per ADOPT item

**Q-corelog — `Core.Log` PSR-3 leveled logging.** This is the single most PHP-familiar observability
gap. Every PHP dev knows PSR-3 (`LoggerInterface`, the eight RFC-5424 levels: debug/info/notice/
warning/error/critical/alert/emergency) and Monolog. A `Core.Log` native module with
`debug`/`info`/`warning`/`error`/… each taking a `string` (and later a context map) maps *directly* to
PHP `error_log()` / a PSR-3 logger and erases cleanly — no new `Op`, no runtime-type machinery, the
same `(module,name)` native registry path already used by `Core.Console`. It is legible, boring, and
makes a `serve` workload debuggable in production today (right now the only Phorj-level output channel
is `Console.println` to stdout, which is wrong for diagnostics). Effort M because it needs a level enum
and a default sink (stderr) plus the transpile mapping; sits naturally in M11 stdlib breadth alongside
`core.json`/`core.list`.

**Q-coretime — `Core.Time` clock.** Observability is impossible without timestamps and durations.
`Core.Time` with `now() -> int` (unix seconds), `nowMillis()`, and a monotonic `elapsed`/`hrtime`
pair maps directly to PHP `time()`/`microtime(true)`/`hrtime(true)` and erases trivially. It is the
foundation Q-serve-reqlog and any future timing/metric work depends on. The one design care item is
**determinism**: wall-clock time is non-deterministic and would break the byte-identical example spine,
exactly like the URL/network deferral — so `Core.Time` must be excluded from the differential oracle
(no time-using example enrolled), the same quarantine already applied to fault cases and floats.
Effort S: a handful of natives, no new types.

**Q-serve-reqlog — structured request/access logging on `phg serve`.** Today `serve.rs` only
`eprintln!`s *failures*; there is no access log of normal requests (method, path, status, duration).
This is runtime glue (already outside the byte-identity spine, like the rest of the transport), so it
is free to be non-deterministic. A one-line-per-request access log (optionally JSON via a
`--log-format=json` flag) is what every PHP dev expects from `php -S` or an FPM access log, and it is
the concrete payoff that makes Q-corelog/Q-coretime worth shipping. Effort M; lands with M6 W4 (`phg
serve` CLI + front-controller), the natural home for serve-operability polish.

**Q-serve-health — health / readiness route helper.** A production HTTP server needs a liveness/
readiness endpoint (Kubernetes probes, load-balancer health checks). Because Phorj's server model is
the pure `handle(Request) -> Response` value function, this is *already expressible* in user code (a
route returning 200 on `/healthz`) — so the gap is a **convenience helper**, not a capability: a tiny
`serve`-level default `/healthz` (overridable) or a documented pattern + example. Keeping it as a thin
helper over the existing handler contract honors "removes surprises, never capability" and adds no new
surface. Effort S; same M6 W4 home as Q-serve-reqlog. It is `adopt` rather than `omit` only because
the *default route + example* is the legible thing a PHP/Go dev expects out of the box.

## Critic pass

Re-checked shipped state directly: `src/native.rs` registry has **no** `Core.Time`/`Core.Log`/
`Core.Env`/`Core.Process`/`Core.Debug` module (only Bytes/Console/File/Html/List/Map/Math/Set/Text) —
[Verified: `grep -oE '"Core\.[A-Za-z]+"' src/native.rs`]; `src/serve.rs` only `eprintln!`s *failures*,
never normal-request access lines — [Verified: read serve.rs lines 38–93, 253–264]; there is **no
`assert` keyword**, no `debug_backtrace`-style trace-as-value, and no runtime log-level/verbosity
control — [Verified: greps over lexer/parser/native]. The compile-time **warning channel**
(`check()` returns `Vec<Diagnostic>`, `W-FORCE-UNWRAP`) exists but is front-end-only — there is no
*runtime* warning/log emission path — [Verified: `src/checker.rs` lines 83–85, 165–176, 3730].

**Mis-listings found: none.** Every original gap is genuinely not-yet-shipped; nothing in the list is
already implemented. (`removed_mislisted = 0`.)

**Newly-found items (the long tail the first pass missed):**

| id | title | kind | fit | rec | milestone | effort |
|---|---|---|---|---|---|---|
| Q-assert | `assert(cond, msg)` dev-time contract check | port | strong | adopt | M11 (stdlib breadth) | S |
| Q-loglevel | Runtime log-level threshold / filtering for `Core.Log` | new | ok | defer | with Q-corelog (M11) | S |
| Q-exitcode | Documented CLI / `serve` exit-code contract | new | ok | defer | M9 (CI/docs) | S |
| Q-debug-trace | `Core.Debug.trace()` — capture current call stack as a value | port | weak | defer | post error-model slice | M |
| Q-log-sink | Pluggable log sink / output target (file, custom) | new | weak | reject | — | M |

**Q-assert — `assert(cond, msg)` dev-time contract check.** PHP `assert()` is a universally-recognized
idiom for "this must be true or the program is broken" — a legible, boring upgrade. Phorj already has
a clean-fault model (checked arithmetic, OOB index, `opt!` force-unwrap all fault byte-identically on
both backends via `Op::Fault`), so a Phorj `assert(cond)` / `assert(cond, "msg")` lowers to exactly
that existing fault path — **no new `Op`** (the S2 `Op::Fault(FaultMsg)` generalization already exists),
a tiny checker rule (the condition must be `bool`), and transpiles to PHP `assert()`. It is *fault*-
domain so it's naturally quarantined from the byte-identity oracle like every other fault. The one
design question — whether asserts compile out in a release build (PHP `zend.assertions`) — is a clean
later refinement; ship the always-on form first. Strong fit, effort S. This is arguably a *stronger*
adopt than Q-debug-dump and sits in the same M11 stdlib-breadth wave. The first pass entirely omitted
it despite it being the most PHP-familiar observability/correctness primitive after logging.

**Q-loglevel — runtime log-level threshold.** Q-corelog ships the eight PSR-3 *levels* but the first
report says nothing about *filtering* by a runtime threshold (PHP `error_reporting()` / a Monolog
handler level / a `LOG_LEVEL` env var). A leveled logger without a threshold logs everything — the
controllability half PHP devs expect. It is a thin addition (a module-level minimum-level state +
compare), depends on Q-corelog landing first, and pairs naturally with Q-env-introspect (read the
threshold from `getenv("LOG_LEVEL")`). Defer to ship *with* Q-corelog as part of the same module, not
a separate milestone. Effort S, `ok` fit (it's a refinement of an adopted item, not a standalone gap).

**Q-exitcode — documented CLI / serve exit-code contract.** `src/main.rs` already exits `0`/`1`/`2`
(success / runtime fault / usage-or-compile error) — [Verified: `grep exit src/main.rs`] — but this is
an *undocumented* behavior, not a stated observability contract. CI pipelines and shell scripts (the
PHP-CLI/Go-binary norm) depend on stable exit codes to observe outcomes. The capability exists; the gap
is **documentation + a stability guarantee**, not code — so it's a low-effort M9 (CI/docs) item, not a
language feature. Defer (it's documentation hardening, sequenced with the M9 doc-SSOT work). Effort S,
`ok` fit.

**Q-debug-trace — `Core.Debug.trace()` capturing the current stack as a value.** Distinct from the
uncaught-fault trace (Q-trace-frames/Q-trace-color, which is the *abort* path): PHP `debug_backtrace()`
returns the *live* call stack as a data structure you can log mid-execution without faulting. It maps
to PHP cleanly, but (1) it needs a `Frame`/trace *value type* to return (gated on the same generic/
value-formatter path as Q-debug-dump, and ideally the typed-frame model the stack-traces slice
defers), and (2) returning a structured stack as a first-class value is mild dynamic-introspection
surface that should follow, not precede, the error model. Weak-to-ok fit; defer to a post-error-model
follow-up of the stack-traces work. Effort M. The first pass covered fault-trace *rendering* but missed
the *programmatic capture* idiom.

**Q-log-sink — pluggable log sink / output target.** Monolog's defining feature is *handlers* (write
to a file, syslog, a stream, multiple targets). A configurable sink interface is the natural "make it
production-grade" pull after Q-corelog. **Reject for the core**, though: a sink abstraction is a
user-library concern built on the `handle`-shaped value contract + `Core.File.write` (already shipped)
+ a future writer interface — baking a handler registry into the language adds surface without earning
its budget, and PHP devs get this from Monolog (a library), not the PHP *core*. Core ships the default
stderr sink (Q-corelog) and a level threshold (Q-loglevel); everything beyond is library territory.
Weak fit, effort M, reject — same reasoning as Q-tracing-spans/Q-serve-metrics-ep (ecosystem add-on,
not a core idiom).

**Sanity-check of the original recommendations against philosophy:** all hold. The four strong adopts
(Core.Log, Core.Time, serve req-log, serve health) have direct, boring PHP targets and are exactly the
"removes surprises, never capability" shape. The rejects (OTel spans, metrics scrape endpoint, trace
color) are correctly rejected — spans/metrics are heavyweight ecosystem machinery that need
concurrency/network (both deferred) and aren't PHP-core idioms, and trace-color contradicts a *recorded
design decision* (plain diagnostics). Q-reflection's `defer/weak` is right — full `ReflectionClass`
clashes with the erasure model; only a principled subset (`typeName(value)`) could earn a place, which
Q-debug-trace/Q-debug-dump partly cover. One nuance: Q-debug-dump is correctly `port/ok/defer`, but
its `var_dump`/`var_export` framing should note that a *typed structural* dump over erased generics
will print `mixed`-shaped data (a known consequence of erasure), not a refined generic type — a doc
caveat, not a blocker.
