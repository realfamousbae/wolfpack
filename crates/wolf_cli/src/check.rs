//! `wolf check`: finds `.wolf` files, lexes and parses them.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use wolf_diag::{Diagnostic, codes};
use wolf_span::{MAX_FILE_SIZE, SourceMap};

use crate::{CheckArgs, Emit};

/// Everything `wolf check` produced.
pub(crate) struct CheckResult {
    pub(crate) map: SourceMap,
    pub(crate) diagnostics: Vec<Diagnostic>,
}

pub(crate) fn run(args: &CheckArgs) -> CheckResult {
    let mut result = CheckResult {
        map: SourceMap::new(),
        diagnostics: Vec::new(),
    };
    let mut files = Vec::new();
    for path in &args.paths {
        collect(path, &mut files, &mut result.diagnostics);
    }
    files.sort();
    files.dedup();
    for path in files {
        check_file(&path, args, &mut result);
    }
    result
}

/// Collects `.wolf` files. Directories are walked in sorted order; hidden directories, the
/// build directories `out` and `target`, and symbolic links to directories are skipped.
fn collect(path: &Path, files: &mut Vec<PathBuf>, diagnostics: &mut Vec<Diagnostic>) {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) => {
            diagnostics.push(read_error(path, &error));
            return;
        }
    };
    if !metadata.is_dir() {
        files.push(path.to_path_buf());
        return;
    }
    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(error) => {
            diagnostics.push(read_error(path, &error));
            return;
        }
    };
    // `file_type` does not follow symbolic links, so a link cannot make the walk loop.
    let mut children: Vec<(PathBuf, bool)> = entries
        .flatten()
        .map(|e| {
            let is_dir = e.file_type().is_ok_and(|t| t.is_dir());
            (e.path(), is_dir)
        })
        .collect();
    children.sort();
    for (child, is_dir) in children {
        let name = child.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if is_dir {
            if !name.starts_with('.') && name != "out" && name != "target" {
                collect(&child, files, diagnostics);
            }
        } else if child.extension().is_some_and(|e| e == "wolf") {
            files.push(child);
        }
    }
}

fn check_file(path: &Path, args: &CheckArgs, result: &mut CheckResult) {
    let name = display(path);
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) => {
            result.diagnostics.push(read_error(path, &error));
            return;
        }
    };
    if bytes.len() > MAX_FILE_SIZE {
        result.diagnostics.push(
            Diagnostic::new(
                codes::E0001,
                format!("cannot read `{name}`: the file is too large"),
            )
            .note("source files are limited to 4 GiB"),
        );
        return;
    }
    let text = match wolf_lexer::decode(bytes) {
        Ok(text) => text,
        Err(invalid) => {
            let id = result.map.add(name, invalid.lossy.clone());
            result.diagnostics.push(invalid.diagnostic(id));
            return;
        }
    };
    let id = result.map.add(name.clone(), text);
    let text = result.map.get(id).map(|f| f.text()).unwrap_or("");
    let lexed = wolf_lexer::lex(id, text);
    let parsed = wolf_parser::parse(id, text, &lexed.tokens);
    let mut stdout = std::io::stdout();
    match args.emit {
        Some(Emit::Tokens) => {
            let _ = writeln!(stdout, "== {name}");
            let _ = stdout.write_all(wolf_lexer::dump(text, &lexed.tokens).as_bytes());
        }
        Some(Emit::Ast) => {
            let _ = writeln!(stdout, "== {name}");
            let _ = stdout.write_all(wolf_ast::dump(&parsed.file).as_bytes());
        }
        None => {}
    }
    result.diagnostics.extend(lexed.diagnostics);
    result.diagnostics.extend(parsed.diagnostics);
}

fn read_error(path: &Path, error: &std::io::Error) -> Diagnostic {
    let reason = match error.kind() {
        std::io::ErrorKind::NotFound => "no such file or directory".to_string(),
        std::io::ErrorKind::PermissionDenied => "permission denied".to_string(),
        _ => error.to_string(),
    };
    Diagnostic::new(
        codes::E0001,
        format!("cannot read `{}`: {reason}", display(path)),
    )
}

/// A path as shown in diagnostics: as given, with `/` separators.
fn display(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}
