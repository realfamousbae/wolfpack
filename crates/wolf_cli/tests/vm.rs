//! The differential tests of the virtual machine (spec chapter 10, section 5): the examples and
//! the end-to-end programs give in `wolf run --vm` and `wolf test --vm` exactly what they give on
//! the targets. These tests need no tools of the targets.

// Test helpers may fail loudly.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn wolf(project: &Path, args: &[&str], input: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_wolf"))
        .args(args)
        .current_dir(project)
        .env("WOLFPACK_NO_UPDATE_CHECK", "1")
        .env("WOLF_VM_TEST", "yes")
        // No tools at all: the VM needs none.
        .env("PATH", "")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(input.as_bytes()).unwrap();
    }
    child.wait_with_output().unwrap()
}

fn dirs(root: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = fs::read_dir(root)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join("wolfpack.toml").is_file())
        .collect();
    dirs.sort();
    dirs
}

fn name(dir: &Path) -> String {
    dir.file_name().unwrap().to_string_lossy().into_owned()
}

/// Modules of these programs call code of the targets: E0807 instead of their output.
fn uses_extern(name: &str) -> bool {
    name.starts_with("E0704") || name.starts_with("extern_") || name.starts_with("native_")
}

/// The output, error output and exit code of a program against its `expected.*` files.
fn compare(dir: &Path, output: &Output) -> Option<String> {
    let read = |f: &str, default: &str| {
        fs::read_to_string(dir.join(f)).unwrap_or_else(|_| default.to_string())
    };
    let (e_out, e_err, e_code) = (
        read("expected.txt", ""),
        read("expected.stderr", ""),
        read("expected.code", "0"),
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let code = output.status.code().unwrap_or(-1).to_string();
    if stdout != e_out || stderr != e_err || code != e_code.trim() {
        Some(format!(
            "--- {}\nexpected (exit {e_code}):\n{e_out}{e_err}\nactual (exit {code}):\n{stdout}{stderr}",
            name(dir)
        ))
    } else {
        None
    }
}

/// Milestone M13: every example without `extern` prints its `expected.txt` in the VM; the
/// examples that call code of the targets report E0807.
#[test]
fn every_example_runs_in_the_vm() {
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let projects = dirs(&examples);
    assert_eq!(projects.len(), 13);
    let mut failures = Vec::new();
    for project in projects {
        let name = name(&project);
        let mut args = vec!["run", "--vm"];
        if name == "textkit" {
            args.extend(["--example", "demo"]);
        }
        let output = wolf(&project, &args, "");
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let ok = if matches!(name.as_str(), "fuzzy" | "textkit" | "native-slug") {
            output.status.code() == Some(1) && stderr.contains("error[E0807]")
        } else {
            let expected = fs::read_to_string(project.join("expected.txt")).unwrap();
            output.status.code() == Some(0) && stdout == expected
        };
        if !ok {
            failures.push(format!(
                "--- {name}: exit {:?}\nstdout:\n{stdout}\nstderr:\n{stderr}",
                output.status.code()
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The programs of `tests/run/` give in the VM the output they give on the targets; E0807 has its
/// own project there.
#[test]
fn run_programs_match_expected_output_in_the_vm() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/run");
    let mut failures = Vec::new();
    for dir in dirs(&root) {
        let name = name(&dir);
        if (name.starts_with("E08") && !name.starts_with("E0807")) || uses_extern(&name) {
            continue;
        }
        let output = wolf(&dir, &["run", "--vm"], "");
        failures.extend(compare(&dir, &output));
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The programs of `tests/vm/`: what the targets cannot share with the VM in `tests/run/`, such
/// as unbounded recursion, standard input and arguments (`input.txt`, `args.txt`).
#[test]
fn vm_programs_match_expected_output() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/vm");
    let mut failures = Vec::new();
    for dir in dirs(&root) {
        let input = fs::read_to_string(dir.join("input.txt")).unwrap_or_default();
        let extra = fs::read_to_string(dir.join("args.txt")).unwrap_or_default();
        let mut args = vec!["run", "--vm", "--"];
        args.extend(extra.split_whitespace());
        let output = wolf(&dir, &args, &input);
        let _ = fs::remove_file(dir.join("out.txt"));
        failures.extend(compare(&dir, &output));
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The projects of `tests/test/` give the report of `wolf test` in the VM.
#[test]
fn test_reports_match_expected_output_in_the_vm() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/test");
    let mut failures = Vec::new();
    for dir in dirs(&root) {
        let name = name(&dir);
        let mut runs = vec![(None, "expected.txt")];
        if let Ok(filter) = fs::read_to_string(dir.join("filter.txt")) {
            runs.push((Some(filter.trim().to_string()), "expected_filtered.txt"));
        }
        for (filter, file) in runs {
            let mut args = vec!["test", "--vm"];
            args.extend(filter.as_deref());
            let output = wolf(&dir, &args, "");
            let actual = format!(
                "{}exit code: {}\n",
                String::from_utf8_lossy(&output.stdout),
                output.status.code().unwrap_or(-1)
            );
            let expected = fs::read_to_string(dir.join(file)).unwrap();
            if actual != expected {
                failures.push(format!(
                    "--- {name} ({file})\nexpected:\n{expected}\nactual:\n{actual}"
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The tests of the examples pass in the VM.
#[test]
fn example_tests_pass_in_the_vm() {
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let mut failures = Vec::new();
    for project in dirs(&examples) {
        let name = name(&project);
        if matches!(name.as_str(), "fuzzy" | "textkit" | "native-slug") {
            continue;
        }
        let output = wolf(&project, &["test", "--vm"], "");
        let stdout = String::from_utf8_lossy(&output.stdout);
        if output.status.code() != Some(0) || !stdout.contains("test result: ok.") {
            failures.push(format!("--- {name}\n{stdout}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `--vm` and `--target` exclude each other (spec chapter 10, section 1, rule 3).
#[test]
fn vm_and_target_exclude_each_other() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/hello");
    for command in ["run", "test"] {
        let output = wolf(&dir, &[command, "--vm", "--target", "go"], "");
        assert_eq!(output.status.code(), Some(2), "{command}");
        assert_eq!(
            String::from_utf8_lossy(&output.stderr),
            "error[E0002]: `--vm` and `--target` exclude each other; pass one of them\n"
        );
    }
}

/// A library runs its examples and its tests in the VM (spec chapter 10, section 1, rule 6);
/// `wolf run --vm` alone is E0002, which `tests/vm/lib_project` checks.
#[test]
fn a_library_runs_examples_and_tests_in_the_vm() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/vm/lib_project");
    let output = wolf(&dir, &["run", "--vm", "--example", "show"], "");
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "42\n");
    let output = wolf(&dir, &["test", "--vm"], "");
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "test src/lib.wolf: doubles ... ok\n\ntest result: ok. 1 passed; 0 failed\n"
    );
}

/// `wolf check --bytecode` prints the bytecode of the functions of `src/`, in the order of the
/// source and closures after their function, with a comment for a module that cannot run in the
/// VM; `--bytecode <name>` only the named ones (spec chapter 10, section 1a). The whole listing
/// is `tests/bytecode/bytecode.txt`; run with `WOLF_BLESS=1` to write it from the output.
#[test]
fn check_bytecode_lists_the_functions() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/bytecode");
    let all = wolf(&dir, &["check", "--bytecode"], "");
    let text = String::from_utf8_lossy(&all.stdout).into_owned();
    assert!(
        all.status.success(),
        "{}",
        String::from_utf8_lossy(&all.stderr)
    );
    let file = dir.join("bytecode.txt");
    if std::env::var_os("WOLF_BLESS").is_some() {
        fs::write(&file, &text).unwrap();
    }
    assert_eq!(text, fs::read_to_string(&file).unwrap());
    let heads = |name: &str| -> Vec<String> {
        let out = wolf(&dir, &["check", "--bytecode", name], "");
        assert!(out.status.success(), "{name}");
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter(|l| l.starts_with("fn ") || l.starts_with("//"))
            .map(|l| l.split(" (").next().unwrap_or(l).to_string())
            .collect()
    };
    assert_eq!(
        heads("area"),
        ["fn shapes.area", "fn shapes.area closure 1"]
    );
    assert_eq!(
        heads("shapes.area"),
        ["fn shapes.area", "fn shapes.area closure 1"]
    );
    assert_eq!(heads("Shape.area"), ["fn shapes.Shape.area"]);
    assert_eq!(heads("shapes.Shape.area"), ["fn shapes.Shape.area"]);
    assert_eq!(heads("main"), ["fn main.main"]);
    assert_eq!(
        heads("loud"),
        ["// module loud cannot run in the VM: it imports text.shout"]
    );
    let unknown = wolf(&dir, &["check", "--bytecode", "shapes.perimeter"], "");
    assert_eq!(unknown.status.code(), Some(1));
    assert!(unknown.stdout.is_empty());
    assert!(String::from_utf8_lossy(&unknown.stderr).contains("error[E0002]"));
}
