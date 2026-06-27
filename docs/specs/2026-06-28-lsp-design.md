# Phorge LSP — design (Item D)

> Status: **design-first** 2026-06-28; scope forks awaiting the developer. Finishes GA rock 2
> (daily-use tooling). Builds on `phg check --json` (structured diagnostics already emitted) and the
> checker's `Diagnostic` surface. See [[ide-tooling-extensions]].

## Goal

A minimal **Language Server** so editors (VSCode, PhpStorm, Neovim, …) show Phorge diagnostics inline
as you type — the single highest-leverage IDE feature. One server, many thin editor clients (the LSP
contract). The server reuses the *exact* checker the CLI uses, so editor diagnostics ≡ `phg check`.

## Hard constraint — zero-dependency ⇒ hand-rolled JSON-RPC

An LSP server is **not** a security-critical primitive, so the dependency policy
(`docs/specs/2026-06-27-dependency-policy.md`) **excludes** the usual crates (`tower-lsp`,
`lsp-server`, `serde`). The server therefore hand-rolls the protocol in `std`:

- **Transport**: LSP base protocol over **stdio** — `Content-Length: N\r\n\r\n<json>` framing, read
  from stdin / write to stdout. (The LSP standard; editors all support stdio. No socket in v1.)
- **JSON**: reuse the project's existing hand-rolled JSON emit (the `diagnostics_json` / `to_json`
  path) for responses, and a **minimal std JSON *parser*** for incoming requests — Phorge has no JSON
  parser for arbitrary input yet (`Core.Json` is the *language's* parser, not usable internally). A
  small, total, std-only request parser (object/string/number/array, enough for LSP message bodies) is
  the one genuinely new internal piece. It is **internal tooling, not on the byte-identity spine** —
  it never touches the three backends, so it carries no parity risk.

This is more code than `tower-lsp`, but keeps the zero-dep invariant intact and is bounded (LSP's
core message set is small).

## Architecture

- **`phg lsp`** — a new CLI subcommand (not a separate binary; reuses the `phg` binary, like
  `serve`/`check`). Starts the stdio JSON-RPC loop.
- **Lifecycle**: `initialize` → advertise capabilities → `initialized`; `textDocument/didOpen` +
  `didChange` + `didClose` maintain an in-memory document map (URI → text); `shutdown`/`exit`.
- **On open/change**: lex + parse + `check_resolutions` the buffer (the same pipeline `check --json`
  uses, via `on_deep_stack`), collect errors + warnings, map to LSP `Diagnostic[]`, send
  `textDocument/publishDiagnostics`. A parse/lex error maps to a single diagnostic at its span.
- **Document sync**: **full** (the client sends the whole text on each change). Simplest, correct, and
  fine for Phorge file sizes; incremental sync is a v2 optimization.

## Diagnostic mapping (checker → LSP)

| Phorge `Diagnostic` | LSP |
|---------------------|-----|
| `line`/`col` (1-based) | `range.start` (0-based: `line-1`, `col-1`); `range.end` = start + token length (v1: a 1-char or word range — the struct flattens span to a point, so v1 highlights from the caret) |
| error vs warning | `severity` 1 (Error) / 2 (Warning) |
| `code` (e.g. `E-UNKNOWN-IDENT`) | `code` + `codeDescription.href` → the `phg explain <CODE>` text surfaced (or a docs URL) |
| `hint` | appended to `message` (v1) or `relatedInformation` (v2) |

A v2 refinement adds true end-positions by threading the diagnostic's `Span.len` through to the LSP
range (the underlying error already has a span; only the flattened `Diagnostic` drops it).

## Scope v1 vs v2 (a fork)

- **v1 (recommended): diagnostics-only.** `publishDiagnostics` on open/change. This is ~80% of the
  daily value and reuses the checker wholesale — no new analysis. Plus `phg explain` surfaced via
  `codeDescription`.
- **v2 (later)**: hover (type at cursor), go-to-definition, document symbols, completion. Each needs
  new query infrastructure over the checker's resolved tables (the checker computes types + resolutions
  but doesn't expose a position→symbol index yet). A bigger slice.

## Editor client (a fork)

The server alone is useless without a client registration. Options: (a) ship a **minimal VSCode
extension** (a thin `package.json` + a few lines launching `phg lsp` — the standard vscode-languageclient
shape) in-repo under `editors/vscode/`; (b) document the generic LSP registration and let users wire
their editor; (c) both.

## Testing

The server is **outside `differential.rs`** (it's not a backend). Test the JSON-RPC layer directly: a
`tests/lsp.rs` that feeds framed `initialize` + `didOpen` (a program with a known error) and asserts
the `publishDiagnostics` notification carries the expected code/range — driving the request parser +
diagnostic mapping without a real editor. The diagnostic *content* is already covered by the checker
tests; the LSP test covers framing + mapping.

## Build slices (after scope is chosen)

1. **JSON-RPC core**: std stdio framing (`Content-Length`) + the minimal request JSON parser + a
   response/notification writer (reusing the existing JSON emit). `tests/lsp.rs` round-trips a frame.
2. **Lifecycle + document store**: `initialize`/`initialized`/`didOpen`/`didChange`/`didClose`/
   `shutdown`/`exit`; URI→text map.
3. **Diagnostics**: run the checker on a buffer, map to LSP `Diagnostic[]`, `publishDiagnostics`;
   `code` + `phg explain` surfaced. Integration test asserts a known error's code/range.
4. **`phg lsp` CLI + `--help`** + docs (README "Editor support" section).
5. **(If chosen)** `editors/vscode/` thin client.

## Open forks for the developer

1. **Scope**: v1 diagnostics-only (recommended) vs include hover/go-to-def now.
2. **Editor client**: ship a VSCode thin client in-repo, docs-only, or both.
3. **Document sync**: full (recommended, simplest) vs incremental.
