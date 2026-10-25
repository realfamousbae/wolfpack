//! The `wolf` command-line tool.
//!
//! `wolf check` checks a project, `wolf build` generates the target project in `out/<target>/`,
//! `wolf run` builds and runs it with the tools of the target, `wolf new` creates a project,
//! `wolf explain` describes an error code, and `wolf setup` installs the tools of the targets.

mod build;
mod check;
mod fmt;
mod lsp;
mod native_modules;
mod setup;
mod tools;
mod uninstall;
mod update;

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

/// The wolfpack compiler.
#[derive(Parser)]
#[command(name = "wolf", version, about = "The wolfpack compiler")]
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
    /// Show a longer description of an error code, with a wrong and a fixed example.
    Explain(ExplainArgs),
    /// Install the tools of the targets (Node.js, TypeScript, Go, Rust) for `wolf run`.
    Setup(SetupArgs),
    /// Serve an editor: the language server, on standard input and output.
    Lsp,
    /// Manage `wolf` itself.
    #[command(name = "self", subcommand)]
    SelfCmd(SelfCommand),
}

/// The subcommands of `wolf self`.
#[derive(Subcommand)]
enum SelfCommand {
    /// Replace `wolf` with the latest release, or with `--version`.
    Update(UpdateArgs),
    /// Remove `wolf`, the `PATH` lines its installer added and, after a question, the tools of
    /// the targets in the wolfpack home.
    Uninstall(UninstallArgs),
}

/// Options of `wolf self uninstall`.
#[derive(clap::Args)]
struct UninstallArgs {
    /// Remove the wolfpack home too, without asking.
    #[arg(long)]
    yes: bool,
}

/// Options of `wolf self update`.
#[derive(clap::Args)]
struct UpdateArgs {
    /// Install this release (for example `0.2.0`) instead of the latest one.
    #[arg(long)]
    version: Option<String>,

    /// Install nothing; exit with code 1 when a newer release exists.
    #[arg(long)]
    check: bool,

    /// Remember the latest release for the question at start (run in the background).
    #[arg(long, hide = true)]
    refresh_cache: bool,
}

/// Options of `wolf setup`.
#[derive(clap::Args)]
struct SetupArgs {
    /// Install only the tools of this target.
    #[arg(long, value_enum)]
    target: Option<TargetArg>,

    /// Install nothing; list the tools, where they are found and their versions.
    #[arg(long)]
    check: bool,
}

/// Options of `wolf explain`.
#[derive(clap::Args)]
struct ExplainArgs {
    /// The error code, for example `E0501`.
    code: String,
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

    /// Run in the virtual machine of `wolf`, without the tools of a target.
    #[arg(long)]
    vm: bool,

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

    /// Run in the virtual machine of `wolf`, without the tools of a target.
    #[arg(long)]
    vm: bool,

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

    /// Print what the compiler read from the code of the native modules the project imports, or
    /// of one native module such as `native.text.slug`.
    #[arg(long, value_name = "MODULE", num_args = 0..=1, default_missing_value = "")]
    pub(crate) native: Option<String>,

    /// Print the bytecode that the VM runs for the functions of `src/`, or for one function
    /// such as `area`, `Shape.area` or `shapes.area`.
    #[arg(long, value_name = "FUNCTION", num_args = 0..=1, default_missing_value = "")]
    pub(crate) bytecode: Option<String>,
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
    update::clean_old();
    if !matches!(
        cli.command,
        Command::SelfCmd(_) | Command::Explain(_) | Command::Lsp
    ) {
        if let Some(code) = update::at_start() {
            return code;
        }
    }
    match cli.command {
        Command::SelfCmd(SelfCommand::Update(args)) => self_update(&args),
        Command::SelfCmd(SelfCommand::Uninstall(args)) => match uninstall::uninstall(args.yes) {
            Ok(()) => ExitCode::SUCCESS,
            Err(d) => {
                render_plain(&[*d]);
                ExitCode::from(EXIT_ERRORS)
            }
        },
        Command::New(args) => new_project(&args),
        Command::Build(args) => {
            let target = match target(args.target, true, false) {
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
            let target = match target(args.target, false, args.vm) {
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
            let target = match target(args.target, false, args.vm) {
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
        Command::Explain(args) => explain(&args.code),
        Command::Lsp => ExitCode::from(lsp::run()),
        Command::Setup(args) => setup(&args),
        Command::Check(args) => {
            let result = check::run(&args);
            // The listings of `--native` and `--bytecode` go to stdout, the diagnostics to stderr
            // (spec chapter 8, section 8b; chapter 10, section 1a).
            for (i, listing) in result.listings.iter().enumerate() {
                if i > 0 {
                    println!();
                }
                print!("{listing}");
            }
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

/// The targets of `--target`; `all` only for `wolf build` (`build` is set). `--vm` excludes
/// `--target` (spec chapter 10, section 1).
fn target(target: Option<TargetArg>, build: bool, vm: bool) -> Result<build::Targets, ExitCode> {
    use wolf_project::Target;
    if vm {
        if target.is_some() {
            let _ = writeln!(
                io::stderr(),
                "error[{}]: `--vm` and `--target` exclude each other; pass one of them",
                codes::E0002.id()
            );
            return Err(ExitCode::from(EXIT_USAGE));
        }
        return Ok(build::Targets::Vm);
    }
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
                native: None,
                bytecode: None,
            };
            report(&mut map, &diagnostics, &args);
            ExitCode::from(code)
        }
    }
}

/// `wolf setup` (spec chapter 8, section 2a).
fn setup(args: &SetupArgs) -> ExitCode {
    use wolf_project::Target;
    let targets: Vec<Target> = match args.target {
        Some(TargetArg::Ts) => vec![Target::Ts],
        Some(TargetArg::Go) => vec![Target::Go],
        Some(TargetArg::Rust) => vec![Target::Rust],
        Some(TargetArg::All) | None => Target::ALL.to_vec(),
    };
    if args.check {
        return if setup::check(&targets) {
            ExitCode::SUCCESS
        } else {
            ExitCode::from(EXIT_ERRORS)
        };
    }
    let diagnostics = setup::install(&targets);
    if diagnostics.is_empty() {
        return ExitCode::SUCCESS;
    }
    let mut stderr = StandardStream::stderr(if io::stderr().is_terminal() {
        ColorChoice::Auto
    } else {
        ColorChoice::Never
    });
    let _ = wolf_diag::render_human(&SourceMap::new(), &diagnostics, &mut stderr);
    ExitCode::from(EXIT_ERRORS)
}

/// `wolf self update` (spec chapter 8, section 2b).
fn self_update(args: &UpdateArgs) -> ExitCode {
    if args.refresh_cache {
        update::refresh();
        return ExitCode::SUCCESS;
    }
    let current = update::Version::current();
    if args.check {
        return match update::latest() {
            Ok(latest) => {
                let _ = writeln!(io::stdout(), "wolf {current}, latest release {latest}");
                if latest > current {
                    ExitCode::from(EXIT_ERRORS)
                } else {
                    ExitCode::SUCCESS
                }
            }
            Err(e) => {
                render_plain(&[Diagnostic::new(
                    codes::E0805,
                    format!("cannot find the latest release: {e}"),
                )]);
                ExitCode::from(EXIT_ERRORS)
            }
        };
    }
    match update::update(args.version.as_deref()) {
        Ok(Some(_)) => ExitCode::SUCCESS,
        Ok(None) => {
            let _ = writeln!(io::stdout(), "wolf {current} is the latest release");
            ExitCode::SUCCESS
        }
        Err(d) => {
            render_plain(&[*d]);
            ExitCode::from(EXIT_ERRORS)
        }
    }
}

/// Diagnostics without a source, on standard error.
pub(crate) fn render_plain(diagnostics: &[Diagnostic]) {
    let mut stderr = StandardStream::stderr(if io::stderr().is_terminal() {
        ColorChoice::Auto
    } else {
        ColorChoice::Never
    });
    let _ = wolf_diag::render_human(&SourceMap::new(), diagnostics, &mut stderr);
}

/// `wolf explain <code>` (spec chapter 9, section 2, rule 8).
fn explain(code: &str) -> ExitCode {
    use wolf_diag::explain::Explanation;
    match wolf_diag::explain::explain(code) {
        Explanation::Text(_, text) => {
            let _ = io::stdout().write_all(text.as_bytes());
            ExitCode::SUCCESS
        }
        Explanation::Retired(id) => {
            let _ = writeln!(io::stdout(), "{id} is no longer used by the compiler");
            ExitCode::SUCCESS
        }
        Explanation::Unknown => {
            let _ = writeln!(
                io::stderr(),
                "error[{}]: there is no error code `{code}`",
                codes::E0002.id()
            );
            ExitCode::from(EXIT_USAGE)
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
