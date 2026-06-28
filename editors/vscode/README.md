# Phorge — VS Code extension

A thin client that connects VS Code to the Phorge language server (`phg lsp`), plus a TextMate grammar
for syntax highlighting. The language *intelligence* lives entirely in the server; this extension
registers the `phorge` language (`*.phg`), ships the grammar (`syntaxes/phorge.tmLanguage.json`), and
launches the server over stdio. (The same grammar + server power the JetBrains/PhpStorm setup — see
`../phpstorm/README.md`.)

## Features

- **Syntax highlighting** — keywords, types, strings with `{…}` interpolation, numbers, comments, and
  `#[…]` attributes (TextMate grammar, no server needed).
- **Diagnostics** — type/parse errors and lints, live as you type (identical to `phg check`).
- **Hover** — the declaration signature of the symbol under the cursor.
- **Go-to-definition** — jump to a function / class / enum / interface / trait / type declaration.
- **Completion** — top-level symbols, in-scope locals/params, and keywords.
- **Document symbols** — the file outline (classes/enums carry their members).
- **Find references** + **document highlight** — every use of the symbol under the cursor
  (scope-accurate).
- **Rename** — rename a symbol and all its uses.
- **Formatting** — reformat via `phg fmt` (comment- and meaning-preserving).

## Prerequisites

- The `phg` binary on your `PATH` (or set `phorge.serverPath` in settings to its absolute path).
  Build it with `cargo build --release` (the binary is `target/release/phg`).

## Run it (Extension Development Host)

```sh
cd editors/vscode
npm install          # fetches vscode-languageclient
code .               # then press F5 → "Run Extension" to launch the dev host
```

Open any `.phg` file in the dev host; diagnostics, hover, and go-to-definition activate automatically.

## Package / install locally

```sh
npm install -g @vscode/vsce
cd editors/vscode
vsce package         # produces phorge-0.2.0.vsix
code --install-extension phorge-0.2.0.vsix
```

## Configuration

- `phorge.serverPath` (default `"phg"`) — path to the `phg` binary; the server is started as
  `phg lsp`.
