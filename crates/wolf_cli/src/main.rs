//! The `wolf` command-line tool.
//!
//! `wolf check` checks a project, `wolf build` generates the target project in `out/<target>/`,
//! `wolf run` builds and runs it with the tools of the target, and `wolf new` creates a project.

mod build;
mod check;
mod fmt;

use std::io::{self, IsTerminal, Write};
use std::panic;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use codespan_reporting::term::termcolor::{ColorChoice, StandardStream};
use wolf_diag::{Diagnostic, codes};
use wolf_span::SourceMap;

/// Exit codes of `wolf` (spec chapter 9, section 4).
pub(crate) const EXIT_ERRORS: u8 = 1;
pub(crate) const EXIT_USAGE: u8 = 2;
pub(crate) const EXIT_INTERNAL: u8 = 3;

/// The Wolfpack compiler.
#[derive(Parser)]
#[command(name = "wolf", version, about = "The Wolfpack compiler")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Check the project without generating code.
    Check(CheckArgs),
    /// Create a new project with the standard layout.
    New(NewArgs),
    /// Generate the project for a target in `out/<target>/`.
    Build(BuildArgs),
    /// Build the project and run it with the tools of the target.
    Run(RunArgs),
    /// Run the tests of the project: `tests` sections and the modules under `tests/`.
    Test(TestArgs),
    /// Format the source files of the project, or the given files and directories.
    Fmt(FmtArgs),
}

/// Options of `wolf fmt`.
#[derive(clap::Args)]
struct FmtArgs {
    /// Files or directories to format; without them, the `.wolf` files of the project.
    paths: Vec<PathBuf>,

    /// Change nothing; list the files that are not formatted and exit with code 1.
    #[arg(long)]
    check: bool,
}

/// Options of `wolf test`.
#[derive(clap::Args)]
struct TestArgs {
    /// Run only the tests whose name contains this text.
    filter: Option<String>,

    /// The target; the default target of `wolfpack.toml` if absent.
    #[arg(long, value_enum)]
    target: Option<TargetArg>,

    /// Build in this directory instead of `out/<target>/` (for tests).
    #[arg(long, hide = true)]
    out: Option<PathBuf>,
}

/// A target language.
#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
enum TargetArg {
    /// TypeScript for Node.js.
    Ts,
    /// Go.
    Go,
    /// Rust.
    Rust,
    /// Every enabled target.
    All,
}

/// Options of `wolf build`.
#[derive(clap::Args)]
struct BuildArgs {
    /// The target; the default target of `wolfpack.toml` if absent.
    #[arg(long, value_enum)]
    target: Option<TargetArg>,

    /// Write the generated project here instead of `out/<target>/` (for tests).
    #[arg(long, hide = true)]
    out: Option<PathBuf>,
}

/// Options of `wolf run`.
#[derive(clap::Args)]
struct RunArgs {
    /// The target; the default target of `wolfpack.toml` if absent.
    #[arg(long, value_enum)]
    target: Option<TargetArg>,

    /// Run the example `examples/<name>.wolf` instead of the program.
    #[arg(long)]
    example: Option<String>,

    /// Build in this directory instead of `out/<target>/` (for tests).
    #[arg(long, hide = true)]
    out: Option<PathBuf>,

    /// Arguments for the program, after `--`.
    #[arg(last = true)]
    args: Vec<String>,
}

/// Options of `wolf new`.
#[derive(clap::Args)]
struct NewArgs {
    /// The project name in kebab-case; also the directory to create.
    name: String,

    /// The kind of project.
    #[arg(long, value_enum, default_value_t = ProjectKind::Cli)]
    kind: ProjectKind,
}

/// `--kind` of `wolf new`.
#[derive(Clone, Copy, ValueEnum)]
enum ProjectKind {
    /// A program with `src/main.wolf`.
    Cli,
    /// A library with `src/lib.wolf`.
    Lib,
}

/// Options of `wolf check`.
#[derive(clap::Args)]
pub(crate) struct CheckArgs {
    /// A project directory, or single `.wolf` files to lex and parse without a project.
    /// Without paths, the project around the current directory is checked.
    pub(crate) paths: Vec<PathBuf>,

    /// Output format of diagnostics.
    #[arg(long, value_enum, default_value_t = MessageFormat::Human)]
    pub(crate) message_format: MessageFormat,

    /// When to use colours in human output.
    #[arg(long, value_enum, default_value_t = Color::Auto)]
    pub(crate) color: Color,

    /// Print intermediate results to standard output (for debugging the compiler).
    #[arg(long, value_enum, hide = true)]
    pub(crate) emit: Option<Emit>,
}

/// How diagnostics are printed.
#[derive(Clone, Copy, ValueEnum)]
pub(crate) enum MessageFormat {
    /// Text for people, in the style of rustc.
    Human,
    /// One JSON object per line.
    Json,
}

/// Colour choice for human output.
#[derive(Clone, Copy, ValueEnum)]
pub(crate) enum Color {
    /// Colours when standard error is a terminal.
    Auto,
    /// Always colours.
    Always,
    /// Never colours.
    Never,
}

/// Intermediate results that can be printed.
#[derive(Clone, Copy, ValueEnum)]
pub(crate) enum Emit {
    /// The token stream of each file.
    Tokens,
    /// The syntax tree of each file.
    Ast,
}

fn main() -> ExitCode {
    // An internal error must not print Rust's panic message (spec chapter 9, section 1.4).
    panic::set_hook(Box::new(|_| {}));
    // The compiler runs on a thread with a large stack, so that input nested up to the parser's
    // limit cannot overflow it (see `wolf_parser::STACK_SIZE`).
    let worker = std::thread::Builder::new()
        .name("wolf".to_string())
        .stack_size(wolf_parser::STACK_SIZE)
        .spawn(|| panic::catch_unwind(run));
    let outcome = match worker {
        Ok(handle) => handle.join().unwrap_or_else(Err),
        // No thread could be started: run on the main thread instead.
        Err(_) => panic::catch_unwind(run),
    };
    match outcome {
        Ok(code) => code,
        Err(payload) => {
            let detail = payload
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_default();
            let diagnostic = Diagnostic::new(codes::E0003, "internal compiler error")
                .note(format!("the compiler failed: {detail}"))
                .help("this is a bug in wolf; please report it with the input that caused it");
            let mut stderr = StandardStream::stderr(ColorChoice::Never);
            let _ = wolf_diag::render_human(&SourceMap::new(), &[diagnostic], &mut stderr);
            ExitCode::from(EXIT_INTERNAL)
        }
    }
}

fn run() -> ExitCode {
    inject_internal_error();
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => return usage_error(&error),
    };
    match cli.command {
        Command::New(args) => new_project(&args),
        Command::Build(args) => {
            let target = match target(args.target, true) {
                Ok(t) => t,
                Err(code) => return code,
            };
            let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            finish(build::build(
                &cwd,
                out_dir(&cwd, args.out.as_deref()).as_deref(),
                build::Then::Nothing,
                target,
            ))
        }
        Command::Test(args) => {
            let target = match target(args.target, false) {
                Ok(t) => t,
                Err(code) => return code,
            };
            let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            let then = build::Then::Test {
                filter: args.filter.clone(),
            };
            finish(build::build(
                &cwd,
                out_dir(&cwd, args.out.as_deref()).as_deref(),
                then,
                target,
            ))
        }
        Command::Run(args) => {
            let target = match target(args.target, false) {
                Ok(t) => t,
                Err(code) => return code,
            };
            let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            let then = build::Then::Run {
                example: args.example.clone(),
                args: args.args.clone(),
            };
            finish(build::build(
                &cwd,
                out_dir(&cwd, args.out.as_deref()).as_deref(),
                then,
                target,
            ))
        }
        Command::Fmt(args) => {
            let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            match fmt::run(&cwd, &args.paths, args.check) {
                fmt::Outcome::Done => ExitCode::SUCCESS,
                fmt::Outcome::Unformatted(files) => {
                    fmt::print_unformatted(&files);
                    ExitCode::from(EXIT_ERRORS)
                }
                fmt::Outcome::Failed(result, code) => finish(build::Outcome::Failed(result, code)),
            }
        }
        Command::Check(args) => {
            let result = check::run(&args);
            let mut map = result.map;
            let mut diagnostics = result.diagnostics;
            wolf_diag::sort(&mut diagnostics);
            report(&mut map, &diagnostics, &args);
            if diagnostics.is_empty() {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(EXIT_ERRORS)
            }
        }
    }
}

/// The targets of `--target`; `all` only for `wolf build` (`build` is set).
fn target(target: Option<TargetArg>, build: bool) -> Result<build::Targets, ExitCode> {
    use wolf_project::Target;
    Ok(match target {
        None => build::Targets::Default,
        Some(TargetArg::Ts) => build::Targets::One(Target::Ts),
        Some(TargetArg::Go) => build::Targets::One(Target::Go),
        Some(TargetArg::Rust) => build::Targets::One(Target::Rust),
        Some(TargetArg::All) if build => build::Targets::All,
        Some(TargetArg::All) => {
            let _ = writeln!(
                io::stderr(),
                "error[{}]: `--target all` works only with `wolf build`; pass one target",
                codes::E0002.id()
            );
            return Err(ExitCode::from(EXIT_USAGE));
        }
    })
}

/// `--out` relative to the current directory: the tools run in other directories.
fn out_dir(cwd: &Path, out: Option<&Path>) -> Option<PathBuf> {
    out.map(|o| cwd.join(o))
}

fn finish(outcome: build::Outcome) -> ExitCode {
    match outcome {
        build::Outcome::Built => ExitCode::SUCCESS,
        build::Outcome::Ran(code) => ExitCode::from(u8::try_from(code).unwrap_or(1)),
        build::Outcome::Failed(result, code) => {
            let result = *result;
            let mut map = result.map;
            let mut diagnostics = result.diagnostics;
            wolf_diag::sort(&mut diagnostics);
            let args = CheckArgs {
                paths: Vec::new(),
                message_format: MessageFormat::Human,
                color: Color::Auto,
                emit: None,
            };
            report(&mut map, &diagnostics, &args);
            ExitCode::from(code)
        }
    }
}

fn new_project(args: &NewArgs) -> ExitCode {
    let kind = match args.kind {
        ProjectKind::Cli => wolf_project::Kind::Cli,
        ProjectKind::Lib => wolf_project::Kind::Lib,
    };
    match wolf_project::create(&PathBuf::from(&args.name), &args.name, kind) {
        Ok(()) => {
            let _ = writeln!(io::stdout(), "created project `{}`", args.name);
            ExitCode::SUCCESS
        }
        Err(message) => {
            let _ = writeln!(io::stderr(), "error[{}]: {message}", codes::E0002.id());
            ExitCode::from(EXIT_USAGE)
        }
    }
}

fn report(map: &mut SourceMap, diagnostics: &[Diagnostic], args: &CheckArgs) {
    match args.message_format {
        MessageFormat::Json => {
            let _ = io::stderr().write_all(wolf_diag::render_json(map, diagnostics).as_bytes());
        }
        MessageFormat::Human => {
            let choice = match args.color {
                Color::Always => ColorChoice::Always,
                Color::Never => ColorChoice::Never,
                Color::Auto if io::stderr().is_terminal() => ColorChoice::Auto,
                Color::Auto => ColorChoice::Never,
            };
            let mut stderr = StandardStream::stderr(choice);
            let _ = wolf_diag::render_human(map, diagnostics, &mut stderr);
            if !diagnostics.is_empty() {
                let count = diagnostics.len();
                let noun = if count == 1 { "error" } else { "errors" };
                let _ = writeln!(stderr, "error: aborting due to {count} {noun}");
            }
        }
    }
}

/// Reports a command-line error as E0002 (spec chapter 9).
fn usage_error(error: &clap::Error) -> ExitCode {
    use clap::error::ErrorKind;
    let mut stderr = io::stderr();
    match error.kind() {
        ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => {
            let _ = error.print();
            return ExitCode::SUCCESS;
        }
        ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand => {
            let help = error.render().to_string();
            let _ = write!(
                stderr,
                "error[{}]: no command given\n\n{help}",
                codes::E0002.id()
            );
        }
        _ => {
            let text = error.render().to_string();
            let text = text.strip_prefix("error: ").unwrap_or(&text);
            let _ = write!(stderr, "error[{}]: {}", codes::E0002.id(), text);
        }
    }
    ExitCode::from(EXIT_USAGE)
}

/// Debug builds only: `WOLF_TEST_INTERNAL_ERROR=1` simulates a compiler bug, so the E0003 path
/// can be tested. Release builds never panic here.
#[cfg(debug_assertions)]
#[allow(clippy::panic)]
fn inject_internal_error() {
    if std::env::var_os("WOLF_TEST_INTERNAL_ERROR").is_some() {
        panic!("simulated internal error");
    }
}

#[cfg(not(debug_assertions))]
fn inject_internal_error() {}
