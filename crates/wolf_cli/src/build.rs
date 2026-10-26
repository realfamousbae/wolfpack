//! `wolf build`, `wolf run` and `wolf test`: lowering to IR, code generation and the tools of
//! the target.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
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
    /// `--vm`: the virtual machine (`wolf run` and `wolf test` only; spec chapter 10).
    Vm,
}

/// Checks, lowers and generates the project around `start`; with `Then::Run`, also runs it.
pub(crate) fn build(start: &Path, out_dir: Option<&Path>, then: Then, targets: Targets) -> Outcome {
    let args = CheckArgs {
        paths: Vec::new(),
        message_format: MessageFormat::Human,
        color: Color::Never,
        emit: None,
        native: None,
        bytecode: None,
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
    // Every module of the build must be available for each target (chapter 7, section 8.5).
    let tests = matches!(then, Then::Test { .. });
    let checked: Vec<Target> = match targets {
        Targets::All => project.manifest.enabled.clone(),
        Targets::Vm => Vec::new(),
        _ => vec![target],
    };
    if targets == Targets::Vm {
        // The VM runs no code of the targets (spec chapter 10, section 3). The help names the
        // first target, the default one first, for which the program can be built.
        let built = |role: wolf_resolve::Role| match role {
            wolf_resolve::Role::Example => !tests,
            wolf_resolve::Role::Test => tests,
            _ => true,
        };
        let mut order = vec![target];
        order.extend(project.manifest.enabled.iter().filter(|t| **t != target));
        let command = if tests { "test" } else { "run" };
        let help = match order
            .iter()
            .find(|t| wolf_resolve::check_target(&inputs, t.name(), built).is_empty())
        {
            Some(t) => format!("run it on a target: `wolf {command} --target {}`", t.name()),
            None => "no target has the code of every module either; see `wolf build`".to_string(),
        };
        let found = wolf_resolve::check_vm(
            &inputs,
            |m| match &then {
                // Tests run from the modules that have them (spec chapter 10, section 3, rule 2).
                Then::Test { .. } => match m.role {
                    wolf_resolve::Role::Entry | wolf_resolve::Role::Test => true,
                    wolf_resolve::Role::Src => m
                        .ast
                        .sections
                        .iter()
                        .any(|s| s.kind == wolf_ast::SectionKind::Tests),
                    _ => false,
                },
                Then::Run {
                    example: Some(name),
                    ..
                } => {
                    m.role == wolf_resolve::Role::Example
                        && m.path.get(1..) == Some(&[name.clone()][..])
                }
                _ => m.role == wolf_resolve::Role::Entry,
            },
            &help,
        );
        result.diagnostics.extend(found);
    }
    for t in checked {
        let found = wolf_resolve::check_target(&inputs, t.name(), |role| match role {
            wolf_resolve::Role::Example => !tests,
            wolf_resolve::Role::Test => tests,
            _ => true,
        });
        result.diagnostics.extend(found);
    }
    if !result.diagnostics.is_empty() {
        return Outcome::Failed(Box::new(result), crate::EXIT_ERRORS);
    }
    let meta = wolf_driver::meta(&project.manifest, true);
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
    if targets == Targets::Vm {
        return vm(result, &program, then);
    }
    if targets == Targets::All {
        for t in project.manifest.enabled.clone() {
            let out = match out_dir {
                Some(d) => d.join(t.name()),
                None => project.root.join("out").join(t.name()),
            };
            result = match generate_for(result, t, &program, &project.root, &out, Then::Nothing) {
                Outcome::Failed(r, code) => return Outcome::Failed(r, code),
                _ => CheckResult {
                    map: wolf_span::SourceMap::new(),
                    diagnostics: Vec::new(),
                    project: None,
                    native: false,
                    listings: Vec::new(),
                },
            };
        }
        return Outcome::Built;
    }
    let out = out_dir
        .map(Path::to_path_buf)
        .unwrap_or_else(|| project.root.join("out").join(target.name()));
    generate_for(result, target, &program, &project.root, &out, then)
}

/// The stack of the thread of the VM: calls from std back into the program (`map`, `sort_by`)
/// nest on it, up to the limit of the VM, also in a debug build. Only the part in use is taken
/// from memory.
const VM_STACK_SIZE: usize = 256 * 1024 * 1024;

/// Runs the program or its tests in the virtual machine (spec chapter 10).
fn vm(mut result: CheckResult, program: &wolf_ir::Program, then: Then) -> Outcome {
    if matches!(then, Then::Nothing) {
        return Outcome::Built;
    }
    let ran = std::thread::scope(|scope| {
        std::thread::Builder::new()
            .name("wolf-vm".to_string())
            .stack_size(VM_STACK_SIZE)
            .spawn_scoped(scope, || run_vm(program, then))
            .map_err(|e| format!("the thread of the VM could not be started: {e}"))
            .and_then(|handle| {
                handle
                    .join()
                    .unwrap_or_else(|_| Err("the VM stopped unexpectedly".to_string()))
            })
    });
    match ran {
        Ok(code) => Outcome::Ran(code),
        Err(e) => {
            result.diagnostics.push(
                Diagnostic::new(codes::E0003, "internal compiler error")
                    .note(format!("the VM failed: {e}"))
                    .help("this is a bug in wolf; please report it with the input that caused it"),
            );
            Outcome::Failed(Box::new(result), crate::EXIT_INTERNAL)
        }
    }
}

fn run_vm(program: &wolf_ir::Program, then: Then) -> Result<i32, String> {
    match then {
        Then::Run { example, args } => {
            let module = program.modules.iter().position(|m| match &example {
                Some(name) => {
                    m.role == wolf_ir::ModuleRole::Example
                        && m.path.get(1..) == Some(&[name.clone()][..])
                }
                None => m.role == wolf_ir::ModuleRole::Entry,
            });
            match module {
                Some(i) => wolf_vm::run_main(program, i, &mut wolf_vm::StdIo::new(args)),
                None => Err("the module to run is missing".to_string()),
            }
        }
        Then::Test { filter } => wolf_vm::run_tests(
            program,
            filter.as_deref(),
            &mut wolf_vm::StdIo::new(Vec::new()),
        ),
        Then::Nothing => Ok(0),
    }
}

fn generate_for(
    mut result: CheckResult,
    target: Target,
    program: &wolf_ir::Program,
    root: &Path,
    out: &Path,
    then: Then,
) -> Outcome {
    let native = wolf_driver::Native::load(root);
    result.native = !native.files(target.name()).is_empty();
    match target {
        Target::Go => go(result, program, &native, root, out, then),
        Target::Rust => rust(result, program, &native, root, out, then),
        Target::Ts => ts(result, program, &native, root, out, then),
    }
}

/// Copies the files of `native/<target>/` byte for byte to `out/<dest>/` (spec chapter 8,
/// section 8a).
fn copy_native(
    native: &wolf_driver::Native,
    root: &Path,
    target: Target,
    out: &Path,
    dest: &str,
) -> std::io::Result<()> {
    let from = root.join("native").join(target.name());
    for rel in native.files(target.name()) {
        let to = out.join(dest).join(rel);
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(from.join(rel), &to)?;
    }
    Ok(())
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
    let diagnostic = if result.native {
        let dir = format!("native/{}/", target.name());
        Diagnostic::new(
            codes::E0806,
            format!("{} rejected the code of `{dir}`", backend(target)),
        )
        .note(text.trim_end().to_string())
        .help(format!(
            "check the files of `{dir}` and that the `extern` declarations match them; if neither is wrong, this is a bug in wolf"
        ))
    } else {
        Diagnostic::new(
            codes::E0802,
            format!("{} rejected the generated code", backend(target)),
        )
        .note(text.trim_end().to_string())
        .help("this is a bug in wolf; please report it with the input that caused it")
    };
    result.diagnostics.push(diagnostic);
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

fn ts(
    result: CheckResult,
    program: &wolf_ir::Program,
    native: &wolf_driver::Native,
    root: &Path,
    out: &Path,
    then: Then,
) -> Outcome {
    let tests = matches!(then, Then::Test { .. });
    let files = match wolf_ts::generate(program, env!("CARGO_PKG_VERSION"), tests) {
        Ok(f) => f,
        Err(e) => return internal(result, Target::Ts, &e),
    };
    let list: Vec<(String, &str)> = files
        .iter()
        .map(|f| (f.path.clone(), f.contents.as_str()))
        .collect();
    // `test/` too: a test file of an earlier build whose `.wolf` file is gone would still be
    // compiled by `tsc`.
    if let Err(e) = write(out, &["src", "examples", "test", "dist"], &list)
        .and_then(|()| copy_native(native, root, Target::Ts, out, "src/native"))
    {
        return write_failed(result, out, &e);
    }
    // Packages of `[extern.ts.dependencies]` are installed before the project is compiled.
    let needs_install = !matches!(then, Then::Nothing)
        && program
            .dependencies
            .get("ts")
            .is_some_and(|d| !d.is_empty());
    let result = if needs_install {
        match npm_install(result, out) {
            Ok(r) => r,
            Err(outcome) => return outcome,
        }
    } else {
        result
    };
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

/// `npm install` in the output project (spec chapter 8, section 2, rule 5).
fn npm_install(mut result: CheckResult, out: &Path) -> Result<CheckResult, Outcome> {
    let Some(npm) = crate::tools::find("npm", out) else {
        return Err(missing_tool(
            result,
            Target::Ts,
            "npm",
            "run `wolf setup`, or install Node.js 22 or later from https://nodejs.org; it includes npm",
        ));
    };
    let output = npm
        .command()
        .args(["install", "--no-audit", "--no-fund", "--loglevel=error"])
        .current_dir(out)
        .output();
    match output {
        Ok(o) if o.status.success() => Ok(result),
        Ok(o) => {
            let text = String::from_utf8_lossy(&o.stderr).trim().to_string();
            result.diagnostics.push(
                Diagnostic::new(codes::E0803, "the dependencies of the target could not be fetched")
                    .note(text)
                    .help("check the names and versions in `[extern.ts.dependencies]` and the network connection"),
            );
            Err(Outcome::Failed(Box::new(result), crate::EXIT_ERRORS))
        }
        Err(e) => Err(missing_tool(
            result,
            Target::Ts,
            "npm",
            &format!("`npm` could not be started: {e}"),
        )),
    }
}

/// `tsc` for the output project; `node` if it built, or the failure.
fn compile_ts(
    result: CheckResult,
    out: &Path,
) -> Result<(crate::tools::Tool, CheckResult), Outcome> {
    let Some(node) = crate::tools::find("node", out) else {
        return Err(missing_tool(
            result,
            Target::Ts,
            "node",
            "run `wolf setup`, or install Node.js 22 or later from https://nodejs.org",
        ));
    };
    let Some(tsc) = crate::tools::find("tsc", out) else {
        return Err(missing_tool(
            result,
            Target::Ts,
            "tsc",
            "run `wolf setup`, or install TypeScript: `npm install -g typescript`",
        ));
    };
    match tsc.command().arg("-p").arg(out).output() {
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
    let mut cmd = node.command();
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
    let status = node.command().arg(out.join(entry)).args(args).status();
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

fn go(
    result: CheckResult,
    program: &wolf_ir::Program,
    native: &wolf_driver::Native,
    root: &Path,
    out: &Path,
    then: Then,
) -> Outcome {
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
    if let Err(e) = write(out, &clean, &list)
        .and_then(|()| copy_native(native, root, Target::Go, out, "native"))
    {
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
fn go_command(go: &crate::tools::Tool, out: &Path) -> Command {
    let mut cmd = go.command();
    cmd.current_dir(out)
        .env("GOTOOLCHAIN", "local")
        .env("GOWORK", "off")
        .env("GOFLAGS", "-mod=mod");
    cmd
}

fn find_go(result: CheckResult, out: &Path) -> Result<(crate::tools::Tool, CheckResult), Outcome> {
    match crate::tools::find("go", out) {
        Some(go) => Ok((go, result)),
        None => Err(missing_tool(
            result,
            Target::Go,
            "go",
            "run `wolf setup`, or install Go 1.24 or later from https://go.dev/dl",
        )),
    }
}

fn run_go(result: CheckResult, out: &Path, package: &str, name: &str, args: &[String]) -> Outcome {
    let (go, result) = match find_go(result, out) {
        Ok(ok) => ok,
        Err(outcome) => return outcome,
    };
    let binary = out.join("bin").join(crate::tools::exe(name));
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

fn rust(
    result: CheckResult,
    program: &wolf_ir::Program,
    native: &wolf_driver::Native,
    root: &Path,
    out: &Path,
    then: Then,
) -> Outcome {
    let tests = matches!(then, Then::Test { .. });
    let project = match wolf_rs::generate(program, env!("CARGO_PKG_VERSION"), tests) {
        Ok(p) => p,
        Err(e) => return internal(result, Target::Rust, &e),
    };
    let native_files = native.files("rust");
    let wolf = program
        .modules
        .iter()
        .any(|m| m.role == wolf_ir::ModuleRole::Native);
    let mods = if native_files.is_empty() && !wolf {
        Vec::new()
    } else {
        wolf_rs::native_modules(native_files, wolf)
    };
    let list: Vec<(String, &str)> = project
        .files
        .iter()
        .chain(&mods)
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
    if let Err(e) = write(out, &clean, &list)
        .and_then(|()| copy_native(native, root, Target::Rust, out, "src/native"))
    {
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
fn cargo_command(cargo: &crate::tools::Tool, out: &Path) -> Command {
    let mut cmd = cargo.command();
    cmd.current_dir(out)
        .env("CARGO_TARGET_DIR", out.join("target"))
        .env_remove("RUSTFLAGS")
        .env_remove("CARGO_BUILD_TARGET");
    cmd
}

fn find_cargo(
    result: CheckResult,
    out: &Path,
) -> Result<(crate::tools::Tool, CheckResult), Outcome> {
    match crate::tools::find("cargo", out) {
        Some(cargo) => Ok((cargo, result)),
        None => Err(missing_tool(
            result,
            Target::Rust,
            "cargo",
            "run `wolf setup`, or install Rust 1.85 or later from https://rustup.rs",
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
            out.join("target")
                .join("debug")
                .join("examples")
                .join(crate::tools::exe(name))
        }
        None => out
            .join("target")
            .join("debug")
            .join(crate::tools::exe(&wolf_rs::bin_name(program))),
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
                cmd.arg("--bin").arg(wolf_rs::bin_name(program));
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
