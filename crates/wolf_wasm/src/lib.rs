//! The wolfpack compiler for the browser playground (`playground/`), compiled to WebAssembly.
//!
//! The playground edits one file, `src/main.wolf` of a `cli` project (`wolf_driver::single`).
//! Every function takes the source and returns a JSON object as a string, so the JavaScript side
//! needs no bindings for Rust types. The same functions are plain Rust and are tested natively.

use wasm_bindgen::prelude::wasm_bindgen;
use wolf_diag::Diagnostic;
use wolf_project::Target;
use wolf_span::SourceMap;

/// Checks the source. Returns `{"errors": n, "text": "...", "diagnostics": [...]}`: the number
/// of errors, the diagnostics as `wolf check` prints them, and as `--message-format json`
/// objects.
#[wasm_bindgen]
pub fn check(source: &str) -> String {
    let single = wolf_driver::single::check(source);
    report(&single.map, &single.diagnostics, None)
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
    let mut single = wolf_driver::single::check(source);
    let files = single.build(target);
    report(&single.map, &single.diagnostics, Some(&files))
}

/// Formats the source as `wolf fmt` does. Returns `{"ok": true, "text": "..."}`, or
/// `{"ok": false, "text": "..."}` with the syntax errors that prevent formatting.
#[wasm_bindgen]
pub fn format(source: &str) -> String {
    let mut map = SourceMap::new();
    let id = map.add(wolf_driver::single::FILE, source.to_string());
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
