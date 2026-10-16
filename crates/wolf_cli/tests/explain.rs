//! `wolf explain` (spec chapter 9, section 3, rule 8): every code has a description, and its
//! examples are true: the wrong one reports the code, the fixed one checks cleanly.

// Test helpers may fail loudly.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn tmp(name: &str) -> PathBuf {
    Path::new(env!("CARGO_TARGET_TMPDIR")).join("explain").join(name)
}

fn wolf(args: &[&str], cwd: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_wolf"))
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap()
}

/// The `wolf` block after a line `label:`.
fn example(text: &str, label: &str) -> Option<String> {
    let at = text.find(&format!("\n{label}:\n"))?;
    let rest = &text[at..];
    let start = rest.find("```")?;
    let rest = &rest[start..];
    let (fence, body) = rest.split_once('\n')?;
    if fence != "```wolf" {
        return None;
    }
    let end = body.find("\n```")?;
    Some(format!("{}\n", &body[..end]))
}

/// `wolf check` on a project whose `src/main.wolf` is `source`.
fn check(name: &str, source: &str) -> Output {
    let dir = tmp(name);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("wolfpack.toml"),
        "[package]\nname = \"example\"\nversion = \"0.1.0\"\nkind = \"cli\"\n",
    )
    .unwrap();
    fs::write(dir.join("src/main.wolf"), source).unwrap();
    wolf(&["check"], &dir)
}

#[test]
fn every_code_is_explained() {
    let missing: Vec<&str> = wolf_diag::codes::ALL
        .iter()
        .map(|c| c.id())
        .filter(|id| wolf_diag::explain(id).is_none())
        .collect();
    assert!(missing.is_empty(), "no description for {}", missing.join(" "));
}

#[test]
fn examples_are_true() {
    let mut failures = Vec::new();
    let mut checked = 0;
    for code in wolf_diag::codes::ALL {
        let id = code.id();
        let Some(text) = wolf_diag::explain(id) else {
            continue;
        };
        assert!(text.starts_with(&format!("# {id}: ")), "{id}: heading");
        if let Some(wrong) = example(text, "Wrong") {
            checked += 1;
            let out = check(&format!("{id}-wrong"), &wrong);
            let stderr = String::from_utf8_lossy(&out.stderr);
            if !stderr.contains(&format!("error[{id}]")) {
                failures.push(format!("--- {id} wrong example does not report it:\n{stderr}"));
            }
        }
        if let Some(fixed) = example(text, "Fixed") {
            checked += 1;
            let out = check(&format!("{id}-fixed"), &fixed);
            if out.status.code() != Some(0) {
                failures.push(format!(
                    "--- {id} fixed example has errors:\n{}",
                    String::from_utf8_lossy(&out.stderr)
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    // Most codes have examples that can be checked.
    assert!(checked >= 120, "only {checked} examples checked");
}

#[test]
fn explain_prints_the_description() {
    let cwd = Path::new(env!("CARGO_MANIFEST_DIR"));
    let out = wolf(&["explain", "E0405"], cwd);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        wolf_diag::explain("E0405").unwrap()
    );
    let short = wolf(&["explain", "0405"], cwd);
    assert_eq!(short.stdout, out.stdout);
    let unknown = wolf(&["explain", "E9999"], cwd);
    assert_eq!(unknown.status.code(), Some(2));
}
