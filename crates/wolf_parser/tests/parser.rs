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

/// Conditions without `let` keep the tree of the precedence table; with a `let` they are chains
/// whose items end at `&&` (chapter 3, section 5, rule 14a).
#[test]
fn conditions_and_chains() {
    let dump = ok(&body(
        "        if a && b || c && !d {\n            f()\n        }\n        while x < 1 && y || z {\n            f()\n        }",
    ));
    assert!(dump.contains("(if (|| (&& a b) (&& c (! d)))"), "{dump}");
    assert!(dump.contains("(while (|| (&& (< x 1) y) z)"), "{dump}");
    let dump = ok(&body(
        "        if let Some(x) = a {\n            f()\n        }\n        if ready && let Some(x) = mut a && x > 0 && let B(y) = own x.b {\n            f()\n        } else {\n            g()\n        }\n        while let Some(t) = s.pop() && t != 0 {\n            f()\n        }",
    ));
    assert!(dump.contains("(if let (Some x) a"), "{dump}");
    let chain = "if let\n          ready\n          (let (Some x) (mut a))\n          (> x 0)\n          (let (B y) (own (. x b)))\n";
    assert!(dump.contains(chain), "{dump}");
    assert!(
        dump.contains("(while let (let (Some t) (call (. s pop))) (!= t 0)"),
        "{dump}"
    );
}

/// The head of an `impl` block may give type arguments; name resolution tells them from
/// parameters, and a type that is not a single name takes no bounds (chapter 4, section 8.3,
/// rule 8; D4.21).
#[test]
fn impl_heads_with_type_arguments() {
    let dump = ok(
        "module main\n\nimpl {\n    Stack<Int> {\n        fn f(self) {}\n    }\n    Pair<String, T: Display> {\n        fn g(self) {}\n    }\n    Stack<List<Int>> {\n        fn h(self) {}\n    }\n    Stack<fn(Int) -> Int>: Shown {\n        fn s(self) {}\n    }\n}\n",
    );
    assert!(dump.contains("(impl Stack (generics Int)"), "{dump}");
    assert!(
        dump.contains("(impl Pair (generics String (T Display))"),
        "{dump}"
    );
    assert!(dump.contains("(impl Stack (generics (List Int))"), "{dump}");
    assert!(
        dump.contains("(impl Stack (generics (fn Int (-> Int))) (for Shown)"),
        "{dump}"
    );
    assert_eq!(
        parse("module main\n\nimpl {\n    Stack<List<Int>: Display> {\n        fn f(self) {}\n    }\n}\n").codes,
        ["E0301"]
    );
}

/// `interface Greeter: Named + Display` names its super-interfaces (chapter 4, section 8.2).
#[test]
fn super_interfaces() {
    let dump = ok(
        "module main\n\ninterfaces {\n    interface Greeter: Named + Display {\n        fn hi(self)\n    }\n    interface Sorted<T>:\n        Container<T> {\n        fn top(self) -> T\n    }\n}\n",
    );
    assert!(
        dump.contains("(interface Greeter (: Named Display)"),
        "{dump}"
    );
    assert!(dump.contains("(: (Container T))"), "{dump}");
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

/// After `.`, `:`, `->`, `=` and binary operators a newline continues the line (chapter 1, 7).
#[test]
fn newline_after_continuation_tokens() {
    let text = "module main\n\nconfig {\n    max:\n        Int =\n        10\n}\n\ntypes {\n    type Ids =\n        List<Int>\n    struct S<T:\n        Copyable> {\n        x:\n            Int\n    }\n}\n\nfunctions {\n    fn f(a:\n        Int) ->\n        Int {\n        let b:\n            Int = a.\n            abs()\n        let g = |x: Int| ->\n            Int { x }\n        b\n    }\n}\n";
    let dump = ok(text);
    assert!(dump.contains("(const max Int 10)"), "{dump}");
    assert!(dump.contains("(alias Ids (List Int))"), "{dump}");
    assert!(dump.contains("(generics (T Copyable))"), "{dump}");
    assert!(dump.contains("(field x Int)"), "{dump}");
    assert!(dump.contains("(params (a Int))\n      (-> Int)"), "{dump}");
    assert!(dump.contains("(let b Int (call (. a abs)))"), "{dump}");
    assert!(dump.contains("(-> Int) (block x)"), "{dump}");
    let impl_text = "module main\n\nimpl {\n    A:\n        B {\n        fn f(self) {}\n    }\n}\n";
    assert!(ok(impl_text).contains("(for B)"));
}

#[test]
fn pub_where_it_is_not_allowed() {
    let codes = |text: &str| parse(text).codes;
    assert_eq!(
        codes("module main\n\ntests {\n    pub test \"x\" {\n    }\n}\n"),
        ["E0301"]
    );
    assert_eq!(
        codes("module main\n\nimpl {\n    pub A {\n        fn f(self) {}\n    }\n}\n"),
        ["E0301"]
    );
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
fn self_in_struct_literals_and_patterns() {
    let text = "module main\n\nimpl {\n    Tree {\n        fn leaf() -> Self {\n            Self.Leaf\n        }\n        fn new(v: Int) -> Self {\n            Self { value: v }\n        }\n        fn size(self) -> Int {\n            match self {\n                Self.Leaf => 0\n                Self.Node(l, _, r) => 1\n                Self { value, .. } => value\n                Self => 2\n            }\n        }\n    }\n}\n";
    let dump = ok(text);
    assert!(dump.contains("(block (. Self Leaf))"), "{dump}");
    assert!(dump.contains("(struct Self (value: v))"), "{dump}");
    assert!(dump.contains("(=> Self.Leaf 0)"), "{dump}");
    assert!(dump.contains("(=> (Self.Node l _ r) 1)"), "{dump}");
    assert!(dump.contains("(=> (Self {} value ..) value)"), "{dump}");
    assert!(dump.contains("(=> Self 2)"), "{dump}");
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

/// `break label: value`, and a labelled `loop` where an expression is expected; in the key of a
/// map literal the name before `:` stays the key (chapter 3, section 4, rule 5; D3.11).
#[test]
fn break_label_with_a_value() {
    let dump = ok(&body(
        "        let found = search: loop {\n            for x in xs {\n                break search: x + 1\n            }\n            break search:\n                0\n        }\n        f(\n            inner: loop {\n                break inner: 2\n            },\n        )\n        let m = [k: loop {\n            break 1\n        }]\n        let n = [(k: loop {\n            break k: 1\n        }): 2]\n        outer: loop {\n            break outer\n        }",
    ));
    assert!(dump.contains("found\n          (search: (loop"), "{dump}");
    assert!(dump.contains("(break search: (+ x 1))"), "{dump}");
    assert!(dump.contains("(break search: 0)"), "{dump}");
    assert!(dump.contains("(call f (inner: (loop"), "{dump}");
    assert!(
        dump.contains("(map (: k (loop (block (break 1))))"),
        "{dump}"
    );
    assert!(dump.contains("(map (: (paren (k: (loop"), "{dump}");
    assert!(
        dump.contains("(outer: (loop (block (break outer:))))"),
        "{dump}"
    );
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
    pub intrinsic type Map<K: Hashable, V>
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

/// Arbitrary token soup through lexer, parser and both renderers: no panic, no hang. Runs on a
/// thread with the stack the compiler uses (`wolf_parser::STACK_SIZE`).
#[test]
fn arbitrary_input_never_panics() {
    std::thread::Builder::new()
        .stack_size(wolf_parser::STACK_SIZE)
        .spawn(soup)
        .unwrap()
        .join()
        .unwrap();
}

fn soup() {
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
            text.push_str(WORDS.get((next() as usize) % WORDS.len()).unwrap_or(&"x"));
            text.push(' ');
        }
        let _ = parse(&text);
    }
    assert!(
        started.elapsed() < Duration::from_secs(60),
        "parsing is too slow"
    );
}
