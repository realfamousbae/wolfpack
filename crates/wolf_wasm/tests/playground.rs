//! The functions of the playground, called natively.

const HELLO: &str = "module main\n\nmain {\n    print(\"hi\")\n}\n";

#[test]
fn a_valid_program_has_no_errors() {
    let result = wolf_wasm::check(HELLO);
    assert!(
        result.starts_with("{\"errors\":0,\"text\":\"\",\"diagnostics\":[]"),
        "{result}"
    );
}

#[test]
fn errors_come_as_text_and_json() {
    let result = wolf_wasm::check(&HELLO.replace("print", "prnt"));
    assert!(result.starts_with("{\"errors\":1,"), "{result}");
    assert!(
        result.contains("error[E0609]: unknown name `prnt`"),
        "{result}"
    );
    assert!(result.contains("--> src/main.wolf:4:5"), "{result}");
    assert!(result.contains("\"code\":\"E0609\""), "{result}");
}

#[test]
fn build_generates_every_target() {
    for (target, file) in [
        ("ts", "src/main.ts"),
        ("go", "main.go"),
        ("rust", "src/main.rs"),
    ] {
        let result = wolf_wasm::build(HELLO, target);
        assert!(result.starts_with("{\"errors\":0,"), "{target}: {result}");
        assert!(
            result.contains(&format!("{{\"path\":\"{file}\"")),
            "{target}: {result}"
        );
    }
}

#[test]
fn build_reports_errors_and_no_files() {
    let result = wolf_wasm::build("module main\n", "ts");
    assert!(result.contains("E0614"), "{result}");
    assert!(result.ends_with("\"files\":[]}"), "{result}");
    assert!(wolf_wasm::build(HELLO, "java").contains("unknown target"));
}

#[test]
fn format_and_explain() {
    let messy = "module main\nmain { print(\"hi\") }\n";
    assert_eq!(
        wolf_wasm::format(messy),
        "{\"ok\":true,\"text\":\"module main\\n\\nmain { print(\\\"hi\\\") }\\n\"}"
    );
    assert!(wolf_wasm::format("module main\nfunctions {\n").starts_with("{\"ok\":false,"));
    assert!(wolf_wasm::explain("e0501").starts_with("# E0501: "));
    assert_eq!(wolf_wasm::explain("E9999"), "");
}

#[test]
fn run_prints_the_output_of_the_vm() {
    let result = wolf_wasm::run(HELLO, "");
    assert!(result.starts_with("{\"errors\":0,"), "{result}");
    assert!(
        result.ends_with("\"stdout\":\"hi\\n\",\"stderr\":\"\",\"code\":0}"),
        "{result}"
    );
}

#[test]
fn run_reads_the_input_and_reports_panics() {
    let source = "module main\n\nimports {\n    std.io\n}\n\nmain {\n    match io.read_line() {\n        Some(line) => print(\"got \" + line)\n        None => print(\"nothing\")\n    }\n    let items = [1]\n    print(items[3].to_string())\n}\n";
    let result = wolf_wasm::run(source, "abc\ndef\n");
    assert!(result.contains("\"stdout\":\"got abc\\n\""), "{result}");
    assert!(
        result.contains("\"stderr\":\"panic: index 3 out of range for length 1\\n\""),
        "{result}"
    );
    assert!(result.ends_with("\"code\":101}"), "{result}");
}

#[test]
fn run_does_not_run_a_program_with_errors_or_files() {
    let result = wolf_wasm::run("module main\n", "");
    assert!(result.contains("E0614"), "{result}");
    assert!(result.ends_with("\"code\":null}"), "{result}");
    let files = "module main\n\nimports {\n    std.fs\n}\n\nmain {\n    match fs.read_text(\"a.txt\") {\n        Ok(text) => print(text)\n        Err(e) => print(e.message())\n    }\n}\n";
    let result = wolf_wasm::run(files, "");
    assert!(
        result.contains("files are not available in the playground"),
        "{result}"
    );
}
