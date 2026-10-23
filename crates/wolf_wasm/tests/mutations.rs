//! Programs a token or a line away from the `.wolf` files of this repository go through the whole
//! compiler: the check, the three backends and the formatter. Nothing may panic (every invalid file
//! yields a diagnostic, spec chapter 9), and formatting must be idempotent and keep the codes of the
//! diagnostics.
//!
//! Each file gives `WOLF_MUTATIONS` changed programs (default 2); `WOLF_MUTATIONS=200` with
//! `--release` is a fuzzing run, and `WOLF_MUTATION_SEED=<n>` makes other changes. The programs
//! that fail are written to `target/tmp/mutations/<n>.wolf`, with the problem in `<n>.txt`.

// Test helpers may fail loudly.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::fs;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};

/// Every `.wolf` file of the examples, the tests and the playground.
fn sources() -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if path.is_dir() {
                if name != "target" && name != "out" && !name.starts_with('.') {
                    walk(&path, out);
                }
            } else if name.ends_with(".wolf") {
                out.push(path);
            }
        }
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut out = Vec::new();
    for dir in ["examples", "crates", "playground"] {
        walk(&root.join(dir), &mut out);
    }
    out.sort();
    out
}

/// A small deterministic generator (xorshift64*), so that a failure is found again.
struct Rng(u64);

impl Rng {
    fn below(&mut self, n: usize) -> usize {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        (self.0.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 33) as usize % n.max(1)
    }
}

/// The source with one change: a token deleted, doubled, replaced by another token of the file or
/// swapped with the next one, or a line deleted or doubled.
fn mutate(source: &str, rng: &mut Rng) -> String {
    let mut map = wolf_span::SourceMap::new();
    let id = map.add("m.wolf", source.to_string());
    let tokens: Vec<(usize, usize)> = wolf_lexer::lex(id, source)
        .tokens
        .iter()
        .map(|t| (t.span.start as usize, t.span.end as usize))
        .filter(|(s, e)| s < e)
        .collect();
    let lines: Vec<&str> = source.split_inclusive('\n').collect();
    let text = |(s, e): (usize, usize)| &source[s..e];
    match (rng.below(6), tokens.len()) {
        (0, n) if n > 0 => {
            let (s, e) = tokens[rng.below(n)];
            format!("{}{}", &source[..s], &source[e..])
        }
        (1, n) if n > 0 => {
            let t = tokens[rng.below(n)];
            format!("{} {}{}", &source[..t.1], text(t), &source[t.1..])
        }
        (2, n) if n > 0 => {
            let (s, e) = tokens[rng.below(n)];
            let other = text(tokens[rng.below(n)]);
            format!("{}{other}{}", &source[..s], &source[e..])
        }
        (3, n) if n > 1 => {
            let i = rng.below(n - 1);
            let (a, b) = (tokens[i], tokens[i + 1]);
            format!(
                "{}{}{}{}{}",
                &source[..a.0],
                text(b),
                &source[a.1..b.0],
                text(a),
                &source[b.1..]
            )
        }
        (4, _) if !lines.is_empty() => {
            let k = rng.below(lines.len());
            lines
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != k)
                .map(|(_, l)| *l)
                .collect()
        }
        _ if !lines.is_empty() => {
            let k = rng.below(lines.len());
            let mut out = String::new();
            for (i, l) in lines.iter().enumerate() {
                out.push_str(l);
                if i == k {
                    out.push_str(l);
                }
            }
            out
        }
        _ => source.to_string(),
    }
}

/// The codes of the diagnostics in the JSON of `wolf_wasm::check`, sorted.
fn codes(report: &str) -> Vec<String> {
    let mut out: Vec<String> = report
        .split("\"code\":\"")
        .skip(1)
        .filter_map(|rest| rest.split('"').next())
        .map(str::to_string)
        .collect();
    out.sort();
    out
}

/// Runs the compiler on one program; a problem is returned as its description.
fn exercise(source: &str) -> Option<String> {
    let report = wolf_wasm::check(source);
    for target in ["ts", "go", "rust"] {
        let built = wolf_wasm::build(source, target);
        if built.contains("internal compiler error") {
            return Some(format!("internal compiler error on {target}: {built}"));
        }
    }
    let mut map = wolf_span::SourceMap::new();
    let id = map.add("main.wolf", source.to_string());
    let Ok(formatted) = wolf_fmt::format(id, source) else {
        return None;
    };
    let again_id = map.add("formatted.wolf", formatted.clone());
    match wolf_fmt::format(again_id, &formatted) {
        Ok(again) if again == formatted => {}
        Ok(again) => {
            return Some(format!(
                "formatting is not idempotent:\n--- once\n{formatted}\n--- twice\n{again}"
            ));
        }
        Err(_) => {
            return Some(format!(
                "the formatted program does not parse:\n{formatted}"
            ));
        }
    }
    let after = wolf_wasm::check(&formatted);
    if codes(&report) != codes(&after) {
        return Some(format!(
            "formatting changed the diagnostics from {:?} to {:?}:\n{formatted}",
            codes(&report),
            codes(&after)
        ));
    }
    None
}

/// The kind of a problem: its first line without the program-specific parts of the JSON.
fn kind_of(problem: &str) -> String {
    let first = problem.lines().next().unwrap_or("");
    match first.find("the backend does not support this yet: ") {
        Some(at) => {
            let rest = &first[at..];
            let end = rest
                .find('\\')
                .or_else(|| rest.find('"'))
                .unwrap_or(rest.len());
            format!(
                "{} {}",
                &first[..first.find(':').unwrap_or(0)],
                &rest[..end]
            )
        }
        None => first.chars().take(160).collect(),
    }
}

#[test]
fn programs_near_the_examples_never_break_the_compiler() {
    let per_file: usize = std::env::var("WOLF_MUTATIONS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2);
    let seed: u64 = std::env::var("WOLF_MUTATION_SEED")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let files = sources();
    assert!(files.len() > 100, "found only {} files", files.len());
    // The compiler needs a deep stack for deeply nested input (`wolf` runs on such a thread too).
    let worker = std::thread::Builder::new()
        .stack_size(512 << 20)
        .spawn(move || {
            std::panic::set_hook(Box::new(|_| {}));
            let mut failures = Vec::new();
            for (n, path) in files.iter().enumerate() {
                let Ok(source) = fs::read_to_string(path) else {
                    continue;
                };
                let mut rng = Rng(0x9e37_79b9_7f4a_7c15 ^ (n as u64 + 1) ^ (seed << 32));
                for _ in 0..per_file {
                    let changed = mutate(&source, &mut rng);
                    let problem = match catch_unwind(AssertUnwindSafe(|| exercise(&changed))) {
                        Ok(problem) => problem,
                        Err(payload) => Some(format!(
                            "panic: {}",
                            payload
                                .downcast_ref::<String>()
                                .cloned()
                                .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
                                .unwrap_or_default()
                        )),
                    };
                    if let Some(problem) = problem {
                        failures.push((path.clone(), changed, problem));
                    }
                }
            }
            let _ = std::panic::take_hook();
            failures
        })
        .unwrap();
    let failures = worker.join().unwrap();
    if failures.is_empty() {
        return;
    }
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("mutations");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    // One line for each kind of problem, with the first program that shows it.
    let mut kinds: Vec<(String, usize, usize)> = Vec::new();
    for (i, (path, changed, problem)) in failures.iter().enumerate() {
        fs::write(dir.join(format!("{i}.wolf")), changed).unwrap();
        fs::write(
            dir.join(format!("{i}.txt")),
            format!("from {}\n{problem}\n", path.display()),
        )
        .unwrap();
        let kind: String = kind_of(problem);
        match kinds.iter_mut().find(|(k, _, _)| *k == kind) {
            Some((_, _, count)) => *count += 1,
            None => kinds.push((kind, i, 1)),
        }
    }
    let report: String = kinds
        .iter()
        .map(|(kind, first, count)| format!("{count} x {kind} (first: {first}.wolf)\n"))
        .collect();
    panic!(
        "{} changed programs broke the compiler; they are in {}\n{report}",
        failures.len(),
        dir.display()
    );
}
