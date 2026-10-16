//! Tests of `wolf explain` (spec chapter 9, section 2, rule 8).
//!
//! Every ```` ```wolf wrong ```` block of a text is checked as a project and must report the
//! explained code first; every ```` ```wolf fixed ```` block must check without errors. A block is
//! `src/main.wolf` of a `cli` project; a line `// file: <path>` starts another file of the project,
//! which may also be `wolfpack.toml`.

// Test helpers may fail loudly.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use wolf_diag::explain;

fn wolf(cwd: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_wolf"))
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap()
}

/// The ```` ```wolf <kind> ```` blocks of a text.
fn blocks(text: &str, kind: &str) -> Vec<String> {
    let fence = format!("```wolf {kind}");
    let mut found = Vec::new();
    let mut current: Option<String> = None;
    for line in text.lines() {
        match &mut current {
            None if line == fence => current = Some(String::new()),
            None => {}
            Some(_) if line == "```" => found.push(current.take().unwrap()),
            Some(block) => {
                block.push_str(line);
                block.push('\n');
            }
        }
    }
    assert!(current.is_none(), "unclosed ```wolf {kind} block");
    found
}

/// The files of a block: `src/main.wolf` first, then one per `// file: <path>` line.
fn files(source: &str) -> Vec<(String, String)> {
    let mut files = vec![("src/main.wolf".to_string(), String::new())];
    for line in source.lines() {
        if let Some(path) = line.strip_prefix("// file: ") {
            files.push((path.to_string(), String::new()));
            continue;
        }
        let text = &mut files.last_mut().unwrap().1;
        text.push_str(line);
        text.push('\n');
    }
    files.retain(|(_, text)| !text.trim().is_empty());
    for (_, text) in &mut files {
        let trimmed = text.trim_end().len();
        text.truncate(trimmed);
        text.push('\n');
    }
    files
}

/// Checks a block as a project; the codes reported, in order.
fn check(dir: &Path, source: &str) -> (Vec<String>, String) {
    let _ = fs::remove_dir_all(dir);
    fs::create_dir_all(dir).unwrap();
    fs::write(
        dir.join("wolfpack.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nkind = \"cli\"\n",
    )
    .unwrap();
    for (path, text) in files(source) {
        let path = dir.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    let output = wolf(dir, &["check", "--message-format", "json"]);
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let codes = stderr
        .lines()
        .filter_map(|line| {
            let start = line.find("\"code\":\"")? + 8;
            Some(line[start..start + 5].to_string())
        })
        .collect();
    (codes, stderr)
}

fn tmp(name: &str) -> PathBuf {
    Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("explain")
        .join(name)
}

#[test]
fn examples_report_the_explained_code() {
    let mut failures = Vec::new();
    for (id, text) in explain::texts() {
        let heading = format!("# {id}: ");
        if !text.starts_with(&heading) {
            failures.push(format!("{id}: the text must start with `{heading}`"));
        }
        if !text.contains("\nWrong:\n") || !text.contains("\nFixed:\n") {
            failures.push(format!("{id}: `Wrong:` or `Fixed:` is missing"));
        }
        for (n, source) in blocks(text, "wrong").iter().enumerate() {
            let (codes, stderr) = check(&tmp(&format!("{id}-wrong-{n}")), source);
            if codes.first().map(String::as_str) != Some(*id) {
                failures.push(format!(
                    "{id}: wrong example {n} reports {codes:?}\n{stderr}"
                ));
            }
        }
        for (n, source) in blocks(text, "fixed").iter().enumerate() {
            let (codes, stderr) = check(&tmp(&format!("{id}-fixed-{n}")), source);
            if !codes.is_empty() {
                failures.push(format!(
                    "{id}: fixed example {n} reports {codes:?}\n{stderr}"
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn explain_prints_the_text() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"));
    let output = wolf(dir, &["explain", "E0501"]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        explain::text("E0501").unwrap()
    );
    let lower = wolf(dir, &["explain", "e0501"]);
    assert_eq!(lower.stdout, output.stdout);
}

#[test]
fn explain_rejects_unknown_codes() {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"));
    for code in ["E9999", "0501", "hello"] {
        let output = wolf(dir, &["explain", code]);
        assert_eq!(output.status.code(), Some(2), "{code}");
        assert_eq!(
            String::from_utf8_lossy(&output.stderr),
            format!("error[E0002]: there is no error code `{code}`\n")
        );
    }
}
