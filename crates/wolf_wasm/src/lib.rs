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

/// Checks the source and runs it in the VM of the compiler (spec chapter 10), with `input` as
/// its standard input. Returns the object of [`check`] with `"stdout"`, `"stderr"` and `"code"`
/// added: the exit code, or `null` when the program has errors and did not run.
#[wasm_bindgen]
pub fn run(source: &str, input: &str) -> String {
    let mut stdout = String::new();
    let mut stderr = String::new();
    let (map, diagnostics, code) = run_with(source, input, &mut |to_err, text| {
        if to_err {
            stderr.push_str(text);
        } else {
            stdout.push_str(text);
        }
    });
    let mut out = report(&map, &diagnostics, None);
    out.pop();
    out.push_str(&format!(
        ",\"stdout\":{},\"stderr\":{},\"code\":{}}}",
        quote(&stdout),
        quote(&stderr),
        code.map_or("null".to_string(), |c| c.to_string())
    ));
    out
}

/// [`run`] for the browser: the output goes to `on_output(stream, text)` as the program writes
/// it, with `stream` `"stdout"` or `"stderr"`, so that the page shows it also when it stops a
/// program that runs too long. Returns the object of [`check`] with `"code"` added.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn run_live(source: &str, input: &str, on_output: &js_sys::Function) -> String {
    let (map, diagnostics, code) = run_with(source, input, &mut |to_err, text| {
        let stream = if to_err { "stderr" } else { "stdout" };
        let _ = on_output.call2(
            &wasm_bindgen::JsValue::NULL,
            &wasm_bindgen::JsValue::from_str(stream),
            &wasm_bindgen::JsValue::from_str(text),
        );
    });
    let mut out = report(&map, &diagnostics, None);
    out.pop();
    out.push_str(&format!(
        ",\"code\":{}}}",
        code.map_or("null".to_string(), |c| c.to_string())
    ));
    out
}

/// Checks and runs the source; the output goes to `write(to_stderr, text)`. The exit code is
/// `None` when the program did not run.
fn run_with(
    source: &str,
    input: &str,
    write: &mut dyn FnMut(bool, &str),
) -> (SourceMap, Vec<Diagnostic>, Option<i32>) {
    let mut single = wolf_driver::single::check(source);
    let Some(program) = single.program_for_vm() else {
        return (single.map, single.diagnostics, None);
    };
    let Some(entry) = program
        .modules
        .iter()
        .position(|m| m.role == wolf_ir::ModuleRole::Entry)
    else {
        return (single.map, single.diagnostics, None);
    };
    let mut io = PlaygroundIo {
        write,
        input: input
            .lines()
            .map(str::to_string)
            .collect::<Vec<_>>()
            .into_iter(),
    };
    match wolf_vm::run_main(&program, entry, &mut io) {
        Ok(code) => (single.map, single.diagnostics, Some(code)),
        Err(e) => {
            single.diagnostics.push(
                Diagnostic::new(wolf_diag::codes::E0003, "internal compiler error")
                    .note(format!("the VM failed: {e}"))
                    .help("this is a bug in wolf; please report it"),
            );
            (single.map, single.diagnostics, None)
        }
    }
}

/// What a program in the playground can reach: its output, the lines of the input box, and no
/// arguments, environment or files.
struct PlaygroundIo<'a> {
    write: &'a mut dyn FnMut(bool, &str),
    input: std::vec::IntoIter<String>,
}

/// The text of `FsError.Other` in the playground.
const NO_FILES: &str = "files are not available in the playground";

impl wolf_vm::Io for PlaygroundIo<'_> {
    fn out(&mut self, text: &str) {
        (self.write)(false, text);
    }

    fn err(&mut self, text: &str) {
        (self.write)(true, text);
    }

    fn read_line(&mut self) -> Option<String> {
        self.input.next()
    }

    fn args(&self) -> Vec<String> {
        Vec::new()
    }

    fn env(&self, _name: &str) -> Option<String> {
        None
    }

    fn read_text(&mut self, _path: &str) -> Result<String, wolf_vm::FsError> {
        Err(wolf_vm::FsError::Other(NO_FILES.to_string()))
    }

    fn write_text(&mut self, _path: &str, _content: &str) -> Result<(), wolf_vm::FsError> {
        Err(wolf_vm::FsError::Other(NO_FILES.to_string()))
    }

    fn exists(&self, _path: &str) -> bool {
        false
    }
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
