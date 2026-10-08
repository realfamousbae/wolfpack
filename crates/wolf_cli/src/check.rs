//! `wolf check`: checks a project, or single `.wolf` files.
//!
//! Without arguments, the project is found by looking for `wolfpack.toml` in the current directory
//! and its parents (spec chapter 6, rule 6.6). A directory argument starts that search there. A
//! `.wolf` file argument is checked on its own, without a project: lexing and parsing only. That
//! mode is a tool for tests and debugging.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use wolf_diag::{Diagnostic, codes};
use wolf_span::{FileId, MAX_FILE_SIZE, SourceMap};

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
    if args.paths.is_empty() {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        project(&cwd, args, &mut result);
        return result;
    }
    for path in &args.paths {
        if path.is_dir() {
            project(path, args, &mut result);
        } else {
            let name = display(path);
            let _ = parse_file(path, name, args, &mut result);
        }
    }
    result
}

fn project(start: &Path, args: &CheckArgs, result: &mut CheckResult) {
    let Some(root) = wolf_project::find_root(start) else {
        result.diagnostics.push(
            Diagnostic::new(
                codes::E0004,
                format!(
                    "no `wolfpack.toml` found in `{}` or its parents",
                    display(start)
                ),
            )
            .help("run `wolf check` inside a project, or create one with `wolf new <name>`"),
        );
        return;
    };
    let manifest_path = root.join("wolfpack.toml");
    let text = match fs::read_to_string(&manifest_path) {
        Ok(text) => text,
        Err(error) => {
            result.diagnostics.push(read_error("wolfpack.toml", &error));
            return;
        }
    };
    let manifest_id = result.map.add("wolfpack.toml", text);
    let manifest_text = result.map.get(manifest_id).map(|f| f.text()).unwrap_or("");
    let (manifest, diagnostics) = wolf_project::parse_manifest(manifest_id, manifest_text);
    result.diagnostics.extend(diagnostics);
    let Some(manifest) = manifest else {
        return;
    };
    let (files, diagnostics) = wolf_project::discover(&root, &manifest);
    result.diagnostics.extend(diagnostics);
    for file in &files {
        let _ = parse_file(&file.path, file.display.clone(), args, result);
    }
}

/// Reads, lexes and parses one file. Returns `None` if it could not be read or decoded.
fn parse_file(
    path: &Path,
    name: String,
    args: &CheckArgs,
    result: &mut CheckResult,
) -> Option<(FileId, wolf_ast::File)> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) => {
            result.diagnostics.push(read_error(&name, &error));
            return None;
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
        return None;
    }
    let text = match wolf_lexer::decode(bytes) {
        Ok(text) => text,
        Err(invalid) => {
            let id = result.map.add(name, invalid.lossy.clone());
            result.diagnostics.push(invalid.diagnostic(id));
            return None;
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
    Some((id, parsed.file))
}

fn read_error(name: &str, error: &std::io::Error) -> Diagnostic {
    let reason = match error.kind() {
        std::io::ErrorKind::NotFound => "no such file or directory".to_string(),
        std::io::ErrorKind::PermissionDenied => "permission denied".to_string(),
        _ => error.to_string(),
    };
    Diagnostic::new(codes::E0001, format!("cannot read `{name}`: {reason}"))
}

/// A path as shown in diagnostics: as given, with `/` separators.
fn display(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}
