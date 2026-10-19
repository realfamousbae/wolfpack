//! The compiler pipeline without a file system: parsing, checking, lowering and code generation of
//! a project whose files are already in memory.
//!
//! `wolf_cli` reads the project from disk and runs these steps; the playground (`wolf_wasm`) runs
//! them on the source in the browser.

use std::collections::BTreeMap;

mod native;

pub use native::{NATIVE_PREFIX, Native, check_native, copied_to, go_package_dir, native_target};

use wolf_diag::{Diagnostic, codes};
use wolf_project::{Manifest, Target};
use wolf_resolve::{ModuleInput, Resolved, Role};
use wolf_span::{FileId, SourceMap};
use wolf_types::Typed;

/// A parsed module: its path, its role, its file and its syntax tree.
pub type Parsed = (Vec<String>, Role, FileId, wolf_ast::File);

/// A checked project: every module with std first, name resolution and types.
pub struct Analysis {
    /// The modules, std first.
    pub parsed: Vec<Parsed>,
    /// Name resolution.
    pub resolved: Resolved,
    /// Types.
    pub typed: Typed,
}

impl Analysis {
    /// The modules as the resolver, the type checker and the lowering take them.
    pub fn inputs(&self) -> Vec<ModuleInput<'_>> {
        inputs(&self.parsed)
    }
}

fn inputs(parsed: &[Parsed]) -> Vec<ModuleInput<'_>> {
    parsed
        .iter()
        .map(|(path, role, file, ast)| ModuleInput {
            path: path.clone(),
            role: *role,
            file: *file,
            ast,
        })
        .collect()
}

/// Lexes and parses a file that is already in `map`.
pub fn parse(map: &SourceMap, file: FileId, diagnostics: &mut Vec<Diagnostic>) -> wolf_ast::File {
    let text = map.get(file).map(|f| f.text()).unwrap_or("");
    let lexed = wolf_lexer::lex(file, text);
    let parsed = wolf_parser::parse(file, text, &lexed.tokens);
    diagnostics.extend(lexed.diagnostics);
    diagnostics.extend(parsed.diagnostics);
    parsed.file
}

/// Adds the std modules to `map` and parses them; they take part in resolution like any module.
pub fn std_modules(map: &mut SourceMap, diagnostics: &mut Vec<Diagnostic>) -> Vec<Parsed> {
    let mut parsed = Vec::new();
    for module in wolf_std::MODULES {
        let id = map.add(module.file_name, module.source.to_string());
        let tree = parse(map, id, diagnostics);
        let role = if module.path == wolf_std::PRELUDE {
            Role::Prelude
        } else {
            Role::Std
        };
        let path = module.path.split('.').map(str::to_string).collect();
        parsed.push((path, role, id, tree));
    }
    parsed
}

/// Resolves names, checks types and, for a program without other errors, ownership.
/// `parsed` holds the std modules first (see [`std_modules`]).
pub fn analyze(
    map: &SourceMap,
    manifest: &Manifest,
    parsed: Vec<Parsed>,
    native: &wolf_resolve::NativeNames,
    diagnostics: &mut Vec<Diagnostic>,
) -> Analysis {
    let inputs = inputs(&parsed);
    let info = wolf_resolve::ProjectInfo {
        kind: manifest.kind,
        config: &manifest.config,
        native,
    };
    let mut resolved = wolf_resolve::resolve(&inputs, &info);
    diagnostics.extend(std::mem::take(&mut resolved.diagnostics));
    let mut typed = wolf_types::check(&inputs, &resolved, &info);
    diagnostics.extend(std::mem::take(&mut typed.diagnostics));
    // Ownership (chapter 5) is checked on the IR of a program without other errors.
    if diagnostics.is_empty() {
        match wolf_ir::lower(&inputs, &resolved, &typed, map, &meta(manifest, false)) {
            Ok(program) => diagnostics.extend(wolf_own::check(&program)),
            Err(e) => diagnostics.push(
                Diagnostic::new(codes::E0003, "internal compiler error")
                    .note(format!("the program cannot be lowered: {}", e.message))
                    .help("this is a bug in wolf; please report it with the input that caused it"),
            ),
        }
    }
    drop(inputs);
    Analysis {
        parsed,
        resolved,
        typed,
    }
}

/// The project data the IR keeps; with `dependencies`, the packages of the targets too.
pub fn meta(manifest: &Manifest, dependencies: bool) -> wolf_ir::ProjectMeta<'_> {
    wolf_ir::ProjectMeta {
        name: &manifest.name,
        version: &manifest.version,
        kind: manifest.kind,
        dependencies: if dependencies {
            manifest
                .dependencies
                .iter()
                .map(|(t, d)| (t.name().to_string(), d.clone()))
                .collect()
        } else {
            BTreeMap::new()
        },
    }
}

/// A generated file: its path in the target project and its contents.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutFile {
    /// The path, with `/` separators.
    pub path: String,
    /// The contents.
    pub contents: String,
}

/// Generates the project of `target`, without the tests. An error names a construct the backend
/// does not support yet.
pub fn generate(
    target: Target,
    program: &wolf_ir::Program,
    wolf_version: &str,
) -> Result<Vec<OutFile>, String> {
    Ok(match target {
        Target::Ts => wolf_ts::generate(program, wolf_version, false)?
            .into_iter()
            .map(|f| OutFile {
                path: f.path,
                contents: f.contents,
            })
            .collect(),
        Target::Go => wolf_go::generate(program, wolf_version, false)?
            .files
            .into_iter()
            .map(|f| OutFile {
                path: f.path,
                contents: f.contents,
            })
            .collect(),
        Target::Rust => wolf_rs::generate(program, wolf_version, false)?
            .files
            .into_iter()
            .map(|f| OutFile {
                path: f.path,
                contents: f.contents,
            })
            .collect(),
    })
}
