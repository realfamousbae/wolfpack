//! `wolf build`, `wolf run` and `wolf test`: lowering to IR, code generation and the tools of
//! the target.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use wolf_diag::{Diagnostic, codes};
use wolf_project::Target;

use crate::check::{self, CheckResult};
use crate::{CheckArgs, Color, MessageFormat};

/// What to do after the code is generated.
pub(crate) enum Then {
    /// Only write `out/<target>/`.
    Nothing,
    /// Build with the target's tools and run the program (or an example).
    Run {
        /// `--example <name>`
        example: Option<String>,
        /// Arguments for the program.
        args: Vec<String>,
    },
    /// Build with the tests and run them.
    Test {
        /// Only tests whose name contains this text.
        filter: Option<String>,
    },
}

/// The outcome of `wolf build` or `wolf run`.
pub(crate) enum Outcome {
    /// Errors to report; the exit code of `wolf`.
    Failed(Box<CheckResult>, u8),
    /// The program ran; its exit code.
    Ran(i32),
    /// The project was generated.
    Built,
}

/// The targets of a command.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Targets {
    /// The default target of `wolfpack.toml`.
    Default,
    /// `--target <name>`
    One(Target),
    /// `--target all`: every enabled target (`wolf build` only).
    All,
}

/// Checks, lowers and generates the project around `start`; with `Then::Run`, also runs it.
pub(crate) fn build(start: &Path, out_dir: Option<&Path>, then: Then, targets: Targets) -> Outcome {
    let args = CheckArgs {
        paths: Vec::new(),
        message_format: MessageFormat::Human,
        color: Color::Never,
        emit: None,
    };
    let mut result = check::load(start, &args);
    if !result.diagnostics.is_empty() {
        return Outcome::Failed(Box::new(result), crate::EXIT_ERRORS);
    }
    let Some(project) = result.project.take() else {
        return Outcome::Failed(Box::new(result), crate::EXIT_ERRORS);
    };
    let target = match targets {
        Targets::One(t) => t,
        _ => project.manifest.default_target,
    };
    let inputs = project.inputs();
    let meta = wolf_ir::ProjectMeta {
        name: &project.manifest.name,
        version: &project.manifest.version,
        kind: project.manifest.kind,
    };
    let program = match wolf_ir::lower(
        &inputs,
        &project.resolved,
        &project.typed,
        &result.map,
        &meta,
    ) {
        Ok(p) => p,
        Err(e) => return internal(result, target, &e.message),
    };
    if let Then::Run { example: None, .. } = &then {
        if program.kind == wolf_ir::ProgramKind::Lib {
            result.diagnostics.push(
                Diagnostic::new(codes::E0002, "a library cannot be run")
                    .help("run one of its examples: `wolf run --example <name>`"),
            );
            return Outcome::Failed(Box::new(result), crate::EXIT_USAGE);
        }
    }
    if let Then::Run {
        example: Some(name),
        ..
    } = &then
    {
        let exists = program.modules.iter().any(|m| {
            m.role == wolf_ir::ModuleRole::Example && m.path.get(1..) == Some(&[name.clone()][..])
        });
        if !exists {
            result.diagnostics.push(Diagnostic::new(
                codes::E0002,
                format!("there is no example `{name}` in `examples/`"),
            ));
            return Outcome::Failed(Box::new(result), crate::EXIT_USAGE);
        }
    }
    if targets == Targets::All {
        for t in project.manifest.enabled.clone() {
            let out = match out_dir {
                Some(d) => d.join(t.name()),
                None => project.root.join("out").join(t.name()),
            };
            result = match generate_for(result, t, &program, &out, Then::Nothing) {
                Outcome::Failed(r, code) => return Outcome::Failed(r, code),
                _ => CheckResult {
                    map: wolf_span::SourceMap::new(),
                    diagnostics: Vec::new(),
                    project: None,
                },
            };
        }
        return Outcome::Built;
    }
    let out = out_dir
        .map(Path::to_path_buf)
        .unwrap_or_else(|| project.root.join("out").join(target.name()));
    generate_for(result, target, &program, &out, then)
}

fn generate_for(
    result: CheckResult,
    target: Target,
    program: &wolf_ir::Program,
    out: &Path,
    then: Then,
) -> Outcome {
    match target {
        Target::Go => go(result, program, out, then),
        Target::Rust => rust(result, program, out, then),
        Target::Ts => ts(result, program, out, then),
    }
}

/// The name of a target's backend in messages.
fn backend(target: Target) -> &'static str {
    match target {
        Target::Ts => "TypeScript",
        Target::Go => "Go",
        Target::Rust => "Rust",
    }
}

fn internal(mut result: CheckResult, target: Target, message: &str) -> Outcome {
    result.diagnostics.push(
        Diagnostic::new(codes::E0003, "internal compiler error")
            .note(format!(
                "the {} backend does not support this yet: {message}",
                backend(target)
            ))
            .help("this is a bug or a gap in wolf; please report it with the input that caused it"),
    );
    Outcome::Failed(Box::new(result), crate::EXIT_INTERNAL)
}

fn write_failed(mut result: CheckResult, out: &Path, e: &std::io::Error) -> Outcome {
    result.diagnostics.push(Diagnostic::new(
        codes::E0001,
        format!("cannot write `{}`: {e}", out.display()),
    ));
    Outcome::Failed(Box::new(result), crate::EXIT_ERRORS)
}

/// Writes generated files under `out`, after removing the listed generated directories.
fn write(out: &Path, clean: &[&str], files: &[(String, &str)]) -> std::io::Result<()> {
    for dir in clean {
        let path = out.join(dir);
        if path.is_dir() {
            fs::remove_dir_all(&path)?;
        } else if path.is_file() {
            fs::remove_file(&path)?;
        }
    }
    for (rel, contents) in files {
        let path = out.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, contents)?;
    }
    Ok(())
}

/// A program on `PATH`, or in `node_modules/.bin` of the output project.
fn find_tool(name: &str, out: &Path) -> Option<PathBuf> {
    let local = out.join("node_modules").join(".bin").join(name);
    if local.is_file() {
        return Some(local);
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

fn missing_tool(mut result: CheckResult, target: Target, tool: &str, help: &str) -> Outcome {
    let note = match target {
        Target::Go => "`wolf run --target go` builds and runs with Go",
        Target::Rust => "`wolf run --target rust` builds and runs with Cargo",
        _ => "`wolf run --target ts` builds with TypeScript and runs with Node.js",
    };
    result.diagnostics.push(
        Diagnostic::new(codes::E0801, format!("`{tool}` is not installed"))
            .note(note)
            .help(help.to_string()),
    );
    Outcome::Failed(Box::new(result), crate::EXIT_ERRORS)
}

fn rejected(mut result: CheckResult, target: Target, output: &std::process::Output) -> Outcome {
    let text = String::from_utf8_lossy(&output.stdout).into_owned()
        + &String::from_utf8_lossy(&output.stderr);
    result.diagnostics.push(
        Diagnostic::new(
            codes::E0802,
            format!("{} rejected the generated code", backend(target)),
        )
        .note(text.trim_end().to_string())
        .help("this is a bug in wolf; please report it with the input that caused it"),
    );
    Outcome::Failed(Box::new(result), crate::EXIT_ERRORS)
}

/// A text as a regular expression that matches it literally.
fn regex_escape(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if "\\^$.|?*+()[]{}/".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

// ----- TypeScript ---------------------------------------------------------------------------------

fn ts(result: CheckResult, program: &wolf_ir::Program, out: &Path, then: Then) -> Outcome {
    let tests = matches!(then, Then::Test { .. });
    let files = match wolf_ts::generate(program, env!("CARGO_PKG_VERSION"), tests) {
        Ok(f) => f,
        Err(e) => return internal(result, Target::Ts, &e),
    };
    let list: Vec<(String, &str)> = files
        .iter()
        .map(|f| (f.path.clone(), f.contents.as_str()))
        .collect();
    if let Err(e) = write(out, &["src", "examples", "dist"], &list) {
        return write_failed(result, out, &e);
    }
    match then {
        Then::Nothing => Outcome::Built,
        Then::Test { filter } => run_ts_tests(
            result,
            out,
            &wolf_ts::test_entries(&files),
            filter.as_deref(),
        ),
        Then::Run { example, args } => run_ts(
            result,
            out,
            &wolf_ts::entry_file(program, example.as_deref()),
            &args,
        ),
    }
}

/// `tsc` for the output project; `node` if it built, or the failure.
fn compile_ts(result: CheckResult, out: &Path) -> Result<(PathBuf, CheckResult), Outcome> {
    let Some(node) = find_tool("node", out) else {
        return Err(missing_tool(
            result,
            Target::Ts,
            "node",
            "install Node.js 22 or later from https://nodejs.org",
        ));
    };
    let Some(tsc) = find_tool("tsc", out) else {
        return Err(missing_tool(
            result,
            Target::Ts,
            "tsc",
            "install TypeScript: `npm install -g typescript`",
        ));
    };
    match Command::new(&tsc).arg("-p").arg(out).output() {
        Ok(o) if o.status.success() => Ok((node, result)),
        Ok(o) => Err(rejected(result, Target::Ts, &o)),
        Err(e) => Err(missing_tool(
            result,
            Target::Ts,
            "tsc",
            &format!("`tsc` could not be started: {e}"),
        )),
    }
}

/// Runs the tests with `node:test` and the generated reporter (spec chapter 8, rule 2.7).
fn run_ts_tests(
    result: CheckResult,
    out: &Path,
    entries: &[String],
    filter: Option<&str>,
) -> Outcome {
    let (node, _result) = match compile_ts(result, out) {
        Ok(ok) => ok,
        Err(outcome) => return outcome,
    };
    if entries.is_empty() {
        println!("test result: ok. 0 passed; 0 failed");
        return Outcome::Ran(0);
    }
    let mut cmd = Command::new(&node);
    cmd.current_dir(out)
        .arg("--test")
        .arg("--test-concurrency=1")
        .arg("--test-reporter=./wolf_test_reporter.mjs");
    if let Some(f) = filter {
        cmd.arg(format!("--test-name-pattern={}", regex_escape(f)));
    }
    cmd.args(entries);
    match cmd.status() {
        Ok(s) if s.success() => Outcome::Ran(0),
        Ok(_) => Outcome::Ran(1),
        Err(e) => {
            eprintln!("error[E0801]: `node` could not be started: {e}");
            Outcome::Ran(1)
        }
    }
}

fn run_ts(result: CheckResult, out: &Path, entry: &str, args: &[String]) -> Outcome {
    let (node, result) = match compile_ts(result, out) {
        Ok(ok) => ok,
        Err(outcome) => return outcome,
    };
    let status = Command::new(&node).arg(out.join(entry)).args(args).status();
    match status {
        Ok(s) => Outcome::Ran(s.code().unwrap_or(1)),
        Err(e) => missing_tool(
            result,
            Target::Ts,
            "node",
            &format!("`node` could not be started: {e}"),
        ),
    }
}

// ----- Go -----------------------------------------------------------------------------------------

fn go(result: CheckResult, program: &wolf_ir::Program, out: &Path, then: Then) -> Outcome {
    let tests = matches!(then, Then::Test { .. });
    let project = match wolf_go::generate(program, env!("CARGO_PKG_VERSION"), tests) {
        Ok(p) => p,
        Err(e) => return internal(result, Target::Go, &e),
    };
    let list: Vec<(String, &str)> = project
        .files
        .iter()
        .map(|f| (f.path.clone(), f.contents.as_str()))
        .collect();
    // Every generated file is removed first, so that no stale package stays behind.
    let mut clean: Vec<String> = Vec::new();
    if let Ok(entries) = fs::read_dir(out) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name != "bin" {
                clean.push(name);
            }
        }
    }
    let clean: Vec<&str> = if out.join("go.mod").is_file() {
        clean.iter().map(String::as_str).collect()
    } else {
        Vec::new()
    };
    if let Err(e) = write(out, &clean, &list) {
        return write_failed(result, out, &e);
    }
    match then {
        Then::Nothing => Outcome::Built,
        Then::Test { filter } => run_go_tests(result, out, &project.tests, filter.as_deref()),
        Then::Run { example, args } => {
            let Some(package) = wolf_go::main_package(program, example.as_deref()) else {
                return Outcome::Built;
            };
            let name = example.unwrap_or_else(|| program.name.clone());
            run_go(result, out, &package, &name, &args)
        }
    }
}

/// `go` with the environment that keeps it local and away from the user's workspace.
fn go_command(go: &Path, out: &Path) -> Command {
    let mut cmd = Command::new(go);
    cmd.current_dir(out)
        .env("GOTOOLCHAIN", "local")
        .env("GOWORK", "off")
        .env("GOFLAGS", "-mod=mod");
    cmd
}

fn find_go(result: CheckResult, out: &Path) -> Result<(PathBuf, CheckResult), Outcome> {
    match find_tool("go", out) {
        Some(go) => Ok((go, result)),
        None => Err(missing_tool(
            result,
            Target::Go,
            "go",
            "install Go 1.24 or later from https://go.dev/dl",
        )),
    }
}

fn run_go(result: CheckResult, out: &Path, package: &str, name: &str, args: &[String]) -> Outcome {
    let (go, result) = match find_go(result, out) {
        Ok(ok) => ok,
        Err(outcome) => return outcome,
    };
    let binary = out.join("bin").join(name);
    let built = go_command(&go, out)
        .arg("build")
        .arg("-o")
        .arg(&binary)
        .arg(package)
        .output();
    match built {
        Ok(o) if o.status.success() => {}
        Ok(o) => return rejected(result, Target::Go, &o),
        Err(e) => {
            return missing_tool(
                result,
                Target::Go,
                "go",
                &format!("`go` could not be started: {e}"),
            );
        }
    }
    match Command::new(&binary).args(args).status() {
        Ok(s) => Outcome::Ran(s.code().unwrap_or(1)),
        Err(e) => {
            eprintln!("error: `{}` could not be started: {e}", binary.display());
            Outcome::Ran(1)
        }
    }
}

/// Runs the tests with `go test -json` and prints the report of `wolf test` (spec chapter 8,
/// rule 2.7).
fn run_go_tests(
    result: CheckResult,
    out: &Path,
    tests: &[wolf_go::TestEntry],
    filter: Option<&str>,
) -> Outcome {
    let (go, result) = match find_go(result, out) {
        Ok(ok) => ok,
        Err(outcome) => return outcome,
    };
    // `go vet` builds every package with its tests: a failure is a bug of the backend.
    match go_command(&go, out).arg("vet").arg("./...").output() {
        Ok(o) if o.status.success() => {}
        Ok(o) => return rejected(result, Target::Go, &o),
        Err(e) => {
            return missing_tool(
                result,
                Target::Go,
                "go",
                &format!("`go` could not be started: {e}"),
            );
        }
    }
    let selected: Vec<&wolf_go::TestEntry> = tests
        .iter()
        // A test is named by its file and its name, as in the report.
        .filter(|t| filter.is_none_or(|f| t.name.contains(f)))
        .collect();
    let mut outcomes: BTreeMap<(String, String), (bool, Option<String>)> = BTreeMap::new();
    if !selected.is_empty() {
        let names: Vec<&str> = selected.iter().map(|t| t.go_name.as_str()).collect();
        let output = go_command(&go, out)
            .arg("test")
            .arg("-json")
            .arg("-count=1")
            .arg("-p=1")
            .arg(format!("-run=^({})$", names.join("|")))
            .arg("./...")
            .output();
        let output = match output {
            Ok(o) => o,
            Err(e) => {
                return missing_tool(
                    result,
                    Target::Go,
                    "go",
                    &format!("`go` could not be started: {e}"),
                );
            }
        };
        let text = String::from_utf8_lossy(&output.stdout);
        for line in text.lines() {
            let (Some(action), Some(package)) =
                (json_field(line, "Action"), json_field(line, "Package"))
            else {
                continue;
            };
            let Some(test) = json_field(line, "Test") else {
                continue;
            };
            let entry = outcomes.entry((package, test)).or_insert((false, None));
            match action.as_str() {
                "pass" => entry.0 = true,
                "output" => {
                    if let Some(m) = json_field(line, "Output")
                        .and_then(|o| o.strip_prefix("--- wolf failure: ").map(str::to_string))
                    {
                        entry.1 = Some(m.trim_end().to_string());
                    }
                }
                _ => {}
            }
        }
    }
    if tests.is_empty() {
        println!("test result: ok. 0 passed; 0 failed");
        return Outcome::Ran(0);
    }
    let mut passed = 0;
    let mut failures = Vec::new();
    for t in &selected {
        let (ok, message) = outcomes
            .get(&(t.package.clone(), t.go_name.clone()))
            .cloned()
            .unwrap_or((false, None));
        if ok {
            passed += 1;
            println!("test {} ... ok", t.name);
        } else {
            println!("test {} ... FAILED", t.name);
            failures.push((
                t.name.clone(),
                message.unwrap_or_else(|| "test failed".to_string()),
            ));
        }
    }
    if !failures.is_empty() {
        println!("\nfailures:");
        for (name, message) in &failures {
            println!("\n  {name}\n    {message}");
        }
    }
    let status = if failures.is_empty() { "ok" } else { "FAILED" };
    println!(
        "\ntest result: {status}. {passed} passed; {} failed",
        failures.len()
    );
    Outcome::Ran(if failures.is_empty() { 0 } else { 1 })
}

/// The string value of a field of a one-line JSON object, such as an event of `go test -json`.
fn json_field(line: &str, key: &str) -> Option<String> {
    let start = line.find(&format!("\"{key}\":\""))? + key.len() + 4;
    let mut out = String::new();
    let mut chars = line.get(start..)?.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(out),
            '\\' => match chars.next()? {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                'r' => out.push('\r'),
                'u' => {
                    let hex: String = chars.by_ref().take(4).collect();
                    let code = u32::from_str_radix(&hex, 16).ok()?;
                    out.push(char::from_u32(code).unwrap_or('\u{fffd}'));
                }
                other => out.push(other),
            },
            c => out.push(c),
        }
    }
    None
}

// ----- Rust ---------------------------------------------------------------------------------------

fn rust(result: CheckResult, program: &wolf_ir::Program, out: &Path, then: Then) -> Outcome {
    let tests = matches!(then, Then::Test { .. });
    let project = match wolf_rs::generate(program, env!("CARGO_PKG_VERSION"), tests) {
        Ok(p) => p,
        Err(e) => return internal(result, Target::Rust, &e),
    };
    let list: Vec<(String, &str)> = project
        .files
        .iter()
        .map(|f| (f.path.clone(), f.contents.as_str()))
        .collect();
    // Every generated file is removed first; the build cache in `target/` stays.
    let mut clean: Vec<String> = Vec::new();
    if out.join("Cargo.toml").is_file() {
        if let Ok(entries) = fs::read_dir(out) {
            for e in entries.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if name != "target" && name != "Cargo.lock" {
                    clean.push(name);
                }
            }
        }
    }
    let clean: Vec<&str> = clean.iter().map(String::as_str).collect();
    if let Err(e) = write(out, &clean, &list) {
        return write_failed(result, out, &e);
    }
    match then {
        Then::Nothing => Outcome::Built,
        Then::Test { filter } => {
            run_rust_tests(result, out, program, &project.tests, filter.as_deref())
        }
        Then::Run { example, args } => run_rust(result, out, program, example.as_deref(), &args),
    }
}

/// `cargo` for the generated project, with its own target directory.
fn cargo_command(cargo: &Path, out: &Path) -> Command {
    let mut cmd = Command::new(cargo);
    cmd.current_dir(out)
        .env("CARGO_TARGET_DIR", out.join("target"))
        .env_remove("RUSTFLAGS")
        .env_remove("CARGO_BUILD_TARGET");
    cmd
}

fn find_cargo(result: CheckResult, out: &Path) -> Result<(PathBuf, CheckResult), Outcome> {
    match find_tool("cargo", out) {
        Some(cargo) => Ok((cargo, result)),
        None => Err(missing_tool(
            result,
            Target::Rust,
            "cargo",
            "install Rust 1.85 or later from https://rustup.rs",
        )),
    }
}

fn run_rust(
    result: CheckResult,
    out: &Path,
    program: &wolf_ir::Program,
    example: Option<&str>,
    args: &[String],
) -> Outcome {
    let (cargo, result) = match find_cargo(result, out) {
        Ok(ok) => ok,
        Err(outcome) => return outcome,
    };
    let mut cmd = cargo_command(&cargo, out);
    cmd.arg("build").arg("--quiet");
    let binary = match example {
        Some(name) => {
            cmd.arg("--example").arg(name);
            out.join("target").join("debug").join("examples").join(name)
        }
        None => out.join("target").join("debug").join(&program.name),
    };
    match cmd.output() {
        Ok(o) if o.status.success() => {}
        Ok(o) => return rejected(result, Target::Rust, &o),
        Err(e) => {
            return missing_tool(
                result,
                Target::Rust,
                "cargo",
                &format!("`cargo` could not be started: {e}"),
            );
        }
    }
    match Command::new(&binary).args(args).status() {
        Ok(s) => Outcome::Ran(s.code().unwrap_or(1)),
        Err(e) => {
            eprintln!("error: `{}` could not be started: {e}", binary.display());
            Outcome::Ran(1)
        }
    }
}

/// Runs the tests with the test harness of Rust, one Cargo target at a time, and prints the
/// report of `wolf test` (spec chapter 8, rule 2.7).
fn run_rust_tests(
    result: CheckResult,
    out: &Path,
    program: &wolf_ir::Program,
    tests: &[wolf_rs::TestEntry],
    filter: Option<&str>,
) -> Outcome {
    let (cargo, result) = match find_cargo(result, out) {
        Ok(ok) => ok,
        Err(outcome) => return outcome,
    };
    if tests.is_empty() {
        println!("test result: ok. 0 passed; 0 failed");
        return Outcome::Ran(0);
    }
    let selected: Vec<&wolf_rs::TestEntry> = tests
        .iter()
        .filter(|t| filter.is_none_or(|f| t.name.contains(f)))
        .collect();
    let mut targets: Vec<&str> = selected.iter().map(|t| t.target.as_str()).collect();
    targets.sort();
    targets.dedup();
    // (target, path) to passed and the failure message.
    let mut outcomes: BTreeMap<(String, String), (bool, Option<String>)> = BTreeMap::new();
    for target in targets {
        let paths: Vec<&str> = selected
            .iter()
            .filter(|t| t.target == target)
            .map(|t| t.path.as_str())
            .collect();
        // Not `--quiet`: that makes the test harness print dots instead of a line per test.
        let mut cmd = cargo_command(&cargo, out);
        cmd.arg("test");
        match target {
            "lib" => {
                cmd.arg("--lib");
            }
            "bin" => {
                cmd.arg("--bin").arg(&program.name);
            }
            name => {
                cmd.arg("--test").arg(name);
            }
        }
        cmd.arg("--")
            .arg("--exact")
            .arg("--test-threads=1")
            .args(&paths);
        let output = match cmd.output() {
            Ok(o) => o,
            Err(e) => {
                return missing_tool(
                    result,
                    Target::Rust,
                    "cargo",
                    &format!("`cargo` could not be started: {e}"),
                );
            }
        };
        let text = String::from_utf8_lossy(&output.stdout).into_owned();
        if !text.contains("test result:") {
            return rejected(result, Target::Rust, &output);
        }
        let mut current: Option<String> = None;
        for line in text.lines() {
            if let Some(rest) = line.strip_prefix("test ") {
                if let Some((name, status)) = rest.rsplit_once(" ... ") {
                    let ok = status == "ok";
                    outcomes
                        .entry((target.to_string(), name.to_string()))
                        .or_insert((false, None))
                        .0 = ok;
                }
            } else if let Some(rest) = line.strip_prefix("---- ") {
                current = rest.strip_suffix(" stdout ----").map(str::to_string);
            } else if let Some(m) = line.strip_prefix("--- wolf failure: ") {
                if let Some(name) = &current {
                    outcomes
                        .entry((target.to_string(), name.clone()))
                        .or_insert((false, None))
                        .1 = Some(m.to_string());
                }
            }
        }
    }
    let mut passed = 0;
    let mut failures = Vec::new();
    for t in &selected {
        let (ok, message) = outcomes
            .get(&(t.target.clone(), t.path.clone()))
            .cloned()
            .unwrap_or((false, None));
        if ok {
            passed += 1;
            println!("test {} ... ok", t.name);
        } else {
            println!("test {} ... FAILED", t.name);
            failures.push((
                t.name.clone(),
                message.unwrap_or_else(|| "test failed".to_string()),
            ));
        }
    }
    if !failures.is_empty() {
        println!("\nfailures:");
        for (name, message) in &failures {
            println!("\n  {name}\n    {message}");
        }
    }
    let status = if failures.is_empty() { "ok" } else { "FAILED" };
    println!(
        "\ntest result: {status}. {passed} passed; {} failed",
        failures.len()
    );
    Outcome::Ran(if failures.is_empty() { 0 } else { 1 })
}

#[cfg(test)]
mod tests {
    use super::json_field;

    #[test]
    fn fields_of_go_test_events() {
        let line = r#"{"Time":"t","Action":"output","Package":"calc","Test":"TestAdds","Output":"--- wolf failure: panic at a \"b\"\n"}"#;
        assert_eq!(json_field(line, "Action").as_deref(), Some("output"));
        assert_eq!(json_field(line, "Test").as_deref(), Some("TestAdds"));
        assert_eq!(
            json_field(line, "Output").as_deref(),
            Some("--- wolf failure: panic at a \"b\"\n")
        );
        assert_eq!(json_field(line, "Elapsed"), None);
    }
}
