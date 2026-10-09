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
pub(crate) fn build(start: &Path, out_dir: Option<&Path>, then: Then) -> Outcome {
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
    let files = match wolf_ts::generate(&program, env!("CARGO_PKG_VERSION")) {
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
