//! Acceptance tests: the M0 examples lex cleanly, a token stream snapshot, and the lexer plus
//! diagnostic rendering never panic on arbitrary input.

use std::fs;
use std::path::{Path, PathBuf};

use wolf_lexer::{dump, lex};
use wolf_span::SourceMap;

fn wolf_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            wolf_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "wolf") {
            out.push(path);
        }
    }
}

#[test]
fn every_example_lexes_without_errors() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let mut files = Vec::new();
    wolf_files(&root, &mut files);
    files.sort();
    assert!(
        files.len() >= 13,
        "examples not found in {}",
        root.display()
    );
    for path in files {
        let text = fs::read_to_string(&path).unwrap();
        let mut map = SourceMap::new();
        let id = map.add(path.display().to_string(), text.clone());
        let lexed = lex(id, &text);
        let rendered = wolf_diag::render_human_to_string(&map, &lexed.diagnostics);
        assert!(lexed.diagnostics.is_empty(), "{rendered}");
    }
}

#[test]
fn token_stream_snapshot() {
    let text = r#"//! Module docs.
module shapes

types {
    /// A shape.
    pub enum Shape {
        Circle(Float)   // radius
        Rect { w: Float, h: Float }
    }
}

functions {
    pub fn area(s: Shape) -> Float {
        match s {
            Shape.Circle(r) => 3.14 * r * r
            Shape.Rect { w, h } => w * h
        }
    }

    fn count(items: List<List<Int>>) -> Int {
        var n = 0x10
        for i in 0..=items.len() { n += i }
        n >>= 1
        n
    }
}
"#;
    let mut map = SourceMap::new();
    let id = map.add("snapshot.wolf", text.to_string());
    let lexed = lex(id, text);
    assert!(lexed.diagnostics.is_empty());
    insta::assert_snapshot!(dump(text, &lexed.tokens));
}

/// A small deterministic generator, so the test needs no dependency and is reproducible.
struct XorShift(u64);

impl XorShift {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

#[test]
fn arbitrary_input_never_panics() {
    // Bytes biased towards characters that matter to the lexer.
    const ALPHABET: &[u8] =
        b"/*!\"\\{}xu0123456789._eE+-'\r\n\t ;@#abcz\xc3\xa9\xff\xd0\xbf\xe2\x80";
    let mut rng = XorShift(0x9e37_79b9_7f4a_7c15);
    for _ in 0..3000 {
        let len = (rng.next() % 64) as usize;
        let bytes: Vec<u8> = (0..len)
            .map(|_| {
                let r = rng.next();
                if r % 7 == 0 {
                    (r >> 8) as u8
                } else {
                    ALPHABET[(r >> 8) as usize % ALPHABET.len()]
                }
            })
            .collect();
        let mut map = SourceMap::new();
        let (id, diagnostics) = match wolf_lexer::decode(bytes) {
            Ok(text) => {
                let id = map.add("fuzz.wolf", text.clone());
                let lexed = lex(id, &text);
                assert!(matches!(
                    lexed.tokens.last().map(|t| &t.kind),
                    Some(wolf_lexer::TokenKind::Eof)
                ));
                (id, lexed.diagnostics)
            }
            Err(error) => {
                let id = map.add("fuzz.wolf", error.lossy.clone());
                let diagnostic = error.diagnostic(id);
                (id, vec![diagnostic])
            }
        };
        let _ = id;
        let _ = wolf_diag::render_human_to_string(&map, &diagnostics);
        let _ = wolf_diag::render_json(&map, &diagnostics);
    }
}
