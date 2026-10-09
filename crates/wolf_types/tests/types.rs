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
    let project = ProjectInfo {
        kind: Kind::Cli,
        config: &[],
    };
    let mut resolved = wolf_resolve::resolve(&inputs, &project);
    diagnostics.append(&mut resolved.diagnostics);
    let typed = wolf_types::check(&inputs, &resolved, &project);
    diagnostics.extend(typed.diagnostics);
    wolf_diag::sort(&mut diagnostics);
    wolf_diag::render_human_to_string(&map, &diagnostics)
}

fn main_with(body: &str) -> String {
    format!("module main\n\nfunctions {{\n    fn main() {{\n{body}\n    }}\n}}\n")
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
        "module main\n\nfunctions {\n    fn drop_last(mut l: List<Int>) {\n        l.pop()\n    }\n\n    fn main() {\n        var l = [1]\n        if l.len() > 0 {\n            print(\"x\")\n        } else {\n            l.pop()\n        }\n        drop_last(mut l)\n    }\n}\n",
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
