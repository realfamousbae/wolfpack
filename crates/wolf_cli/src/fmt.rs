//! `wolf fmt`: formats the source files of a project or the given files (spec chapter 3,
//! section 8a).

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use wolf_diag::{Diagnostic, codes};

use crate::check::CheckResult;

/// What `wolf fmt` did.
pub(crate) enum Outcome {
    /// Every file is formatted (with `--check`: was already formatted).
    Done,
    /// With `--check`: these files are not formatted.
    Unformatted(Vec<String>),
    /// Errors; nothing was written for the files that have them.
    Failed(Box<CheckResult>, u8),
}

/// Formats `paths` (files or directories), or the project around `cwd` without paths.
pub(crate) fn run(cwd: &Path, paths: &[PathBuf], check: bool) -> Outcome {
    let mut result = CheckResult {
        map: wolf_span::SourceMap::new(),
        diagnostics: Vec::new(),
        project: None,
    };
    let mut files = Vec::new();
    if paths.is_empty() {
        let Some(root) = wolf_project::find_root(cwd) else {
            result.diagnostics.push(
                Diagnostic::new(
                    codes::E0004,
                    format!(
                        "no `wolfpack.toml` found in `{}` or its parents",
                        cwd.display()
                    ),
                )
                .help("run `wolf fmt` inside a project, or name the files to format"),
            );
            return Outcome::Failed(Box::new(result), crate::EXIT_ERRORS);
        };
        for dir in ["src", "tests", "examples"] {
            collect(&root.join(dir), &mut files);
        }
    } else {
        for p in paths {
            let p = cwd.join(p);
            if p.is_dir() {
                collect(&p, &mut files);
            } else {
                files.push(p);
            }
        }
    }
    let mut unformatted = Vec::new();
    let mut code = 0;
    for path in files {
        let name = relative(cwd, &path);
        let text = match fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                result.diagnostics.push(Diagnostic::new(
                    codes::E0001,
                    format!("cannot read `{name}`: {e}"),
                ));
                code = crate::EXIT_ERRORS;
                continue;
            }
        };
        let id = result.map.add(name.clone(), text.clone());
        match wolf_fmt::format(id, &text) {
            Ok(out) if out == text => {}
            Ok(out) => {
                if check {
                    unformatted.push(name);
                } else if let Err(e) = fs::write(&path, out) {
                    result.diagnostics.push(Diagnostic::new(
                        codes::E0001,
                        format!("cannot write `{name}`: {e}"),
                    ));
                    code = crate::EXIT_ERRORS;
                }
            }
            Err(wolf_fmt::FmtError::Syntax(d)) => {
                result.diagnostics.extend(d);
                code = code.max(crate::EXIT_ERRORS);
            }
            Err(wolf_fmt::FmtError::Internal(message)) => {
                result.diagnostics.push(
                    Diagnostic::new(codes::E0003, "internal compiler error")
                        .note(format!("`wolf fmt` left `{name}` unchanged: {message}"))
                        .help("this is a bug in wolf; please report it with the file"),
                );
                code = crate::EXIT_INTERNAL;
            }
        }
    }
    if !result.diagnostics.is_empty() {
        return Outcome::Failed(Box::new(result), code.max(crate::EXIT_ERRORS));
    }
    if unformatted.is_empty() {
        Outcome::Done
    } else {
        Outcome::Unformatted(unformatted)
    }
}

/// The `.wolf` files under `dir`, without the generated `out/` directories.
fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n != "out") {
                collect(&path, out);
            }
        } else if path.extension().is_some_and(|e| e == "wolf") {
            out.push(path);
        }
    }
}

fn relative(cwd: &Path, path: &Path) -> String {
    path.strip_prefix(cwd)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Prints the files that `--check` found unformatted, one per line.
pub(crate) fn print_unformatted(files: &[String]) {
    let mut out = std::io::stdout();
    for f in files {
        let _ = writeln!(out, "{f}");
    }
}
