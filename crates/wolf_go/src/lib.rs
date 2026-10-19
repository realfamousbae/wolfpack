//! The Go backend (spec chapter 8).
//!
//! It reads only the IR. Each module becomes a package (spec chapter 8, section 2, rule 1),
//! built as a Go tree and then printed. std and the runtime are the package `wolfrt`, which holds
//! only the pieces the program uses.

mod emit;
mod go;
mod names;
mod runtime;

use wolf_ir::{ModuleRole, Program, ProgramKind};

pub use emit::TestEntry;

/// A generated file, with its path relative to `out/go/`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutFile {
    /// The path, with `/` separators.
    pub path: String,
    /// The contents.
    pub contents: String,
}

/// A generated project.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Project {
    /// The files, sorted by path.
    pub files: Vec<OutFile>,
    /// The tests of a test build, in the order of the report (by file, then by declaration).
    pub tests: Vec<TestEntry>,
}

/// Generates the Go project for a program; with `tests`, a test build that also contains the
/// tests in `*_test.go` files. An error names a construct the backend does not support yet; the
/// driver reports it as an internal compiler error.
pub fn generate(program: &Program, wolf_version: &str, tests: bool) -> Result<Project, String> {
    // `Equal` and `Copy` of a type are generated when something uses them, which is known only
    // after emitting: emit until the set of such types no longer grows.
    let mut e = emit::Emitter::new(program, tests);
    let mut files: Vec<OutFile>;
    let mut rounds = 0;
    loop {
        let before = (e.need_eq.len(), e.need_copy.len(), e.need_copy_iface.len());
        e.test_list.clear();
        e.reset_tests();
        files = Vec::new();
        for (i, m) in program.modules.iter().enumerate() {
            let wanted = match m.role {
                ModuleRole::Entry | ModuleRole::Src | ModuleRole::Native => true,
                ModuleRole::Example => !tests,
                ModuleRole::Test => tests,
                ModuleRole::Std => false,
            };
            if wanted {
                for (path, contents) in e.module(i)? {
                    files.push(OutFile { path, contents });
                }
            }
        }
        rounds += 1;
        if (e.need_eq.len(), e.need_copy.len(), e.need_copy_iface.len()) == before || rounds > 64 {
            break;
        }
    }
    files.push(OutFile {
        path: "wolfrt/wolfrt.go".to_string(),
        contents: runtime::file(&e.rt, wolf_version),
    });
    files.push(OutFile {
        path: "go.mod".to_string(),
        contents: go_mod(program),
    });
    files.sort_by(|a, b| a.path.cmp(&b.path));
    let mut list = e.test_list.clone();
    list.sort_by(|a, b| {
        let file = |t: &TestEntry| t.name.split(".wolf: ").next().unwrap_or("").to_string();
        file(a).cmp(&file(b))
    });
    Ok(Project { files, tests: list })
}

/// The first elements of the import paths of Go's standard library. A module path without a dot
/// in its first element is reserved for it, so a package named like one of these would make its
/// imports ambiguous.
const GO_STD: &[&str] = &[
    "archive",
    "bufio",
    "builtin",
    "bytes",
    "cmd",
    "cmp",
    "compress",
    "container",
    "context",
    "crypto",
    "database",
    "debug",
    "embed",
    "encoding",
    "errors",
    "expvar",
    "flag",
    "fmt",
    "go",
    "hash",
    "html",
    "image",
    "index",
    "internal",
    "io",
    "iter",
    "log",
    "maps",
    "math",
    "mime",
    "net",
    "os",
    "path",
    "plugin",
    "reflect",
    "regexp",
    "runtime",
    "slices",
    "sort",
    "strconv",
    "strings",
    "structs",
    "sync",
    "syscall",
    "testing",
    "text",
    "time",
    "unicode",
    "unique",
    "unsafe",
    "uuid",
    "vendor",
    "weak",
];

/// The module path of `go.mod`: the package name, or `wolfpack.local/<name>` if Go's standard
/// library has a package of that name.
pub(crate) fn module_path(name: &str) -> String {
    if GO_STD.contains(&name) {
        format!("wolfpack.local/{name}")
    } else {
        name.to_string()
    }
}

/// `go.mod`, with the modules of `[extern.go.dependencies]` as requirements.
fn go_mod(program: &Program) -> String {
    let mut out = format!("module {}\n\ngo 1.24\n", module_path(&program.name));
    match program.dependencies.get("go").map(Vec::as_slice) {
        None | Some([]) => {}
        Some([(name, version)]) => out.push_str(&format!("\nrequire {name} {version}\n")),
        Some(list) => {
            out.push_str("\nrequire (\n");
            for (name, version) in list {
                out.push_str(&format!("\t{name} {version}\n"));
            }
            out.push_str(")\n");
        }
    }
    out
}

/// The package to build for `wolf run`: the root package or an example.
pub fn main_package(program: &Program, example: Option<&str>) -> Option<String> {
    match example {
        Some(name) => Some(format!("./examples/{name}")),
        None => match program.kind {
            ProgramKind::Cli => Some(".".to_string()),
            ProgramKind::Lib => None,
        },
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn module_paths_avoid_the_standard_library() {
        assert_eq!(super::module_path("cmp"), "wolfpack.local/cmp");
        assert_eq!(super::module_path("word-count"), "word-count");
    }

    #[test]
    fn test_names_are_pascal_case() {
        assert_eq!(
            super::emit::test_name_for("adds two numbers"),
            "TestAddsTwoNumbers"
        );
    }
}
