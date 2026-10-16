//! Tests of the code in the language guide, `docs/guide/*.md`, and in the `README.md` of the
//! repository.
//!
//! - A ```` ```wolf ```` block is a complete project (see `common`). It must check without errors,
//!   be formatted as `wolf fmt` writes it, and build for every target.
//! - An ```` ```output ```` block belongs to the `wolf` block before it: that program is run on
//!   TypeScript, Go and Rust and must print exactly this text on each.
//! - A ```` ```ts generated src/main.ts ```` block (or `go`, `rust`) belongs to the `wolf` block
//!   before it: the build of that program for the target must produce exactly this file.
//! - A ```` ```wolf error E0501 ```` block must report that code first.
//! - A ```` ```wolf fragment ```` block is a piece of code that is not checked.
//!
//! These tests need `node`, `tsc`, `go` and `cargo` on `PATH`, like the other end-to-end tests.

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
        /// Generated files shown in the guide: target, path in `out/<target>/`, text.
        generated: Vec<(String, String, String)>,
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
                        generated: Vec::new(),
                    }),
                    "output" => match found.last_mut() {
                        Some(Block::Program {
                            output: o @ None, ..
                        }) => *o = Some(body),
                        _ => failures.push(format!("{file}: `output` without a program before it")),
                    },
                    "wolf fragment" => {}
                    _ if generated(&info).is_some() => {
                        let (target, path) = generated(&info).unwrap();
                        match found.last_mut() {
                            Some(Block::Program { generated: g, .. }) => {
                                g.push((target, path, body))
                            }
                            _ => failures
                                .push(format!("{file}: `{info}` without a program before it")),
                        }
                    }
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

/// `ts generated src/main.ts` gives the target and the path.
fn generated(info: &str) -> Option<(String, String)> {
    let mut words = info.split(' ');
    let target = words.next()?;
    if !["ts", "go", "rust"].contains(&target) || words.next()? != "generated" {
        return None;
    }
    let path = words.next()?;
    Some((target.to_string(), path.to_string()))
}

fn guide_files() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/guide");
    let mut files: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "md"))
        .collect();
    files.sort();
    // The README of the repository follows the same rules.
    files.push(dir.join("../../README.md"));
    files
}

fn tmp(name: &str) -> PathBuf {
    Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("guide")
        .join(name)
}

/// Checks a program block; the problems found.
fn program(
    dir: &Path,
    source: &str,
    output: Option<&str>,
    generated: &[(String, String, String)],
) -> Vec<String> {
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
    for (target, path, expected) in generated {
        let actual = fs::read_to_string(out.join(target).join(path)).unwrap_or_default();
        if &actual != expected {
            problems.push(format!("generates another {target} {path}:\n{actual}"));
        }
    }
    if let Some(expected) = output {
        for target in ["ts", "go", "rust"] {
            let run_out = dir.with_extension(format!("run-{target}"));
            let run = wolf(
                dir,
                &[
                    "run",
                    "--target",
                    target,
                    "--out",
                    run_out.to_str().unwrap(),
                ],
            );
            let stdout = String::from_utf8_lossy(&run.stdout);
            if stdout != expected || !run.status.success() {
                problems.push(format!(
                    "prints on {target}\n{stdout}\nexpected\n{expected}\n{}",
                    String::from_utf8_lossy(&run.stderr)
                ));
            }
        }
    }
    problems
}

/// Checks every block of one guide file; the problems found.
fn check_file(path: &Path) -> Vec<String> {
    let mut failures = Vec::new();
    let name = path.file_stem().unwrap().to_string_lossy().into_owned();
    let text = fs::read_to_string(path).unwrap();
    for (n, block) in blocks(&text, &mut failures, &name).iter().enumerate() {
        let dir = tmp(&format!("{name}-{n}"));
        match block {
            Block::Program {
                source,
                output,
                generated,
            } => {
                for problem in program(&dir, source, output.as_deref(), generated) {
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
    failures
}

#[test]
fn guide_examples_check_build_and_print_their_output() {
    let files = guide_files();
    assert!(!files.is_empty());
    // One thread per chapter: running the programs on three targets is slow.
    let failures: Vec<String> = std::thread::scope(|scope| {
        let handles: Vec<_> = files
            .iter()
            .map(|path| scope.spawn(move || check_file(path)))
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().unwrap())
            .collect()
    });
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The examples of the playground (`playground/examples/*.wolf`) are programs like those of the
/// guide: they check, are formatted, build for every target and run on TypeScript.
#[test]
fn playground_examples_check_build_and_run() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../playground/examples");
    let mut files: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "wolf"))
        .collect();
    files.sort();
    assert!(!files.is_empty());
    let mut failures = Vec::new();
    for path in files {
        let name = path.file_stem().unwrap().to_string_lossy().into_owned();
        let source = fs::read_to_string(&path).unwrap();
        let project = tmp(&format!("playground-{name}"));
        for problem in program(&project, &source, None, &[]) {
            failures.push(format!("playground example {name}: {problem}"));
        }
        let run_out = project.with_extension("run");
        let run = wolf(&project, &["run", "--out", run_out.to_str().unwrap()]);
        if !run.status.success() {
            failures.push(format!(
                "playground example {name} does not run\n{}",
                String::from_utf8_lossy(&run.stderr)
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
