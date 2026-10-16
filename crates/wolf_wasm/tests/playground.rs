//! The functions of the playground, called natively.

const HELLO: &str =
    "module main\n\nfunctions {\n    fn main() {\n        print(\"hi\")\n    }\n}\n";

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
    assert!(result.contains("--> src/main.wolf:5:9"), "{result}");
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
    let messy = "module main\nfunctions {\n    fn main() { print(\"hi\")   }\n}\n";
    assert_eq!(
        wolf_wasm::format(messy),
        "{\"ok\":true,\"text\":\"module main\\n\\nfunctions {\\n    fn main() { print(\\\"hi\\\") }\\n}\\n\"}"
    );
    assert!(wolf_wasm::format("module main\nfunctions {\n").starts_with("{\"ok\":false,"));
    assert!(wolf_wasm::explain("e0501").starts_with("# E0501: "));
    assert_eq!(wolf_wasm::explain("E9999"), "");
}
