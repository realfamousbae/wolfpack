//! The bytecode of small functions (snapshots), programs run in memory with their output, and
//! bytecode files (spec chapter 10, section 1b) written, read back and damaged.

// Test helpers may fail loudly.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use wolf_span::SourceMap;
use wolf_vm::{FsError, Func, Io};

const MANIFEST: &str = "[package]\nname = \"vm\"\nversion = \"0.1.0\"\nkind = \"cli\"\n";

/// The IR of a program of one module.
fn program(source: &str) -> wolf_ir::Program {
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
    let meta = wolf_driver::meta(&manifest, false);
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
    let p = program(source);
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

/// A program with most kinds of instructions and constants: config values of every kind, a
/// struct update, an interface call, a match on variants, a map, closures, a string slice, the
/// arguments and an `Err` from `main`.
const RICH: &str = r#"module main

imports {
    std.map.{get}
}

config {
    limit: Int = 3
    name: String = "wolf"
    loud: Bool = true
    ratio: Float = 0.5
}

types {
    struct Point {
        x: Int
        y: Int
    }

    enum Shape {
        Circle(Float)
        Rect(Point)
    }
}

interfaces {
    interface Sized {
        fn size(self) -> Int
    }
}

impl {
    Point: Sized {
        fn size(self) -> Int {
            self.x * self.y
        }
    }
}

main(args: List<String>) -> Result<Void, Error> {
    let p = Point { x: 2, y: 5 }
    let q = Point { y: 7, ..p.copy() }
    let things: List<Sized> = [p, q.copy()]
    var total = 0
    for t in things {
        total += t.size()
    }
    print(f"{name} {total} {limit} {loud} {ratio}")
    let shapes = [Shape.Circle(1.5), Shape.Rect(q)]
    for s in shapes {
        match s {
            Shape.Circle(r) => print(f"circle {r * 2.0}")
            Shape.Rect(pt) => print(f"rect {pt.x}")
        }
    }
    var counts: Map<String, Int> = ["a": 1]
    counts["a"] += 1
    let found = get(counts, "a")
    print(f"{found} {counts}")
    let add = |n: Int| |x: Int| x + n
    let add4 = add(4)
    let word = "wolfpack"
    print(f"{add4(5)} {word[0..4]} {args}")
    let n = "x".parse_int()?
    print(n.to_string())
    Ok(())
}
"#;

const VERSION: &str = "1.2.3";

/// The bytecode file of a program of one module.
fn file(source: &str) -> Vec<u8> {
    let p = program(source);
    let entry = p
        .modules
        .iter()
        .position(|m| m.role == wolf_ir::ModuleRole::Entry)
        .unwrap();
    wolf_vm::write_file(&p, entry, VERSION).unwrap()
}

#[test]
fn a_bytecode_file_runs_like_the_program() {
    let bytes = file(RICH);
    assert!(bytes.starts_with(b"wolfpack bytecode 1.2.3\n"));
    assert_eq!(
        bytes,
        file(RICH),
        "a build writes the same bytes every time"
    );
    let exe = wolf_vm::read_file(&bytes, VERSION).unwrap();
    let mut io = Captured::default();
    let code = exe.run(&mut io).unwrap();
    assert_eq!(run(RICH), (code, io.out.clone(), io.err.clone()));
    assert_eq!(
        (code, io.out.as_str(), io.err.as_str()),
        (
            1,
            "wolf 24 3 true 0.5\ncircle 3.0\nrect 2\nSome(2) [\"a\": 2]\n9 wolf [\"one\"]\n",
            "error: cannot parse x\n"
        )
    );
}

#[test]
fn a_file_of_another_version_is_not_read() {
    let bytes = file(RICH);
    assert_eq!(
        wolf_vm::read_file(&bytes, "1.2.4").err(),
        Some(wolf_vm::ReadError::OtherVersion(VERSION.to_string()))
    );
    assert_eq!(
        wolf_vm::read_file(b"fn main() {}\n", VERSION).err(),
        Some(wolf_vm::ReadError::NotBytecode)
    );
}

/// The checksum of a file, as the format describes it: FNV-1a of 64 bits.
fn checksum(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        hash ^= u64::from(*b);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// About 3000 positions of a file of `len` bytes: every one of the first 64 (the header and the
/// checksum), then evenly spread.
fn positions(len: usize) -> Vec<usize> {
    let step = (len / 3000).max(1);
    (0..len.min(64)).chain((64..len).step_by(step)).collect()
}

/// The changes made to one byte.
fn changes(b: u8) -> [u8; 4] {
    [b ^ 0xff, b ^ 0x01, 0x00, 0xff]
}

/// Any changed byte and any cut is found, and reading never panics.
#[test]
fn a_damaged_file_is_an_error() {
    let bytes = file(RICH);
    for at in positions(bytes.len()) {
        for new in changes(bytes[at]) {
            let mut damaged = bytes.clone();
            damaged[at] = new;
            if damaged != bytes {
                assert!(
                    wolf_vm::read_file(&damaged, VERSION).is_err(),
                    "byte {at} set to {new}"
                );
            }
        }
    }
    for cut in positions(bytes.len()) {
        assert!(
            wolf_vm::read_file(&bytes[..cut], VERSION).is_err(),
            "cut at {cut}"
        );
    }
}

/// Damage that the checksum cannot see, because the file was changed and its checksum written
/// again, is found by the checks of the image or reads as another program: reading never panics
/// and the VM is never given an image it cannot run.
#[test]
fn a_damaged_file_with_a_right_checksum_never_panics() {
    let bytes = file(RICH);
    let header = bytes.iter().position(|b| *b == b'\n').unwrap() + 1;
    let body = &bytes[header + 8..];
    let seal = |body: &[u8]| {
        let mut out = bytes[..header].to_vec();
        out.extend(checksum(body).to_le_bytes());
        out.extend(body);
        out
    };
    assert!(wolf_vm::read_file(&seal(body), VERSION).is_ok());
    let mut rejected = 0;
    let mut tried = 0;
    for at in positions(body.len()) {
        for new in changes(body[at]) {
            let mut damaged = body.to_vec();
            damaged[at] = new;
            tried += 1;
            if wolf_vm::read_file(&seal(&damaged), VERSION).is_err() {
                rejected += 1;
            }
        }
    }
    // Most changes break the structure; the others change a number or a text that may be right.
    assert!(rejected * 2 > tried, "{rejected} of {tried} rejected");
    for cut in positions(body.len()) {
        assert!(
            wolf_vm::read_file(&seal(&body[..cut]), VERSION).is_err(),
            "cut at {cut}"
        );
    }
}
