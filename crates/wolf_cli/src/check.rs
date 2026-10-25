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
    /// The target being built has code in `native/<target>/`: a rejection by its tools is
    /// E0806, not E0802 (spec chapter 8, section 9).
    pub(crate) native: bool,
    /// What `wolf check --native` prints (spec chapter 8, section 8b).
    pub(crate) listings: Vec<String>,
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

/// Texts that replace files on disk, by canonical path: the documents an editor has open
/// (`wolf lsp`).
pub(crate) type Overlay = BTreeMap<PathBuf, String>;

pub(crate) fn run(args: &CheckArgs) -> CheckResult {
    let mut result = CheckResult {
        map: SourceMap::new(),
        diagnostics: Vec::new(),
        project: None,
        native: false,
        listings: Vec::new(),
    };
    if args.paths.is_empty() {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        project(&cwd, args, &Overlay::new(), &mut result);
        return result;
    }
    for path in &args.paths {
        if path.is_dir() {
            project(path, args, &Overlay::new(), &mut result);
        } else {
            let name = display(path);
            let _ = parse_file(path, name, args, &Overlay::new(), &mut result);
        }
    }
    result
}

fn project(start: &Path, args: &CheckArgs, overlay: &Overlay, result: &mut CheckResult) {
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
    let mut parsed = wolf_driver::std_modules(&mut result.map, &mut result.diagnostics);
    for file in &files {
        if let Some((id, ast)) = parse_file(&file.path, file.display.clone(), args, overlay, result)
        {
            let role = match file.role {
                wolf_project::Role::Entry => wolf_resolve::Role::Entry,
                wolf_project::Role::Src => wolf_resolve::Role::Src,
                wolf_project::Role::Test => wolf_resolve::Role::Test,
                wolf_project::Role::Example => wolf_resolve::Role::Example,
            };
            parsed.push((file.module.clone(), role, id, ast));
        }
    }
    // Native modules imported by the project (spec chapter 6, section 1a).
    let loaded =
        crate::native_modules::load(&root, &parsed, &mut result.map, args.native.as_deref());
    result.diagnostics.extend(loaded.diagnostics);
    result.listings = loaded.listings;
    let unavailable = loaded.names;
    parsed.extend(loaded.modules);
    let analysis = wolf_driver::analyze(
        &result.map,
        &manifest,
        parsed,
        &unavailable,
        &mut result.diagnostics,
    );
    let native = wolf_driver::Native::load(&root);
    result
        .diagnostics
        .extend(wolf_driver::check_native(&native, &analysis.parsed));
    result.project = Some(Project {
        root,
        manifest,
        parsed: analysis.parsed,
        resolved: analysis.resolved,
        typed: analysis.typed,
    });
    if let Some(name) = &args.bytecode {
        if result.diagnostics.is_empty() {
            bytecode(result, name);
        }
    }
}

/// What `wolf check --bytecode` prints (spec chapter 10, section 1a): the bytecode of the
/// functions of the modules of `src/`, or of those named `name` (`area`, `Shape.area`,
/// `shapes.area`, `main`), each with its closures; a module that cannot run in the VM is a
/// comment.
fn bytecode(result: &mut CheckResult, name: &str) {
    let Some(project) = &result.project else {
        return;
    };
    let inputs = project.inputs();
    let unavailable = wolf_resolve::vm_unavailable(&inputs);
    let meta = wolf_driver::meta(&project.manifest, false);
    let lowered = wolf_ir::lower(
        &inputs,
        &project.resolved,
        &project.typed,
        &result.map,
        &meta,
    );
    let compiled = lowered
        .map_err(|e| e.message)
        .and_then(|p| wolf_vm::compile(&p).map(|image| (p, image)));
    let (program, image) = match compiled {
        Ok(x) => x,
        Err(e) => {
            result.diagnostics.push(
                Diagnostic::new(codes::E0003, "internal compiler error")
                    .note(format!("the program cannot be compiled to bytecode: {e}"))
                    .help("this is a bug in wolf; please report it with the input that caused it"),
            );
            return;
        }
    };
    let query: Option<Vec<&str>> = (!name.is_empty()).then(|| name.split('.').collect());
    let mut modules: Vec<(String, usize)> = program
        .modules
        .iter()
        .enumerate()
        .filter(|(_, m)| {
            matches!(
                m.role,
                wolf_ir::ModuleRole::Entry | wolf_ir::ModuleRole::Src
            )
        })
        .map(|(i, m)| (m.path.join("."), i))
        .collect();
    modules.sort();
    let mut listings = Vec::new();
    for (path, index) in modules {
        let prefix = format!("{path}.");
        // The functions in the order of the source, each followed by its closures.
        let mut groups: Vec<(u32, Vec<u32>)> = Vec::new();
        for &(_, func, def) in image.listed.iter().filter(|(m, _, _)| *m == index) {
            match def {
                Some(d) => groups.push((source_start(project, &program, d), vec![func])),
                None => {
                    if let Some(g) = groups.last_mut() {
                        g.1.push(func);
                    }
                }
            }
        }
        groups.sort_by_key(|g| g.0);
        // Each function by the name after its module, closures by the function they are in.
        let mut found = false;
        for func in groups.into_iter().flat_map(|g| g.1) {
            let Some(wolf_vm::Func::Code(chunk)) = image.funcs.get(func as usize) else {
                continue;
            };
            let rest = chunk.name.strip_prefix(&prefix).unwrap_or(&chunk.name);
            let owner = rest.split(" closure ").next().unwrap_or(rest);
            let wanted = match &query {
                None => true,
                Some(q) => named(q, &path, owner),
            };
            if !wanted {
                continue;
            }
            found = true;
            if unavailable.contains_key(&path) {
                continue;
            }
            if let Some(text) = image.listing(func) {
                listings.push(text);
            }
        }
        if let Some(reason) = unavailable.get(&path) {
            if query.is_none() || found {
                listings.push(format!("// module {path} cannot run in the VM: {reason}\n"));
            }
        }
    }
    if query.is_some() && listings.is_empty() {
        result.diagnostics.push(
            Diagnostic::new(
                codes::E0002,
                format!("no function `{name}` in the modules of `src/`"),
            )
            .help("name a function as `area`, a method as `Shape.area`, either with its module as `shapes.area`, or the `main` block as `main`"),
        );
        return;
    }
    result.listings.extend(listings);
}

/// Where a function of the IR is declared in its file: by its declaration, or, for a method of
/// an interface implementation, which the IR numbers itself, by the method of its type.
fn source_start(project: &Project, program: &wolf_ir::Program, def: wolf_ir::DefRef) -> u32 {
    let info = program.def(def);
    if let Some(d) = project.resolved.def(wolf_resolve::DefId(def.0)) {
        if info.is_some_and(|i| i.name == d.name) {
            return d.span.start;
        }
    }
    let Some(wolf_ir::DefInfo {
        name,
        kind: wolf_ir::DefKind::Method { owner, .. },
        ..
    }) = info
    else {
        return u32::MAX;
    };
    project
        .typed
        .decls
        .methods
        .iter()
        .find(|m| m.owner.0 == owner.0 && &m.name == name)
        .map_or(u32::MAX, |m| m.span.start)
}

/// Whether the function `rest` (`area`, `Shape.area`) of the module `module` has the name
/// `query`: the same, or with the module before it.
fn named(query: &[&str], module: &str, rest: &str) -> bool {
    let parts: Vec<&str> = rest.split('.').collect();
    if query == parts.as_slice() {
        return true;
    }
    let full: Vec<&str> = module.split('.').chain(parts).collect();
    query == full.as_slice()
}

/// Loads and checks the project around `start` (used by `wolf build` and `wolf run`).
pub(crate) fn load(start: &Path, args: &CheckArgs) -> CheckResult {
    load_with(start, args, &Overlay::new())
}

/// [`load`], with the texts of `overlay` instead of the files on disk.
pub(crate) fn load_with(start: &Path, args: &CheckArgs, overlay: &Overlay) -> CheckResult {
    let mut result = CheckResult {
        map: SourceMap::new(),
        diagnostics: Vec::new(),
        project: None,
        native: false,
        listings: Vec::new(),
    };
    project(start, args, overlay, &mut result);
    result
}

/// Reads, lexes and parses one file, or its text in `overlay`. Returns `None` if it could not be
/// read or decoded.
fn parse_file(
    path: &Path,
    name: String,
    args: &CheckArgs,
    overlay: &Overlay,
    result: &mut CheckResult,
) -> Option<(FileId, wolf_ast::File)> {
    let open = if overlay.is_empty() {
        None
    } else {
        fs::canonicalize(path).ok().and_then(|p| overlay.get(&p))
    };
    let bytes = match open {
        Some(text) => text.clone().into_bytes(),
        None => match fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) => {
                result.diagnostics.push(read_error(&name, &error));
                return None;
            }
        },
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
    let file = wolf_driver::parse(&result.map, id, &mut result.diagnostics);
    let mut stdout = std::io::stdout();
    match args.emit {
        Some(Emit::Tokens) => {
            let text = result.map.get(id).map(|f| f.text()).unwrap_or("");
            let lexed = wolf_lexer::lex(id, text);
            let _ = writeln!(stdout, "== {name}");
            let _ = stdout.write_all(wolf_lexer::dump(text, &lexed.tokens).as_bytes());
        }
        Some(Emit::Ast) => {
            let _ = writeln!(stdout, "== {name}");
            let _ = stdout.write_all(wolf_ast::dump(&file).as_bytes());
        }
        None => {}
    }
    Some((id, file))
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
