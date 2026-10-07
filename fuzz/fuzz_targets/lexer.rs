//! Lexes arbitrary bytes and renders the diagnostics. Any panic is a bug.

#![no_main]

use libfuzzer_sys::fuzz_target;
use wolf_span::SourceMap;

fuzz_target!(|data: &[u8]| {
    let mut map = SourceMap::new();
    let diagnostics = match wolf_lexer::decode(data.to_vec()) {
        Ok(text) => {
            let id = map.add("fuzz.wolf", text.clone());
            wolf_lexer::lex(id, &text).diagnostics
        }
        Err(invalid) => {
            let id = map.add("fuzz.wolf", invalid.lossy.clone());
            vec![invalid.diagnostic(id)]
        }
    };
    let _ = wolf_diag::render_human_to_string(&map, &diagnostics);
    let _ = wolf_diag::render_json(&map, &diagnostics);
});
