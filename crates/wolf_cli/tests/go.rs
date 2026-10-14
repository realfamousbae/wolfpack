//! End-to-end tests of the Go backend (milestone M9): every example and every program of
//! `tests/run/` gives on Go the output it gives on TypeScript, the generated code is formatted as
//! `gofmt` formats it and `go vet` has nothing to report. These tests need `go` on `PATH`.

// Test helpers may fail loudly.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn tmp(name: &str) -> PathBuf {
    Path::new(env!("CARGO_TARGET_TMPDIR")).join("go").join(name)
}

fn wolf(command: &str, project: &Path, out: &Path, extra: &[&str], path: Option<&str>) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_wolf"));
    cmd.arg(command)
        .arg("--target")
        .arg("go")
        .arg("--out")
        .arg(out)
        .args(extra)
        .current_dir(project);
    if let Some(path) = path {
        cmd.env("PATH", path);
    }
    cmd.output().unwrap()
}

/// Problems that `gofmt -l` and `go vet` report for a generated project.
fn go_checks(out: &Path) -> Option<String> {
    let fmt = Command::new("gofmt")
        .arg("-l")
        .arg(".")
        .current_dir(out)
        .output()
        .unwrap();
    let unformatted = String::from_utf8_lossy(&fmt.stdout).into_owned();
    let vet = Command::new("go")
        .args(["vet", "./..."])
        .env("GOTOOLCHAIN", "local")
        .env("GOWORK", "off")
        .current_dir(out)
        .output()
        .unwrap();
    let mut problems = String::new();
    if !unformatted.is_empty() {
        problems.push_str(&format!("not formatted by gofmt: {unformatted}"));
    }
    if !vet.status.success() {
        problems.push_str(&String::from_utf8_lossy(&vet.stderr));
    }
    (!problems.is_empty()).then_some(problems)
}

fn examples() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let mut projects: Vec<PathBuf> = fs::read_dir(&root)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join("wolfpack.toml").is_file())
        .collect();
    projects.sort();
    projects
}

/// Milestone M9: every example runs on Go and prints `expected.txt`; the code passes `gofmt` and
/// `go vet`.
#[test]
fn every_example_runs_on_go() {
    let projects = examples();
    assert_eq!(projects.len(), 10);
    let mut failures = Vec::new();
    for project in projects {
        let name = project.file_name().unwrap().to_string_lossy().into_owned();
        let extra: &[&str] = if name == "textkit" {
            &["--example", "demo"]
        } else {
            &[]
        };
        let out = tmp(&name);
        let output = wolf("run", &project, &out, extra, None);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let expected = fs::read_to_string(project.join("expected.txt")).unwrap();
        if stdout != expected || output.status.code() != Some(0) {
            failures.push(format!(
                "--- {name}: exit {:?}\nstdout:\n{stdout}\nstderr:\n{}",
                output.status.code(),
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        if let Some(problems) = go_checks(&out) {
            failures.push(format!("--- {name}:\n{problems}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The programs of `tests/run/` give on Go the standard output, standard error and exit code
/// they give on TypeScript.
#[test]
fn run_programs_match_expected_output_on_go() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/run");
    let mut failures = Vec::new();
    let mut dirs: Vec<PathBuf> = fs::read_dir(&root)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .collect();
    dirs.sort();
    for dir in dirs {
        let name = dir.file_name().unwrap().to_string_lossy().into_owned();
        if name.starts_with("E07") || name.starts_with("E08") || name.starts_with("default_target")
        {
            continue;
        }
        let out = tmp(&format!("run-{name}"));
        let output = wolf("run", &dir, &out, &[], None);
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        let code = output.status.code().unwrap_or(-1).to_string();
        let read = |f: &str, default: &str| {
            fs::read_to_string(dir.join(f)).unwrap_or_else(|_| default.to_string())
        };
        let (e_out, e_err, e_code) = (
            read("expected.txt", ""),
            read("expected.stderr", ""),
            read("expected.code", "0"),
        );
        if stdout != e_out || stderr != e_err || code != e_code.trim() {
            failures.push(format!(
                "--- {name}\nexpected (exit {e_code}):\n{e_out}{e_err}\nactual (exit {code}):\n{stdout}{stderr}"
            ));
        }
        if let Some(problems) = go_checks(&out) {
            failures.push(format!("--- {name}:\n{problems}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The tests of every example pass on Go.
#[test]
fn every_example_passes_its_tests_on_go() {
    let mut failures = Vec::new();
    for project in examples() {
        let name = project.file_name().unwrap().to_string_lossy().into_owned();
        let output = wolf("test", &project, &tmp(&format!("test-{name}")), &[], None);
        let stdout = String::from_utf8_lossy(&output.stdout);
        if output.status.code() != Some(0) || !stdout.contains("test result: ok.") {
            failures.push(format!(
                "--- {name}\n{stdout}{}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The report of `wolf test` on Go is the report on TypeScript (spec chapter 8, rule 2.7).
#[test]
fn test_reports_on_go_match_typescript() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/test");
    let mut failures = Vec::new();
    for entry in fs::read_dir(&root).unwrap().flatten() {
        let dir = entry.path();
        let name = dir.file_name().unwrap().to_string_lossy().into_owned();
        let mut runs = vec![(None, "expected.txt")];
        if let Ok(filter) = fs::read_to_string(dir.join("filter.txt")) {
            runs.push((Some(filter.trim().to_string()), "expected_filtered.txt"));
        }
        for (filter, file) in runs {
            let extra: Vec<&str> = filter.iter().map(String::as_str).collect();
            let out = tmp(&format!("t-{name}"));
            let output = wolf("test", &dir, &out, &extra, None);
            let actual = format!(
                "{}exit code: {}\n",
                String::from_utf8_lossy(&output.stdout),
                output.status.code().unwrap_or(-1)
            );
            let expected = fs::read_to_string(dir.join(file)).unwrap_or_default();
            if actual != expected {
                failures.push(format!(
                    "--- {name} ({file})\nexpected:\n{expected}\nactual:\n{actual}{}",
                    String::from_utf8_lossy(&output.stderr)
                ));
            }
            if let Some(problems) = go_checks(&out) {
                failures.push(format!("--- {name}:\n{problems}"));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `wolf test <text>` selects the same tests on both targets: the text may be part of the file
/// or of the test name.
#[test]
fn test_filters_select_the_same_tests_on_both_targets() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/test/mixed");
    for filter in ["util", "halve", "nothing matches this"] {
        let go = wolf("test", &dir, &tmp("filter-go"), &[filter], None);
        let ts = Command::new(env!("CARGO_BIN_EXE_wolf"))
            .args(["test", filter, "--out"])
            .arg(tmp("filter-ts"))
            .current_dir(&dir)
            .output()
            .unwrap();
        assert_eq!(
            String::from_utf8_lossy(&go.stdout),
            String::from_utf8_lossy(&ts.stdout),
            "filter {filter}"
        );
        assert_eq!(go.status.code(), ts.status.code(), "filter {filter}");
    }
}

/// Tests and test helpers are not part of `wolf build --target go`.
#[test]
fn tests_are_not_in_the_normal_go_build() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/test/mixed");
    let out = tmp("build-mixed");
    let output = wolf("build", &dir, &out, &[], None);
    assert_eq!(output.status.code(), Some(0));
    let math = fs::read_to_string(out.join("util/math/math.go")).unwrap();
    assert!(!math.contains("twice"), "{math}");
    assert!(!out.join("main_test.go").exists());
    assert!(!out.join("util/math/math_test.go").exists());
}

/// E0801: `go` is missing.
#[test]
fn e0801_missing_go() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/run/E0801_missing_tool");
    let empty = tmp("empty_path");
    fs::create_dir_all(&empty).unwrap();
    let output = wolf(
        "run",
        &dir,
        &tmp("E0801"),
        &[],
        Some(empty.to_str().unwrap()),
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    let expected = fs::read_to_string(dir.join("expected_go.stderr")).unwrap();
    assert_eq!(stderr, expected);
    assert_eq!(output.status.code(), Some(1));
}

/// E0802: `go` rejects the generated code (simulated by a fake `go`).
#[test]
fn e0802_rejected_by_go() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/run/E0802_rejected_code");
    let fake = tmp("fake_go");
    fs::create_dir_all(&fake).unwrap();
    let go = fake.join("go");
    fs::write(
        &go,
        "#!/bin/sh\necho \"./main.go:1:1: simulated rejection\" >&2\nexit 1\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&go, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let path = fake.display().to_string();
    let output = wolf("run", &dir, &tmp("E0802"), &[], Some(&path));
    let stderr = String::from_utf8_lossy(&output.stderr);
    let expected = fs::read_to_string(dir.join("expected_go.stderr")).unwrap();
    assert_eq!(stderr, expected);
    assert_eq!(output.status.code(), Some(1));
}

/// `--out` is relative to the current directory, although the tools run elsewhere.
#[test]
fn relative_out_directory_on_go() {
    let hello = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/hello");
    let project = tmp("relative");
    let _ = fs::remove_dir_all(&project);
    fs::create_dir_all(project.join("src")).unwrap();
    fs::copy(hello.join("wolfpack.toml"), project.join("wolfpack.toml")).unwrap();
    fs::copy(hello.join("src/main.wolf"), project.join("src/main.wolf")).unwrap();
    let output = wolf("run", &project, Path::new("build/out"), &[], None);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "Hello, Wolfpack!\n",
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(project.join("build/out").is_dir());
}

/// Programs that only Go and Rust support yet give their expected output.
#[test]
fn native_programs_on_go() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/native");
    for entry in fs::read_dir(&root).unwrap().flatten() {
        let dir = entry.path();
        let name = dir.file_name().unwrap().to_string_lossy().into_owned();
        let out = tmp(&format!("native-{name}"));
        let output = wolf("run", &dir, &out, &[], None);
        let expected = fs::read_to_string(dir.join("expected.txt")).unwrap();
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            expected,
            "{name}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(go_checks(&out), None, "{name}");
    }
}
