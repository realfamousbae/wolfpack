//! Lexes and parses arbitrary bytes and renders the diagnostics. Any panic is a bug.

#![no_main]

use libfuzzer_sys::fuzz_target;
use wolf_span::SourceMap;

fuzz_target!(|data: &[u8]| {
    // The compiler guarantees no stack overflow only on a thread of `STACK_SIZE` (the `wolf`
    // binary runs on one), so the fuzzer does the same.
    let data = data.to_vec();
    let worker = std::thread::Builder::new()
        .stack_size(wolf_parser::STACK_SIZE)
        .spawn(move || check(&data));
    if let Ok(handle) = worker {
        if let Err(payload) = handle.join() {
            std::panic::resume_unwind(payload);
        }
    }
});

fn check(data: &[u8]) {
    let mut map = SourceMap::new();
    let diagnostics = match wolf_lexer::decode(data.to_vec()) {
        Ok(text) => {
            let id = map.add("fuzz.wolf", text.clone());
            let lexed = wolf_lexer::lex(id, &text);
            let parsed = wolf_parser::parse(id, &text, &lexed.tokens);
            let _ = wolf_ast::dump(&parsed.file);
            let mut diagnostics = lexed.diagnostics;
            diagnostics.extend(parsed.diagnostics);
            diagnostics
        }
        Err(invalid) => {
            let id = map.add("fuzz.wolf", invalid.lossy.clone());
            vec![invalid.diagnostic(id)]
        }
    };
    let _ = wolf_diag::render_human_to_string(&map, &diagnostics);
    let _ = wolf_diag::render_json(&map, &diagnostics);
}
