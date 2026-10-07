//! UI tests: every `.wolf` file under `tests/ui/` is checked with `wolf check`, and its standard
//! error must equal the `.stderr` file next to it (spec chapter 9, section 5).
//!
//! Run with `WOLF_BLESS=1` to write the `.stderr` files from the current output.

// Test helpers may fail loudly.
#![allow(clippy::unwrap_used)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn ui_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/ui")
}

fn wolf_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            wolf_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "wolf") {
            out.push(path);
        }
    }
}

fn wolf(cwd: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_wolf"))
        .args(args)
        .current_dir(cwd)
        .env_remove("WOLF_TEST_INTERNAL_ERROR")
        .output()
        .unwrap()
}

#[test]
fn ui_files_match_expected_stderr() {
    let root = ui_dir();
    let bless = std::env::var_os("WOLF_BLESS").is_some();
    let mut files = Vec::new();
    wolf_files(&root, &mut files);
    assert!(!files.is_empty());
    let mut failures = Vec::new();
    for path in files {
        let relative = path
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let output = wolf(&root, &["check", "--color", "never", &relative]);
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        let expected_path = path.with_extension("stderr");
        if bless {
            fs::write(&expected_path, &stderr).unwrap();
            continue;
        }
        let expected = fs::read_to_string(&expected_path).unwrap_or_default();
        if stderr != expected {
            failures.push(format!(
                "--- {relative}\nexpected:\n{expected}\nactual:\n{stderr}"
            ));
        }
        // Every UI file contains at least one error.
        if output.status.code() != Some(1) {
            failures.push(format!(
                "--- {relative}: exit code {:?}, expected 1",
                output.status.code()
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn every_lexical_code_has_a_ui_test() {
    let mut files = Vec::new();
    wolf_files(&ui_dir(), &mut files);
    let names: Vec<String> = files
        .iter()
        .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .collect();
    for code in wolf_diag::codes::ALL {
        if code.id().starts_with("E01") {
            assert!(
                names.iter().any(|n| n.starts_with(code.id())),
                "no UI test for {}",
                code.id()
            );
        }
    }
}

#[test]
fn examples_have_no_lexical_errors() {
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let output = wolf(&examples, &["check", "."]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn e0001_missing_path() {
    let output = wolf(
        &ui_dir(),
        &["check", "--color", "never", "no_such_file.wolf"],
    );
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.starts_with(
            "error[E0001]: cannot read `no_such_file.wolf`: no such file or directory\n"
        ),
        "{stderr}"
    );
}

#[test]
fn e0002_bad_usage() {
    let output = wolf(&ui_dir(), &["check", "--message-format", "xml"]);
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.starts_with("error[E0002]: "), "{stderr}");
}

#[test]
fn e0003_internal_error() {
    let output = Command::new(env!("CARGO_BIN_EXE_wolf"))
        .args(["check", "."])
        .current_dir(ui_dir())
        .env("WOLF_TEST_INTERNAL_ERROR", "1")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(3));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.starts_with("error[E0003]: internal compiler error"),
        "{stderr}"
    );
    assert!(!stderr.contains("panicked"), "{stderr}");
}

#[test]
fn json_output_is_one_object_per_line() {
    let output = wolf(
        &ui_dir(),
        &[
            "check",
            "--message-format",
            "json",
            "lexer/E0107_malformed_number.wolf",
        ],
    );
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    let lines: Vec<&str> = stderr.lines().collect();
    assert_eq!(lines.len(), 7, "{stderr}");
    for line in lines {
        assert!(
            line.starts_with("{\"code\":\"E0107\",\"severity\":\"error\""),
            "{line}"
        );
        assert!(line.ends_with('}'));
    }
}

#[test]
fn emit_tokens_prints_the_token_stream() {
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/hello");
    let output = wolf(&examples, &["check", "--emit", "tokens", "src/main.wolf"]);
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.starts_with("== src/main.wolf\n0..6 module\n"),
        "{stdout}"
    );
    assert!(stdout.contains("Str \"Hello, Wolfpack!\""), "{stdout}");
}
