//! The bytecode of small functions (snapshots) and programs run in memory, with their output.

// Test helpers may fail loudly.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use wolf_span::SourceMap;
use wolf_vm::{FsError, Func, Io};

const MANIFEST: &str = "[package]\nname = \"vm\"\nversion = \"0.1.0\"\nkind = \"cli\"\n";

/// The IR of a program of one module.
fn program(source: &str) -> wolf_ir::Program {
    program_with(source, false)
}

/// The IR of a program of one module; `debug` asks for a debug build.
fn program_with(source: &str, debug: bool) -> wolf_ir::Program {
    let mut map = SourceMap::new();
    let mut diagnostics = Vec::new();
    let id = map.add("wolfpack.toml", MANIFEST.to_string());
    let (manifest, found) = wolf_project::parse_manifest(id, MANIFEST);
    assert!(found.is_empty());
    let manifest = manifest.unwrap();
    let mut parsed = wolf_driver::std_modules(&mut map, &mut diagnostics);
    let id = map.add("src/main.wolf", source.to_string());
    let tree = wolf_driver::parse(&map, id, &mut diagnostics);
    parsed.push((
        vec!["main".to_string()],
        wolf_resolve::Role::Entry,
        id,
        tree,
    ));
    let analysis = wolf_driver::analyze(
        &map,
        &manifest,
        parsed,
        &wolf_resolve::NativeNames::new(),
        &mut diagnostics,
    );
    assert!(
        diagnostics.is_empty(),
        "{}",
        wolf_diag::render_human_to_string(&map, &diagnostics)
    );
    let inputs = analysis.inputs();
    let mut meta = wolf_driver::meta(&manifest, false);
    meta.debug = debug;
    wolf_ir::lower(&inputs, &analysis.resolved, &analysis.typed, &map, &meta).unwrap()
}

/// The listing of the function `name` of a program.
fn listing(source: &str, name: &str) -> String {
    let image = wolf_vm::compile(&program(source)).unwrap();
    let func = image
        .funcs
        .iter()
        .position(|f| matches!(f, Func::Code(c) if c.name == name))
        .unwrap();
    image.listing(u32::try_from(func).unwrap()).unwrap()
}

#[derive(Default)]
struct Captured {
    out: String,
    err: String,
    input: Vec<String>,
}

impl Io for Captured {
    fn out(&mut self, text: &str) {
        self.out.push_str(text);
    }
    fn err(&mut self, text: &str) {
        self.err.push_str(text);
    }
    fn read_line(&mut self) -> Option<String> {
        if self.input.is_empty() {
            None
        } else {
            Some(self.input.remove(0))
        }
    }
    fn args(&self) -> Vec<String> {
        vec!["one".to_string()]
    }
    fn env(&self, _: &str) -> Option<String> {
        None
    }
    fn read_text(&mut self, _: &str) -> Result<String, FsError> {
        Err(FsError::NotFound)
    }
    fn write_text(&mut self, _: &str, _: &str) -> Result<(), FsError> {
        Err(FsError::Other("read-only".to_string()))
    }
    fn exists(&self, _: &str) -> bool {
        false
    }
}

/// Runs a program and gives its exit code, output and error output.
fn run(source: &str) -> (i32, String, String) {
    run_with(source, false)
}

/// Runs a program, built for debugging if `debug`.
fn run_with(source: &str, debug: bool) -> (i32, String, String) {
    let p = program_with(source, debug);
    let entry = p
        .modules
        .iter()
        .position(|m| m.role == wolf_ir::ModuleRole::Entry)
        .unwrap();
    let mut io = Captured::default();
    let code = wolf_vm::run_main(&p, entry, &mut io).unwrap();
    (code, io.out, io.err)
}

#[test]
fn bytecode_of_a_loop_and_a_call() {
    let source = r#"module main

main {
    print(sum(4).to_string())
}

functions {
    fn sum(n: Int) -> Int {
        var total = 0
        for i in 0..n {
            total += i
        }
        total
    }
}
"#;
    insta::assert_snapshot!(listing(source, "main.sum"));
}

#[test]
fn bytecode_of_a_match_and_a_mut_parameter() {
    let source = r#"module main

types {
    enum Shape {
        Circle(Float)
        Square(Float)
    }
}

main {
    var s = Shape.Circle(1.0)
    grow(mut s)
}

functions {
    fn grow(mut s: Shape) {
        match mut s {
            Shape.Circle(r) => {
                r *= 2.0
            }
            Shape.Square(_) => {}
        }
    }
}
"#;
    insta::assert_snapshot!(listing(source, "main.grow"));
}

#[test]
fn programs_print_and_exit_like_the_targets() {
    let (code, out, err) = run(r#"module main

main(args: List<String>) -> Result<Void, Error> {
    print(f"{args.len()} {args[0]}")
    let n = "x".parse_int()?
    print(n.to_string())
    Ok(())
}
"#);
    assert_eq!(
        (code, out.as_str(), err.as_str()),
        (1, "1 one\n", "error: cannot parse x\n")
    );
    let (code, out, err) = run(r#"module main

main {
    let items = [1, 2]
    print("before")
    print(items[2].to_string())
}
"#);
    assert_eq!(
        (code, out.as_str(), err.as_str()),
        (
            101,
            "before\n",
            "panic: index 2 out of range for length 2\n"
        )
    );
}

#[test]
fn overflow_and_division_panic() {
    for (expr, message) in [
        ("9223372036854775807 + 1", "integer overflow"),
        ("-9223372036854775807 - 2", "integer overflow"),
        ("1 / (2 - 2)", "division by zero"),
        ("1 << 64", "shift amount out of range"),
        ("(-9223372036854775807 - 1).abs()", "integer overflow"),
        ("2.pow(63)", "integer overflow"),
    ] {
        let (code, _, err) = run(&format!(
            "module main\n\nmain {{\n    print(({expr}).to_string())\n}}\n"
        ));
        assert_eq!((code, err), (101, format!("panic: {message}\n")), "{expr}");
    }
}

/// In a debug build the panics of the language print the location of the operation: the
/// operator after the left operand with its parentheses, the `[`, or the start of the call.
#[test]
fn debug_builds_locate_the_panics_of_the_language() {
    // The expression, the text at its location and the message.
    for (expr, at, message) in [
        ("(big + 0) * 2", "* 2", "integer overflow"),
        ("big /* (+) */ + 1", "+ 1", "integer overflow"),
        ("-(0 - big - 1)", "-(", "integer overflow"),
        ("[big, 2]  [7]", "[7]", "index 7 out of range for length 2"),
        (
            "[big, 2][3..1].len()",
            "[3",
            "slice 3..1 out of range for length 2",
        ),
        ("big.pow(2)", "big", "integer overflow"),
        ("[big].map(|x| x * x).len()", "* x", "integer overflow"),
    ] {
        let source = format!(
            "module main\n\nmain {{\n    let big = 9223372036854775807\n    print(({expr}).to_string())\n}}\n"
        );
        // `    print((` is 11 characters.
        let column = 12 + expr.find(at).unwrap();
        let (code, _, err) = run_with(&source, true);
        let expected = format!("panic at src/main.wolf:5:{column}: {message}\n");
        assert_eq!((code, err), (101, expected), "{expr}");
        let (code, _, err) = run(&source);
        assert_eq!((code, err), (101, format!("panic: {message}\n")), "{expr}");
    }
}

/// Stack overflow keeps no location in a debug build, and a panic keeps its own.
#[test]
fn debug_builds_keep_other_panics() {
    let source = "module main\n\nmain {\n    print(down(1).to_string())\n}\n\nfunctions {\n    fn down(n: Int) -> Int {\n        down(n + 1)\n    }\n}\n";
    let (code, _, err) = run_with(source, true);
    assert_eq!((code, err.as_str()), (101, "panic: stack overflow\n"));
    let source = "module main\n\nmain {\n    panic(\"stop\")\n}\n";
    let (code, _, err) = run_with(source, true);
    assert_eq!(
        (code, err.as_str()),
        (101, "panic at src/main.wolf:4:5: stop\n")
    );
}
