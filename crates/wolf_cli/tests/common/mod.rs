//! Helpers shared by the tests that check code blocks of Markdown files (`explain.rs`,
//! `guide.rs`).
//!
//! A block is `src/main.wolf` of a `cli` project; a line `// file: <path>` starts another file of
//! the project, which may also be `wolfpack.toml`.

// Test helpers may fail loudly; each test binary uses only some of them.
#![allow(clippy::unwrap_used, dead_code)]

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

/// Runs `wolf` in `cwd`.
pub fn wolf(cwd: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_wolf"))
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap()
}

/// The files of a block: `src/main.wolf` first, then one per `// file: <path>` line.
pub fn files(source: &str) -> Vec<(String, String)> {
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

/// Writes a block as a project and checks it; the codes reported, in order, and the output.
pub fn check(dir: &Path, source: &str) -> (Vec<String>, String) {
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
