//! End-to-end tests of the TypeScript backend: `wolf run` builds with `tsc --strict` and runs
//! with Node.js, and the output must match exactly. These tests need `node` and `tsc` on `PATH`.

// Test helpers may fail loudly.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn tmp(name: &str) -> PathBuf {
    Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("run")
        .join(name)
}

fn wolf_run(project: &Path, out: &Path, extra: &[&str], path: Option<&str>) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_wolf"));
    cmd.arg("run")
        .arg("--out")
        .arg(out)
        .args(extra)
        .current_dir(project);
    if let Some(path) = path {
        cmd.env("PATH", path);
    }
    cmd.output().unwrap()
}

fn tool_dir(name: &str) -> PathBuf {
    let path = std::env::var_os("PATH").unwrap_or_default();
    std::env::split_paths(&path)
        .find(|d| d.join(name).is_file())
        .unwrap_or_else(|| panic!("`{name}` must be installed to run these tests"))
}

/// Milestone M6: every example runs in Node.js and prints `expected.txt`.
#[test]
fn every_example_runs_and_prints_expected_output() {
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let mut projects: Vec<PathBuf> = fs::read_dir(&examples)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join("wolfpack.toml").is_file())
        .collect();
    projects.sort();
    assert_eq!(projects.len(), 10);
    let mut failures = Vec::new();
    for project in projects {
        let name = project.file_name().unwrap().to_string_lossy().into_owned();
        let extra: &[&str] = if name == "textkit" {
            &["--example", "demo"]
        } else {
            &[]
        };
        let output = wolf_run(&project, &tmp(&name), extra, None);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let expected = fs::read_to_string(project.join("expected.txt")).unwrap();
        if stdout != expected || output.status.code() != Some(0) {
            failures.push(format!(
                "--- {name}: exit {:?}\nstdout:\n{stdout}\nstderr:\n{}",
                output.status.code(),
                String::from_utf8_lossy(&output.stderr)
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Programs under `tests/run/` with their standard output, standard error and exit code.
#[test]
fn run_programs_match_expected_output() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/run");
    let bless = std::env::var_os("WOLF_BLESS").is_some();
    let mut failures = Vec::new();
    let mut dirs: Vec<PathBuf> = fs::read_dir(&root)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .collect();
    dirs.sort();
    for dir in dirs {
        let name = dir.file_name().unwrap().to_string_lossy().into_owned();
        if name.starts_with("E08") {
            continue;
        }
        let output = wolf_run(&dir, &tmp(&name), &[], None);
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        let code = output.status.code().unwrap_or(-1).to_string();
        if bless {
            fs::write(dir.join("expected.txt"), &stdout).unwrap();
            if stderr.is_empty() {
                let _ = fs::remove_file(dir.join("expected.stderr"));
            } else {
                fs::write(dir.join("expected.stderr"), &stderr).unwrap();
            }
            if code == "0" {
                let _ = fs::remove_file(dir.join("expected.code"));
            } else {
                fs::write(dir.join("expected.code"), &code).unwrap();
            }
            continue;
        }
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
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// E0801: a tool of the target is missing.
#[test]
fn e0801_missing_tool() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/run/E0801_missing_tool");
    let empty = tmp("empty_path");
    fs::create_dir_all(&empty).unwrap();
    let output = wolf_run(&dir, &tmp("E0801"), &[], Some(empty.to_str().unwrap()));
    let stderr = String::from_utf8_lossy(&output.stderr);
    let expected = fs::read_to_string(dir.join("expected.stderr")).unwrap();
    assert_eq!(stderr, expected);
    assert_eq!(output.status.code(), Some(1));
}

/// E0802: the target's tool rejects the generated code (simulated by a fake `tsc`).
#[test]
fn e0802_rejected_code() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/run/E0802_rejected_code");
    let fake = tmp("fake_tsc");
    fs::create_dir_all(&fake).unwrap();
    let tsc = fake.join("tsc");
    fs::write(
        &tsc,
        "#!/bin/sh\necho \"main.ts(1,1): error TS0000: simulated rejection\"\nexit 2\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&tsc, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let path = format!("{}:{}", fake.display(), tool_dir("node").display());
    let output = wolf_run(&dir, &tmp("E0802"), &[], Some(&path));
    let stderr = String::from_utf8_lossy(&output.stderr);
    let expected = fs::read_to_string(dir.join("expected.stderr")).unwrap();
    assert_eq!(stderr, expected);
    assert_eq!(output.status.code(), Some(1));
}
