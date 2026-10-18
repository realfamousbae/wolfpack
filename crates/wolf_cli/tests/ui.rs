//! UI tests (spec chapter 9, section 5):
//! - every `.wolf` file under `tests/ui/lexer` and `tests/ui/parser` is checked alone with
//!   `wolf check <file>`;
//! - every directory under `tests/ui/project` is a project checked with `wolf check` inside it.
//!
//! Standard error must equal the `.stderr` file next to the input.
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
            if path.file_name().is_some_and(|n| n == "project") {
                continue;
            }
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

fn project_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = fs::read_dir(ui_dir().join("project"))
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    dirs
}

#[test]
fn ui_projects_match_expected_stderr() {
    let bless = std::env::var_os("WOLF_BLESS").is_some();
    let mut failures = Vec::new();
    for dir in project_dirs() {
        let name = dir.file_name().unwrap().to_string_lossy().into_owned();
        let output = wolf(&dir, &["check", "--color", "never"]);
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        let expected_path = dir.with_extension("stderr");
        if bless {
            fs::write(&expected_path, &stderr).unwrap();
            continue;
        }
        let expected = fs::read_to_string(&expected_path).unwrap_or_default();
        if stderr != expected {
            failures.push(format!(
                "--- {name}\nexpected:\n{expected}\nactual:\n{stderr}"
            ));
        }
        if output.status.code() != Some(1) {
            failures.push(format!(
                "--- {name}: exit code {:?}, expected 1",
                output.status.code()
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Every source diagnostic code has a UI test named after it: a file, or a project directory.
/// Driver codes (E00xx) are tested by the CLI tests below.
#[test]
fn every_code_has_a_ui_test() {
    let mut files = Vec::new();
    wolf_files(&ui_dir(), &mut files);
    let mut names: Vec<String> = files
        .iter()
        .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        .collect();
    names.extend(
        project_dirs()
            .iter()
            .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().into_owned())),
    );
    // Toolchain errors (E08xx) are tested by `tests/run.rs` with projects in `tests/run/`.
    let run = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/run");
    names.extend(
        fs::read_dir(&run)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned()),
    );
    // `wolf setup` has no project: E0804 is tested by `e0804_setup_cannot_download` there.
    names.push("E0804_setup_cannot_download".to_string());
    names.push("E0805_update_cannot_download".to_string());
    for code in wolf_diag::codes::ALL {
        if !code.id().starts_with("E00") {
            assert!(
                names.iter().any(|n| n.starts_with(code.id())),
                "no UI test for {}",
                code.id()
            );
        }
    }
}

/// The acceptance test of the milestones so far: every example project checks cleanly.
#[test]
fn every_example_project_checks_cleanly() {
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let mut projects: Vec<PathBuf> = fs::read_dir(&examples)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join("wolfpack.toml").is_file())
        .collect();
    projects.sort();
    assert_eq!(projects.len(), 12);
    for project in projects {
        let output = wolf(&project, &["check", "--color", "never"]);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(
            output.status.code(),
            Some(0),
            "{}: {stderr}",
            project.display()
        );
        assert!(stderr.is_empty(), "{stderr}");
        // The project is also found from a subdirectory, with the same output.
        let from_src = wolf(&project.join("src"), &["check", "--color", "never"]);
        assert_eq!(from_src.status.code(), Some(0));
    }
}

/// Output does not depend on the directory `wolf check` runs from inside the project.
#[test]
fn output_is_the_same_from_a_subdirectory() {
    for name in ["E0604_import_cycle", "E0618_unused_variable"] {
        let dir = ui_dir().join("project").join(name);
        let expected = fs::read_to_string(dir.with_extension("stderr")).unwrap();
        let output = wolf(&dir.join("src"), &["check", "--color", "never"]);
        assert_eq!(String::from_utf8_lossy(&output.stderr), expected, "{name}");
    }
}

#[test]
fn e0004_no_manifest() {
    let dir = std::env::temp_dir().join(format!("wolf_no_manifest_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let output = wolf(&dir, &["check", "--color", "never"]);
    let _ = fs::remove_dir_all(&dir);
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.starts_with("error[E0004]: no `wolfpack.toml` found"),
        "{stderr}"
    );
}

#[test]
fn wolf_new_creates_projects_that_check() {
    let base = std::env::temp_dir().join(format!("wolf_new_{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(&base).unwrap();
    for (name, kind) in [("my-app", "cli"), ("my-lib", "lib")] {
        let output = wolf(&base, &["new", name, "--kind", kind]);
        assert_eq!(
            output.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let project = base.join(name);
        assert!(project.join(".gitignore").is_file());
        assert!(project.join("tests").is_dir());
        let check = wolf(&project, &["check", "--color", "never"]);
        assert_eq!(
            check.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&check.stderr)
        );
    }
    // An existing non-empty directory and an invalid name are refused.
    let again = wolf(&base, &["new", "my-app"]);
    assert_eq!(again.status.code(), Some(2));
    let bad = wolf(&base, &["new", "My_App"]);
    assert_eq!(bad.status.code(), Some(2));
    let _ = fs::remove_dir_all(&base);
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

/// The simulated internal error exists only in debug builds.
#[cfg(debug_assertions)]
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
fn e0002_missing_command() {
    let output = wolf(&ui_dir(), &[]);
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.starts_with("error[E0002]: no command given\n"),
        "{stderr}"
    );
}

/// A symbolic link back to an ancestor must not make `wolf check` loop or report twice.
#[cfg(unix)]
#[test]
fn symlink_loops_are_not_followed() {
    let dir = std::env::temp_dir().join(format!("wolf_symlink_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("wolfpack.toml"),
        "[package]\nname = \"loop\"\nversion = \"0.1.0\"\nkind = \"cli\"\n",
    )
    .unwrap();
    fs::write(dir.join("src/main.wolf"), "module main\n@\n").unwrap();
    std::os::unix::fs::symlink(&dir, dir.join("src/loop")).unwrap();
    let output = wolf(&dir, &["check", "--color", "never"]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let _ = fs::remove_dir_all(&dir);
    assert_eq!(stderr.matches("error[E0102]").count(), 1, "{stderr}");
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
    assert!(stdout.contains("Str \"Hello, wolfpack!\""), "{stdout}");
}

/// The valid half of each ownership rule (spec chapter 5, section 10): projects under
/// `tests/valid/` check without errors.
#[test]
fn valid_projects_check_cleanly() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/valid");
    let mut failures = Vec::new();
    for entry in fs::read_dir(&root).unwrap().flatten() {
        let dir = entry.path();
        let output = wolf(&dir, &["check", "--color", "never"]);
        if output.status.code() != Some(0) {
            failures.push(format!(
                "--- {}\n{}",
                dir.display(),
                String::from_utf8_lossy(&output.stderr)
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
