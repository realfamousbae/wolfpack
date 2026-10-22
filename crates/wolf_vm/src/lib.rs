//! The virtual machine of wolfpack (spec chapter 10): `wolf run --vm` and `wolf test --vm`.
//!
//! The IR of a checked program is compiled into a bytecode of its own ([`code`]) and run by a
//! stack machine ([`vm`]), with the intrinsic functions of std written in Rust. The VM behaves
//! like the three targets: the same output, the same panic messages, the same exit codes.

mod code;
mod compile;
mod intrinsics;
mod text;
mod value;
mod vm;

use std::io::Write as _;

pub use code::Chunk;
pub use compile::{Func, Image, compile};

use vm::Vm;

/// Why a program stops early.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Stop {
    /// A panic with its whole message, such as `panic: integer overflow`.
    Panic(String),
    /// `std.process.exit(code)`.
    Exit(i32),
    /// A bug of the VM or the compiler: a broken invariant.
    Internal(String),
}

impl Stop {
    /// A panic raised by the language itself, without a location.
    pub fn fail(message: impl std::fmt::Display) -> Stop {
        Stop::Panic(format!("panic: {message}"))
    }
}

/// An error of `std.fs`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FsError {
    /// The file does not exist.
    NotFound,
    /// The file cannot be read or written.
    PermissionDenied,
    /// Another error, with the text of the system.
    Other(String),
}

/// What a program can reach outside the VM: standard streams, arguments, the environment and
/// files. The command line uses [`StdIo`]; tests and the playground can use their own.
pub trait Io {
    /// Writes to standard output.
    fn out(&mut self, text: &str);
    /// Writes to standard error.
    fn err(&mut self, text: &str);
    /// Reads a line of standard input, without its terminator; `None` at the end.
    fn read_line(&mut self) -> Option<String>;
    /// The arguments of the program.
    fn args(&self) -> Vec<String>;
    /// A variable of the environment.
    fn env(&self, name: &str) -> Option<String>;
    /// Reads a text file.
    fn read_text(&mut self, path: &str) -> Result<String, FsError>;
    /// Writes a text file.
    fn write_text(&mut self, path: &str, content: &str) -> Result<(), FsError>;
    /// Whether a path exists.
    fn exists(&self, path: &str) -> bool;
    /// Writes out what is buffered.
    fn flush(&mut self) {}
}

/// The process's own streams, arguments, environment and files.
pub struct StdIo {
    out: std::io::BufWriter<std::io::Stdout>,
    args: Vec<String>,
}

impl StdIo {
    /// Standard streams, with these arguments for the program.
    pub fn new(args: Vec<String>) -> StdIo {
        StdIo {
            out: std::io::BufWriter::new(std::io::stdout()),
            args,
        }
    }
}

fn fs_error(error: &std::io::Error) -> FsError {
    match error.kind() {
        std::io::ErrorKind::NotFound => FsError::NotFound,
        std::io::ErrorKind::PermissionDenied => FsError::PermissionDenied,
        _ => FsError::Other(error.to_string()),
    }
}

impl Io for StdIo {
    fn out(&mut self, text: &str) {
        let _ = self.out.write_all(text.as_bytes());
    }

    fn err(&mut self, text: &str) {
        let _ = self.out.flush();
        let _ = std::io::stderr().write_all(text.as_bytes());
    }

    fn read_line(&mut self) -> Option<String> {
        let _ = self.out.flush();
        let mut line = String::new();
        match std::io::stdin().read_line(&mut line) {
            Ok(0) | Err(_) => None,
            Ok(_) => {
                let line = line.strip_suffix('\n').unwrap_or(&line);
                Some(line.strip_suffix('\r').unwrap_or(line).to_string())
            }
        }
    }

    fn args(&self) -> Vec<String> {
        self.args.clone()
    }

    fn env(&self, name: &str) -> Option<String> {
        std::env::var(name).ok()
    }

    fn read_text(&mut self, path: &str) -> Result<String, FsError> {
        std::fs::read_to_string(path).map_err(|e| fs_error(&e))
    }

    fn write_text(&mut self, path: &str, content: &str) -> Result<(), FsError> {
        std::fs::write(path, content).map_err(|e| fs_error(&e))
    }

    fn exists(&self, path: &str) -> bool {
        std::path::Path::new(path).exists()
    }

    fn flush(&mut self) {
        let _ = self.out.flush();
    }
}

/// Runs the `main` of a module (the entry module or an example) and returns the exit code of
/// the program: 0, the code of `exit`, 1 for an `Err` from `main`, 101 for a panic. An `Err` is
/// a bug of the VM, with its description.
pub fn run_main(program: &wolf_ir::Program, module: usize, io: &mut dyn Io) -> Result<i32, String> {
    let image = compile(program)?;
    let (func, takes_args, result) = image
        .main_of(module)
        .ok_or_else(|| "the module has no `main`".to_string())?;
    let mut vm = Vm::new(&image, io);
    let outcome = vm.main(func, takes_args, result);
    let code = match outcome {
        Ok(None) => 0,
        Ok(Some(message)) => {
            vm.io().err(&format!("error: {message}\n"));
            1
        }
        Err(Stop::Panic(message)) => {
            vm.io().err(&format!("{message}\n"));
            101
        }
        Err(Stop::Exit(code)) => code,
        Err(Stop::Internal(e)) => {
            vm.io().flush();
            return Err(e);
        }
    };
    vm.io().flush();
    Ok(code)
}

/// Runs the tests whose name contains `filter` and prints the report of spec chapter 8,
/// section 2, rule 7. It returns 0 if every test passed and 1 otherwise.
pub fn run_tests(
    program: &wolf_ir::Program,
    filter: Option<&str>,
    io: &mut dyn Io,
) -> Result<i32, String> {
    let image = compile(program)?;
    if image.tests.is_empty() {
        io.out("test result: ok. 0 passed; 0 failed\n");
        io.flush();
        return Ok(0);
    }
    let mut passed = 0;
    let mut failures = Vec::new();
    for t in image
        .tests
        .iter()
        .filter(|t| filter.is_none_or(|f| t.name.contains(f)))
    {
        let outcome = {
            let mut quiet = Quiet(&mut *io);
            let mut vm = Vm::new(&image, &mut quiet);
            vm.test(t.func)
        };
        match outcome {
            Ok(()) => {
                passed += 1;
                io.out(&format!("test {} ... ok\n", t.name));
            }
            Err(Stop::Internal(e)) => {
                io.flush();
                return Err(e);
            }
            Err(stop) => {
                io.out(&format!("test {} ... FAILED\n", t.name));
                let message = match stop {
                    Stop::Panic(m) => m,
                    Stop::Exit(c) => format!("the test exited with code {c}"),
                    Stop::Internal(m) => m,
                };
                failures.push((t.name.clone(), message));
            }
        }
    }
    if !failures.is_empty() {
        io.out("\nfailures:\n");
        for (name, message) in &failures {
            io.out(&format!("\n  {name}\n    {message}\n"));
        }
    }
    let status = if failures.is_empty() { "ok" } else { "FAILED" };
    io.out(&format!(
        "\ntest result: {status}. {passed} passed; {} failed\n",
        failures.len()
    ));
    io.flush();
    Ok(if failures.is_empty() { 0 } else { 1 })
}

/// The streams of a test: what the test prints is not shown, as on the targets.
struct Quiet<'a>(&'a mut dyn Io);

impl Io for Quiet<'_> {
    fn out(&mut self, _: &str) {}
    fn err(&mut self, _: &str) {}
    fn read_line(&mut self) -> Option<String> {
        self.0.read_line()
    }
    fn args(&self) -> Vec<String> {
        Vec::new()
    }
    fn env(&self, name: &str) -> Option<String> {
        self.0.env(name)
    }
    fn read_text(&mut self, path: &str) -> Result<String, FsError> {
        self.0.read_text(path)
    }
    fn write_text(&mut self, path: &str, content: &str) -> Result<(), FsError> {
        self.0.write_text(path, content)
    }
    fn exists(&self, path: &str) -> bool {
        self.0.exists(path)
    }
}
