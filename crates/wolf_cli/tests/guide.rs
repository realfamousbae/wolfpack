//! Tests of the code in the language guide, `docs/guide/*.md`.
//!
//! - A ```` ```wolf ```` block is a complete project (see `common`). It must check without errors,
//!   be formatted as `wolf fmt` writes it, and build for every target.
//! - An ```` ```output ```` block belongs to the `wolf` block before it: that program is run on
//!   TypeScript and must print exactly this text.
//! - A ```` ```wolf error E0501 ```` block must report that code first.
//! - A ```` ```wolf fragment ```` block is a piece of code that is not checked.
//!
//! These tests need `node` and `tsc` on `PATH`, like the other end-to-end tests.

// Test helpers may fail loudly.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use common::{check, wolf};

/// A checked block of a guide file.
enum Block {
    Program {
        source: String,
        output: Option<String>,
    },
    Error {
        code: String,
        source: String,
    },
}

/// The checked blocks of a file, or the problems with its fences.
fn blocks(text: &str, failures: &mut Vec<String>, file: &str) -> Vec<Block> {
    let mut found = Vec::new();
    let mut fence: Option<(String, String)> = None;
    for line in text.lines() {
        match &mut fence {
            None => {
                if let Some(info) = line.strip_prefix("```") {
                    fence = Some((info.to_string(), String::new()));
                }
            }
            Some(_) if line == "```" => {
                let (info, body) = fence.take().unwrap();
                match info.as_str() {
                    "wolf" => found.push(Block::Program {
                        source: body,
                        output: None,
                    }),
                    "output" => match found.last_mut() {
                        Some(Block::Program {
                            output: o @ None, ..
                        }) => *o = Some(body),
                        _ => failures.push(format!("{file}: `output` without a program before it")),
                    },
                    "wolf fragment" => {}
                    _ if info.starts_with("wolf error E")
                        && info.len() == "wolf error E0000".len() =>
                    {
                        found.push(Block::Error {
                            code: info["wolf error ".len()..].to_string(),
                            source: body,
                        })
                    }
                    _ if info.starts_with("wolf") || info == "output" => {
                        failures.push(format!("{file}: unknown fence `{info}`"));
                    }
                    _ => {}
                }
            }
            Some((_, body)) => {
                body.push_str(line);
                body.push('\n');
            }
        }
    }
    if fence.is_some() {
        failures.push(format!("{file}: unclosed fence"));
    }
    found
}

fn guide_files() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/guide");
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "md"))
        .collect();
    files.sort();
    files
}

fn tmp(name: &str) -> PathBuf {
    Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("guide")
        .join(name)
}

/// Checks a program block; the problems found.
fn program(dir: &Path, source: &str, output: Option<&str>) -> Vec<String> {
    let (codes, stderr) = check(dir, source);
    if !codes.is_empty() {
        return vec![format!("reports {codes:?}\n{stderr}")];
    }
    let mut problems = Vec::new();
    let fmt = wolf(dir, &["fmt", "--check"]);
    if !fmt.status.success() {
        problems.push(format!(
            "is not formatted\n{}",
            String::from_utf8_lossy(&fmt.stdout)
        ));
    }
    let out = dir.with_extension("out");
    let _ = fs::remove_dir_all(&out);
    let build = wolf(
        dir,
        &["build", "--target", "all", "--out", out.to_str().unwrap()],
    );
    if !build.status.success() {
        problems.push(format!(
            "does not build\n{}",
            String::from_utf8_lossy(&build.stderr)
        ));
    }
    if let Some(expected) = output {
        let run_out = dir.with_extension("run");
        let run = wolf(
            dir,
            &["run", "--target", "ts", "--out", run_out.to_str().unwrap()],
        );
        let stdout = String::from_utf8_lossy(&run.stdout);
        if stdout != expected || !run.status.success() {
            problems.push(format!(
                "prints\n{stdout}\nexpected\n{expected}\n{}",
                String::from_utf8_lossy(&run.stderr)
            ));
        }
    }
    problems
}

#[test]
fn guide_examples_check_build_and_print_their_output() {
    let mut failures = Vec::new();
    let files = guide_files();
    assert!(!files.is_empty());
    for path in files {
        let name = path.file_stem().unwrap().to_string_lossy().into_owned();
        let text = fs::read_to_string(&path).unwrap();
        for (n, block) in blocks(&text, &mut failures, &name).iter().enumerate() {
            let dir = tmp(&format!("{name}-{n}"));
            match block {
                Block::Program { source, output } => {
                    for problem in program(&dir, source, output.as_deref()) {
                        failures.push(format!("{name}, block {n}: {problem}"));
                    }
                }
                Block::Error { code, source } => {
                    let (codes, stderr) = check(&dir, source);
                    if codes.first() != Some(code) {
                        failures.push(format!(
                            "{name}, block {n}: expected {code}, reports {codes:?}\n{stderr}"
                        ));
                    }
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
