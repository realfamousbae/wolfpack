//! The `wolf` command-line tool.
//!
//! `wolf check` lexes and parses `.wolf` files and reports their errors (milestone M2).
//! Until project discovery arrives in M3, `wolf check` takes files or directories directly.

mod check;

use std::io::{self, IsTerminal, Write};
use std::panic;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use codespan_reporting::term::termcolor::{ColorChoice, StandardStream};
use wolf_diag::{Diagnostic, codes};
use wolf_span::SourceMap;

/// Exit codes of `wolf` (spec chapter 9, section 4).
const EXIT_ERRORS: u8 = 1;
const EXIT_USAGE: u8 = 2;
const EXIT_INTERNAL: u8 = 3;

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
