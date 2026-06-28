# Phorge for PhpStorm / IntelliJ IDEA (and other JetBrains IDEs)

Phorge ships a single language server (`phg lsp`) and a single TextMate grammar
(`../vscode/syntaxes/phorge.tmLanguage.json`). JetBrains IDEs consume **both** without a compiled
plugin — using two built-in/marketplace mechanisms:

1. **Syntax highlighting** — JetBrains' native **TextMate Bundles** support reads the same grammar the
   VSCode extension uses.
2. **Language intelligence** (diagnostics, hover, go-to-definition, completion, document symbols,
   references, rename, formatting) — the **LSP4IJ** plugin runs `phg lsp` as an external language server.

This is the recommended, no-build path: identical behavior to the CLI (`phg check` / `phg fmt`) and to
the VSCode extension, because all three drive the *same* `phg lsp` server and the *same* grammar.

> A natively-compiled JetBrains plugin (richer integration, marketplace one-click install) is tracked as
> a follow-up — it would still wrap this same `phg lsp` server (JetBrains' own LSP API or LSP4IJ as a
> library). The setup below already delivers the full feature set today.

## Prerequisites

- The `phg` binary on your `PATH` (or note its absolute path). Build it with
  `cargo build --release` → `target/release/phg`.
- PhpStorm / IntelliJ **2023.2+**.

## 1. Syntax highlighting (TextMate bundle)

`Settings → Editor → TextMate Bundles → +` and select this repository's **`editors/vscode/`** directory.
JetBrains reads its `package.json` `grammars` entry and loads `syntaxes/phorge.tmLanguage.json`, so
`.phg` files are highlighted (keywords, types, strings + `{…}` interpolation, numbers, comments,
attributes). No VSCode required — the directory is just a standard TextMate-compatible bundle.

## 2. Language server (LSP4IJ)

1. Install **LSP4IJ** (`Settings → Plugins → Marketplace → "LSP4IJ"`).
2. `Settings → Languages & Frameworks → Language Servers → +` (a new *user-defined* language server):
   - **Name:** `Phorge`
   - **Command:** `phg lsp` (or `/absolute/path/to/phg lsp`)
   - **Mappings → File name patterns:** `*.phg` → language id `phorge`
3. Apply. Open any `.phg` file: diagnostics appear inline (identical to `phg check`), and hover,
   go-to-definition (`Ctrl/Cmd+Click`), completion, structure view (document symbols), find-usages
   (references), rename, and reformat (`phg fmt`) all work through the server.

### Notes

- **Formatting** routes to `phg fmt` (comment- and meaning-preserving); reformatting a file that does
  not parse is a no-op (the server never corrupts an in-progress buffer).
- References / rename are **single-document** today (cross-file is a server follow-up).
- The server is **off the byte-identity spine** — it never runs the three execution backends, so it
  carries no `run`/`runvm`/PHP parity risk; its diagnostics equal `phg check` exactly.
