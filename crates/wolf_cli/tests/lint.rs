//! The lint matrix: every example and every project of `tests/run/`, `tests/test/`,
//! `tests/native/` and `tests/native_modules/`, built for every target in every mode (the normal
//! build, `--debug`, the test build of `wolf test` and the test build with `--debug`), gives code
//! that the tools of the target accept without a remark: `tsc --strict` for TypeScript, `gofmt`
//! and `go vet` for Go, `rustfmt` and `cargo clippy` for Rust. The other end-to-end tests check
//! the code of the modes they run; this file checks every mode, examples included. These tests need
//! `tsc`, `go` and `cargo` with `rustfmt` and `clippy` on `PATH`.

// Test helpers may fail loudly.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn tmp(name: &str) -> PathBuf {
    Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("lint")
        .join(name)
}

/// The projects to build for `target`: those that can be built for it without an error.
fn projects(target: &str) -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut projects = Vec::new();
    for dir in [
        "../../examples",
        "tests/run",
        "tests/test",
        "tests/native",
        "tests/native_modules",
    ] {
        let mut found: Vec<PathBuf> = fs::read_dir(root.join(dir))
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.join("wolfpack.toml").is_file())
            .collect();
        found.sort();
        projects.extend(found);
    }
    projects.retain(|p| {
        let name = p.file_name().unwrap().to_string_lossy();
        // Programs that show an error of the target or of the driver are rejected on purpose.
        let rejected = ["E07", "E08", "default_target"]
            .iter()
            .any(|prefix| name.starts_with(prefix));
        // Programs of `tests/native/` use what only Go and Rust support.
        let unsupported = target == "ts" && p.parent().unwrap().ends_with("tests/native");
        // A project with native code for some targets builds only for those.
        let native = p.join("native");
        let no_native = native.is_dir() && !native.join(target).is_dir();
        !rejected && !unsupported && !no_native
    });
    projects
}

/// The modes of a build: the command of `wolf` and its flags.
const MODES: [(&str, &[&str]); 4] = [
    ("build", &[]),
    ("build", &["--debug"]),
    ("test", &[]),
    ("test", &["--debug"]),
];

/// Builds `project` for `target` in every mode and returns the remarks of `check` on the code.
fn lint(target: &str, check: impl Fn(&Path) -> Option<String>) {
    let mut failures = Vec::new();
    for project in projects(target) {
        let name = project.file_name().unwrap().to_string_lossy().into_owned();
        // A project with dependencies gets them fetched by `wolf run`, not by `wolf build`.
        let dependencies = fs::read_to_string(project.join("wolfpack.toml"))
            .unwrap()
            .contains(".dependencies]");
        for (command, flags) in MODES {
            let command = if command == "build" && dependencies {
                "run"
            } else {
                command
            };
            let mode = format!("{command}{}", flags.join(""));
            let out = tmp(&format!("{target}-{name}-{mode}"));
            let _ = fs::remove_dir_all(&out);
            // The exit code does not matter here: some programs and tests fail on purpose.
            let output = Command::new(env!("CARGO_BIN_EXE_wolf"))
                .arg(command)
                .args(["--target", target, "--out"])
                .arg(&out)
                .args(flags)
                .current_dir(&project)
                .output()
                .unwrap();
            if !out.is_dir() {
                failures.push(format!(
                    "--- {name} ({mode}): not built\n{}",
                    String::from_utf8_lossy(&output.stderr)
                ));
                continue;
            }
            if let Some(problems) = check(&out) {
                failures.push(format!("--- {name} ({mode}):\n{problems}"));
            }
            let _ = fs::remove_dir_all(&out);
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The generated TypeScript passes `tsc --strict` (the `tsconfig.json` of the project).
#[test]
fn typescript_of_every_mode_passes_tsc() {
    lint("ts", |out| {
        let tsc = Command::new("tsc")
            .args(["--noEmit", "-p", "."])
            .current_dir(out)
            .output()
            .unwrap();
        (!tsc.status.success()).then(|| String::from_utf8_lossy(&tsc.stdout).into_owned())
    });
}

/// The generated Go is formatted as `gofmt` formats it and `go vet` has nothing to report.
#[test]
fn go_of_every_mode_passes_gofmt_and_vet() {
    lint("go", |out| {
        let fmt = Command::new("gofmt")
            .args(["-l", "."])
            .current_dir(out)
            .output()
            .unwrap();
        let vet = Command::new("go")
            .args(["vet", "./..."])
            .current_dir(out)
            .output()
            .unwrap();
        let mut problems = String::new();
        let unformatted = String::from_utf8_lossy(&fmt.stdout);
        if !unformatted.trim().is_empty() {
            problems.push_str(&format!("not formatted by gofmt: {unformatted}"));
        }
        if !vet.status.success() {
            problems.push_str(&String::from_utf8_lossy(&vet.stderr));
        }
        (!problems.is_empty()).then_some(problems)
    });
}

/// The generated Rust is formatted as `rustfmt` formats it and `cargo clippy` has nothing to
/// report. The projects share one target directory, so the runtime is checked once.
#[test]
fn rust_of_every_mode_passes_rustfmt_and_clippy() {
    let target_dir = tmp("rust-target");
    lint("rust", |out| {
        let cargo = |args: &[&str]| {
            Command::new("cargo")
                .args(args)
                .env("CARGO_TARGET_DIR", &target_dir)
                .env_remove("RUSTFLAGS")
                .current_dir(out)
                .output()
                .unwrap()
        };
        let fmt = cargo(&["fmt", "--check"]);
        let clippy = cargo(&["clippy", "--quiet", "--all-targets", "--", "-D", "warnings"]);
        let mut problems = String::new();
        if !fmt.status.success() {
            problems.push_str(&format!(
                "not formatted by rustfmt:\n{}",
                String::from_utf8_lossy(&fmt.stdout)
            ));
        }
        if !clippy.status.success() || !clippy.stderr.is_empty() {
            problems.push_str(&String::from_utf8_lossy(&clippy.stderr));
        }
        (!problems.is_empty()).then_some(problems)
    });
}
