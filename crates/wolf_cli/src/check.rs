//! `wolf check`: checks a project, or single `.wolf` files.
//!
//! Without arguments, the project is found by looking for `wolfpack.toml` in the current directory
//! and its parents (spec chapter 6, rule 6.6). A directory argument starts that search there. A
//! `.wolf` file argument is checked on its own, without a project: lexing and parsing only. That
//! mode is a tool for tests and debugging.

use std::collections::BTreeMap;
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
    /// The checked project, if one was found and loaded.
    pub(crate) project: Option<Project>,
}

/// A loaded and checked project.
pub(crate) struct Project {
    /// The directory of `wolfpack.toml`.
    pub(crate) root: PathBuf,
    pub(crate) manifest: wolf_project::Manifest,
    /// Every module, std first: path, role, file and syntax tree.
    pub(crate) parsed: Vec<(Vec<String>, wolf_resolve::Role, FileId, wolf_ast::File)>,
    pub(crate) resolved: wolf_resolve::Resolved,
    pub(crate) typed: wolf_types::Typed,
}

impl Project {
    /// The modules as the resolver and the type checker take them.
    pub(crate) fn inputs(&self) -> Vec<wolf_resolve::ModuleInput<'_>> {
        self.parsed
            .iter()
            .map(|(path, role, file, ast)| wolf_resolve::ModuleInput {
                path: path.clone(),
                role: *role,
                file: *file,
                ast,
            })
            .collect()
    }
}

pub(crate) fn run(args: &CheckArgs) -> CheckResult {
    let mut result = CheckResult {
        map: SourceMap::new(),
        diagnostics: Vec::new(),
        project: None,
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
    // The std modules take part in resolution like any module.
    let mut parsed: Vec<(Vec<String>, wolf_resolve::Role, FileId, wolf_ast::File)> = Vec::new();
    for module in wolf_std::MODULES {
        let id = result.map.add(module.file_name, module.source.to_string());
        let lexed = wolf_lexer::lex(id, module.source);
        let tree = wolf_parser::parse(id, module.source, &lexed.tokens);
        result.diagnostics.extend(lexed.diagnostics);
        result.diagnostics.extend(tree.diagnostics);
        let role = if module.path == wolf_std::PRELUDE {
            wolf_resolve::Role::Prelude
        } else {
            wolf_resolve::Role::Std
        };
        let path = module.path.split('.').map(str::to_string).collect();
        parsed.push((path, role, id, tree.file));
    }
    for file in &files {
        if let Some((id, ast)) = parse_file(&file.path, file.display.clone(), args, result) {
            let role = match file.role {
                wolf_project::Role::Entry => wolf_resolve::Role::Entry,
                wolf_project::Role::Src => wolf_resolve::Role::Src,
                wolf_project::Role::Test => wolf_resolve::Role::Test,
                wolf_project::Role::Example => wolf_resolve::Role::Example,
            };
            parsed.push((file.module.clone(), role, id, ast));
        }
    }
    let inputs: Vec<wolf_resolve::ModuleInput<'_>> = parsed
        .iter()
        .map(|(path, role, file, ast)| wolf_resolve::ModuleInput {
            path: path.clone(),
            role: *role,
            file: *file,
            ast,
        })
        .collect();
    let info = wolf_resolve::ProjectInfo {
        kind: manifest.kind,
        config: &manifest.config,
    };
    let mut resolved = wolf_resolve::resolve(&inputs, &info);
    result
        .diagnostics
        .extend(std::mem::take(&mut resolved.diagnostics));
    let mut typed = wolf_types::check(&inputs, &resolved, &info);
    result
        .diagnostics
        .extend(std::mem::take(&mut typed.diagnostics));
    // Ownership (chapter 5) is checked on the IR of a program without other errors.
    if result.diagnostics.is_empty() {
        let meta = wolf_ir::ProjectMeta {
            name: &manifest.name,
            version: &manifest.version,
            kind: manifest.kind,
            dependencies: BTreeMap::new(),
        };
        match wolf_ir::lower(&inputs, &resolved, &typed, &result.map, &meta) {
            Ok(program) => result.diagnostics.extend(wolf_own::check(&program)),
            Err(e) => result.diagnostics.push(
                Diagnostic::new(codes::E0003, "internal compiler error")
                    .note(format!("the program cannot be lowered: {}", e.message))
                    .help("this is a bug in wolf; please report it with the input that caused it"),
            ),
        }
    }
    drop(inputs);
    result.project = Some(Project {
        root,
        manifest,
        parsed,
        resolved,
        typed,
    });
}

/// Loads and checks the project around `start` (used by `wolf build` and `wolf run`).
pub(crate) fn load(start: &Path, args: &CheckArgs) -> CheckResult {
    let mut result = CheckResult {
        map: SourceMap::new(),
        diagnostics: Vec::new(),
        project: None,
    };
    project(start, args, &mut result);
    result
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
