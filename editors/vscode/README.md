# Wolfpack for VS Code

Highlighting of `.wolf` files, and, from the language server `wolf lsp`: the diagnostics of
`wolf check` as you type, the type or signature of a name on hover, going to the declaration of a
name (F12), and formatting with `wolf fmt` (Format Document).

The extension is not on the Marketplace yet. To try it from this repository:

```bash
cd editors/vscode
```

```bash
npm install
```

```bash
npx @vscode/vsce package
```

```bash
code --install-extension wolfpack-0.5.0.vsix
```

The server is the `wolf` command itself, so a `wolf` that has `wolf lsp` (0.4.0 or newer)
must be on `PATH`, or `wolfpack.server.path` must name it. Any other editor with a client for the Language Server
Protocol can use `wolf lsp` the same way: it speaks the protocol on standard input and output.
