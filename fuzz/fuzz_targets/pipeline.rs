//! Checks arbitrary text as the program of a project of one file, builds it for every target and
//! formats it, as the playground does. Any panic is a bug: every invalid file yields a diagnostic
//! (spec chapter 9). An internal compiler error (E0003) is turned into a panic, so that the fuzzer
//! keeps the input.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(source) = std::str::from_utf8(data) else {
        return;
    };
    let source = source.to_string();
    // As in the `lexer` target: no stack overflow only on a thread of `STACK_SIZE`.
    let worker = std::thread::Builder::new()
        .stack_size(wolf_parser::STACK_SIZE)
        .spawn(move || {
            for target in wolf_project::Target::ALL {
                let mut single = wolf_driver::single::check(&source);
                let _ = single.build(target);
                if let Some(d) = single
                    .diagnostics
                    .iter()
                    .find(|d| d.code == wolf_diag::codes::E0003)
                {
                    panic!("internal compiler error on {}: {:?}", target.name(), d.notes);
                }
            }
            let mut map = wolf_span::SourceMap::new();
            let id = map.add("main.wolf", source.clone());
            let _ = wolf_fmt::format(id, &source);
        });
    if let Ok(handle) = worker {
        if let Err(payload) = handle.join() {
            std::panic::resume_unwind(payload);
        }
    }
});
