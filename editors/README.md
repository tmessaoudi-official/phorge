# Editor support

Phorj's editor integration is **one language server + one grammar**, reused by every editor — so
behavior is identical across IDEs and matches the CLI (`phg check`, `phg fmt`).

- **Language server:** `phg lsp` (`src/lsp/`) — diagnostics, hover, go-to-definition, completion,
  document symbols, find-references, document-highlight, rename, and formatting. Hand-rolled JSON-RPC,
  std-only, off the byte-identity spine (it never runs the execution backends).
- **Grammar:** `vscode/syntaxes/phorj.tmLanguage.json` — a TextMate grammar (keywords, types, strings
  with `{…}` interpolation, numbers, comments, `#[…]` attributes), consumed by both VSCode and JetBrains.

| Editor | Setup |
|--------|-------|
| **VS Code** | `vscode/` — a thin `vscode-languageclient` client + the grammar. See `vscode/README.md`. |
| **PhpStorm / IntelliJ** (any JetBrains IDE) | `phpstorm/` — native TextMate Bundle (the `vscode/` dir) for highlighting + **LSP4IJ** running `phg lsp` for intelligence. No compiled plugin needed. See `phpstorm/README.md`. |

A natively-compiled JetBrains marketplace plugin (one-click install, wrapping the same `phg lsp`) is a
tracked follow-up; the LSP4IJ path already delivers the full feature set today.
