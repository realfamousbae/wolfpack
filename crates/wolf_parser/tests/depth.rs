//! Deeply nested input must give E0314, not a stack overflow.
//!
//! The tests run on a thread with the stack size the `wolf` binary uses
//! ([`wolf_parser::STACK_SIZE`]), because that is the guarantee: at the nesting limit, parsing,
//! printing and dropping the tree fit into that stack.

// Test helpers may fail loudly.
#![allow(clippy::unwrap_used)]

use wolf_span::SourceMap;

fn parse(text: &str) -> Vec<&'static str> {
    let text = text.to_string();
    std::thread::Builder::new()
        .stack_size(wolf_parser::STACK_SIZE)
        .spawn(move || parse_here(&text))
        .unwrap()
        .join()
        .unwrap()
}

fn parse_here(text: &str) -> Vec<&'static str> {
    let mut map = SourceMap::new();
    let id = map.add("deep.wolf", text.to_string());
    let lexed = wolf_lexer::lex(id, text);
    let parsed = wolf_parser::parse(id, text, &lexed.tokens);
    // Rendering and dropping the tree must not overflow either.
    let _ = wolf_ast::dump(&parsed.file);
    let _ = wolf_diag::render_human_to_string(&map, &parsed.diagnostics);
    parsed.diagnostics.iter().map(|d| d.code.id()).collect()
}

fn in_function(body: &str) -> String {
    format!("module main\n\nmain {{\n    let x = {body}\n}}\n")
}

const DEEP: usize = 10_000;

#[test]
fn deep_parentheses() {
    let body = format!("{}1{}", "(".repeat(DEEP), ")".repeat(DEEP));
    assert_eq!(parse(&in_function(&body)), ["E0314"]);
}

#[test]
fn deep_prefix_operators() {
    let body = format!("{}1", "-".repeat(DEEP));
    assert_eq!(parse(&in_function(&body)), ["E0314"]);
}

#[test]
fn long_binary_chain() {
    let body = vec!["1"; DEEP].join(" + ");
    assert_eq!(parse(&in_function(&body)), ["E0314"]);
}

#[test]
fn long_postfix_chain() {
    let body = format!("a{}", ".b".repeat(DEEP));
    assert_eq!(parse(&in_function(&body)), ["E0314"]);
    let body = format!("a{}", "?".repeat(DEEP));
    assert_eq!(parse(&in_function(&body)), ["E0314"]);
}

#[test]
fn deep_lists_and_blocks() {
    let body = format!("{}{}", "[".repeat(DEEP), "]".repeat(DEEP));
    assert_eq!(parse(&in_function(&body)), ["E0314"]);
    let body = format!("{}1{}", "|| ".repeat(DEEP), "");
    assert_eq!(parse(&in_function(&body)), ["E0314"]);
    let blocks = format!(
        "module main\n\nmain {}x{}\n",
        "{ if a ".repeat(DEEP),
        " }".repeat(DEEP)
    );
    assert_eq!(parse(&blocks), ["E0314"]);
}

#[test]
fn deep_types_and_patterns() {
    let ty = format!(
        "module main\n\nconfig {{\n    x: {}Int{} = 1\n}}\n",
        "List<".repeat(DEEP),
        ">".repeat(DEEP)
    );
    assert_eq!(parse(&ty), ["E0314"]);
    let pat = in_function(&format!(
        "match v {{\n            {}x{} => 1\n        }}",
        "Some(".repeat(DEEP),
        ")".repeat(DEEP)
    ));
    assert_eq!(parse(&pat), ["E0314"]);
}

#[test]
fn deep_matches_struct_literals_calls_and_loops() {
    let matches = in_function(&format!(
        "{}1{}",
        "match v { _ => { ".repeat(DEEP),
        " } }".repeat(DEEP)
    ));
    assert_eq!(parse(&matches), ["E0314"]);
    let literals = format!("{}1{}", "A { x: ".repeat(DEEP), " }".repeat(DEEP));
    assert_eq!(parse(&in_function(&literals)), ["E0314"]);
    let calls = format!("{}1{}", "f(".repeat(DEEP), ")".repeat(DEEP));
    assert_eq!(parse(&in_function(&calls)), ["E0314"]);
    let loops = format!(
        "module main\n\nmain {{\n{}x{}\n}}\n",
        "for i in xs {\n".repeat(DEEP),
        "\n}".repeat(DEEP)
    );
    assert_eq!(parse(&loops), ["E0314"]);
}

/// The real requirement is far below [`wolf_parser::STACK_SIZE`]: the deepest accepted input
/// also fits into 8 MiB in a debug build. This keeps the documented margin honest.
#[test]
fn the_limit_fits_into_eight_mebibytes() {
    let depth = wolf_parser::MAX_DEPTH as usize - 10;
    let shapes = [
        in_function(&format!("{}1{}", "(".repeat(depth), ")".repeat(depth))),
        in_function(&format!(
            "{}1{}",
            "match v { _ => { ".repeat(depth / 2),
            " } }".repeat(depth / 2)
        )),
        format!(
            "module main\n\nmain {}{{ x }}{}\n",
            "{ if a ".repeat(depth / 2),
            " }".repeat(depth / 2)
        ),
    ];
    for text in shapes {
        let codes = std::thread::Builder::new()
            .stack_size(8 * 1024 * 1024)
            .spawn(move || parse_here(&text))
            .unwrap()
            .join()
            .unwrap();
        assert!(codes.is_empty(), "{codes:?}");
    }
}

#[test]
fn moderate_nesting_is_fine() {
    let body = format!("{}1{}", "(".repeat(100), ")".repeat(100));
    assert!(parse(&in_function(&body)).is_empty());
}
