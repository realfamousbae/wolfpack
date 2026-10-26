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

/// A string as a JSON string literal, as `wolf_wasm` writes it.
fn quote(s: &str) -> String {
    let mut out = String::from("\"");
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

/// The programs of `crates/wolf_cli/tests/run/` that the playground can hold, one file without
/// code of the targets, files or the environment and not built for debugging, give in its VM the
/// standard output, error output and exit code they give on the targets (spec chapter 10).
#[test]
fn run_programs_give_their_target_output_in_the_playground() {
    use std::fs;
    use std::path::Path;
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../wolf_cli/tests/run");
    let mut dirs: Vec<_> = fs::read_dir(&root)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .collect();
    dirs.sort();
    let (mut ran, mut failures) = (0, Vec::new());
    for dir in dirs {
        let name = dir.file_name().unwrap().to_string_lossy().into_owned();
        let manifest = fs::read_to_string(dir.join("wolfpack.toml")).unwrap_or_default();
        let sources: Vec<_> = fs::read_dir(dir.join("src"))
            .map(|d| d.flatten().map(|e| e.path()).collect())
            .unwrap_or_default();
        let Ok(source) = fs::read_to_string(dir.join("src/main.wolf")) else {
            continue;
        };
        // The playground makes no debug builds (`debug.txt`: built with `--debug`).
        let held = !name.starts_with("E0")
            && !dir.join("debug.txt").is_file()
            && sources.len() == 1
            && !dir.join("native").exists()
            && !manifest.contains("[config")
            && !manifest.contains("[extern")
            && !manifest.contains("kind = \"lib\"")
            && !source.contains("extern {")
            && !source.contains("std.fs")
            && !source.contains("process.env");
        if !held {
            continue;
        }
        let read = |f: &str, default: &str| {
            fs::read_to_string(dir.join(f)).unwrap_or_else(|_| default.to_string())
        };
        let tail = format!(
            ",\"stdout\":{},\"stderr\":{},\"code\":{}}}",
            quote(&read("expected.txt", "")),
            quote(&read("expected.stderr", "")),
            read("expected.code", "0").trim()
        );
        let result = wolf_wasm::run(&source, "");
        if !result.ends_with(&tail) {
            failures.push(format!(
                "--- {name}\nexpected the end {tail}\nactual {result}"
            ));
        }
        ran += 1;
    }
    assert!(ran > 20, "only {ran} programs ran");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
