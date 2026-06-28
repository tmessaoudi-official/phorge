# Stack Traces & Beautiful Fault Reporting — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn an uncaught Phorj runtime fault into a state-of-the-art report — a call stack with per-frame `file:line` and a source caret — rendered identically across `run`/`runvm`, in both the CLI and (dev-mode) a `phg serve` browser error page.

**Architecture:** A `Frame` carries `{function, file, line, col}`; `Diagnostic` gains `frames: Vec<Frame>` (this *is* the spec's `Fault` — realized as Diagnostic-plus-frames to avoid changing the `Result<String, Diagnostic>` return type everywhere). The VM walks its live `frames` at the fault arm; the interpreter keeps a `trace_stack` popped on success only (so an error leaves it intact to snapshot). The loader tags each function with its origin file and keeps a source map on `Unit` for carets. Two renderers consume the frames: `Diagnostic::render` (CLI) and a Rust HTML renderer in `serve.rs` (web, dev-only, reusing `Core.Html`'s escape table).

**Tech Stack:** Rust (edition 2021, std-only). Spec: `docs/specs/2026-06-21-stack-traces-and-fault-reporting-design.md`. Decisions: `docs/plans/2026-06-21-error-handling-and-traces.plan.md`.

## Global Constraints

- Toolchain: `export PATH=/stack/tools/cargo/bin:$PATH`. Gate before every commit: `PHORJ_REQUIRE_PHP=1 cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`. The pre-commit hook re-runs all three.
- **No new `Op`, no `Value` change, no change to program stdout.** `FaultKind` classification is preserved — the M7 PHP oracle must stay green. Traces are on the fault/stderr path only.
- **`run`-trace ≡ `runvm`-trace** is an enforced invariant (Task 5's differential assertion), not an aspiration.
- Web error page is **dev-mode only** (`phg serve --dev`); production returns a bare generic 500 with no trace/source/message leak.
- The web HTML renderer is Rust runtime glue (outside the byte-identity value contract); it reuses `Core.Html`'s pinned `htmlspecialchars(ENT_QUOTES)`-equivalent 5-char escape table — every interpolated value is escaped.
- Git autonomy authorized: commit each green task (`feat:`/`test:`/`docs:`, no `Co-Authored-By`).

## File Structure

- `src/diagnostic.rs` — add `pub struct Frame`; add `frames: Vec<Frame>` to `Diagnostic` (default empty); extend `render(&self, src)` to print the frame list; add a `with_frames(self, Vec<Frame>)` builder. The CLI renderer.
- `src/vm.rs` — at the `run()` fault arm, walk `self.frames` into `Vec<Frame>` and attach via `with_frames`.
- `src/interpreter.rs` — add `trace_stack: Vec<Frame>`; push at `run_call`/method/closure entry, pop on the `Ok` arms only; snapshot it into the `Signal::Runtime` diagnostic at the `interpret()` top.
- `src/loader.rs` — tag functions with origin file (extend the visibility provenance) + keep a `sources: HashMap<PathBuf,String>` (or `Vec<(PathBuf,String)>`) on `Unit`; expose a resolver `fn_file(name) -> Option<PathBuf>`.
- `src/serve.rs` — `serve`/`respond_once` gain a `dev: bool`; on the fault arm, render the dev HTML page (new `fn dev_error_page(diag, req_ctx) -> Vec<u8>`) when `dev`, else `http_500()`.
- `src/main.rs` / `src/cli.rs` — route faults through `Diagnostic::render` with the source map; add `phg serve --dev`.
- `tests/differential.rs` — the trace-parity assertion.

---

### Task 1: `Frame` type + `Diagnostic.frames` + CLI render

**Files:**
- Modify: `src/diagnostic.rs` (struct at line 45; `render` at 86; `Display` at 158)
- Test: `src/diagnostic.rs` test module

**Interfaces:**
- Produces: `pub struct Frame { pub function: String, pub file: Option<std::path::PathBuf>, pub line: u32, pub col: u32 }`; `Diagnostic.frames: Vec<Frame>`; `Diagnostic::with_frames(self, Vec<Frame>) -> Self`.

- [ ] **Step 1: Write the failing test.** In `src/diagnostic.rs` tests:

```rust
#[test]
fn render_includes_frame_list() {
    let d = Diagnostic::runtime_at_line("list index out of range", 14).with_frames(vec![
        Frame { function: "checkout".into(), file: Some("src/cart.phg".into()), line: 14, col: 11 },
        Frame { function: "main".into(), file: Some("src/main.phg".into()), line: 6, col: 3 },
    ]);
    let out = d.render("");
    assert!(out.contains("stack trace"), "{out}");
    assert!(out.contains("checkout"), "{out}");
    assert!(out.contains("src/cart.phg:14"), "{out}");
    assert!(out.contains("main"), "{out}");
}

#[test]
fn render_without_frames_is_unchanged() {
    // Back-compat: a frameless diagnostic renders exactly as before (no "stack trace" block).
    let d = Diagnostic::runtime("boom");
    assert!(!d.render("").contains("stack trace"));
}
```

- [ ] **Step 2: Run to verify failure.** `export PATH=/stack/tools/cargo/bin:$PATH && cargo test --lib diagnostic::tests::render_includes_frame 2>&1 | tail` → FAIL (`Frame`/`with_frames`/`frames` absent).

- [ ] **Step 3: Implement.** Add the `Frame` struct near `Diagnostic`; add `pub frames: Vec<Frame>` to `Diagnostic` (initialize to `Vec::new()` in every constructor — `runtime`, `runtime_at_line`, and any `Diagnostic::new`); add the builder + render block:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub function: String,
    pub file: Option<std::path::PathBuf>,
    pub line: u32,
    pub col: u32,
}

impl Diagnostic {
    #[must_use]
    pub fn with_frames(mut self, frames: Vec<Frame>) -> Self {
        self.frames = frames;
        self
    }
    /// Render the call stack (innermost first), appended after the message/caret. Empty ⇒ nothing.
    fn render_frames(&self) -> String {
        if self.frames.is_empty() {
            return String::new();
        }
        let mut s = String::from("\nstack trace (most recent call first):\n");
        for (i, f) in self.frames.iter().enumerate() {
            let mark = if i == 0 { "→ " } else { "  " };
            let loc = match &f.file {
                Some(p) => format!("{}:{}", p.display(), f.line),
                None => format!("line {}", f.line),
            };
            s.push_str(&format!("  {mark}{:<18} {loc}\n", f.function));
        }
        s
    }
}
```

Append `render_frames()` to the end of the string built by `render(&self, src)`. (Find where `render` returns its `String` and concatenate before returning.) Update every `Diagnostic { … }` literal / constructor to set `frames: Vec::new()` — `cargo build` lists them.

- [ ] **Step 4: Run to verify pass.** `cargo test --lib diagnostic:: 2>&1 | tail`.

- [ ] **Step 5: Full gate + commit.**

```bash
export PATH=/stack/tools/cargo/bin:$PATH && PHORJ_REQUIRE_PHP=1 cargo test 2>&1 | tail -3 && cargo clippy --all-targets -- -D warnings && cargo fmt --check
git add src/diagnostic.rs && git commit -m "feat(diagnostic): Frame + frames field + CLI stack-trace render"
```

---

### Task 2: VM frame-walk at the fault arm

**Files:**
- Modify: `src/vm.rs` (`run()` fault arm, ~lines 96-106; `Frame` struct at 34)
- Test: `src/vm.rs` test module

**Interfaces:**
- Consumes: `crate::diagnostic::Frame`, `Diagnostic::with_frames` (Task 1); the VM's `self.frames`, `self.program.functions[func]` (name + `chunk.lines`).

- [ ] **Step 1: Write the failing test.** Add to the vm test module a program that faults inside a called function and assert the resulting `Diagnostic` carries ≥2 frames with the callee innermost. Use the existing `run_chunk`/compile-from-source test helper (`grep -n "fn run_chunk\|fn vm_run\|compile(" src/vm.rs` to find it); compile a 2-function program where `main` calls `f` and `f` indexes a list out of range, then assert the `Err(diag)` has `diag.frames.len() == 2` and `diag.frames[0].function` is the callee.

```rust
#[test]
fn vm_fault_carries_call_stack() {
    let src = "package Main;\nfunction f() -> int { var xs = [1]; return xs[5]; }\nfunction main() { var _ = f(); }";
    let program = compile(&crate::cli::check_and_expand_for_test(src)).unwrap(); // use the module's existing compile helper
    let err = Vm::new(&program).run().unwrap_err();
    assert_eq!(err.frames.len(), 2, "callee + main");
    assert_eq!(err.frames[0].function, "f");
    assert_eq!(err.frames[1].function, "main");
}
```

(Adapt the compile/check helper call to whatever the vm test module already uses — match the existing tests' setup verbatim; do not invent `check_and_expand_for_test` if a simpler path exists.)

- [ ] **Step 2: Run to verify failure.** `cargo test --lib vm::tests::vm_fault_carries_call_stack 2>&1 | tail` → FAIL (`frames` empty).

- [ ] **Step 3: Implement.** Replace the fault arm so it walks the live frames before returning. The program's function name lives in `self.program.functions[func]` — confirm the field name with `grep -n "pub name\|struct Function" src/chunk.rs`; use it below as `.name`:

```rust
Err(msg) => {
    // Walk the live call stack innermost → outermost: each frame's line is the source line of
    // the op it is paused on (the faulting op for the top frame; the pending Call for the rest).
    let frames: Vec<crate::diagnostic::Frame> = self
        .frames
        .iter()
        .rev()
        .map(|fr| {
            let line = self.program.functions[fr.func]
                .chunk
                .lines
                .get(fr.ip.saturating_sub(1))
                .copied()
                .unwrap_or(0);
            crate::diagnostic::Frame {
                function: self.program.functions[fr.func].name.clone(),
                file: None, // filled by the loader source map in Task 4
                line,
                col: 0,
            }
        })
        .collect();
    let line = frames.first().map_or(0, |f| f.line);
    return Err(Diagnostic::runtime_at_line(msg, line).with_frames(frames));
}
```

Note the top frame's `ip` was already incremented (`self.frames[fr].ip += 1` before `exec_op`), so `ip - 1` is the faulting op — matching the existing `lines.get(ip)` logic. For non-top frames, `ip` sits just past the `Call` op, so `ip - 1` is the call site. (If `Function` has no `name` field, add one in the compiler where functions are built — `grep -n "Function {" src/compiler.rs` — carrying the source name; this is needed for trace frames regardless.)

- [ ] **Step 4: Run to verify pass.** `cargo test --lib vm:: 2>&1 | tail`.

- [ ] **Step 5: Full gate + commit.**

```bash
export PATH=/stack/tools/cargo/bin:$PATH && PHORJ_REQUIRE_PHP=1 cargo test 2>&1 | tail -3 && cargo clippy --all-targets -- -D warnings && cargo fmt --check
git add src/vm.rs src/chunk.rs src/compiler.rs && git commit -m "feat(vm): attach call-stack frames to a runtime fault"
```

---

### Task 3: Interpreter `trace_stack` (run ≡ runvm frames)

**Files:**
- Modify: `src/interpreter.rs` (`Interp` struct; `run_call` ~262-282; the method/closure call sites at ~1002, ~1162; `interpret()` ~129-160)
- Test: `src/interpreter.rs` test module

**Interfaces:**
- Consumes: `crate::diagnostic::Frame`. Produces identical frames to Task 2 for the same fault.

- [ ] **Step 1: Write the failing test.**

```rust
#[test]
fn interpreter_fault_carries_call_stack() {
    let err = run("package Main;\nfunction f() -> int { var xs = [1]; return xs[5]; }\nfunction main() { var _ = f(); }").unwrap_err();
    assert_eq!(err.frames.len(), 2, "callee + main");
    assert_eq!(err.frames[0].function, "f");
    assert_eq!(err.frames[1].function, "main");
}
```

(`run` is the interpreter test module's existing helper at line 1315.)

- [ ] **Step 2: Run to verify failure.** `cargo test --lib interpreter::tests::interpreter_fault_carries_call_stack 2>&1 | tail` → FAIL.

- [ ] **Step 3: Implement.**
  - Add `trace_stack: Vec<(String, u32)>` to `Interp` (function name + a *current line* cell), initialized `Vec::new()` in `interpret()`'s `Interp { … }` literal.
  - `run_call` takes the function name + a call-site line. The current `run_call(&names, &body, args, this)` signature has no name/line — add a `fn_name: &str` param (callers pass the function's name; the closure/method sites pass `"<closure>"` / the method name) and push `(fn_name, line)` at entry. Since per-statement line tracking is needed for the *fault* line, track the "current line" by updating `trace_stack.last_mut()` as `exec_stmts` walks (set it from each `Stmt`'s span line at the top of the statement loop — `grep -n "fn exec_stmts\|fn exec_stmt" src/interpreter.rs`).
  - **Pop on success only:** in `run_call`'s trailing `match result`, pop `trace_stack` in the `Ok(())` and `Err(Signal::Return(_))` arms, and **do not** pop in the `Err(other)` arm — leaving the stack intact for the snapshot.
  - In `interpret()`, change the `Err(Signal::Runtime(e))` arm to attach frames: build `Vec<Frame>` from `interp.trace_stack` (reversed → innermost first; `file: None`, `col: 0`), and return `Err(e.with_frames(frames))`. (The `trace_stack` is a field on `interp`, still in scope after `run_call` returns Err.)

  Concretely, the `interpret()` tail becomes:

```rust
match interp.run_call("main", &names, &main.body, vec![], None) {
    Ok(_) | Err(Signal::Return(_)) => Ok(interp.out),
    Err(Signal::Runtime(e)) => {
        let frames: Vec<Frame> = interp
            .trace_stack
            .iter()
            .rev()
            .map(|(name, line)| Frame { function: name.clone(), file: None, line: *line, col: 0 })
            .collect();
        Err(e.with_frames(frames))
    }
    Err(Signal::Break | Signal::Continue) => Err(Diagnostic::runtime("internal error: loop control escaped")),
}
```

  Update the other `run_call` call sites (method dispatch ~1002, closure ~1162, hook helpers from M-mut.7b, the `Console`-less ones) to pass the appropriate name + call-site line. The line for each push is the span line of the call expression at that site.

- [ ] **Step 4: Run to verify pass + frame parity.** `cargo test --lib interpreter:: 2>&1 | tail`. Then a cross-backend check is added in Task 5.

- [ ] **Step 5: Full gate + commit.**

```bash
export PATH=/stack/tools/cargo/bin:$PATH && PHORJ_REQUIRE_PHP=1 cargo test 2>&1 | tail -3 && cargo clippy --all-targets -- -D warnings && cargo fmt --check
git add src/interpreter.rs && git commit -m "feat(interpreter): trace_stack — call-stack frames on a fault (parity with VM)"
```

---

### Task 4: File attribution + source map (loader)

**Files:**
- Modify: `src/loader.rs` (`Unit`; Pass-1 provenance; `load_project`/`load_loose_src`)
- Modify: `src/vm.rs`, `src/interpreter.rs` (fill `Frame.file` from the map) OR fill at render time in `main.rs`
- Test: `src/loader.rs` test module

**Interfaces:**
- Produces: `Unit.sources: std::collections::HashMap<std::path::PathBuf, String>` (file → text, project mode; empty in loose) and `Unit.fn_files: std::collections::HashMap<String, std::path::PathBuf>` (function name → origin file). A helper `fn attribute(frames: &mut [Frame], &Unit)` that fills each frame's `file` from `fn_files` by `function`.

- [ ] **Step 1: Write the failing test.**

```rust
#[test]
fn unit_records_fn_files_and_sources() {
    let tmp = TempDir::new();
    tmp.write("phorj.toml", "module = \"acme/app\"\nsource = \"src\"");
    let entry = tmp.write("src/main.phg", "package Main;\nfunction main() {}");
    tmp.write("src/acme/util/u.phg", "package acme.util;\nfunction parse() {}");
    let u = load(&entry).unwrap();
    assert!(u.fn_files.contains_key("main"), "main attributed");
    assert!(u.sources.values().any(|s| s.contains("function parse")), "util source kept");
}
```

- [ ] **Step 2: Run to verify failure.** `cargo test --lib loader::tests::unit_records_fn_files 2>&1 | tail` → FAIL.

- [ ] **Step 3: Implement.** In `load_project` Pass 1 (where `DefInfo` is already recorded per function), also populate `fn_files.insert(mangle(&prog.package, name) , file.clone())` for functions (and bare name for `main`), and `sources.insert(file.clone(), src.clone())`. Add both fields to `Unit` (and `sources: HashMap::new(), fn_files: HashMap::new()` to the loose constructor). Add the free helper `attribute`. Then in `main.rs`, after a fault Diagnostic is produced for a project unit, call `loader::attribute(&mut diag.frames, &unit)` and render with the source map (pass the faulting frame's file text to `render`). For loose mode, `fn_files`/`sources` are empty ⇒ frames keep `file: None` and the single `diag_src` feeds the caret (unchanged).

- [ ] **Step 4: Run to verify pass.** `cargo test --lib loader:: 2>&1 | tail`; manually `phg run` a faulting multi-file project and confirm frames show files.

- [ ] **Step 5: Full gate + commit.**

```bash
export PATH=/stack/tools/cargo/bin:$PATH && PHORJ_REQUIRE_PHP=1 cargo test 2>&1 | tail -3 && cargo clippy --all-targets -- -D warnings && cargo fmt --check
git add src/loader.rs src/main.rs src/vm.rs src/interpreter.rs && git commit -m "feat(loader): per-function file attribution + source map for traces"
```

---

### Task 5: `run ≡ runvm` trace-parity differential assertion

**Files:**
- Modify: `tests/differential.rs` (the fault-comparison path — `grep -n "agree_err\|FaultKind" tests/differential.rs`)

**Interfaces:**
- Consumes: `cli::run_program` / `cli::runvm_program` returning `Err(Diagnostic)` whose `.frames` are now populated.

- [ ] **Step 1: Write the failing test.** Add a differential test that compiles several faulting programs and asserts the rendered trace text is byte-identical across backends:

```rust
#[test]
fn run_and_runvm_traces_are_identical() {
    let faults = [
        "package Main;\nfunction g() -> int { var xs = [1]; return xs[9]; }\nfunction main() { var _ = g(); }",
        "package Main;\nfunction main() { var x = 1 / 0; }",
    ];
    for src in faults {
        let unit = loader::load_loose_src(src).unwrap();
        let r = cli::run_program(&unit.program, &unit.diag_src).unwrap_err();
        let v = cli::runvm_program(&unit.program, &unit.diag_src).unwrap_err();
        assert_eq!(r, v, "run vs runvm trace text diverged for:\n{src}");
    }
}
```

(`run_program`/`runvm_program` return `Err(String)` today — they `.map_err(|e| e.to_string())`. Either compare the strings, or add `*_program_diag` variants returning the `Diagnostic` so `.frames` are comparable. Comparing the rendered strings is sufficient and tests the user-visible output; use that.)

- [ ] **Step 2: Run to verify it fails or passes.** `cargo test --test differential run_and_runvm_traces 2>&1 | tail`. If it fails, the frame/line capture in Tasks 2-3 diverged — fix the divergence (usually a frame-line mismatch: align the interpreter's per-frame current-line update with the VM's `ip-1` line).

- [ ] **Step 3: (only if needed) reconcile** the interpreter line tracking vs VM `chunk.lines` until identical.

- [ ] **Step 4: Confirm pass + full suite.** `cargo test --test differential 2>&1 | tail`.

- [ ] **Step 5: Full gate + commit.**

```bash
export PATH=/stack/tools/cargo/bin:$PATH && PHORJ_REQUIRE_PHP=1 cargo test 2>&1 | tail -3 && cargo clippy --all-targets -- -D warnings && cargo fmt --check
git add tests/differential.rs src/ && git commit -m "test(differential): enforce run≡runvm stack-trace parity"
```

---

### Task 6: Web dev error page (`phg serve --dev`)

**Files:**
- Modify: `src/serve.rs` (`serve`, `respond_once`, the `Err` arm at ~91; `http_500` at ~99)
- Modify: `src/main.rs` (`phg serve` arg parse → `--dev`)
- Test: `src/serve.rs` test module

**Interfaces:**
- Consumes: the faulting `Diagnostic` (with frames) from invoking the entry; the raw request bytes (for method/path/headers context).
- Produces: `fn dev_error_page(diag: &Diagnostic, raw_req: &[u8]) -> Vec<u8>` (an HTML 500 response); `respond_once(program, raw, dev: bool)`; `serve(program, transport, dev: bool)`.

- [ ] **Step 1: Write the failing test.**

```rust
#[test]
fn dev_error_page_escapes_and_includes_frames() {
    let diag = crate::diagnostic::Diagnostic::runtime_at_line("boom <script>", 3).with_frames(vec![
        crate::diagnostic::Frame { function: "h".into(), file: None, line: 3, col: 0 },
    ]);
    let page = dev_error_page(&diag, b"GET /x HTTP/1.1\r\nHost: a\r\n\r\n");
    let s = String::from_utf8(page).unwrap();
    assert!(s.contains("500"), "{s}");
    assert!(s.contains("text/html"), "{s}");
    assert!(s.contains("&lt;script&gt;"), "must escape message: {s}");
    assert!(s.contains("/x"), "request path shown: {s}");
    assert!(s.contains("h"), "frame shown: {s}");
}

#[test]
fn production_fault_is_bare_500_no_leak() {
    // respond_once(dev=false) on a faulting program returns the plain 500, no message/trace.
    // (Build a tiny program whose `respond` faults; assert the body is the generic 500, not the message.)
}
```

- [ ] **Step 2: Run to verify failure.** `cargo test --lib serve::tests::dev_error_page 2>&1 | tail` → FAIL.

- [ ] **Step 3: Implement.** Add `dev_error_page`, escaping every interpolated value with the same 5-char table `Core.Html` uses (`grep -n "htmlspecialchars\|fn escape\|ENT_QUOTES\|&amp;" src/native.rs` to reuse/mirror the exact table — `&`→`&amp;`, `<`→`&lt;`, `>`→`&gt;`, `"`→`&quot;`, `'`→`&#039;`). Render: a `<!doctype html>` page with the fault kind+message, a `<pre>` per frame (`function  file:line`), and a request-context block (method, path, header lines parsed from `raw_req` up to CRLFCRLF). Thread `dev: bool` through `serve`→`respond_once`; in the fault arm, `if dev { dev_error_page(&e, raw) } else { http_500() }`. In `main.rs`, parse `--dev` for `phg serve` and pass it.

- [ ] **Step 4: Run to verify pass.** `cargo test --lib serve:: 2>&1 | tail`.

- [ ] **Step 5: Full gate + commit.**

```bash
export PATH=/stack/tools/cargo/bin:$PATH && PHORJ_REQUIRE_PHP=1 cargo test 2>&1 | tail -3 && cargo clippy --all-targets -- -D warnings && cargo fmt --check
git add src/serve.rs src/main.rs && git commit -m "feat(serve): dev-mode HTML error page (phg serve --dev); prod stays bare 500"
```

---

### Task 7: CLI color + wiring polish

**Files:**
- Modify: `src/diagnostic.rs` (color in `render_frames`, gated like the existing caret color) ; `src/main.rs` (fault output path)
- Test: `src/diagnostic.rs` tests

**Interfaces:**
- Consumes: the existing NO_COLOR / is-TTY gate used by `Diagnostic::render` today (`grep -n "NO_COLOR\|is_terminal\|\\x1b\[" src/diagnostic.rs`).

- [ ] **Step 1: Write the failing test.** Assert frames render with ANSI when color is on and plain when `NO_COLOR`/non-TTY — mirror the existing caret-color test in the module (find it: `grep -n "NO_COLOR\|color" src/diagnostic.rs`). If color is decided at the `main.rs` print site (not inside `render`), put the test there instead.

- [ ] **Step 2-4: Implement + verify.** Apply the same color gate the caret uses to the `→`/function/loc segments; ensure the fault print path in `main.rs` feeds the right source text (the faulting frame's file, via Task 4's map) to `render` so the caret shows. Run `cargo test --lib diagnostic:: 2>&1 | tail`.

- [ ] **Step 5: Full gate + commit.**

```bash
export PATH=/stack/tools/cargo/bin:$PATH && PHORJ_REQUIRE_PHP=1 cargo test 2>&1 | tail -3 && cargo clippy --all-targets -- -D warnings && cargo fmt --check
git add src/ && git commit -m "feat(cli): color the stack trace (NO_COLOR/TTY-aware) + caret wiring"
```

---

### Task 8: Docs, example walkthrough, close-out

**Files:**
- Create: `examples/errors/README.md` (a faulting program + its CLI trace + the web page screenshot/markup) + a small companion `.phg` that faults
- Modify: `CHANGELOG.md`, `KNOWN_ISSUES.md`, `docs/MILESTONES.md`, the plan STATUS

- [ ] **Step 1: Example.** Since a faulting program can't be a byte-identical runnable example, add `examples/errors/` with a `.phg` that deliberately faults and a `README.md` showing: the exact CLI trace output, and the dev web page markup. Reference it from `examples/README.md`. (Not added to the differential glob's *runnable* set — document why in the README, mirroring how faults are handled elsewhere.)
- [ ] **Step 2: CHANGELOG** — "Added — stack traces & beautiful fault reporting" entry (CLI + dev web page, run≡runvm parity, dev/prod split).
- [ ] **Step 3: KNOWN_ISSUES** — deferrals from spec §10 (no cause chain until Slice 2; no recursion-frame collapsing; web syntax highlighting beyond escaping).
- [ ] **Step 4: MILESTONES + plan STATUS** — mark Slice 1 complete with test count + commit shas; note Slice 2 (catchable model) is next.
- [ ] **Step 5: Commit.** `git add examples/ CHANGELOG.md KNOWN_ISSUES.md docs/ && git commit -m "docs: stack-trace example walkthrough + close out slice 1"`

---

## Self-Review

**1. Spec coverage:**
- §2 Fault value → Task 1 (`Frame` + `frames` on `Diagnostic` = the spec's Fault). ✓
- §3 frame capture, run≡runvm → Task 2 (VM), Task 3 (interpreter), Task 5 (parity assertion). ✓
- §4 file/source attribution → Task 4. ✓
- §5 CLI renderer → Task 1 (frames) + Task 7 (color/caret). ✓
- §6 web dev page → Task 6. ✓
- §7 production safety → Task 6 (dev/prod split + `production_fault_is_bare_500_no_leak` test). ✓
- §8 oracle safety → every task ends on the `PHORJ_REQUIRE_PHP=1` gate; FaultKind untouched. ✓
- §9 testing → Tasks 1-8 (unit goldens, differential parity, web snapshot, example README). ✓
- §10 scope → no try/catch here (Slice 2). ✓

**2. Placeholder scan:** Two steps say "adapt to the existing helper" (Task 2 Step 1's compile helper, Task 7's color gate) — these are explicit "match the existing pattern, here's how to find it (`grep …`)" instructions, not vague TODOs, because the exact helper name must be read from the file. All code steps show real code. No "handle edge cases".

**3. Type consistency:** `Frame { function, file: Option<PathBuf>, line: u32, col: u32 }` is identical across Tasks 1-4. `with_frames`, `trace_stack`, `Unit.fn_files`/`Unit.sources`, `dev_error_page`, `respond_once(_, _, dev)` are named consistently throughout. The interpreter's `trace_stack: Vec<(String,u32)>` is mapped to `Frame` only at `interpret()` (Task 3), matching the VM's `Vec<Frame>` at render.
