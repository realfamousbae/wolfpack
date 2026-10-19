//! Type checking of std, of a valid program that uses most rules, and of inference cases.

// Test helpers may fail loudly.
#![allow(clippy::unwrap_used)]

use wolf_project::Kind;
use wolf_resolve::{ModuleInput, ProjectInfo, Role};
use wolf_span::SourceMap;

/// Parses std and one `main` module, resolves and type-checks them, and returns the rendered
/// diagnostics.
fn check(main: &str) -> String {
    let mut map = SourceMap::new();
    let mut sources: Vec<(String, Role, String, String)> = wolf_std::MODULES
        .iter()
        .map(|m| {
            let role = if m.path == wolf_std::PRELUDE {
                Role::Prelude
            } else {
                Role::Std
            };
            (
                m.path.to_string(),
                role,
                m.file_name.to_string(),
                m.source.to_string(),
            )
        })
        .collect();
    sources.push((
        "main".to_string(),
        Role::Entry,
        "main.wolf".to_string(),
        main.to_string(),
    ));
    let mut diagnostics = Vec::new();
    let mut trees = Vec::new();
    for (path, role, name, source) in &sources {
        let id = map.add(name.clone(), source.clone());
        let lexed = wolf_lexer::lex(id, source);
        let tree = wolf_parser::parse(id, source, &lexed.tokens);
        diagnostics.extend(lexed.diagnostics);
        diagnostics.extend(tree.diagnostics);
        let segments: Vec<String> = path.split('.').map(str::to_string).collect();
        trees.push((segments, *role, id, tree.file));
    }
    let inputs: Vec<ModuleInput<'_>> = trees
        .iter()
        .map(|(path, role, file, ast)| ModuleInput {
            path: path.clone(),
            role: *role,
            file: *file,
            ast,
        })
        .collect();
    let native = wolf_resolve::NativeNames::new();
    let project = ProjectInfo {
        kind: Kind::Cli,
        config: &[],
        native: &native,
    };
    let mut resolved = wolf_resolve::resolve(&inputs, &project);
    diagnostics.append(&mut resolved.diagnostics);
    let typed = wolf_types::check(&inputs, &resolved, &project);
    diagnostics.extend(typed.diagnostics);
    wolf_diag::sort(&mut diagnostics);
    wolf_diag::render_human_to_string(&map, &diagnostics)
}

fn main_with(body: &str) -> String {
    format!("module main\n\nmain {{\n{body}\n}}\n")
}

#[test]
fn std_type_checks_cleanly() {
    let out = check(&main_with("        print(\"hi\")"));
    assert!(out.is_empty(), "{out}");
}

#[test]
fn valid_program_type_checks_cleanly() {
    let out = check(include_str!("valid/main.wolf"));
    assert!(out.is_empty(), "{out}");
}

#[test]
fn inference_flows_both_ways() {
    let cases = [
        // A list gets its element type from a later `push`.
        "        var names = []\n        names.push(\"a\")\n        print(names.len().to_string())",
        // Closure parameters come from the expected function type.
        "        let lens = [\"a\", \"bb\"].map(|s| s.len())\n        print(lens.len().to_string())",
        // `None` and `[:]` get their types from an annotation.
        "        let o: Option<Int> = None\n        let m: Map<String, Int> = [:]\n        let _ = [o.is_none_or_zero(), m.len()]",
        // Operators fix the type of a closure parameter.
        "        let inc = |x| x + 1\n        print(inc(1).to_string())",
        // A later call fixes the type of a closure parameter (chapter 4, rule 10.3).
        "        let size = |s| s.trim().len()\n        print(size(\" a \").to_string())",
    ];
    for (i, body) in cases.iter().enumerate() {
        let out = check(&main_with(body));
        if i == 2 {
            // `Option` has no such method: only that error.
            assert!(out.contains("E0424") && !out.contains("E0420"), "{out}");
        } else {
            assert!(out.is_empty(), "case {i}: {out}");
        }
    }
}

#[test]
fn void_blocks_discard_their_final_value() {
    let out = check(
        "module main\n\nmain {\n    var l = [1]\n    if l.len() > 0 {\n        print(\"x\")\n    } else {\n        l.pop()\n    }\n    drop_last(mut l)\n}\n\nfunctions {\n    fn drop_last(mut l: List<Int>) {\n        l.pop()\n    }\n}\n",
    );
    assert!(out.is_empty(), "{out}");
}

#[test]
fn deep_nesting_on_the_compiler_stack() {
    let depth = wolf_parser::MAX_DEPTH as usize - 10;
    let expr = format!("{}1{}", "(".repeat(depth), ")".repeat(depth));
    let source = main_with(&format!("        print(({expr}).to_string())"));
    let handle = std::thread::Builder::new()
        .stack_size(wolf_parser::STACK_SIZE)
        .spawn(move || check(&source))
        .unwrap();
    let out = handle.join().unwrap();
    assert!(out.is_empty(), "{out}");
}

#[test]
fn generic_code_type_checks_cleanly() {
    let out = check(
        "module main

types {
    struct Stack<T> {
        items: List<T>
    }
}

interfaces {
    interface Container<T> {
        fn add(mut self, own item: T)
        fn size(self) -> Int
    }
}

impl {
    Stack<T> {
        fn new() -> Stack<T> {
            Stack { items: [] }
        }

        fn map<U>(self, f: fn(T) -> U) -> Stack<U> {
            Stack { items: self.items.map(f) }
        }
    }

    Stack<T>: Container<T> {
        fn add(mut self, own item: T) {
            self.items.push(own item)
        }

        fn size(self) -> Int {
            self.items.len()
        }
    }
}

main {
    let s: Stack<Int> = Stack { items: [1, 2] }
    let labels = s.map(|n| n.to_string())
    print(describe(s) + labels.items[0] + max_of(\"a\", \"b\"))
    let empty: Stack<String> = Stack.new()
    let _ = empty
}

functions {
    fn describe(c: Container<Int>) -> String {
        c.size().to_string()
    }

    fn max_of<T: Comparable + Copyable>(a: T, b: T) -> T {
        if a.compare(b) == Ordering.Less { b.copy() } else { a.copy() }
    }
}
",
    );
    assert!(out.is_empty(), "{out}");
}

#[test]
fn match_on_a_type_learned_later_is_checked() {
    let out = check(&main_with(
        "        let pick = |s| match s.find(\"x\") {\n            Some(i) => i\n        }\n        print(pick(\"abc\").to_string())",
    ));
    assert!(
        out.contains("E0410") && out.contains("`None` is missing"),
        "{out}"
    );
}

#[test]
fn impl_blocks_inherit_the_bounds_of_their_type() {
    let out = check(
        "module main

types {
    struct Sorted<T: Comparable> {
        items: List<T>
    }
}

impl {
    Sorted<T> {
        fn new() -> Sorted<T> {
            Sorted { items: [] }
        }

        fn better(self, a: T, b: T) -> Bool {
            a.compare(b) == Ordering.Greater
        }
    }
}

main {
    let s: Sorted<Int> = Sorted.new()
    print(s.better(1, 2).to_string())
}
",
    );
    assert!(out.is_empty(), "{out}");
}

#[test]
fn methods_on_struct_literals_are_checked() {
    let out = check(
        "module main\n\ntypes {\n    struct P {\n        x: Int\n    }\n}\n\nimpl {\n    P {\n        fn get(self) -> Int {\n            self.x\n        }\n    }\n}\n\nmain {\n    print(P { x: 1 }.get().to_string())\n    print(P { x: 1 }.nope().to_string())\n}",
    );
    assert!(
        out.contains("E0424") && out.matches("error[").count() == 1,
        "{out}"
    );
}
