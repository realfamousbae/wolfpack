//! The examples, std and the test programs are already in the layout of `wolf fmt`, and
//! formatting is idempotent.

// Test helpers may fail loudly.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::fs;
use std::path::{Path, PathBuf};

use wolf_span::FileId;

fn wolf_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    entries.sort();
    for path in entries {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if path.is_dir() {
            if name != "out" && name != "target" && name != "node_modules" {
                wolf_files(&path, out);
            }
        } else if name.ends_with(".wolf") {
            out.push(path);
        }
    }
}

fn sources(roots: &[&str]) -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = Vec::new();
    for r in roots {
        wolf_files(&root.join(r), &mut files);
    }
    files
}

/// The first differing line of two texts.
fn first_difference(a: &str, b: &str) -> String {
    for (i, (x, y)) in a.lines().zip(b.lines()).enumerate() {
        if x != y {
            return format!("line {}:\n  source: {x}\n  fmt:    {y}", i + 1);
        }
    }
    format!(
        "lengths differ: {} and {} lines",
        a.lines().count(),
        b.lines().count()
    )
}

#[test]
fn examples_and_std_are_formatted() {
    let mut failures = Vec::new();
    for path in sources(&[
        "examples",
        "crates/wolf_std/std",
        "crates/wolf_cli/tests/run",
        "crates/wolf_cli/tests/test",
        "crates/wolf_cli/tests/native",
    ]) {
        let source = fs::read_to_string(&path).unwrap();
        match wolf_fmt::format(FileId::ZERO, &source) {
            Ok(out) if out == source => {}
            Ok(out) => failures.push(format!(
                "{}: {}",
                path.display(),
                first_difference(&source, &out)
            )),
            Err(e) => failures.push(format!("{}: {e:?}", path.display())),
        }
    }
    assert!(
        failures.is_empty(),
        "{} files:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// Each `tests/cases/<name>.wolf` formats to `<name>.formatted`, which is stable.
#[test]
fn cases_format_to_their_expected_text() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/cases");
    let mut inputs: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "wolf"))
        .collect();
    inputs.sort();
    assert!(!inputs.is_empty());
    for input in inputs {
        let source = fs::read_to_string(&input).unwrap();
        let expected = fs::read_to_string(input.with_extension("formatted")).unwrap();
        let out = wolf_fmt::format(FileId::ZERO, &source).unwrap();
        assert_eq!(out, expected, "{}", input.display());
        let again = wolf_fmt::format(FileId::ZERO, &out).unwrap();
        assert_eq!(again, out, "not stable: {}", input.display());
    }
}

/// A file with syntax errors is not formatted.
#[test]
fn syntax_errors_are_returned() {
    let result = wolf_fmt::format(FileId::ZERO, "module main\n\nfunctions {\n    fn f( {\n}\n");
    assert!(matches!(result, Err(wolf_fmt::FmtError::Syntax(d)) if !d.is_empty()));
}
