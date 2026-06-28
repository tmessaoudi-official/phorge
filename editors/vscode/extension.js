// Phorj VS Code thin client — launches the `phg lsp` language server over stdio and routes
// `*.phg` documents to it. The server does all the work (diagnostics, hover, go-to-definition); this
// is only the registration. No build step: plain CommonJS, depends only on vscode-languageclient.

const { workspace } = require("vscode");
const { LanguageClient, TransportKind } = require("vscode-languageclient/node");

/** @type {import('vscode-languageclient/node').LanguageClient | undefined} */
let client;

function activate(_context) {
  const command = workspace.getConfiguration("phorj").get("serverPath", "phg");
  // The server is `phg lsp`, speaking LSP on stdin/stdout.
  const server = { command, args: ["lsp"], transport: TransportKind.stdio };
  const serverOptions = { run: server, debug: server };
  const clientOptions = {
    documentSelector: [{ scheme: "file", language: "phorj" }],
    synchronize: { fileEvents: workspace.createFileSystemWatcher("**/*.phg") },
  };
  client = new LanguageClient("phorj", "Phorj Language Server", serverOptions, clientOptions);
  client.start();
}

function deactivate() {
  return client ? client.stop() : undefined;
}

module.exports = { activate, deactivate };
