// Starts `wolf lsp` for the `.wolf` files of the workspace and connects VS Code to it.

const vscode = require("vscode");
const { LanguageClient } = require("vscode-languageclient/node");

let client;

function activate() {
  const command = vscode.workspace.getConfiguration("wolfpack").get("server.path", "wolf");
  client = new LanguageClient(
    "wolfpack",
    "Wolfpack",
    { command, args: ["lsp"] },
    { documentSelector: [{ scheme: "file", language: "wolfpack" }] },
  );
  return client.start().catch((error) => {
    vscode.window.showErrorMessage(
      `Wolfpack: \`${command} lsp\` could not start (${error.message}). ` +
        "Install wolf or set wolfpack.server.path.",
    );
  });
}

function deactivate() {
  return client ? client.stop() : undefined;
}

module.exports = { activate, deactivate };
