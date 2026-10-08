//! The Wolfpack parser: tokens to a syntax tree (spec chapters 2 and 3).
//!
//! Recursive descent for declarations and statements, a Pratt parser for expressions. The parser
//! checks the file skeleton (chapter 2) while it reads the sections. It never stops at an error:
//! it reports the problem, skips to a safe point (the end of the line or a closing delimiter) and
//! continues, so that all errors of a file are shown at once. It never panics, and a nesting limit
//! (E0314) keeps deep input from overflowing the stack.

mod expr;
mod items;
mod skeleton;
mod ty;

use wolf_ast::{File, NodeId};
use wolf_diag::{Diagnostic, codes};
use wolf_lexer::{Token, TokenKind};
use wolf_span::{FileId, Span};

/// The deepest nesting of expressions, statements, types and patterns the parser accepts.
pub const MAX_DEPTH: u32 = 256;

/// The stack size needed to parse, print and drop a tree nested up to [`MAX_DEPTH`] in a debug
/// build, with a wide margin. The `wolf` binary runs the compiler on a thread of this size;
/// other users of the parser should do the same.
pub const STACK_SIZE: usize = 64 * 1024 * 1024;

/// The result of parsing one file.
#[derive(Debug)]
pub struct Parsed {
    /// The syntax tree, possibly with error nodes.
    pub file: File,
    /// Syntax errors, in the order they were found.
    pub diagnostics: Vec<Diagnostic>,
}

/// Parses the tokens of one file, as produced by `wolf_lexer::lex`.
pub fn parse(file: FileId, text: &str, tokens: &[Token]) -> Parsed {
    let mut parser = Parser::new(file, text, tokens);
    let tree = parser.file();
    let too_deep = parser.depth_reported;
    let mut diagnostics = parser.diagnostics;
    if !too_deep {
        diagnostics.extend(skeleton::check(file, &tree));
    }
    Parsed {
        file: tree,
        diagnostics,
    }
}

/// Whether newlines are significant in the current delimiter (spec chapter 1, section 7).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Lines {
    /// Inside a block, a `match`, a declaration or at the top level: a newline ends things.
    Significant,
    /// Inside `( )`, `[ ]` or the braces of a literal or pattern: newlines are ignored.
    Ignored,
}

pub(crate) struct Parser<'a> {
    file: FileId,
    text: &'a str,
    /// The tokens; the parser may split `>>` into two `>` tokens in type context.
    tokens: Vec<Token>,
    pos: usize,
    lines: Vec<Lines>,
    pub(crate) diagnostics: Vec<Diagnostic>,
    next_id: u32,
    depth: u32,
    depth_reported: bool,
    /// Set when a section header was found inside a block: every open block closes at once.
    pub(crate) resync_to_top: bool,
    /// Set when an `Error` token from the lexer was consumed. The rest of the line is skipped
    /// without further messages, because the lexer already reported the problem.
    poisoned: bool,
    /// Error count at the start of the current section entry.
    pub(crate) line_errors: usize,
}

impl<'a> Parser<'a> {
    fn new(file: FileId, text: &'a str, tokens: &[Token]) -> Parser<'a> {
        let mut tokens = tokens.to_vec();
        let ends_with_eof = tokens.last().is_some_and(|t| t.kind == TokenKind::Eof);
        if !ends_with_eof {
            let end = u32::try_from(text.len()).unwrap_or(u32::MAX);
            tokens.push(Token {
                kind: TokenKind::Eof,
                span: Span::new(file, end, end),
            });
        }
        Parser {
            file,
            text,
            tokens,
            pos: 0,
            lines: vec![Lines::Significant],
            diagnostics: Vec::new(),
            next_id: 0,
            depth: 0,
            depth_reported: false,
            resync_to_top: false,
            poisoned: false,
            line_errors: 0,
        }
    }

    // ----- token access ----------------------------------------------------------------------

    /// The token `n` positions ahead of `pos`, without skipping anything. Clamped to `Eof`.
    fn raw(&self, n: usize) -> &Token {
        let index = (self.pos + n).min(self.tokens.len().saturating_sub(1));
        // `tokens` always ends with Eof, so it is never empty.
        match self.tokens.get(index) {
            Some(token) => token,
            None => &EOF_TOKEN,
        }
    }

    fn ignoring_lines(&self) -> bool {
        self.lines.last() == Some(&Lines::Ignored)
    }

    /// Index of the `n`-th next token that is visible in the current newline mode.
    fn visible_index(&self, n: usize) -> usize {
        let mut index = self.pos;
        let mut seen = 0;
        loop {
            let token = self.tokens.get(index);
            let skip = self.ignoring_lines() && token.is_some_and(|t| t.kind == TokenKind::Newline);
            if !skip {
                if seen == n || token.is_none_or(|t| t.kind == TokenKind::Eof) {
                    return index.min(self.tokens.len().saturating_sub(1));
                }
                seen += 1;
            }
            index += 1;
        }
    }

    /// The `n`-th next visible token.
    pub(crate) fn nth(&self, n: usize) -> &Token {
        match self.tokens.get(self.visible_index(n)) {
            Some(token) => token,
            None => &EOF_TOKEN,
        }
    }

    pub(crate) fn peek(&self) -> &TokenKind {
        &self.nth(0).kind
    }

    pub(crate) fn peek_span(&self) -> Span {
        self.nth(0).span
    }

    /// Whether the next visible token has the same kind as `kind` (literal values are ignored).
    pub(crate) fn at(&self, kind: &TokenKind) -> bool {
        same_kind(self.peek(), kind)
    }

    pub(crate) fn nth_is(&self, n: usize, kind: &TokenKind) -> bool {
        same_kind(&self.nth(n).kind, kind)
    }

    /// Whether the next visible token is the identifier `word` (a contextual keyword).
    pub(crate) fn at_word(&self, word: &str) -> bool {
        self.nth_word(0, word)
    }

    pub(crate) fn nth_word(&self, n: usize, word: &str) -> bool {
        let token = self.nth(n);
        token.kind == TokenKind::Ident && self.source(token.span) == word
    }

    /// Consumes the next visible token and returns it.
    pub(crate) fn bump(&mut self) -> Token {
        let index = self.visible_index(0);
        let token = self.tokens.get(index).cloned().unwrap_or(EOF_TOKEN.clone());
        if token.kind == TokenKind::Error {
            self.poisoned = true;
        }
        if token.kind != TokenKind::Eof {
            self.pos = index + 1;
        } else {
            self.pos = index;
        }
        token
    }

    pub(crate) fn eat(&mut self, kind: &TokenKind) -> bool {
        if self.at(kind) {
            self.bump();
            true
        } else {
            false
        }
    }

    /// Consumes `kind` and then any newlines: after `.`, `:`, `->`, `=` and binary operators a
    /// line break does not end anything (spec chapter 1, section 7, rule 2).
    pub(crate) fn eat_then_lines(&mut self, kind: &TokenKind) -> bool {
        if self.eat(kind) {
            self.skip_newlines();
            true
        } else {
            false
        }
    }

    /// Like [`Parser::expect`], then skips newlines.
    pub(crate) fn expect_then_lines(&mut self, kind: &TokenKind, what: &str) -> Option<Span> {
        let span = self.expect(kind, what)?;
        self.skip_newlines();
        Some(span)
    }

    pub(crate) fn eat_word(&mut self, word: &str) -> bool {
        if self.at_word(word) {
            self.bump();
            true
        } else {
            false
        }
    }

    /// Consumes `kind` or reports E0301. Returns the span of the token when it was there.
    pub(crate) fn expect(&mut self, kind: &TokenKind, what: &str) -> Option<Span> {
        if self.at(kind) {
            return Some(self.bump().span);
        }
        self.unexpected(what);
        None
    }

    /// Skips newline tokens, even where they are significant.
    pub(crate) fn skip_newlines(&mut self) {
        while self.raw(0).kind == TokenKind::Newline {
            self.pos += 1;
        }
    }

    /// Whether the next raw token ends the current line (or the file).
    pub(crate) fn at_line_end(&self) -> bool {
        matches!(self.raw(0).kind, TokenKind::Newline | TokenKind::Eof)
    }

    pub(crate) fn source(&self, span: Span) -> &'a str {
        self.text.get(span.range()).unwrap_or("")
    }

    pub(crate) fn prev_span(&self) -> Span {
        let index = self.pos.saturating_sub(1);
        self.tokens
            .get(index)
            .map(|t| t.span)
            .unwrap_or(Span::new(self.file, 0, 0))
    }

    pub(crate) fn span_from(&self, start: Span) -> Span {
        let end = self.prev_span();
        if end.end < start.start {
            start
        } else {
            start.to(end)
        }
    }

    pub(crate) fn with_lines<T>(&mut self, mode: Lines, f: impl FnOnce(&mut Self) -> T) -> T {
        self.lines.push(mode);
        let result = f(self);
        self.lines.pop();
        result
    }

    pub(crate) fn ignore_lines<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        self.with_lines(Lines::Ignored, f)
    }

    pub(crate) fn significant_lines<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        self.with_lines(Lines::Significant, f)
    }

    pub(crate) fn id(&mut self) -> NodeId {
        let id = NodeId(self.next_id);
        self.next_id += 1;
        id
    }

    // ----- errors -----------------------------------------------------------------------------

    pub(crate) fn report(&mut self, diagnostic: Diagnostic) {
        // After the nesting limit the parser skips to the end; anything else would be noise.
        if self.depth_reported && diagnostic.code != codes::E0314 {
            return;
        }
        self.diagnostics.push(diagnostic);
    }

    /// Reports E0301 at the next token, unless it is an `Error` token (already reported by the
    /// lexer) or the nesting limit was hit.
    pub(crate) fn unexpected(&mut self, expected: &str) {
        let token = self.nth(0).clone();
        if token.kind == TokenKind::Error || self.depth_reported || self.poisoned {
            return;
        }
        let found = describe(&token, self.source(token.span));
        // Point at the end of the line, not at the line break itself.
        let span = if token.kind == TokenKind::Newline {
            Span::new(token.span.file, token.span.start, token.span.start)
        } else {
            token.span
        };
        self.report(
            Diagnostic::new(codes::E0301, format!("expected {expected}, found {found}"))
                .primary(span, format!("expected {expected}")),
        );
    }

    /// Ends the current line after a statement or entry. If the line contained a lexer error,
    /// the rest of it is skipped silently and `true` is returned.
    pub(crate) fn end_line_if_poisoned(&mut self) -> bool {
        if !self.poisoned {
            return false;
        }
        self.recover();
        self.poisoned = false;
        true
    }

    /// Clears the poisoned state at the start of a line and remembers how many errors there
    /// were, so the end of the line can tell whether this line already reported one.
    pub(crate) fn start_line(&mut self) -> usize {
        self.poisoned = false;
        self.diagnostics.len()
    }

    /// At the end of a statement or entry: if the line already has an error, or the next token
    /// is a range (E0307), handle that and skip the rest of the line. Returns `true` if handled.
    pub(crate) fn end_line_after_error(&mut self, errors_before: usize) -> bool {
        // A section header was found inside a block: stay right there.
        if self.resync_to_top {
            return true;
        }
        if self.end_line_if_poisoned() {
            return true;
        }
        if matches!(self.raw(0).kind, TokenKind::DotDot | TokenKind::DotDotEq) {
            let span = self.raw(0).span;
            self.report(
                Diagnostic::new(codes::E0307, "range outside `for` or index brackets").primary(
                    span,
                    "ranges are allowed only in `for i in a..b` and `items[a..b]`",
                ),
            );
            self.recover();
            return true;
        }
        if self.diagnostics.len() > errors_before {
            self.recover();
            return true;
        }
        false
    }

    /// Skips to a safe point: the next newline outside nested delimiters, or before a closing
    /// delimiter that belongs to an enclosing construct.
    pub(crate) fn recover(&mut self) {
        let mut depth = 0u32;
        loop {
            let kind = self.raw(0).kind.clone();
            match kind {
                TokenKind::Eof => return,
                TokenKind::Newline if depth == 0 && !self.ignoring_lines() => return,
                TokenKind::LParen | TokenKind::LBracket | TokenKind::LBrace => depth += 1,
                TokenKind::RParen | TokenKind::RBracket | TokenKind::RBrace => {
                    if depth == 0 {
                        return;
                    }
                    depth -= 1;
                }
                _ => {}
            }
            self.pos += 1;
        }
    }

    /// Enters one nesting level. Returns `false` (and reports E0314 once) past the limit.
    pub(crate) fn enter(&mut self) -> bool {
        self.depth += 1;
        if self.depth <= MAX_DEPTH {
            return true;
        }
        if !self.depth_reported {
            self.depth_reported = true;
            let span = self.peek_span();
            self.report(
                Diagnostic::new(codes::E0314, "nesting too deep")
                    .primary(span, format!("more than {MAX_DEPTH} nested levels"))
                    .help("split the expression or the code into smaller parts"),
            );
        }
        false
    }

    pub(crate) fn leave(&mut self) {
        self.depth = self.depth.saturating_sub(1);
    }

    /// Skips everything up to the end of the file. Used after the nesting limit was hit.
    pub(crate) fn give_up(&mut self) {
        self.pos = self.tokens.len().saturating_sub(1);
    }

    pub(crate) fn too_deep(&self) -> bool {
        self.depth_reported
    }
}

static EOF_TOKEN: Token = Token {
    kind: TokenKind::Eof,
    span: Span {
        file: FILE_ZERO,
        start: 0,
        end: 0,
    },
};

const FILE_ZERO: FileId = wolf_span::FileId::ZERO;

/// Compares token kinds without looking at literal values.
pub(crate) fn same_kind(a: &TokenKind, b: &TokenKind) -> bool {
    std::mem::discriminant(a) == std::mem::discriminant(b)
}

/// "identifier `foo`", "`}`", "new line" for messages.
pub(crate) fn describe(token: &Token, source: &str) -> String {
    match token.kind {
        TokenKind::Ident => format!("identifier `{source}`"),
        TokenKind::Int(_) | TokenKind::Float(_) | TokenKind::Str(_) => format!("`{source}`"),
        _ => token.kind.to_string(),
    }
}
