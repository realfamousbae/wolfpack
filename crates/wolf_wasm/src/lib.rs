//! The wolfpack compiler for the browser playground (`playground/`), compiled to WebAssembly.
//!
//! The playground edits one file, `src/main.wolf` of a `cli` project. Every function takes the
//! source and returns a JSON object as a string, so the JavaScript side needs no bindings for Rust
//! types. The same functions are plain Rust and are tested natively.

use wasm_bindgen::prelude::wasm_bindgen;
use wolf_diag::Diagnostic;
use wolf_project::{Manifest, Target};
use wolf_span::SourceMap;

/// The manifest of the playground project.
const MANIFEST: &str = "[package]\nname = \"playground\"\nversion = \"0.1.0\"\nkind = \"cli\"\n";

/// The name of the edited file in diagnostics.
const FILE: &str = "src/main.wolf";

/// Checks the source. Returns `{"errors": n, "text": "...", "diagnostics": [...]}`: the number
/// of errors, the diagnostics as `wolf check` prints them, and as `--message-format json`
/// objects.
#[wasm_bindgen]
pub fn check(source: &str) -> String {
    let (map, diagnostics, _) = analyze(source);
    report(&map, &diagnostics, None)
}

/// Checks the source and generates the project of `target` (`ts`, `go` or `rust`). Returns the
/// object of [`check`] with `"files": [{"path": "...", "contents": "..."}]` added; the list is
/// empty when the program has errors.
#[wasm_bindgen]
pub fn build(source: &str, target: &str) -> String {
    let Some(target) = Target::ALL.into_iter().find(|t| t.name() == target) else {
        return format!(
            "{{\"errors\":1,\"text\":{},\"diagnostics\":[],\"files\":[]}}",
            quote(&format!("unknown target `{target}`"))
        );
    };
    let (map, mut diagnostics, project) = analyze(source);
    let mut files = Vec::new();
    if let (true, Some((manifest, analysis))) = (diagnostics.is_empty(), project) {
        let inputs = analysis.inputs();
        diagnostics.extend(wolf_resolve::check_target(&inputs, target.name(), |role| {
            role != wolf_resolve::Role::Test
        }));
        if diagnostics.is_empty() {
            let meta = wolf_driver::meta(&manifest, true);
            let generated =
                wolf_ir::lower(&inputs, &analysis.resolved, &analysis.typed, &map, &meta)
                    .map_err(|e| e.message)
                    .and_then(|program| {
                        wolf_driver::generate(target, &program, env!("CARGO_PKG_VERSION"))
                    });
            match generated {
                Ok(f) => files = f,
                Err(message) => diagnostics.push(
                    Diagnostic::new(wolf_diag::codes::E0003, "internal compiler error")
                        .note(format!("the backend does not support this yet: {message}"))
                        .help("this is a bug or a gap in wolf; please report it"),
                ),
            }
        }
    }
    report(&map, &diagnostics, Some(&files))
}

/// Formats the source as `wolf fmt` does. Returns `{"ok": true, "text": "..."}`, or
/// `{"ok": false, "text": "..."}` with the syntax errors that prevent formatting.
#[wasm_bindgen]
pub fn format(source: &str) -> String {
    let mut map = SourceMap::new();
    let id = map.add(FILE, source.to_string());
    match wolf_fmt::format(id, source) {
        Ok(text) => format!("{{\"ok\":true,\"text\":{}}}", quote(&text)),
        Err(wolf_fmt::FmtError::Syntax(diagnostics)) => format!(
            "{{\"ok\":false,\"text\":{}}}",
            quote(&wolf_diag::render_human_to_string(&map, &diagnostics))
        ),
        Err(wolf_fmt::FmtError::Internal(message)) => format!(
            "{{\"ok\":false,\"text\":{}}}",
            quote(&format!("internal error of the formatter: {message}"))
        ),
    }
}

/// The text of `wolf explain <code>`, or an empty string for an unknown code.
#[wasm_bindgen]
pub fn explain(code: &str) -> String {
    match wolf_diag::explain::explain(code) {
        wolf_diag::explain::Explanation::Text(_, text) => text.to_string(),
        wolf_diag::explain::Explanation::Retired(id) => {
            format!("{id} is no longer used by the compiler\n")
        }
        wolf_diag::explain::Explanation::Unknown => String::new(),
    }
}

/// The version of the compiler.
#[wasm_bindgen]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Parses and checks the source as the only module of the playground project.
fn analyze(
    source: &str,
) -> (
    SourceMap,
    Vec<Diagnostic>,
    Option<(Manifest, wolf_driver::Analysis)>,
) {
    let mut map = SourceMap::new();
    let mut diagnostics = Vec::new();
    let manifest_id = map.add("wolfpack.toml", MANIFEST.to_string());
    let (manifest, found) = wolf_project::parse_manifest(manifest_id, MANIFEST);
    diagnostics.extend(found);
    let Some(manifest) = manifest else {
        return (map, diagnostics, None);
    };
    let mut parsed = wolf_driver::std_modules(&mut map, &mut diagnostics);
    let id = map.add(FILE, source.to_string());
    let tree = wolf_driver::parse(&map, id, &mut diagnostics);
    parsed.push((
        vec!["main".to_string()],
        wolf_resolve::Role::Entry,
        id,
        tree,
    ));
    let analysis = wolf_driver::analyze(&map, &manifest, parsed, &mut diagnostics);
    (map, diagnostics, Some((manifest, analysis)))
}

/// The JSON result of [`check`] and [`build`].
fn report(
    map: &SourceMap,
    diagnostics: &[Diagnostic],
    files: Option<&[wolf_driver::OutFile]>,
) -> String {
    let mut sorted = diagnostics.to_vec();
    wolf_diag::sort(&mut sorted);
    let mut text = wolf_diag::render_human_to_string(map, &sorted);
    if !sorted.is_empty() {
        let noun = if sorted.len() == 1 { "error" } else { "errors" };
        text.push_str(&format!("error: aborting due to {} {noun}\n", sorted.len()));
    }
    // `render_json` writes one object per line.
    let json = wolf_diag::render_json(map, &sorted);
    let objects = json.lines().collect::<Vec<_>>().join(",");
    let mut out = format!(
        "{{\"errors\":{},\"text\":{},\"diagnostics\":[{}]",
        sorted.len(),
        quote(&text),
        objects
    );
    if let Some(files) = files {
        let list: Vec<String> = files
            .iter()
            .map(|f| {
                format!(
                    "{{\"path\":{},\"contents\":{}}}",
                    quote(&f.path),
                    quote(&f.contents)
                )
            })
            .collect();
        out.push_str(&format!(",\"files\":[{}]", list.join(",")));
    }
    out.push('}');
    out
}

/// A string as a JSON string literal.
fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
