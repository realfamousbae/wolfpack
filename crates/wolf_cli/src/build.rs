//! `wolf build` and `wolf run`: lowering to IR, code generation and the tools of the target.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use wolf_diag::{Diagnostic, codes};

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

/// Checks, lowers and generates the project around `start`; with `Then::Run`, also runs it.
/// `explicit` is whether `--target ts` was given; otherwise the manifest's default is used.
pub(crate) fn build(start: &Path, out_dir: Option<&Path>, then: Then, explicit: bool) -> Outcome {
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
    if !explicit && project.manifest.default_target != wolf_project::Target::Ts {
        let name = match project.manifest.default_target {
            wolf_project::Target::Go => "go",
            _ => "rust",
        };
        result.diagnostics.push(
            Diagnostic::new(
                codes::E0002,
                format!("the default target `{name}` of `wolfpack.toml` is not available yet"),
            )
            .help("only TypeScript can be built so far: pass `--target ts`"),
        );
        return Outcome::Failed(Box::new(result), crate::EXIT_USAGE);
    }
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
        Err(e) => return internal(result, &e.message),
    };
    let tests = matches!(then, Then::Test { .. });
    let files = match wolf_ts::generate(&program, env!("CARGO_PKG_VERSION"), tests) {
        Ok(f) => f,
        Err(e) => return internal(result, &e),
    };
    let out = out_dir
        .map(Path::to_path_buf)
        .unwrap_or_else(|| project.root.join("out").join("ts"));
    if let Err(e) = write(&out, &files) {
        result.diagnostics.push(Diagnostic::new(
            codes::E0001,
            format!("cannot write `{}`: {e}", out.display()),
        ));
        return Outcome::Failed(Box::new(result), crate::EXIT_ERRORS);
    }
    match then {
        Then::Nothing => Outcome::Built,
        Then::Test { filter } => run_tests(
            result,
            &out,
            &wolf_ts::test_entries(&files),
            filter.as_deref(),
        ),
        Then::Run { example, args } => {
            if example.is_none() && program.kind == wolf_ir::ProgramKind::Lib {
                result.diagnostics.push(
                    Diagnostic::new(codes::E0002, "a library cannot be run")
                        .help("run one of its examples: `wolf run --example <name>`"),
                );
                return Outcome::Failed(Box::new(result), crate::EXIT_USAGE);
            }
            if let Some(name) = &example {
                if !files
                    .iter()
                    .any(|f| f.path == format!("examples/{name}.ts"))
                {
                    result.diagnostics.push(Diagnostic::new(
                        codes::E0002,
                        format!("there is no example `{name}` in `examples/`"),
                    ));
                    return Outcome::Failed(Box::new(result), crate::EXIT_USAGE);
                }
            }
            run_ts(
                result,
                &out,
                &wolf_ts::entry_file(&program, example.as_deref()),
                &args,
            )
        }
    }
}

fn internal(mut result: CheckResult, message: &str) -> Outcome {
    result.diagnostics.push(
        Diagnostic::new(codes::E0003, "internal compiler error")
            .note(format!(
                "the TypeScript backend does not support this yet: {message}"
            ))
            .help("this is a bug or a gap in wolf; please report it with the input that caused it"),
    );
    Outcome::Failed(Box::new(result), crate::EXIT_INTERNAL)
}

/// Writes the generated files, removing older generated sources first.
fn write(out: &Path, files: &[wolf_ts::OutFile]) -> std::io::Result<()> {
    for dir in ["src", "examples", "dist"] {
        let path = out.join(dir);
        if path.exists() {
            fs::remove_dir_all(&path)?;
        }
    }
    for f in files {
        let path = out.join(&f.path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, &f.contents)?;
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

fn missing_tool(mut result: CheckResult, tool: &str, help: &str) -> Outcome {
    result.diagnostics.push(
        Diagnostic::new(codes::E0801, format!("`{tool}` is not installed"))
            .note("`wolf run --target ts` builds with TypeScript and runs with Node.js")
            .help(help.to_string()),
    );
    Outcome::Failed(Box::new(result), crate::EXIT_ERRORS)
}

/// `tsc` for the output project; `None` if it built, or the failure.
fn compile(result: CheckResult, out: &Path) -> Result<(PathBuf, CheckResult), Outcome> {
    let Some(node) = find_tool("node", out) else {
        return Err(missing_tool(
            result,
            "node",
            "install Node.js 22 or later from https://nodejs.org",
        ));
    };
    let Some(tsc) = find_tool("tsc", out) else {
        return Err(missing_tool(
            result,
            "tsc",
            "install TypeScript: `npm install -g typescript`",
        ));
    };
    let mut result = result;
    match Command::new(&tsc).arg("-p").arg(out).output() {
        Ok(o) if o.status.success() => Ok((node, result)),
        Ok(o) => {
            let text = String::from_utf8_lossy(&o.stdout).into_owned()
                + &String::from_utf8_lossy(&o.stderr);
            result.diagnostics.push(
                Diagnostic::new(codes::E0802, "TypeScript rejected the generated code")
                    .note(text.trim_end().to_string())
                    .help("this is a bug in wolf; please report it with the input that caused it"),
            );
            Err(Outcome::Failed(Box::new(result), crate::EXIT_ERRORS))
        }
        Err(e) => Err(missing_tool(
            result,
            "tsc",
            &format!("`tsc` could not be started: {e}"),
        )),
    }
}

/// Runs the tests with `node:test` and the generated reporter (spec chapter 8, rule 2.7).
fn run_tests(result: CheckResult, out: &Path, entries: &[String], filter: Option<&str>) -> Outcome {
    let (node, _result) = match compile(result, out) {
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

fn run_ts(mut result: CheckResult, out: &Path, entry: &str, args: &[String]) -> Outcome {
    let Some(node) = find_tool("node", out) else {
        return missing_tool(
            result,
            "node",
            "install Node.js 22 or later from https://nodejs.org",
        );
    };
    let Some(tsc) = find_tool("tsc", out) else {
        return missing_tool(
            result,
            "tsc",
            "install TypeScript: `npm install -g typescript`",
        );
    };
    let built = Command::new(&tsc).arg("-p").arg(out).output();
    match built {
        Ok(o) if o.status.success() => {}
        Ok(o) => {
            let text = String::from_utf8_lossy(&o.stdout).into_owned()
                + &String::from_utf8_lossy(&o.stderr);
            result.diagnostics.push(
                Diagnostic::new(codes::E0802, "TypeScript rejected the generated code")
                    .note(text.trim_end().to_string())
                    .help("this is a bug in wolf; please report it with the input that caused it"),
            );
            return Outcome::Failed(Box::new(result), crate::EXIT_ERRORS);
        }
        Err(e) => {
            return missing_tool(result, "tsc", &format!("`tsc` could not be started: {e}"));
        }
    }
    let status = Command::new(&node).arg(out.join(entry)).args(args).status();
    match status {
        Ok(s) => Outcome::Ran(s.code().unwrap_or(1)),
        Err(e) => missing_tool(result, "node", &format!("`node` could not be started: {e}")),
    }
}
