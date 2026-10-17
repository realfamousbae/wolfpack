//! Resolution of std itself, of small projects, and of deeply nested input.

// Test helpers may fail loudly.
#![allow(clippy::unwrap_used)]

use wolf_project::Kind;
use wolf_resolve::{ModuleInput, ProjectInfo, Role};
use wolf_span::SourceMap;

/// Parses std and the given project modules, resolves them and returns the error codes.
fn resolve(project: &[(&str, Role, &str)], kind: Kind) -> Vec<&'static str> {
    let mut map = SourceMap::new();
    let mut parsed = Vec::new();
    for module in wolf_std::MODULES {
        let role = if module.path == wolf_std::PRELUDE {
            Role::Prelude
        } else {
            Role::Std
        };
        parsed.push((
            module.path.to_string(),
            role,
            module.file_name.to_string(),
            module.source.to_string(),
        ));
    }
    for (path, role, source) in project {
        parsed.push((
            path.to_string(),
            *role,
            format!("{path}.wolf"),
            source.to_string(),
        ));
    }
    let mut trees = Vec::new();
    let mut codes = Vec::new();
    for (path, role, name, source) in &parsed {
        let id = map.add(name.clone(), source.clone());
        let lexed = wolf_lexer::lex(id, source);
        let tree = wolf_parser::parse(id, source, &lexed.tokens);
        codes.extend(lexed.diagnostics.iter().map(|d| d.code.id()));
        codes.extend(tree.diagnostics.iter().map(|d| d.code.id()));
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
    let resolved = wolf_resolve::resolve(&inputs, &ProjectInfo { kind, config: &[] });
    let rendered = wolf_diag::render_human_to_string(&map, &resolved.diagnostics);
    codes.extend(resolved.diagnostics.iter().map(|d| d.code.id()));
    if codes.is_empty() {
        assert!(rendered.is_empty());
    }
    codes
}

const MAIN: &str = "module main\n\nmain {\n    print(\"hi\")\n}";

#[test]
fn std_resolves_cleanly() {
    assert!(resolve(&[("main", Role::Entry, MAIN)], Kind::Cli).is_empty());
}

#[test]
fn std_names_resolve() {
    let main = "module main\n\nimports {\n    std.fs\n    std.list.{sort_by}\n    std.math\n}\n\nmain {\n    var items: List<Int> = List.new()\n    items.push(math.int_max)\n    sort_by(mut items, |a, b| a.compare(b))\n    let e = fs.FsError.NotFound(\"x\")\n    match e {\n        fs.FsError.NotFound(p) => print(p)\n        _ => print(\"other\")\n    }\n    let o = Ordering.Less\n    let _ = o\n    let _ = load(\"a\")\n}\n\nfunctions {\n    fn load(path: String) -> Result<String, Error> {\n        let text = fs.read_text(path)?\n        Ok(text)\n    }\n}\n";
    let codes = resolve(&[("main", Role::Entry, main)], Kind::Cli);
    assert!(codes.is_empty(), "{codes:?}");
}

#[test]
fn deep_nesting_on_the_compiler_stack() {
    let depth = wolf_parser::MAX_DEPTH as usize - 10;
    let main = format!(
        "module main\n\nmain {{\n    let x = {}1{}\n    print(x.to_string())\n}}\n",
        "(".repeat(depth),
        ")".repeat(depth)
    );
    let codes = std::thread::Builder::new()
        .stack_size(wolf_parser::STACK_SIZE)
        .spawn(move || resolve(&[("main", Role::Entry, &main)], Kind::Cli))
        .unwrap()
        .join()
        .unwrap();
    assert!(codes.is_empty(), "{codes:?}");
}
