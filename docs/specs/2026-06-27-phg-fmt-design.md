# `phg fmt` — a comment-preserving formatter (design)

> Status: **design — pending developer confirmation** (2026-06-27). Goal: a canonical-form formatter
> (`gofmt`/`rustfmt` shaped) for `.phg`. Plan slot: GA-sequence item 2.

## The blocker that shapes everything
The lexer **discards comments** (`lexer/mod.rs` `skip_line_comment`/`skip_block_comment`) — they never
reach the AST. The existing `src/lift/printer.rs` (an AST→source printer, reused from lift) therefore
**cannot** be a formatter as-is: a round-trip would **delete every comment**. So the entire design
problem is *comment (trivia) preservation*. Whitespace/blank-line normalization is the easy part; the
printer already produces canonical layout.

## Key decisions (recommendation + rationale; **confirm before building**)

### D1 — trivia model  → **recommend: a comment side-channel + position-based reattachment**
Three candidate architectures:
- **(A) Full lossless CST** — re-lex into a tree of *all* tokens incl. whitespace+comments, format the
  CST. Most robust (rustfmt-grade) but a large re-architecture of lexer+parser; overkill for v1.
- **(B) Comment side-channel (recommended)** — the lexer additionally collects each comment as
  `Comment { span, text, kind: Line|Block, own_line: bool }` into a `Vec` (it already *finds* them in
  the skip fns — emit instead of drop). The parser/AST are unchanged. A new **comment-aware printer**
  walks the AST and, keyed by byte span, flushes any pending comments *before* the node whose span
  starts after them; a trailing line-comment on the same source line as the preceding node is emitted
  inline. Covers the real cases (own-line comments above a decl/stmt, trailing `// …`, block comments
  between items). Documented limitation: a comment in a *pathological* mid-expression position may
  reattach to the nearest stmt boundary rather than its exact slot.
- **(C) Token-stream reformatter** — format by re-spacing the raw token stream (comments included),
  never building on the AST. Simple for trivia but re-implements all layout logic the printer already
  has, and can't do AST-level decisions (wrapping, alignment).

**Recommend (B)**: smallest change that yields a *usable, comment-safe* formatter, and the comment
side-channel **also improves lift round-trip fidelity** (L5) — a shared win. Upgrade to (A) only if (B)
proves too lossy in practice.

### D2 — CLI surface  → **recommend: mirror `gofmt`/`cargo fmt`**
- `phg fmt <file>` — format in place (write only if changed).
- `phg fmt --check [file|dir]` — exit `1` if any file is not already formatted (print the diff/paths),
  exit `0` if clean. No writes. (CI gate.)
- `phg fmt -` / stdin — format stdin → stdout.
- `phg fmt` (no path, in a project) — format every `*.phg` under the source root.
- **Idempotent**: `fmt(fmt(x)) == fmt(x)` — a test invariant.
- **Never reformat an unparseable file** — a parse error exits `2` with the diagnostic, file untouched
  (a formatter must not corrupt broken source).

### D3 — what it normalizes (v1) → indentation (2-space, matching the codebase), one statement per
line, canonical brace/spacing, trailing-newline, collapse >1 blank line to 1, normalize string quotes
only where unambiguous. **No line-wrapping/width-reflow in v1** (that's the hard, opinion-heavy part —
add later behind a width setting). v1 = "tidy + comment-safe," not "opinionated reflow."

## Slices
- **F1 — comment capture**: lexer emits `Comment{span,text,kind,own_line}` into a side `Vec` (returned
  alongside tokens); everything downstream ignores it for now (zero behavior change — proves capture).
- **F2 — comment-aware printer**: a printer variant that interleaves the captured comments by span.
  Unit-tested on round-trip + comment-placement fixtures; idempotence test.
- **F3 — `phg fmt` CLI**: `cmd_fmt` (in-place / `--check` / stdin / project-wide), exit codes,
  parse-error safety. `phg fmt --help`.
- **F4 — dogfood**: run `phg fmt --check` over `examples/` + `src/**.phg` test programs; wire into the
  existing `/fmt`-style gate if desired.
- **F5 (bonus) — lift L5 fidelity**: reuse the comment channel so `phg lift` preserves PHP comments.

## Open questions for the developer (confirm before F1)
1. **D1** trivia model: comment side-channel + reattachment (recommended) vs full lossless CST?
2. **D3** scope: "tidy + comment-safe, no reflow" v1 (recommended) vs include line-wrapping now?
3. Quote normalization: leave string quotes exactly as written, or canonicalize? (recommend: leave —
   fewer surprises, no interpolation-edge risk.)
4. Should `phg fmt --check` become a *commit gate* for the repo, or stay an on-demand tool for v1?
