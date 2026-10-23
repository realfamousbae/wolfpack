//! A program of one file: `src/main.wolf` of a `cli` project with nothing else in it. The
//! playground compiles such a program in the browser, and the fuzzer feeds the compiler with them.

use wolf_diag::{Diagnostic, codes};
use wolf_project::{Manifest, Target};
use wolf_span::SourceMap;

use crate::{Analysis, Native, OutFile, analyze, check_native, generate, meta, parse, std_modules};

/// The manifest of the project.
pub const MANIFEST: &str =
    "[package]\nname = \"playground\"\nversion = \"0.1.0\"\nkind = \"cli\"\n";

/// The name of the file in diagnostics.
pub const FILE: &str = "src/main.wolf";

/// A checked program of one file.
pub struct Single {
    /// The files: the manifest, std and the program.
    pub map: SourceMap,
    /// Every diagnostic so far.
    pub diagnostics: Vec<Diagnostic>,
    project: Option<(Manifest, Analysis)>,
}

/// Parses and checks `source` as the only module of the project.
pub fn check(source: &str) -> Single {
    let mut map = SourceMap::new();
    let mut diagnostics = Vec::new();
    let manifest_id = map.add("wolfpack.toml", MANIFEST.to_string());
    let (manifest, found) = wolf_project::parse_manifest(manifest_id, MANIFEST);
    diagnostics.extend(found);
    let Some(manifest) = manifest else {
        return Single {
            map,
            diagnostics,
            project: None,
        };
    };
    let mut parsed = std_modules(&mut map, &mut diagnostics);
    let id = map.add(FILE, source.to_string());
    let tree = parse(&map, id, &mut diagnostics);
    parsed.push((
        vec!["main".to_string()],
        wolf_resolve::Role::Entry,
        id,
        tree,
    ));
    // There is no `native/` (spec chapter 8, section 8a), so there are no native modules either.
    let analysis = analyze(
        &map,
        &manifest,
        parsed,
        &wolf_resolve::NativeNames::new(),
        &mut diagnostics,
    );
    diagnostics.extend(check_native(&Native::none(), &analysis.parsed));
    Single {
        map,
        diagnostics,
        project: Some((manifest, analysis)),
    }
}

impl Single {
    /// The program for the VM (spec chapter 10); nothing when it has errors. Code of a target
    /// cannot run in the VM (E0807).
    pub fn program_for_vm(&mut self) -> Option<wolf_ir::Program> {
        let (manifest, analysis) = self.project.as_ref()?;
        if !self.diagnostics.is_empty() {
            return None;
        }
        let inputs = analysis.inputs();
        self.diagnostics.extend(wolf_resolve::check_vm(
            &inputs,
            |m| m.role == wolf_resolve::Role::Entry,
            "the playground runs programs in the VM; build it for a target with `wolf build`",
        ));
        if !self.diagnostics.is_empty() {
            return None;
        }
        match wolf_ir::lower(
            &inputs,
            &analysis.resolved,
            &analysis.typed,
            &self.map,
            &meta(manifest, false),
        ) {
            Ok(program) => Some(program),
            Err(e) => {
                self.diagnostics.push(
                    Diagnostic::new(codes::E0003, "internal compiler error")
                        .note(format!("the program cannot be lowered: {}", e.message))
                        .help("this is a bug in wolf; please report it"),
                );
                None
            }
        }
    }

    /// Generates the project of `target`; nothing when the program has errors. Errors of the
    /// target (a module bound to another target) and gaps of a backend become diagnostics.
    pub fn build(&mut self, target: Target) -> Vec<OutFile> {
        let Some((manifest, analysis)) = &self.project else {
            return Vec::new();
        };
        if !self.diagnostics.is_empty() {
            return Vec::new();
        }
        let inputs = analysis.inputs();
        self.diagnostics
            .extend(wolf_resolve::check_target(&inputs, target.name(), |role| {
                role != wolf_resolve::Role::Test
            }));
        if !self.diagnostics.is_empty() {
            return Vec::new();
        }
        let generated = wolf_ir::lower(
            &inputs,
            &analysis.resolved,
            &analysis.typed,
            &self.map,
            &meta(manifest, true),
        )
        .map_err(|e| e.message)
        .and_then(|program| generate(target, &program, env!("CARGO_PKG_VERSION")));
        match generated {
            Ok(files) => files,
            Err(message) => {
                self.diagnostics.push(
                    Diagnostic::new(codes::E0003, "internal compiler error")
                        .note(format!("the backend does not support this yet: {message}"))
                        .help("this is a bug or a gap in wolf; please report it"),
                );
                Vec::new()
            }
        }
    }
}
