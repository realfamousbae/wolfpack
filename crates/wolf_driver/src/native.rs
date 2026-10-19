//! Code of the project in `native/<target>/` (spec chapter 7, section 8.2a; chapter 8,
//! section 8a): which files there are, and the checks of the `native:` paths that name them
//! (E0709) and of the generated paths they take (E0710).

use std::collections::BTreeMap;
use std::path::Path;

use wolf_ast::ItemKind;
use wolf_diag::{Diagnostic, codes};
use wolf_resolve::Role;

use crate::Parsed;

/// The prefix of a path string that names code of the project.
pub const NATIVE_PREFIX: &str = "native:";

/// The files of `native/`: for each target (`ts`, `go`, `rust`), their paths relative to
/// `native/<target>/`, with `/` separators, in a fixed order.
#[derive(Clone, Debug, Default)]
pub struct Native {
    files: BTreeMap<String, Vec<String>>,
}

impl Native {
    /// The files of `native/` of the project at `root`.
    pub fn load(root: &Path) -> Native {
        let mut files = BTreeMap::new();
        for target in wolf_project::NATIVE_DIRS {
            let list: Vec<String> = wolf_project::native_files(root, target)
                .into_iter()
                .map(|(rel, _)| {
                    rel.components()
                        .map(|c| c.as_os_str().to_string_lossy().into_owned())
                        .collect::<Vec<_>>()
                        .join("/")
                })
                .collect();
            files.insert((*target).to_string(), list);
        }
        Native { files }
    }

    /// A project without native code, such as the playground.
    pub fn none() -> Native {
        Native::default()
    }

    /// The files of `native/<target>/`.
    pub fn files(&self, target: &str) -> &[String] {
        self.files.get(target).map_or(&[], Vec::as_slice)
    }

    fn has(&self, target: &str, file: &str) -> bool {
        self.files(target).iter().any(|f| f == file)
    }

    /// Whether `native/go/<dir>/` holds a Go file directly.
    fn has_go_package(&self, dir: &str) -> bool {
        self.files("go").iter().any(|f| {
            f.strip_prefix(dir)
                .and_then(|rest| rest.strip_prefix('/'))
                .is_some_and(|name| !name.contains('/') && name.ends_with(".go"))
        })
    }

    /// The file of the Rust module `segments` (`a/b.rs` or `a/b/mod.rs`), if there is one.
    fn rust_module(&self, segments: &[&str]) -> Option<String> {
        let base = segments.join("/");
        [format!("{base}.rs"), format!("{base}/mod.rs")]
            .into_iter()
            .find(|f| self.has("rust", f))
    }
}

/// Where a `native:` path of the target `target` points: the file or package directory inside
/// `native/<target>/`, or `None` if `native/<target>/` does not have it. `rest` is the path
/// without `native:`, already of the form of chapter 7, section 8.2a (E0705).
pub fn native_target(native: &Native, target: &str, rest: &str) -> Option<String> {
    match target {
        "ts" => {
            let (file, _) = rest.split_once('#')?;
            let file = format!("{file}.ts");
            native.has("ts", &file).then_some(file)
        }
        "go" => {
            let dir = go_package_dir(rest)?;
            native.has_go_package(dir).then(|| dir.to_string())
        }
        _ => {
            let segments: Vec<&str> = rest.split("::").collect();
            // The longest prefix that is a module: `slug::Index::new` is in `slug`.
            (1..segments.len())
                .rev()
                .find_map(|k| native.rust_module(segments.get(..k)?))
        }
    }
}

/// The package directory of a Go `native:` path without its prefix: `text/slug` of
/// `text/slug.Slugify`, `(*text/slug.Index).Add` and `*text/slug.Index`.
pub fn go_package_dir(rest: &str) -> Option<&str> {
    let rest = rest.strip_prefix('(').unwrap_or(rest);
    let rest = rest.strip_prefix('*').unwrap_or(rest);
    let start = rest.rfind('/').map_or(0, |i| i + 1);
    let dot = rest.get(start..)?.find('.')? + start;
    rest.get(..dot)
}

/// Checks the `native:` paths of the project's modules against `native/` (E0709) and the
/// generated paths that the native files take (E0710).
pub fn check_native(native: &Native, parsed: &[Parsed]) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for (_, role, _, file) in parsed {
        if matches!(role, Role::Std | Role::Prelude) {
            continue;
        }
        for section in &file.sections {
            for item in &section.items {
                let ItemKind::ExternTarget(block) = &item.kind else {
                    continue;
                };
                let target = block.target.name.as_str();
                for decl in &block.decls {
                    let Some(rest) = decl.from.value.strip_prefix(NATIVE_PREFIX) else {
                        continue;
                    };
                    if native_target(native, target, rest).is_some() {
                        continue;
                    }
                    out.push(missing(target, rest, decl.from.span));
                }
            }
        }
    }
    // `wolf` next to the native code holds the generated modules of native modules.
    for target in wolf_project::NATIVE_DIRS {
        let taken = native.files(target).iter().find(|f| {
            f.split('/').next().is_some_and(|first| {
                first == "wolf"
                    || first.strip_suffix(".rs").or(first.strip_suffix(".ts")) == Some("wolf")
            })
        });
        if let Some(file) = taken {
            out.push(
                Diagnostic::new(
                    codes::E0710,
                    format!("`native/{target}/{file}` takes the path of generated code"),
                )
                .note(format!(
                    "`wolf build` writes the code of native modules to `{}wolf/`",
                    copied_to(target)
                ))
                .help("rename it: `wolf` is taken next to the native code"),
            );
        }
    }
    if native.has("rust", "mod.rs") {
        out.push(
            Diagnostic::new(
                codes::E0710,
                "`native/rust/mod.rs` takes the path of a generated file",
            )
            .note("`wolf build` generates `src/native/mod.rs`, which declares the modules of `native/rust/`")
            .help("move the code of `native/rust/mod.rs` into a module of its own, such as `native/rust/util.rs`"),
        );
    }
    out
}

/// Where the files of `native/<target>/` are copied in the generated project.
pub fn copied_to(target: &str) -> &'static str {
    match target {
        "ts" => "out/ts/src/native/",
        "go" => "out/go/native/",
        _ => "out/rust/src/native/",
    }
}

fn missing(target: &str, rest: &str, span: wolf_span::Span) -> Diagnostic {
    let (what, wanted) = match target {
        "ts" => (
            "file",
            format!(
                "native/ts/{}.ts",
                rest.split_once('#').map_or(rest, |(f, _)| f)
            ),
        ),
        "go" => (
            "package",
            format!("native/go/{}/", go_package_dir(rest).unwrap_or(rest)),
        ),
        _ => {
            let segments: Vec<&str> = rest.split("::").collect();
            let module = segments
                .get(..segments.len().saturating_sub(1))
                .unwrap_or(&[])
                .join("/");
            ("module", format!("native/rust/{module}.rs"))
        }
    };
    Diagnostic::new(
        codes::E0709,
        format!("the native {what} `{wanted}` does not exist"),
    )
    .primary(span, format!("no `{wanted}` in the project"))
    .help(format!(
        "create `{wanted}`, or correct the path after `native:`"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn native(files: &[(&str, &[&str])]) -> Native {
        Native {
            files: files
                .iter()
                .map(|(t, f)| (t.to_string(), f.iter().map(|s| s.to_string()).collect()))
                .collect(),
        }
    }

    #[test]
    fn targets_of_native_paths() {
        let n = native(&[
            ("ts", &["slug.ts", "text/case.ts"]),
            ("go", &["slug/slug.go", "slug/doc.txt", "text/case/case.go"]),
            ("rust", &["slug.rs", "text/mod.rs", "text/case.rs"]),
        ]);
        assert_eq!(
            native_target(&n, "ts", "slug#slugify").as_deref(),
            Some("slug.ts")
        );
        assert_eq!(
            native_target(&n, "ts", "text/case#upper").as_deref(),
            Some("text/case.ts")
        );
        assert_eq!(native_target(&n, "ts", "case#upper"), None);
        assert_eq!(
            native_target(&n, "go", "slug.Slugify").as_deref(),
            Some("slug")
        );
        assert_eq!(
            native_target(&n, "go", "(*text/case.Builder).Add").as_deref(),
            Some("text/case")
        );
        assert_eq!(native_target(&n, "go", "text.Upper"), None);
        assert_eq!(
            native_target(&n, "rust", "slug::slugify").as_deref(),
            Some("slug.rs")
        );
        assert_eq!(
            native_target(&n, "rust", "slug::Index::new").as_deref(),
            Some("slug.rs")
        );
        assert_eq!(
            native_target(&n, "rust", "text::case::upper").as_deref(),
            Some("text/case.rs")
        );
        assert_eq!(
            native_target(&n, "rust", "text::upper").as_deref(),
            Some("text/mod.rs")
        );
        assert_eq!(native_target(&n, "rust", "case::upper"), None);
    }
}
