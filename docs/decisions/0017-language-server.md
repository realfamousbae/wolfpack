# 0017: The language server

Status: proposed (2026-10-23). The author asked for the language server after 0.3.0 (`PLAN.md`,
section 11: "LSP for editors" after M13); the items are [INTERNAL] choices recorded so that the
author can object. Publishing the extension needs the author's word.

## Decision

1. **`wolf lsp` is the server**, a subcommand of `wolf` that speaks the Language Server Protocol on
   standard input and output. The protocol code is the crate `wolfpack-lsp` (`wolf_lsp`), built on
   `lsp-server` and `lsp-types`; it reads no files itself. `wolf_cli` gives it the check of
   `wolf check` (`check::load_with`), which takes the texts of the open documents instead of the
   files on disk, so the native modules and the manifest are read as in a build.
2. **What it does**: after every change of a document it checks the whole project again and
   publishes the diagnostics of every file of the project (the code, the notes and the help in the
   message, the other labels as related places; diagnostics without a place go to
   `wolfpack.toml`). Hover shows the declaration of the name under the cursor as wolfpack code (a
   signature, the fields of a struct, the variants of an enum, the type of a local) and its doc
   comments; go to definition jumps to the name in its declaration; formatting is `wolf fmt` of
   the document, and nothing for a document with syntax errors. A file outside any project is
   checked alone, as the playground checks its file.
3. **Positions are UTF-16 code units**, the default of the protocol, converted from the byte
   offsets of the compiler.
4. **Names are found through the tables of the compiler**: the resolver records where each
   resolved node is (`Resolved::spans`), and the server takes the innermost use, local declaration
   or declaration at the cursor. Methods and fields, which only the type checker resolves, have no
   hover or definition yet.
5. **The VS Code extension is `editors/vscode/`**: a TextMate grammar for highlighting, the
   language configuration (comments, brackets) and a client that starts `wolf lsp`
   (`wolfpack.server.path`). It is built with `npx @vscode/vsce package` and not published.

## Reasons

1. One binary serves the command line and the editor, so their diagnostics are the same; the
   project loader stays in one place.
2. Checking the whole project on each change is simple and fast enough for projects of the size
   wolfpack has today; incremental checking can come when a project makes it slow.
3. A grammar gives highlighting even without the server, in any editor that reads TextMate
   grammars.
