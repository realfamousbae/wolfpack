//! Tests of `wolf explain` (spec chapter 9, section 2, rule 8).
//!
//! Every ```` ```wolf wrong ```` block of a text is checked as a project and must report the
//! explained code first; every ```` ```wolf fixed ```` block must check without errors and build
//! for the default target. Blocks are written as projects as described in `common`.

// Test helpers may fail loudly.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use common::{check, wolf};
use wolf_diag::explain;

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

fn tmp(name: &str) -> PathBuf {
    Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("explain")
        .join(name)
}

/// Codes whose wrong example cannot be a checked project: driver errors, bytes that a Markdown
/// file cannot hold, input too large to show, and errors of `wolf build` or the target tools.
const NO_CHECKED_WRONG: &[&str] = &[
    "E0001", "E0002", "E0003", "E0004", "E0101", "E0109", "E0110", "E0314", "E0704", "E0801",
    "E0802", "E0803", "E0804", "E0805",
];

/// Codes whose fixed example cannot be a checked project.
const NO_CHECKED_FIXED: &[&str] = &[
    "E0001", "E0002", "E0003", "E0004", "E0101", "E0110", "E0801", "E0802", "E0803", "E0804",
    "E0805",
];

/// Builds a checked project for its default target; the error output if the build fails.
fn build(dir: &Path) -> Option<String> {
    let out = dir.with_extension("out");
    let _ = fs::remove_dir_all(&out);
    let output = wolf(dir, &["build", "--out", out.to_str().unwrap()]);
    if output.status.success() {
        None
    } else {
        Some(String::from_utf8_lossy(&output.stderr).into_owned())
    }
}

#[test]
fn examples_report_the_explained_code() {
    let mut failures = Vec::new();
    for (id, text) in explain::texts() {
        let code = wolf_diag::codes::ALL.iter().find(|c| c.id() == *id);
        let heading = format!("# {id}: {}\n", code.map(|c| c.title()).unwrap_or_default());
        if !text.starts_with(&heading) {
            failures.push(format!(
                "{id}: the text must start with `{}`",
                heading.trim_end()
            ));
        }
        if !text.contains("\nWrong:\n") || !text.contains("\nFixed:\n") {
            failures.push(format!("{id}: `Wrong:` or `Fixed:` is missing"));
        }
        for line in text.lines() {
            if line.starts_with("```wolf") && line != "```wolf wrong" && line != "```wolf fixed" {
                failures.push(format!("{id}: unknown fence `{line}`"));
            }
        }
        let wrong = blocks(text, "wrong");
        let fixed = blocks(text, "fixed");
        if wrong.is_empty() != NO_CHECKED_WRONG.contains(id) {
            failures.push(format!(
                "{id}: a checked wrong example is expected iff not listed"
            ));
        }
        if fixed.is_empty() != NO_CHECKED_FIXED.contains(id) {
            failures.push(format!(
                "{id}: a checked fixed example is expected iff not listed"
            ));
        }
        for (n, source) in wrong.iter().enumerate() {
            let (codes, stderr) = check(&tmp(&format!("{id}-wrong-{n}")), source);
            if codes.first().map(String::as_str) != Some(*id) {
                failures.push(format!(
                    "{id}: wrong example {n} reports {codes:?}\n{stderr}"
                ));
            }
        }
        for (n, source) in fixed.iter().enumerate() {
            let dir = tmp(&format!("{id}-fixed-{n}"));
            let (codes, stderr) = check(&dir, source);
            if !codes.is_empty() {
                failures.push(format!(
                    "{id}: fixed example {n} reports {codes:?}\n{stderr}"
                ));
            } else if let Some(stderr) = build(&dir) {
                failures.push(format!("{id}: fixed example {n} does not build\n{stderr}"));
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

#[test]
fn every_code_has_a_text() {
    let ids: Vec<&str> = wolf_diag::codes::ALL.iter().map(|c| c.id()).collect();
    for id in &ids {
        assert!(
            explain::text(id).is_some(),
            "{id} has no text in `explain/`"
        );
    }
    for (id, _) in explain::texts() {
        assert!(
            ids.contains(id),
            "{id} has a text but is not in `codes::ALL`"
        );
    }
}
