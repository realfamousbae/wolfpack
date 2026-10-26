//! End-to-end tests of the Rust backend (milestone M10): every example and every program of
//! `tests/run/` gives on Rust the output it gives on TypeScript, the generated code is formatted as
//! `rustfmt` formats it and `cargo clippy` has nothing to report. These tests need `cargo` with
//! `rustfmt` and `clippy` on `PATH`.

// Test helpers may fail loudly.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn tmp(name: &str) -> PathBuf {
    Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("rust")
        .join(name)
}

fn wolf(command: &str, project: &Path, out: &Path, extra: &[&str], path: Option<&str>) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_wolf"));
    cmd.arg(command)
        .arg("--target")
        .arg("rust")
        .arg("--out")
        .arg(out)
        .args(extra)
        .current_dir(project);
    if let Some(path) = path {
        // Only this PATH: no tools installed by `wolf setup` either.
        cmd.env("PATH", path).env(
            "WOLFPACK_HOME",
            Path::new(env!("CARGO_TARGET_TMPDIR")).join("no-wolfpack-home"),
        );
    }
    cmd.output().unwrap()
}

/// Problems that `cargo fmt --check` and `cargo clippy` report for a generated project.
fn rust_checks(out: &Path) -> Option<String> {
    let cargo = |args: &[&str]| {
        Command::new("cargo")
            .args(args)
            .env("CARGO_TARGET_DIR", out.join("target"))
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
}

/// `--debug` for a project with a `debug.txt`: a debug build (spec chapter 7, section 3, rule 1).
fn debug_flag(dir: &Path) -> &'static [&'static str] {
    if dir.join("debug.txt").is_file() {
        &["--debug"]
    } else {
        &[]
    }
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

/// Milestone M10: every example runs on Rust and prints `expected.txt`; the code passes
/// `rustfmt` and `cargo clippy`.
#[test]
fn every_example_runs_on_rust() {
    let projects = examples();
    assert_eq!(projects.len(), 13);
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
        if let Some(problems) = rust_checks(&out) {
            failures.push(format!("--- {name}:\n{problems}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The programs of `tests/run/` give on Rust the standard output, standard error and exit code
/// they give on TypeScript.
#[test]
fn run_programs_match_expected_output_on_rust() {
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
        let output = wolf("run", &dir, &out, debug_flag(&dir), None);
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
        if let Some(problems) = rust_checks(&out) {
            failures.push(format!("--- {name}:\n{problems}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The tests of every example pass on Rust.
#[test]
fn every_example_passes_its_tests_on_rust() {
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

/// The report of `wolf test` on Rust is the report on TypeScript (spec chapter 8, rule 2.7).
#[test]
fn test_reports_on_rust_match_typescript() {
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
            let mut extra: Vec<&str> = filter.iter().map(String::as_str).collect();
            extra.extend(debug_flag(&dir));
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
            if let Some(problems) = rust_checks(&out) {
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
        let rs = wolf("test", &dir, &tmp("filter-rs"), &[filter], None);
        let ts = Command::new(env!("CARGO_BIN_EXE_wolf"))
            .args(["test", filter, "--out"])
            .arg(tmp("filter-ts"))
            .current_dir(&dir)
            .output()
            .unwrap();
        assert_eq!(
            String::from_utf8_lossy(&rs.stdout),
            String::from_utf8_lossy(&ts.stdout),
            "filter {filter}"
        );
        assert_eq!(rs.status.code(), ts.status.code(), "filter {filter}");
    }
}

/// Tests and test helpers are not part of `wolf build --target rust`.
#[test]
fn tests_are_not_in_the_normal_rust_build() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/test/mixed");
    let out = tmp("build-mixed");
    let _ = fs::remove_dir_all(&out);
    let output = wolf("build", &dir, &out, &[], None);
    assert_eq!(output.status.code(), Some(0));
    let mut sources = Vec::new();
    collect_sources(&out, &mut sources);
    assert!(!sources.is_empty());
    for (path, text) in sources {
        assert!(!text.contains("twice"), "{path}:\n{text}");
        assert!(!text.contains("#[test]"), "{path}:\n{text}");
    }
    assert!(!out.join("tests").exists());
}

/// The `.rs` files and `Cargo.toml` of a generated project, without the build directory.
fn collect_sources(dir: &Path, sources: &mut Vec<(String, String)>) {
    let mut entries: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n != "target") {
                collect_sources(&path, sources);
            }
        } else if path.extension().is_some_and(|e| e == "rs" || e == "toml") {
            sources.push((
                path.display().to_string(),
                fs::read_to_string(&path).unwrap(),
            ));
        }
    }
}

/// Two builds of the same project give the same files, byte for byte.
#[test]
fn rust_output_is_deterministic() {
    for project in examples() {
        let name = project.file_name().unwrap().to_string_lossy().into_owned();
        let mut builds = Vec::new();
        for run in ["a", "b"] {
            let out = tmp(&format!("det-{name}-{run}"));
            let _ = fs::remove_dir_all(&out);
            let output = wolf("build", &project, &out, &[], None);
            assert_eq!(output.status.code(), Some(0), "{name}");
            let mut sources = Vec::new();
            collect_sources(&out, &mut sources);
            let relative: Vec<(String, String)> = sources
                .into_iter()
                .map(|(p, t)| (p.replace(&out.display().to_string(), ""), t))
                .collect();
            builds.push(relative);
        }
        assert!(builds[0] == builds[1], "{name}: builds differ");
    }
}

/// E0801: `cargo` is missing.
#[test]
fn e0801_missing_cargo() {
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
    let expected = fs::read_to_string(dir.join("expected_rust.stderr")).unwrap();
    assert_eq!(stderr, expected);
    assert_eq!(output.status.code(), Some(1));
}

/// E0806: `cargo` rejects a project with code in `native/rust/`.
#[test]
fn e0806_rejected_native_code_by_cargo() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/run/E0806_rejected_native");
    let output = wolf("run", &dir, &tmp("E0806"), &[], None);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.starts_with("error[E0806]: Rust rejected the code of `native/rust/`"),
        "{stderr}"
    );
    assert_eq!(output.status.code(), Some(1));
}

/// E0802: `cargo` rejects the generated code (simulated by a fake `cargo`).
#[test]
fn e0802_rejected_by_cargo() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/run/E0802_rejected_code");
    let fake = tmp("fake_cargo");
    fs::create_dir_all(&fake).unwrap();
    let cargo = fake.join("cargo");
    fs::write(
        &cargo,
        "#!/bin/sh\necho \"error[E0425]: simulated rejection\" >&2\nexit 101\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&cargo, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let path = fake.display().to_string();
    let output = wolf("run", &dir, &tmp("E0802"), &[], Some(&path));
    let stderr = String::from_utf8_lossy(&output.stderr);
    let expected = fs::read_to_string(dir.join("expected_rust.stderr")).unwrap();
    assert_eq!(stderr, expected);
    assert_eq!(output.status.code(), Some(1));
}

/// `--out` is relative to the current directory, although the tools run elsewhere.
#[test]
fn relative_out_directory_on_rust() {
    let hello = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/hello");
    let project = tmp("relative");
    let _ = fs::remove_dir_all(&project);
    fs::create_dir_all(project.join("src")).unwrap();
    fs::copy(hello.join("wolfpack.toml"), project.join("wolfpack.toml")).unwrap();
    fs::copy(hello.join("src/main.wolf"), project.join("src/main.wolf")).unwrap();
    let output = wolf("run", &project, Path::new("build/out"), &[], None);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "Hello, wolfpack!\n",
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(project.join("build/out").is_dir());
}

/// Native modules read from Rust code (spec chapter 7, section 8.7) give their expected output, and
/// the generated code passes rustfmt and clippy.
#[test]
fn native_modules_on_rust() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/native_modules");
    for entry in fs::read_dir(&root).unwrap().flatten() {
        let dir = entry.path();
        if !dir.join("native/rust").is_dir() {
            continue;
        }
        let name = dir.file_name().unwrap().to_string_lossy().into_owned();
        let out = tmp(&format!("native-module-{name}"));
        let output = wolf("run", &dir, &out, &[], None);
        let expected = fs::read_to_string(dir.join("expected.txt")).unwrap();
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            expected,
            "{name}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(rust_checks(&out), None, "{name}");
    }
}

/// Programs that only Go and Rust support yet give their expected output.
#[test]
fn native_programs_on_rust() {
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
        assert_eq!(rust_checks(&out), None, "{name}");
    }
}
