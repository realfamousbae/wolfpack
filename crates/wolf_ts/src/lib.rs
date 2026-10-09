//! The TypeScript backend (spec chapter 8).
//!
//! It reads only the IR. Each module becomes a file under `src/` (std modules under `src/std/`,
//! examples under `examples/`), built as a TypeScript tree and then printed. The runtime
//! `src/wolf_rt.ts` holds only the pieces the program uses.

mod emit;
mod names;
mod runtime;
mod ts;

use std::collections::BTreeSet;

use wolf_ir::{ModuleRole, Program, ProgramKind};

/// A generated file, with its path relative to `out/ts/`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutFile {
    /// The path, with `/` separators.
    pub path: String,
    /// The contents.
    pub contents: String,
}

/// Generates the TypeScript project for a program. An error names a construct the backend does
/// not support yet; the driver reports it as an internal compiler error.
pub fn generate(program: &Program, wolf_version: &str) -> Result<Vec<OutFile>, String> {
    let mut e = emit::Emitter::new(program);
    let mut files: Vec<OutFile> = Vec::new();
    let put = |files: &mut Vec<OutFile>, (path, contents): (String, String)| match files
        .iter_mut()
        .find(|f| f.path == path)
    {
        Some(f) => f.contents = contents,
        None => files.push(OutFile { path, contents }),
    };
    for (i, m) in program.modules.iter().enumerate() {
        if matches!(
            m.role,
            ModuleRole::Entry | ModuleRole::Src | ModuleRole::Example
        ) {
            if let Some(f) = e.module(i)? {
                put(&mut files, f);
            }
        }
    }
    // Std modules that the program refers to, until no new ones are needed.
    let mut emitted: BTreeSet<usize> = BTreeSet::new();
    loop {
        let pending: Vec<usize> = e.wanted.difference(&emitted).copied().collect();
        if pending.is_empty() {
            break;
        }
        for i in pending {
            emitted.insert(i);
            if let Some(f) = e.module(i)? {
                put(&mut files, f);
            }
        }
    }
    // Re-emit std modules once more so that snippets requested later are included.
    for i in emitted.clone() {
        if let Some(f) = e.module(i)? {
            put(&mut files, f);
        }
    }
    files.push(OutFile {
        path: "src/wolf_rt.ts".to_string(),
        contents: emit::runtime_file(&e.rt, wolf_version),
    });
    files.push(OutFile {
        path: "package.json".to_string(),
        contents: package_json(program),
    });
    files.push(OutFile {
        path: "tsconfig.json".to_string(),
        contents: TSCONFIG.to_string(),
    });
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

/// The command that runs the built program: the entry file under `dist/`.
pub fn entry_file(program: &Program, example: Option<&str>) -> String {
    match example {
        Some(name) => format!("dist/examples/{name}.js"),
        None => match program.kind {
            ProgramKind::Cli => "dist/src/main.js".to_string(),
            ProgramKind::Lib => "dist/src/lib.js".to_string(),
        },
    }
}

const TSCONFIG: &str = r#"{
  "compilerOptions": {
    "strict": true,
    "target": "ES2022",
    "lib": ["ES2022"],
    "module": "NodeNext",
    "moduleResolution": "NodeNext",
    "rootDir": ".",
    "outDir": "dist",
    "skipLibCheck": true
  },
  "include": ["src", "examples"]
}
"#;

fn package_json(program: &Program) -> String {
    let exports = match program.kind {
        ProgramKind::Lib => "  \"exports\": \"./dist/src/lib.js\",\n",
        ProgramKind::Cli => "",
    };
    format!(
        "{{\n  \"name\": \"{}\",\n  \"version\": \"{}\",\n  \"private\": true,\n  \"type\": \"module\",\n{exports}  \"scripts\": {{\n    \"build\": \"tsc\"\n  }},\n  \"devDependencies\": {{\n    \"typescript\": \"^5.0.0\"\n  }}\n}}\n",
        program.name, program.version
    )
}
