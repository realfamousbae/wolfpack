//! The project layout (spec chapter 6, sections 1 and 7).

use std::fs;
use std::path::{Path, PathBuf};

use wolf_diag::{Diagnostic, codes};

use crate::manifest::{Kind, Manifest};

/// What a source file is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Role {
    /// `src/main.wolf` or `src/lib.wolf`.
    Entry,
    /// Another file under `src/`.
    Src,
    /// A file under `tests/`.
    Test,
    /// A file under `examples/`.
    Example,
}

/// One `.wolf` file of a project.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModuleFile {
    /// The module path derived from the file path, for example `["users", "storage"]`.
    pub module: Vec<String>,
    /// The role of the file.
    pub role: Role,
    /// The file on disk.
    pub path: PathBuf,
    /// The path relative to the project root with `/`, as shown in diagnostics.
    pub display: String,
}

/// Reserved words that cannot be module path segments (spec chapter 1, section 4).
const RESERVED: &[&str] = &[
    "module",
    "extern",
    "impl",
    "fn",
    "pub",
    "let",
    "var",
    "type",
    "struct",
    "enum",
    "interface",
    "if",
    "else",
    "match",
    "while",
    "for",
    "in",
    "return",
    "break",
    "continue",
    "mut",
    "own",
    "self",
    "true",
    "false",
    "async",
    "await",
];

/// Finds the project root: the nearest directory, starting at `start`, containing
/// `wolfpack.toml` (spec chapter 6, rule 6.6).
pub fn find_root(start: &Path) -> Option<PathBuf> {
    let mut dir = Some(start);
    while let Some(current) = dir {
        if current.join("wolfpack.toml").is_file() {
            return Some(current.to_path_buf());
        }
        dir = current.parent();
    }
    None
}

/// The directories of `native/`, one per target (spec chapter 6, section 7, rule 7).
pub const NATIVE_DIRS: &[&str] = &["ts", "go", "rust"];

/// The files of `native/<target>/` with their paths relative to it, in a fixed order; empty
/// when the directory does not exist. Hidden files and directories are left out.
pub fn native_files(root: &Path, target: &str) -> Vec<(PathBuf, PathBuf)> {
    fn walk(dir: &Path, rel: &Path, out: &mut Vec<(PathBuf, PathBuf)>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        let mut entries: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
        entries.sort();
        for path in entries {
            let Some(name) = path.file_name() else {
                continue;
            };
            if name.to_string_lossy().starts_with('.') {
                continue;
            }
            let rel = rel.join(name);
            if path.is_dir() {
                walk(&path, &rel, out);
            } else {
                out.push((rel, path));
            }
        }
    }
    let mut out = Vec::new();
    walk(&root.join("native").join(target), Path::new(""), &mut out);
    out
}

/// Lists the `.wolf` files of the project in a fixed order and reports layout errors (E0612).
pub fn discover(root: &Path, manifest: &Manifest) -> (Vec<ModuleFile>, Vec<Diagnostic>) {
    let mut out = Walk {
        root: root.to_path_buf(),
        files: Vec::new(),
        diagnostics: Vec::new(),
    };
    let src = root.join("src");
    if !src.is_dir() {
        out.diagnostics.push(
            Diagnostic::new(codes::E0612, "the project has no `src/` directory")
                .help("create `src/` with the entry file"),
        );
        return (out.files, out.diagnostics);
    }
    out.walk(&src, &mut Vec::new(), Area::Src);
    for (dir, area, prefix) in [
        ("tests", Area::Tests, "tests"),
        ("examples", Area::Examples, "examples"),
    ] {
        let path = root.join(dir);
        if path.is_dir() {
            out.walk(&path, &mut vec![prefix.to_string()], area);
        }
    }
    out.check_native();
    out.check_entry(manifest);
    out.files.sort_by(|a, b| a.display.cmp(&b.display));
    (out.files, out.diagnostics)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Area {
    Src,
    Tests,
    Examples,
}

struct Walk {
    root: PathBuf,
    files: Vec<ModuleFile>,
    diagnostics: Vec<Diagnostic>,
}

impl Walk {
    /// `native/` holds only the directories of the targets.
    fn check_native(&mut self) {
        let Ok(entries) = std::fs::read_dir(self.root.join("native")) else {
            return;
        };
        let mut names: Vec<String> = entries
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| !n.starts_with('.'))
            .filter(|n| {
                !(NATIVE_DIRS.contains(&n.as_str()) && self.root.join("native").join(n).is_dir())
            })
            .collect();
        names.sort();
        for name in names {
            self.diagnostics.push(
                Diagnostic::new(codes::E0612, format!("unexpected `native/{name}`"))
                    .help("`native/` holds only the directories `ts`, `go` and `rust`"),
            );
        }
    }

    fn display(&self, path: &Path) -> String {
        path.strip_prefix(&self.root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/")
    }

    fn layout_error(&mut self, message: String, help: &str) {
        self.diagnostics
            .push(Diagnostic::new(codes::E0612, message).help(help.to_string()));
    }

    fn walk(&mut self, dir: &Path, module: &mut Vec<String>, area: Area) {
        let Ok(entries) = fs::read_dir(dir) else {
            let shown = self.display(dir);
            self.diagnostics.push(Diagnostic::new(
                codes::E0001,
                format!("cannot read `{shown}`"),
            ));
            return;
        };
        let mut children: Vec<(PathBuf, bool, bool)> = entries
            .flatten()
            .map(|e| {
                let file_type = e.file_type().ok();
                let is_dir = file_type.is_some_and(|t| t.is_dir());
                let is_link = file_type.is_some_and(|t| t.is_symlink());
                (e.path(), is_dir, is_link)
            })
            .collect();
        children.sort();
        let names: Vec<String> = children
            .iter()
            .filter_map(|(p, _, _)| p.file_name().and_then(|n| n.to_str()).map(str::to_string))
            .collect();
        for (path, is_dir, is_link) in children {
            let shown = self.display(&path);
            let Some(name) = path
                .file_name()
                .and_then(|n| n.to_str())
                .map(str::to_string)
            else {
                self.layout_error(
                    format!("file name is not valid UTF-8: `{shown}`"),
                    "rename the file using ASCII letters, digits and `_`",
                );
                continue;
            };
            if name.starts_with('.') {
                continue;
            }
            if is_link {
                // Symbolic links are not followed, so the walk cannot loop.
                continue;
            }
            if is_dir {
                if !self.check_segment(&name, &shown) {
                    continue;
                }
                if area == Area::Src
                    && module.is_empty()
                    && ["std", "tests", "examples", "native"].contains(&name.as_str())
                {
                    self.layout_error(
                        format!("`{shown}/` uses the reserved module name `{name}`"),
                        "`std`, `tests`, `examples` and `native` are reserved root names; rename the directory",
                    );
                    continue;
                }
                // `src/users.wolf` next to `src/users/` would be an index file (rule 1.5).
                if names.iter().any(|n| *n == format!("{name}.wolf")) {
                    self.layout_error(
                        format!("`{shown}.wolf` and the directory `{shown}/` both exist"),
                        "a directory is only a namespace; move the code of the file into a module inside the directory",
                    );
                }
                module.push(name);
                self.walk(&path, module, area);
                module.pop();
                continue;
            }
            let Some(stem) = name.strip_suffix(".wolf") else {
                if area == Area::Src {
                    self.layout_error(
                        format!("unexpected file `{shown}`"),
                        "`src/` contains only `.wolf` files and directories",
                    );
                }
                continue;
            };
            if !self.check_segment(stem, &shown) {
                continue;
            }
            let mut module_path = module.clone();
            module_path.push(stem.to_string());
            let role = match area {
                Area::Src if module.is_empty() && (stem == "main" || stem == "lib") => Role::Entry,
                Area::Src => Role::Src,
                Area::Tests => Role::Test,
                Area::Examples => Role::Example,
            };
            if area == Area::Src && module.is_empty() {
                if let Some(reserved) = ["std", "tests", "examples", "native"]
                    .iter()
                    .find(|r| **r == stem)
                {
                    self.layout_error(
                        format!("`{shown}` uses the reserved module name `{reserved}`"),
                        "`std`, `tests`, `examples` and `native` are reserved root names; rename the file",
                    );
                    continue;
                }
            }
            self.files.push(ModuleFile {
                module: module_path,
                role,
                path,
                display: shown,
            });
        }
    }

    /// File and directory names are module path segments: `snake_case`, not a keyword.
    fn check_segment(&mut self, name: &str, shown: &str) -> bool {
        if RESERVED.contains(&name) {
            self.layout_error(
                format!("`{shown}` uses the keyword `{name}` as a module name"),
                "rename it",
            );
            return false;
        }
        if !is_snake_case(name) {
            self.layout_error(
                format!("`{shown}` is not a valid module name"),
                "file and directory names are `snake_case`: lower-case words separated by `_`",
            );
            return false;
        }
        true
    }

    fn check_entry(&mut self, manifest: &Manifest) {
        let has = |files: &[ModuleFile], name: &str| {
            files
                .iter()
                .any(|f| f.role == Role::Entry && f.module == [name.to_string()])
        };
        let (wanted, other) = match manifest.kind {
            Kind::Cli => ("main", "lib"),
            Kind::Lib => ("lib", "main"),
        };
        let kind = match manifest.kind {
            Kind::Cli => "cli",
            Kind::Lib => "lib",
        };
        if !has(&self.files, wanted) {
            self.diagnostics.push(
                Diagnostic::new(
                    codes::E0612,
                    format!("missing entry file `src/{wanted}.wolf`"),
                )
                .primary(
                    manifest.kind_span,
                    format!("a `{kind}` project needs `src/{wanted}.wolf`"),
                ),
            );
        }
        if has(&self.files, other) {
            self.diagnostics.push(
                Diagnostic::new(
                    codes::E0612,
                    format!("unexpected entry file `src/{other}.wolf`"),
                )
                .primary(
                    manifest.kind_span,
                    format!("a `{kind}` project has no `src/{other}.wolf`"),
                )
                .help(format!(
                    "rename the file, or change `kind` to `\"{}\"`",
                    if other == "main" { "cli" } else { "lib" }
                )),
            );
            // The wrong entry is treated as an ordinary module.
            for file in &mut self.files {
                if file.role == Role::Entry && file.module == [other.to_string()] {
                    file.role = Role::Src;
                }
            }
        }
    }
}

/// `[a-z][a-z0-9]*(_[a-z0-9]+)*` (spec chapter 1, section 3).
pub fn is_snake_case(name: &str) -> bool {
    let mut parts = name.split('_');
    let first_ok = parts.next().is_some_and(|p| {
        p.starts_with(|c: char| c.is_ascii_lowercase())
            && p.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
    });
    first_ok
        && parts.all(|p| {
            !p.is_empty()
                && p.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        })
}
