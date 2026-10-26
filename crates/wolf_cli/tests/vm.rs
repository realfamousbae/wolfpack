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
/// as the nesting of calls from std back into the program, standard input and arguments
/// (`input.txt`, `args.txt`).
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

// ----- bytecode files (spec chapter 10, section 1b) --------------------------------------------

/// A new empty directory for the files of a test.
fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("bytecode")
        .join(name);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// Copies a directory with everything in it.
fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            copy_dir(&path, &to.join(entry.file_name()));
        } else {
            fs::copy(&path, to.join(entry.file_name())).unwrap();
        }
    }
}

/// `wolf build --vm` of a project (with these extra arguments) into `out`; the one file written.
fn build_file(project: &Path, extra: &[&str], out: &Path) -> PathBuf {
    let mut args = vec!["build", "--vm", "--out", out.to_str().unwrap()];
    args.extend(extra);
    let built = wolf(project, &args, "");
    assert_eq!(
        built.status.code(),
        Some(0),
        "{}: {}",
        project.display(),
        String::from_utf8_lossy(&built.stderr)
    );
    assert!(built.stdout.is_empty() && built.stderr.is_empty());
    let files: Vec<PathBuf> = fs::read_dir(out)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .collect();
    assert_eq!(files.len(), 1, "{files:?}");
    files.into_iter().next().unwrap()
}

/// The same output, error output and exit code.
fn same(a: &Output, b: &Output) -> bool {
    a.stdout == b.stdout && a.stderr == b.stderr && a.status.code() == b.status.code()
}

/// Every example built with `wolf build --vm` runs from its file as with `wolf run --vm`, from a
/// directory without a project; the examples that call code of the targets cannot be built
/// (E0807).
#[test]
fn examples_run_from_bytecode_files() {
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let elsewhere = scratch("examples");
    let mut failures = Vec::new();
    for project in dirs(&examples) {
        let name = name(&project);
        if matches!(name.as_str(), "fuzzy" | "textkit" | "native-slug") {
            let out = elsewhere.join(&name);
            let built = wolf(
                &project,
                &["build", "--vm", "--out", out.to_str().unwrap()],
                "",
            );
            let stderr = String::from_utf8_lossy(&built.stderr);
            if built.status.code() != Some(1) || !stderr.contains("error[E0807]") || out.exists() {
                failures.push(format!("--- {name}: built\n{stderr}"));
            }
            continue;
        }
        let file = build_file(&project, &[], &elsewhere.join(&name));
        let in_memory = wolf(&project, &["run", "--vm", "--", "x"], "");
        let from_file = wolf(&elsewhere, &["run", file.to_str().unwrap(), "--", "x"], "");
        if !same(&in_memory, &from_file) {
            failures.push(format!(
                "--- {name}\nwolf run --vm (exit {:?}):\n{}{}\nfrom the file (exit {:?}):\n{}{}",
                in_memory.status.code(),
                String::from_utf8_lossy(&in_memory.stdout),
                String::from_utf8_lossy(&in_memory.stderr),
                from_file.status.code(),
                String::from_utf8_lossy(&from_file.stdout),
                String::from_utf8_lossy(&from_file.stderr),
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The programs of `tests/run/` and `tests/vm/` give from their bytecode files what they give in
/// `wolf run --vm`: output, error output and exit code, with standard input and arguments, run
/// from a directory without a project.
#[test]
fn run_programs_match_expected_output_from_bytecode_files() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let elsewhere = scratch("run");
    let mut failures = Vec::new();
    let mut projects = dirs(&manifest.join("tests/run"));
    projects.extend(dirs(&manifest.join("tests/vm")));
    for dir in projects {
        let name = name(&dir);
        // Errors of the build and programs that cannot run in the VM have tests of their own.
        if name.starts_with("E0")
            || uses_extern(&name)
            || matches!(name.as_str(), "extern_go_only" | "lib_project")
        {
            continue;
        }
        let input = fs::read_to_string(dir.join("input.txt")).unwrap_or_default();
        let extra = fs::read_to_string(dir.join("args.txt")).unwrap_or_default();
        let file = build_file(&dir, &[], &elsewhere.join(&name));
        let mut args = vec!["run", file.to_str().unwrap(), "--"];
        args.extend(extra.split_whitespace());
        // Outside the project, whose `out.txt` the test of `wolf run --vm` writes at this time.
        let output = wolf(file.parent().unwrap(), &args, &input);
        failures.extend(compare(&dir, &output));
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `wolf build --vm` writes `out/vm/<package name>.wbc` and puts the overrides of
/// `[config.<module>]` into it: the file does not read `wolfpack.toml` again.
#[test]
fn a_bytecode_file_keeps_the_config_of_its_build() {
    let project = scratch("config");
    copy_dir(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/run/names_and_config"),
        &project,
    );
    let built = wolf(&project, &["build", "--vm"], "");
    assert_eq!(built.status.code(), Some(0));
    assert!(project.join("out/vm/names-and-config.wbc").is_file());
    let toml = fs::read_to_string(project.join("wolfpack.toml")).unwrap();
    fs::write(
        project.join("wolfpack.toml"),
        toml.replace("hi from wolfpack.toml", "changed after the build"),
    )
    .unwrap();
    let ran = wolf(&project, &["run", "out/vm/names-and-config.wbc"], "");
    assert_eq!(ran.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&ran.stdout),
        "HI FROM WOLFPACK.TOML!\ninner\nhi from wolfpack.toml\n"
    );
    let in_memory = wolf(&project, &["run", "--vm"], "");
    assert!(String::from_utf8_lossy(&in_memory.stdout).contains("changed after the build"));
}

/// The error output of `wolf run <file>` that is E0809, after checking that it ran nothing.
fn e0809(file: &Path) -> String {
    let dir = file.parent().unwrap();
    let output = wolf(dir, &["run", file.to_str().unwrap()], "");
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// A file of another version of `wolf`, a file that is not a bytecode file and a damaged file
/// run nothing (E0809); a file that does not exist is E0001.
#[test]
fn a_bytecode_file_of_another_version_is_e0809() {
    let hello = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/hello");
    let dir = scratch("e0809");
    let file = build_file(&hello, &[], &dir.join("built"));
    let bytes = fs::read(&file).unwrap();
    let version = env!("CARGO_PKG_VERSION");
    let header = format!("wolfpack bytecode {version}\n");
    assert!(bytes.starts_with(header.as_bytes()));

    let old = dir.join("old.wbc");
    let mut other = b"wolfpack bytecode 0.0.1\n".to_vec();
    other.extend(&bytes[header.len()..]);
    fs::write(&old, other).unwrap();
    assert_eq!(
        e0809(&old),
        format!(
            "error[E0809]: `{}` was built by wolf 0.0.1, and this is wolf {version}\n \
             = help: build it again with this `wolf`: `wolf build --vm`\n \
             = note: a bytecode file runs only with the version of `wolf` that built it\n\n\
             error: aborting due to 1 error\n",
            old.to_str().unwrap().replace('\\', "/")
        )
    );

    let text = dir.join("main.wolf");
    fs::copy(hello.join("src/main.wolf"), &text).unwrap();
    let stderr = e0809(&text);
    assert!(stderr.contains("` is not a bytecode file\n"), "{stderr}");
    assert!(
        stderr.contains(&format!("this is wolf {version}")),
        "{stderr}"
    );

    let cut = dir.join("cut.wbc");
    fs::write(&cut, &bytes[..bytes.len() / 2]).unwrap();
    let stderr = e0809(&cut);
    assert!(stderr.contains("` is damaged: "), "{stderr}");
    assert!(
        stderr.contains("build it again with `wolf build --vm`"),
        "{stderr}"
    );

    let changed = dir.join("changed.wbc");
    let mut damaged = bytes.clone();
    let last = damaged.len() - 1;
    damaged[last] ^= 1;
    fs::write(&changed, damaged).unwrap();
    assert!(e0809(&changed).contains("` is damaged: "));

    let missing = wolf(&dir, &["run", "missing.wbc"], "");
    assert_eq!(missing.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&missing.stderr)
            .starts_with("error[E0001]: cannot read `missing.wbc`")
    );
}

/// A library has no bytecode file of its own, but its examples do; a program that calls code of
/// the targets cannot be built for the VM (E0807).
#[test]
fn wolf_build_vm_refuses_a_library_and_extern() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let lib = manifest.join("tests/vm/lib_project");
    let dir = scratch("refused");
    let out = dir.join("lib");
    let built = wolf(&lib, &["build", "--vm", "--out", out.to_str().unwrap()], "");
    assert_eq!(built.status.code(), Some(2));
    assert_eq!(
        String::from_utf8_lossy(&built.stderr),
        "error[E0002]: a library cannot be built into a bytecode file\n \
         = help: build one of its examples: `wolf build --vm --example <name>`\n\n\
         error: aborting due to 1 error\n"
    );
    assert!(!out.exists());
    let file = build_file(&lib, &["--example", "show"], &out);
    assert_eq!(file.file_name().unwrap(), "show.wbc");
    let ran = wolf(&dir, &["run", file.to_str().unwrap()], "");
    assert_eq!(ran.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&ran.stdout), "42\n");
    let unknown = wolf(&lib, &["build", "--vm", "--example", "nope"], "");
    assert_eq!(unknown.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&unknown.stderr).contains("there is no example `nope`"));

    let out = dir.join("extern");
    let ext = manifest.join("tests/run/E0807_vm_extern");
    let built = wolf(&ext, &["build", "--vm", "--out", out.to_str().unwrap()], "");
    let stderr = String::from_utf8_lossy(&built.stderr);
    assert_eq!(built.status.code(), Some(1));
    assert!(stderr.starts_with("error[E0807]: module "), "{stderr}");
    assert!(
        stderr.contains("= help: build it for a target: `wolf build --target "),
        "{stderr}"
    );
    assert!(!out.exists());
}

/// A bytecode file runs as it was built: `--vm`, `--target` and `--example` do not apply to it,
/// and `wolf build --example` needs `--vm` (E0002).
#[test]
fn bytecode_files_take_no_target_options() {
    let dir = scratch("usage");
    for (flag, args) in [
        ("--vm", vec!["run", "--vm", "app.wbc"]),
        ("--target", vec!["run", "--target", "go", "app.wbc"]),
        ("--example", vec!["run", "--example", "demo", "app.wbc"]),
    ] {
        let output = wolf(&dir, &args, "");
        assert_eq!(output.status.code(), Some(2), "{flag}");
        assert_eq!(
            String::from_utf8_lossy(&output.stderr),
            format!(
                "error[E0002]: `{flag}` does not apply to a bytecode file, which runs in the VM \
                 as it was built\n"
            )
        );
    }
    let hello = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/hello");
    let output = wolf(&hello, &["build", "--example", "demo"], "");
    assert_eq!(output.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .starts_with("error[E0002]: `--example` works only with `--vm`")
    );
    let output = wolf(&hello, &["build", "--vm", "--target", "go"], "");
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "error[E0002]: `--vm` and `--target` exclude each other; pass one of them\n"
    );
}
