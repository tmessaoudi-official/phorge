# Phorge Architecture

A one-page map of the codebase. For the *rules* that keep it correct see `docs/INVARIANTS.md`; for
the load-bearing decisions see `docs/adr/`, and for the fuller design rationale see `docs/specs/`
(see "Decision records" below).

## Pipeline

```
source .phg
  │
  ▼  lexer.rs        (iterative; &str → Vec<Token>)
tokens
  │
  ▼  parser.rs       (recursive descent; Vec<Token> → ast::Program)   depth-guarded: MAX_NEST_DEPTH
AST (untyped)
  │
  ▼  checker.rs      (type-check gate; validates, does NOT annotate)  depth-guarded: MAX_EXPR_DEPTH
AST (validated)
  │
  ├─▶ interpreter.rs     tree-walker            → stdout   ┐ reference semantics (the oracle)
  │                                                        │
  ├─▶ compiler.rs ─▶ chunk.rs (Op/Chunk) ─▶ vm.rs          │ bytecode backend; byte-identical to ──┐
  │       AST → BytecodeProgram        stack VM → stdout    │ the interpreter (differential spine)  │
  │                                                         │                                       │
  └─▶ transpile.rs       AST → PHP source       → stdout   ┘ runs under real PHP, byte-identical ───┘
```

The whole pipeline runs on a 256 MB worker thread (`cli::on_deep_stack`) so the explicit depth
limits in `limits.rs`, not Rust's ambient stack, bound recursion (invariant #6).

## Modules (`src/`)

| File | Role |
|------|------|
| `lexer.rs` / `token.rs` | source → tokens; `Span` = source-position truth |
| `parser.rs` / `ast.rs` | tokens → untyped AST |
| `checker.rs` | type-check gate (no annotation) |
| `interpreter.rs` | tree-walking evaluator — the reference semantics |
| `compiler.rs` | AST → `BytecodeProgram` |
| `chunk.rs` | `Op`, `Chunk` (code + consts + line table), `BytecodeProgram` + `validate` |
| `vm.rs` | stack VM; `exec_op` dispatch; reified call `Frame { func, ip, slot_base }` |
| `transpile.rs` | AST → PHP source |
| `value.rs` | `Value` + single-sourced arith/compare kernels (both backends) |
| `native.rs` | namespaced stdlib registry keyed by `(module,name)` (`console.println`, …); single-sources each native's checker sig + shared `eval` + PHP emission; target of `import core.*` + `Op::CallNative` |
| `diagnostic.rs` | unified `Diagnostic { stage, message, line, col }` |
| `limits.rs` | recursion/nesting caps + numeric-width policy |
| `mem.rs` | std-only Linux `/proc` RSS sampler (`VmRSS`/`VmHWM` + `clear_refs` peak reset) for `bench` |
| `serve.rs` | M6 HTTP serve runtime — `Transport` seam + `TcpTransport`; the determinism quarantine (outside `tests/differential.rs`, covered by `tests/serve.rs`) |
| `cli.rs` / `main.rs` | command pipelines (`run`/`runvm`/`check`/`parse`/`lex`/`transpile`/`disasm`/`bench`/`build`/`vendor`/`serve`/`explain`) + thin dispatcher |

## Two `Frame`s — not the same thing
`vm::Frame` is a reified call record (`{func, ip, slot_base}`) on an explicit frame stack — the
future green-thread substrate (M2.5/M6). `interpreter::CallScopes` is the *block-scope chain* of
the currently executing call; the tree-walker keeps its call records on the native Rust stack. They
are opposite concepts (the rename in P3.5 removed the old name collision).

## Backends today vs. planned
Three backends exist as **free functions** dispatched by a string `match` in `main.rs`
(`cmd_run`/`cmd_runvm`/`cmd_transpile`). There is no `Backend` trait yet (`grep 'trait ' src/` = 0);
it is deferred to the 4th backend (`phg build`, M2.5) per the Rule of Three — see ecosystem
spec E-1.

## Decision records
Phorge keeps architecture decisions in three complementary places:
- **`docs/adr/`** — the canonical one-page **Architecture Decision Records** for the load-bearing
  decisions: the *verdict + consequences*, in Nygard format, **immutable once Accepted**. Start here.
- **`docs/specs/`** — frozen design docs with numbered decision logs (e.g. the M2 VM design's
  `## 11. Decisions Log`, the language and ecosystem specs). These hold the *fuller design
  exploration* each ADR links back to.
- **`docs/plans/*.md`** — per-milestone plans, each with a `## Decisions Log` capturing
  implementation-time choices.

**Authority split:** an ADR is canonical for the *decision*; the spec is canonical for the
*exploration* behind it. A new decision gets an ADR (and may also extend the relevant spec/plan log);
a reversal **supersedes** the ADR rather than editing it. See `docs/adr/README.md`.

> History: through M8 these decisions lived only in `docs/specs/` + `docs/plans/` (no `adr/` tree),
> on the reasoning that a standalone set would duplicate them. M9 split that: a 7–16 KB design spec
> is a design *document*, not a discoverable one-page *record* — so the ADRs now distill the verdict
> while the specs keep the exploration. See [ADR-0001](adr/0001-no-shared-run-vm-ir.md).
