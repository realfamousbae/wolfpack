//! The wolfpack lexer: source text to tokens (spec chapter 1).
//!
//! The lexer is written by hand. It never stops at an error: every problem becomes a
//! diagnostic, the offending text becomes an [`TokenKind::Error`] token, and lexing continues.
//! It never panics on any input.

mod cursor;
mod lexer;
mod token;

use wolf_diag::{Diagnostic, codes};
use wolf_span::{FileId, Span};

pub use token::{Comment, FPart, Token, TokenKind};

/// The result of lexing one file.
#[derive(Debug, Default)]
pub struct Lexed {
    /// The tokens, always ending with [`TokenKind::Eof`].
    pub tokens: Vec<Token>,
    /// Ordinary comments, in source order.
    pub comments: Vec<Comment>,
    /// Lexical errors, in source order.
    pub diagnostics: Vec<Diagnostic>,
}

/// Lexes the text of one file.
pub fn lex(file: FileId, text: &str) -> Lexed {
    lexer::Lexer::new(file, text).run()
}

/// A file whose bytes are not valid UTF-8 (spec chapter 1, rule 1.1).
#[derive(Debug)]
pub struct InvalidUtf8 {
    /// The text with invalid sequences replaced by U+FFFD, for showing the error.
    pub lossy: String,
    /// Byte offset of the first invalid sequence, in both the original bytes and `lossy`.
    pub offset: usize,
}

impl InvalidUtf8 {
    /// The E0101 diagnostic for this file, once `lossy` has been added to the source map as
    /// `file`.
    pub fn diagnostic(&self, file: FileId) -> Diagnostic {
        let start = u32::try_from(self.offset).unwrap_or(u32::MAX);
        // U+FFFD takes 3 bytes in `lossy`, so the label covers a whole character.
        let end = start.saturating_add(3);
        Diagnostic::new(codes::E0101, "file is not valid UTF-8")
            .primary(Span::new(file, start, end), "invalid byte sequence here")
            .note("Wolfpack source files must be encoded in UTF-8")
    }
}

/// Decodes the bytes of a source file. A byte order mark is kept; the lexer skips it.
pub fn decode(bytes: Vec<u8>) -> Result<String, InvalidUtf8> {
    match String::from_utf8(bytes) {
        Ok(text) => Ok(text),
        Err(error) => {
            let offset = error.utf8_error().valid_up_to();
            let lossy = String::from_utf8_lossy(error.as_bytes()).into_owned();
            Err(InvalidUtf8 { lossy, offset })
        }
    }
}

/// Renders tokens one per line, for snapshot tests and `wolf check --emit tokens`.
pub fn dump(text: &str, tokens: &[Token]) -> String {
    let mut out = String::new();
    for token in tokens {
        let source = text.get(token.span.range()).unwrap_or("");
        let kind = match &token.kind {
            TokenKind::Ident => format!("Ident {source:?}"),
            TokenKind::Int(value) => format!("Int {value}"),
            TokenKind::Float(value) => format!("Float {value:?}"),
            TokenKind::Str(value) => format!("Str {value:?}"),
            TokenKind::FStr(parts) => {
                let parts: Vec<String> = parts
                    .iter()
                    .map(|part| match part {
                        FPart::Text(text) => format!("{text:?}"),
                        FPart::Expr(tokens, _) => format!("{{{}}}", tokens.len().saturating_sub(1)),
                    })
                    .collect();
                format!("FStr [{}]", parts.join(", "))
            }
            TokenKind::DocOuter(value) => format!("DocOuter {value:?}"),
            TokenKind::DocInner(value) => format!("DocInner {value:?}"),
            TokenKind::Error => format!("Error {source:?}"),
            other => match other.spelling() {
                Some(spelling) => spelling.to_string(),
                None => format!("{other:?}"),
            },
        };
        out.push_str(&format!(
            "{}..{} {}\n",
            token.span.start, token.span.end, kind
        ));
    }
    out
}
