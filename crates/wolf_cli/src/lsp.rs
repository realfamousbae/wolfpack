//! `wolf lsp`: the language server for editors (crate `wolfpack-lsp`), with the checks of
//! `wolf check` on the texts the editor has open.

use std::path::Path;

use crate::check::{self, Overlay};
use crate::{CheckArgs, Color, MessageFormat};

/// Serves an editor on standard input and output until it asks the server to exit.
pub(crate) fn run() -> u8 {
    let args = CheckArgs {
        paths: Vec::new(),
        message_format: MessageFormat::Human,
        color: Color::Never,
        emit: None,
        native: None,
        bytecode: None,
    };
    let check = |dir: &Path, overlay: &Overlay| -> wolf_lsp::Checked {
        let root = wolf_project::find_root(dir);
        if root.is_none() {
            return wolf_lsp::Checked {
                map: wolf_span::SourceMap::new(),
                diagnostics: Vec::new(),
                root: None,
                project: None,
            };
        }
        let result = check::load_with(dir, &args, overlay);
        wolf_lsp::Checked {
            map: result.map,
            diagnostics: result.diagnostics,
            root,
            project: result.project.map(|p| wolf_lsp::Project {
                parsed: p
                    .parsed
                    .into_iter()
                    .map(|(_, _, f, ast)| (f, ast))
                    .collect(),
                resolved: p.resolved,
                typed: p.typed,
            }),
        }
    };
    match wolf_lsp::serve(&check) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("error: the language server stopped: {e}");
            crate::EXIT_INTERNAL
        }
    }
}
