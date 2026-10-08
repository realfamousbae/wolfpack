//! The lexer state machine.

use wolf_diag::{Diagnostic, codes};
use wolf_span::{FileId, Span};

use crate::Lexed;
use crate::cursor::Cursor;
use crate::token::{Comment, Token, TokenKind};

/// The largest integer literal: 2^63, valid only directly under a unary minus.
const INT_LIMIT: u128 = 1 << 63;

pub(crate) struct Lexer<'a> {
    file: FileId,
    text: &'a str,
    cursor: Cursor,
    out: Lexed,
}

impl<'a> Lexer<'a> {
    pub(crate) fn new(file: FileId, text: &'a str) -> Lexer<'a> {
        Lexer {
            file,
            text,
            cursor: Cursor::new(text),
            out: Lexed::default(),
        }
    }

    pub(crate) fn run(mut self) -> Lexed {
        // A byte order mark is ignored, but only at the very start (rule 1.2).
        self.cursor.eat('\u{feff}');
        while let Some(c) = self.cursor.peek(0) {
            let start = self.cursor.offset();
            match c {
                ' ' | '\t' => {
                    self.cursor.bump();
                }
                '\n' => {
                    self.cursor.bump();
                    self.line_end(start);
                }
                '\r' => {
                    self.cursor.bump();
                    if self.cursor.eat('\n') {
                        self.line_end(start);
                    } else {
                        self.lone_cr(start);
                    }
                }
                '/' if self.cursor.peek(1) == Some('/') => self.line_comment(start),
                '/' if self.cursor.peek(1) == Some('*') => self.block_comment(start),
                '"' => self.string(start),
                '0'..='9' => self.number(start),
                c if is_ident_start(c) => self.ident(start),
                _ => self.punctuation(start, c),
            }
        }
        let end = self.cursor.offset();
        self.push(TokenKind::Eof, end, end);
        self.out
    }

    // ----- helpers -------------------------------------------------------------------------

    fn span(&self, start: u32, end: u32) -> Span {
        Span::new(self.file, start, end)
    }

    fn push(&mut self, kind: TokenKind, start: u32, end: u32) {
        let span = self.span(start, end);
        self.out.tokens.push(Token { kind, span });
    }

    fn error(&mut self, diagnostic: Diagnostic) {
        self.out.diagnostics.push(diagnostic);
    }

    fn source(&self, start: u32, end: u32) -> &'a str {
        self.text.get(start as usize..end as usize).unwrap_or("")
    }

    /// Records a line end. Consecutive line ends and comment-only lines collapse into one
    /// `Newline` token, and no `Newline` is emitted before the first token (section 7).
    fn line_end(&mut self, start: u32) {
        let end = self.cursor.offset();
        match self.out.tokens.last() {
            None => {}
            Some(token) if token.kind == TokenKind::Newline => {}
            Some(_) => self.push(TokenKind::Newline, start, end),
        }
    }

    fn lone_cr(&mut self, start: u32) {
        let span = self.span(start, start + 1);
        self.error(
            Diagnostic::new(codes::E0110, "carriage return without line feed")
                .primary(span, "a line must end with `\\n` or `\\r\\n`")
                .help("save the file with LF or CRLF line endings"),
        );
    }

    // ----- comments ------------------------------------------------------------------------

    fn line_comment(&mut self, start: u32) {
        // `//` already checked.
        self.cursor.bump();
        self.cursor.bump();
        let kind = match (self.cursor.peek(0), self.cursor.peek(1)) {
            (Some('/'), Some('/')) => LineKind::Plain,
            (Some('/'), _) => LineKind::Outer,
            (Some('!'), _) => LineKind::Inner,
            _ => LineKind::Plain,
        };
        if kind != LineKind::Plain {
            self.cursor.bump();
        }
        let content_start = self.cursor.offset();
        while let Some(c) = self.cursor.peek(0) {
            if c == '\n' || (c == '\r' && self.cursor.peek(1) == Some('\n')) {
                break;
            }
            let at = self.cursor.offset();
            self.cursor.bump();
            if c == '\r' {
                self.lone_cr(at);
            }
        }
        let end = self.cursor.offset();
        let content = self.source(content_start, end).to_string();
        match kind {
            LineKind::Plain => self.out.comments.push(Comment {
                span: self.span(start, end),
                block: false,
            }),
            LineKind::Outer => self.push(TokenKind::DocOuter(content), start, end),
            LineKind::Inner => self.push(TokenKind::DocInner(content), start, end),
        }
    }

    fn block_comment(&mut self, start: u32) {
        // `/*` already checked.
        self.cursor.bump();
        self.cursor.bump();
        let kind = match (self.cursor.peek(0), self.cursor.peek(1)) {
            // `/**/` and `/***` are ordinary comments.
            (Some('*'), Some('*')) | (Some('*'), Some('/')) => LineKind::Plain,
            (Some('*'), _) => LineKind::Outer,
            (Some('!'), _) => LineKind::Inner,
            _ => LineKind::Plain,
        };
        if kind != LineKind::Plain {
            self.cursor.bump();
        }
        let content_start = self.cursor.offset();
        let mut depth = 1u32;
        let mut has_newline = false;
        let mut content_end = content_start;
        while depth > 0 {
            let at = self.cursor.offset();
            match (self.cursor.peek(0), self.cursor.peek(1)) {
                (None, _) => {
                    let span = self.span(start, start + 2);
                    self.error(
                        Diagnostic::new(codes::E0112, "unterminated block comment")
                            .primary(span, "this comment is never closed")
                            .note("block comments nest: every `/*` needs its own `*/`"),
                    );
                    content_end = at;
                    break;
                }
                (Some('*'), Some('/')) => {
                    self.cursor.bump();
                    self.cursor.bump();
                    depth -= 1;
                    content_end = at;
                }
                (Some('/'), Some('*')) => {
                    self.cursor.bump();
                    self.cursor.bump();
                    depth += 1;
                }
                (Some('\n'), _) => {
                    self.cursor.bump();
                    has_newline = true;
                }
                (Some('\r'), next) => {
                    self.cursor.bump();
                    if next == Some('\n') {
                        self.cursor.bump();
                        has_newline = true;
                    } else {
                        self.lone_cr(at);
                    }
                }
                (Some(_), _) => {
                    self.cursor.bump();
                }
            }
        }
        let end = self.cursor.offset();
        let content = self.source(content_start, content_end).to_string();
        match kind {
            LineKind::Plain => {
                self.out.comments.push(Comment {
                    span: self.span(start, end),
                    block: true,
                });
                // A block comment containing a line end counts as a line end (section 2).
                if has_newline {
                    self.line_end(start);
                }
            }
            LineKind::Outer => self.push(TokenKind::DocOuter(content), start, end),
            LineKind::Inner => self.push(TokenKind::DocInner(content), start, end),
        }
    }

    // ----- identifiers and keywords --------------------------------------------------------

    fn ident(&mut self, start: u32) {
        let mut non_ascii = false;
        while let Some(c) = self.cursor.peek(0) {
            if is_ident_continue(c) {
                non_ascii |= !c.is_ascii();
                self.cursor.bump();
            } else {
                break;
            }
        }
        let end = self.cursor.offset();
        let text = self.source(start, end);
        if non_ascii {
            let span = self.span(start, end);
            self.error(
                Diagnostic::new(
                    codes::E0111,
                    format!("non-ASCII character in identifier `{text}`"),
                )
                .primary(
                    span,
                    "identifiers may only contain ASCII letters, digits and `_`",
                )
                .note("names are translated to the naming style of each target, which needs ASCII"),
            );
            self.push(TokenKind::Error, start, end);
            return;
        }
        if self.cursor.peek(0) == Some('"') {
            // `f"..."`, `r"..."`: reserved for future string forms (rule 5.3.5).
            let span = self.span(start, end + 1);
            self.error(
                Diagnostic::new(codes::E0113, format!("reserved literal prefix `{text}`"))
                    .primary(span, "an identifier directly before `\"` is reserved")
                    .help(format!("add a space after `{text}` or remove the prefix")),
            );
            self.push(TokenKind::Error, start, end);
            return;
        }
        let kind = if text == "_" {
            TokenKind::Underscore
        } else {
            TokenKind::keyword(text).unwrap_or(TokenKind::Ident)
        };
        self.push(kind, start, end);
    }

    // ----- numbers -------------------------------------------------------------------------

    fn number(&mut self, start: u32) {
        let mut problems = Vec::new();
        let radix = match (self.cursor.peek(0), self.cursor.peek(1)) {
            (Some('0'), Some(p @ ('x' | 'o' | 'b' | 'X' | 'O' | 'B'))) => {
                self.cursor.bump();
                self.cursor.bump();
                if p.is_ascii_uppercase() {
                    problems.push(format!(
                        "the prefix must be lower case: `0{}`",
                        p.to_ascii_lowercase()
                    ));
                }
                match p.to_ascii_lowercase() {
                    'x' => 16,
                    'o' => 8,
                    _ => 2,
                }
            }
            _ => 10,
        };

        let digits = self.digits(radix, &mut problems);
        if radix != 10 {
            if digits.is_empty() {
                problems.push("expected digits after the prefix".to_string());
            }
            self.suffix(&mut problems);
            let end = self.cursor.offset();
            self.finish_int(start, end, &digits, radix, problems);
            return;
        }

        if digits.len() > 1 && digits.starts_with('0') {
            problems
                .push("leading zeros are not allowed; octal numbers are written `0o7`".to_string());
        }

        let mut is_float = false;
        let mut float_text = digits.clone();
        // A fraction needs a digit after the dot; `1..5` and `1.copy()` are not floats.
        if self.cursor.peek(0) == Some('.') {
            match self.cursor.peek(1) {
                Some(d) if d.is_ascii_digit() => {
                    self.cursor.bump();
                    let fraction = self.digits(10, &mut problems);
                    float_text.push('.');
                    float_text.push_str(&fraction);
                    is_float = true;
                }
                Some(c) if c == '.' || is_ident_start(c) => {}
                _ => {
                    self.cursor.bump();
                    problems
                        .push("a float needs a digit after the dot, for example `1.0`".to_string());
                }
            }
        }
        if let Some('e' | 'E') = self.cursor.peek(0) {
            let sign = matches!(self.cursor.peek(1), Some('+' | '-'));
            let digit_at = if sign { 2 } else { 1 };
            if self
                .cursor
                .peek(digit_at)
                .is_some_and(|c| c.is_ascii_digit())
            {
                self.cursor.bump();
                float_text.push('e');
                if sign {
                    if let Some(s) = self.cursor.bump() {
                        float_text.push(s);
                    }
                }
                let exponent = self.digits(10, &mut problems);
                float_text.push_str(&exponent);
                is_float = true;
            } else {
                self.cursor.bump();
                if sign {
                    self.cursor.bump();
                }
                problems.push("an exponent needs digits, for example `1e10`".to_string());
            }
        }
        self.suffix(&mut problems);
        let end = self.cursor.offset();

        if !problems.is_empty() || !is_float {
            self.finish_int(start, end, &digits, 10, problems);
            return;
        }
        match float_text.parse::<f64>() {
            Ok(value) if value.is_finite() => self.push(TokenKind::Float(value), start, end),
            _ => {
                let span = self.span(start, end);
                self.error(
                    Diagnostic::new(codes::E0108, "float literal out of range")
                        .primary(span, "this value is too large for `Float`")
                        .note("the largest `Float` is about 1.8e308"),
                );
                self.push(TokenKind::Error, start, end);
            }
        }
    }

    /// Reads digits of `radix` with `_` separators. Returns the digits without separators.
    fn digits(&mut self, radix: u32, problems: &mut Vec<String>) -> String {
        let mut out = String::new();
        let mut bad_separator = false;
        while let Some(c) = self.cursor.peek(0) {
            if c.is_digit(radix) {
                out.push(c);
                self.cursor.bump();
            } else if c == '_' {
                let before = self.cursor.prev().is_some_and(|p| p.is_digit(radix));
                let after = self.cursor.peek(1).is_some_and(|n| n.is_digit(radix));
                // Only a separator between two digits is allowed (rule 5.1.2).
                if !(before && after) || out.is_empty() {
                    bad_separator = true;
                }
                self.cursor.bump();
            } else {
                break;
            }
        }
        if bad_separator {
            problems.push("`_` may only stand between two digits".to_string());
        }
        out
    }

    /// Consumes letters and digits glued to a number (reserved for type suffixes, rule 5.1.5).
    fn suffix(&mut self, problems: &mut Vec<String>) {
        let start = self.cursor.offset();
        while self.cursor.peek(0).is_some_and(is_ident_continue) {
            self.cursor.bump();
        }
        let end = self.cursor.offset();
        if end > start {
            let text = self.source(start, end);
            problems.push(format!(
                "unexpected `{text}` after the number; separate them with a space"
            ));
        }
    }

    fn finish_int(
        &mut self,
        start: u32,
        end: u32,
        digits: &str,
        radix: u32,
        problems: Vec<String>,
    ) {
        let span = self.span(start, end);
        if let Some(first) = problems.into_iter().next() {
            let text = self.source(start, end);
            self.error(
                Diagnostic::new(codes::E0107, format!("malformed number literal `{text}`"))
                    .primary(span, first),
            );
            self.push(TokenKind::Error, start, end);
            return;
        }
        let mut value: u128 = 0;
        for c in digits.chars() {
            let digit = c.to_digit(radix).map(u128::from).unwrap_or(0);
            value = value
                .saturating_mul(u128::from(radix))
                .saturating_add(digit);
            if value > INT_LIMIT {
                break;
            }
        }
        match u64::try_from(value) {
            Ok(value) if u128::from(value) <= INT_LIMIT => {
                self.push(TokenKind::Int(value), start, end)
            }
            _ => {
                self.error(
                    Diagnostic::new(codes::E0106, "integer literal too large")
                        .primary(span, "this value does not fit into `Int`")
                        .note("`Int` is a signed 64-bit integer; the largest value is 9223372036854775807"),
                );
                self.push(TokenKind::Error, start, end);
            }
        }
    }

    // ----- strings -------------------------------------------------------------------------

    fn string(&mut self, start: u32) {
        self.cursor.bump(); // opening quote
        let mut value = String::new();
        loop {
            let at = self.cursor.offset();
            match self.cursor.peek(0) {
                None | Some('\n') => {
                    self.unterminated(start, at);
                    break;
                }
                Some('\r') if self.cursor.peek(1) == Some('\n') => {
                    self.unterminated(start, at);
                    break;
                }
                Some('"') => {
                    self.cursor.bump();
                    break;
                }
                Some('\\') => {
                    self.cursor.bump();
                    if let Some(c) = self.escape(at) {
                        value.push(c);
                    }
                }
                Some('\r') => {
                    self.cursor.bump();
                    self.lone_cr(at);
                }
                Some(c) => {
                    self.cursor.bump();
                    if (c.is_control() && c != '\t') || c == '\u{7f}' {
                        let span = self.span(at, self.cursor.offset());
                        self.error(
                            Diagnostic::new(
                                codes::E0109,
                                format!(
                                    "unescaped control character U+{:04X} in a string",
                                    c as u32
                                ),
                            )
                            .primary(span, "control characters must be escaped")
                            .help(format!("write it as `\\u{{{:x}}}`", c as u32)),
                        );
                    } else {
                        value.push(c);
                    }
                }
            }
        }
        let end = self.cursor.offset();
        self.push(TokenKind::Str(value), start, end);
    }

    fn unterminated(&mut self, start: u32, at: u32) {
        let span = self.span(start, at);
        self.error(
            Diagnostic::new(codes::E0103, "unterminated string literal")
                .primary(span, "this string is not closed before the end of the line")
                .help("add the closing `\"`; strings cannot span several lines"),
        );
    }

    /// Reads an escape after `\`. `at` is the offset of the backslash.
    fn escape(&mut self, at: u32) -> Option<char> {
        let c = match self.cursor.peek(0) {
            // The line ends right after `\`: the string is unterminated, reported by the caller.
            None | Some('\n') | Some('\r') => return None,
            Some(c) => c,
        };
        self.cursor.bump();
        match c {
            'n' => Some('\n'),
            'r' => Some('\r'),
            't' => Some('\t'),
            '0' => Some('\0'),
            '\\' => Some('\\'),
            '"' => Some('"'),
            'x' => {
                let mut digits = String::new();
                for _ in 0..2 {
                    match self.cursor.peek(0) {
                        Some(d) if d.is_ascii_hexdigit() => {
                            digits.push(d);
                            self.cursor.bump();
                        }
                        _ => break,
                    }
                }
                let value = u32::from_str_radix(&digits, 16).ok();
                match value {
                    Some(v) if digits.len() == 2 && v <= 0x7f => char::from_u32(v),
                    _ => {
                        self.bad_escape(at, "`\\x` needs exactly two hex digits, at most `7F`");
                        None
                    }
                }
            }
            'u' => self.unicode_escape(at),
            other => {
                let span = self.span(at, self.cursor.offset());
                self.error(
                    Diagnostic::new(codes::E0104, format!("unknown escape sequence `\\{other}`"))
                        .primary(span, "not a valid escape")
                        .help("valid escapes are `\\n`, `\\r`, `\\t`, `\\0`, `\\\\`, `\\\"`, `\\xHH` and `\\u{...}`"),
                );
                None
            }
        }
    }

    fn unicode_escape(&mut self, at: u32) -> Option<char> {
        if !self.cursor.eat('{') {
            self.bad_escape(at, "write a Unicode escape as `\\u{1F600}`");
            return None;
        }
        let mut digits = String::new();
        while let Some(d) = self.cursor.peek(0) {
            if d.is_ascii_hexdigit() {
                digits.push(d);
                self.cursor.bump();
            } else {
                break;
            }
        }
        if !self.cursor.eat('}') {
            self.bad_escape(at, "a Unicode escape must end with `}`");
            return None;
        }
        if digits.is_empty() || digits.len() > 6 {
            self.bad_escape(at, "a Unicode escape needs 1 to 6 hex digits");
            return None;
        }
        let value = u32::from_str_radix(&digits, 16).ok();
        match value.and_then(char::from_u32) {
            Some(c) => Some(c),
            None => {
                self.bad_escape(
                    at,
                    "not a Unicode scalar value (surrogate or above U+10FFFF)",
                );
                None
            }
        }
    }

    fn bad_escape(&mut self, at: u32, label: &str) {
        let end = self.cursor.offset();
        let span = self.span(at, end);
        let text = self.source(at, end);
        self.error(
            Diagnostic::new(codes::E0105, format!("invalid character escape `{text}`"))
                .primary(span, label),
        );
    }

    // ----- punctuation ---------------------------------------------------------------------

    fn punctuation(&mut self, start: u32, c: char) {
        use TokenKind::*;
        self.cursor.bump();
        let kind = match c {
            '(' => LParen,
            ')' => RParen,
            '[' => LBracket,
            ']' => RBracket,
            '{' => LBrace,
            '}' => RBrace,
            ',' => Comma,
            ':' => Colon,
            '?' => Question,
            '~' => Tilde,
            '.' => {
                if self.cursor.eat('.') {
                    if self.cursor.eat('=') {
                        DotDotEq
                    } else {
                        DotDot
                    }
                } else if self.cursor.peek(0).is_some_and(|d| d.is_ascii_digit()) {
                    // `.5`: a float needs a digit before the dot (rule 5.2.1).
                    while self.cursor.peek(0).is_some_and(is_ident_continue) {
                        self.cursor.bump();
                    }
                    let end = self.cursor.offset();
                    let text = self.source(start, end);
                    let span = self.span(start, end);
                    self.error(
                        Diagnostic::new(codes::E0107, format!("malformed number literal `{text}`"))
                            .primary(span, "a float needs a digit before the dot")
                            .help(format!("write `0{text}`")),
                    );
                    Error
                } else {
                    Dot
                }
            }
            '=' => self.pick(&[('=', EqEq), ('>', FatArrow)], Eq),
            '-' => self.pick(&[('>', Arrow), ('=', MinusEq)], Minus),
            '+' => self.pick(&[('=', PlusEq)], Plus),
            '*' => self.pick(&[('=', StarEq)], Star),
            '/' => self.pick(&[('=', SlashEq)], Slash),
            '%' => self.pick(&[('=', PercentEq)], Percent),
            '^' => self.pick(&[('=', CaretEq)], Caret),
            '!' => self.pick(&[('=', NotEq)], Bang),
            '&' => self.pick(&[('&', AndAnd), ('=', AmpEq)], Amp),
            '|' => self.pick(&[('|', OrOr), ('=', PipeEq)], Pipe),
            '<' => {
                if self.cursor.eat('<') {
                    self.pick(&[('=', ShlEq)], Shl)
                } else {
                    self.pick(&[('=', Le)], Lt)
                }
            }
            '>' => {
                if self.cursor.eat('>') {
                    self.pick(&[('=', ShrEq)], Shr)
                } else {
                    self.pick(&[('=', Ge)], Gt)
                }
            }
            '\'' => {
                // `'x'` is one mistake, not two: consume up to the closing quote on this line.
                let mut ahead = 0;
                while let Some(next) = self.cursor.peek(ahead) {
                    if next == '\n' || next == '\r' {
                        break;
                    }
                    if next == '\'' {
                        for _ in 0..=ahead {
                            self.cursor.bump();
                        }
                        break;
                    }
                    ahead += 1;
                }
                self.unexpected(start, '\'');
                Error
            }
            other => {
                self.unexpected(start, other);
                Error
            }
        };
        let end = self.cursor.offset();
        self.push(kind, start, end);
    }

    /// Consumes one of the given follow characters and returns its kind, or returns `single`.
    fn pick(&mut self, options: &[(char, TokenKind)], single: TokenKind) -> TokenKind {
        for (next, kind) in options {
            if self.cursor.eat(*next) {
                return kind.clone();
            }
        }
        single
    }

    fn unexpected(&mut self, start: u32, c: char) {
        let end = self.cursor.offset();
        let span = self.span(start, end);
        let shown = if c.is_ascii_graphic() {
            format!("`{c}`")
        } else {
            format!("U+{:04X}", c as u32)
        };
        let mut diagnostic = Diagnostic::new(codes::E0102, format!("unexpected character {shown}"))
            .primary(span, "this character is not valid here");
        diagnostic = match c {
            ';' => diagnostic.help("there is no `;`: put each statement on its own line"),
            '\'' => diagnostic.help("strings use double quotes: `\"text\"`"),
            c if c.is_whitespace() => {
                diagnostic.help("only spaces, tabs and line ends count as whitespace")
            }
            _ => diagnostic,
        };
        self.error(diagnostic);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LineKind {
    Plain,
    Outer,
    Inner,
}

/// Starts an identifier. Non-ASCII letters are accepted here only to report E0111 for the whole
/// word instead of one error per character.
fn is_ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_' || (!c.is_ascii() && c.is_alphabetic())
}

fn is_ident_continue(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || (!c.is_ascii() && c.is_alphanumeric())
}
