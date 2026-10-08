//! Parser tests: the M0 examples, focused grammar rules, and arbitrary input.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use wolf_span::SourceMap;

struct Result {
    dump: String,
    codes: Vec<&'static str>,
    rendered: String,
}

fn parse(text: &str) -> Result {
    let mut map = SourceMap::new();
    let id = map.add("test.wolf", text.to_string());
    let lexed = wolf_lexer::lex(id, text);
    let parsed = wolf_parser::parse(id, text, &lexed.tokens);
    let mut diagnostics = lexed.diagnostics;
    diagnostics.extend(parsed.diagnostics);
    Result {
        dump: wolf_ast::dump(&parsed.file),
        codes: diagnostics.iter().map(|d| d.code.id()).collect(),
        rendered: wolf_diag::render_human_to_string(&map, &diagnostics),
    }
}

/// Wraps statements into a function so that only the statement grammar is tested.
fn body(stmts: &str) -> String {
    format!("module main\n\nfunctions {{\n    fn f() {{\n{stmts}\n    }}\n}}\n")
}

fn ok(text: &str) -> String {
    let result = parse(text);
    assert!(result.codes.is_empty(), "{}", result.rendered);
    result.dump
}

fn wolf_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            wolf_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "wolf") {
            out.push(path);
        }
    }
}

/// Acceptance criterion of M2: every example parses, and its tree is pinned by a snapshot.
#[test]
fn every_example_parses() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let mut files = Vec::new();
    wolf_files(&root, &mut files);
    files.sort();
    assert!(files.len() >= 13);
    for path in files {
        let text = fs::read_to_string(&path).unwrap();
        let result = parse(&text);
        assert!(
            result.codes.is_empty(),
            "{}: {}",
            path.display(),
            result.rendered
        );
        let name = path
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .replace(['/', '\\'], "__")
            .trim_end_matches(".wolf")
            .to_string();
        insta::with_settings!({ snapshot_suffix => name, prepend_module_to_snapshot => false }, {
            insta::assert_snapshot!("example", result.dump);
        });
    }
}

#[test]
fn precedence_follows_the_table() {
    let dump = ok(&body(
        "        let x = a + b * c << 1 & m == 0 || !d && -e.f()",
    ));
    assert!(
        dump.contains("(|| (== (& (<< (+ a (* b c)) 1) m) 0) (&& (! d) (neg (call (. e f)))))"),
        "{dump}"
    );
}

#[test]
fn binary_operators_are_left_associative() {
    let dump = ok(&body("        let x = a - b - c"));
    assert!(dump.contains("(- (- a b) c)"), "{dump}");
}

#[test]
fn newline_rules() {
    // A trailing operator continues the line; a leading dot continues the chain.
    let dump = ok(&body(
        "        let total = price +\n            tax\n        let names = users\n            .filter(is_active)\n            .map(name_of)",
    ));
    assert!(dump.contains("(let total (+ price tax))"), "{dump}");
    assert!(
        dump.contains("(call (. (call (. users filter) is_active) map) name_of)"),
        "{dump}"
    );
    // Inside parentheses newlines are ignored, but a block inside makes them significant again.
    let dump = ok(&body(
        "        print(\n            a,\n            b,\n        )\n        list.map(|x| {\n            let y = x * 2\n            y\n        })",
    ));
    assert!(dump.contains("(call print a b)"), "{dump}");
    assert!(dump.contains("(block (let y (* x 2)) y)"), "{dump}");
}

#[test]
fn nested_generics_split_shift_tokens() {
    let dump = ok("module main\n\nconfig {\n    x: Map<String, List<List<Int>>> = [:]\n}\n");
    assert!(dump.contains("(Map String (List (List Int)))"), "{dump}");
}

#[test]
fn struct_literals_and_conditions() {
    let dump = ok(&body(
        "        let p = Point { x: 1, y }\n        if p == (Point { x: 0, y: 0 }) {\n            print(\"origin\")\n        }\n        let s = Stack.<Int> { items: [] }",
    ));
    assert!(dump.contains("(let p (struct Point (x: 1) y))"), "{dump}");
    assert!(
        dump.contains("(paren (struct Point (x: 0) (y: 0)))"),
        "{dump}"
    );
    assert!(
        dump.contains("(struct Stack (.<> Int) (items: []))"),
        "{dump}"
    );
    // `match` with a path scrutinee is not a struct literal.
    ok(&body(
        "        match Color.Red {\n            _ => 1\n        }",
    ));
}

#[test]
fn one_line_match_and_arms() {
    let dump = ok(&body(
        "        let n = match x { Some(v) => v }\n        match y {\n            A | B => 1\n            C { x, .. } if x > 0 => 2\n            -1 => 3\n            None => return 0\n        }",
    ));
    assert!(dump.contains("(=> (Some v) v)"), "{dump}");
    assert!(dump.contains("(=> (| A B) 1)"), "{dump}");
    assert!(dump.contains("(=> (C {} x ..) (if (> x 0)) 2)"), "{dump}");
    assert!(dump.contains("(=> -1 3)"), "{dump}");
    assert!(dump.contains("(=> None (return 0))"), "{dump}");
}

#[test]
fn ranges_slices_and_loops() {
    let dump = ok(&body(
        "        for i in 0..=n {\n            print(s[1..i])\n        }\n        for u in mut users {\n            u.age += 1\n        }\n        let t = s[..3]",
    ));
    assert!(dump.contains("(..= 0 n)"), "{dump}");
    assert!(dump.contains("(slice.. s 1 i)"), "{dump}");
    assert!(dump.contains("(mut users)"), "{dump}");
    assert!(dump.contains("(slice.. s _ 3)"), "{dump}");
}

#[test]
fn closures_and_calls_with_modes() {
    let dump = ok(&body(
        "        sort(mut items)\n        save(own user)\n        let f = |a, b: Int| -> Int { a + b }\n        run(|| print(\"x\"))\n        let n = parse.<Int>(text)",
    ));
    assert!(dump.contains("(call sort (mut items))"), "{dump}");
    assert!(dump.contains("(call save (own user))"), "{dump}");
    assert!(
        dump.contains("(closure (params a (b Int)) (-> Int) (block (+ a b)))"),
        "{dump}"
    );
    assert!(dump.contains("(closure || (call print \"x\"))"), "{dump}");
    assert!(dump.contains("(call (.<> parse Int) text)"), "{dump}");
}

#[test]
fn return_with_and_without_value() {
    let dump = ok(&body(
        "        if a {\n            return\n        }\n        return b",
    ));
    assert!(dump.contains("(block return)"), "{dump}");
    assert!(dump.contains("(return b)"), "{dump}");
}

#[test]
fn contextual_keywords_are_identifiers_elsewhere() {
    ok(&body(
        "        let from = 1\n        let types = from + 1\n        let test = types\n        print(test.to_string())",
    ));
}

#[test]
fn declarations_of_every_kind() {
    let text = r#"//! Docs.
module lib

imports {
    pub std.fs as files
    users.model.{User, Role as Kind}
}

extern {
    go {
        pub fn upper(s: String) -> String from "strings.ToUpper"
        type Hasher from "hash.Hash"
    }
}

config {
    pub max: Int = 10 * 2
}

types {
    pub struct Box<T: Copyable> {
        pub value: T
    }
    enum Shape { Circle(Float), Rect { w: Float, h: Float } }
    type Ids = List<Int>
}

interfaces {
    interface Container<T> {
        fn get(self, i: Int) -> Option<T>
    }
}

impl {
    Box<T: Copyable>: Container<T> {
        fn get(self, i: Int) -> Option<T> {
            None
        }
    }
}

functions {
    intrinsic fn len(s: String) -> Int
    fn apply(f: fn(mut List<Int>) -> Bool) {}
}

tests {
    fn helper() -> Int {
        1
    }

    test "works" {
        assert(helper() == 1)
    }
}
"#;
    let dump = ok(text);
    insta::assert_snapshot!(dump);
}

#[test]
fn skeleton_errors() {
    let codes = |text: &str| parse(text).codes;
    assert_eq!(codes("functions {\n    fn f() {}\n}\n"), ["E0201"]);
    assert_eq!(codes("module main\n\ntypes {\n}\n"), ["E0204"]);
    assert_eq!(
        codes("module main\n\nfunctions {\n    fn f() {}\n}\n\ntypes {\n    type A = Int\n}\n"),
        ["E0202"]
    );
    assert_eq!(
        codes("module main\n\ntypes {\n    type A = Int\n}\n\ntypes {\n    type B = Int\n}\n"),
        ["E0203"]
    );
    assert_eq!(codes("module main\n\nfn f() {}\n"), ["E0207"]);
}

#[test]
fn parser_errors() {
    let codes = |stmts: &str| parse(&body(stmts)).codes;
    assert_eq!(codes("        let a = 1 < 2 < 3"), ["E0305"]);
    assert_eq!(codes("        let a = b = 1"), ["E0308"]);
    assert_eq!(codes("        let a = 1 let b = 2"), ["E0313"]);
    assert_eq!(codes("        let r = 0..1"), ["E0307"]);
    assert_eq!(codes("        let a = 1\n            + 2"), ["E0303"]);
    assert_eq!(
        codes("        if a {\n        }\n        else {\n        }"),
        ["E0304"]
    );
    assert_eq!(codes("        let n = parse<Int>(s)"), ["E0311"]);
    assert_eq!(codes("        let f = |x: Int| -> Int x"), ["E0310"]);
    assert_eq!(codes("        if p == P { x: 1 } {\n        }"), ["E0306"]);
    assert_eq!(codes("        let a = (1, 2)"), ["E0301"]);
}

/// Arbitrary token soup through lexer, parser and both renderers: no panic, no hang.
#[test]
fn arbitrary_input_never_panics() {
    const WORDS: &[&str] = &[
        "module",
        "main",
        "functions",
        "types",
        "impl",
        "extern",
        "fn",
        "struct",
        "enum",
        "let",
        "var",
        "if",
        "else",
        "match",
        "for",
        "in",
        "while",
        "return",
        "mut",
        "own",
        "self",
        "x",
        "Point",
        "test",
        "pub",
        "{",
        "}",
        "(",
        ")",
        "[",
        "]",
        "<",
        ">",
        ">>",
        ",",
        ".",
        "..",
        ":",
        "=",
        "==",
        "=>",
        "->",
        "+",
        "-",
        "|",
        "||",
        "?",
        "_",
        "1",
        "2.5",
        "\"s\"",
        "\n",
        "\n",
        "///d\n",
        "//!d\n",
        "@",
        "'",
        "/*",
        "*/",
    ];
    let mut state = 0x2545_f491_4f6c_dd1du64;
    let mut next = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let started = Instant::now();
    for _ in 0..2000 {
        let len = (next() % 80) as usize;
        let mut text = String::new();
        for _ in 0..len {
            text.push_str(WORDS[(next() as usize) % WORDS.len()]);
            text.push(' ');
        }
        let _ = parse(&text);
    }
    assert!(
        started.elapsed() < Duration::from_secs(60),
        "parsing is too slow"
    );
}
